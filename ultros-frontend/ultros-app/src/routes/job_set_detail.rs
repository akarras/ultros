//! Per-set detail page at `/items/jobset/:jobset/set/:ilvl`. Filters
//! the job's equippable items to the one [`JobSetGroup`] whose iLvl
//! matches the route, then renders a per-slot price grid plus side-
//! by-side totals for the user's current price zone and their home
//! world.

use std::collections::HashSet;

use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;
use ultros_api_types::cheapest_listings::CheapestListingsMap;
use xiv_gen::ClassJobCategoryId;

use crate::CheapestPrices;
use crate::api::get_cheapest_listings;
use crate::components::cheapest_price::CheapestPrice;
use crate::components::gil::Gil;
use crate::components::item_icon::{IconSize, ItemIcon};
use crate::components::job_set_grouping::{GroupableItem, JobSetGroup, group_into_sets};
use crate::components::meta::{MetaDescription, MetaTitle};
use crate::global_state::home_world::use_home_world;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use crate::routes::item_explorer::job_category_lookup;

/// Sum the cheapest-of-(NQ,HQ) price across every item in the set
/// using the given listings map. Mirrors the helper in `JobSetCard`
/// so the card-level total and the detail-page total stay in sync.
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

#[component]
pub fn JobSetDetail() -> impl IntoView {
    let i18n = use_i18n();
    let params = use_params_map();
    let data = tracked_data();
    let (home_world, _) = use_home_world();

    // Resolve the job acronym from the route, same as `JobItems` does.
    let canonical_abbr = Memo::new(move |_| {
        let raw = params().get("jobset").map(|s| s.to_string())?;
        let decoded = percent_encoding::percent_decode_str(&raw)
            .decode_utf8()
            .map(|s| s.to_string())
            .unwrap_or_else(|_| raw.clone());
        let lower = decoded.to_lowercase();
        Some(
            data.class_jobs
                .iter()
                .find_map(|(_id, job)| {
                    let abbr = job.abbreviation.as_str();
                    let name = job.name.as_str();
                    if abbr.eq_ignore_ascii_case(&lower) || name.eq_ignore_ascii_case(&lower) {
                        Some(abbr.to_string())
                    } else {
                        None
                    }
                })
                .unwrap_or(decoded),
        )
    });

    let target_ilvl = Memo::new(move |_| {
        params()
            .get("ilvl")
            .as_ref()
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(0)
    });

    let group = Memo::new(move |_| {
        let abbr = canonical_abbr.get()?;
        let job_categories: HashSet<_> = data
            .class_job_categorys
            .iter()
            .filter(|(_id, c)| job_category_lookup(c, &abbr))
            .map(|(id, _)| *id)
            .collect();

        let projections: Vec<GroupableItem> = data
            .items
            .iter()
            .filter(|(_, item)| {
                job_categories.contains(&ClassJobCategoryId(item.class_job_category))
            })
            .filter(|(_, item)| item.level_item > 0)
            .map(|(id, item)| GroupableItem {
                id: *id,
                name: item.name.clone(),
                ilvl: item.level_item,
            })
            .collect();

        let (groups, _ungrouped) = group_into_sets(projections);
        groups.into_iter().find(|g| g.ilvl == target_ilvl.get())
    });

    // Home-world-only listings: a separate Resource keyed off the
    // user's `use_home_world` selection. Lives behind a Suspense so
    // SSR works without forcing us to wait for the user's cookie to
    // flush. Returns `None` when the cookie isn't set, which the
    // view renders as a "—" placeholder.
    let home_world_listings = Resource::new(
        move || home_world.get().map(|w| w.name),
        move |world_name| async move {
            let world_name = world_name?;
            get_cheapest_listings(&world_name)
                .await
                .ok()
                .map(CheapestListingsMap::from)
        },
    );

    // Default-zone listings already live in app context — reuse them.
    let cheapest_prices = use_context::<CheapestPrices>();
    let default_zone_listings = cheapest_prices.map(|p| p.read_listings);

    let default_total = Memo::new(move |_| {
        let g = group.get()?;
        let listings = default_zone_listings?;
        listings.with(|data| match data {
            Some(Ok(map)) => set_total(&g, map, false),
            _ => None,
        })
    });

    let home_total = Memo::new(move |_| {
        let g = group.get()?;
        home_world_listings
            .get()
            .flatten()
            .as_ref()
            .and_then(|map| set_total(&g, map, false))
    });

    let set_stem = Memo::new(move |_| group.get().map(|g| g.stem).unwrap_or_default());
    let job_name = Memo::new(move |_| {
        canonical_abbr
            .get()
            .unwrap_or_else(|| t_string!(i18n, job_set_default).to_string())
    });
    let back_href = Memo::new(move |_| {
        format!(
            "/items/jobset/{}",
            params()
                .get("jobset")
                .as_ref()
                .map(|s| s.to_string())
                .unwrap_or_default()
        )
    });

    view! {
        <MetaTitle title=move || t_string!(i18n, job_set_detail_title).to_string().replace("%set%", &set_stem()) />
        <MetaDescription text=move || t_string!(i18n, job_set_detail_desc).to_string().replace("%set%", &set_stem()) />

        <div class="flex flex-col gap-4">
            <div class="flex flex-row items-center gap-3">
                <A
                    href=back_href
                    attr:class="text-xs font-bold uppercase tracking-wider px-3 py-1.5 rounded-lg \
                               bg-white/5 hover:bg-white/10 text-[color:var(--color-text-muted)] \
                               border border-white/5 transition-colors"
                >
                    {move || t_string!(i18n, job_set_detail_back).to_string().replace("%job%", &job_name())}
                </A>
            </div>

            <div class="flex flex-row items-baseline gap-3 flex-wrap">
                <h3 class="text-2xl font-bold">{set_stem}</h3>
                <span class="text-xs font-bold px-1.5 py-0.5 rounded bg-white/10 text-[color:var(--color-text-muted)] whitespace-nowrap">
                    {t!(i18n, item_explorer_ilvl_prefix)} " " {move || target_ilvl.get()}
                </span>
            </div>

            // Per-slot grid, every piece in the set with its NQ/HQ
            // cheapest from the user's active price zone.
            {move || match group.get() {
                None => view! { <div class="text-[color:var(--color-text-muted)] italic">"—"</div> }.into_any(),
                Some(g) => {
                    view! {
                        <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-4 gap-3">
                            {g.items.into_iter().map(|item| {
                                let item_id = item.id.0;
                                let item_name = item.name.clone();
                                view! {
                                    <div class="flex flex-col p-3 rounded-lg panel border border-white/5">
                                        <div class="flex flex-row items-center gap-3 mb-2">
                                            <A
                                                href=format!("/item/{}", item_id)
                                                attr:class="shrink-0"
                                            >
                                                <ItemIcon item_id=item_id icon_size=IconSize::Medium />
                                            </A>
                                            <A
                                                href=format!("/item/{}", item_id)
                                                attr:class="font-medium text-sm leading-snug \
                                                           hover:text-brand-300 transition-colors line-clamp-2"
                                            >
                                                {item_name}
                                            </A>
                                        </div>
                                        <div class="flex flex-col gap-1.5 mt-1 pt-2 border-t border-white/5 text-sm">
                                            <CheapestPrice item_id=xiv_gen::ItemId(item_id) show_hq=false label=t_string!(i18n, nq).to_string() />
                                            <CheapestPrice item_id=xiv_gen::ItemId(item_id) show_hq=true label=t_string!(i18n, hq).to_string() />
                                        </div>
                                    </div>
                                }
                            }).collect::<Vec<_>>()}
                        </div>
                    }
                    .into_any()
                }
            }}

            // Side-by-side totals. The default-zone column uses the
            // shared CheapestPrices resource; the home-world column
            // fetches its own listings keyed on `use_home_world()`.
            <div class="grid grid-cols-1 md:grid-cols-2 gap-4 mt-4">
                <div class="panel p-4 rounded-xl border border-white/5">
                    <div class="text-xs font-bold uppercase tracking-wider text-[color:var(--color-text-muted)] mb-1">
                        {t!(i18n, job_set_detail_set_total)}
                    </div>
                    <div class="text-xl font-bold">
                        {move || match default_total.get() {
                            Some(t) => view! { <Gil amount=t as i32 /> }.into_any(),
                            None => view! { <span class="text-[color:var(--color-text-muted)]">"—"</span> }.into_any(),
                        }}
                    </div>
                </div>
                <div class="panel p-4 rounded-xl border border-white/5">
                    <div class="text-xs font-bold uppercase tracking-wider text-[color:var(--color-text-muted)] mb-1">
                        {t!(i18n, job_set_detail_home_world_total)}
                    </div>
                    <div class="text-xl font-bold">
                        <Suspense fallback=move || view! { <span class="text-[color:var(--color-text-muted)]">"…"</span> }>
                            {move || match home_total.get() {
                                Some(t) => view! { <Gil amount=t as i32 /> }.into_any(),
                                None => view! { <span class="text-[color:var(--color-text-muted)]">"—"</span> }.into_any(),
                            }}
                        </Suspense>
                    </div>
                </div>
            </div>
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn map_with(rows: &[(i32, bool, i32)]) -> CheapestListingsMap {
        let mut map = HashMap::new();
        for (item_id, hq, price) in rows {
            map.insert(
                CheapestListingMapKey {
                    hq: *hq,
                    item_id: *item_id,
                },
                CheapestListingData {
                    price: *price,
                    world_id: 1,
                },
            );
        }
        CheapestListingsMap { map }
    }

    #[test]
    fn detail_set_total_picks_lowest_of_nq_and_hq_per_item() {
        // Same contract the JobSetCard total uses; keeping a copy
        // of the test here means the detail-page math is independently
        // covered if the helpers ever drift.
        let group = JobSetGroup {
            stem: "x".to_string(),
            ilvl: 770,
            items: vec![item(1, "a"), item(2, "b")],
        };
        let prices = map_with(&[(1, false, 100), (1, true, 200), (2, true, 50)]);
        assert_eq!(set_total(&group, &prices, false), Some(150));
    }

    #[test]
    fn detail_set_total_none_when_map_is_empty() {
        let group = JobSetGroup {
            stem: "x".to_string(),
            ilvl: 770,
            items: vec![item(1, "a")],
        };
        assert_eq!(set_total(&group, &map_with(&[]), false), None);
    }
}
