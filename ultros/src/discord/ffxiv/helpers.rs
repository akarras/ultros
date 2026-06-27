//! Pure helpers extracted from the Discord command handlers so they can be unit-tested
//! without spinning up Serenity, Poise, the DB, or the world cache.

use anyhow::anyhow;
use ultros_api_types::world_helper::AnySelector;
use ultros_db::entity::active_listing;
use xiv_gen::{ItemId, Language};

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

/// Map Discord's locale string (e.g. "ja", "de", "zh-CN") to the closest
/// `xiv_gen::Language`. Falls back to English for unrecognized or missing locales.
///
/// Discord's documented locale codes:
/// <https://discord.com/developers/docs/reference#locales>
pub(crate) fn discord_locale_to_xiv_language(locale: Option<&str>) -> Language {
    match locale.unwrap_or("") {
        "ja" => Language::Ja,
        "de" => Language::De,
        "fr" => Language::Fr,
        "zh-CN" => Language::Cn,
        "ko" => Language::Ko,
        "zh-TW" => Language::Tc,
        _ => Language::En,
    }
}

/// Look up an item id by name across every supported locale. Matches the lowercased
/// name exactly; returns the first locale that contains the name. Used so users can
/// paste a localized item name from anywhere on the site and have the bot resolve it.
pub(crate) fn resolve_item_id_any_locale(name: &str) -> Option<i32> {
    if let Some(id) = name.parse::<i32>().ok().filter(|id| {
        xiv_gen_db::data_for(Language::En)
            .items
            .contains_key(&ItemId(*id))
    }) {
        return Some(id);
    }
    let lowered = name.to_lowercase();
    for (_, data) in xiv_gen_db::all_locales() {
        if let Some((ItemId(id), _)) = data
            .items
            .iter()
            .find(|(_, item)| item.name.to_lowercase() == lowered)
        {
            return Some(*id);
        }
    }
    None
}

/// Truncate a string to at most 100 Unicode characters. Discord's autocomplete
/// choice names and values must be between 1 and 100 characters in length.
pub(crate) fn truncate_100(s: &str) -> String {
    if s.chars().count() <= 100 {
        s.to_string()
    } else {
        s.chars().take(100).collect()
    }
}

/// A single autocomplete suggestion: a display label (localized into the user's
/// language when possible) and the item id it resolves to. Returned by
/// [`localized_item_matches`] and used to build the two flavors of Discord
/// autocomplete (string-valued and integer-valued).
pub(crate) struct LocalizedItemMatch {
    pub label: String,
    pub item_id: i32,
}

/// Search every supported locale for items whose name contains `partial`. Each
/// matched item appears at most once: when more than one locale matched, the
/// user's locale wins for the display name; otherwise the first locale that
/// matched wins. If the localized display differs from the English name, the
/// English name is appended in parentheses so the user can confirm the item.
pub(crate) fn localized_item_matches(
    partial: &str,
    user_lang: Language,
) -> Vec<LocalizedItemMatch> {
    let needle = partial.to_lowercase();
    let en = xiv_gen_db::data_for(Language::En);

    let mut seen: std::collections::HashMap<i32, (Language, String)> =
        std::collections::HashMap::new();
    for (lang, data) in xiv_gen_db::all_locales() {
        for (ItemId(id), item) in &data.items {
            if item.name.is_empty() || !name_matches_lowered(&item.name, &needle) {
                continue;
            }
            seen.entry(*id)
                .and_modify(|(existing_lang, existing_name)| {
                    if *existing_lang != user_lang && lang == user_lang {
                        *existing_lang = lang;
                        *existing_name = item.name.to_string();
                    }
                })
                .or_insert_with(|| (lang, item.name.to_string()));
        }
    }

    let mut out: Vec<LocalizedItemMatch> = seen
        .into_iter()
        .filter_map(|(id, (_, display))| {
            let en_name = en.items.get(&ItemId(id))?.name.to_string();
            if en_name.is_empty() {
                return None;
            }
            let label = truncate_100(&if display == en_name {
                en_name
            } else {
                format!("{display} ({en_name})")
            });
            Some(LocalizedItemMatch { label, item_id: id })
        })
        .collect();
    out.sort_by(|a, b| a.label.cmp(&b.label));
    out.truncate(99);
    out
}

/// Discord autocomplete handler for item-name string args. Searches every supported
/// locale for substring matches; deduplicates by item id; prefers the user's locale
/// for display when the same item matched in multiple locales. The choice value is
/// the FFXIV item id stringified so that [`resolve_item_id`] can always find it
/// regardless of whether the display name was truncated to 100 characters.
pub(crate) async fn autocomplete_item<'a>(
    ctx: Context<'_>,
    partial: &'a str,
) -> impl Iterator<Item = poise::serenity_prelude::AutocompleteChoice> + 'a {
    let user_lang = discord_locale_to_xiv_language(ctx.locale());
    localized_item_matches(partial, user_lang)
        .into_iter()
        .map(move |m| {
            poise::serenity_prelude::AutocompleteChoice::new(m.label, m.item_id.to_string())
        })
}

/// Resolve a user-supplied item name (case-insensitive exact match) to an FFXIV item id.
/// Returns `None` if no item with that exact name exists in any supported locale.
pub(crate) fn resolve_item_id(name: &str) -> Option<i32> {
    resolve_item_id_any_locale(name)
}

/// Return the localized display name for an item id, falling back to the English name,
/// then to the empty string if the item is unknown.
pub(crate) fn localized_item_name(item_id: i32, lang: Language) -> String {
    let id = ItemId(item_id);
    if let Some(item) = xiv_gen_db::data_for(lang).items.get(&id)
        && !item.name.is_empty()
    {
        return item.name.to_string();
    }
    xiv_gen_db::data_for(Language::En)
        .items
        .get(&id)
        .map(|i| i.name.to_string())
        .unwrap_or_default()
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

    // ---------- truncate_100 ----------

    #[test]
    fn truncate_100_passes_short_strings_through() {
        assert_eq!(truncate_100("short"), "short");
        assert_eq!(truncate_100(""), "");
    }

    #[test]
    fn truncate_100_truncates_long_ascii() {
        let long = "a".repeat(110);
        let result = truncate_100(&long);
        assert_eq!(result.len(), 100);
        assert_eq!(result, "a".repeat(100));
    }

    #[test]
    fn truncate_100_handles_unicode_correctly() {
        // Each of these is 1 character but multiple bytes
        let crab = "🦀";
        let long_crabs = crab.repeat(110);
        let result = truncate_100(&long_crabs);
        assert_eq!(result.chars().count(), 100);
        assert_eq!(result, crab.repeat(100));
    }

    #[test]
    fn truncate_100_at_boundary() {
        let exactly_100 = "b".repeat(100);
        assert_eq!(truncate_100(&exactly_100), exactly_100);
    }

    #[test]
    fn test_localized_item_matches_truncation() {
        let results = localized_item_matches("Torn from the Heavens", Language::Fr);
        let m = results
            .iter()
            .find(|m| m.item_id == 31681)
            .expect("should find the medley orchestrion roll");

        assert!(m.label.chars().count() <= 100);
    }

    #[test]
    fn test_resolve_item_id_any_locale_with_string_id() {
        assert_eq!(resolve_item_id_any_locale("31681"), Some(31681));
        assert_eq!(resolve_item_id_any_locale("not-an-id"), None);
    }
}
