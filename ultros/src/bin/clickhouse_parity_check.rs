//! Verify count + sum(quantity) parity between Postgres `sale_history` and
//! ClickHouse `sales`, grouped by `(world_id, year-month)`.
//!
//! Usage:
//!   cargo run --bin clickhouse_parity_check
//!
//! Prints one line per (world, ym) tuple and an OK/MISMATCH marker. Exits 0
//! when all tuples are within tolerance (default 0.5% on count, exact match
//! on sum(quantity)), nonzero otherwise.

use std::collections::HashMap;
use std::env;

use anyhow::Result;
use sea_orm::{DbBackend, FromQueryResult, Statement};
use tracing::info;
use ultros_clickhouse::ClickHouseClient;
use ultros_db::UltrosDb;

#[derive(Debug, FromQueryResult)]
struct PgAgg {
    world_id: i32,
    ym: i32,
    sale_count: i64,
    qty_sum: Option<i64>,
}

#[derive(Debug, clickhouse::Row, serde::Deserialize)]
struct ChAgg {
    world_id: i32,
    ym: u32,
    sale_count: u64,
    qty_sum: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,sqlx=warn")),
        )
        .init();

    let start_year: i32 = env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(2026);
    let start_ym = start_year * 100 + 1;

    let pg = UltrosDb::connect().await?;
    let ch = ClickHouseClient::from_env();

    info!(start_year, "fetching Postgres aggregates");
    let pg_rows: Vec<PgAgg> = PgAgg::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
        SELECT world_id::int AS world_id,
               (EXTRACT(YEAR FROM sold_date) * 100 + EXTRACT(MONTH FROM sold_date))::int AS ym,
               count(*)::bigint AS sale_count,
               sum(quantity)::bigint AS qty_sum
        FROM sale_history
        WHERE EXTRACT(YEAR FROM sold_date) >= $1
        GROUP BY world_id, ym
        ORDER BY world_id, ym
        "#,
        vec![start_year.into()],
    ))
    .all(pg.get_connection())
    .await?;
    info!(rows = pg_rows.len(), "Postgres aggregates fetched");

    info!("fetching ClickHouse aggregates");
    let ch_rows: Vec<ChAgg> = ch
        .client()
        .query(
            "SELECT world_id, toYYYYMM(sold_date) AS ym, count() AS sale_count, \
             sum(quantity) AS qty_sum \
             FROM sales WHERE toYYYYMM(sold_date) >= ? \
             GROUP BY world_id, ym ORDER BY world_id, ym",
        )
        .bind(start_ym as u32)
        .fetch_all()
        .await?;
    info!(rows = ch_rows.len(), "ClickHouse aggregates fetched");

    // Build CH lookup by (world, ym).
    let ch_map: HashMap<(i32, u32), &ChAgg> =
        ch_rows.iter().map(|r| ((r.world_id, r.ym), r)).collect();

    let mut mismatches = 0usize;
    let mut total_pg_count = 0i64;
    let mut total_ch_count = 0u64;
    let mut total_pg_qty = 0i64;
    let mut total_ch_qty = 0u64;
    for row in &pg_rows {
        let ym = row.ym as u32;
        let pg_count = row.sale_count;
        let pg_qty = row.qty_sum.unwrap_or(0);
        total_pg_count += pg_count;
        total_pg_qty += pg_qty;

        match ch_map.get(&(row.world_id, ym)) {
            Some(ch) => {
                total_ch_count += ch.sale_count;
                total_ch_qty += ch.qty_sum;
                let drift = if pg_count > 0 {
                    (ch.sale_count as i64 - pg_count).abs() as f64 / pg_count as f64
                } else {
                    0.0
                };
                let qty_match = pg_qty as u64 == ch.qty_sum;
                let count_ok = drift <= 0.005; // 0.5% tolerance
                if qty_match && count_ok {
                    tracing::debug!(
                        world_id = row.world_id,
                        ym = ym,
                        pg_count,
                        ch_count = ch.sale_count,
                        pg_qty,
                        ch_qty = ch.qty_sum,
                        "OK"
                    );
                } else {
                    mismatches += 1;
                    tracing::warn!(
                        world_id = row.world_id,
                        ym = ym,
                        pg_count,
                        ch_count = ch.sale_count,
                        pg_qty,
                        ch_qty = ch.qty_sum,
                        drift_pct = drift * 100.0,
                        qty_match,
                        "MISMATCH"
                    );
                }
            }
            None => {
                mismatches += 1;
                tracing::warn!(
                    world_id = row.world_id,
                    ym = ym,
                    pg_count,
                    pg_qty,
                    "MISSING in ClickHouse"
                );
            }
        }
    }

    println!();
    println!("=== Parity Summary ===");
    println!("PG aggregates:      {} tuples", pg_rows.len());
    println!("CH aggregates:      {} tuples", ch_rows.len());
    println!(
        "PG totals:          count={}  sum(quantity)={}",
        total_pg_count, total_pg_qty
    );
    println!(
        "CH totals:          count={}  sum(quantity)={}",
        total_ch_count, total_ch_qty
    );
    println!(
        "CH/PG count ratio:  {:.4}",
        total_ch_count as f64 / total_pg_count.max(1) as f64
    );
    println!("Mismatched tuples:  {}", mismatches);

    if mismatches > 0 {
        eprintln!("FAIL: {} tuples mismatched", mismatches);
        std::process::exit(1);
    }
    println!("OK");
    Ok(())
}
