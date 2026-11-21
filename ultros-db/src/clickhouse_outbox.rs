//! Transactional Postgres outbox for durable ClickHouse `sales` projection.
//!
//! [`enqueue_sales`] runs inside the same transaction as `sale_history`
//! inserts (`UltrosDb::update_sales`). A background drainer fetches committed
//! non-quarantined batches in `id ASC` order, writes them to ClickHouse's
//! `ReplacingMergeTree` `sales` table, and deletes acknowledged outbox IDs
//! only after ClickHouse confirms the insert.
//!
//! Because acknowledgement deletes exact row IDs rather than advancing a
//! high-water mark, transactions that commit out of `id` order — and wrapped
//! negative `sale_history.id` (`pg_id`) values — can never be skipped. Rows
//! whose payload cannot be represented in ClickHouse (such as pre-1970
//! timestamps) are durably marked with `quarantined_reason` so they stay
//! operator-visible without stalling valid sales behind them.

use anyhow::Result;
use chrono::{DateTime, NaiveDateTime, Utc};
use sea_orm::{ConnectionTrait, DatabaseTransaction, DbBackend, FromQueryResult, Statement, Value};
use ultros_api_types::{SaleHistory, UnknownCharacter};

use crate::UltrosDb;

/// Maximum rows inserted or deleted per SQL statement. Keeps bind parameter
/// counts well below Postgres's 65,535 limit (9 params/row × 500 = 4,500).
const SQL_CHUNK_SIZE: usize = 500;

/// One committed `sale_history` row queued for ClickHouse `sales` projection.
#[derive(Clone, Debug, PartialEq, Eq, FromQueryResult)]
pub struct SaleOutboxRow {
    pub id: i64,
    pub pg_id: i32,
    pub sold_date: NaiveDateTime,
    pub item_id: i32,
    pub hq: bool,
    pub world_id: i32,
    pub price_per_item: i32,
    pub quantity: i32,
    pub buying_character_id: i32,
    pub buyer_name: String,
    pub created_at: DateTime<Utc>,
}

/// Cheap O(1) outbox backlog snapshot computed from the partial pending index.
#[derive(Clone, Copy, Debug, Default, PartialEq, FromQueryResult)]
pub struct SaleOutboxMetrics {
    /// Approximate pending rows (`max(id) - min(id) + 1`, or `0` when empty).
    /// Avoids full-table `COUNT(*)` scans during multi-hour ClickHouse outages.
    pub approx_pending_rows: i64,
    /// Age in seconds of the oldest non-quarantined row (`0.0` when empty).
    pub oldest_age_seconds: f64,
}

/// Insert newly recorded sales into `clickhouse_sale_outbox` inside the
/// caller's committing transaction.
pub(crate) async fn enqueue_sales(
    tx: &DatabaseTransaction,
    sales: &[(SaleHistory, UnknownCharacter)],
) -> Result<()> {
    if sales.is_empty() {
        return Ok(());
    }
    for chunk in sales.chunks(SQL_CHUNK_SIZE) {
        let (sql, values) = build_enqueue_sql(chunk);
        tx.execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            sql,
            values,
        ))
        .await?;
    }
    Ok(())
}

fn build_enqueue_sql(chunk: &[(SaleHistory, UnknownCharacter)]) -> (String, Vec<Value>) {
    let mut values = Vec::with_capacity(chunk.len() * 9);
    let tuples = chunk
        .iter()
        .enumerate()
        .map(|(i, (sale, buyer))| {
            let buyer_name = sale
                .buyer_name
                .as_deref()
                .unwrap_or(buyer.name.as_str())
                .to_string();
            values.extend([
                sale.id.into(),
                sale.sold_date.into(),
                sale.sold_item_id.into(),
                sale.hq.into(),
                sale.world_id.into(),
                sale.price_per_item.into(),
                sale.quantity.into(),
                sale.buying_character_id.into(),
                buyer_name.into(),
            ]);
            let at = i * 9;
            format!(
                "(${},${},${},${},${},${},${},${},${})",
                at + 1,
                at + 2,
                at + 3,
                at + 4,
                at + 5,
                at + 6,
                at + 7,
                at + 8,
                at + 9
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "INSERT INTO clickhouse_sale_outbox \
         (pg_id,sold_date,item_id,hq,world_id,price_per_item,quantity,buying_character_id,buyer_name) \
         VALUES {tuples}"
    );
    (sql, values)
}

fn build_ack_sql(chunk: &[i64]) -> (String, Vec<Value>) {
    let placeholders = (1..=chunk.len())
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(",");
    let values = chunk.iter().copied().map(Value::from).collect();
    let sql = format!("DELETE FROM clickhouse_sale_outbox WHERE id IN ({placeholders})");
    (sql, values)
}

impl UltrosDb {
    /// Fetch up to `limit` oldest committed non-quarantined rows from
    /// `clickhouse_sale_outbox`.
    ///
    /// Runs outside any long-lived transaction so ClickHouse HTTP round-trips
    /// never hold Postgres connections or locks open.
    pub async fn fetch_sale_outbox_batch(&self, limit: i64) -> Result<Vec<SaleOutboxRow>> {
        if limit <= 0 {
            return Ok(Vec::new());
        }
        let rows = SaleOutboxRow::find_by_statement(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT id, pg_id, sold_date, item_id, hq, world_id, price_per_item, \
                    quantity, buying_character_id, buyer_name, created_at \
             FROM clickhouse_sale_outbox \
             WHERE quarantined_reason IS NULL \
             ORDER BY id ASC \
             LIMIT $1",
            [limit.into()],
        ))
        .all(&self.db)
        .await?;
        Ok(rows)
    }

    /// Delete acknowledged outbox rows after ClickHouse confirms the insert.
    pub async fn ack_sale_outbox_ids(&self, ids: &[i64]) -> Result<u64> {
        if ids.is_empty() {
            return Ok(0);
        }
        let mut deleted = 0u64;
        for chunk in ids.chunks(SQL_CHUNK_SIZE) {
            let (sql, values) = build_ack_sql(chunk);
            let res = self
                .db
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Postgres,
                    sql,
                    values,
                ))
                .await?;
            deleted += res.rows_affected();
        }
        Ok(deleted)
    }

    /// Durably quarantine rows that cannot be serialized into ClickHouse (for
    /// example, timestamps outside ClickHouse's `DateTime` bounds) so valid
    /// sales behind them keep draining while preserving operator visibility.
    pub async fn quarantine_sale_outbox_rows(&self, rows: &[(i64, String)]) -> Result<u64> {
        let mut updated = 0u64;
        for (id, reason) in rows {
            let res = self
                .db
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Postgres,
                    "UPDATE clickhouse_sale_outbox \
                     SET quarantined_reason = $2 \
                     WHERE id = $1 AND quarantined_reason IS NULL",
                    [(*id).into(), reason.clone().into()],
                ))
                .await?;
            updated += res.rows_affected();
        }
        Ok(updated)
    }

    /// O(1) outbox backlog depth and oldest-row age from the partial pending
    /// index (`clickhouse_sale_outbox_pending_idx`), safe to poll even during
    /// prolonged ClickHouse outages without scanning the table.
    pub async fn sale_outbox_metrics(&self) -> Result<SaleOutboxMetrics> {
        let row = SaleOutboxMetrics::find_by_statement(Statement::from_string(
            DbBackend::Postgres,
            "SELECT \
                COALESCE( \
                    (SELECT max(id) - min(id) + 1 \
                     FROM clickhouse_sale_outbox \
                     WHERE quarantined_reason IS NULL), \
                    0 \
                )::bigint AS approx_pending_rows, \
                GREATEST( \
                    COALESCE( \
                        (SELECT EXTRACT(EPOCH FROM (now() - created_at)) \
                         FROM clickhouse_sale_outbox \
                         WHERE quarantined_reason IS NULL \
                         ORDER BY id ASC \
                         LIMIT 1), \
                        0 \
                    ), \
                    0 \
                )::double precision AS oldest_age_seconds",
        ))
        .one(&self.db)
        .await?;
        Ok(row.unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;

    fn sample_sale(id: i32, buyer_name: Option<&str>) -> (SaleHistory, UnknownCharacter) {
        let sale = SaleHistory {
            id,
            quantity: 3,
            price_per_item: 1500,
            buying_character_id: 77,
            buyer_name: buyer_name.map(str::to_string),
            hq: true,
            sold_item_id: 42,
            sold_date: DateTime::from_timestamp(1_700_000_000, 0)
                .unwrap()
                .naive_utc(),
            world_id: 54,
        };
        let buyer = UnknownCharacter {
            id: 77,
            name: "Fallback Buyer".to_string(),
        };
        (sale, buyer)
    }

    #[test]
    fn enqueue_sql_binds_all_columns_including_negative_pg_id() {
        let sales = vec![sample_sale(i32::MIN, Some("Alice")), sample_sale(5, None)];
        let (sql, values) = build_enqueue_sql(&sales);
        assert!(
            sql.contains(
                "VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9),($10,$11,$12,$13,$14,$15,$16,$17,$18)"
            )
        );
        assert_eq!(values.len(), 18);
        assert_eq!(values[0], Value::from(i32::MIN));
        assert_eq!(values[8], Value::from("Alice".to_string()));
        assert_eq!(values[17], Value::from("Fallback Buyer".to_string()));
    }

    #[test]
    fn ack_sql_binds_exact_outbox_ids() {
        let (sql, values) = build_ack_sql(&[10, 42, 99]);
        assert_eq!(
            sql,
            "DELETE FROM clickhouse_sale_outbox WHERE id IN ($1,$2,$3)"
        );
        assert_eq!(
            values,
            vec![Value::from(10i64), Value::from(42i64), Value::from(99i64)]
        );
    }

    #[tokio::test]
    #[ignore = "requires ULTROS_PLUGIN_TEST_DATABASE_URL pointing at a disposable loopback database"]
    async fn update_sales_enqueues_quarantines_and_drains_outbox_atomically_postgres() {
        use universalis::{ItemId, SaleView, WorldId};

        let db = crate::common::test_db::connect().await;
        crate::common::test_db::seed_market(&db).await;
        db.db
            .execute_raw(Statement::from_string(
                DbBackend::Postgres,
                "DELETE FROM clickhouse_sale_outbox",
            ))
            .await
            .unwrap();

        let world = WorldId(30001);
        let item = ItemId(9099);
        let sale = SaleView {
            hq: true,
            price_per_unit: 12_345,
            quantity: 2,
            timestamp: DateTime::from_timestamp(1_700_123_456, 0).unwrap().into(),
            on_mannequin: false,
            world_name: None,
            world_id: None,
            buyer_name: "OutboxBuyer".into(),
            total: 24_690,
        };
        let recorded = db.update_sales(vec![sale], item, world).await.unwrap();
        assert_eq!(recorded.len(), 1);
        let pg_id = recorded[0].0.id;

        let pending = db.fetch_sale_outbox_batch(10).await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].pg_id, pg_id);
        assert_eq!(pending[0].item_id, item.0);
        assert_eq!(pending[0].world_id, world.0);
        assert_eq!(pending[0].buyer_name, "OutboxBuyer");
        let stats = db.sale_outbox_metrics().await.unwrap();
        assert_eq!(stats.approx_pending_rows, 1);

        // Quarantining hides the row from future fetches without deleting it.
        assert_eq!(
            db.quarantine_sale_outbox_rows(&[(pending[0].id, "bad timestamp".into())])
                .await
                .unwrap(),
            1
        );
        assert!(db.fetch_sale_outbox_batch(10).await.unwrap().is_empty());
        assert_eq!(
            db.sale_outbox_metrics().await.unwrap().approx_pending_rows,
            0
        );

        let acked = db.ack_sale_outbox_ids(&[pending[0].id]).await.unwrap();
        assert_eq!(acked, 1);
    }

    #[tokio::test]
    #[ignore = "requires ULTROS_PLUGIN_TEST_DATABASE_URL pointing at a disposable loopback database"]
    async fn failed_outbox_enqueue_rolls_back_sale_history_postgres() {
        use crate::entity::sale_history;
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        use universalis::{ItemId, SaleView, WorldId};

        let db = crate::common::test_db::connect().await;
        crate::common::test_db::seed_market(&db).await;
        let world = WorldId(30001);
        let item = ItemId(9098);
        sale_history::Entity::delete_many()
            .filter(sale_history::Column::WorldId.eq(world.0))
            .filter(sale_history::Column::SoldItemId.eq(item.0))
            .exec(&db.db)
            .await
            .unwrap();

        // Force outbox inserts for this test item to fail inside the
        // transaction so we can assert `sale_history` rolls back atomically.
        db.db
            .execute_raw(Statement::from_string(
                DbBackend::Postgres,
                "ALTER TABLE clickhouse_sale_outbox \
                 DROP CONSTRAINT IF EXISTS test_reject_item_9098; \
                 ALTER TABLE clickhouse_sale_outbox \
                 ADD CONSTRAINT test_reject_item_9098 CHECK (item_id <> 9098)",
            ))
            .await
            .unwrap();

        let sale = SaleView {
            hq: false,
            price_per_unit: 999,
            quantity: 1,
            timestamp: DateTime::from_timestamp(1_700_222_222, 0).unwrap().into(),
            on_mannequin: false,
            world_name: None,
            world_id: None,
            buyer_name: "RollbackBuyer".into(),
            total: 999,
        };
        let res = db.update_sales(vec![sale], item, world).await;

        db.db
            .execute_raw(Statement::from_string(
                DbBackend::Postgres,
                "ALTER TABLE clickhouse_sale_outbox \
                 DROP CONSTRAINT IF EXISTS test_reject_item_9098",
            ))
            .await
            .unwrap();

        assert!(
            res.is_err(),
            "outbox constraint violation must fail update_sales"
        );
        let remaining = sale_history::Entity::find()
            .filter(sale_history::Column::WorldId.eq(world.0))
            .filter(sale_history::Column::SoldItemId.eq(item.0))
            .all(&db.db)
            .await
            .unwrap();
        assert!(
            remaining.is_empty(),
            "failed outbox enqueue must roll back the sale_history insert"
        );
    }
}
