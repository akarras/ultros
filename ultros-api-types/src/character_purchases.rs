//! Payload for the owned-character purchase history
//! (`GET /api/v1/characters/{id}/purchases`).
//!
//! Ultros attributes every sale it ingests to the buyer Universalis named on
//! it, so the buy side of the market has always been recorded — it is just
//! only ever been readable one item at a time. These types read it the other
//! way round: everything one character bought.
//!
//! The identity this is keyed on is weaker than it looks, and
//! [`CharacterPurchaseHistory::scoped_world_ids`] plus the ambiguity note the
//! UI is expected to render are the honest way to present that. Universalis
//! reports a buyer as a bare character name with no world attached, and
//! `unknown_final_fantasy_character.name` is `UNIQUE`, so characters who share
//! a name across worlds share one buyer id in Ultros' data and their
//! purchases are already interleaved with nothing to separate them. Claiming a
//! character proves who *you* are; it does not prove the purchases filed under
//! your name are yours.

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::FfxivCharacter;

/// A single purchase: one market-board sale, seen from the buyer's side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterPurchase {
    pub item_id: i32,
    pub hq: bool,
    /// The world the *retainer* sold on, which is where the purchase happened
    /// — not necessarily the buyer's home world.
    pub world_id: i32,
    pub price_per_item: i32,
    pub quantity: i32,
    pub sold_date: NaiveDateTime,
}

impl CharacterPurchase {
    /// Total gil this purchase cost, before tax.
    pub fn total_gil(&self) -> i64 {
        i64::from(self.price_per_item) * i64::from(self.quantity)
    }
}

/// Totals over the character's entire history in scope, independent of how
/// many rows [`CharacterPurchaseHistory::purchases`] actually carries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterPurchaseSummary {
    pub total_purchases: u64,
    pub total_gil: u64,
    pub total_units: u64,
    pub distinct_items: u64,
    /// `None` when there are no purchases in scope.
    pub first_purchase: Option<NaiveDateTime>,
    pub last_purchase: Option<NaiveDateTime>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterPurchaseHistory {
    pub character: FfxivCharacter,
    /// Newest first, capped server-side. `truncated` says whether the cap bit.
    pub purchases: Vec<CharacterPurchase>,
    pub summary: CharacterPurchaseSummary,
    /// Worlds the search covered: the character's whole region, because a
    /// buyer can data-center travel and the sale row records the seller's
    /// world rather than the buyer's.
    pub scoped_world_ids: Vec<i32>,
    /// `true` when the row list hit the server-side cap, so the UI can say the
    /// table is a window onto a longer history rather than the whole of it.
    /// The summary totals are unaffected.
    pub truncated: bool,
}

impl CharacterPurchaseHistory {
    /// An empty history for a character Ultros has never seen buy anything.
    /// Distinct from an error: a character with no recorded purchases is a
    /// perfectly ordinary result, and common for anyone whose server isn't
    /// well covered by Universalis uploaders.
    pub fn empty(character: FfxivCharacter, scoped_world_ids: Vec<i32>) -> Self {
        Self {
            character,
            purchases: Vec::new(),
            summary: CharacterPurchaseSummary::default(),
            scoped_world_ids,
            truncated: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn purchase(price: i32, quantity: i32) -> CharacterPurchase {
        CharacterPurchase {
            item_id: 5,
            hq: false,
            world_id: 40,
            price_per_item: price,
            quantity,
            sold_date: NaiveDate::from_ymd_opt(2026, 8, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
        }
    }

    #[test]
    fn total_gil_multiplies_price_by_quantity() {
        assert_eq!(purchase(1500, 4).total_gil(), 6000);
    }

    /// A full stack of an expensive item overflows `i32` when multiplied out,
    /// which is why the total widens to `i64`.
    #[test]
    fn total_gil_does_not_overflow_on_a_large_stack() {
        assert_eq!(purchase(99_999_999, 999).total_gil(), 99_899_999_001);
    }

    #[test]
    fn empty_history_has_zeroed_totals_and_no_dates() {
        let history = CharacterPurchaseHistory::empty(FfxivCharacter::default(), vec![40, 41]);
        assert!(history.purchases.is_empty());
        assert!(!history.truncated);
        assert_eq!(history.summary.total_gil, 0);
        assert_eq!(history.summary.first_purchase, None);
        assert_eq!(history.scoped_world_ids, vec![40, 41]);
    }
}
