//! Diagnostic binary for inspecting the analyzer rollups.
//!
//! Usage:
//!   cargo run --bin clickhouse_inspect            # corpus-wide overview
//!   cargo run --bin clickhouse_inspect 49325      # drill into one item across worlds
//!
//! Prints:
//!   - filter rate by window
//!   - quality-score band distribution
//!   - top 15 most-filtered items (suspected launder hotspots)
//!   - if an item_id is provided, per-world stats for that item

use std::env;

use anyhow::Result;
use ultros_clickhouse::ClickHouseClient;

#[derive(clickhouse::Row, serde::Deserialize)]
struct WindowOverview {
    window_days: u16,
    tuples: u64,
    total_samples: u64,
    total_excluded: u64,
    pct_excluded: f64,
}

#[derive(clickhouse::Row, serde::Deserialize)]
struct BandRow {
    band: String,
    n: u64,
}

#[derive(clickhouse::Row, serde::Deserialize)]
struct Suspect {
    item_id: i32,
    world_id: i32,
    sample_size: u32,
    excluded_count: u32,
    pct_excluded: f64,
    p50: u32,
    vwap: u32,
}

#[derive(clickhouse::Row, serde::Deserialize)]
struct ItemDetail {
    world_id: i32,
    // window_days kept in SELECT for ergonomics; not displayed since we
    // already filter to 30d below
    #[allow(dead_code)]
    window_days: u16,
    sample_size: u32,
    cleaned_sample_size: u32,
    excluded_count: u32,
    vwap: u32,
    p50: u32,
    median_abs_deviation: u32,
    unique_buyers: u32,
    quality_score: u8,
    confidence_band: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let ch = ClickHouseClient::from_env();

    let item_id: Option<i32> = env::args().nth(1).and_then(|s| s.parse().ok());

    if let Some(item_id) = item_id {
        inspect_item(&ch, item_id).await?;
    } else {
        inspect_corpus(&ch).await?;
    }
    Ok(())
}

async fn inspect_corpus(ch: &ClickHouseClient) -> Result<()> {
    println!("=== Window overview (post-filter) ===");
    let rows: Vec<WindowOverview> = ch
        .client()
        .query(
            "SELECT window_days, count() AS tuples, \
             sum(sample_size) AS total_samples, \
             sum(excluded_count) AS total_excluded, \
             round(100 * sum(excluded_count) / greatest(sum(sample_size), 1), 2) AS pct_excluded \
             FROM item_stats_window FINAL \
             GROUP BY window_days ORDER BY window_days",
        )
        .fetch_all()
        .await?;
    for r in &rows {
        println!(
            "  {:>4}d: {:>8} tuples | {:>10} samples | {:>8} excluded ({:>5.2}%)",
            r.window_days, r.tuples, r.total_samples, r.total_excluded, r.pct_excluded
        );
    }

    println!("\n=== Quality score bands ===");
    let bands: Vec<BandRow> = ch
        .client()
        .query(
            "SELECT toString(confidence_band) AS band, count() AS n \
             FROM item_quality_score FINAL \
             GROUP BY band ORDER BY band",
        )
        .fetch_all()
        .await?;
    let total: u64 = bands.iter().map(|b| b.n).sum();
    for b in &bands {
        let pct = if total > 0 {
            100.0 * b.n as f64 / total as f64
        } else {
            0.0
        };
        println!("  {:>10}: {:>8} ({:>5.2}%)", b.band, b.n, pct);
    }

    println!("\n=== Top 15 most-filtered items (suspected launder hotspots) ===");
    let suspects: Vec<Suspect> = ch
        .client()
        .query(
            "SELECT item_id, world_id, sample_size, excluded_count, \
             round(100.0 * excluded_count / sample_size, 1) AS pct_excluded, \
             p50, vwap \
             FROM item_stats_window FINAL \
             WHERE window_days = 30 AND sample_size >= 20 \
             ORDER BY pct_excluded DESC LIMIT 15",
        )
        .fetch_all()
        .await?;
    println!(
        "  {:>8} | {:>8} | {:>5} | {:>5} | {:>5}% | {:>10} | {:>10}",
        "item", "world", "n", "excl", "pct", "p50", "vwap"
    );
    for s in &suspects {
        println!(
            "  {:>8} | {:>8} | {:>5} | {:>5} | {:>5.1} | {:>10} | {:>10}",
            s.item_id, s.world_id, s.sample_size, s.excluded_count, s.pct_excluded, s.p50, s.vwap
        );
    }

    Ok(())
}

async fn inspect_item(ch: &ClickHouseClient, item_id: i32) -> Result<()> {
    println!("=== Item {item_id} per-world stats (30d window) ===");
    // LEFT JOIN against item_quality_score: ClickHouse fills unmatched rows
    // with the column type's zero value rather than NULL. We use sentinel
    // values (quality_score=0 with band='unmatched') and convert at the
    // display layer.
    let rows: Vec<ItemDetail> = ch
        .client()
        .query(
            "SELECT w.world_id, w.window_days, w.sample_size, \
                    w.cleaned_sample_size, w.excluded_count, \
                    w.vwap, w.p50, w.median_abs_deviation, w.unique_buyers, \
                    if(q.computed_at > 0, q.quality_score, toUInt8(0)) AS quality_score, \
                    if(q.computed_at > 0, toString(q.confidence_band), '—') AS confidence_band \
             FROM item_stats_window w FINAL \
             LEFT JOIN item_quality_score q FINAL \
               ON w.item_id = q.item_id AND w.hq = q.hq AND w.world_id = q.world_id \
             WHERE w.item_id = ? AND w.window_days = 30 \
             ORDER BY w.world_id",
        )
        .bind(item_id)
        .fetch_all()
        .await?;

    if rows.is_empty() {
        println!("  (no data — item not seen in last 30 days)");
        return Ok(());
    }
    println!(
        "  {:>5} | {:>5} | {:>5} | {:>5} | {:>9} | {:>9} | {:>5} | {:>5} | {:>3} | {:>10}",
        "world", "n", "clean", "excl", "vwap", "p50", "mad", "buyrs", "qs", "band"
    );
    for r in &rows {
        println!(
            "  {:>5} | {:>5} | {:>5} | {:>5} | {:>9} | {:>9} | {:>5} | {:>5} | {:>3} | {:>10}",
            r.world_id,
            r.sample_size,
            r.cleaned_sample_size,
            r.excluded_count,
            r.vwap,
            r.p50,
            r.median_abs_deviation,
            r.unique_buyers,
            if r.confidence_band == "—" {
                "—".to_string()
            } else {
                r.quality_score.to_string()
            },
            r.confidence_band,
        );
    }
    Ok(())
}
