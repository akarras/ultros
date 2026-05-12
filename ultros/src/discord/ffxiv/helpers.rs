//! Pure helpers extracted from the Discord command handlers so they can be unit-tested
//! without spinning up Serenity, Poise, the DB, or the world cache.

use anyhow::anyhow;
use ultros_api_types::world_helper::AnySelector;
use ultros_db::entity::active_listing;
use xiv_gen::ItemId;

use super::{Context, Error};
use crate::analyzer_service::{SoldAmount, SoldWithin};

/// Map a user-supplied "threshold in days" into the appropriate `SoldWithin` bucket.
///
/// The bucket boundaries match the previous inline ladder in `analyze::profit`:
/// `≤1` → Today, `≤7` → Week, `≤30` → Month, `≤365` → Year, otherwise → YearsAgo(days/365).
/// `YearsAgo` clamps the year count to `1..=255` to fit in `u8`.
pub(crate) fn threshold_days_to_sold_within(threshold_days: i32, amount: SoldAmount) -> SoldWithin {
    if threshold_days <= 1 {
        SoldWithin::Today(amount)
    } else if threshold_days <= 7 {
        SoldWithin::Week(amount)
    } else if threshold_days <= 30 {
        SoldWithin::Month(amount)
    } else if threshold_days <= 365 {
        SoldWithin::Year(amount)
    } else {
        SoldWithin::YearsAgo(((threshold_days / 365).clamp(1, 255)) as u8, amount)
    }
}

/// Clamp a user-supplied "number recently sold" into a `SoldAmount` (`u8`-bounded).
///
/// Negative values become 0; values above 255 saturate to 255.
pub(crate) fn clamp_sold_amount(number_recently_sold: i32) -> SoldAmount {
    SoldAmount(number_recently_sold.clamp(0, 255) as u8)
}

/// Case-insensitive substring match used by Discord autocomplete handlers. The caller
/// is expected to lower `partial_lower` once before iterating so the hot loop doesn't
/// re-allocate a `String` per item. An empty needle matches any haystack. Unicode-aware
/// via `to_lowercase`.
pub(crate) fn name_matches_lowered(haystack: &str, partial_lower: &str) -> bool {
    haystack.to_lowercase().contains(partial_lower)
}

/// ASCII-only variant of [`name_matches_lowered`]. Cheaper for plain-text fields like
/// retainer names that are guaranteed to be ASCII in practice. Caller pre-lowers needle.
pub(crate) fn name_matches_lowered_ascii(haystack: &str, partial_lower: &str) -> bool {
    haystack.to_ascii_lowercase().contains(partial_lower)
}

/// Return the cheapest `limit` listings, optionally filtered by HQ.
///
/// Sorts by `price_per_unit` ascending. When `hq_filter` is `Some(true)`, only HQ listings
/// are kept; `Some(false)` keeps only NQ; `None` keeps both.
pub(crate) fn top_n_cheapest_listings(
    mut listings: Vec<active_listing::Model>,
    hq_filter: Option<bool>,
    limit: usize,
) -> Vec<active_listing::Model> {
    listings.sort_by_key(|l| l.price_per_unit);
    listings
        .into_iter()
        .filter(|l| hq_filter.map(|hq| l.hq == hq).unwrap_or(true))
        .take(limit)
        .collect()
}

/// Discord autocomplete handler for item-name string args. Returns up to 99 game items
/// whose lowercased name contains the partial input. The choice value is the item name
/// (callers should resolve it to an id via [`resolve_item_id`]).
pub(crate) async fn autocomplete_item<'a>(
    _ctx: Context<'_>,
    partial: &'a str,
) -> impl Iterator<Item = poise::serenity_prelude::AutocompleteChoice> + 'a {
    let partial = partial.to_lowercase();
    xiv_gen_db::data()
        .items
        .values()
        .filter(move |item| name_matches_lowered(&item.name, &partial))
        .map(|item| {
            poise::serenity_prelude::AutocompleteChoice::new(
                item.name.to_string(),
                item.name.to_string(),
            )
        })
        .take(99)
}

/// Resolve a user-supplied item name (case-insensitive exact match) to an FFXIV item id.
/// Returns `None` if no item with that exact name exists.
pub(crate) fn resolve_item_id(name: &str) -> Option<i32> {
    let lowered = name.to_lowercase();
    xiv_gen_db::data()
        .items
        .iter()
        .find(|(_, item)| item.name.to_lowercase() == lowered)
        .map(|(ItemId(id), _)| *id)
}

/// Parse a world/datacenter/region name from a user-supplied string via the world cache.
/// Returns an `AnySelector` suitable for serializing into an alert's `world_selector` JSON.
pub(crate) async fn parse_world_selector(
    ctx: &Context<'_>,
    name: &str,
) -> Result<AnySelector, Error> {
    use ultros_db::world_data::world_cache::AnySelector as DbAnySelector;
    let result = ctx
        .data()
        .world_cache
        .lookup_value_by_name(name)
        .map_err(|e| anyhow!("unknown world/datacenter/region '{name}': {e}"))?;
    let db_sel: DbAnySelector = (&result).into();
    Ok(match db_sel {
        DbAnySelector::World(id) => AnySelector::World(id),
        DbAnySelector::Datacenter(id) => AnySelector::Datacenter(id),
        DbAnySelector::Region(id) => AnySelector::Region(id),
    })
}

/// Default world selector for the caller: returns `AnySelector::World(w)` where `w` is the
/// world of one of the caller's owned characters. Errors if the user has no claimed
/// characters, since the codebase doesn't store a per-user "home world" column.
pub(crate) async fn user_home_world_selector(ctx: &Context<'_>) -> Result<AnySelector, Error> {
    let owner = ctx.author().id.get() as i64;
    let chars = ctx
        .data()
        .db
        .get_all_characters_for_discord_user(owner)
        .await?;
    let world_id = chars
        .into_iter()
        .find_map(|(_, ch)| ch.map(|c| c.world_id))
        .ok_or_else(|| {
            anyhow!(
                "no world specified and no claimed character on file. \
                 Pass `world:<name>` or claim a character with `/ffxiv character`."
            )
        })?;
    Ok(AnySelector::World(world_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    fn listing(id: i32, price: i32, hq: bool) -> active_listing::Model {
        active_listing::Model {
            id,
            world_id: 1,
            item_id: 1,
            retainer_id: 1,
            price_per_unit: price,
            quantity: 1,
            hq,
            timestamp: NaiveDateTime::default(),
        }
    }

    // ---------- threshold_days_to_sold_within ----------

    #[test]
    fn threshold_zero_or_negative_falls_into_today() {
        // The ladder uses `<= 1` for Today, so 0 and negative values land here.
        assert!(matches!(
            threshold_days_to_sold_within(0, SoldAmount(1)),
            SoldWithin::Today(_)
        ));
        assert!(matches!(
            threshold_days_to_sold_within(-5, SoldAmount(1)),
            SoldWithin::Today(_)
        ));
        assert!(matches!(
            threshold_days_to_sold_within(i32::MIN, SoldAmount(1)),
            SoldWithin::Today(_)
        ));
    }

    #[test]
    fn threshold_one_is_today() {
        assert!(matches!(
            threshold_days_to_sold_within(1, SoldAmount(0)),
            SoldWithin::Today(_)
        ));
    }

    #[test]
    fn threshold_two_through_seven_is_week() {
        for d in 2..=7 {
            assert!(
                matches!(
                    threshold_days_to_sold_within(d, SoldAmount(0)),
                    SoldWithin::Week(_)
                ),
                "expected Week for {d} days",
            );
        }
    }

    #[test]
    fn threshold_eight_through_thirty_is_month() {
        for d in [8, 15, 30] {
            assert!(
                matches!(
                    threshold_days_to_sold_within(d, SoldAmount(0)),
                    SoldWithin::Month(_)
                ),
                "expected Month for {d} days",
            );
        }
    }

    #[test]
    fn threshold_thirty_one_through_365_is_year() {
        for d in [31, 100, 365] {
            assert!(
                matches!(
                    threshold_days_to_sold_within(d, SoldAmount(0)),
                    SoldWithin::Year(_)
                ),
                "expected Year for {d} days",
            );
        }
    }

    #[test]
    fn threshold_just_over_a_year_is_one_year_ago() {
        // 366 / 365 = 1
        assert_eq!(
            threshold_days_to_sold_within(366, SoldAmount(0)),
            SoldWithin::YearsAgo(1, SoldAmount(0))
        );
    }

    #[test]
    fn threshold_two_years_floor_division() {
        // 730 / 365 = 2
        assert_eq!(
            threshold_days_to_sold_within(730, SoldAmount(2)),
            SoldWithin::YearsAgo(2, SoldAmount(2))
        );
        // 731 / 365 = 2 (floor)
        assert_eq!(
            threshold_days_to_sold_within(731, SoldAmount(0)),
            SoldWithin::YearsAgo(2, SoldAmount(0))
        );
    }

    #[test]
    fn threshold_extreme_clamps_to_255_years() {
        // 256 * 365 days would overflow u8 if not clamped.
        let huge = 256 * 365;
        assert_eq!(
            threshold_days_to_sold_within(huge, SoldAmount(1)),
            SoldWithin::YearsAgo(255, SoldAmount(1))
        );
        assert_eq!(
            threshold_days_to_sold_within(i32::MAX, SoldAmount(1)),
            SoldWithin::YearsAgo(255, SoldAmount(1))
        );
    }

    #[test]
    fn threshold_passes_amount_through_unchanged() {
        let amt = SoldAmount(42);
        if let SoldWithin::Year(a) = threshold_days_to_sold_within(100, amt) {
            assert_eq!(a, amt);
        } else {
            panic!("expected Year bucket");
        }
    }

    // ---------- clamp_sold_amount ----------

    #[test]
    fn clamp_sold_amount_passes_in_range_values_through() {
        assert_eq!(clamp_sold_amount(0), SoldAmount(0));
        assert_eq!(clamp_sold_amount(42), SoldAmount(42));
        assert_eq!(clamp_sold_amount(255), SoldAmount(255));
    }

    #[test]
    fn clamp_sold_amount_clamps_negative_to_zero() {
        assert_eq!(clamp_sold_amount(-1), SoldAmount(0));
        assert_eq!(clamp_sold_amount(i32::MIN), SoldAmount(0));
    }

    #[test]
    fn clamp_sold_amount_saturates_above_255() {
        assert_eq!(clamp_sold_amount(256), SoldAmount(255));
        assert_eq!(clamp_sold_amount(10_000), SoldAmount(255));
        assert_eq!(clamp_sold_amount(i32::MAX), SoldAmount(255));
    }

    // ---------- name_matches_lowered ----------

    #[test]
    fn name_matches_lowered_does_not_lower_needle() {
        // Caller is expected to pass already-lowered needle; uppercase needle won't match.
        assert!(!name_matches_lowered("Adamantoise", "ADA"));
        assert!(name_matches_lowered("Adamantoise", "ada"));
    }

    #[test]
    fn name_matches_lowered_lowers_haystack_only() {
        // Haystack is lowered each call; mixed-case haystacks still match lowercase needles.
        assert!(name_matches_lowered("AdamANToise", "manto"));
    }

    #[test]
    fn name_matches_lowered_empty_needle_matches_anything() {
        assert!(name_matches_lowered("anything", ""));
        assert!(name_matches_lowered("", ""));
    }

    #[test]
    fn name_matches_lowered_substring_anywhere() {
        assert!(name_matches_lowered("Behemoth", "hemo"));
        assert!(name_matches_lowered("Behemoth", "moth"));
    }

    #[test]
    fn name_matches_lowered_no_match_returns_false() {
        assert!(!name_matches_lowered("Adamantoise", "xyz"));
        assert!(!name_matches_lowered("", "anything"));
    }

    #[test]
    fn name_matches_lowered_handles_unicode() {
        // Turkish dotless I lowercases differently than ASCII.
        assert!(name_matches_lowered("İstanbul", "i\u{0307}stan"));
        // CJK names in the game data should match by exact lowercase identity.
        assert!(name_matches_lowered("中国", "中国"));
    }

    // ---------- name_matches_lowered_ascii ----------

    #[test]
    fn name_matches_lowered_ascii_does_not_lower_needle() {
        assert!(!name_matches_lowered_ascii("Bob", "BOB"));
        assert!(name_matches_lowered_ascii("Bob", "bob"));
    }

    #[test]
    fn name_matches_lowered_ascii_empty_needle_matches_anything() {
        assert!(name_matches_lowered_ascii("Bob", ""));
    }

    #[test]
    fn name_matches_lowered_ascii_does_not_lowercase_non_ascii() {
        // Uppercase 'İ' is NOT lowercased by to_ascii_lowercase, so a lowered-needle
        // of "i" won't find it in the haystack.
        assert!(!name_matches_lowered_ascii("İstanbul", "i"));
    }

    #[test]
    fn name_matches_lowered_ascii_finds_substring_with_prelowered_needle() {
        for (h, lower) in [("Bob", "bo"), ("Retainer42", "ainer"), ("Cap", "p")] {
            assert!(name_matches_lowered_ascii(h, lower));
        }
    }

    // ---------- top_n_cheapest_listings ----------

    #[test]
    fn top_n_cheapest_returns_listings_in_ascending_price_order() {
        let listings = vec![
            listing(1, 500, false),
            listing(2, 100, false),
            listing(3, 250, false),
        ];
        let result = top_n_cheapest_listings(listings, None, 10);
        let prices: Vec<_> = result.iter().map(|l| l.price_per_unit).collect();
        assert_eq!(prices, vec![100, 250, 500]);
    }

    #[test]
    fn top_n_cheapest_truncates_to_limit() {
        let listings = (1..=20).map(|i| listing(i, i * 10, false)).collect();
        let result = top_n_cheapest_listings(listings, None, 5);
        assert_eq!(result.len(), 5);
        let prices: Vec<_> = result.iter().map(|l| l.price_per_unit).collect();
        assert_eq!(prices, vec![10, 20, 30, 40, 50]);
    }

    #[test]
    fn top_n_cheapest_limit_zero_returns_empty() {
        let listings = vec![listing(1, 1, false)];
        assert!(top_n_cheapest_listings(listings, None, 0).is_empty());
    }

    #[test]
    fn top_n_cheapest_empty_input_stays_empty() {
        let result = top_n_cheapest_listings(vec![], None, 10);
        assert!(result.is_empty());
    }

    #[test]
    fn top_n_cheapest_hq_only_filters_to_hq() {
        let listings = vec![
            listing(1, 100, false),
            listing(2, 200, true),
            listing(3, 50, false),
            listing(4, 300, true),
        ];
        let result = top_n_cheapest_listings(listings, Some(true), 10);
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|l| l.hq));
        assert_eq!(result[0].price_per_unit, 200);
        assert_eq!(result[1].price_per_unit, 300);
    }

    #[test]
    fn top_n_cheapest_nq_only_filters_to_nq() {
        let listings = vec![
            listing(1, 100, false),
            listing(2, 200, true),
            listing(3, 50, false),
        ];
        let result = top_n_cheapest_listings(listings, Some(false), 10);
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|l| !l.hq));
        assert_eq!(result[0].price_per_unit, 50);
    }

    #[test]
    fn top_n_cheapest_filter_applied_after_sort_so_limit_counts_filtered_results() {
        // Cheapest 3 are NQ; HQ are 200 and 300. Asking for top 5 HQ should still only return 2.
        let listings = vec![
            listing(1, 10, false),
            listing(2, 20, false),
            listing(3, 30, false),
            listing(4, 200, true),
            listing(5, 300, true),
        ];
        let result = top_n_cheapest_listings(listings, Some(true), 5);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].price_per_unit, 200);
    }

    #[test]
    fn top_n_cheapest_handles_ties_in_price() {
        let listings = vec![
            listing(1, 100, false),
            listing(2, 100, true),
            listing(3, 100, false),
        ];
        let result = top_n_cheapest_listings(listings, None, 10);
        assert_eq!(result.len(), 3);
        assert!(result.iter().all(|l| l.price_per_unit == 100));
    }
}
