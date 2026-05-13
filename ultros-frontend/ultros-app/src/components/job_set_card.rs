//! Compact card that stands in for a whole gear set on the Job Sets
//! view. Renders the slot icons in a grid plus NQ/HQ "cost to buy
//! the set" totals, summed from the cheapest listings in the user's
//! current price zone (region/DC/world). Clicking the card navigates
//! to the per-set detail page.

use leptos::prelude::*;
use leptos_router::components::A;
use ultros_api_types::cheapest_listings::CheapestListingsMap;

use crate::components::gil::Gil;
use crate::components::item_icon::{IconSize, ItemIcon};
use crate::components::job_set_grouping::JobSetGroup;
use crate::global_state::cheapest_prices::CheapestPrices;
use crate::i18n::*;

/// Sum the cheapest price in the active zone across all items in the
/// set. `hq_only` picks the HQ price only; otherwise we take the
/// lower of NQ/HQ (matching the "buy this slot at the best price"
/// intent). Returns `None` if no item in the set has any listing —
/// callers render a placeholder dash in that case.
fn set_total(group: &JobSetGroup, prices: &CheapestListingsMap, hq_only: bool) -> Option<i64> {
    let mut total: i64 = 0;
    let mut had_any = false;
    for item in &group.items {
        let summary = prices.find_matching_listings(item.id.0);
        let price = if hq_only {
            summary.hq.map(|hq| hq.price)
        } else {
            summary.lowest_gil()
        };
        if let Some(p) = price {
            total += p as i64;
            had_any = true;
        }
    }
    had_any.then_some(total)
}

/// Stable URL slug for the per-set detail page. We key on `ilvl`
/// rather than the (locale-dependent) stem so links survive language
/// switches and so deep-links don't break if the prefix detection
/// changes in a future patch.
fn detail_href(jobset: &str, group: &JobSetGroup) -> String {
    format!(
        "/items/jobset/{}/set/{}",
        jobset.replace('/', "%2F"),
        group.ilvl
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::job_set_grouping::GroupableItem;
    use std::collections::HashMap;
    use ultros_api_types::cheapest_listings::{
        CheapestListingData, CheapestListingMapKey, CheapestListingsMap,
    };
    use xiv_gen::ItemId;

    fn item(id: i32, name: &str) -> GroupableItem {
        GroupableItem {
            id: ItemId(id),
            name: name.to_string(),
            ilvl: 770,
        }
    }

    fn listing(price: i32) -> CheapestListingData {
        CheapestListingData { price, world_id: 1 }
    }

    fn map_with(rows: &[(i32, bool, i32)]) -> CheapestListingsMap {
        let mut map = HashMap::new();
        for (item_id, hq, price) in rows {
            map.insert(
                CheapestListingMapKey {
                    hq: *hq,
                    item_id: *item_id,
                },
                listing(*price),
            );
        }
        CheapestListingsMap { map }
    }

    #[test]
    fn detail_href_uses_ilvl_as_stable_slug() {
        // The slug is the iLvl, not the locale-dependent stem, so
        // language switches don't break deep-links.
        let group = JobSetGroup {
            stem: "Courtly Lover's".to_string(),
            ilvl: 770,
            items: vec![item(1, "Courtly Lover's Sword")],
        };
        assert_eq!(detail_href("PLD", &group), "/items/jobset/PLD/set/770");
    }

    #[test]
    fn detail_href_percent_encodes_slash_in_jobset() {
        // Defensive — at the call-site `jobset` is the path param
        // straight out of the router. A literal `/` here would break
        // the route, so we encode it the same way the existing
        // explorer links do.
        let group = JobSetGroup {
            stem: "x".to_string(),
            ilvl: 1,
            items: vec![item(1, "x")],
        };
        assert_eq!(detail_href("a/b", &group), "/items/jobset/a%2Fb/set/1");
    }

    #[test]
    fn set_total_sums_lowest_per_slot_when_not_hq_only() {
        let group = JobSetGroup {
            stem: "x".to_string(),
            ilvl: 770,
            items: vec![item(1, "a"), item(2, "b")],
        };
        // Item 1: NQ 100, HQ 200 -> takes 100.
        // Item 2: NQ none, HQ 50 -> takes 50.
        let prices = map_with(&[(1, false, 100), (1, true, 200), (2, true, 50)]);
        assert_eq!(set_total(&group, &prices, false), Some(150));
    }

    #[test]
    fn set_total_hq_only_ignores_nq_listings() {
        let group = JobSetGroup {
            stem: "x".to_string(),
            ilvl: 770,
            items: vec![item(1, "a"), item(2, "b")],
        };
        // Only HQ listings count when hq_only=true. Item 2 has no
        // HQ listing, so it contributes 0 to the partial total but
        // the result is still Some(...) because item 1 had one.
        let prices = map_with(&[(1, false, 100), (1, true, 250), (2, false, 50)]);
        assert_eq!(set_total(&group, &prices, true), Some(250));
    }

    #[test]
    fn set_total_returns_none_when_no_item_has_a_listing() {
        let group = JobSetGroup {
            stem: "x".to_string(),
            ilvl: 770,
            items: vec![item(1, "a"), item(2, "b")],
        };
        let prices = map_with(&[]);
        assert_eq!(set_total(&group, &prices, false), None);
        assert_eq!(set_total(&group, &prices, true), None);
    }

    #[test]
    fn set_total_handles_partial_coverage() {
        // Only one of three items has a listing — total still
        // returns Some, summed over what's known. The UI labels
        // this with a tooltip in the detail page; the card just
        // shows the partial number so the user has something to
        // act on.
        let group = JobSetGroup {
            stem: "x".to_string(),
            ilvl: 770,
            items: vec![item(1, "a"), item(2, "b"), item(3, "c")],
        };
        let prices = map_with(&[(2, false, 75)]);
        assert_eq!(set_total(&group, &prices, false), Some(75));
    }
}

#[component]
pub fn JobSetCard(group: JobSetGroup, jobset: String) -> impl IntoView {
    let i18n = use_i18n();
    let cheapest_prices = use_context::<CheapestPrices>();
    let read_listings = cheapest_prices.map(|c| c.read_listings);

    let group_for_view = group.clone();
    let group_for_total = group.clone();
    let group_for_href = group.clone();
    let stem = group.stem.clone();
    let item_count = group.items.len();
    let ilvl = group.ilvl;
    let href = detail_href(&jobset, &group_for_href);

    let totals = Memo::new(move |_| {
        let Some(listings) = read_listings else {
            return (None, None);
        };
        listings.with(|data| match data {
            Some(Ok(map)) => (
                set_total(&group_for_total, map, false),
                set_total(&group_for_total, map, true),
            ),
            _ => (None, None),
        })
    });

    let total_nq = move || totals.with(|(nq, _)| *nq);
    let total_hq = move || totals.with(|(_, hq)| *hq);

    view! {
        <div class="group relative flex flex-col p-4 rounded-xl panel
                    border border-white/5 hover:border-brand-500/30
                    hover:shadow-lg hover:shadow-brand-500/5
                    transition-all duration-300">
            <div class="flex flex-row items-start justify-between gap-3 mb-3">
                <div class="flex flex-col min-w-0">
                    <div class="flex items-center gap-2 mb-1.5 flex-wrap">
                        <span class="text-xs font-bold px-1.5 py-0.5 rounded bg-white/10 text-[color:var(--color-text-muted)] whitespace-nowrap">
                            {t!(i18n, item_explorer_ilvl_prefix)} " " {ilvl}
                        </span>
                        <span class="text-xs px-1.5 py-0.5 rounded bg-white/5 text-[color:var(--color-text-muted)] whitespace-nowrap">
                            {move || t_string!(i18n, job_set_card_pieces).to_string().replace("%count%", &item_count.to_string())}
                        </span>
                    </div>
                    <A
                        href=href.clone()
                        attr:class="font-bold text-base leading-snug text-[color:var(--color-text)] \
                                   group-hover:text-brand-300 transition-colors line-clamp-2 \
                                   hover:underline decoration-brand-300/30 underline-offset-4"
                    >
                        {stem}
                    </A>
                </div>
            </div>

            <div class="grid grid-cols-6 sm:grid-cols-5 gap-1.5 mb-3">
                {group_for_view.items.iter().map(|item| {
                    let id = item.id.0;
                    let title = item.name.clone();
                    view! {
                        <A
                            href=format!("/item/{}", id)
                            attr:class="flex items-center justify-center aspect-square rounded bg-white/5 hover:bg-white/10 \
                                       border border-white/5 hover:border-brand-500/30 \
                                       transition-colors p-0.5"
                            attr:title=title
                        >
                            <ItemIcon item_id=id icon_size=IconSize::Small />
                        </A>
                    }
                }).collect::<Vec<_>>()}
            </div>

            <div class="flex-1" />
            <div class="flex flex-col gap-2 mt-2 pt-3 border-t border-white/5 text-sm">
                <div class="flex flex-col">
                    <span class="text-xs text-[color:var(--color-text-muted)] uppercase tracking-wider mb-0.5">
                        {t!(i18n, job_set_card_total_nq)}
                    </span>
                    <div class="flex flex-row items-center gap-1.5">
                        {move || match total_nq() {
                            Some(t) => view! { <Gil amount=t as i32 /> }.into_any(),
                            None => view! { <span class="text-[color:var(--color-text-muted)]">"—"</span> }.into_any(),
                        }}
                    </div>
                </div>
                <div class="flex flex-col">
                    <span class="text-xs text-[color:var(--color-text-muted)] uppercase tracking-wider mb-0.5">
                        {t!(i18n, job_set_card_total_hq)}
                    </span>
                    <div class="flex flex-row items-center gap-1.5">
                        {move || match total_hq() {
                            Some(t) => view! { <Gil amount=t as i32 /> }.into_any(),
                            None => view! { <span class="text-[color:var(--color-text-muted)]">"—"</span> }.into_any(),
                        }}
                    </div>
                </div>
                <A
                    href=href
                    attr:class="mt-1 inline-flex items-center justify-center text-xs font-medium px-3 py-1.5 rounded-lg \
                               bg-brand-500/10 hover:bg-brand-500/20 text-brand-300 \
                               border border-brand-500/20 transition-colors"
                >
                    {t!(i18n, job_set_card_view_details)}
                </A>
            </div>
        </div>
    }
    .into_any()
}
