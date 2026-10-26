//! Fixtures must opt into a disposable loopback database explicitly.

use migration::{Migrator, MigratorTrait};
use sea_orm::{ConnectOptions, ConnectionTrait, Database};

pub(crate) async fn connect() -> crate::UltrosDb {
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
    crate::UltrosDb::from_connection(connection)
}

pub(crate) async fn seed_market(db: &crate::UltrosDb) {
    db.get_connection().execute_unprepared(
        "INSERT INTO region(id,name) VALUES(30001,'AuditRegion') ON CONFLICT DO NOTHING;
         INSERT INTO retainer_city(id,name) VALUES(1,'AuditCity') ON CONFLICT DO NOTHING;
         INSERT INTO datacenter(id,name,region_id) VALUES(30001,'AuditDC',30001) ON CONFLICT DO NOTHING;
         INSERT INTO world(id,name,datacenter_id) VALUES(30001,'AuditWorld',30001) ON CONFLICT DO NOTHING;
         DELETE FROM active_listing WHERE world_id=30001;
         DELETE FROM listing_board_review WHERE world_id=30001;
         DELETE FROM listing_board_revision WHERE world_id=30001;
         DELETE FROM listing_last_updated WHERE world_id=30001;"
    ).await.unwrap();
}

pub(crate) fn listing(id: &str, price: u32) -> universalis::ListingView {
    universalis::ListingView {
        last_review_time: chrono::Local::now(),
        price_per_unit: Some(price),
        quantity: Some(1),
        hq: false,
        listing_id: Some(id.into()),
        retainer_name: format!("Audit{id}"),
        retainer_id: None,
        retainer_city: 1,
        total: price,
        tax: 0,
        stain_id: None,
        world_name: None,
        world_id: None,
        creator_name: None,
        creator_id: None,
        is_crafted: false,
        materia: vec![],
        on_mannequin: false,
        seller_id: None,
    }
}

pub(crate) fn disposable_plugin_database(url: &str) -> bool {
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
