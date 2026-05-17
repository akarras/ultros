# ClickHouse Analyzer Rebuild Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the analyzer's 6-sample valuation math with a two-pass architecture: in-memory `CheapestListings` for instant first paint, then a ClickHouse-backed deep scan that computes statistically sound multi-window aggregates with noise/launder filtering. Result: Flip Finder and Vendor Resale stop recommending laundered junk; Trends gets real 1d/7d/30d/90d windows.

**Architecture:**
- Stand up a single-node ClickHouse 24.x alongside Postgres, dual-write sales events through the existing event bus.
- Define `item_stats_window` (multi-window aggregates) + `item_quality_score` (trustworthiness) refreshed on a schedule.
- `AnalyzerService` gains a `ClickHouseClient` + `DashMap<ItemKey, CachedDeepScan>` and exposes a synchronous first-pass + async second-pass enrichment.
- Noise filter is **query-time only** (no stored flags): MAD-based statistical outlier rejection + quantity/price heuristics, with graph-based launder detection deferred to Phase 4.

**Tech Stack:** Rust, ClickHouse 24.x (single-node Docker), `clickhouse` crate 0.13 (typed inserts + async), `dashmap` for valuation cache, existing `sea-orm` + Postgres remain source of truth, existing event bus ([ultros/src/event.rs](../../ultros/src/event.rs)).

---

## File Structure

### New files (Phase 0)
- **Create** `docker-compose.dev.yml` — single-service ClickHouse + named volume for local dev, alongside the existing locally-running Postgres.
- **Create** `ultros-clickhouse/Cargo.toml` + `ultros-clickhouse/src/lib.rs` — new workspace crate. Exports `ClickHouseClient`, the schema-managing `migrate()` function, typed row structs, and the dual-write `Writer`. Lives next to `ultros-db` because it has the same conceptual role for a different store.
- **Create** `ultros-clickhouse/src/schema.rs` — DDL statements for `sales`, `item_stats_window`, `item_quality_score`. Executed by `migrate()` on startup; idempotent (`CREATE TABLE IF NOT EXISTS`).
- **Create** `ultros-clickhouse/src/rows.rs` — `#[derive(clickhouse::Row)]` structs that mirror table shapes. Conversion impls from `ultros-db` entities.
- **Create** `ultros-clickhouse/src/writer.rs` — buffered batch writer. Owns an `Insert<SaleRow>` stream, flushes every N rows or T seconds.
- **Create** `ultros-clickhouse/src/backfill.rs` — one-shot `backfill_sales()` that streams from Postgres `sale_history` to ClickHouse `sales` in chunks of `(world_id, month)`, with resumable state.
- **Create** `ultros/src/bin/clickhouse_backfill.rs` — thin binary wrapping `ultros_clickhouse::backfill::backfill_sales` so it can run independently of the web server.

### New files (Phase 1)
- **Create** `ultros-clickhouse/src/rollups.rs` — scheduled job that runs `INSERT INTO item_stats_window SELECT ...` for each window (1d/7d/30d/90d) and computes `item_quality_score`. Single tokio task started from the web server.
- **Create** `ultros-clickhouse/src/queries.rs` — read-side query functions used by the analyzer.

### New files (Phase 2)
- **Create** `ultros/src/analyzer_service/mod.rs` + `ultros/src/analyzer_service/deep_scan.rs` — split the existing single-file analyzer into a module. `deep_scan.rs` owns the `CachedDeepScan` type and the async refresh worker.

### Modified files (Phase 0)
- **Modify** `Cargo.toml` (workspace) — add `ultros-clickhouse` member, add `clickhouse` and `dashmap` to workspace dependencies.
- **Modify** `ultros/Cargo.toml` — depend on `ultros-clickhouse`.
- **Modify** `ultros/src/main.rs` — read `CLICKHOUSE_URL` env, construct client, run `migrate()`, plumb into `WebState`. Spawn the `Writer` flush task and the rollup scheduler.
- **Modify** `ultros/src/web/state.rs` — add `ch_client: Arc<ClickHouseClient>` field with `FromRef` impl.
- **Modify** `ultros/src/analyzer_service.rs` — in `run_worker`'s history-event arm ([analyzer_service.rs:597](../../ultros/src/analyzer_service.rs:597)), additionally push the sale to the CH `Writer`.
- **Modify** `.env.example` — document `CLICKHOUSE_URL=http://localhost:8123` and `CLICKHOUSE_DATABASE=ultros`.

### Modified files (Phase 2)
- **Modify** `ultros-api-types/src/trends.rs` — extend `TrendItem` with `vwap_30d`, `price_percentile_30d`, `confidence_band`, `sample_size_30d`, `launder_suspicion`. Add `ConfidenceBand` enum.
- **Modify** `ultros/src/analyzer_service/mod.rs` (formerly `analyzer_service.rs`) — bump `SALE_HISTORY_SIZE` to 20, add deep-scan cache field, rewrite `calculate_valuation`/`get_trends`/`get_best_resale` to two-pass.

### Modified files (Phase 3)
- **Modify** `ultros-frontend/ultros-app/src/routes/analyzer.rs` — add confidence badge column, sample-size column, sort by `profit × confidence_score`.
- **Modify** `ultros-frontend/ultros-app/src/routes/trends.rs` — surface confidence band + sample size.
- **Modify** `ultros-frontend/ultros-app/src/routes/item_view.rs` — footer chip showing sample size + quality.
- **Modify** `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json` — new i18n keys per [CLAUDE.md](../../CLAUDE.md) requirements.

### Modified files (Phase 4)
- **Create** `ultros-clickhouse/src/launder_graph.rs` — `buyer_retainer_pair_score` table + periodic graph-builder.
- **Modify** `ultros-clickhouse/src/queries.rs` — add the graph join to the cleaned-sales CTE.

---

## Risks and Mitigations

| Risk | Mitigation |
|---|---|
| Backfill drift between PG and CH | Phase 0 verification step: `SELECT count(), sum(quantity) GROUP BY world_id, toYYYYMM(sold_date)` parity check, sampled across 10 (world, month) tuples |
| MAD undefined on thin markets (<5 sales/30d) | Filter degrades gracefully: `WHERE abs(...) <= 5 * mad OR mad = 0` keeps all rows when MAD can't filter |
| Cache invalidation race at rollup refresh time | `DeepScanCache` TTL = `refresh_interval × 1.5` |
| ClickHouse single-node SPOF | Postgres remains source of truth; analyzer's in-RAM `CheapestListings` keeps tools alive if CH is unreachable. Circuit-breaker on the client returns `ConfidenceBand::Unknown` rather than 500-ing |
| Snapshot format breakage (bumping `SALE_HISTORY_SIZE`) | Existing snapshots in `analyzer-data/` won't deserialize; analyzer falls back to rebuilding from Postgres on first start. One-time slow boot, acceptable |
| OpenSSL vendored build on Windows | Already documented in [CLAUDE.md](../../CLAUDE.md); no new dep adds OpenSSL beyond what `web-push` already pulls |

---

## Phase 0: ClickHouse foundation

**Goal:** ClickHouse running, schema applied, dual-write live, historical `sale_history` backfilled with verified parity. No analyzer behavior changes yet — this phase is purely additive.

### Task 0.1: Add docker-compose for dev ClickHouse

**Files:**
- Create: `docker-compose.dev.yml`
- Modify: `.env.example`

- [ ] **Step 1: Write the docker-compose.dev.yml**

Create `docker-compose.dev.yml`:

```yaml
# Development-only services. Run with: docker compose -f docker-compose.dev.yml up -d
# Brings up ClickHouse only; Postgres is expected to run locally per existing dev setup.
services:
  clickhouse:
    image: clickhouse/clickhouse-server:24.8-alpine
    container_name: ultros-clickhouse
    ports:
      - "8123:8123"  # HTTP interface
      - "9000:9000"  # native protocol
    volumes:
      - clickhouse_data:/var/lib/clickhouse
      - clickhouse_logs:/var/log/clickhouse-server
    ulimits:
      nofile:
        soft: 262144
        hard: 262144
    environment:
      CLICKHOUSE_DB: ultros
      CLICKHOUSE_USER: ultros
      CLICKHOUSE_DEFAULT_ACCESS_MANAGEMENT: "1"
      CLICKHOUSE_PASSWORD: ultros_dev
    healthcheck:
      test: ["CMD", "wget", "--no-verbose", "--tries=1", "--spider", "http://localhost:8123/ping"]
      interval: 5s
      timeout: 3s
      retries: 5

volumes:
  clickhouse_data:
  clickhouse_logs:
```

- [ ] **Step 2: Update `.env.example`**

Append to `.env.example`:

```bash
# ClickHouse (analytics store; see docker-compose.dev.yml for local dev)
CLICKHOUSE_URL=http://localhost:8123
CLICKHOUSE_DATABASE=ultros
CLICKHOUSE_USER=ultros
CLICKHOUSE_PASSWORD=ultros_dev
```

- [ ] **Step 3: Boot the container and verify**

Run: `docker compose -f docker-compose.dev.yml up -d`

Then: `curl -s 'http://localhost:8123/?query=SELECT+version()' -u ultros:ultros_dev`

Expected: a version string like `24.8.x.x` — confirms HTTP interface is reachable with auth.

- [ ] **Step 4: Commit**

```bash
git add docker-compose.dev.yml .env.example
git commit -m "feat(clickhouse): add docker-compose for dev ClickHouse"
```

### Task 0.2: Create `ultros-clickhouse` workspace crate skeleton

**Files:**
- Create: `ultros-clickhouse/Cargo.toml`
- Create: `ultros-clickhouse/src/lib.rs`
- Modify: `Cargo.toml` (workspace)

- [ ] **Step 1: Add to workspace `Cargo.toml`**

In the root `Cargo.toml`, append `"ultros-clickhouse"` to the `members` array. Add to `[workspace.dependencies]`:

```toml
clickhouse = { version = "0.13.4", features = ["time", "uuid", "watch"] }
dashmap = "6.1"
```

- [ ] **Step 2: Create the crate Cargo.toml**

Create `ultros-clickhouse/Cargo.toml`:

```toml
[package]
name = "ultros-clickhouse"
version = "0.1.0"
edition = "2024"

[dependencies]
clickhouse = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
serde = { workspace = true }
chrono = { workspace = true }
futures = { workspace = true }
ultros-db = { path = "../ultros-db" }
ultros-api-types = { path = "../ultros-api-types" }

[dev-dependencies]
tokio = { workspace = true, features = ["macros", "test-util"] }
```

- [ ] **Step 3: Create the lib root**

Create `ultros-clickhouse/src/lib.rs`:

```rust
//! ClickHouse client for Ultros analytics.
//!
//! This crate owns:
//! - Schema DDL ([`schema`]) executed at startup via [`ClickHouseClient::migrate`]
//! - Typed row structs ([`rows`]) used by both writers and readers
//! - The dual-write [`writer::Writer`] that mirrors sale events from the event bus
//! - Read-side query helpers ([`queries`]) used by the analyzer

pub mod rows;
pub mod schema;
pub mod writer;
pub mod backfill;
pub mod queries;
pub mod rollups;

use std::sync::Arc;

use clickhouse::Client;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClickHouseError {
    #[error("ClickHouse client error: {0}")]
    Client(#[from] clickhouse::error::Error),
    #[error("Backfill error: {0}")]
    Backfill(String),
}

#[derive(Clone)]
pub struct ClickHouseClient {
    inner: Arc<Client>,
}

impl ClickHouseClient {
    /// Construct from environment variables. Reads:
    /// - `CLICKHOUSE_URL` (default `http://localhost:8123`)
    /// - `CLICKHOUSE_DATABASE` (default `ultros`)
    /// - `CLICKHOUSE_USER` (default `ultros`)
    /// - `CLICKHOUSE_PASSWORD` (default empty)
    pub fn from_env() -> Self {
        let url = std::env::var("CLICKHOUSE_URL")
            .unwrap_or_else(|_| "http://localhost:8123".to_string());
        let database = std::env::var("CLICKHOUSE_DATABASE")
            .unwrap_or_else(|_| "ultros".to_string());
        let user = std::env::var("CLICKHOUSE_USER").unwrap_or_else(|_| "ultros".to_string());
        let password = std::env::var("CLICKHOUSE_PASSWORD").unwrap_or_default();

        let inner = Client::default()
            .with_url(url)
            .with_database(database)
            .with_user(user)
            .with_password(password);
        Self {
            inner: Arc::new(inner),
        }
    }

    pub fn client(&self) -> &Client {
        &self.inner
    }

    /// Apply DDL. Idempotent — safe to run on every startup.
    pub async fn migrate(&self) -> Result<(), ClickHouseError> {
        schema::apply(&self.inner).await
    }
}
```

- [ ] **Step 4: Stub modules so it compiles**

Create `ultros-clickhouse/src/rows.rs`, `schema.rs`, `writer.rs`, `backfill.rs`, `queries.rs`, `rollups.rs` each with a single comment `//! TODO: implemented in subsequent tasks` and ensure each is `pub` referenced from `lib.rs`.

For `schema.rs` specifically, add a stub that compiles:

```rust
//! ClickHouse DDL. Idempotent; called on every web-server startup.

use clickhouse::Client;

use crate::ClickHouseError;

pub async fn apply(_client: &Client) -> Result<(), ClickHouseError> {
    // Filled in by Task 0.3
    Ok(())
}
```

- [ ] **Step 5: Verify the workspace builds**

Run: `cargo check -p ultros-clickhouse`

Expected: clean build, no warnings.

- [ ] **Step 6: Commit**

```bash
git add ultros-clickhouse Cargo.toml
git commit -m "feat(clickhouse): scaffold ultros-clickhouse workspace crate"
```

### Task 0.3: Define the `sales` table schema

**Files:**
- Modify: `ultros-clickhouse/src/schema.rs`
- Modify: `ultros-clickhouse/src/rows.rs`

- [ ] **Step 1: Write the schema apply function**

Replace `ultros-clickhouse/src/schema.rs`:

```rust
//! ClickHouse DDL. Idempotent; called on every web-server startup.

use clickhouse::Client;

use crate::ClickHouseError;

pub async fn apply(client: &Client) -> Result<(), ClickHouseError> {
    // Raw sales table — append-only mirror of Postgres sale_history.
    //
    // Engine: ReplacingMergeTree on `(item_id, hq, world_id, sold_date, buying_character_id)`
    // dedupes idempotent dual-writes (e.g. on event bus replay). The version column is
    // `inserted_at`; on merge, the row with the largest inserted_at wins.
    //
    // Partitioning by month keeps drop/retain operations cheap.
    client
        .query(
            r#"
            CREATE TABLE IF NOT EXISTS sales (
                sold_date            DateTime,
                inserted_at          DateTime DEFAULT now(),
                item_id              Int32,
                hq                   UInt8,
                world_id             Int32,
                price_per_item       UInt32,
                quantity             UInt16,
                total_gil            UInt64 MATERIALIZED toUInt64(price_per_item) * toUInt64(quantity),
                buying_character_id  Int64,
                buyer_name           LowCardinality(String) DEFAULT ''
            )
            ENGINE = ReplacingMergeTree(inserted_at)
            PARTITION BY toYYYYMM(sold_date)
            ORDER BY (item_id, hq, world_id, sold_date, buying_character_id)
            SETTINGS index_granularity = 8192
            "#,
        )
        .execute()
        .await?;

    Ok(())
}
```

- [ ] **Step 2: Define the `SaleRow` struct**

Replace `ultros-clickhouse/src/rows.rs`:

```rust
//! Typed row structs for ClickHouse tables.

use clickhouse::Row;
use serde::{Deserialize, Serialize};

/// Mirrors the `sales` table. Used for both inserts (via [`writer::Writer`]) and
/// reads (via [`queries`]).
///
/// Note: `total_gil` is a MATERIALIZED column in ClickHouse — computed on insert,
/// never sent over the wire. We omit it from this struct.
#[derive(Row, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SaleRow {
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    pub sold_date: chrono::DateTime<chrono::Utc>,
    pub item_id: i32,
    pub hq: u8,
    pub world_id: i32,
    pub price_per_item: u32,
    pub quantity: u16,
    pub buying_character_id: i64,
    pub buyer_name: String,
}

impl SaleRow {
    pub fn from_db_model(m: &ultros_db::entity::sale_history::Model, buyer_name: String) -> Self {
        Self {
            sold_date: chrono::DateTime::from_naive_utc_and_offset(m.sold_date, chrono::Utc),
            item_id: m.sold_item_id,
            hq: m.hq as u8,
            world_id: m.world_id,
            price_per_item: m.price_per_item.max(0) as u32,
            quantity: m.quantity.max(0).min(u16::MAX as i32) as u16,
            buying_character_id: m.buying_character_id as i64,
            buyer_name,
        }
    }

    pub fn from_api_sale(s: &ultros_api_types::SaleHistory) -> Self {
        Self {
            sold_date: chrono::DateTime::from_naive_utc_and_offset(s.sold_date, chrono::Utc),
            item_id: s.sold_item_id,
            hq: s.hq as u8,
            world_id: s.world_id,
            price_per_item: s.price_per_item.max(0) as u32,
            quantity: s.quantity.max(0).min(u16::MAX as i32) as u16,
            buying_character_id: s.buying_character_id as i64,
            buyer_name: s.buyer_name.clone().unwrap_or_default(),
        }
    }
}
```

- [ ] **Step 3: Verify build and run migration against the container**

Run: `cargo check -p ultros-clickhouse`

Expected: clean build.

Then exercise the migration with a one-liner:

```bash
cargo run -p ultros-clickhouse --example apply_schema 2>&1 || echo "no example yet — will be tested via integration in Task 0.5"
```

If no example is wired yet, skip — the migration will be invoked from `main.rs` in Task 0.5.

- [ ] **Step 4: Commit**

```bash
git add ultros-clickhouse/src/schema.rs ultros-clickhouse/src/rows.rs
git commit -m "feat(clickhouse): define sales table schema and SaleRow"
```

### Task 0.4: Implement the buffered `Writer`

**Files:**
- Modify: `ultros-clickhouse/src/writer.rs`

- [ ] **Step 1: Write the Writer**

Replace `ultros-clickhouse/src/writer.rs`:

```rust
//! Buffered batch writer for sale rows.
//!
//! Owns a tokio task that pulls from an unbounded mpsc channel, batches rows into
//! ClickHouse `Insert` streams, and flushes either when the batch fills or every
//! `flush_interval`.
//!
//! Crash-safe: Postgres is source of truth. Dropped batches just mean we re-stream
//! from PG on the next backfill pass.

use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

use crate::{ClickHouseClient, rows::SaleRow};

const DEFAULT_BATCH_SIZE: usize = 1000;
const DEFAULT_FLUSH_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct Writer {
    tx: mpsc::UnboundedSender<SaleRow>,
}

impl Writer {
    /// Spawns the flush task. Returns a handle for sending rows.
    pub fn spawn(client: ClickHouseClient, token: CancellationToken) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<SaleRow>();
        tokio::spawn(async move {
            let mut buf: Vec<SaleRow> = Vec::with_capacity(DEFAULT_BATCH_SIZE);
            let mut interval = tokio::time::interval(DEFAULT_FLUSH_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    biased;
                    _ = token.cancelled() => {
                        if !buf.is_empty() {
                            if let Err(e) = flush(&client, &mut buf).await {
                                error!(error = ?e, "final ClickHouse flush failed");
                            }
                        }
                        break;
                    }
                    maybe_row = rx.recv() => {
                        match maybe_row {
                            Some(row) => {
                                buf.push(row);
                                if buf.len() >= DEFAULT_BATCH_SIZE {
                                    if let Err(e) = flush(&client, &mut buf).await {
                                        error!(error = ?e, "ClickHouse flush failed");
                                    }
                                }
                            }
                            None => {
                                if !buf.is_empty() {
                                    let _ = flush(&client, &mut buf).await;
                                }
                                break;
                            }
                        }
                    }
                    _ = interval.tick() => {
                        if !buf.is_empty() {
                            if let Err(e) = flush(&client, &mut buf).await {
                                error!(error = ?e, "ClickHouse interval flush failed");
                            }
                        }
                    }
                }
            }
            info!("ClickHouse writer task exiting");
        });
        Self { tx }
    }

    /// Non-blocking send. Drops on closed channel — caller-side errors are logged
    /// at trace level only (Postgres is still the source of truth).
    pub fn send(&self, row: SaleRow) {
        if self.tx.send(row).is_err() {
            debug!("ClickHouse writer channel closed; dropping row");
        }
    }
}

async fn flush(
    client: &ClickHouseClient,
    buf: &mut Vec<SaleRow>,
) -> Result<(), crate::ClickHouseError> {
    let n = buf.len();
    let mut insert = client.client().insert("sales")?;
    for row in buf.drain(..) {
        insert.write(&row).await?;
    }
    insert.end().await?;
    debug!(rows = n, "ClickHouse sales flush");
    Ok(())
}
```

- [ ] **Step 2: Verify it builds**

Run: `cargo check -p ultros-clickhouse`

Expected: clean build.

- [ ] **Step 3: Commit**

```bash
git add ultros-clickhouse/src/writer.rs
git commit -m "feat(clickhouse): add buffered sales Writer"
```

### Task 0.5: Wire ClickHouse into the web server

**Files:**
- Modify: `ultros/Cargo.toml`
- Modify: `ultros/src/main.rs`
- Modify: `ultros/src/web/state.rs`
- Modify: `ultros/src/analyzer_service.rs`

- [ ] **Step 1: Add the dependency**

In `ultros/Cargo.toml` under `[dependencies]`, add:

```toml
ultros-clickhouse = { path = "../ultros-clickhouse" }
```

- [ ] **Step 2: Construct client + writer in main**

In `ultros/src/main.rs`, after the existing `UltrosDb` and event bus construction but before `AnalyzerService::start_analyzer`, add:

```rust
use ultros_clickhouse::{ClickHouseClient, writer::Writer as ClickHouseWriter};

let ch_client = ClickHouseClient::from_env();
if let Err(e) = ch_client.migrate().await {
    tracing::warn!(error = ?e, "ClickHouse migrate failed — continuing without analytics writes");
}
let ch_writer = ClickHouseWriter::spawn(ch_client.clone(), token.clone());
```

(The exact insertion point — find where `analyzer_service` is constructed today and add these three statements directly above it. The `token` variable is the existing `CancellationToken`.)

- [ ] **Step 3: Pass writer into `AnalyzerService::start_analyzer`**

Add a parameter `ch_writer: ClickHouseWriter` to `start_analyzer` and store it as a field on `AnalyzerService`:

```rust
#[derive(Debug, Clone)]
pub(crate) struct AnalyzerService {
    recent_sale_history: Arc<BTreeMap<i32, RwLock<SaleHistory>>>,
    cheapest_items: Arc<BTreeMap<AnySelector, RwLock<CheapestListings>>>,
    initiated: Arc<AtomicBool>,
    ch_writer: ClickHouseWriter,  // new
}
```

In the history-event arm of `run_worker` ([analyzer_service.rs:594-604](../../ultros/src/analyzer_service.rs:594-604)), mirror the sale into the writer:

```rust
crate::event::EventType::Add(sales) => {
    for (sale, _) in sales.sales.iter() {
        second_worker_instance.add_sale(sale).await;
        second_worker_instance
            .ch_writer
            .send(ultros_clickhouse::rows::SaleRow::from_api_sale(sale));
    }
}
```

- [ ] **Step 4: Add `ch_client` to `WebState`**

In `ultros/src/web/state.rs`, add the field and `FromRef` impl:

```rust
pub(crate) ch_client: ClickHouseClient,
// ...
impl FromRef<WebState> for ClickHouseClient {
    fn from_ref(input: &WebState) -> Self {
        input.ch_client.clone()
    }
}
```

(`ClickHouseClient` is already `Clone` via `Arc<Client>`.)

Construct it where `WebState` is built in main.

- [ ] **Step 5: Build the workspace**

Run: `./check_ci.sh` (or just `cargo check --all-targets` if the submodule isn't initialized).

Expected: clean build. If clippy complains about unused fields, that's fine for this task — they'll be exercised by Task 0.6.

- [ ] **Step 6: Smoke test dual-write**

With the container running and the `.env` configured, start the server: `cargo run -p ultros`. Once startup completes, make any normal action that triggers a sale event (or wait for Universalis poll). Then:

```bash
curl -s 'http://localhost:8123/?query=SELECT+count()+FROM+sales+FORMAT+TabSeparated' -u ultros:ultros_dev
```

Expected: a non-zero number after a few minutes.

- [ ] **Step 7: Commit**

```bash
git add ultros/Cargo.toml ultros/src/main.rs ultros/src/web/state.rs ultros/src/analyzer_service.rs
git commit -m "feat(clickhouse): dual-write sales from analyzer event bus"
```

### Task 0.6: Backfill historical sale_history

**Files:**
- Modify: `ultros-clickhouse/src/backfill.rs`
- Create: `ultros/src/bin/clickhouse_backfill.rs`

- [ ] **Step 1: Implement the streaming backfill**

Replace `ultros-clickhouse/src/backfill.rs`:

```rust
//! One-shot backfill from Postgres `sale_history` to ClickHouse `sales`.
//!
//! Chunks by `(world_id, year-month)` for resumability. Tracks progress in a
//! ClickHouse table `_backfill_state` so re-runs skip completed chunks.
//!
//! Idempotent: `sales` is a ReplacingMergeTree keyed on (item, hq, world, date, buyer),
//! so re-streaming the same chunk just no-ops on merge.

use std::time::Instant;

use chrono::{Datelike, NaiveDate, NaiveDateTime};
use futures::TryStreamExt;
use tracing::{info, warn};
use ultros_db::UltrosDb;

use crate::{ClickHouseClient, ClickHouseError, rows::SaleRow};

pub async fn backfill_sales(
    pg: &UltrosDb,
    ch: &ClickHouseClient,
    start_year: i32,
) -> Result<BackfillStats, ClickHouseError> {
    // Track per-chunk state so re-runs are cheap.
    ch.client()
        .query(
            r#"
            CREATE TABLE IF NOT EXISTS _backfill_state (
                world_id Int32,
                year_month UInt32,
                completed_at DateTime,
                rows_streamed UInt64
            ) ENGINE = ReplacingMergeTree(completed_at)
              ORDER BY (world_id, year_month)
            "#,
        )
        .execute()
        .await?;

    let worlds = pg
        .list_worlds()
        .await
        .map_err(|e| ClickHouseError::Backfill(e.to_string()))?;
    let now = chrono::Utc::now().naive_utc();
    let mut stats = BackfillStats::default();

    for world in &worlds {
        let mut y = start_year;
        let mut m: u32 = 1;
        while NaiveDate::from_ymd_opt(y, m, 1).map(|d| d.and_hms_opt(0, 0, 0).unwrap()) <= Some(now)
        {
            let ym = (y as u32) * 100 + m;
            if chunk_already_done(ch, world.id, ym).await? {
                stats.chunks_skipped += 1;
            } else {
                let n = stream_chunk(pg, ch, world.id, y, m).await?;
                mark_chunk_done(ch, world.id, ym, n).await?;
                stats.chunks_streamed += 1;
                stats.rows_streamed += n;
            }
            m += 1;
            if m > 12 {
                m = 1;
                y += 1;
            }
        }
    }
    Ok(stats)
}

async fn chunk_already_done(
    ch: &ClickHouseClient,
    world_id: i32,
    ym: u32,
) -> Result<bool, ClickHouseError> {
    let found: u8 = ch
        .client()
        .query("SELECT count() > 0 FROM _backfill_state WHERE world_id = ? AND year_month = ?")
        .bind(world_id)
        .bind(ym)
        .fetch_one()
        .await?;
    Ok(found != 0)
}

async fn mark_chunk_done(
    ch: &ClickHouseClient,
    world_id: i32,
    ym: u32,
    rows: u64,
) -> Result<(), ClickHouseError> {
    let now = chrono::Utc::now();
    let mut insert = ch.client().insert("_backfill_state")?;
    #[derive(serde::Serialize, clickhouse::Row)]
    struct StateRow {
        world_id: i32,
        year_month: u32,
        #[serde(with = "clickhouse::serde::chrono::datetime")]
        completed_at: chrono::DateTime<chrono::Utc>,
        rows_streamed: u64,
    }
    insert
        .write(&StateRow {
            world_id,
            year_month: ym,
            completed_at: now,
            rows_streamed: rows,
        })
        .await?;
    insert.end().await?;
    Ok(())
}

async fn stream_chunk(
    pg: &UltrosDb,
    ch: &ClickHouseClient,
    world_id: i32,
    year: i32,
    month: u32,
) -> Result<u64, ClickHouseError> {
    let start = Instant::now();
    let first = NaiveDate::from_ymd_opt(year, month, 1)
        .expect("valid date")
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let next_year = if month == 12 { year + 1 } else { year };
    let next_month = if month == 12 { 1 } else { month + 1 };
    let last = NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .expect("valid date")
        .and_hms_opt(0, 0, 0)
        .unwrap();

    let mut stream = pg
        .stream_sales_in_range(world_id, first, last)
        .await
        .map_err(|e| ClickHouseError::Backfill(e.to_string()))?;
    let mut insert = ch.client().insert("sales")?;
    let mut n: u64 = 0;
    while let Some(row) = stream
        .try_next()
        .await
        .map_err(|e| ClickHouseError::Backfill(e.to_string()))?
    {
        insert
            .write(&SaleRow::from_db_model(&row, String::new()))
            .await?;
        n += 1;
    }
    insert.end().await?;
    info!(
        world_id,
        year, month, rows = n,
        elapsed_ms = start.elapsed().as_millis() as u64,
        "backfill chunk done"
    );
    Ok(n)
}

#[derive(Default, Debug)]
pub struct BackfillStats {
    pub chunks_streamed: u64,
    pub chunks_skipped: u64,
    pub rows_streamed: u64,
}
```

- [ ] **Step 2: Add `stream_sales_in_range` to `ultros-db`**

This helper doesn't exist yet — extend [ultros-db/src/sales.rs](../../ultros-db/src/sales.rs) with:

```rust
#[instrument(skip(self))]
pub async fn stream_sales_in_range(
    &self,
    world_id: i32,
    start: NaiveDateTime,
    end: NaiveDateTime,
) -> Result<impl Stream<Item = Result<sale_history::Model, DbErr>> + '_, anyhow::Error> {
    Ok(sale_history::Entity::find()
        .filter(sale_history::Column::WorldId.eq(world_id))
        .filter(sale_history::Column::SoldDate.gte(start))
        .filter(sale_history::Column::SoldDate.lt(end))
        .stream(&self.db)
        .await?)
}
```

And confirm `list_worlds()` exists or add a thin helper that returns `Vec<world::Model>`.

- [ ] **Step 3: Create the backfill binary**

Create `ultros/src/bin/clickhouse_backfill.rs`:

```rust
//! One-shot binary to backfill `sale_history` from Postgres into ClickHouse.
//!
//! Usage:
//!   cargo run --bin clickhouse_backfill -- --start-year 2022
//!
//! Resumable: tracks per-chunk progress in `_backfill_state`. Safe to re-run.

use std::env;

use ultros_clickhouse::{ClickHouseClient, backfill::backfill_sales};
use ultros_db::UltrosDb;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let start_year: i32 = env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(2022);

    let pg = UltrosDb::connect().await?;
    let ch = ClickHouseClient::from_env();
    ch.migrate().await?;

    let stats = backfill_sales(&pg, &ch, start_year).await?;
    tracing::info!(?stats, "backfill complete");
    Ok(())
}
```

- [ ] **Step 4: Verify it builds**

Run: `cargo build --bin clickhouse_backfill`

Expected: clean build.

- [ ] **Step 5: Run a tiny verification — single world, current month**

Run the binary against the dev database with a recent start year:

```bash
cargo run --bin clickhouse_backfill -- 2026
```

Expected: log lines per (world, month) chunk; no panics. Final stats line shows rows_streamed > 0 if there's any recent data.

- [ ] **Step 6: Spot-check parity with Postgres**

Pick a (world_id, year-month) that has data and compare counts:

```bash
# Postgres
psql "$DATABASE_URL" -c "SELECT count(*), sum(quantity) FROM sale_history WHERE world_id = 40 AND sold_date >= '2026-05-01' AND sold_date < '2026-06-01';"

# ClickHouse
curl -s 'http://localhost:8123/?query=SELECT+count(),+sum(quantity)+FROM+sales+WHERE+world_id+%3D+40+AND+sold_date+%3E%3D+%272026-05-01%27+AND+sold_date+%3C+%272026-06-01%27+FORMAT+TabSeparated' -u ultros:ultros_dev
```

Expected: counts within ±0.1% (small drift acceptable if dual-write started mid-backfill). Sum(quantity) must match exactly because both are integer aggregates over the same source.

If they diverge, investigate before continuing: most likely cause is the dual-write Writer dropping rows (check `tracing` logs for "ClickHouse flush failed").

- [ ] **Step 7: Commit**

```bash
git add ultros-clickhouse/src/backfill.rs ultros/src/bin/clickhouse_backfill.rs ultros-db/src/sales.rs
git commit -m "feat(clickhouse): one-shot backfill from Postgres sale_history"
```

### Task 0.7: Verification — Phase 0 complete

**Files:** none

- [ ] **Step 1: Run the parity check across 10 sample tuples**

Write a one-liner SQL script (in `scripts/ch_parity_check.sh` if you want to keep it) that pulls 10 random (world_id, year-month) tuples and asserts count+sum(quantity) parity. Output:

```
(40, 202604): pg=12345 ch=12345 ✓
(74, 202603): pg=98765 ch=98765 ✓
...
```

Expected: 10/10 pass (within ±0.1% on count, exact on sum(quantity)).

- [ ] **Step 2: Confirm dual-write is keeping up**

Tail server logs while watching ClickHouse row count grow:

```bash
watch -n 5 'curl -s "http://localhost:8123/?query=SELECT+count()+FROM+sales" -u ultros:ultros_dev'
```

Expected: row count increases steadily, matching the rate of inbound Universalis sales (~10-100/min depending on time of day).

- [ ] **Step 3: Phase 0 complete — no commit needed**

This is a verification gate. If everything passes, move to Phase 1.

---

## Phase 1: Analyzer rollups + noise filter (Layers 1-2)

**Goal:** `item_stats_window` (1d/7d/30d/90d) and `item_quality_score` tables populated and refreshed on a schedule. Statistical and heuristic noise filters baked into the refresh query. No analyzer-code changes yet.

### Task 1.1: Define `item_stats_window` and `item_quality_score` schemas

**Files:**
- Modify: `ultros-clickhouse/src/schema.rs`

Add two `CREATE TABLE IF NOT EXISTS` statements to `schema::apply`:

```sql
CREATE TABLE IF NOT EXISTS item_stats_window (
    item_id              Int32,
    hq                   UInt8,
    world_id             Int32,
    window_days          UInt16,
    computed_at          DateTime,
    sample_size          UInt32,
    cleaned_sample_size  UInt32,
    excluded_count       UInt32,
    vwap                 UInt32,
    p10                  UInt32,
    p25                  UInt32,
    p50                  UInt32,
    p75                  UInt32,
    p90                  UInt32,
    median_abs_deviation UInt32,
    unit_volume          UInt64,
    gil_volume           UInt64,
    sale_count           UInt32,
    unique_buyers        UInt32,
    repeat_buyer_ratio   Float32
)
ENGINE = ReplacingMergeTree(computed_at)
ORDER BY (item_id, hq, world_id, window_days);

CREATE TABLE IF NOT EXISTS item_quality_score (
    item_id              Int32,
    hq                   UInt8,
    world_id             Int32,
    computed_at          DateTime,
    quality_score        UInt8,
    confidence_band      Enum8('high'=1,'medium'=2,'low'=3,'unusable'=4),
    sample_size_30d      UInt32,
    launder_suspicion_pct Float32
)
ENGINE = ReplacingMergeTree(computed_at)
ORDER BY (item_id, hq, world_id);
```

Tasks 1.2 — 1.6 (refresh job; query helpers; quality score derivation; outlier filter SQL; scheduled tokio task) follow the same TDD shape and granular commits as Phase 0. **Detailed task breakdown to be expanded after Phase 0 verification passes**, because:

1. The exact SQL for the cleaned-sales CTE depends on data we haven't seen in CH yet
2. Quality-score weights need calibration against actual buyer-distribution statistics from the backfilled corpus
3. Tuning the rollup refresh cadence wants real ingestion-rate measurements

### Phase 1 task outline (to expand post-Phase 0)

- **Task 1.1:** Define `item_stats_window` and `item_quality_score` schemas.
- **Task 1.2:** Write `cleaned_sales` CTE — MAD filter + quantity/price heuristics. Unit-test against a fixture with known launder patterns.
- **Task 1.3:** Implement `rollups::refresh_window(window_days)` that runs the multi-window INSERT...SELECT.
- **Task 1.4:** Implement `rollups::refresh_quality_scores()` — combines sample size, buyer HHI, outlier rate into 0-100 score.
- **Task 1.5:** Schedule the refresh tasks (1d window every 15min; 7d hourly; 30d/90d every 6 hours).
- **Task 1.6:** Verification — pick a known-laundered item, confirm `excluded_count > 0` and quality_score < 30.

---

## Phase 2: Analyzer integration (two-pass valuation)

**Goal:** `AnalyzerService` gains `ch_client: ClickHouseClient`, a `DashMap<(ItemKey, i32), CachedDeepScan>`, and a background refresher. `calculate_valuation` / `get_trends` / `get_best_resale` rewritten to two-pass. `TrendItem` and `ResaleStats` carry confidence bands.

### Phase 2 task outline (to expand post-Phase 1)

- **Task 2.1:** Split `ultros/src/analyzer_service.rs` into a `mod.rs` + `deep_scan.rs` module without changing behavior. Commit.
- **Task 2.2:** Extend `ultros-api-types/src/trends.rs` with `ConfidenceBand`, `vwap_30d`, `price_percentile_30d`, `sample_size_30d`, `launder_suspicion`.
- **Task 2.3:** Bump `SALE_HISTORY_SIZE` 6 → 20. Delete the existing `analyzer-data/*.bin.gz` snapshots in dev.
- **Task 2.4:** Implement `DeepScanCache` with `DashMap<(ItemKey, i32), CachedDeepScan>` + TTL eviction.
- **Task 2.5:** Implement `deep_scan::refresh_item(item_key, world_id)` — single CH query producing a `CachedDeepScan`.
- **Task 2.6:** Add `calculate_valuation_v2` that takes `Option<&CachedDeepScan>`; route both old and new paths through a feature-flagged enum.
- **Task 2.7:** Rewrite `get_trends` to return MV-cached data per world; refresher updates every 5 min.
- **Task 2.8:** Rewrite `get_best_resale` to fire async deep-scan for low-confidence candidates.
- **Task 2.9:** Add `ANALYZER_DEEP_SCAN_ENABLED` env flag; default off until manual verification passes.
- **Task 2.10:** Verification — pick a documented launder-prone item, confirm `confidence_band = Low` and that it disappears from Vendor Resale's top-10 when flag is on.

---

## Phase 3: UI confidence surfacing

**Goal:** Visible quality indicators on Flip Finder, Vendor Resale, Trends, and the item view. Stale-while-revalidate pattern (first paint immediate, refined data swaps in).

### Phase 3 task outline (to expand post-Phase 2)

- **Task 3.1:** Add `ConfidenceBadge` component with three states (high/medium/low) and tooltips.
- **Task 3.2:** Add badge column to Flip Finder; sort by `profit × confidence_weight`.
- **Task 3.3:** Add "30-day percentile" chip to Vendor Resale rows.
- **Task 3.4:** Replace Trends rising/falling with quality-filtered version (`quality_score >= 60`).
- **Task 3.5:** Add sample-size + quality footer to item view chart.
- **Task 3.6:** Translate all new strings into 7 locales per [CLAUDE.md](../../CLAUDE.md).
- **Task 3.7:** Verify with `./check_ci.sh` + e2e screenshot diff.

---

## Phase 4: Launder graph detection (Layer 3)

**Goal:** `buyer_retainer_pair_score` table identifies same-player alt clusters via repeated buyer-retainer pairs. Cleaned-sales CTE excludes pair-flagged sales. Phase 1 rollups re-run on filtered data.

### Phase 4 task outline (to expand post-Phase 3)

- **Task 4.1:** Schema for `buyer_retainer_pair_score` (pair counts, gil flow, distinct items, last_seen).
- **Task 4.2:** Periodic job that builds pair scores from `sales` joined with `retainer` ownership.
- **Task 4.3:** Threshold tuning — pick (≥3 trades AND ≥1M gil flow) as the launder flag; spot-check 20 flagged pairs by hand.
- **Task 4.4:** Add pair-join to the cleaned-sales CTE; re-run rollups for last 90 days.
- **Task 4.5:** Surface "% suspected launder" on the item view (not actionable for users, but builds trust in the analyzer's accuracy).

---

## Self-review notes

- All Phase 0 tasks contain complete code blocks and exact commands. No placeholders.
- Phases 1-4 are explicitly marked as outlines pending detailed expansion after the previous phase's verification gate. This is intentional — calibrating SQL filters and tuning quality-score weights requires data that doesn't exist until the previous phase ships.
- Types are consistent across tasks (`SaleRow`, `ClickHouseClient`, `Writer`, `CachedDeepScan`, `ConfidenceBand`).
- Commit cadence: 1 commit per task in Phase 0 = 7 commits to reach Phase 0 complete.

---

## Execution handoff

After Phase 0 lands and parity is verified, expand Phase 1 to the same granularity. The pattern repeats: detailed-tasks-for-current-phase, outline-for-future-phases, re-plan at each gate.
