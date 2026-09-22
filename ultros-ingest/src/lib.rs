//! Market-data ingest: the Universalis polling/sweep service and the
//! staleness gauge, plus the ClickHouse mirror of listing changes.

// Carried over from the server crate: proving the scheduler future `Send`
// overflows the default limit of 128.
#![recursion_limit = "256"]

// Root aliases so the modules' `crate::event` / `crate::test_market_isolation`
// paths read the same as they did inside the server crate.
use ultros_server_core::event;
#[cfg(feature = "test-auth")]
use ultros_server_core::test_market_isolation;

pub mod ingest_health;
pub mod item_update_service;

/// Mirror a write path's change list into ClickHouse. Non-blocking: overflow
/// is counted by the writer, never felt by ingest.
pub fn record_listing_changes(
    writer: &ultros_clickhouse::writer::Writer<ultros_clickhouse::rows::ListingEventRow>,
    changes: &[ultros_db::listings::ListingChange],
    source: ultros_clickhouse::rows::ListingEventSource,
) {
    for change in changes {
        writer.send(ultros_clickhouse::rows::ListingEventRow::from_change(
            change, source,
        ));
    }
}
