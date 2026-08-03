//! Pure "which items should Market Trends ask ClickHouse about?" policy.
//!
//! Split out of `analyzer_service` for the same reason as
//! `resale_eligibility`: so the choice is unit-testable without standing up
//! an `AnalyzerService` (whose test binary does not link on Windows).
//!
//! Trends can only afford a bounded number of item tuples per request, so
//! something has to pick. The obvious pick — iterate the cheapest-listings
//! map and take the first N — is silently catastrophic, because that map is
//! a `BTreeMap` keyed on item id: taking the first N takes the N *lowest
//! item ids in the game*. Everything above that id boundary can never
//! appear on the page no matter how briskly it trades, which is why whole
//! categories render empty.
//!
//! The signal used instead is the one the page is actually about: has this
//! item sold recently. That is the same driver the v1 `get_trends` path
//! uses (it iterates the sale history and looks the cheapest price up),
//! and it is available in-process from the analyzer's per-world sale
//! buffer — no extra query.

use std::cmp::Reverse;

use chrono::NaiveDateTime;

/// A listed item on one world, plus whatever recent-sale signal the
/// analyzer has buffered for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TrendCandidate {
    pub(crate) item_id: i32,
    pub(crate) hq: bool,
    /// When this item most recently sold on this world, if the analyzer's
    /// bounded buffer has seen it sell at all.
    pub(crate) last_sale: Option<NaiveDateTime>,
    /// How many sales are currently buffered for it (saturates at the
    /// buffer size, so it separates "trades constantly" from "sold once").
    pub(crate) buffered_sales: u8,
}

/// Total order on "how much does this item belong on the Trends page",
/// most relevant first.
///
/// Sorting by a tuple that ends in the item id keeps this a *total* order,
/// so the selection is deterministic — two replicas answering the same
/// request pick the same items.
fn relevance(
    candidate: &TrendCandidate,
) -> (Reverse<Option<NaiveDateTime>>, Reverse<u8>, i32, bool) {
    // `None` sorts below every `Some`, so reversing puts the most recently
    // traded items first and the never-seen-selling ones last.
    (
        Reverse(candidate.last_sale),
        Reverse(candidate.buffered_sales),
        candidate.item_id,
        candidate.hq,
    )
}

/// Keep the `max` most relevant candidates, most relevant first.
///
/// Items the analyzer has never seen sell keep their old relative order
/// (ascending item id) at the back of the list, so a world whose sale
/// buffer is still cold behaves exactly as it did before this existed.
pub(crate) fn select_trend_candidates(
    mut candidates: Vec<TrendCandidate>,
    max: usize,
) -> Vec<TrendCandidate> {
    if max == 0 {
        return Vec::new();
    }
    // Partition before sorting so a board of tens of thousands of listed
    // items costs O(n) rather than O(n log n); `relevance` is a total
    // order, so the unstable variants are safe here.
    if candidates.len() > max {
        candidates.select_nth_unstable_by_key(max, relevance);
        candidates.truncate(max);
    }
    candidates.sort_unstable_by_key(relevance);
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(day: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 8, day)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
    }

    /// A listed item the sale buffer has never seen sell.
    fn dormant(item_id: i32) -> TrendCandidate {
        TrendCandidate {
            item_id,
            hq: false,
            last_sale: None,
            buffered_sales: 0,
        }
    }

    /// A listed item that sold on `day`, with `count` sales buffered.
    fn traded(item_id: i32, day: u32, count: u8) -> TrendCandidate {
        TrendCandidate {
            item_id,
            hq: false,
            last_sale: Some(at(day)),
            buffered_sales: count,
        }
    }

    fn ids(picked: &[TrendCandidate]) -> Vec<i32> {
        picked.iter().map(|c| c.item_id).collect()
    }

    /// The regression. Candidates arrive in ascending item-id order because
    /// the cheapest-listings map is a `BTreeMap`, so a budget spent in
    /// arrival order buys nothing but the oldest items in the game.
    #[test]
    fn an_actively_traded_high_id_item_beats_dormant_low_id_items() {
        // Ids 2 and 3 are early-ARR junk sitting on the board; 44242 is a
        // current-expansion item that sold twice today.
        let candidates = vec![dormant(2), dormant(3), traded(44242, 10, 6)];

        let picked = select_trend_candidates(candidates, 2);

        assert!(
            ids(&picked).contains(&44242),
            "the only item that actually traded was dropped in favour of \
             two dormant low-id items; picked {:?}",
            ids(&picked)
        );
    }

    #[test]
    fn more_recently_traded_items_rank_first() {
        let candidates = vec![traded(100, 1, 6), traded(200, 20, 6), traded(300, 10, 6)];

        let picked = select_trend_candidates(candidates, 3);

        assert_eq!(ids(&picked), vec![200, 300, 100]);
    }

    #[test]
    fn ties_on_recency_break_on_how_much_is_buffered() {
        let candidates = vec![traded(100, 5, 1), traded(200, 5, 6), traded(300, 5, 3)];

        let picked = select_trend_candidates(candidates, 3);

        assert_eq!(ids(&picked), vec![200, 300, 100]);
    }

    /// A world whose sale buffer is still cold must behave exactly as it
    /// did before relevance ranking existed: ascending item id.
    #[test]
    fn a_cold_sale_buffer_falls_back_to_ascending_item_id() {
        let candidates = vec![dormant(2), dormant(3), dormant(44242)];

        let picked = select_trend_candidates(candidates, 2);

        assert_eq!(ids(&picked), vec![2, 3]);
    }

    #[test]
    fn every_traded_item_outranks_every_dormant_one() {
        let candidates = vec![
            dormant(1),
            traded(50_000, 1, 1),
            dormant(2),
            traded(40_000, 2, 1),
        ];

        let picked = select_trend_candidates(candidates, 4);

        assert_eq!(ids(&picked), vec![40_000, 50_000, 1, 2]);
    }

    #[test]
    fn hq_and_lq_of_one_item_are_separate_candidates_in_a_stable_order() {
        let lq = TrendCandidate {
            item_id: 5,
            hq: false,
            last_sale: Some(at(3)),
            buffered_sales: 2,
        };
        let hq = TrendCandidate { hq: true, ..lq };

        let picked = select_trend_candidates(vec![hq, lq], 2);

        assert_eq!(picked, vec![lq, hq], "lq must sort ahead of hq on ties");
    }

    #[test]
    fn a_budget_larger_than_the_board_keeps_everything() {
        let candidates = vec![traded(100, 1, 1), dormant(2)];

        let picked = select_trend_candidates(candidates, 500);

        assert_eq!(ids(&picked), vec![100, 2]);
    }

    #[test]
    fn a_zero_budget_selects_nothing() {
        assert!(select_trend_candidates(vec![traded(100, 1, 1)], 0).is_empty());
    }
}
