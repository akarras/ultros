mod account;
mod alerts;
pub mod auth_revocation;
pub mod clickhouse_outbox;
pub mod common;
pub mod common_type_conversions;
mod discord;
pub mod entity;
mod ffxiv_character;
pub mod group_roles;
pub mod guest_list_adoption;
mod ingest_budget;
pub mod list_doc;
pub mod list_projection_check;
pub mod listings;
pub mod lists;
pub mod market_sweep;
pub mod notification_outbox;
pub mod plugin;
pub mod recent_sales;
pub mod recently_updated;
pub mod retainers;
pub mod sales;
pub mod wakeup;
pub mod world_data;

pub use alerts::{NewAlertEvent, NewMarketTriggerAlert};
pub use sea_orm::ActiveValue;
pub use sea_orm::error::DbErr as SeaDbErr;

use anyhow::Result;
use dotenvy::dotenv;
use migration::{Migrator, MigratorTrait};

use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, ConnectOptions, Database,
    DatabaseConnection, DbErr, EntityTrait, FromQueryResult, IntoActiveModel, ModelTrait,
    QueryFilter, QueryOrder, QuerySelect, Set,
};

use std::sync::Arc;

use tracing::{error, info, instrument, warn};
use ultros_api_types::Retainer;
use universalis::{ItemId, ListingView, WorldId};

use crate::entity::*;

/// Env var that skips running `Migrator::up` during `UltrosDb::connect`.
///
/// Useful for read/web replicas when migrations are owned by a single-owner
/// worker or a pre-deploy migration step, and required when rolling N-1
/// replicas alongside a newer N schema (SeaORM 2.0.4 refuses to run
/// `Migrator::up` against a database containing migrations unknown to the
/// booting binary).
pub const SKIP_MIGRATIONS_ENV: &str = "ULTROS_SKIP_MIGRATIONS";

/// Stable Postgres session advisory-lock key serializing `Migrator::up` across
/// replicas that boot concurrently against the same database (`"ULTROSMI"`).
pub const MIGRATION_LOCK_KEY: i64 = 0x55_4c_54_52_4f_53_4d_49;

/// Parse an opt-in boolean env flag using the same fail-closed conventions as
/// `ultros_server_core::env_flag`.
fn env_flag_enabled(name: &str, value: Option<&str>) -> bool {
    match value.map(str::trim) {
        None | Some("") => false,
        Some(v)
            if v.eq_ignore_ascii_case("0")
                || v.eq_ignore_ascii_case("false")
                || v.eq_ignore_ascii_case("no")
                || v.eq_ignore_ascii_case("off") =>
        {
            false
        }
        Some(v)
            if v.eq_ignore_ascii_case("1")
                || v.eq_ignore_ascii_case("true")
                || v.eq_ignore_ascii_case("yes")
                || v.eq_ignore_ascii_case("on") =>
        {
            true
        }
        Some(other) => {
            warn!(
                variable = name,
                value = other,
                "unrecognised boolean env value; treating as enabled"
            );
            true
        }
    }
}

/// Polling cadence while waiting for another replica to finish running
/// migrations under [`MIGRATION_LOCK_KEY`].
///
/// Using non-blocking `pg_try_advisory_lock` instead of blocking
/// `pg_advisory_lock` is essential in Postgres: a session blocked inside
/// `SELECT pg_advisory_lock($1)` holds an active `virtualxid`, which deadlocks
/// any `CREATE INDEX CONCURRENTLY` migration running on the lock holder
/// (because `CREATE INDEX CONCURRENTLY` waits for every older `virtualxid` to
/// finish). Polling leaves waiting sessions idle between short attempts so
/// concurrent index builds always complete immediately.
const MIGRATION_LOCK_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

/// Connect a dedicated single-session Postgres `DatabaseConnection` that
/// refuses to open replacement connections once the initial session connects.
///
/// Paired with [`run_migrations_locked`], this guarantees that if the
/// lock-owning Postgres backend ever disconnects (mid-migration or between
/// SeaORM's nontransactional migration statements), SQLx cannot silently open a
/// fresh connection without [`MIGRATION_LOCK_KEY`] and keep executing
/// migrations unlocked alongside another replica.
pub async fn connect_single_session_for_migrations(
    url: &str,
    search_path: Option<&str>,
) -> Result<DatabaseConnection> {
    use std::sync::atomic::{AtomicBool, Ordering};

    let established = Arc::new(AtomicBool::new(false));
    let search_path = search_path.map(str::to_owned);
    let pool = sea_orm::sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .min_connections(1)
        .after_connect(move |conn, _meta| {
            let established = established.clone();
            let search_path = search_path.clone();
            Box::pin(async move {
                if established.swap(true, Ordering::SeqCst) {
                    return Err(sea_orm::sqlx::Error::Protocol(
                        "Postgres migration lock session disconnected; refusing replacement connection"
                            .into(),
                    ));
                }
                if let Some(schema) = search_path {
                    sea_orm::sqlx::query("SELECT set_config('search_path', $1, false)")
                        .bind(schema)
                        .execute(conn)
                        .await?;
                }
                Ok(())
            })
        })
        .connect(url)
        .await?;
    Ok(sea_orm::SqlxPostgresConnector::from_sqlx_postgres_pool(
        pool,
    ))
}

/// Run `Migrator::up` serialized under a Postgres session advisory lock held on
/// `migration_db`, which MUST be constructed with
/// [`connect_single_session_for_migrations`] so the advisory lock and every
/// migration statement share the exact same non-replaceable Postgres backend
/// session.
///
/// Before each pending migration step, this also verifies `pg_backend_pid()`
/// still matches the PID that acquired [`MIGRATION_LOCK_KEY`].
pub async fn run_migrations_locked(migration_db: &DatabaseConnection) -> Result<()> {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    info!(
        lock_key = MIGRATION_LOCK_KEY,
        "acquiring Postgres migration advisory lock"
    );
    let mut logged_wait = false;
    let lock_pid: i32 = loop {
        let row = migration_db
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "SELECT pg_try_advisory_lock($1) AS acquired, pg_backend_pid() AS pid",
                [MIGRATION_LOCK_KEY.into()],
            ))
            .await?
            .ok_or_else(|| anyhow::anyhow!("pg_try_advisory_lock returned no row"))?;
        let acquired: bool = row.try_get("", "acquired")?;
        if acquired {
            break row.try_get("", "pid")?;
        }
        if !logged_wait {
            info!(
                lock_key = MIGRATION_LOCK_KEY,
                "waiting for another replica to finish database migrations"
            );
            logged_wait = true;
        }
        tokio::time::sleep(MIGRATION_LOCK_POLL_INTERVAL).await;
    };

    let migration_result: Result<()> = async {
        loop {
            verify_migration_lock_session(migration_db, lock_pid).await?;
            let pending = Migrator::get_pending_migrations(migration_db).await?;
            if pending.is_empty() {
                // Still run `up(..., None)` with zero pending migrations so
                // SeaORM performs its applied-migration integrity check while
                // we hold the lock.
                verify_migration_lock_session(migration_db, lock_pid).await?;
                Migrator::up(migration_db, None).await?;
                break;
            }
            verify_migration_lock_session(migration_db, lock_pid).await?;
            Migrator::up(migration_db, Some(1)).await?;
        }
        Ok(())
    }
    .await;

    let unlock_result = async {
        verify_migration_lock_session(migration_db, lock_pid).await?;
        migration_db
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "SELECT pg_advisory_unlock($1)",
                [MIGRATION_LOCK_KEY.into()],
            ))
            .await?;
        Ok::<(), anyhow::Error>(())
    }
    .await;
    if let Err(error) = unlock_result {
        warn!(
            ?error,
            lock_pid,
            "failed to explicitly unlock Postgres migration advisory lock; closing session"
        );
    }

    migration_result
}

async fn verify_migration_lock_session(
    migration_db: &DatabaseConnection,
    expected_pid: i32,
) -> Result<()> {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    let row = migration_db
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT pg_backend_pid() AS pid",
            [],
        ))
        .await?
        .ok_or_else(|| anyhow::anyhow!("pg_backend_pid returned no row"))?;
    let current_pid: i32 = row.try_get("", "pid")?;
    if current_pid != expected_pid {
        anyhow::bail!(
            "Postgres migration lock session dropped (expected backend pid {expected_pid}, got {current_pid}); aborting migrations"
        );
    }
    Ok(())
}

/// Pool ceiling when `POSTGRES_MAX_CONNECTIONS` is unset.
///
/// Was 300, which exceeds a stock Postgres `max_connections = 200` on its own:
/// one instance could claim the whole server, and two were enough to hand the
/// third `pool timed out while waiting for an open connection` at startup. A
/// default that cannot be run twice on one database is a footgun, so this now
/// matches the value `.env.example` has always suggested. Deployments that
/// genuinely need a larger pool set the variable explicitly.
const DEFAULT_MAX_CONNECTIONS: u32 = 50;

/// A default above a stock Postgres `max_connections = 200` lets one instance
/// claim the whole server. Whatever this value becomes, four instances against
/// a stock server have to stay inside it — enforced at compile time so raising
/// the default is a deliberate act rather than an accident.
const _: () = assert!(DEFAULT_MAX_CONNECTIONS * 4 <= 200);

/// Connections the pool opens eagerly and holds, when the ceiling allows it.
const DEFAULT_MIN_CONNECTIONS: u32 = 10;

/// Read a `u32` from `name`, logging and ignoring a value that will not parse.
fn env_u32(name: &str) -> Option<u32> {
    std::env::var(name).ok().and_then(|raw| {
        raw.parse::<u32>()
            .map_err(|e| error!(error = %e, variable = name, "Unable to read env variable"))
            .ok()
    })
}

/// Decide how many connections the pool opens eagerly and holds.
///
/// The floor used to be a hard-coded 10, which made a small pool impossible to
/// ask for: `POSTGRES_MAX_CONNECTIONS=2` still opened ten connections eagerly,
/// five times the ceiling it was given.
///
/// Clamping to the ceiling is necessary but not sufficient. A floor of 10 under
/// a ceiling of 15 leaves only five connections of headroom, and `Migrator::up`
/// needs some of that to run — which is why a server would not start at
/// `POSTGRES_MAX_CONNECTIONS=15` even with 21 connections free on the box. So
/// the floor is also settable outright via `POSTGRES_MIN_CONNECTIONS`, for
/// exactly the case this knob exists for: several instances sharing one
/// Postgres, where eagerly holding connections you are not using is the
/// problem.
fn min_connections_for(max_connections: u32, requested: Option<u32>) -> u32 {
    requested
        .unwrap_or(DEFAULT_MIN_CONNECTIONS)
        .min(max_connections)
}

#[derive(Clone, Debug)]
pub struct UltrosDb {
    // Connections here
    db: DatabaseConnection,
    ingest_budget: ingest_budget::IngestBudget,
    /// Per-process identifier embedded in `pg_notify` payloads so a replica
    /// ignores its own echoes.
    notify_origin: Arc<str>,
}

impl UltrosDb {
    /// Borrow the underlying `DatabaseConnection`. Used by one-shot maintenance
    /// binaries (e.g. ClickHouse parity check) that need to run raw SQL not
    /// covered by an existing helper. Production code should prefer the typed
    /// methods on `UltrosDb`.
    pub fn get_connection(&self) -> &DatabaseConnection {
        &self.db
    }

    /// Process-scoped identifier attached to `pg_notify` wake-up payloads.
    pub fn notify_origin(&self) -> &str {
        &self.notify_origin
    }

    #[instrument]
    pub async fn connect() -> Result<Self> {
        let _ = dotenv();
        let url = std::env::var("DATABASE_URL").expect("Missing DATABASE_URL environment variable");
        let mut opt = ConnectOptions::new(url.clone());
        let max_connections =
            env_u32("POSTGRES_MAX_CONNECTIONS").unwrap_or(DEFAULT_MAX_CONNECTIONS);
        let min_connections =
            min_connections_for(max_connections, env_u32("POSTGRES_MIN_CONNECTIONS"));
        info!(max_connections, min_connections, "sizing the database pool");
        opt.max_connections(max_connections)
            .min_connections(min_connections);
        let db: DatabaseConnection = Database::connect(opt).await?;
        let skip_migrations = env_flag_enabled(
            SKIP_MIGRATIONS_ENV,
            std::env::var(SKIP_MIGRATIONS_ENV).ok().as_deref(),
        );
        if skip_migrations {
            info!(
                variable = SKIP_MIGRATIONS_ENV,
                "skipping database migrations on startup"
            );
        } else if db.get_database_backend() == sea_orm::DbBackend::Postgres {
            // Dedicated non-reconnecting single-session pool ties Migrator::up
            // strictly to the Postgres backend holding MIGRATION_LOCK_KEY.
            let migration_db = connect_single_session_for_migrations(&url, None).await?;
            let result = run_migrations_locked(&migration_db).await;
            let _ = migration_db.close().await;
            result?;
        } else {
            Migrator::up(&db, None).await?;
        }

        Ok(Self::from_connection(db))
    }

    /// Wrap an already-open connection **without** running migrations.
    ///
    /// For one-shot tools and integration tests that read a database owned by
    /// something else — `connect()` refuses a database that has migrations this
    /// build doesn't know about (a shared dev Postgres that a newer branch has
    /// migrated), and a tool that only reads must not be blocked by that.
    pub fn from_connection(db: DatabaseConnection) -> Self {
        let pool_max = match db.get_database_backend() {
            sea_orm::DbBackend::Postgres => db
                .get_postgres_connection_pool()
                .options()
                .get_max_connections(),
            // SQLite fixtures and single-connection callers have no pool
            // headroom to reserve, but must retain their supported behavior.
            _ => 1,
        };
        Self {
            db,
            ingest_budget: ingest_budget::IngestBudget::new(pool_max),
            notify_origin: Arc::from(wakeup::generate_notify_origin()),
        }
    }

    #[instrument(skip(self))]
    pub async fn insert_default_retainer_cities(&self) -> Result<()> {
        struct RetainerCityData {
            id: i32,
            name: &'static str,
        }
        let cities = [
            RetainerCityData {
                id: 1,
                name: "Limsa Lominsa",
            },
            RetainerCityData {
                id: 2,
                name: "Gridania",
            },
            RetainerCityData {
                id: 3,
                name: "Ul'dah",
            },
            RetainerCityData {
                id: 4,
                name: "Ishguard",
            },
            RetainerCityData {
                id: 7,
                name: "Kugane",
            },
            RetainerCityData {
                id: 10,
                name: "Crystarium",
            },
            RetainerCityData {
                id: 12,
                name: "Old Sharlyan",
            },
            RetainerCityData {
                id: 14,
                name: "Tuliyollal",
            },
        ];
        // check if the database matches our coded data
        let db_cities = retainer_city::Entity::find().all(&self.db).await?;

        let cities_not_in_db: Vec<_> = cities
            .iter()
            .filter(|a| !db_cities.iter().any(|c| a.id.eq(&c.id)))
            .map(|m| retainer_city::ActiveModel {
                id: Set(m.id),
                name: Set(m.name.to_string()),
            })
            .collect();
        if !cities_not_in_db.is_empty() {
            let insert = retainer_city::Entity::insert_many(cities_not_in_db)
                .exec(&self.db)
                .await?;
            info!(
                // SeaORM 2.0: `last_insert_id` is `Option`, `None` when nothing was inserted.
                "Added retainer home cities. Last insert id: {:?}",
                insert.last_insert_id
            );
        }
        Ok(())
    }

    #[instrument(skip(self))]
    pub async fn search_retainers(&self, retainer_name: &str) -> Result<Vec<Retainer>> {
        // retainer names are forced to be lower except for the first character which will be uppercase.
        let retainer_name: String = retainer_name
            .chars()
            .enumerate()
            .map(|(i, byte)| {
                if i == 0 {
                    byte.to_ascii_uppercase()
                } else {
                    byte.to_ascii_lowercase()
                }
            })
            .collect();

        let val = retainer::Entity::find()
            .filter(
                retainer::Column::Name
                    .like(format!("{}%", group_roles::escape_like(&retainer_name))),
            )
            .limit(10)
            .all(&self.db)
            .await?;
        Ok(val.into_iter().map(Retainer::from).collect())
    }

    #[instrument(skip(self))]
    pub async fn get_retainer_listings(
        &self,
        retainer_id: i32,
    ) -> Result<(retainer::Model, Vec<active_listing::Model>)> {
        use retainer::*;
        let query = Entity::find()
            .filter(Column::Id.eq(retainer_id))
            .find_with_related(active_listing::Entity)
            .all(&self.db)
            .await?;
        query
            .into_iter()
            .next()
            .ok_or(DbErr::RecordNotFound(retainer_id.to_string()).into())
    }

    /// Looks up a world via it's world name. Requires exact match
    #[instrument(skip(self))]
    pub async fn get_world(&self, world_name: &str) -> Result<world::Model> {
        use world::*;
        let worlds = Entity::find()
            .filter(Column::Name.eq(world_name))
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("World not found"))?;
        Ok(worlds)
    }

    /// All worlds known to the database, in id order. Used by the ClickHouse
    /// backfill which iterates worlds × month chunks for resumability.
    #[instrument(skip(self))]
    pub async fn list_worlds(&self) -> Result<Vec<world::Model>> {
        use world::*;
        Ok(Entity::find()
            .order_by_asc(Column::Id)
            .all(&self.db)
            .await?)
    }

    #[instrument(skip(self))]
    pub async fn get_datacenter_from_world(
        &self,
        world: &world::Model,
    ) -> Result<datacenter::Model> {
        datacenter::Entity::find_by_id(world.datacenter_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("Datacenter not found"))
    }

    #[instrument(skip(self))]
    pub async fn get_region_from_datacenter(
        &self,
        datacenter: &datacenter::Model,
    ) -> Result<region::Model> {
        region::Entity::find_by_id(datacenter.region_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("Region not found"))
    }

    #[instrument(skip(self))]
    pub async fn get_listings_for_world_hq(
        &self,
        world: WorldId,
        item: ItemId,
        hq: bool,
    ) -> Result<Vec<active_listing::Model>> {
        use active_listing::*;
        let listings = Entity::find()
            .filter(Column::ItemId.eq(item.0))
            .filter(Column::WorldId.eq(world.0))
            .filter(Column::Hq.eq(hq))
            .all(&self.db)
            .await?;
        Ok(listings)
    }

    #[instrument(skip(self))]
    pub async fn create_alert(&self, owner: discord_user::Model) -> Result<alert::Model> {
        use alert::ActiveModel;
        Ok(ActiveModel {
            owner: Set(owner.id),
            ..Default::default()
        }
        .insert(&self.db)
        .await?)
    }

    #[instrument(skip(self))]
    pub async fn add_discord_notification_to_alert(
        &self,
        alert: &alert::Model,
        discord_channel: i64,
    ) -> Result<alert_discord_destination::Model> {
        use alert_discord_destination::ActiveModel;
        let model = ActiveModel {
            alert_id: Set(alert.id),
            channel_id: Set(discord_channel),
            ..Default::default()
        }
        .insert(&self.db)
        .await?;
        Ok(model)
    }

    #[instrument(skip(self))]
    pub async fn get_world_from_retainer(
        &self,
        retainer: &retainer::Model,
    ) -> Result<Option<world::Model>> {
        let world = retainer.find_related(world::Entity).one(&self.db).await?;

        Ok(world)
    }

    #[instrument(skip(self))]
    pub async fn store_retainer(
        &self,
        retainer_id: &str,
        retainer_name: &str,
        world_id: WorldId,
        retainer_city_id: i32,
    ) -> Result<retainer::Model> {
        use retainer::*;
        let active_model = retainer::ActiveModel {
            id: NotSet,
            world_id: Set(world_id.0),
            name: Set(retainer_name.to_string()),
            retainer_city_id: Set(retainer_city_id),
        };
        let model = Entity::insert(active_model)
            .on_conflict(
                sea_query::OnConflict::columns([Column::Name, Column::WorldId].into_iter())
                    // Unknown source metadata must not erase a known city,
                    // including when another ingestion inserts this identity
                    // between the resolver's lookup and this upsert.
                    .value(Column::RetainerCityId, sea_query::Expr::cust(
                        "CASE WHEN excluded.retainer_city_id > 0 THEN excluded.retainer_city_id ELSE retainer.retainer_city_id END",
                    ))
                    .to_owned(),
            )
            .exec_with_returning(&self.db)
            .await?;

        Ok(model)
    }

    fn listing_model(
        listing: &ListingView,
        item_id: ItemId,
        world_id: WorldId,
        retainer_id: i32,
    ) -> active_listing::ActiveModel {
        let price_per_unit = listing.price_per_unit.unwrap_or(listing.total) as i32;
        let quantity = listing.quantity.unwrap_or(1) as i32;
        let materia = (!listing.materia.is_empty()).then(|| {
            active_listing::MateriaList(
                listing
                    .materia
                    .iter()
                    .map(|m| active_listing::Materia {
                        slot_id: m.slot_id.map(|s| s as i32),
                        materia_id: m.materia_id as i32,
                    })
                    .collect(),
            )
        });
        active_listing::ActiveModel {
            world_id: Set(world_id.0),
            item_id: Set(item_id.0),
            retainer_id: Set(retainer_id),
            price_per_unit: Set(price_per_unit),
            quantity: Set(quantity),
            hq: Set(listing.hq),
            timestamp: Set(listing.last_review_time.naive_utc()),
            listing_id: Set(listing.listing_id.clone()),
            materia: Set(materia),
            stain_id: Set(listing.stain_id.map(|s| s as i32)),
            creator_name: Set(listing.creator_name.clone().filter(|n| !n.is_empty())),
            is_crafted: Set(listing.is_crafted),
            on_mannequin: Set(listing.on_mannequin),
            ..Default::default()
        }
    }

    #[instrument(skip(self, names))]
    async fn get_retainer_ids_from_name(
        &self,
        names: impl Iterator<Item = &str>,
        world_id: i32,
    ) -> Result<Vec<retainer::Model>> {
        use retainer::*;
        let names: Vec<&str> = names.collect();
        if names.is_empty() {
            return Ok(vec![]);
        }
        let retainers = Entity::find()
            .filter(Column::Name.is_in(names))
            .filter(Column::WorldId.eq(world_id))
            .all(&self.db)
            .await?;
        Ok(retainers)
    }

    /// Stores a region. This generally assumes the regions haven't changed and really is just querying for region IDs
    #[instrument(skip(self))]
    pub async fn store_region(&self, region_name: &str) -> Result<region::Model> {
        if let Some(value) = region::Entity::find()
            .filter(region::Column::Name.eq(region_name))
            .one(&self.db)
            .await?
        {
            return Ok(value);
        }
        info!("Inserting region {region_name}");
        Ok(region::ActiveModel {
            name: Set(region_name.to_string()),
            ..Default::default()
        }
        .insert(&self.db)
        .await?)
    }

    /// Stores a datacenter. Similarly to the region, this will mostly just update.
    /// It will try to update the datacenter if the region somehow changed. (unlikely)
    #[instrument(skip(self))]
    pub async fn store_datacenter(
        &self,
        datacenter_name: &str,
        region_name: &str,
    ) -> Result<datacenter::Model> {
        let region = region::Entity::find()
            .filter(region::Column::Name.eq(region_name))
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("Region not found"))?;
        if let Some(dc) = datacenter::Entity::find()
            .filter(datacenter::Column::Name.eq(datacenter_name))
            .one(&self.db)
            .await?
        {
            // check if the region has changed
            if dc.region_id != region.id {
                // update the new region
                let mut active_dc = dc.into_active_model();
                active_dc.region_id = Set(region.id);
                return Ok(active_dc.update(&self.db).await?);
            }
            return Ok(dc);
        }

        let dc = datacenter::ActiveModel {
            name: Set(datacenter_name.to_string()),
            region_id: Set(region.id),
            ..Default::default()
        }
        .insert(&self.db)
        .await?;
        Ok(dc)
    }

    /// Stores a world. Similar to the region/datacenter, but the final step.
    #[instrument(skip(self))]
    pub async fn store_world(
        &self,
        world_id: WorldId,
        world_name: &str,
        datacenter_name: &str,
    ) -> Result<world::Model> {
        let datacenter = datacenter::Entity::find()
            .filter(datacenter::Column::Name.eq(datacenter_name))
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("Datacenter required for world insertion"))?;
        if let Some(world) = world::Entity::find()
            .filter(world::Column::Name.eq(world_name))
            .one(&self.db)
            .await?
        {
            if world.datacenter_id != datacenter.id {
                info!("updating {world_name} datacenter to {datacenter_name}");
                let mut active_world = world.into_active_model();
                active_world.datacenter_id = Set(datacenter.id);
                return Ok(active_world.update(&self.db).await?);
            }
            return Ok(world);
        }

        info!("Inserting world {world_name}");
        Ok(world::ActiveModel {
            id: Set(world_id.0),
            name: Set(world_name.to_string()),
            datacenter_id: Set(datacenter.id),
        }
        .insert(&self.db)
        .await?)
    }

    #[instrument(skip(self))]
    pub async fn get_unique_item_ids(&self) -> Result<Vec<i32>> {
        let items = active_listing::Entity::find()
            .select_only()
            .column(active_listing::Column::ItemId)
            .distinct()
            .into_model::<UniqueItemId>()
            .all(&self.db)
            .await?;

        Ok(items.into_iter().map(|i| i.item).collect())
    }
}
#[derive(Debug, FromQueryResult)]
pub struct UniqueItemId {
    pub item: i32,
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_MIN_CONNECTIONS, SKIP_MIGRATIONS_ENV, connect_single_session_for_migrations,
        env_flag_enabled, min_connections_for, run_migrations_locked,
    };

    #[test]
    fn skip_migrations_flag_matches_boolean_env_contract() {
        for off in [
            None,
            Some(""),
            Some("  "),
            Some("0"),
            Some("false"),
            Some("FALSE"),
            Some("no"),
            Some("off"),
        ] {
            assert!(!env_flag_enabled(SKIP_MIGRATIONS_ENV, off), "{off:?}");
        }
        for on in ["1", "true", "TRUE", "yes", "on", " true ", "anything-else"] {
            assert!(env_flag_enabled(SKIP_MIGRATIONS_ENV, Some(on)), "{on:?}");
        }
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn concurrent_locked_migrations_serialize_without_deadlock_on_single_conn_pool() {
        use sea_orm::{ConnectOptions, ConnectionTrait, Database};

        let url = std::env::var("MIGRATION_TEST_DATABASE_URL").unwrap();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let schema = format!("locked_migrations_{}_{nonce}", std::process::id());

        let mut setup_opt = ConnectOptions::new(url.clone());
        setup_opt
            .max_connections(1)
            .min_connections(1)
            .sqlx_logging(false);
        let setup_db = Database::connect(setup_opt).await.unwrap();
        setup_db
            .execute_unprepared(&format!("CREATE SCHEMA {schema}"))
            .await
            .unwrap();

        let db1 = connect_single_session_for_migrations(&url, Some(&schema))
            .await
            .unwrap();
        let db2 = connect_single_session_for_migrations(&url, Some(&schema))
            .await
            .unwrap();

        let (res1, res2) = tokio::join!(run_migrations_locked(&db1), run_migrations_locked(&db2));
        res1.unwrap();
        res2.unwrap();

        // Verify that if the pinned migration session is terminated, the pool
        // refuses to silently open an unlocked replacement connection.
        use sea_orm::{DbBackend, Statement};
        let pid_row = db1
            .query_one_raw(Statement::from_string(
                DbBackend::Postgres,
                "SELECT pg_backend_pid() AS pid",
            ))
            .await
            .unwrap()
            .unwrap();
        let pid: i32 = pid_row.try_get("", "pid").unwrap();
        setup_db
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "SELECT pg_terminate_backend($1)",
                [pid.into()],
            ))
            .await
            .unwrap();
        let after_terminate = db1
            .query_one_raw(Statement::from_string(DbBackend::Postgres, "SELECT 1"))
            .await;
        assert!(
            after_terminate.is_err(),
            "single-session migration pool must reject replacement connections after disconnect"
        );

        let _ = db1.close().await;
        let _ = db2.close().await;
        setup_db
            .execute_unprepared(&format!("DROP SCHEMA {schema} CASCADE"))
            .await
            .unwrap();
    }

    #[test]
    fn a_roomy_ceiling_keeps_the_usual_floor() {
        assert_eq!(min_connections_for(300, None), DEFAULT_MIN_CONNECTIONS);
        assert_eq!(min_connections_for(50, None), DEFAULT_MIN_CONNECTIONS);
        assert_eq!(min_connections_for(10, None), DEFAULT_MIN_CONNECTIONS);
    }

    #[test]
    fn a_small_ceiling_lowers_the_floor_to_match() {
        // THE BUG: the floor was a hard-coded 10, so `POSTGRES_MAX_CONNECTIONS=2`
        // eagerly opened five times the pool it asked for. Several instances
        // sharing one Postgres is precisely when someone sets it that low.
        assert_eq!(min_connections_for(2, None), 2);
        assert_eq!(min_connections_for(1, None), 1);
    }

    #[test]
    fn an_explicit_floor_wins_under_a_roomy_ceiling() {
        // Clamping alone does not cover this: a ceiling of 15 leaves the default
        // floor of 10 untouched, and the five connections of headroom left over
        // were not enough for `Migrator::up` to start a server. Being able to
        // say "hold two" is what makes the ceiling usable at that size.
        assert_eq!(min_connections_for(15, Some(2)), 2);
        assert_eq!(min_connections_for(300, Some(0)), 0);
    }

    #[test]
    fn an_explicit_floor_is_still_clamped_to_the_ceiling() {
        // A floor above the ceiling is not a pool, it is a deadlock waiting to
        // happen, so the ceiling wins regardless of what was asked for.
        assert_eq!(min_connections_for(5, Some(50)), 5);
    }
}
