//! ClickHouse client for Ultros analytics.
//!
//! This crate owns:
//! - Schema DDL ([`schema`]) executed at startup via [`ClickHouseClient::migrate`]
//! - Typed row structs ([`rows`]) used by both writers and readers
//! - The dual-write [`writer::Writer`] that mirrors sale events from the event bus
//! - Read-side query helpers ([`queries`]) used by the analyzer
//! - One-shot backfill ([`backfill`]) from Postgres `sale_history`
//! - Scheduled rollup refreshers ([`rollups`])
//!
//! ClickHouse complements rather than replaces Postgres. PG stays the source of
//! truth; CH is the analytical engine. The analyzer's in-RAM `CheapestListings`
//! remains the hot path for snappy tools (Flip Finder, Vendor Resale, Recipe
//! Analyzer, FC Crafting). CH backs the deeper trend/historical math.

pub mod backfill;
pub mod queries;
pub mod rollups;
pub mod rows;
pub mod schema;
pub mod writer;

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

/// Cheaply-cloneable handle to a configured ClickHouse client.
///
/// The inner `Client` is wrapped in `Arc` so cloning is just a refcount bump —
/// freely pass this around (into `WebState`, into the analyzer, into background
/// tasks).
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
        let url =
            std::env::var("CLICKHOUSE_URL").unwrap_or_else(|_| "http://localhost:8123".to_string());
        let database =
            std::env::var("CLICKHOUSE_DATABASE").unwrap_or_else(|_| "ultros".to_string());
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

    /// Access the underlying `clickhouse::Client`. Use for queries/inserts that
    /// don't have a dedicated helper yet.
    pub fn client(&self) -> &Client {
        &self.inner
    }

    /// Apply DDL. Idempotent — safe to run on every startup.
    pub async fn migrate(&self) -> Result<(), ClickHouseError> {
        schema::apply(&self.inner).await
    }
}
