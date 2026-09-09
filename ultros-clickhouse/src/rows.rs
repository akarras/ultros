//! Typed row structs for ClickHouse tables.
//!
//! Each struct mirrors a table column-for-column (minus MATERIALIZED columns
//! which CH computes on insert). Conversion impls from existing
//! `ultros-db`/`ultros-api-types` types live alongside the struct so the
//! mapping is obvious at the call site.

use chrono::{DateTime, Utc};
use clickhouse::Row;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use ultros_db::{
    entity::active_listing,
    listings::{ListingChange, ListingChangeKind},
};

/// A ClickHouse table a [`crate::writer::Writer`] can insert into.
///
/// The client validates each insert against the live table schema
/// (`RowBinaryWithNamesAndTypes`), so a struct's field *names* must match the
/// table's column names exactly, in addition to the types. `RowOwned` +
/// `RowWrite` is what `Insert::write` needs for a plain (non-borrowing) row.
pub trait TableRow: clickhouse::RowOwned + clickhouse::RowWrite + Send + Sync {
    const TABLE: &'static str;
}

/// Mirrors the `sales` table. Used by [`crate::writer::Writer`] for inserts
/// and by [`crate::queries`] / [`crate::backfill`] for reads.
///
/// Field ordering matches the CH column ordering (excluding MATERIALIZED
/// columns); the `clickhouse::Row` derive relies on this for native-protocol
/// inserts.
///
/// Negative source values from Postgres are clamped at zero on conversion —
/// `sale_history` columns are signed `i32` for historical reasons, but
/// quantities and prices are domain-constrained to be non-negative.
#[derive(Row, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SaleRow {
    /// Carries the Postgres `sale_history.id` so each PG row maps to a
    /// distinct CH row. It is the last component of the `sales` ORDER BY key,
    /// so without a real value two sales sharing
    /// `(item_id, hq, world_id, sold_date)` collapse under the
    /// `ReplacingMergeTree` dedup — and Universalis-side replays produce a
    /// non-trivial number of those. Every producer must supply the real id:
    /// a placeholder makes the live path and the backfill disagree on row
    /// identity, so the same sale is stored twice and never merges.
    pub pg_id: i32,
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
    /// Build a `SaleRow` from the SeaORM `sale_history::Model` (used by the
    /// backfill path, which streams directly from Postgres).
    pub fn from_db_model(m: &ultros_db::entity::sale_history::Model, buyer_name: String) -> Self {
        Self {
            pg_id: m.id,
            sold_date: chrono::DateTime::from_naive_utc_and_offset(m.sold_date, chrono::Utc),
            item_id: m.sold_item_id,
            hq: m.hq as u8,
            world_id: m.world_id,
            price_per_item: clamp_price(m.price_per_item),
            quantity: clamp_qty(m.quantity),
            buying_character_id: m.buying_character_id as i64,
            buyer_name,
        }
    }

    /// Build a `SaleRow` from the API type (used by the dual-write path, which
    /// reads from the existing event bus and already has a richer payload).
    pub fn from_api_sale(s: &ultros_api_types::SaleHistory) -> Self {
        Self {
            pg_id: s.id,
            sold_date: chrono::DateTime::from_naive_utc_and_offset(s.sold_date, chrono::Utc),
            item_id: s.sold_item_id,
            hq: s.hq as u8,
            world_id: s.world_id,
            price_per_item: clamp_price(s.price_per_item),
            quantity: clamp_qty(s.quantity),
            buying_character_id: s.buying_character_id as i64,
            buyer_name: s.buyer_name.clone().unwrap_or_default(),
        }
    }
}

impl TableRow for SaleRow {
    const TABLE: &'static str = "sales";
}

/// `listing_events.kind`. Values are the Enum8 codes in the DDL.
#[derive(Serialize_repr, Deserialize_repr, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i8)]
pub enum ListingEventKind {
    Added = 1,
    Updated = 2,
    Removed = 3,
}

impl From<ListingChangeKind> for ListingEventKind {
    fn from(kind: ListingChangeKind) -> Self {
        match kind {
            ListingChangeKind::Added => Self::Added,
            ListingChangeKind::Updated => Self::Updated,
            ListingChangeKind::Removed => Self::Removed,
        }
    }
}

/// `listing_events.source`: which ingest path observed the change.
#[derive(Serialize_repr, Deserialize_repr, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i8)]
pub enum ListingEventSource {
    /// The Universalis websocket listener.
    Websocket = 1,
    /// `UpdateService` catch-up and full sweeps (REST boards).
    Catchup = 2,
    /// The `/item/refresh/{world}/{item}` route.
    Manual = 3,
    /// The one-time seed from the current `active_listing` table.
    Snapshot = 4,
}

/// Mirrors the `listing_events` table. Append-only; one row per observed
/// change to `active_listing`. See the 2026-09-07 listing-history spec.
#[derive(Row, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ListingEventRow {
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    pub event_time: DateTime<Utc>,
    pub kind: ListingEventKind,
    pub source: ListingEventSource,
    pub item_id: i32,
    pub hq: u8,
    pub world_id: i32,
    /// Universalis' listing identity; empty for rows stored before the
    /// identity migration.
    pub listing_id: String,
    /// `active_listing.id`, so a legacy add/remove pair can still be joined.
    pub pg_listing_id: i32,
    pub retainer_id: i32,
    pub price_per_unit: u32,
    pub quantity: u16,
    /// 0 unless `kind == Updated`.
    pub prev_price: u32,
    /// 0 unless `kind == Updated`.
    pub prev_quantity: u16,
    /// Universalis `last_review_time`, distinct from our observation time.
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    pub reviewed_at: DateTime<Utc>,
}

impl ListingEventRow {
    pub fn from_change(change: &ListingChange, source: ListingEventSource) -> Self {
        let row = &change.row;
        Self {
            event_time: change.observed_at,
            kind: change.kind.into(),
            source,
            item_id: row.item_id,
            hq: row.hq as u8,
            world_id: row.world_id,
            listing_id: row.listing_id.clone().unwrap_or_default(),
            pg_listing_id: row.id,
            retainer_id: row.retainer_id,
            price_per_unit: clamp_price(row.price_per_unit),
            quantity: clamp_qty(row.quantity),
            prev_price: change.prev_price_per_unit.map(clamp_price).unwrap_or(0),
            prev_quantity: change.prev_quantity.map(clamp_qty).unwrap_or(0),
            reviewed_at: DateTime::from_naive_utc_and_offset(row.timestamp, Utc),
        }
    }

    /// A seed row: the listing was on the board when recording started.
    pub fn from_snapshot(row: &active_listing::Model, observed_at: DateTime<Utc>) -> Self {
        Self::from_change(
            &ListingChange::added(row.clone(), observed_at),
            ListingEventSource::Snapshot,
        )
    }
}

impl TableRow for ListingEventRow {
    const TABLE: &'static str = "listing_events";
}

/// `floor_changes.reason`: which analyzer path moved the floor.
#[derive(Serialize_repr, Deserialize_repr, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i8)]
pub enum FloorChangeReason {
    /// A listing event lowered (or created) the floor.
    Listing = 1,
    /// The cheapest listing was removed and the floor was re-read from Postgres.
    Refill = 2,
    /// A full rebuild of the map from Postgres (boot, or after bus lag) found a
    /// different value than the map held.
    Resync = 3,
}

/// Mirrors the `floor_changes` table: one row per transition of the
/// world-level lowest listing price for an `(item, hq)`. `price_per_unit = 0`
/// means the board emptied.
#[derive(Row, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FloorChangeRow {
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    pub event_time: DateTime<Utc>,
    pub item_id: i32,
    pub hq: u8,
    pub world_id: i32,
    pub price_per_unit: u32,
    pub reason: FloorChangeReason,
}

impl FloorChangeRow {
    pub fn new(
        event_time: DateTime<Utc>,
        item_id: i32,
        hq: bool,
        world_id: i32,
        price_per_unit: i32,
        reason: FloorChangeReason,
    ) -> Self {
        Self {
            event_time,
            item_id,
            hq: hq as u8,
            world_id,
            price_per_unit: clamp_price(price_per_unit),
            reason,
        }
    }
}

impl TableRow for FloorChangeRow {
    const TABLE: &'static str = "floor_changes";
}

/// Prices are domain-constrained non-negative; the Postgres column is `i32`.
fn clamp_price(p: i32) -> u32 {
    p.max(0) as u32
}

/// Clamp an `i32` quantity into a non-negative `u16`. FFXIV stacks cap at 999
/// so `u16` is plenty; the clamp protects against historical-data weirdness
/// (negative values, ints larger than 65535 that shouldn't exist but we don't
/// want to truncate silently).
fn clamp_qty(q: i32) -> u16 {
    q.max(0).min(u16::MAX as i32) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn fixture_sale(price: i32, qty: i32) -> ultros_api_types::SaleHistory {
        ultros_api_types::SaleHistory {
            id: 1,
            quantity: qty,
            price_per_item: price,
            buying_character_id: 42,
            hq: true,
            sold_item_id: 7,
            sold_date: NaiveDate::from_ymd_opt(2026, 5, 15)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap(),
            world_id: 40,
            buyer_name: Some("Test Buyer".to_string()),
        }
    }

    #[test]
    fn from_api_sale_maps_fields_directly() {
        let sale = fixture_sale(1500, 5);
        let row = SaleRow::from_api_sale(&sale);
        assert_eq!(row.pg_id, 1);
        assert_eq!(row.item_id, 7);
        assert_eq!(row.world_id, 40);
        assert_eq!(row.price_per_item, 1500);
        assert_eq!(row.quantity, 5);
        assert_eq!(row.hq, 1);
        assert_eq!(row.buying_character_id, 42);
        assert_eq!(row.buyer_name, "Test Buyer");
    }

    /// The dual-write path is `update_sales` -> `sale_history::Model` (from
    /// `INSERT ... RETURNING`) -> `SaleHistory` -> `SaleRow`. `pg_id` has to
    /// survive all of it: it's the last component of the `sales` ORDER BY key,
    /// so a zero here collapses distinct sales on merge *and* leaves the
    /// backfill (which reads real ids from Postgres) writing a second copy of
    /// every row that never merges with the live one.
    #[test]
    fn freshly_recorded_sale_carries_a_nonzero_pg_id() {
        let recorded = ultros_db::entity::sale_history::Model {
            id: 918_273,
            quantity: 3,
            price_per_item: 2500,
            buying_character_id: 42,
            hq: false,
            sold_item_id: 7,
            sold_date: NaiveDate::from_ymd_opt(2026, 5, 15)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap(),
            world_id: 40,
        };
        let api_sale: ultros_api_types::SaleHistory =
            ultros_db::common_type_conversions::SaleHistoryReturn(recorded, None).into();

        let row = SaleRow::from_api_sale(&api_sale);

        assert_ne!(row.pg_id, 0, "pg_id must not be a placeholder");
        assert_eq!(row.pg_id, 918_273);
    }

    /// Both writers must agree on `pg_id` for the same Postgres row, otherwise
    /// the live insert and the backfill insert are two distinct keys and
    /// ReplacingMergeTree never collapses them.
    #[test]
    fn live_and_backfill_paths_agree_on_pg_id() {
        let recorded = ultros_db::entity::sale_history::Model {
            id: 555,
            quantity: 1,
            price_per_item: 100,
            buying_character_id: 42,
            hq: true,
            sold_item_id: 7,
            sold_date: NaiveDate::from_ymd_opt(2026, 5, 15)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap(),
            world_id: 40,
        };
        let backfill_row = SaleRow::from_db_model(&recorded, "Test Buyer".to_string());
        let api_sale: ultros_api_types::SaleHistory =
            ultros_db::common_type_conversions::SaleHistoryReturn(recorded.clone(), None).into();
        let live_row = SaleRow::from_api_sale(&api_sale);

        assert_eq!(live_row.pg_id, backfill_row.pg_id);
        // Every column of the ReplacingMergeTree sort key must match.
        assert_eq!(live_row.item_id, backfill_row.item_id);
        assert_eq!(live_row.hq, backfill_row.hq);
        assert_eq!(live_row.world_id, backfill_row.world_id);
        assert_eq!(live_row.sold_date, backfill_row.sold_date);
    }

    #[test]
    fn from_api_sale_clamps_negative_price_to_zero() {
        // `sale_history.price_per_item` is signed i32 in PG; defensively clamp.
        let sale = fixture_sale(-50, 1);
        let row = SaleRow::from_api_sale(&sale);
        assert_eq!(row.price_per_item, 0);
    }

    #[test]
    fn from_api_sale_clamps_quantity_to_u16_range() {
        // 65535 is u16::MAX — anything above should clamp, anything negative → 0.
        let sale = fixture_sale(100, 70_000);
        let row = SaleRow::from_api_sale(&sale);
        assert_eq!(row.quantity, u16::MAX);

        let sale = fixture_sale(100, -3);
        let row = SaleRow::from_api_sale(&sale);
        assert_eq!(row.quantity, 0);
    }

    #[test]
    fn from_api_sale_treats_missing_buyer_name_as_empty() {
        let mut sale = fixture_sale(100, 1);
        sale.buyer_name = None;
        let row = SaleRow::from_api_sale(&sale);
        assert_eq!(row.buyer_name, "");
    }

    #[test]
    fn hq_flag_round_trips_as_uint8() {
        let mut sale = fixture_sale(100, 1);
        sale.hq = false;
        let row = SaleRow::from_api_sale(&sale);
        assert_eq!(row.hq, 0);
        sale.hq = true;
        let row = SaleRow::from_api_sale(&sale);
        assert_eq!(row.hq, 1);
    }

    fn fixture_listing(price: i32, quantity: i32) -> active_listing::Model {
        active_listing::Model {
            id: 77,
            world_id: 40,
            item_id: 7,
            retainer_id: 12,
            price_per_unit: price,
            quantity,
            hq: true,
            timestamp: NaiveDate::from_ymd_opt(2026, 9, 1)
                .unwrap()
                .and_hms_opt(8, 0, 0)
                .unwrap(),
            listing_id: Some("123456789012345678".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn listing_event_from_updated_change_carries_previous_state() {
        let observed_at = Utc::now();
        let previous = fixture_listing(500, 3);
        let change = ListingChange::upserted(fixture_listing(450, 2), Some(&previous), observed_at);
        let row = ListingEventRow::from_change(&change, ListingEventSource::Websocket);
        assert_eq!(row.kind, ListingEventKind::Updated);
        assert_eq!(row.source, ListingEventSource::Websocket);
        assert_eq!(row.event_time, observed_at);
        assert_eq!(row.item_id, 7);
        assert_eq!(row.hq, 1);
        assert_eq!(row.world_id, 40);
        assert_eq!(row.listing_id, "123456789012345678");
        assert_eq!(row.pg_listing_id, 77);
        assert_eq!(row.retainer_id, 12);
        assert_eq!(row.price_per_unit, 450);
        assert_eq!(row.quantity, 2);
        assert_eq!(row.prev_price, 500);
        assert_eq!(row.prev_quantity, 3);
        assert_eq!(row.reviewed_at.naive_utc(), change.row.timestamp);
    }

    #[test]
    fn listing_event_from_added_or_removed_change_has_zero_previous_state() {
        let observed_at = Utc::now();
        let added = ListingChange::added(fixture_listing(100, 1), observed_at);
        let row = ListingEventRow::from_change(&added, ListingEventSource::Catchup);
        assert_eq!(row.kind, ListingEventKind::Added);
        assert_eq!(row.prev_price, 0);
        assert_eq!(row.prev_quantity, 0);

        let removed = ListingChange::removed(fixture_listing(100, 1), observed_at);
        let row = ListingEventRow::from_change(&removed, ListingEventSource::Manual);
        assert_eq!(row.kind, ListingEventKind::Removed);
        assert_eq!(row.price_per_unit, 100);
    }

    #[test]
    fn listing_event_uses_empty_listing_id_for_legacy_rows_and_clamps() {
        let observed_at = Utc::now();
        let mut legacy = fixture_listing(-5, 70_000);
        legacy.listing_id = None;
        let change = ListingChange::added(legacy, observed_at);
        let row = ListingEventRow::from_change(&change, ListingEventSource::Websocket);
        assert_eq!(row.listing_id, "");
        assert_eq!(row.price_per_unit, 0);
        assert_eq!(row.quantity, u16::MAX);
    }

    #[test]
    fn listing_event_from_snapshot_is_an_added_snapshot_row() {
        let observed_at = Utc::now();
        let row = ListingEventRow::from_snapshot(&fixture_listing(300, 4), observed_at);
        assert_eq!(row.kind, ListingEventKind::Added);
        assert_eq!(row.source, ListingEventSource::Snapshot);
        assert_eq!(row.event_time, observed_at);
        assert_eq!(row.price_per_unit, 300);
    }

    #[test]
    fn floor_change_row_maps_hq_and_clamps_price() {
        let now = Utc::now();
        let row = FloorChangeRow::new(now, 7, true, 40, 250, FloorChangeReason::Refill);
        assert_eq!(row.hq, 1);
        assert_eq!(row.price_per_unit, 250);
        assert_eq!(row.reason, FloorChangeReason::Refill);
        let empty = FloorChangeRow::new(now, 7, false, 40, -1, FloorChangeReason::Resync);
        assert_eq!(empty.hq, 0);
        assert_eq!(empty.price_per_unit, 0);
    }

    #[test]
    fn table_names_match_the_schema() {
        assert_eq!(SaleRow::TABLE, "sales");
        assert_eq!(ListingEventRow::TABLE, "listing_events");
        assert_eq!(FloorChangeRow::TABLE, "floor_changes");
    }
}

/// Receipt evidence exists only for newly inserted websocket sales. Backfill
/// must never populate this table: insertion time is not original receipt time.
#[derive(Row, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SaleReceiptRow {
    pub pg_id: i32,
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    pub received_at: DateTime<Utc>,
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    pub sold_at: DateTime<Utc>,
    pub item_id: i32,
    pub hq: u8,
    pub world_id: i32,
    pub price_per_unit: u32,
    pub quantity: u16,
}
impl SaleReceiptRow {
    pub fn from_sale(s: &ultros_api_types::SaleHistory, received_at: DateTime<Utc>) -> Self {
        Self {
            pg_id: s.id,
            received_at,
            sold_at: DateTime::from_naive_utc_and_offset(s.sold_date, Utc),
            item_id: s.sold_item_id,
            hq: s.hq as u8,
            world_id: s.world_id,
            price_per_unit: clamp_price(s.price_per_item),
            quantity: clamp_qty(s.quantity),
        }
    }
}
impl TableRow for SaleReceiptRow {
    const TABLE: &'static str = "sale_receipts";
}
