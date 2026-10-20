//! Run only against an explicitly supplied, disposable loopback database.
use super::*;
use crate::{NewAlertEvent, UltrosDb};
use migration::{Migrator, MigratorTrait};
use sea_orm::{ConnectOptions, ConnectionTrait, Database};
use serde_json::json;
use ultros_api_types::{list::ListPermission, plugin::*};
use ultros_list_doc::{ListDocument, RowKey};

fn disposable_plugin_database(url: &str) -> bool {
    // Match a deliberately narrow URL grammar: no password, alternate host,
    // escaped names, query options or fragment can redirect a destructive fixture.
    let Some(address) = url.strip_prefix("postgres://ultros_plugin@127.0.0.1:") else {
        return false;
    };
    let Some((port, database)) = address.split_once('/') else {
        return false;
    };
    let Some(name) = database.strip_prefix("ultros_plugin_") else {
        return false;
    };
    !port.is_empty()
        && port.bytes().all(|c| c.is_ascii_digit())
        && port.parse::<u16>().is_ok_and(|port| port > 0)
        && !name.is_empty()
        && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

#[test]
fn disposable_plugin_database_guard_is_strict() {
    assert!(disposable_plugin_database(
        "postgres://ultros_plugin@127.0.0.1:55437/ultros_plugin_review_20261020"
    ));
    for url in [
        "postgres://ultros_plugin@127.0.0.1:0/ultros_plugin_test",
        "postgres://ultros_plugin@127.0.0.1:65536/ultros_plugin_test",
        "postgres://ultros_plugin@localhost:55437/ultros_plugin_test",
        "postgres://ultros_plugin:secret@127.0.0.1:55437/ultros_plugin_test",
        "postgres://ultros_plugin@127.0.0.1:55437/ultros_plugin_test?host=elsewhere",
        "postgres://ultros_plugin@127.0.0.1:55437/ultros_plugin_test#fragment",
        "postgres://ultros_plugin@127.0.0.1:55437/production",
        "postgres://ultros_plugin@127.0.0.1:55437/ultros_plugin_",
    ] {
        assert!(!disposable_plugin_database(url));
    }
}

fn market_listing(name: &str, city: u32, price: u32, hq: bool) -> universalis::ListingView {
    universalis::ListingView {
        // Seller review age is intentionally old, and must not be reused as
        // the time Ultros reconciled a complete board.
        last_review_time: (chrono::Utc::now() - chrono::Duration::days(7)).into(),
        price_per_unit: Some(price),
        quantity: Some(1),
        stain_id: None,
        world_name: None,
        world_id: None,
        creator_name: None,
        creator_id: None,
        hq,
        is_crafted: false,
        listing_id: None,
        materia: vec![],
        on_mannequin: false,
        retainer_city: city,
        retainer_id: None,
        retainer_name: name.into(),
        seller_id: None,
        total: price,
        tax: 0,
    }
}

async fn pair(
    db: &UltrosDb,
    keys: &PluginKeys,
    owner: i64,
    requested: Vec<String>,
    ip: &str,
) -> TokenPair {
    let (device, code, _link) = db
        .create_plugin_pairing(
            keys,
            &CreatePairing {
                label: "Local integration client".into(),
                scopes: requested.clone(),
            },
            ip,
            None,
        )
        .await
        .unwrap();
    let (cookie, csrf) = db.create_plugin_approval(owner).await.unwrap();
    let selection = db
        .select_plugin_pairing(keys, owner, &cookie, &csrf, &code, ip)
        .await
        .unwrap();
    assert!(
        db.approve_plugin_pairing(
            owner + 1,
            &cookie,
            &selection.csrf,
            &selection.nonce,
            &requested,
            false
        )
        .await
        .is_err()
    );
    db.approve_plugin_pairing(
        owner,
        &cookie,
        &selection.csrf,
        &selection.nonce,
        &requested,
        false,
    )
    .await
    .unwrap();
    db.poll_plugin_pairing(&device).await.unwrap()
}

fn fire(alert: i32, title: &str) -> NewAlertEvent {
    NewAlertEvent {
        alert_id: alert,
        item_id: 1,
        matched_listing_id: None,
        matched_price: Some(100),
        delivered: false,
        delivery_error: None,
        title: title.into(),
        body: "Fixture body".into(),
        click_url: "/item/1".into(),
    }
}

#[tokio::test]
#[ignore = "requires ULTROS_PLUGIN_TEST_DATABASE_URL pointing at a disposable loopback database"]
async fn plugin_pairing_links_postgres() {
    let url = std::env::var("ULTROS_PLUGIN_TEST_DATABASE_URL")
        .expect("explicit disposable PostgreSQL URL required");
    assert!(
        disposable_plugin_database(&url),
        "refusing a non-test database"
    );
    let mut options = ConnectOptions::new(url);
    options
        .max_connections(5)
        .min_connections(1)
        .sqlx_logging(false);
    let connection = Database::connect(options).await.unwrap();
    Migrator::up(&connection, None).await.unwrap();
    let db = UltrosDb::from_connection(connection);
    let keys = PluginKeys::new(&[11; 32], &[29; 32], "http://localhost:8080").unwrap();
    let wrong_origin = PluginKeys::new(&[11; 32], &[29; 32], "https://ultros.app").unwrap();
    let owner = 900_000_000_000_000_115_i64;
    db.get_or_create_discord_user(owner as u64, "Direct link fixture".into())
        .await
        .unwrap();
    let ip = format!("link-fixture/{}", crypto::random(8).unwrap());
    let request = CreatePairing {
        label: "Direct link fixture".into(),
        scopes: vec!["lists:read".into()],
    };
    let (device, code, link) = db
        .create_plugin_pairing(&keys, &request, &ip, None)
        .await
        .unwrap();
    assert!(
        device != link,
        "browser and polling credentials must be independent"
    );
    db.validate_plugin_pairing_link(&keys, &link, &ip)
        .await
        .unwrap();
    assert!(
        db.validate_plugin_pairing_link(&wrong_origin, &link, &ip)
            .await
            .is_err()
    );
    assert!(
        db.validate_plugin_pairing_link(&keys, &device, &ip)
            .await
            .is_err()
    );
    assert!(db.authenticate_plugin(&link).await.is_err());
    assert!(db.poll_plugin_pairing(&link).await.is_err());
    let (cookie, csrf) = db.create_plugin_approval(owner).await.unwrap();
    assert!(
        db.select_plugin_pairing_link(&keys, owner, &cookie, "wrong-csrf", &link, &ip)
            .await
            .is_err()
    );
    let selected = db
        .select_plugin_pairing_link(&keys, owner, &cookie, &csrf, &link, &ip)
        .await
        .unwrap();
    assert_eq!(selected.code, normalize_human_code(&code).unwrap());
    assert!(
        matches!(
            db.poll_plugin_pairing(&device).await,
            Err(PluginError::Pending)
        ),
        "opening a link must not approve the pairing"
    );
    db.approve_plugin_pairing(
        owner,
        &cookie,
        &selected.csrf,
        &selected.nonce,
        &request.scopes,
        false,
    )
    .await
    .unwrap();
    assert!(
        db.validate_plugin_pairing_link(&keys, &link, &ip)
            .await
            .is_err()
    );
    exec(
        &db.db,
        "UPDATE plugin_pairing SET next_poll_at=now() WHERE device_hash=$1",
        vec![crypto::digest(device.as_bytes()).into()],
    )
    .await
    .unwrap();
    let tokens = db.poll_plugin_pairing(&device).await.unwrap();
    assert!(
        db.select_plugin_pairing_link(&keys, owner, &cookie, &selected.csrf, &link, &ip)
            .await
            .is_err()
    );
    db.revoke_plugin_credential(&tokens.refresh_token)
        .await
        .unwrap();
    let (expired_device, _, expired_link) = db
        .create_plugin_pairing(&keys, &request, &ip, None)
        .await
        .unwrap();
    exec(
        &db.db,
        "UPDATE plugin_pairing SET expires_at=now()-interval '1 second' WHERE device_hash=$1",
        vec![crypto::digest(expired_device.as_bytes()).into()],
    )
    .await
    .unwrap();
    assert!(
        db.validate_plugin_pairing_link(&keys, &expired_link, &ip)
            .await
            .is_err()
    );
}

#[tokio::test]
#[ignore = "requires ULTROS_PLUGIN_TEST_DATABASE_URL pointing at a disposable loopback database"]
async fn plugin_connected_flows_postgres() {
    let url = std::env::var("ULTROS_PLUGIN_TEST_DATABASE_URL")
        .expect("explicit disposable PostgreSQL URL required");
    assert!(
        disposable_plugin_database(&url),
        "refusing a non-test database"
    );
    let mut options = ConnectOptions::new(url);
    options
        .max_connections(10)
        .min_connections(1)
        .sqlx_logging(false);
    let connection = Database::connect(options).await.unwrap();
    Migrator::up(&connection, None).await.unwrap();
    let db = UltrosDb::from_connection(connection);
    let keys = PluginKeys::new(&[11; 32], &[29; 32], "http://localhost:8080").unwrap();
    let owner = 900_000_000_000_000_101_i64;
    // The app seeds these after migrations; this fixture starts from a bare DB.
    db.insert_default_retainer_cities().await.unwrap();
    let other = owner + 1;
    // Only these fixture accounts are reset. The URL assertion excludes configured app databases.
    exec(
        &db.db,
        "DELETE FROM owned_retainers WHERE discord_id IN ($1,$2)",
        vec![owner.into(), other.into()],
    )
    .await
    .unwrap();
    exec(
        &db.db,
        "DELETE FROM discord_user WHERE id IN ($1,$2)",
        vec![owner.into(), other.into()],
    )
    .await
    .unwrap();
    db.get_or_create_discord_user(owner as u64, "Plugin fixture owner".into())
        .await
        .unwrap();
    db.get_or_create_discord_user(other as u64, "Other fixture owner".into())
        .await
        .unwrap();
    db.db.execute_unprepared("INSERT INTO region(id,name) VALUES(30000,'PluginTestRegion') ON CONFLICT DO NOTHING; INSERT INTO datacenter(id,name,region_id) VALUES(30000,'PluginTestDC',30000) ON CONFLICT DO NOTHING; INSERT INTO world(id,name,datacenter_id) VALUES(30000,'PluginTestWorld',30000) ON CONFLICT DO NOTHING").await.unwrap();
    let suffix = crypto::random(8).unwrap();
    let all = SCOPES
        .iter()
        .map(|(id, _)| id.to_string())
        .collect::<Vec<_>>();
    let tokens = pair(
        &db,
        &keys,
        owner,
        all.clone(),
        &format!("fixture/{suffix}/one"),
    )
    .await;
    assert!(db.authenticate_plugin(&tokens.refresh_token).await.is_err());
    let principal = db.authenticate_plugin(&tokens.access_token).await.unwrap();
    let session = db.plugin_session(&principal).await.unwrap();
    assert!(session.endpoint_id.is_some());
    let read_tokens = pair(
        &db,
        &keys,
        other,
        vec!["lists:read".into()],
        &format!("fixture/{suffix}/two"),
    )
    .await;
    let read_only = db
        .authenticate_plugin(&read_tokens.access_token)
        .await
        .unwrap();
    assert!(matches!(
        db.plugin_characters(&read_only).await,
        Err(PluginError::Forbidden)
    ));

    let observation = ObserveCharacter {
        content_id: "111111".into(),
        name: "Fixture Character".into(),
        home_world: 30000,
        retainers: vec![ObservedRetainer {
            content_id: "222222".into(),
            name: "FixtureRetainer".into(),
            city: Some(1),
        }],
    };
    let observed = db
        .observe_plugin_character(&principal, &keys, &observation)
        .await
        .unwrap();
    assert!(!observed[0].registered && !observed[0].verified);
    assert!(
        db.registered_game_characters(owner)
            .await
            .unwrap()
            .is_empty(),
        "observing alone must not register retainers on the website"
    );
    let registration = RegisterRetainers {
        operation_id: format!("register-{suffix}"),
        character: observation.clone(),
    };
    let registered = db
        .register_plugin_retainers(&principal, &keys, &registration)
        .await
        .unwrap();
    let again = db
        .register_plugin_retainers(&principal, &keys, &registration)
        .await
        .unwrap();
    assert_eq!(registered[0].id, again[0].id);
    assert_eq!(registered[0].retainers.len(), 1);
    let website = db.registered_game_characters(owner).await.unwrap();
    assert_eq!(website[0].id, registered[0].id);
    assert_eq!(website[0].retainers[0].name, "FixtureRetainer");
    let public_retainer = website[0].retainers[0]
        .public_retainer_id
        .expect("registration creates a real retainer identity");
    let normal = db.website_retainers(owner).await.unwrap();
    assert!(matches!(
        &normal.retainers[0].0,
        Some(ultros_api_types::user::RetainerCharacter::Unverified { .. })
    ));
    assert!(
        normal
            .retainers
            .iter()
            .flat_map(|(_, r)| r)
            .any(|(_, r)| r.id == public_retainer)
    );
    assert!(
        db.registered_game_characters(other)
            .await
            .unwrap()
            .is_empty(),
        "saved game rosters must be isolated by website account"
    );
    let changed = RegisterRetainers {
        operation_id: registration.operation_id.clone(),
        character: ObserveCharacter {
            name: "Changed payload".into(),
            ..observation.clone()
        },
    };
    assert!(matches!(
        db.register_plugin_retainers(&principal, &keys, &changed)
            .await,
        Err(PluginError::Conflict("operation_id_reused"))
    ));
    let character = registered[0].id.parse::<i64>().unwrap();
    let retainer = registered[0].retainers[0].id.parse::<i64>().unwrap();
    let mut capture = ListingCapture {
        capture_id: format!("capture-one-{suffix}"),
        base_revision: 0,
        capture_epoch: 0,
        observed_at: chrono::Utc::now(),
        complete: true,
        listings: vec![PrivateListing {
            slot: 0,
            item_id: 1,
            hq: false,
            quantity: 5,
            unit_price: 100,
        }],
    };
    assert!(
        db.upload_plugin_listings(&principal, retainer, &capture)
            .await
            .is_err()
    );
    let consent = db
        .set_plugin_upload_consent(
            &principal,
            character,
            &UploadConsent {
                enabled: true,
                delete_snapshots: false,
            },
        )
        .await
        .unwrap();
    capture.capture_epoch = consent[0].capture_epoch;
    let receipt = db
        .upload_plugin_listings(&principal, retainer, &capture)
        .await
        .unwrap();
    assert_eq!(receipt.revision, 1);
    let private = db.website_retainer_listings(owner).await.unwrap();
    let private_rows = &private.retainers[0].1[0].1;
    assert_eq!(private_rows.len(), 1);
    assert!(matches!(
        private_rows[0].id,
        ultros_api_types::user::RetainerListingId::InGame { slot: 0, .. }
    ));
    assert!(
        db.get_retainer_listings(public_retainer)
            .await
            .unwrap()
            .1
            .is_empty(),
        "an upload must not enter the public listing table"
    );
    assert!(
        db.website_retainer_listings(other)
            .await
            .unwrap()
            .retainers
            .is_empty(),
        "another account cannot see private listings"
    );
    let listing = &capture.listings[0];
    // Price assistance reads private captures without promoting them to public
    // listings, and excludes all this account's retainers (not just this one).
    assert!(matches!(
        db.plugin_retainer_prices(&read_only, retainer).await,
        Err(PluginError::Forbidden)
    ));
    let other_prices = pair(
        &db,
        &keys,
        other,
        vec!["retainers:read".into()],
        &format!("fixture/{suffix}/prices"),
    )
    .await;
    let other_prices = db
        .authenticate_plugin(&other_prices.access_token)
        .await
        .unwrap();
    assert!(matches!(
        db.plugin_retainer_prices(&other_prices, retainer).await,
        Err(PluginError::Conflict("retainer_registration_required"))
    ));
    let own_comparison = one(
        &db.db,
        "INSERT INTO retainer(name,world_id,retainer_city_id) VALUES($1,30000,1) RETURNING id",
        vec![format!("OwnComparison{suffix}").into()],
    )
    .await
    .unwrap()
    .try_get::<i32>("", "id")
    .unwrap();
    let competitor = one(
        &db.db,
        "INSERT INTO retainer(name,world_id,retainer_city_id) VALUES($1,30000,1) RETURNING id",
        vec![format!("OtherComparison{suffix}").into()],
    )
    .await
    .unwrap()
    .try_get::<i32>("", "id")
    .unwrap();
    exec(
        &db.db,
        "INSERT INTO owned_retainers(discord_id,retainer_id) VALUES($1,$2)",
        vec![owner.into(), own_comparison.into()],
    )
    .await
    .unwrap();
    exec(&db.db,
        "INSERT INTO active_listing(world_id,retainer_id,item_id,hq,quantity,price_per_unit,timestamp)
         VALUES(30000,$1,1,false,1,100,now()),(30000,$2,1,false,1,1,now()),
               (30000,$3,1,false,1,250,now()),(30000,$3,1,true,1,5,now())",
        vec![public_retainer.into(),own_comparison.into(),competitor.into()]).await.unwrap();
    exec(
        &db.db,
        "INSERT INTO listing_last_updated(world_id,item_id,date_time) VALUES(30000,1,now())
         ON CONFLICT(world_id,item_id) DO UPDATE SET date_time=excluded.date_time",
        vec![],
    )
    .await
    .unwrap();
    exec(
        &db.db,
        "DELETE FROM listing_board_review WHERE world_id=30000 AND item_id=1",
        vec![],
    )
    .await
    .unwrap();
    db.update_sales(vec![], universalis::ItemId(1), universalis::WorldId(30000))
        .await
        .unwrap();
    let unknown = db
        .plugin_retainer_prices(&principal, retainer)
        .await
        .unwrap();
    assert_eq!(unknown.listings[0].status, "unknown_age");
    assert_eq!(unknown.listings[0].suggested_price, None);
    let board = vec![
        market_listing("FixtureRetainer", 1, 100, false),
        market_listing(&format!("OwnComparison{suffix}"), 1, 1, false),
        market_listing(&format!("OtherComparison{suffix}"), 1, 250, false),
        market_listing(&format!("OtherComparison{suffix}"), 1, 5, true),
    ];
    db.add_listings(
        vec![board[0].clone()],
        universalis::ItemId(1),
        universalis::WorldId(30000),
    )
    .await
    .unwrap();
    assert_eq!(
        db.plugin_retainer_prices(&principal, retainer)
            .await
            .unwrap()
            .listings[0]
            .status,
        "unknown_age"
    );
    db.update_listings(
        board.clone(),
        universalis::ItemId(1),
        universalis::WorldId(30000),
    )
    .await
    .unwrap();
    let prices = db
        .plugin_retainer_prices(&principal, retainer)
        .await
        .unwrap();
    assert_eq!(prices.source, "in_game");
    assert_eq!(prices.world_name, "PluginTestWorld");
    assert_eq!(
        prices.observed_at.unwrap().timestamp_micros(),
        capture.observed_at.timestamp_micros()
    );
    assert_eq!(prices.listings.len(), 1);
    assert_eq!(prices.listings[0].current_price, 100);
    assert_eq!(prices.listings[0].competitor_price, Some(250));
    assert_eq!(prices.listings[0].suggested_price, Some(249));
    assert_eq!(prices.listings[0].status, "fresh");
    exec(&db.db,"UPDATE listing_board_review SET date_time=now()-interval '31 minutes' WHERE world_id=30000 AND item_id=1",vec![]).await.unwrap();
    // Neither sales-only ingestion nor a delta proves untouched competitors
    // were reviewed, even though both advance the catch-up ingest marker.
    db.update_sales(
        vec![universalis::SaleView {
            hq: false,
            price_per_unit: 250,
            quantity: 1,
            timestamp: chrono::Local::now(),
            on_mannequin: false,
            world_name: None,
            world_id: None,
            buyer_name: format!("PriceFixtureBuyer{suffix}"),
            total: 250,
        }],
        universalis::ItemId(1),
        universalis::WorldId(30000),
    )
    .await
    .unwrap();
    db.remove_listings(
        vec![board[0].clone()],
        universalis::ItemId(1),
        universalis::WorldId(30000),
    )
    .await
    .unwrap();
    db.add_listings(
        vec![board[0].clone()],
        universalis::ItemId(1),
        universalis::WorldId(30000),
    )
    .await
    .unwrap();
    let stale_prices = db
        .plugin_retainer_prices(&principal, retainer)
        .await
        .unwrap();
    assert_eq!(stale_prices.listings[0].competitor_price, Some(250));
    assert_eq!(stale_prices.listings[0].suggested_price, None);
    assert_eq!(stale_prices.listings[0].status, "stale");
    assert_eq!(one(&db.db,"SELECT count(*)::bigint AS n FROM active_listing WHERE world_id=30000 AND retainer_id IN ($1,$2,$3)",
        vec![public_retainer.into(),own_comparison.into(),competitor.into()]).await.unwrap().try_get::<i64>("","n").unwrap(),4,
        "lookup must not create or alter public listing rows");
    let ingest_before = one(
        &db.db,
        "SELECT date_time FROM listing_last_updated WHERE world_id=30000 AND item_id=1",
        vec![],
    )
    .await
    .unwrap()
    .try_get::<chrono::NaiveDateTime>("", "date_time")
    .unwrap();
    // A transient failure in the new review write must leave catch-up eligible.
    db.db.execute_unprepared("CREATE OR REPLACE FUNCTION ultros_plugin_review_fail() RETURNS trigger
        LANGUAGE plpgsql AS $fixture$ BEGIN RAISE EXCEPTION 'review fixture failure'; END; $fixture$;
        DROP TRIGGER IF EXISTS ultros_plugin_review_fail ON listing_board_review;
        CREATE TRIGGER ultros_plugin_review_fail BEFORE INSERT OR UPDATE ON listing_board_review
        FOR EACH ROW WHEN (NEW.world_id=30000 AND NEW.item_id=1) EXECUTE FUNCTION ultros_plugin_review_fail()")
        .await.unwrap();
    let failed_review = db
        .update_listings(
            board.clone(),
            universalis::ItemId(1),
            universalis::WorldId(30000),
        )
        .await;
    db.db
        .execute_unprepared(
            "DROP TRIGGER ultros_plugin_review_fail ON listing_board_review;
        DROP FUNCTION ultros_plugin_review_fail()",
        )
        .await
        .unwrap();
    assert!(failed_review.is_err());
    assert_eq!(
        one(
            &db.db,
            "SELECT date_time FROM listing_last_updated WHERE world_id=30000 AND item_id=1",
            vec![]
        )
        .await
        .unwrap()
        .try_get::<chrono::NaiveDateTime>("", "date_time")
        .unwrap(),
        ingest_before
    );
    assert_eq!(
        db.plugin_retainer_prices(&principal, retainer)
            .await
            .unwrap()
            .listings[0]
            .status,
        "stale"
    );
    db.update_listings(board, universalis::ItemId(1), universalis::WorldId(30000))
        .await
        .unwrap();
    assert_eq!(
        db.plugin_retainer_prices(&principal, retainer)
            .await
            .unwrap()
            .listings[0]
            .suggested_price,
        Some(249)
    );
    exec(
        &db.db,
        "DELETE FROM listing_board_review WHERE world_id=30000 AND item_id=1",
        vec![],
    )
    .await
    .unwrap();
    exec(
        &db.db,
        "DELETE FROM active_listing WHERE world_id=30000 AND retainer_id IN ($1,$2,$3)",
        vec![
            public_retainer.into(),
            own_comparison.into(),
            competitor.into(),
        ],
    )
    .await
    .unwrap();
    exec(
        &db.db,
        "DELETE FROM owned_retainers WHERE discord_id=$1 AND retainer_id=$2",
        vec![owner.into(), own_comparison.into()],
    )
    .await
    .unwrap();
    exec(
        &db.db,
        "DELETE FROM retainer WHERE id IN ($1,$2)",
        vec![own_comparison.into(), competitor.into()],
    )
    .await
    .unwrap();
    exec(
        &db.db,
        "DELETE FROM listing_last_updated WHERE world_id=30000 AND item_id=1",
        vec![],
    )
    .await
    .unwrap();
    let canonical = one(&db.db,
        "INSERT INTO active_listing(world_id,retainer_id,item_id,hq,quantity,price_per_unit,timestamp) VALUES(30000,$1,$2,$3,$4,$5,$6) RETURNING id",
        vec![public_retainer.into(),listing.item_id.into(),listing.hq.into(),listing.quantity.into(),(listing.unit_price as i32).into(),capture.observed_at.naive_utc().into()]).await.unwrap().try_get::<i64>("","id").unwrap();
    let confirmed = db.website_retainer_listings(owner).await.unwrap();
    assert_eq!(
        confirmed.retainers[0].1[0].1.len(),
        1,
        "confirmation must not duplicate the uploaded stack"
    );
    assert!(
        matches!(confirmed.retainers[0].1[0].1[0].id,ultros_api_types::user::RetainerListingId::Universalis { id } if id==canonical)
    );
    exec(
        &db.db,
        "DELETE FROM active_listing WHERE id=$1",
        vec![canonical.into()],
    )
    .await
    .unwrap();
    assert!(
        db.website_retainer_listings(owner).await.unwrap().retainers[0].1[0]
            .1
            .is_empty(),
        "a confirmed sale must not revive the private row"
    );
    assert_eq!(
        db.upload_plugin_listings(&principal, retainer, &capture)
            .await
            .unwrap()
            .revision,
        1
    );
    let changed_capture = ListingCapture {
        listings: vec![],
        ..capture.clone()
    };
    assert!(matches!(
        db.upload_plugin_listings(&principal, retainer, &changed_capture)
            .await,
        Err(PluginError::Conflict("operation_id_reused"))
    ));
    let stale = ListingCapture {
        capture_id: format!("capture-stale-{suffix}"),
        ..capture.clone()
    };
    assert!(matches!(
        db.upload_plugin_listings(&principal, retainer, &stale)
            .await,
        Err(PluginError::Conflict("revisit_retainer_required"))
    ));
    let incomplete = ListingCapture {
        complete: false,
        capture_id: format!("capture-bad-{suffix}"),
        ..capture.clone()
    };
    assert!(
        db.upload_plugin_listings(&principal, retainer, &incomplete)
            .await
            .is_err()
    );
    let empty = ListingCapture {
        capture_id: format!("capture-empty-{suffix}"),
        base_revision: 1,
        listings: vec![],
        ..capture.clone()
    };
    assert_eq!(
        db.upload_plugin_listings(&principal, retainer, &empty)
            .await
            .unwrap()
            .revision,
        2
    );
    assert!(
        db.plugin_retainer_snapshots(&principal).await.unwrap()[0]
            .listings
            .is_empty()
    );
    db.set_plugin_upload_consent(
        &principal,
        character,
        &UploadConsent {
            enabled: false,
            delete_snapshots: true,
        },
    )
    .await
    .unwrap();
    assert!(
        db.plugin_retainer_snapshots(&principal)
            .await
            .unwrap()
            .is_empty()
    );
    let fallback_row = one(&db.db,
        "INSERT INTO active_listing(world_id,retainer_id,item_id,hq,quantity,price_per_unit,timestamp) VALUES(30000,$1,1,false,5,100,now()) RETURNING id",
        vec![public_retainer.into()]).await.unwrap().try_get::<i64>("","id").unwrap();
    let fallback = db
        .plugin_retainer_prices(&principal, retainer)
        .await
        .unwrap();
    assert_eq!(fallback.source, "universalis");
    assert_eq!(fallback.listings.len(), 1);
    assert_eq!(fallback.listings[0].competitor_price, None);
    assert_eq!(fallback.listings[0].suggested_price, None);
    exec(
        &db.db,
        "DELETE FROM active_listing WHERE id=$1",
        vec![fallback_row.into()],
    )
    .await
    .unwrap();
    assert!(
        db.upload_plugin_listings(&principal, retainer, &capture)
            .await
            .is_err()
    );
    let public = one(
        &db.db,
        "SELECT count(*)::bigint AS n FROM retainer WHERE name='FixtureRetainer'",
        vec![],
    )
    .await
    .unwrap();
    assert_eq!(
        public.try_get::<i64>("", "n").unwrap(),
        1,
        "registration creates the retainer identity without creating public listings"
    );

    // Existing public market records participate in the normal website flows.
    // Keep an existing character assignment and ordering across fresh operation
    // IDs, and never replace its city with the game's private observation.
    let public_name = format!("MarketFixture{suffix}");
    let public_id = one(
        &db.db,
        "INSERT INTO retainer(name,world_id,retainer_city_id) VALUES($1,30000,2) RETURNING id",
        vec![public_name.clone().into()],
    )
    .await
    .unwrap()
    .try_get::<i32>("", "id")
    .unwrap();
    let existing_character = 30001;
    db.insert_character(existing_character, "Existing", "Character", 30000)
        .await
        .unwrap();
    db.create_owned_character(owner, existing_character)
        .await
        .unwrap();
    exec(&db.db,"INSERT INTO owned_retainers(discord_id,retainer_id,character_id,weight) VALUES($1,$2,$3,7)",
        vec![owner.into(),public_id.into(),existing_character.into()]).await.unwrap();
    let market_observation = ObserveCharacter {
        retainers: vec![ObservedRetainer {
            content_id: "333333".into(),
            name: public_name,
            city: Some(1),
        }],
        ..observation.clone()
    };
    for operation in ["market-first", "market-repeat"] {
        let saved = db
            .register_plugin_retainers(
                &principal,
                &keys,
                &RegisterRetainers {
                    operation_id: format!("{operation}-{suffix}"),
                    character: market_observation.clone(),
                },
            )
            .await
            .unwrap();
        assert!(
            saved
                .iter()
                .flat_map(|c| &c.retainers)
                .any(|r| r.public_retainer_id == Some(public_id))
        );
    }
    let owned = query(
        &db.db,
        "SELECT character_id,weight FROM owned_retainers WHERE discord_id=$1 AND retainer_id=$2",
        vec![owner.into(), public_id.into()],
    )
    .await
    .unwrap();
    assert_eq!(owned.len(), 1);
    assert_eq!(
        owned[0].try_get::<Option<i32>>("", "character_id").unwrap(),
        Some(existing_character)
    );
    assert_eq!(
        owned[0].try_get::<Option<i32>>("", "weight").unwrap(),
        Some(7)
    );
    assert_eq!(
        one(
            &db.db,
            "SELECT retainer_city_id FROM retainer WHERE id=$1",
            vec![public_id.into()]
        )
        .await
        .unwrap()
        .try_get::<i32>("", "retainer_city_id")
        .unwrap(),
        2
    );
    exec(
        &db.db,
        "DELETE FROM owned_retainers WHERE discord_id=$1 AND retainer_id=$2",
        vec![owner.into(), public_id.into()],
    )
    .await
    .unwrap();
    // Registration also creates the normal owned row when it didn't exist.
    db.register_plugin_retainers(
        &principal,
        &keys,
        &RegisterRetainers {
            operation_id: format!("market-create-{suffix}"),
            character: market_observation,
        },
    )
    .await
    .unwrap();
    assert!(
        db.get_owned_retainer_ids(owner)
            .await
            .unwrap()
            .contains(&public_id)
    );
    exec(
        &db.db,
        "DELETE FROM owned_retainers WHERE discord_id=$1 AND retainer_id=$2",
        vec![owner.into(), public_id.into()],
    )
    .await
    .unwrap();
    exec(
        &db.db,
        "DELETE FROM retainer WHERE id=$1",
        vec![public_id.into()],
    )
    .await
    .unwrap();
    exec(
        &db.db,
        "DELETE FROM owned_ffxiv_character WHERE discord_user_id=$1 AND ffxiv_character_id=$2",
        vec![owner.into(), existing_character.into()],
    )
    .await
    .unwrap();

    db.test_plugin_delivery(&principal, &format!("test-delivery-{suffix}"))
        .await
        .unwrap();
    db.test_plugin_delivery(&principal, &format!("test-delivery-{suffix}"))
        .await
        .unwrap();
    let pending = db.pending_plugin_deliveries(&principal).await.unwrap();
    assert_eq!(pending.deliveries.len(), 1);
    let ids = DeliveryIds {
        ids: vec![pending.deliveries[0].id.clone()],
    };
    assert!(
        db.acknowledge_plugin_deliveries(&read_only, &ids, false)
            .await
            .is_err()
    );
    db.acknowledge_plugin_deliveries(&principal, &ids, false)
        .await
        .unwrap();
    assert!(
        db.pending_plugin_deliveries(&principal)
            .await
            .unwrap()
            .deliveries
            .is_empty()
    );

    let alert = one(
        &db.db,
        "INSERT INTO alert(owner,enabled,cooldown_seconds) VALUES($1,true,60) RETURNING id",
        vec![owner.into()],
    )
    .await
    .unwrap()
    .try_get::<i32>("", "id")
    .unwrap();
    let webhook = db
        .create_endpoint(
            owner,
            "Fixture webhook",
            "Webhook",
            json!({"url":"https://discord.com/api/webhooks/1/fixture"}),
        )
        .await
        .unwrap();
    exec(
        &db.db,
        "INSERT INTO alert_notification_rule(alert_id,endpoint_id) VALUES($1,$2),($1,$3)",
        vec![
            alert.into(),
            session.endpoint_id.unwrap().into(),
            webhook.into(),
        ],
    )
    .await
    .unwrap();
    let policy = json!({"click_url":"/item/1","ttl_secs":300,"topic":"fixture"});
    // Any failure inserting destination work rolls the event and cooldown back too.
    db.db.execute_unprepared("CREATE OR REPLACE FUNCTION plugin_fixture_reject() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.payload->>'title'='reject fixture' THEN RAISE EXCEPTION 'fixture queue failure'; END IF; RETURN NEW; END $$; DROP TRIGGER IF EXISTS plugin_fixture_reject ON notification_outbox; CREATE TRIGGER plugin_fixture_reject BEFORE INSERT ON notification_outbox FOR EACH ROW EXECUTE FUNCTION plugin_fixture_reject()").await.unwrap();
    assert!(
        db.accept_alert_fire(owner, fire(alert, "reject fixture"), policy.clone())
            .await
            .is_err()
    );
    assert!(
        db.get_alert(alert)
            .await
            .unwrap()
            .unwrap()
            .last_fired_at
            .is_none()
    );
    assert!(
        query(
            &db.db,
            "SELECT id FROM alert_event WHERE alert_id=$1",
            vec![alert.into()]
        )
        .await
        .unwrap()
        .is_empty()
    );
    assert!(
        db.pending_plugin_deliveries(&principal)
            .await
            .unwrap()
            .deliveries
            .is_empty()
    );
    let (a, b) = tokio::join!(
        db.accept_alert_fire(owner, fire(alert, "accepted"), policy.clone()),
        db.accept_alert_fire(owner, fire(alert, "accepted"), policy)
    );
    assert_eq!(
        usize::from(a.as_ref().unwrap().is_some()) + usize::from(b.as_ref().unwrap().is_some()),
        1
    );
    let accepted = a.unwrap().or(b.unwrap()).unwrap();
    assert!(accepted.accepted_at.is_some());
    assert!(!accepted.delivered);
    assert_eq!(accepted.pending_destinations, 2);
    let game = db
        .pending_plugin_deliveries(&principal)
        .await
        .unwrap()
        .deliveries;
    assert_eq!(game.len(), 1);
    assert_eq!(game[0].event_id, Some(accepted.id));
    let delivery_ids = DeliveryIds {
        ids: vec![game[0].id.clone()],
    };
    db.acknowledge_plugin_deliveries(&principal, &delivery_ids, false)
        .await
        .unwrap();
    let event = db
        .get_alert_event_by_id_owned_by(owner, accepted.id)
        .await
        .unwrap();
    assert!(event.delivered);
    assert!(event.read_at.is_none());
    assert_eq!(event.pending_destinations, 1);
    db.acknowledge_plugin_deliveries(&principal, &delivery_ids, true)
        .await
        .unwrap();
    assert!(
        db.get_alert_event_by_id_owned_by(owner, accepted.id)
            .await
            .unwrap()
            .read_at
            .is_some()
    );
    let (first, second) = tokio::join!(
        db.claim_notification_outbox(true),
        db.claim_notification_outbox(true)
    );
    let mut jobs = first.unwrap();
    jobs.extend(second.unwrap());
    assert_eq!(jobs.len(), 1);
    let job = jobs.pop().unwrap();
    assert!(db.authorize_notification_job(&job).await.unwrap().is_some());
    exec(&db.db,"UPDATE notification_outbox SET lease_until=now()-interval '1 second',next_attempt_at=now() WHERE id=$1",vec![job.id.into()]).await.unwrap();
    let fresh = db
        .claim_notification_outbox(true)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert!(
        db.finish_notification_job(&job, true, false)
            .await
            .unwrap()
            .is_none(),
        "a stale lease cannot complete the next worker's claim"
    );
    assert!(
        db.finish_notification_job(&fresh, true, false)
            .await
            .unwrap()
            .is_some()
    );

    let user = db
        .get_or_create_discord_user(owner as u64, "Plugin fixture owner".into())
        .await
        .unwrap();
    let list = db
        .create_list(
            user,
            "Plugin cart".into(),
            Some(crate::world_data::world_cache::AnySelector::World(30000)),
        )
        .await
        .unwrap();
    let stored = db.plugin_list_snapshot(&principal, list.id).await.unwrap();
    let local = ListDocument::from_snapshot(&stored.snapshot).unwrap();
    local.add_row(RowKey::new(1, None), 5, None).unwrap();
    db.apply_plugin_list_update(
        &principal,
        list.id,
        &local.export_since(&stored.version).unwrap(),
    )
    .await
    .unwrap();
    let stored_again = db.plugin_list_snapshot(&principal, list.id).await.unwrap();
    let hidden_meta = ListDocument::from_snapshot(&stored_again.snapshot).unwrap();
    hidden_meta.rename("Temporarily changed").unwrap();
    hidden_meta.rename("Plugin cart").unwrap();
    assert!(
        matches!(
            db.apply_plugin_list_update(
                &principal,
                list.id,
                &hidden_meta.export_since(&stored_again.version).unwrap()
            )
            .await,
            Err(crate::list_doc::ListDocError::MetaForbidden)
        ),
        "even losing or reverted metadata operations exceed a row-only grant"
    );
    local.rename("Forbidden metadata").unwrap();
    assert!(matches!(
        db.apply_plugin_list_update(&principal, list.id, &local.export_all().unwrap())
            .await,
        Err(crate::list_doc::ListDocError::MetaForbidden)
    ));
    db.share_list_with_user(list.id, owner, other, ListPermission::Write)
        .await
        .unwrap();
    assert!(
        db.apply_plugin_list_update(&read_only, list.id, &local.export_all().unwrap())
            .await
            .is_err(),
        "resource write permission does not replace the lists:write scope"
    );
    assert!(db.plugin_list_snapshot(&read_only, list.id).await.is_ok());
    db.unshare_list_from_user(list.id, owner, other)
        .await
        .unwrap();
    assert!(db.plugin_list_snapshot(&read_only, list.id).await.is_err());

    // Private registration can create unknown or client-reported city metadata.
    // Both must yield to Universalis, without later private observations undoing it.
    for (index, city) in [None, Some(1)].into_iter().enumerate() {
        let name = format!("CityFixture{suffix}{index}");
        let character = ObserveCharacter {
            content_id: format!("70000{index}"),
            name: format!("City Character {index}"),
            home_world: 30000,
            retainers: vec![ObservedRetainer {
                content_id: format!("80000{index}"),
                name: name.clone(),
                city,
            }],
        };
        db.register_plugin_retainers(
            &principal,
            &keys,
            &RegisterRetainers {
                operation_id: format!("city-{suffix}-{index}"),
                character: character.clone(),
            },
        )
        .await
        .unwrap();
        let city_row = || {
            one(
                &db.db,
                "SELECT id,retainer_city_id,xmin::text AS row_version FROM retainer WHERE world_id=30000 AND name=$1",
                vec![name.clone().into()],
            )
        };
        assert_eq!(
            city_row()
                .await
                .unwrap()
                .try_get::<i32>("", "retainer_city_id")
                .unwrap(),
            city.unwrap_or(0)
        );
        db.add_listings(
            vec![market_listing(&name, 2, 100, false)],
            universalis::ItemId(2),
            universalis::WorldId(30000),
        )
        .await
        .unwrap();
        let authoritative = city_row().await.unwrap();
        assert_eq!(
            authoritative
                .try_get::<i32>("", "retainer_city_id")
                .unwrap(),
            2
        );
        db.add_listings(
            vec![market_listing(&name, 0, 100, false)],
            universalis::ItemId(2),
            universalis::WorldId(30000),
        )
        .await
        .unwrap();
        assert_eq!(
            city_row()
                .await
                .unwrap()
                .try_get::<i32>("", "retainer_city_id")
                .unwrap(),
            2,
            "unknown authoritative metadata must preserve a known city"
        );
        // Exercise the upsert race fallback independently of the resolver's
        // existing-row fast path.
        assert_eq!(
            db.store_retainer("", &name, universalis::WorldId(30000), 0)
                .await
                .unwrap()
                .retainer_city_id,
            2
        );
        let authoritative = city_row().await.unwrap();
        db.add_listings(
            vec![market_listing(&name, 2, 100, false)],
            universalis::ItemId(2),
            universalis::WorldId(30000),
        )
        .await
        .unwrap();
        assert_eq!(
            city_row()
                .await
                .unwrap()
                .try_get::<String>("", "row_version")
                .unwrap(),
            authoritative.try_get::<String>("", "row_version").unwrap(),
            "unchanged public city must not cause a retainer write"
        );
        db.register_plugin_retainers(
            &principal,
            &keys,
            &RegisterRetainers {
                operation_id: format!("city-repeat-{suffix}-{index}"),
                character,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            city_row()
                .await
                .unwrap()
                .try_get::<i32>("", "retainer_city_id")
                .unwrap(),
            2
        );
        let id = authoritative.try_get::<i32>("", "id").unwrap();
        exec(
            &db.db,
            "DELETE FROM active_listing WHERE retainer_id=$1",
            vec![id.into()],
        )
        .await
        .unwrap();
    }

    let next = db
        .refresh_plugin_session(&tokens.refresh_token)
        .await
        .unwrap();
    assert!(db.authenticate_plugin(&next.access_token).await.is_ok());
    assert!(
        db.refresh_plugin_session(&tokens.refresh_token)
            .await
            .is_err()
    );
    assert!(
        db.authenticate_plugin(&next.access_token).await.is_err(),
        "refresh reuse revokes the whole family"
    );
    assert!(db.plugin_session(&principal).await.is_err());
    db.revoke_plugin_credential(&tokens.refresh_token)
        .await
        .unwrap();
    println!(
        "Device pairing, scope isolation, private registration/upload, durable notification rollback/fencing, CRDT authorization and refresh reuse passed."
    );
}

#[tokio::test]
#[ignore = "requires an idle disposable ULTROS_PLUGIN_TEST_DATABASE_URL"]
async fn discordless_workers_preserve_discord_attempts_postgres() {
    let url = std::env::var("ULTROS_PLUGIN_TEST_DATABASE_URL").unwrap();
    assert!(
        disposable_plugin_database(&url),
        "refusing a non-test database"
    );
    let db = UltrosDb::from_connection(Database::connect(url).await.unwrap());
    Migrator::up(db.get_connection(), None).await.unwrap();
    let owner = 900_000_000_000_000_123_i64;
    exec(
        &db.db,
        "DELETE FROM discord_user WHERE id=$1",
        vec![owner.into()],
    )
    .await
    .unwrap();
    db.get_or_create_discord_user(owner as u64, "Readiness fixture".into())
        .await
        .unwrap();
    let alert = one(
        &db.db,
        "INSERT INTO alert(owner,enabled,cooldown_seconds) VALUES($1,true,0) RETURNING id",
        vec![owner.into()],
    )
    .await
    .unwrap()
    .try_get::<i32>("", "id")
    .unwrap();
    for (method, config) in [
        ("DiscordChannel", json!({"channel_id":123})),
        ("DiscordDm", json!({"user_id":owner})),
        (
            "Webhook",
            json!({"url":"https://discord.com/api/webhooks/1/fixture"}),
        ),
    ] {
        let endpoint = db
            .create_endpoint(owner, method, method, config)
            .await
            .unwrap();
        exec(
            &db.db,
            "INSERT INTO alert_notification_rule(alert_id,endpoint_id) VALUES($1,$2)",
            vec![alert.into(), endpoint.into()],
        )
        .await
        .unwrap();
    }
    exec(
        &db.db,
        "INSERT INTO alert_discord_destination(alert_id,channel_id) VALUES($1,456)",
        vec![alert.into()],
    )
    .await
    .unwrap();
    let event = db
        .accept_alert_fire(owner, fire(alert, "Readiness fixture"), json!({}))
        .await
        .unwrap()
        .unwrap();
    let jobs = db.claim_notification_outbox(false).await.unwrap();
    let own: Vec<_> = jobs.iter().filter(|job| job.event_id == event.id).collect();
    assert_eq!(own.len(), 1, "botless workers still claim webhooks");
    let rows = query(&db.db, "SELECT o.attempts,n.method FROM notification_outbox o LEFT JOIN notification_endpoint n ON n.id=o.endpoint_id WHERE o.event_id=$1", vec![event.id.into()]).await.unwrap();
    assert_eq!(rows.len(), 4);
    for row in rows {
        let webhook = row
            .try_get::<Option<String>>("", "method")
            .unwrap()
            .as_deref()
            == Some("Webhook");
        assert_eq!(
            row.try_get::<i32>("", "attempts").unwrap(),
            i32::from(webhook)
        );
    }
    db.finish_notification_job(own[0], true, false)
        .await
        .unwrap();
    let jobs = db.claim_notification_outbox(true).await.unwrap();
    let own: Vec<_> = jobs.iter().filter(|job| job.event_id == event.id).collect();
    assert_eq!(
        own.len(),
        3,
        "ready workers retain Discord endpoints and old-policy legacy jobs"
    );
    for job in own {
        assert!(db.authorize_notification_job(job).await.unwrap().is_some());
        db.finish_notification_job(job, true, false).await.unwrap();
    }
}
