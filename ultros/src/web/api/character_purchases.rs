//! `GET /api/v1/characters/{id}/purchases` — what one of *your* claimed
//! characters has bought off the market board.
//!
//! Ultros has always recorded the buyer on every sale it ingests; the item page
//! renders that name in its sale-history table. This endpoint reads the same
//! data by buyer instead of by item.
//!
//! It is deliberately scoped to characters the caller has claimed. Two reasons,
//! and only one of them is privacy:
//!
//! 1. Aggregating a name's purchases into one view turns per-item market data
//!    into a spending profile. Doing that for arbitrary names, on request, is a
//!    different feature with a different set of questions attached.
//! 2. The claim is what supplies a *world*, and the world is what makes the
//!    numbers meaningfully scoped at all — see [`region_world_ids`].
//!
//! What the claim does not supply is proof. Claims are unverified (see
//! `claim_character`), and buyer identity upstream is a bare name with no world
//! on it, so the rows here may belong to a same-named character elsewhere in
//! the region. The response carries `scoped_world_ids` so the UI can say what
//! it actually searched.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;
use ultros_api_types::{
    FfxivCharacter,
    character_purchases::{CharacterPurchase, CharacterPurchaseHistory, CharacterPurchaseSummary},
    world_helper::{AnySelector, WorldHelper},
};
use ultros_clickhouse::{
    ClickHouseClient,
    queries::{
        CharacterPurchaseSummaryRow, MAX_CHARACTER_PURCHASES, purchase_summary_for_character,
        purchases_by_character,
    },
};
use ultros_db::UltrosDb;

use crate::web::error::{ApiError, ClickHouseQueryError};
use crate::web::oauth::AuthDiscordUser;

/// Default row count when the caller doesn't ask for one. Enough to fill the
/// table and scroll for a while without shipping a character's whole history
/// on first paint.
const DEFAULT_LIMIT: u32 = 500;

#[derive(Debug, Deserialize)]
pub(crate) struct PurchaseQuery {
    limit: Option<u32>,
}

/// Every world in the character's region.
///
/// A sale row records the world the *retainer* sold on, never where the buyer
/// was standing, and data-center travel puts a player's purchases anywhere in
/// their region. So the character's own world is far too narrow a filter, and
/// no filter at all is too wide: it would pull in same-named characters from
/// other regions, who cannot be the same person and whose rows would silently
/// pad both the table and the totals. The region is the widest scope the
/// character could actually have shopped in, which makes it the right one.
fn region_world_ids(world_helper: &WorldHelper, world_id: i32) -> Vec<i32> {
    let Some(world) = world_helper.lookup_selector(AnySelector::World(world_id)) else {
        return Vec::new();
    };
    let region = world_helper.get_region(world);
    region
        .datacenters
        .iter()
        .flat_map(|dc| dc.worlds.iter())
        .map(|w| w.id)
        .collect()
}

/// Convert a whole-history rollup into the API shape, mapping ClickHouse's
/// epoch-for-empty timestamps onto `None`.
fn summary_from_row(row: CharacterPurchaseSummaryRow) -> CharacterPurchaseSummary {
    let timestamp = |seconds: u32| {
        // `min`/`max` over an empty set give the DateTime epoch rather than
        // NULL, so a character with no purchases would otherwise report having
        // bought something on 1970-01-01.
        (row.total_purchases > 0)
            .then(|| chrono::DateTime::from_timestamp(i64::from(seconds), 0))
            .flatten()
            .map(|dt| dt.naive_utc())
    };
    CharacterPurchaseSummary {
        total_purchases: row.total_purchases,
        total_gil: row.total_gil,
        total_units: row.total_units,
        distinct_items: row.distinct_items,
        first_purchase: timestamp(row.first_purchase),
        last_purchase: timestamp(row.last_purchase),
    }
}

pub(crate) async fn get_character_purchases(
    State(db): State<UltrosDb>,
    State(ch): State<ClickHouseClient>,
    State(world_helper): State<Arc<WorldHelper>>,
    user: AuthDiscordUser,
    Path(character_id): Path<i32>,
    Query(query): Query<PurchaseQuery>,
) -> Result<Json<CharacterPurchaseHistory>, ApiError> {
    if !db.user_owns_character(user.id as i64, character_id).await? {
        // Deliberately the same answer for "not yours" and "doesn't exist":
        // the two are indistinguishable to the caller, so claiming a character
        // can't be used to probe whether someone else has claimed it.
        return Err(ApiError::Forbidden(
            "You can only view purchases for a character you have claimed",
        ));
    }
    let character = db
        .get_character(character_id)
        .await?
        .ok_or(ApiError::Forbidden(
            "You can only view purchases for a character you have claimed",
        ))?;
    let character = FfxivCharacter::from(character);

    let world_ids = region_world_ids(&world_helper, character.world_id);
    if world_ids.is_empty() {
        return Ok(Json(CharacterPurchaseHistory::empty(character, world_ids)));
    }

    // Buyer rows are keyed on the character's *name*, because that is all
    // Universalis reports. No row means Ultros has never ingested a sale to
    // this name — ordinary for a quiet world, not an error.
    let name = format!("{} {}", character.first_name, character.last_name);
    let Some(buyer) = db.get_unknown_character_by_name(&name).await? else {
        return Ok(Json(CharacterPurchaseHistory::empty(character, world_ids)));
    };
    let buyer_id = i64::from(buyer.id);

    let limit = query
        .limit
        .unwrap_or(DEFAULT_LIMIT)
        .min(MAX_CHARACTER_PURCHASES);
    let rows = purchases_by_character(&ch, buyer_id, &world_ids, limit)
        .await
        .map_err(|e| ClickHouseQueryError::new("purchases_by_character", e))?;
    let summary = purchase_summary_for_character(&ch, buyer_id, &world_ids)
        .await
        .map_err(|e| ClickHouseQueryError::new("purchase_summary_for_character", e))?;

    let truncated = rows.len() as u32 >= limit && summary.total_purchases > rows.len() as u64;
    let purchases = rows
        .into_iter()
        .map(|row| CharacterPurchase {
            item_id: row.item_id,
            hq: row.hq != 0,
            world_id: row.world_id,
            price_per_item: row.price_per_item as i32,
            quantity: i32::from(row.quantity),
            sold_date: row.sold_date.naive_utc(),
        })
        .collect();

    Ok(Json(CharacterPurchaseHistory {
        character,
        purchases,
        summary: summary_from_row(summary),
        scoped_world_ids: world_ids,
        truncated,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(total_purchases: u64, first: u32, last: u32) -> CharacterPurchaseSummaryRow {
        CharacterPurchaseSummaryRow {
            total_purchases,
            total_gil: 1234,
            total_units: 7,
            distinct_items: 3,
            first_purchase: first,
            last_purchase: last,
        }
    }

    #[test]
    fn summary_carries_the_totals_through() {
        let summary = summary_from_row(row(9, 1_700_000_000, 1_760_000_000));
        assert_eq!(summary.total_purchases, 9);
        assert_eq!(summary.total_gil, 1234);
        assert_eq!(summary.distinct_items, 3);
        assert!(summary.first_purchase.is_some());
        assert!(summary.last_purchase.is_some());
    }

    /// ClickHouse answers `min`/`max` over an empty set with the DateTime
    /// epoch, not NULL. Passing that straight through would tell a character
    /// with no recorded purchases that they first bought something in 1970.
    #[test]
    fn empty_history_reports_no_dates_rather_than_the_epoch() {
        let summary = summary_from_row(row(0, 0, 0));
        assert_eq!(summary.total_purchases, 0);
        assert_eq!(summary.first_purchase, None);
        assert_eq!(summary.last_purchase, None);
    }
}
