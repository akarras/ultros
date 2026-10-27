//! Compact, transactionally maintained analyzer history. Historical seeding is
//! explicit maintenance, never part of a schema migration or normal startup.

use crate::{UltrosDb, entity::sale_history};
use anyhow::{Result, ensure};
use chrono::NaiveDateTime;
use sea_orm::{
    AccessMode, ConnectionTrait, DatabaseTransaction, DbBackend, DbErr, FromQueryResult,
    IsolationLevel, Statement, TransactionTrait,
};
use serde::Serialize;
use std::time::Duration;
use universalis::{ItemId, WorldId};

pub const RECENT_SALE_LIMIT: usize = 6;
const COLUMNS: &str = "sold_item_id,hq,price_per_item,sold_date,world_id";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, FromQueryResult)]
pub struct BackfillStatus {
    pub cursor_item: Option<i32>,
    pub cursor_world: Option<i32>,
    pub complete: bool,
    pub retention: i32,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum BackfillStep {
    Advanced { item_id: i32, world_id: i32 },
    Complete,
    Raced,
}

#[derive(Clone, Copy, Debug, FromQueryResult)]
struct Board {
    sold_item_id: i32,
    world_id: i32,
}

#[derive(Clone, Debug, FromQueryResult)]
struct RecentSale {
    pg_id: i32,
    sold_item_id: i32,
    hq: bool,
    price_per_item: i32,
    sold_date: NaiveDateTime,
    world_id: i32,
}

impl From<&sale_history::Model> for RecentSale {
    fn from(sale: &sale_history::Model) -> Self {
        Self {
            pg_id: sale.id,
            sold_item_id: sale.sold_item_id,
            hq: sale.hq,
            price_per_item: sale.price_per_item,
            sold_date: sale.sold_date,
            world_id: sale.world_id,
        }
    }
}

struct Staged {
    status: BackfillStatus,
    board: Option<Board>,
    sales: Vec<RecentSale>,
}

async fn status_on(db: &impl ConnectionTrait) -> Result<BackfillStatus, DbErr> {
    BackfillStatus::find_by_statement(Statement::from_string(DbBackend::Postgres,
        "SELECT cursor_item,cursor_world,complete,retention FROM recent_sales_backfill_state WHERE id=1"))
        .one(db).await?.ok_or_else(||DbErr::Custom("recent-sales backfill state is missing".into()))
}

async fn set_timeouts(tx: &DatabaseTransaction, timeout: Duration) -> Result<()> {
    let millis = timeout.as_millis();
    ensure!(
        (1..=300_000).contains(&millis),
        "statement timeout must be 1..=300000 ms"
    );
    tx.execute_raw(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "SELECT set_config('statement_timeout',$1,true),set_config('lock_timeout',$1,true)",
        [format!("{millis}ms").into()],
    ))
    .await?;
    Ok(())
}

/// The caller already holds this board's writer lock. Insert and trim in the
/// same transaction as the source sales and ingest marker.
pub(crate) async fn project_sales(
    tx: &DatabaseTransaction,
    sales: &[sale_history::Model],
    world: WorldId,
    item: ItemId,
) -> Result<()> {
    let sales = sales.iter().map(RecentSale::from).collect::<Vec<_>>();
    merge_on(
        tx,
        Board {
            world_id: world.0,
            sold_item_id: item.0,
        },
        &sales,
    )
    .await
}

async fn merge_on(tx: &DatabaseTransaction, board: Board, sales: &[RecentSale]) -> Result<()> {
    ensure!(
        sales
            .iter()
            .all(|sale| sale.world_id == board.world_id && sale.sold_item_id == board.sold_item_id),
        "recent sale belongs to another board"
    );
    for chunk in sales.chunks(500) {
        let mut values = Vec::with_capacity(chunk.len() * 6);
        let tuples = chunk
            .iter()
            .enumerate()
            .map(|(i, sale)| {
                values.extend([
                    sale.world_id.into(),
                    sale.sold_item_id.into(),
                    sale.hq.into(),
                    sale.pg_id.into(),
                    sale.price_per_item.into(),
                    sale.sold_date.into(),
                ]);
                let at = i * 6;
                format!(
                    "(${},${},${},${},${},${})",
                    at + 1,
                    at + 2,
                    at + 3,
                    at + 4,
                    at + 5,
                    at + 6
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        tx.execute_raw(Statement::from_sql_and_values(DbBackend::Postgres,
            format!("INSERT INTO recent_sale_history(world_id,sold_item_id,hq,pg_id,price_per_item,sold_date) VALUES {tuples} ON CONFLICT(world_id,sold_item_id,hq,pg_id) DO NOTHING"),values)).await?;
    }
    // Only the compact board (at most 12 old rows plus this batch) is ranked.
    tx.execute_raw(Statement::from_sql_and_values(DbBackend::Postgres,r#"
        DELETE FROM recent_sale_history p USING (
            SELECT pg_id,hq,ROW_NUMBER() OVER(PARTITION BY hq ORDER BY sold_date DESC,pg_id DESC) AS position
            FROM recent_sale_history WHERE world_id=$1 AND sold_item_id=$2
        ) expired WHERE p.world_id=$1 AND p.sold_item_id=$2 AND p.hq=expired.hq
          AND p.pg_id=expired.pg_id AND expired.position>$3
    "#,[board.world_id.into(),board.sold_item_id.into(),(RECENT_SALE_LIMIT as i32).into()])).await?;
    Ok(())
}

impl UltrosDb {
    pub async fn recent_sales_backfill_status(&self) -> Result<BackfillStatus, DbErr> {
        status_on(&self.db).await
    }

    pub(crate) async fn recent_sales_statement(&self, count: i32) -> Result<Statement, DbErr> {
        if count <= 0 {
            return Ok(Statement::from_string(
                DbBackend::Postgres,
                format!("SELECT {COLUMNS} FROM recent_sale_history WHERE FALSE"),
            ));
        }
        let status = self.recent_sales_backfill_status().await?;
        if status.complete
            && status.retention == RECENT_SALE_LIMIT as i32
            && count <= status.retention
        {
            if count == status.retention {
                return Ok(Statement::from_string(
                    DbBackend::Postgres,
                    format!("SELECT {COLUMNS} FROM recent_sale_history"),
                ));
            }
            return Ok(Statement::from_sql_and_values(
                DbBackend::Postgres,
                format!(
                    r#"
                SELECT {COLUMNS} FROM (SELECT p.*,ROW_NUMBER() OVER(
                    PARTITION BY sold_item_id,hq,world_id ORDER BY sold_date DESC,pg_id DESC
                ) AS position FROM recent_sale_history p) recent WHERE position<=$1
            "#
                ),
                [count.into()],
            ));
        }
        tracing::warn!(
            count,
            complete = status.complete,
            retention = status.retention,
            "recent-sales projection unavailable for this limit; using legacy history scan (run recent_sales_backfill after deploying all sale writers)"
        );
        Ok(Statement::from_sql_and_values(
            DbBackend::Postgres,
            format!(
                r#"
            SELECT {COLUMNS} FROM (SELECT h.*,RANK() OVER(
                PARTITION BY sold_item_id,hq,world_id ORDER BY sold_date DESC
            ) AS position FROM sale_history h) history WHERE position<=$1
        "#
            ),
            [count.into()],
        ))
    }

    /// One bounded, resumable board attempt. A losing concurrent operator
    /// rolls back its merge; neither live writes nor another cursor are lost.
    pub async fn backfill_recent_sales_step(&self, timeout: Duration) -> Result<BackfillStep> {
        let _budget = self.ingest_budget.acquire().await?;
        let staged = self.stage_recent_sales(timeout).await?;
        self.commit_recent_sales(staged, timeout).await
    }

    async fn stage_recent_sales(&self, timeout: Duration) -> Result<Staged> {
        let tx = self
            .db
            .begin_with_config(
                Some(IsolationLevel::RepeatableRead),
                Some(AccessMode::ReadOnly),
            )
            .await?;
        set_timeouts(&tx, timeout).await?;
        let status = status_on(&tx).await?;
        ensure!(
            status.retention == RECENT_SALE_LIMIT as i32,
            "backfill retention differs from this build; migrate the projection before continuing"
        );
        let board = if status.complete {
            None
        } else {
            let statement = match (status.cursor_item, status.cursor_world) {
                (None, None) => Statement::from_string(
                    DbBackend::Postgres,
                    "SELECT sold_item_id,world_id FROM sale_history ORDER BY sold_item_id,world_id LIMIT 1",
                ),
                (Some(item), Some(world)) => Statement::from_sql_and_values(
                    DbBackend::Postgres,
                    "SELECT sold_item_id,world_id FROM sale_history WHERE (sold_item_id,world_id)>($1,$2) ORDER BY sold_item_id,world_id LIMIT 1",
                    [item.into(), world.into()],
                ),
                _ => anyhow::bail!("invalid recent-sales cursor"),
            };
            Board::find_by_statement(statement).one(&tx).await?
        };
        let mut sales = Vec::new();
        if let Some(board) = board {
            // Seek the latest six for each quality using the existing board/date
            // index. No date cutoff excludes old or quiet markets.
            for hq in [false, true] {
                sales.extend(RecentSale::find_by_statement(Statement::from_sql_and_values(DbBackend::Postgres,
                    "SELECT id AS pg_id,sold_item_id,hq,price_per_item,sold_date,world_id FROM sale_history WHERE sold_item_id=$1 AND world_id=$2 AND hq=$3 ORDER BY sold_date DESC,id DESC LIMIT $4",
                    [board.sold_item_id.into(),board.world_id.into(),hq.into(),(RECENT_SALE_LIMIT as i32).into()]
                )).all(&tx).await?);
            }
        }
        tx.commit().await?;
        Ok(Staged {
            status,
            board,
            sales,
        })
    }

    async fn commit_recent_sales(&self, staged: Staged, timeout: Duration) -> Result<BackfillStep> {
        let Staged {
            status,
            board,
            sales,
        } = staged;
        if status.complete {
            return Ok(BackfillStep::Complete);
        }
        // The read transaction is closed before this connection is acquired,
        // allowing maintenance to run even with a one-connection pool.
        let tx = self.db.begin().await?;
        set_timeouts(&tx, timeout).await?;
        if let Some(board) = board {
            self.lock_listing_board_on(
                &tx,
                WorldId(board.world_id),
                ItemId(board.sold_item_id),
                None,
            )
            .await?;
            merge_on(&tx, board, &sales).await?;
        }
        let (item, world) = board
            .map(|b| (Some(b.sold_item_id), Some(b.world_id)))
            .unwrap_or((status.cursor_item, status.cursor_world));
        let changed=tx.execute_raw(Statement::from_sql_and_values(DbBackend::Postgres,r#"
            UPDATE recent_sales_backfill_state SET cursor_item=$1,cursor_world=$2,complete=$3
            WHERE id=1 AND cursor_item IS NOT DISTINCT FROM $4 AND cursor_world IS NOT DISTINCT FROM $5
              AND complete=FALSE AND retention=$6
        "#,[item.into(),world.into(),board.is_none().into(),status.cursor_item.into(),status.cursor_world.into(),status.retention.into()])).await?;
        if changed.rows_affected() != 1 {
            tx.rollback().await?;
            return Ok(BackfillStep::Raced);
        }
        tx.commit().await?;
        Ok(board
            .map(|board| BackfillStep::Advanced {
                item_id: board.sold_item_id,
                world_id: board.world_id,
            })
            .unwrap_or(BackfillStep::Complete))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{common::test_db, entity::unknown_final_fantasy_character};
    use futures::TryStreamExt;
    use sea_orm::{ActiveModelTrait, ActiveValue::Set, ConnectOptions, Database, EntityTrait};
    use universalis::SaleView;

    fn incoming(price: i32, second: i64) -> SaleView {
        SaleView {
            hq: false,
            price_per_unit: price,
            quantity: 1,
            timestamp: chrono::DateTime::from_timestamp(second, 0).unwrap().into(),
            on_mannequin: false,
            world_name: None,
            world_id: None,
            buyer_name: "Projection live buyer".into(),
            total: price,
        }
    }

    async fn projected(db: &UltrosDb, item: i32) -> Vec<RecentSale> {
        RecentSale::find_by_statement(Statement::from_sql_and_values(DbBackend::Postgres,
            "SELECT * FROM recent_sale_history WHERE world_id=30001 AND sold_item_id=$1 ORDER BY hq,sold_date DESC,pg_id DESC",[item.into()]))
            .all(&db.db).await.unwrap()
    }

    #[tokio::test]
    #[ignore = "requires an explicitly opted-in disposable PostgreSQL database"]
    async fn projection_backfill_resume_live_merge_cas_and_rollback_postgres() {
        let db = test_db::connect().await;
        test_db::seed_market(&db).await;
        // This regression runs against its own opted-in database. Historical
        // inserts deliberately bypass the new writer, simulating pre-deploy data.
        db.db.execute_unprepared("DELETE FROM recent_sale_history;
            DELETE FROM sale_history WHERE world_id=30001;
            UPDATE recent_sales_backfill_state SET cursor_item=NULL,cursor_world=NULL,complete=FALSE;").await.unwrap();
        let buyer = unknown_final_fantasy_character::ActiveModel {
            name: Set(format!(
                "Projection historical {}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )),
            ..Default::default()
        }
        .insert(&db.db)
        .await
        .unwrap();
        let old = chrono::NaiveDate::from_ymd_opt(1999, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        let historical =
            sale_history::Entity::insert_many([false, true].into_iter().flat_map(|hq| {
                (0..10).map(move |price| sale_history::ActiveModel {
                    sold_item_id: Set(42),
                    world_id: Set(30001),
                    hq: Set(hq),
                    price_per_item: Set(100 + price),
                    quantity: Set(1),
                    buying_character_id: Set(buyer.id),
                    sold_date: Set(old),
                    ..Default::default()
                })
            }))
            .exec_with_returning(&db.db)
            .await
            .unwrap();
        sale_history::Entity::insert_many((0..2).map(|price| sale_history::ActiveModel {
            sold_item_id: Set(43),
            world_id: Set(30001),
            hq: Set(false),
            price_per_item: Set(500 + price),
            quantity: Set(1),
            buying_character_id: Set(buyer.id),
            sold_date: Set(old),
            ..Default::default()
        }))
        .exec(&db.db)
        .await
        .unwrap();
        assert!(projected(&db, 42).await.is_empty());
        assert!(
            db.recent_sales_statement(6)
                .await
                .unwrap()
                .sql
                .contains("FROM sale_history h")
        );
        assert!(!db.recent_sales_backfill_status().await.unwrap().complete);
        let timeout = Duration::from_secs(5);
        let first = db.stage_recent_sales(timeout).await.unwrap();
        let stale = db.stage_recent_sales(timeout).await.unwrap();
        assert_eq!(first.board.unwrap().sold_item_id, 42);
        let live = db
            .update_sales(
                vec![incoming(900, 1_700_000_000)],
                ItemId(42),
                WorldId(30001),
            )
            .await
            .unwrap()[0]
            .0
            .clone();
        assert_eq!(projected(&db, 42).await.len(), 1);
        assert_eq!(
            db.commit_recent_sales(first, timeout).await.unwrap(),
            BackfillStep::Advanced {
                item_id: 42,
                world_id: 30001
            }
        );
        let rows = projected(&db, 42).await;
        assert_eq!(rows.len(), 12);
        assert_eq!(rows.iter().filter(|row| !row.hq).count(), 6);
        assert_eq!(rows.iter().filter(|row| row.hq).count(), 6);
        assert!(rows.iter().any(|row| row.pg_id == live.id));
        // Exactly six samples even for a timestamp tie, deterministically by ID.
        let mut expected: Vec<_> = historical
            .iter()
            .filter(|row| row.hq)
            .map(|row| row.id)
            .collect();
        expected.sort_unstable_by_key(|id| std::cmp::Reverse(*id));
        expected.truncate(RECENT_SALE_LIMIT);
        assert_eq!(
            rows.iter()
                .filter(|row| row.hq)
                .map(|row| row.pg_id)
                .collect::<Vec<_>>(),
            expected
        );
        let ids: Vec<_> = rows.iter().map(|row| row.pg_id).collect();
        assert_eq!(
            db.commit_recent_sales(stale, timeout).await.unwrap(),
            BackfillStep::Raced
        );
        assert_eq!(
            projected(&db, 42)
                .await
                .iter()
                .map(|row| row.pg_id)
                .collect::<Vec<_>>(),
            ids
        );
        assert_eq!(
            db.recent_sales_backfill_status().await.unwrap().cursor_item,
            Some(42)
        );
        // A new process resumes from committed progress, retaining quiet history.
        let resumed = UltrosDb::from_connection(db.db.clone());
        assert_eq!(
            resumed.backfill_recent_sales_step(timeout).await.unwrap(),
            BackfillStep::Advanced {
                item_id: 43,
                world_id: 30001
            }
        );
        assert!(
            !resumed
                .recent_sales_backfill_status()
                .await
                .unwrap()
                .complete
        );
        assert_eq!(
            resumed.backfill_recent_sales_step(timeout).await.unwrap(),
            BackfillStep::Complete
        );
        assert_eq!(
            resumed.backfill_recent_sales_step(timeout).await.unwrap(),
            BackfillStep::Complete
        );
        let statement = db.recent_sales_statement(6).await.unwrap();
        assert!(!statement.sql.contains("FROM sale_history "));
        let plan = db
            .db
            .query_one_raw(Statement::from_string(
                DbBackend::Postgres,
                format!("EXPLAIN (FORMAT JSON) {}", statement.sql),
            ))
            .await
            .unwrap()
            .unwrap();
        let plan: serde_json::Value = plan.try_get("", "QUERY PLAN").unwrap();
        let encoded = plan.to_string();
        assert!(encoded.contains("recent_sale_history"));
        assert!(!encoded.contains("\"Relation Name\":\"sale_history\""));
        assert!(!encoded.contains("WindowAgg"));
        println!("Ready projection plan: {plan}");
        let loaded = db
            .last_n_sales(6)
            .await
            .unwrap()
            .try_collect::<Vec<_>>()
            .await
            .unwrap();
        assert_eq!(loaded.len(), 14);
        assert!(
            loaded
                .iter()
                .any(|sale| sale.sold_item_id == 43 && sale.sold_date == old)
        );
        assert_eq!(
            db.last_n_sales(3)
                .await
                .unwrap()
                .try_collect::<Vec<_>>()
                .await
                .unwrap()
                .len(),
            8
        );
        assert!(
            db.last_n_sales(0)
                .await
                .unwrap()
                .try_collect::<Vec<_>>()
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            db.recent_sales_statement(7)
                .await
                .unwrap()
                .sql
                .contains("FROM sale_history h")
        );
        // New boards behind the maintenance cursor are covered by live writers.
        let new = incoming(700, 1_700_000_100);
        db.update_sales(vec![new.clone()], ItemId(40), WorldId(30001))
            .await
            .unwrap();
        assert!(
            db.update_sales(vec![new], ItemId(40), WorldId(30001))
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(projected(&db, 40).await.len(), 1);
        assert!(
            db.last_n_sales(6)
                .await
                .unwrap()
                .try_collect::<Vec<_>>()
                .await
                .unwrap()
                .iter()
                .any(|sale| sale.sold_item_id == 40)
        );

        // A marker failure rolls back both source sales and the derived rows.
        let before = projected(&db, 43)
            .await
            .iter()
            .map(|row| (row.pg_id, row.price_per_item))
            .collect::<Vec<_>>();
        db.db.execute_unprepared("CREATE OR REPLACE FUNCTION ultros_audit_projection_fail() RETURNS trigger
            LANGUAGE plpgsql AS $fixture$ BEGIN RAISE EXCEPTION 'projection fixture failure'; END; $fixture$;
            DROP TRIGGER IF EXISTS ultros_audit_projection_fail ON listing_last_updated;
            CREATE TRIGGER ultros_audit_projection_fail BEFORE INSERT OR UPDATE ON listing_last_updated
            FOR EACH ROW WHEN (NEW.world_id=30001 AND NEW.item_id=43) EXECUTE FUNCTION ultros_audit_projection_fail()"
        ).await.unwrap();
        let failed = db
            .update_sales(
                vec![incoming(999999, 1_700_000_200)],
                ItemId(43),
                WorldId(30001),
            )
            .await;
        db.db.execute_unprepared("DROP TRIGGER ultros_audit_projection_fail ON listing_last_updated; DROP FUNCTION ultros_audit_projection_fail()").await.unwrap();
        assert!(failed.is_err());
        assert_eq!(
            projected(&db, 43)
                .await
                .iter()
                .map(|row| (row.pg_id, row.price_per_item))
                .collect::<Vec<_>>(),
            before
        );
        assert_eq!(
            db.get_sale_history_for_item(30001, 43, 100)
                .await
                .unwrap()
                .len(),
            2
        );
        assert!(
            db.get_listing_last_updated_for_worlds(ItemId(43), &[30001])
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            db.update_sales(
                vec![incoming(999999, 1_700_000_200)],
                ItemId(43),
                WorldId(30001)
            )
            .await
            .unwrap()
            .len(),
            1
        );

        // Deadlines apply before taking a contested board lock; failure cannot
        // advance the cursor. Resuming with a one-connection pool does not
        // require a second connection while holding the read transaction.
        db.db.execute_unprepared("UPDATE recent_sales_backfill_state SET cursor_item=NULL,cursor_world=NULL,complete=FALSE").await.unwrap();
        let held = db
            .lock_listing_board(WorldId(30001), ItemId(40), None)
            .await
            .unwrap();
        let bounded = tokio::time::timeout(
            Duration::from_secs(2),
            db.backfill_recent_sales_step(Duration::from_millis(50)),
        )
        .await;
        held.rollback().await.unwrap();
        assert!(bounded.unwrap().is_err());
        assert_eq!(
            db.recent_sales_backfill_status().await.unwrap().cursor_item,
            None
        );
        let mut options =
            ConnectOptions::new(std::env::var("ULTROS_PLUGIN_TEST_DATABASE_URL").unwrap());
        options
            .max_connections(1)
            .min_connections(0)
            .sqlx_logging(false);
        let single = UltrosDb::from_connection(Database::connect(options).await.unwrap());
        assert_eq!(
            tokio::time::timeout(
                Duration::from_secs(2),
                single.backfill_recent_sales_step(timeout)
            )
            .await
            .unwrap()
            .unwrap(),
            BackfillStep::Advanced {
                item_id: 40,
                world_id: 30001
            }
        );
        for _ in 0..4 {
            if single.backfill_recent_sales_step(timeout).await.unwrap() == BackfillStep::Complete {
                break;
            }
        }
        assert!(
            single
                .recent_sales_backfill_status()
                .await
                .unwrap()
                .complete
        );
        assert!(
            single
                .backfill_recent_sales_step(Duration::ZERO)
                .await
                .is_err()
        );
    }
}
