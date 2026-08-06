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
pub mod quality_filter;
pub mod queries;
pub mod rollups;
pub mod rows;
pub mod schema;
pub mod writer;

pub use quality_filter::ResaleQualityFilter;

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

/// A stable, low-cardinality label for *why* a ClickHouse call failed.
///
/// Error reporting groups events by their message, so the message has to name
/// the failure class and nothing else. ClickHouse's own text can't do that job:
/// it embeds live figures that differ on every occurrence —
///
/// ```text
/// Code: 241 ... would use 5.40 GiB ... current RSS: 3.96 GiB, maximum: 5.40 GiB
/// Code: 241 ... would use 5.44 GiB ... current RSS: 4.06 GiB, maximum: 5.40 GiB
/// ```
///
/// Interpolating that into the message would make one incident look like
/// hundreds of unrelated one-off issues, so nothing ever crosses an alert
/// threshold. Report the *kind* and keep the raw text in a structured field,
/// where it stays readable without splintering the group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickHouseErrorKind {
    /// Server-side `Code: 241`: the query wanted more memory than the server's
    /// ceiling allows. Usually a capacity problem (container limit, thread
    /// count, missing spill-to-disk) rather than a bug in the query.
    MemoryLimitExceeded,
    /// The request did not complete in time.
    Timeout,
    /// The server could not be reached at all.
    Unavailable,
    /// Anything else — including decode failures, which are the shape a schema
    /// drift between a row struct and the live table takes.
    Other,
}

impl ClickHouseErrorKind {
    /// Snake-case label, stable across releases: it ends up in issue titles and
    /// alert rules, so renaming one silently re-groups every historical issue
    /// that used it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MemoryLimitExceeded => "memory_limit_exceeded",
            Self::Timeout => "timeout",
            Self::Unavailable => "unavailable",
            Self::Other => "other",
        }
    }
}

impl std::fmt::Display for ClickHouseErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl ClickHouseError {
    /// Classify this error for reporting. See [`ClickHouseErrorKind`].
    pub fn kind(&self) -> ClickHouseErrorKind {
        match self {
            // Backfill failures wrap a Postgres-side message; there is no
            // ClickHouse status code to read.
            ClickHouseError::Backfill(_) => ClickHouseErrorKind::Other,
            ClickHouseError::Client(e) => classify_client_error(&e.to_string()),
        }
    }
}

/// Match on the rendered text rather than `clickhouse::error::Error`'s variants:
/// the condition most worth distinguishing (`Code: 241`) arrives as an opaque
/// `BadResponse(String)`, so the variant alone cannot tell a capacity problem
/// from a malformed query. Deliberately conservative — an unrecognised error
/// falls through to `Other` rather than being mislabelled, because a wrong label
/// is worse than a vague one when it points an operator at a subsystem.
fn classify_client_error(text: &str) -> ClickHouseErrorKind {
    if text.contains("Code: 241") || text.contains("MEMORY_LIMIT_EXCEEDED") {
        ClickHouseErrorKind::MemoryLimitExceeded
    } else if text.contains("timed out") || text.contains("TimedOut") {
        ClickHouseErrorKind::Timeout
    } else if text.contains("Connection refused")
        || text.contains("tcp connect error")
        || text.contains("dns error")
    {
        ClickHouseErrorKind::Unavailable
    } else {
        ClickHouseErrorKind::Other
    }
}

#[cfg(test)]
mod error_kind_tests {
    use super::*;

    /// Verbatim from the 2026-08-02 outage, when a 6 GB container cap had
    /// ClickHouse killing roughly half of all row-returning queries. The two
    /// samples differ only in their memory figures — which is precisely why the
    /// message must not carry them, and why both have to land on one kind.
    #[test]
    fn code_241_is_a_memory_limit_whatever_the_figures_say() {
        let a = "bad response: Code: 241. DB::Exception: (total) memory limit exceeded: \
                 would use 5.40 GiB (attempt to allocate chunk of 0.00 B bytes), current RSS: \
                 3.96 GiB, maximum: 5.40 GiB. (MEMORY_LIMIT_EXCEEDED)";
        let b = "bad response: Code: 241. DB::Exception: (total) memory limit exceeded: \
                 would use 5.44 GiB (attempt to allocate chunk of 0.00 B bytes), current RSS: \
                 4.06 GiB, maximum: 5.40 GiB. (MEMORY_LIMIT_EXCEEDED)";
        assert_eq!(
            classify_client_error(a),
            ClickHouseErrorKind::MemoryLimitExceeded
        );
        assert_eq!(
            classify_client_error(a),
            classify_client_error(b),
            "two occurrences of one incident must group together"
        );
    }

    #[test]
    fn transport_failures_are_distinguishable() {
        assert_eq!(
            classify_client_error("error sending request: tcp connect error: Connection refused"),
            ClickHouseErrorKind::Unavailable
        );
        assert_eq!(
            classify_client_error("operation timed out"),
            ClickHouseErrorKind::Timeout
        );
    }

    /// An unrecognised error must degrade to `Other`, never borrow a label that
    /// would send an operator after the wrong subsystem.
    #[test]
    fn unknown_errors_do_not_borrow_a_label() {
        assert_eq!(
            classify_client_error("bad response: Code: 62. DB::Exception: Syntax error"),
            ClickHouseErrorKind::Other
        );
    }

    /// These labels reach alert rules, so they are API.
    #[test]
    fn labels_are_stable() {
        assert_eq!(
            ClickHouseErrorKind::MemoryLimitExceeded.as_str(),
            "memory_limit_exceeded"
        );
        assert_eq!(ClickHouseErrorKind::Other.as_str(), "other");
    }
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
