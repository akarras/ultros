//! Per-set detail page at `/items/jobset/:jobset/set/:ilvl`. Filters
//! the job's equippable items to the one [`JobSetGroup`] whose iLvl
//! matches the route, then renders a per-slot price grid plus side-
//! by-side totals for the user's current price zone and their home
//! world.

use std::collections::{BTreeMap, HashSet};

use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;
use ultros_api_types::cheapest_listings::CheapestListingsMap;
use xiv_gen::{ClassJobCategoryId, ItemId};

use crate::CheapestPrices;
use crate::api::get_cheapest_listings;
use crate::components::add_set_to_list::AddSetToList;
use crate::components::cheapest_price::CheapestPrice;
use crate::components::crafting_cost::IngredientsIter;
use crate::components::gil::{Gil, GilOrDash};
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

/// Sum the cheapest NQ unit price across `materials`, multiplied by the
/// per-entry amount. `include_shards` controls whether crystal/shard
/// rows contribute (the UI exposes a toggle so the user can see the
/// "ingredient cost minus crystals" total they actually buy from
/// market). Returns `None` when no material has a listing, so the UI
/// can render a `—` placeholder instead of a misleading 0.
pub(crate) fn materials_total(
    materials: &[MaterialEntry],
    prices: &CheapestListingsMap,
    include_shards: bool,
) -> Option<i64> {
    let mut total: i64 = 0;
    let mut had_any = false;
    for m in materials {
        if !include_shards && m.is_shard {
            continue;
        }
        let summary = prices.find_matching_listings(m.id.0);
        if let Some(unit) = summary.lowest_gil() {
            total += unit as i64 * m.amount as i64;
            had_any = true;
        }
    }
    had_any.then_some(total)
}

/// English-name heuristic for the slot a piece of gear occupies. Used
/// to label each tile on the detail page so a reader doesn't have to
/// squint at the icon. Returns `None` when the name doesn't match a
/// known pattern (non-English localisations, novelty items) — the
/// caller hides the chip rather than printing a misleading label.
///
/// Patterns are checked most-specific-first so accessory keywords don't
/// swallow matching armour names (e.g. "Temple Chain of Striking" is a
/// helmet despite the "chain" keyword).
pub(crate) fn slot_label_from_name(name: &str) -> Option<&'static str> {
    let lower = name.to_lowercase();

    // Two-handed weapons / off-hands first — "shield" is unambiguous.
    if lower.contains("shield") {
        return Some("OFF-HAND");
    }

    // Head pieces. Temple Chain is a circlet-style helm in modern
    // FFXIV sets, so check it before any neck patterns.
    const HEAD: &[&str] = &[
        "temple chain",
        "hairpin",
        "helm",
        "hat",
        "cap",
        "crown",
        "coronet",
        "visor",
        "mask",
        "turban",
        "hood",
        "circlet",
        "headgear",
        "spectacles",
        "goggles",
        "tiara",
    ];
    if HEAD.iter().any(|p| lower.contains(p)) {
        return Some("HEAD");
    }

    // Chest pieces. "Cloak of Striking" is a chest piece in Dawntrail
    // crafted sets (it's the body slot, not a back/shoulder item).
    const CHEST: &[&str] = &[
        "surcoat", "cuirass", "robe", "jerkin", "doublet", "tunic", "armor", "cloak", "smock",
        "jacket", "vest", "kurta", "haori", "top",
    ];
    if CHEST.iter().any(|p| lower.contains(p)) || lower.contains(" coat") || lower.ends_with("coat")
    {
        return Some("CHEST");
    }

    const HANDS: &[&str] = &[
        "gauntlet",
        "glove",
        "mitt",
        "armguard",
        "vambrace",
        "halfgloves",
    ];
    if HANDS.iter().any(|p| lower.contains(p)) {
        return Some("HANDS");
    }

    const LEGS: &[&str] = &[
        "trousers",
        "breeches",
        "brais",
        "tights",
        "hose",
        "slops",
        "tassets",
        "kecks",
        "bottoms",
        "pantaloons",
        "skirt",
    ];
    if LEGS.iter().any(|p| lower.contains(p)) {
        return Some("LEGS");
    }

    const FEET: &[&str] = &[
        "boots",
        "sabatons",
        "sandals",
        "greaves",
        "shoes",
        "crakows",
        "highboots",
    ];
    if FEET.iter().any(|p| lower.contains(p)) {
        return Some("FEET");
    }

    if lower.contains("earring") {
        return Some("EAR");
    }
    if lower.contains("choker")
        || lower.contains("necklace")
        || lower.contains("gorget")
        || lower.contains("neckband")
    {
        return Some("NECK");
    }
    if lower.contains("bracelet")
        || lower.contains("wristlet")
        || lower.contains("wristband")
        || lower.contains("armillae")
    {
        return Some("WRIST");
    }
    if lower.contains("ring") && !lower.contains("earring") {
        return Some("RING");
    }

    // Anything else with a weapon-shaped keyword. Kept last because the
    // armour patterns above are higher-specificity.
    const WEAPONS: &[&str] = &[
        "sword",
        "blade",
        "labrys",
        "axe",
        "partisan",
        "spear",
        "longbow",
        "bow",
        "cane",
        "scepter",
        "staff",
        "rod",
        "index",
        "codex",
        "tome",
        "grimoire",
        "pistol",
        "musketoon",
        "knuckles",
        "baghnakhs",
        "claws",
        "katana",
        "wakizashi",
        "tachi",
        "saber",
        "dagger",
        "war scythe",
        "scythe",
        "rapier",
        "gunblade",
        "war quoits",
        "twinfangs",
        "filbert brush",
        "brush",
        "pendulums",
        "astrometer",
        "globe",
        "orrery",
    ];
    if WEAPONS.iter().any(|p| lower.contains(p)) {
        return Some("WEAPON");
    }

    None
}

/// Pick the gear set at `target_ilvl` for a job, using the same item
/// filters the parent `JobItems` route applies by default (market-listable,
/// non-zero iLvl). Without the market filter, non-market items at the
/// same iLvl (raid drops, quest rewards) pollute the bucket and break
/// the LCP-based grouping — that's the bug where `/jobset/SAM/set/770`
/// rendered empty even though the parent page showed the card.
pub(crate) fn find_set_for_job<'a, I, F>(
    items: I,
    is_job_match: F,
    target_ilvl: i32,
) -> Option<JobSetGroup>
where
    I: IntoIterator<Item = &'a xiv_gen::Item>,
    F: Fn(&xiv_gen::Item) -> bool,
{
    let mut projections: Vec<GroupableItem> = items
        .into_iter()
        .filter(|item| is_job_match(item))
        .filter(|item| item.level_item == target_ilvl)
        .filter(|item| item.item_search_category > 0)
        .map(|item| GroupableItem {
            id: item.key_id,
            name: item.name.clone(),
            ilvl: item.level_item,
        })
        .collect();
    projections.sort_by(|a, b| {
        a.ilvl
            .cmp(&b.ilvl)
            .then(a.name.cmp(&b.name))
            .then(a.id.0.cmp(&b.id.0))
    });

    let (groups, _) = group_into_sets(projections);
    groups.into_iter().next()
}

/// Aggregated crafting material entry across every craftable item in
/// the set. `amount` is the sum of `recipe.amount_ingredient` across
/// every recipe whose `item_result` lands in the set, so the user sees
/// "the total stack I need to buy/farm to make every piece."
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MaterialEntry {
    pub id: ItemId,
    pub name: String,
    pub amount: i32,
    /// True for crystal/shard/cluster (item_search_category == 59) —
    /// the UI groups these visually since they're cheap and not really
    /// part of the "ingredient shopping list" most users care about.
    pub is_shard: bool,
}

/// Sum ingredients across every recipe in `recipes` whose output
/// item_result is one of the items in `set`. Returns entries sorted
/// non-shards first, then by descending total amount so the busiest
/// material rises to the top.
pub(crate) fn aggregate_materials(
    set: &JobSetGroup,
    recipes: &std::collections::HashMap<xiv_gen::RecipeId, xiv_gen::Recipe>,
    items: &std::collections::HashMap<ItemId, xiv_gen::Item>,
) -> Vec<MaterialEntry> {
    let set_ids: HashSet<i32> = set.items.iter().map(|i| i.id.0).collect();
    let mut totals: BTreeMap<i32, i32> = BTreeMap::new();
    for recipe in recipes.values() {
        if !set_ids.contains(&recipe.item_result) {
            continue;
        }
        for (id, amount) in IngredientsIter::new(recipe) {
            *totals.entry(id.0).or_insert(0) += amount;
        }
    }
    let mut entries: Vec<MaterialEntry> = totals
        .into_iter()
        .filter_map(|(id, amount)| {
            let item = items.get(&ItemId(id))?;
            Some(MaterialEntry {
                id: ItemId(id),
                name: item.name.clone(),
                amount,
                is_shard: item.item_search_category == 59,
            })
        })
        .collect();
    entries.sort_by(|a, b| {
        a.is_shard
            .cmp(&b.is_shard)
            .then(b.amount.cmp(&a.amount))
            .then(a.name.cmp(&b.name))
    });
    entries
}

/// Compact row used by both the main materials grid and the shards
/// section. Inlines an icon, name, quantity, and cheapest NQ price so
/// the user can eyeball "how much will this set cost in ingredients?"
fn material_row(m: MaterialEntry) -> impl IntoView {
    let id = m.id.0;
    let name = m.name.clone();
    let amount = m.amount;
    view! {
        <A
            href=format!("/item/{}", id)
            attr:class="group flex flex-row items-center gap-2 p-2 rounded-lg panel \
                       border border-white/5 hover:border-brand-500/30 transition-colors"
        >
            <div class="shrink-0 flex items-center justify-center w-8 h-8">
                <ItemIcon item_id=id icon_size=IconSize::Small />
            </div>
            <div class="flex flex-col min-w-0 flex-1">
                <span class="font-medium text-xs leading-snug line-clamp-1 group-hover:text-brand-300 transition-colors">
                    {name}
                </span>
                <div class="flex flex-row items-center gap-1.5 text-[10px] text-[color:var(--color-text-muted)]">
                    <span>"× "{amount}</span>
                    <span>"•"</span>
                    <CheapestPrice item_id=xiv_gen::ItemId(id) show_hq=false />
                </div>
            </div>
        </A>
    }
    .into_any()
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

        find_set_for_job(
            data.items.values(),
            |item| job_categories.contains(&ClassJobCategoryId(item.class_job_category)),
            target_ilvl.get(),
        )
    });

    let materials = Memo::new(move |_| {
        let g = group.get()?;
        Some(aggregate_materials(&g, &data.recipes, &data.items))
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

    let set_stem = Signal::derive(move || group.get().map(|g| g.stem).unwrap_or_default());
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

    // Add-to-list payloads. Set pieces: every item in the group at qty
    // 1. Materials: every aggregated (non-zero) ingredient at its summed
    // amount. Each is a Signal so the modal can snapshot it on open.
    let set_entries: Signal<Vec<(ItemId, i32)>> = Signal::derive(move || {
        group
            .get()
            .map(|g| g.items.into_iter().map(|i| (i.id, 1)).collect())
            .unwrap_or_default()
    });
    let material_entries: Signal<Vec<(ItemId, i32)>> = Signal::derive(move || {
        materials
            .get()
            .map(|ms| ms.into_iter().map(|m| (m.id, m.amount)).collect())
            .unwrap_or_default()
    });
    let has_materials = Memo::new(move |_| materials.get().is_some_and(|m| !m.is_empty()));

    // Defer the price-resource-driven materials totals until after the first
    // client render. The default-zone column reads the shared `CheapestPrices`
    // `read_listings` resource and the home-world column reads
    // `home_world_listings`, both via `.with()`/`.get()` — which (same gotcha
    // as #740/#742) do NOT subscribe-and-suspend the wrapping `<Suspense>`. So
    // SSR renders the body with the resource pending (`total`/`shard_total`
    // both `None`, so the `match` arm collapses to `()`), while the first CSR
    // hydration render sees the serialised/resolved resource and may emit the
    // extra "with shards" `<div>`. That structural divergence trips tachys'
    // walker at `tachys-0.2.15/src/hydration.rs:227` (`failed_to_cast_text_node`,
    // the post-debug-strip `unreachable!()`) and cascades into the
    // `RefCell already borrowed` panic from the wasm-bindgen-futures executor —
    // the `/items/jobset/<JOB>/set/<ilvl>` mirror of the cluster #740
    // (`<CheapestPrice>`) and #742 (`<RelatedItems>`) already fixed. An
    // `Effect`-driven `hydrated` flag (effects run client-only, after the first
    // render) makes SSR and the first CSR render both emit the Suspense-fallback
    // shape, so the trees agree; a frame later the effect fires and the totals
    // swap in reactively. The sibling `set_total` columns below only render a
    // structurally-stable `<GilOrDash>`, so they don't need the gate.
    let hydrated = RwSignal::new(false);
    Effect::new(move |_| {
        hydrated.set(true);
    });

    view! {
        <MetaTitle title=move || t_string!(i18n, job_set_detail_title).to_string().replace("%set%", &set_stem()) />
        <MetaDescription text=move || t_string!(i18n, job_set_detail_desc).to_string().replace("%set%", &set_stem()) />

        <div class="flex flex-col gap-4">
            <div class="flex flex-row items-center gap-3 flex-wrap">
                <A
                    href=back_href
                    attr:class="text-xs font-bold uppercase tracking-wider px-3 py-1.5 rounded-lg \
                               bg-white/5 hover:bg-white/10 text-[color:var(--color-text-muted)] \
                               border border-white/5 transition-colors"
                >
                    {move || t_string!(i18n, job_set_detail_back).to_string().replace("%job%", &job_name())}
                </A>
                <Show when=move || !set_entries.get().is_empty()>
                    <AddSetToList
                        button_label=Signal::derive(move || t_string!(i18n, job_set_detail_add_set_button).to_string())
                        tooltip=Signal::derive(move || t_string!(i18n, job_set_detail_add_set_tooltip).to_string())
                        modal_title=Signal::derive(move || t_string!(i18n, job_set_detail_add_set_modal_title).to_string())
                        subject=Signal::derive(move || set_stem.get())
                        entries=set_entries
                    />
                </Show>
                <Show when=move || has_materials.get()>
                    <AddSetToList
                        button_label=Signal::derive(move || t_string!(i18n, job_set_detail_add_materials_button).to_string())
                        tooltip=Signal::derive(move || t_string!(i18n, job_set_detail_add_materials_tooltip).to_string())
                        modal_title=Signal::derive(move || t_string!(i18n, job_set_detail_add_materials_modal_title).to_string())
                        subject=Signal::derive(move || set_stem.get())
                        entries=material_entries
                    />
                </Show>
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
                                let slot = slot_label_from_name(&item_name);
                                view! {
                                    <div class="flex flex-col p-3 rounded-lg panel border border-white/5">
                                        <div class="flex flex-row items-center gap-3 mb-2">
                                            <A
                                                href=format!("/item/{}", item_id)
                                                attr:class="shrink-0 flex items-center justify-center w-12 h-12"
                                            >
                                                <ItemIcon item_id=item_id icon_size=IconSize::Medium />
                                            </A>
                                            <div class="flex flex-col min-w-0">
                                                {if let Some(label) = slot {
                                                    view! {
                                                        <span class="text-[10px] font-bold uppercase tracking-wider px-1.5 py-0.5 rounded bg-brand-500/15 text-brand-300 self-start mb-1">
                                                            {label}
                                                        </span>
                                                    }.into_any()
                                                } else {
                                                    ().into_any()
                                                }}
                                                <A
                                                    href=format!("/item/{}", item_id)
                                                    attr:class="font-medium text-sm leading-snug \
                                                               hover:text-brand-300 transition-colors line-clamp-2"
                                                >
                                                    {item_name}
                                                </A>
                                            </div>
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

            // Aggregated crafting materials across every craftable
            // piece in the set. Hidden entirely when no item in the
            // set has a recipe (raid drops, vendor gear, etc.).
            {move || {
                let entries = materials.get().unwrap_or_default();
                if entries.is_empty() {
                    return ().into_any();
                }
                let main: Vec<_> = entries.iter().filter(|e| !e.is_shard).cloned().collect();
                let shards: Vec<_> = entries.iter().filter(|e| e.is_shard).cloned().collect();
                let entries_for_default = entries.clone();
                let entries_for_home = entries.clone();
                view! {
                    <div class="mt-4">
                        <h4 class="text-base font-bold mb-2">{t!(i18n, job_set_detail_materials_heading)}</h4>
                        <p class="text-xs text-[color:var(--color-text-muted)] mb-3">
                            {t!(i18n, job_set_detail_materials_desc)}
                        </p>

                        <div class="grid grid-cols-1 md:grid-cols-2 gap-3 mb-3">
                            <div class="panel p-3 rounded-lg border border-white/5">
                                <div class="text-[10px] font-bold uppercase tracking-wider text-[color:var(--color-text-muted)] mb-1">
                                    {t!(i18n, job_set_detail_materials_total_default_zone)}
                                </div>
                                <Suspense fallback=move || view! { <span class="text-[color:var(--color-text-muted)]">"…"</span> }>
                                    {
                                        let entries_for_total = entries_for_default;
                                        move || {
                                            if !hydrated.get() {
                                                return view! { <span class="text-[color:var(--color-text-muted)]">"…"</span> }.into_any();
                                            }
                                            let entries = entries_for_total.clone();
                                            let total = default_zone_listings.and_then(|listings| {
                                                listings.with(|data| match data {
                                                    Some(Ok(map)) => materials_total(&entries, map, false),
                                                    _ => None,
                                                })
                                            });
                                            let shard_total = default_zone_listings.and_then(|listings| {
                                                listings.with(|data| match data {
                                                    Some(Ok(map)) => materials_total(&entries, map, true),
                                                    _ => None,
                                                })
                                            });
                                            view! {
                                                <div class="text-lg font-bold">
                                                    <GilOrDash amount=total.map(|t| t as i32) />
                                                </div>
                                                {match (shard_total, total) {
                                                    (Some(with_shards), Some(no_shards)) if with_shards > no_shards => view! {
                                                        <div class="text-[11px] text-[color:var(--color-text-muted)]">
                                                            {t!(i18n, job_set_detail_materials_total_with_shards)} " "
                                                            <Gil amount=with_shards as i32 />
                                                        </div>
                                                    }.into_any(),
                                                    _ => ().into_any(),
                                                }}
                                            }.into_any()
                                        }
                                    }
                                </Suspense>
                            </div>
                            <div class="panel p-3 rounded-lg border border-white/5">
                                <div class="text-[10px] font-bold uppercase tracking-wider text-[color:var(--color-text-muted)] mb-1">
                                    {t!(i18n, job_set_detail_materials_total_home_world)}
                                </div>
                                <Suspense fallback=move || view! { <span class="text-[color:var(--color-text-muted)]">"…"</span> }>
                                    {
                                        let entries_for_total = entries_for_home;
                                        move || {
                                            if !hydrated.get() {
                                                return view! { <span class="text-[color:var(--color-text-muted)]">"…"</span> }.into_any();
                                            }
                                            let entries = entries_for_total.clone();
                                            let total = home_world_listings
                                                .get()
                                                .flatten()
                                                .as_ref()
                                                .and_then(|map| materials_total(&entries, map, false));
                                            let shard_total = home_world_listings
                                                .get()
                                                .flatten()
                                                .as_ref()
                                                .and_then(|map| materials_total(&entries, map, true));
                                            view! {
                                                <div class="text-lg font-bold">
                                                    <GilOrDash amount=total.map(|t| t as i32) />
                                                </div>
                                                {match (shard_total, total) {
                                                    (Some(with_shards), Some(no_shards)) if with_shards > no_shards => view! {
                                                        <div class="text-[11px] text-[color:var(--color-text-muted)]">
                                                            {t!(i18n, job_set_detail_materials_total_with_shards)} " "
                                                            <Gil amount=with_shards as i32 />
                                                        </div>
                                                    }.into_any(),
                                                    _ => ().into_any(),
                                                }}
                                            }.into_any()
                                        }
                                    }
                                </Suspense>
                            </div>
                        </div>

                        <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-4 gap-2">
                            {main.into_iter().map(material_row).collect::<Vec<_>>()}
                        </div>
                        {if !shards.is_empty() {
                            view! {
                                <div class="mt-3 pt-3 border-t border-white/5">
                                    <div class="text-xs uppercase tracking-wider text-[color:var(--color-text-muted)] mb-2">
                                        {t!(i18n, job_set_detail_materials_shards)}
                                    </div>
                                    <div class="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-4 2xl:grid-cols-6 gap-2">
                                        {shards.into_iter().map(material_row).collect::<Vec<_>>()}
                                    </div>
                                </div>
                            }.into_any()
                        } else {
                            ().into_any()
                        }}
                    </div>
                }.into_any()
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
                        <Suspense fallback=move || view! { <span class="text-[color:var(--color-text-muted)]">"…"</span> }>
                            {move || {
                                let total = group.get().and_then(|g| {
                                    let listings = default_zone_listings?;
                                    listings.with(|data| match data {
                                        Some(Ok(map)) => set_total(&g, map, false),
                                        _ => None,
                                    })
                                });
                                view! { <GilOrDash amount=total.map(|t| t as i32) /> }
                            }}
                        </Suspense>
                    </div>
                </div>
                <div class="panel p-4 rounded-xl border border-white/5">
                    <div class="text-xs font-bold uppercase tracking-wider text-[color:var(--color-text-muted)] mb-1">
                        {t!(i18n, job_set_detail_home_world_total)}
                    </div>
                    <div class="text-xl font-bold">
                        <Suspense fallback=move || view! { <span class="text-[color:var(--color-text-muted)]">"…"</span> }>
                            {move || {
                                let total = group.get().and_then(|g| {
                                    home_world_listings
                                        .get()
                                        .flatten()
                                        .as_ref()
                                        .and_then(|map| set_total(&g, map, false))
                                });
                                view! { <GilOrDash amount=total.map(|t| t as i32) /> }
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
    use xiv_gen::{Item, ItemId, Recipe, RecipeId};

    fn item(id: i32, name: &str) -> GroupableItem {
        GroupableItem {
            id: ItemId(id),
            name: name.to_string(),
            ilvl: 770,
        }
    }

    fn make_item(
        id: i32,
        name: &str,
        ilvl: i32,
        class_job_category: i32,
        item_search_category: i32,
    ) -> Item {
        Item {
            key_id: ItemId(id),
            name: name.to_string(),
            description: String::new(),
            icon: 0,
            item_ui_category: 0,
            item_search_category,
            base_param: [0; 6],
            base_param_value: [0; 6],
            base_param_special: [0; 6],
            base_param_value_special: [0; 6],
            item_sort_category: 0,
            level_item: ilvl,
            level_equip: 0,
            can_be_hq: true,
            is_collectable: false,
            price_mid: 0,
            price_low: 0,
            stack_size: 1,
            class_job_category,
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

    #[test]
    fn slot_label_recognises_courtly_lover_striking_set() {
        // The exact piece names from the FFXIV CSV at iLvl 770 SAM
        // gear. If any of these stop labelling we lose the per-tile
        // chip on the detail page, which is the whole point of the
        // feature.
        assert_eq!(
            slot_label_from_name("Courtly Lover's Temple Chain of Striking"),
            Some("HEAD")
        );
        assert_eq!(
            slot_label_from_name("Courtly Lover's Cloak of Striking"),
            Some("CHEST")
        );
        assert_eq!(
            slot_label_from_name("Courtly Lover's Armguards of Striking"),
            Some("HANDS")
        );
        assert_eq!(
            slot_label_from_name("Courtly Lover's Brais of Striking"),
            Some("LEGS")
        );
        assert_eq!(
            slot_label_from_name("Courtly Lover's Boots of Striking"),
            Some("FEET")
        );
    }

    #[test]
    fn slot_label_for_tank_fending_set() {
        // Surcoat + Hairpin + Gauntlets + Breeches + Boots is the
        // tank flavour of the Dawntrail crafted set.
        assert_eq!(
            slot_label_from_name("Courtly Lover's Hairpin of Fending"),
            Some("HEAD")
        );
        assert_eq!(
            slot_label_from_name("Courtly Lover's Surcoat of Fending"),
            Some("CHEST")
        );
        assert_eq!(
            slot_label_from_name("Courtly Lover's Gauntlets of Fending"),
            Some("HANDS")
        );
        assert_eq!(
            slot_label_from_name("Courtly Lover's Breeches of Fending"),
            Some("LEGS")
        );
        assert_eq!(
            slot_label_from_name("Courtly Lover's Boots of Fending"),
            Some("FEET")
        );
    }

    #[test]
    fn slot_label_accessories_and_weapons() {
        assert_eq!(
            slot_label_from_name("Courtly Lover's Shield"),
            Some("OFF-HAND")
        );
        assert_eq!(
            slot_label_from_name("Courtly Lover's Sword"),
            Some("WEAPON")
        );
        assert_eq!(
            slot_label_from_name("Courtly Lover's Blade"),
            Some("WEAPON")
        );
        assert_eq!(slot_label_from_name("Earring of Fending"), Some("EAR"));
        assert_eq!(slot_label_from_name("Choker of Slaying"), Some("NECK"));
        assert_eq!(slot_label_from_name("Bracelet of Striking"), Some("WRIST"));
        assert_eq!(slot_label_from_name("Ring of Casting"), Some("RING"));
    }

    #[test]
    fn slot_label_returns_none_for_unknown_pattern() {
        // Random non-equipment items shouldn't get a label. This
        // keeps the chip honest — better to omit than mislabel.
        assert_eq!(slot_label_from_name("Garlean Fiber"), None);
        assert_eq!(slot_label_from_name("Adamantite Ingot"), None);
    }

    #[test]
    fn find_set_skips_non_market_items_to_avoid_polluting_bucket() {
        // Regression for /items/jobset/SAM/set/770 rendering empty:
        // when items at the same iLvl are NOT on the market (raid
        // drops with item_search_category == 0), they used to leak
        // into the LCP-based grouping and either change the stem or
        // collapse it to whitespace, so the detail page's
        // `.find(|g| g.ilvl == 770)` returned None. The helper now
        // applies the same `item_search_category > 0` filter the
        // parent JobItems route uses by default.
        const SAM_CAT: i32 = 65; // Striking — anything SAM-equippable
        let items = [
            make_item(
                1,
                "Courtly Lover's Temple Chain of Striking",
                770,
                SAM_CAT,
                9820,
            ),
            make_item(2, "Courtly Lover's Cloak of Striking", 770, SAM_CAT, 9821),
            make_item(
                3,
                "Courtly Lover's Armguards of Striking",
                770,
                SAM_CAT,
                9822,
            ),
            make_item(4, "Courtly Lover's Brais of Striking", 770, SAM_CAT, 9823),
            make_item(5, "Courtly Lover's Boots of Striking", 770, SAM_CAT, 9824),
            // Non-market raid drops at the same iLvl with a totally
            // different name. Pre-fix, these were keeping the grouper
            // from picking up "Courtly Lover's" as the common prefix.
            make_item(101, "Sky Lemures' Mask of Striking", 770, SAM_CAT, 0),
            make_item(102, "Sky Lemures' Top of Striking", 770, SAM_CAT, 0),
            make_item(103, "Sky Lemures' Bottoms of Striking", 770, SAM_CAT, 0),
        ];

        let group = find_set_for_job(items.iter(), |it| it.class_job_category == SAM_CAT, 770)
            .expect("770 set must resolve");
        assert_eq!(group.stem, "Courtly Lover's");
        assert_eq!(group.items.len(), 5);
        let mut got_ids: Vec<i32> = group.items.iter().map(|i| i.id.0).collect();
        got_ids.sort();
        assert_eq!(got_ids, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn find_set_returns_stable_item_order_for_hydration() {
        // The real route feeds `find_set_for_job` from a HashMap. SSR
        // and WASM can observe different HashMap iteration orders; if
        // the detail grid order changes during hydration, Leptos can
        // pair an item's icon/link href with a neighboring item's name.
        const SAM_CAT: i32 = 65;
        let items = [
            make_item(5, "Courtly Lover's Boots of Striking", 770, SAM_CAT, 9824),
            make_item(2, "Courtly Lover's Cloak of Striking", 770, SAM_CAT, 9821),
            make_item(
                1,
                "Courtly Lover's Temple Chain of Striking",
                770,
                SAM_CAT,
                9820,
            ),
            make_item(4, "Courtly Lover's Brais of Striking", 770, SAM_CAT, 9823),
            make_item(
                3,
                "Courtly Lover's Armguards of Striking",
                770,
                SAM_CAT,
                9822,
            ),
        ];

        let group = find_set_for_job(items.iter(), |it| it.class_job_category == SAM_CAT, 770)
            .expect("770 set must resolve");
        let got_names: Vec<_> = group.items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(
            got_names,
            vec![
                "Courtly Lover's Armguards of Striking",
                "Courtly Lover's Boots of Striking",
                "Courtly Lover's Brais of Striking",
                "Courtly Lover's Cloak of Striking",
                "Courtly Lover's Temple Chain of Striking",
            ]
        );
    }

    #[test]
    fn find_set_returns_none_for_unknown_ilvl() {
        const SAM_CAT: i32 = 65;
        let items = [
            make_item(1, "Courtly Lover's Cloak of Striking", 770, SAM_CAT, 9821),
            make_item(2, "Courtly Lover's Brais of Striking", 770, SAM_CAT, 9823),
        ];
        // No 600-iLvl set in the fixture.
        assert!(
            find_set_for_job(items.iter(), |it| it.class_job_category == SAM_CAT, 600).is_none()
        );
    }

    #[test]
    fn find_set_ignores_items_outside_the_job() {
        // Tank items at 770 must NOT bleed into a SAM detail page.
        const SAM_CAT: i32 = 65;
        const TANK_CAT: i32 = 66;
        let items = [
            make_item(1, "Courtly Lover's Cloak of Striking", 770, SAM_CAT, 9821),
            make_item(2, "Courtly Lover's Brais of Striking", 770, SAM_CAT, 9823),
            // Fending pieces — different class_job_category, so the
            // job-filter rejects them before grouping runs.
            make_item(
                10,
                "Courtly Lover's Surcoat of Fending",
                770,
                TANK_CAT,
                9811,
            ),
            make_item(
                11,
                "Courtly Lover's Breeches of Fending",
                770,
                TANK_CAT,
                9813,
            ),
            make_item(12, "Courtly Lover's Boots of Fending", 770, TANK_CAT, 9814),
        ];

        let group = find_set_for_job(items.iter(), |it| it.class_job_category == SAM_CAT, 770)
            .expect("SAM 770 set");
        // The Fending pieces don't appear, even though they share the
        // "Courtly Lover's" stem at the same iLvl.
        assert_eq!(group.items.len(), 2);
        for item in &group.items {
            assert!(!item.name.contains("Fending"));
        }
    }

    fn make_recipe(id: i32, result: i32, ingredients: &[(i32, i32)]) -> Recipe {
        let mut ing = [0i32; 8];
        let mut amt = [0i32; 8];
        for (i, (iid, q)) in ingredients.iter().enumerate() {
            ing[i] = *iid;
            amt[i] = *q;
        }
        Recipe {
            key_id: RecipeId(id),
            item_result: result,
            amount_result: 1,
            ingredient: ing,
            amount_ingredient: amt,
            craft_type: 0,
            recipe_level_table: 0,
        }
    }

    #[test]
    fn aggregate_materials_sums_across_recipes_and_demotes_shards() {
        let set = JobSetGroup {
            stem: "Courtly Lover's".to_string(),
            ilvl: 770,
            items: vec![item(1, "Cloak"), item(2, "Brais")],
        };
        // Item 1 needs 2 fiber + 3 shards; item 2 needs 1 fiber + 5 shards.
        // Item 99 is a different set's recipe — must NOT contribute.
        let recipes: HashMap<RecipeId, Recipe> = [
            (RecipeId(10), make_recipe(10, 1, &[(100, 2), (59, 3)])),
            (RecipeId(11), make_recipe(11, 2, &[(100, 1), (59, 5)])),
            (RecipeId(12), make_recipe(12, 99, &[(100, 1000)])),
        ]
        .into_iter()
        .collect();
        let items: HashMap<ItemId, Item> = [
            (ItemId(100), make_item(100, "Garlean Fiber", 0, 0, 51)),
            (ItemId(59), make_item(59, "Wind Shard", 0, 0, 59)),
        ]
        .into_iter()
        .collect();

        let entries = aggregate_materials(&set, &recipes, &items);
        assert_eq!(entries.len(), 2);
        // Non-shards first, then by descending qty.
        assert_eq!(entries[0].id, ItemId(100));
        assert_eq!(entries[0].amount, 3);
        assert!(!entries[0].is_shard);
        assert_eq!(entries[1].id, ItemId(59));
        assert_eq!(entries[1].amount, 8);
        assert!(entries[1].is_shard);
    }

    #[test]
    fn aggregate_materials_empty_when_no_recipe_matches_set() {
        let set = JobSetGroup {
            stem: "Vendor".to_string(),
            ilvl: 100,
            items: vec![item(500, "Vendor Sword")],
        };
        let recipes: HashMap<RecipeId, Recipe> = HashMap::new();
        let items: HashMap<ItemId, Item> = HashMap::new();
        assert!(aggregate_materials(&set, &recipes, &items).is_empty());
    }

    fn material(id: i32, amount: i32, is_shard: bool) -> MaterialEntry {
        MaterialEntry {
            id: ItemId(id),
            name: format!("m{id}"),
            amount,
            is_shard,
        }
    }

    #[test]
    fn materials_total_sums_amount_times_unit_price_and_excludes_shards_by_default() {
        // 2 fiber @ 100g + 5 fiber @ 0g (missing) + 10 shards @ 5g.
        // Default (include_shards=false): 2*100 = 200. Missing items
        // contribute 0 but don't flip had_any to false on their own.
        let materials = vec![
            material(100, 2, false),
            material(101, 5, false),
            material(59, 10, true),
        ];
        let prices = map_with(&[(100, false, 100), (59, false, 5)]);
        assert_eq!(materials_total(&materials, &prices, false), Some(200));
        // With shards: 200 + 10*5 = 250.
        assert_eq!(materials_total(&materials, &prices, true), Some(250));
    }

    #[test]
    fn materials_total_none_when_no_listings() {
        // Every material is missing a listing — nothing to total.
        let materials = vec![material(100, 2, false), material(101, 5, false)];
        let prices = map_with(&[]);
        assert_eq!(materials_total(&materials, &prices, false), None);
    }
}
