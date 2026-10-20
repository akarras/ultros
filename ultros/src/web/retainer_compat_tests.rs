use super::*;
use axum::body::to_bytes;
use migration::{Migrator, MigratorTrait};
use sea_orm::{ConnectOptions, ConnectionTrait, Database};
use serde::Deserialize;
use ultros_api_types::user::{UserRetainerListings, UserRetainers};

// These DTOs deliberately mirror the pre-plugin client, not the new API types.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OldCharacter {
    id: i32,
    first_name: String,
    last_name: String,
    world_id: i32,
}
#[derive(Deserialize)]
struct OldRetainers {
    retainers: OldRetainerGroups,
}
type OldRetainerGroups = Vec<(Option<OldCharacter>, Vec<(OwnedRetainer, Retainer)>)>;
#[derive(Deserialize)]
struct OldListings {
    retainers: OldListingGroups,
}
type OldListingGroups = Vec<(Option<OldCharacter>, Vec<(Retainer, Vec<ActiveListing>)>)>;
async fn json(response: impl IntoResponse) -> Vec<u8> {
    let response = response.into_response();
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    to_bytes(response.into_body(), 1_000_000)
        .await
        .unwrap()
        .to_vec()
}

#[tokio::test]
#[ignore = "requires ULTROS_PLUGIN_TEST_DATABASE_URL pointing at a disposable loopback database"]
async fn legacy_retainer_handlers_remain_readable_with_private_captures() {
    let url = std::env::var("ULTROS_PLUGIN_TEST_DATABASE_URL").unwrap();
    assert!(
        url.starts_with("postgres://ultros_plugin@127.0.0.1:55432/ultros_plugin_")
            || url.starts_with("postgres://ultros_plugin@127.0.0.1:55437/ultros_plugin_"),
        "refusing a non-test database"
    );
    let mut options = ConnectOptions::new(url);
    options
        .max_connections(3)
        .min_connections(1)
        .sqlx_logging(false);
    let connection = Database::connect(options).await.unwrap();
    Migrator::up(&connection, None).await.unwrap();
    let db = UltrosDb::from_connection(connection);
    db.insert_default_retainer_cities().await.unwrap();
    db.get_connection().execute_unprepared(r#"
        DELETE FROM owned_retainers WHERE discord_id=900000000000000121;
        DELETE FROM discord_user WHERE id=900000000000000121;
        INSERT INTO discord_user(id,username) VALUES(900000000000000121,'Legacy handler fixture');
        INSERT INTO region(id,name) VALUES(30001,'CompatibilityRegion') ON CONFLICT DO NOTHING;
        INSERT INTO datacenter(id,name,region_id) VALUES(30001,'CompatibilityDC',30001) ON CONFLICT DO NOTHING;
        INSERT INTO world(id,name,datacenter_id) VALUES(30001,'CompatibilityWorld',30001) ON CONFLICT DO NOTHING;
        INSERT INTO final_fantasy_character(id,first_name,last_name,world_id) VALUES(90000121,'Verified','Fixture',30001) ON CONFLICT DO NOTHING;
        INSERT INTO retainer(id,world_id,name,retainer_city_id) VALUES(90000121,30001,'VerifiedFixtureRetainer',1),(90000122,30001,'ObservedFixtureRetainer',1) ON CONFLICT DO NOTHING;
        INSERT INTO owned_retainers(retainer_id,discord_id,character_id) VALUES(90000121,900000000000000121,90000121),(90000122,900000000000000121,NULL);
        DELETE FROM active_listing WHERE id=900000121;
        INSERT INTO active_listing(id,world_id,retainer_id,item_id,hq,price_per_unit,quantity,timestamp) VALUES(900000121,30001,90000121,8,false,100,5,now());
        WITH c AS (INSERT INTO plugin_character(owner,fingerprint,name,home_world,registered) VALUES(900000000000000121,'\x010203','Observed Fixture',30001,true) RETURNING id),
        r AS (INSERT INTO plugin_retainer(owner,fingerprint,name,observed_character,registered_character,public_retainer_id,revision) SELECT 900000000000000121,'\x040506','ObservedFixtureRetainer',id,id,90000122,1 FROM c RETURNING id)
        INSERT INTO plugin_snapshot(owner,retainer_id,revision,capture_id,body_hash,listings,observed_at) SELECT 900000000000000121,id,1,'compatibility-capture','\x070809','[{"slot":0,"item_id":9,"hq":false,"quantity":2,"unit_price":200}]',now() FROM r;
    "#).await.unwrap();
    let user = AuthDiscordUser {
        id: 900_000_000_000_000_121,
        name: "Legacy handler fixture".into(),
        avatar_url: String::new(),
    };
    let old: OldRetainers = serde_json::from_slice(
        &json(
            user_retainers(State(db.clone()), user.clone())
                .await
                .unwrap(),
        )
        .await,
    )
    .unwrap();
    let character = old.retainers.iter().find_map(|(c, _)| c.as_ref()).unwrap();
    assert_eq!((character.id, character.world_id), (90000121, 30001));
    assert_eq!(
        (&*character.first_name, &*character.last_name),
        ("Verified", "Fixture")
    );
    assert_eq!(old.retainers.iter().flat_map(|(_, rows)| rows).count(), 2);
    let old: OldListings = serde_json::from_slice(
        &json(
            user_retainer_listings(State(db.clone()), user.clone())
                .await
                .unwrap(),
        )
        .await,
    )
    .unwrap();
    assert_eq!(old.retainers.iter().filter(|(c, _)| c.is_some()).count(), 1);
    let rows: Vec<_> = old
        .retainers
        .into_iter()
        .flat_map(|(_, r)| r)
        .flat_map(|(_, rows)| rows)
        .collect();
    assert_eq!(rows.len(), 1, "v1 must not expose private listing rows");
    assert_eq!(rows[0].id, 900000121, "legacy listing ids remain integers");
    let modern: UserRetainers = serde_json::from_slice(
        &json(
            user_retainers_v2(State(db.clone()), user.clone())
                .await
                .unwrap(),
        )
        .await,
    )
    .unwrap();
    assert!(
        modern
            .retainers
            .iter()
            .any(|(c, _)| c.as_ref().is_some_and(|c| c.is_unverified()))
    );
    let modern: UserRetainerListings = serde_json::from_slice(
        &json(user_retainer_listings_v2(State(db), user).await.unwrap()).await,
    )
    .unwrap();
    let rows: Vec<_> = modern
        .retainers
        .into_iter()
        .flat_map(|(_, r)| r)
        .flat_map(|(_, rows)| rows)
        .collect();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|r| matches!(
        r.id,
        ultros_api_types::user::RetainerListingId::InGame { .. }
    )));
}
