//! Typed row structs for ClickHouse tables.
//!
//! Each struct mirrors a table column-for-column (minus MATERIALIZED columns
//! which CH computes on insert). Conversion impls from existing
//! `ultros-db`/`ultros-api-types` types live alongside the struct so the
//! mapping is obvious at the call site.

use clickhouse::Row;
use serde::{Deserialize, Serialize};

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
            sold_date: chrono::DateTime::from_naive_utc_and_offset(m.sold_date, chrono::Utc),
            item_id: m.sold_item_id,
            hq: m.hq as u8,
            world_id: m.world_id,
            price_per_item: m.price_per_item.max(0) as u32,
            quantity: clamp_qty(m.quantity),
            buying_character_id: m.buying_character_id as i64,
            buyer_name,
        }
    }

    /// Build a `SaleRow` from the API type (used by the dual-write path, which
    /// reads from the existing event bus and already has a richer payload).
    pub fn from_api_sale(s: &ultros_api_types::SaleHistory) -> Self {
        Self {
            sold_date: chrono::DateTime::from_naive_utc_and_offset(s.sold_date, chrono::Utc),
            item_id: s.sold_item_id,
            hq: s.hq as u8,
            world_id: s.world_id,
            price_per_item: s.price_per_item.max(0) as u32,
            quantity: clamp_qty(s.quantity),
            buying_character_id: s.buying_character_id as i64,
            buyer_name: s.buyer_name.clone().unwrap_or_default(),
        }
    }
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
        assert_eq!(row.item_id, 7);
        assert_eq!(row.world_id, 40);
        assert_eq!(row.price_per_item, 1500);
        assert_eq!(row.quantity, 5);
        assert_eq!(row.hq, 1);
        assert_eq!(row.buying_character_id, 42);
        assert_eq!(row.buyer_name, "Test Buyer");
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
}
