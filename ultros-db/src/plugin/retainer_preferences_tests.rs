use super::tests::pair;
use super::*;
use crate::UltrosDb;
use crate::common::test_db::disposable_plugin_database;
use migration::{Migrator, MigratorTrait};
use sea_orm::{ConnectionTrait, Database};
use ultros_api_types::plugin::*;

#[tokio::test]
#[ignore = "requires ULTROS_PLUGIN_TEST_DATABASE_URL pointing at a disposable loopback database"]
async fn retainer_preferences_postgres() {
    let url =
        std::env::var("ULTROS_PLUGIN_TEST_DATABASE_URL").expect("disposable database URL required");
    assert!(disposable_plugin_database(&url));
    let connection = Database::connect(url).await.unwrap();
    Migrator::up(&connection, None).await.unwrap();
    let db = UltrosDb::from_connection(connection);
    let keys = PluginKeys::new(&[11; 32], &[29; 32], "http://localhost:8080").unwrap();
    let owner = 900_000_000_000_000_131_i64;
    let suffix = crypto::random(8).unwrap();
    db.get_or_create_discord_user(owner as u64, "Order fixture".into())
        .await
        .unwrap();
    db.insert_default_retainer_cities().await.unwrap();
    db.db.execute_unprepared("INSERT INTO region(id,name) VALUES(30100,'RetainerPrefsRegion') ON CONFLICT DO NOTHING; INSERT INTO datacenter(id,name,region_id) VALUES(30100,'RetainerPrefsDC',30100) ON CONFLICT DO NOTHING; INSERT INTO world(id,name,datacenter_id) VALUES(30100,'RetainerPrefsWorld',30100) ON CONFLICT DO NOTHING").await.unwrap();
    let tokens = pair(
        &db,
        &keys,
        owner,
        SCOPES.iter().map(|(name, _)| name.to_string()).collect(),
        &format!("order/{suffix}"),
    )
    .await;
    let principal = db.authenticate_plugin(&tokens.access_token).await.unwrap();
    let observation = ObserveCharacter {
        content_id: "717171".into(),
        name: format!("Order {suffix}"),
        home_world: 30100,
        retainers: vec![
            ObservedRetainer {
                content_id: "717172".into(),
                name: format!("A{suffix}"),
                city: Some(1),
                game_order: Some(2),
                available: Some(true),
            },
            ObservedRetainer {
                content_id: "717173".into(),
                name: format!("B{suffix}"),
                city: Some(1),
                game_order: Some(0),
                available: Some(true),
            },
            ObservedRetainer {
                content_id: "717174".into(),
                name: format!("C{suffix}"),
                city: Some(1),
                game_order: Some(1),
                available: Some(false),
            },
        ],
    };
    let registered = db
        .register_plugin_retainers(
            &principal,
            &keys,
            &RegisterRetainers {
                operation_id: format!("register-{suffix}"),
                character: observation.clone(),
            },
        )
        .await
        .unwrap();
    let character = registered
        .iter()
        .find(|c| c.name == observation.name)
        .unwrap()
        .clone();
    let retainer = |name: &str| {
        character
            .retainers
            .iter()
            .find(|r| r.name == format!("{name}{suffix}"))
            .unwrap()
            .clone()
    };
    let (a, b, c) = (retainer("A"), retainer("B"), retainer("C"));
    let second = ObserveCharacter {
        content_id: "717175".into(),
        name: format!("Other {suffix}"),
        home_world: 30100,
        retainers: vec![ObservedRetainer {
            content_id: "717176".into(),
            name: format!("D{suffix}"),
            city: Some(1),
            game_order: Some(0),
            available: Some(true),
        }],
    };
    let registered = db
        .register_plugin_retainers(
            &principal,
            &keys,
            &RegisterRetainers {
                operation_id: format!("other-{suffix}"),
                character: second.clone(),
            },
        )
        .await
        .unwrap();
    let other = registered
        .iter()
        .find(|c| c.name == second.name)
        .unwrap()
        .retainers[0]
        .clone();
    for (retainer, weight) in [(&a, 10_i32), (&other, 15), (&c, 20), (&b, 30)] {
        exec(
            &db.db,
            "UPDATE owned_retainers SET weight=$1 WHERE discord_id=$2 AND retainer_id=$3",
            vec![
                weight.into(),
                owner.into(),
                retainer.public_retainer_id.unwrap().into(),
            ],
        )
        .await
        .unwrap();
    }
    let roster = db.plugin_characters(&principal).await.unwrap();
    assert_eq!(
        roster
            .iter()
            .find(|r| r.id == character.id)
            .unwrap()
            .retainers
            .iter()
            .map(|r| r.id.clone())
            .collect::<Vec<_>>(),
        vec![a.id.clone(), c.id.clone(), b.id.clone()],
        "the game API must follow website weights, not names"
    );
    let request = RetainerGameOrder {
        operation_id: format!("order-{suffix}"),
        retainer_ids: vec![b.id.clone(), c.id.clone(), a.id.clone()],
    };
    let ordered = db
        .set_plugin_retainer_order(&principal, character.id.parse().unwrap(), &request)
        .await
        .unwrap();
    let current = ordered.iter().find(|r| r.id == character.id).unwrap();
    assert_eq!(
        current
            .retainers
            .iter()
            .map(|r| r.id.clone())
            .collect::<Vec<_>>(),
        request.retainer_ids
    );
    db.set_plugin_retainer_order(&principal, character.id.parse().unwrap(), &request)
        .await
        .unwrap();
    let all = query(
        &db.db,
        "SELECT retainer_id FROM owned_retainers WHERE discord_id=$1 ORDER BY weight,id",
        vec![owner.into()],
    )
    .await
    .unwrap();
    assert_eq!(
        all.iter()
            .map(|r| r.try_get::<i32>("", "retainer_id").unwrap())
            .collect::<Vec<_>>(),
        vec![
            b.public_retainer_id.unwrap(),
            other.public_retainer_id.unwrap(),
            c.public_retainer_id.unwrap(),
            a.public_retainer_id.unwrap()
        ],
        "reordering one character moved another character's relative position"
    );
    let wrong = RetainerGameOrder {
        operation_id: format!("wrong-{suffix}"),
        retainer_ids: vec![other.id.clone()],
    };
    assert!(
        db.set_plugin_retainer_order(&principal, character.id.parse().unwrap(), &wrong)
            .await
            .is_err(),
        "another character's retainers were reordered"
    );
    let duplicate = RetainerGameOrder {
        operation_id: format!("duplicate-{suffix}"),
        retainer_ids: vec![a.id.clone(), a.id.clone()],
    };
    assert!(
        db.set_plugin_retainer_order(&principal, character.id.parse().unwrap(), &duplicate)
            .await
            .is_err()
    );
    let mut capture = ListingCapture {
        capture_id: format!("capture-{suffix}"),
        base_revision: 0,
        capture_epoch: character.capture_epoch,
        observed_at: chrono::Utc::now(),
        complete: true,
        listings: vec![PrivateListing {
            slot: 0,
            item_id: 8,
            hq: false,
            quantity: 1,
            unit_price: 70,
        }],
    };
    db.upload_plugin_listings(&principal, a.id.parse().unwrap(), &capture)
        .await
        .unwrap();
    let disabled = db
        .set_plugin_retainer_enabled(&principal, a.id.parse().unwrap(), false)
        .await
        .unwrap();
    let disabled = disabled.iter().find(|r| r.id == character.id).unwrap();
    assert!(
        disabled
            .retainers
            .iter()
            .find(|r| r.id == a.id)
            .unwrap()
            .disabled
    );
    let epoch = disabled.capture_epoch;
    let repeated = db
        .set_plugin_retainer_enabled(&principal, a.id.parse().unwrap(), false)
        .await
        .unwrap();
    assert_eq!(
        repeated
            .iter()
            .find(|r| r.id == character.id)
            .unwrap()
            .capture_epoch,
        epoch,
        "an idempotent disable invalidated unrelated captures again"
    );
    assert!(
        db.plugin_retainer_snapshots(&principal)
            .await
            .unwrap()
            .iter()
            .any(|s| s.retainer_id == a.id),
        "disabling deleted the saved snapshot"
    );
    assert!(
        !db.website_retainer_listings(owner)
            .await
            .unwrap()
            .retainers
            .iter()
            .flat_map(|(_, r)| r)
            .any(|(r, _)| Some(r.id) == a.public_retainer_id),
        "the website still displayed a disabled retainer"
    );
    assert!(
        !db.get_retainer_listings_for_discord_user(owner as u64)
            .await
            .unwrap()
            .iter()
            .any(|(_, r, _)| Some(r.id) == a.public_retainer_id),
        "undercut lookups included a disabled retainer"
    );
    capture.capture_id = format!("disabled-capture-{suffix}");
    capture.base_revision = 1;
    assert!(matches!(
        db.upload_plugin_listings(&principal, a.id.parse().unwrap(), &capture)
            .await,
        Err(PluginError::Conflict("retainer_disabled"))
    ));
    let mut legacy = observation.clone();
    for r in &mut legacy.retainers {
        r.game_order = None;
        r.available = None;
    }
    let refreshed = db
        .register_plugin_retainers(
            &principal,
            &keys,
            &RegisterRetainers {
                operation_id: format!("legacy-{suffix}"),
                character: legacy,
            },
        )
        .await
        .unwrap();
    let refreshed = refreshed.iter().find(|r| r.id == character.id).unwrap();
    assert!(
        refreshed
            .retainers
            .iter()
            .find(|r| r.id == a.id)
            .unwrap()
            .disabled,
        "refreshing from an older client re-enabled a retired retainer"
    );
    assert_eq!(
        refreshed
            .retainers
            .iter()
            .find(|r| r.id == c.id)
            .unwrap()
            .available,
        Some(false)
    );
    assert_eq!(
        refreshed
            .retainers
            .iter()
            .map(|r| r.id.clone())
            .collect::<Vec<_>>(),
        request.retainer_ids,
        "registration replaced the saved game order"
    );
    let enabled = db
        .set_plugin_retainer_enabled(&principal, a.id.parse().unwrap(), true)
        .await
        .unwrap();
    let enabled = enabled.iter().find(|r| r.id == character.id).unwrap();
    assert!(enabled.capture_epoch > epoch);
    assert!(
        matches!(
            db.upload_plugin_listings(&principal, a.id.parse().unwrap(), &capture)
                .await,
            Err(PluginError::Conflict("upload_consent_changed"))
        ),
        "an old queued capture survived disable/re-enable"
    );
    assert!(
        db.website_retainer_listings(owner)
            .await
            .unwrap()
            .retainers
            .iter()
            .flat_map(|(_, r)| r)
            .any(|(r, rows)| Some(r.id) == a.public_retainer_id && !rows.is_empty()),
        "re-enabling did not restore saved listings"
    );
    let read = pair(
        &db,
        &keys,
        owner,
        vec!["retainers:read".into()],
        &format!("order-read/{suffix}"),
    )
    .await;
    let read = db.authenticate_plugin(&read.access_token).await.unwrap();
    assert!(matches!(
        db.set_plugin_retainer_enabled(&read, a.id.parse().unwrap(), false)
            .await,
        Err(PluginError::Forbidden)
    ));
    assert!(matches!(
        db.set_plugin_retainer_order(&read, character.id.parse().unwrap(), &request)
            .await,
        Err(PluginError::Forbidden)
    ));
}
