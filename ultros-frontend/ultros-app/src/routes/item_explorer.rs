use std::fmt::Display;
use std::{collections::HashSet, str::FromStr};

use crate::CheapestPrices;
use crate::components::clipboard::Clipboard;
use crate::components::gil::Gil;
use crate::components::icon::Icon;
use crate::components::job_set_card::JobSetCard;
use crate::components::job_set_grouping::{GroupableItem, group_into_sets};
use crate::components::loading::Loading;
use crate::components::query_button::QueryButton;
use crate::components::toggle::Toggle;
use crate::components::world_name::WorldName;
use crate::components::{add_to_list::*, cheapest_price::*, item_icon::*, meta::*};
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use crate::routes::item_explorer_scope::{ExplorerPriceScope, use_explorer_price_scope};
use icondata as i;
use itertools::Itertools;
use leptos::prelude::*;
use leptos::reactive::wrappers::write::SignalSetter;
use leptos_router::components::A;
use leptos_router::components::Outlet;
use leptos_router::hooks::{query_signal, use_location, use_params_map};
use leptos_router::location::Location;
use paginate::Pages;
use percent_encoding::percent_decode_str;
use ultros_api_types::world_helper::AnySelector;
use xiv_gen::{ClassJobCategory, ClassJobCategoryId, Item, ItemId};

/// Return true if the given acronym is in the given class job category
pub(crate) fn job_category_lookup(
    class_job_category: &ClassJobCategory,
    job_acronym: &str,
) -> bool {
    let lower_case = job_acronym.to_lowercase();
    // this is kind of dumb, but this should give a compile time error whenever a job changes.
    let ClassJobCategory {
        key_id: _,
        name: _,
        adv,
        gla,
        pgl,
        mrd,
        lnc,
        arc,
        cnj,
        thm,
        crp,
        bsm,
        arm,
        gsm,
        ltw,
        wvr,
        alc,
        cul,
        min,
        btn,
        fsh,
        pld,
        mnk,
        war,
        drg,
        brd,
        whm,
        blm,
        acn,
        smn,
        sch,
        rog,
        nin,
        mch,
        drk,
        ast,
        sam,
        rdm,
        blu,
        gnb,
        dnc,
        rpr,
        sge,
        vpr,
        pct,
        ..
    } = class_job_category;
    match lower_case.as_str() {
        "adv" => *adv,
        "gla" => *gla,
        "pgl" => *pgl,
        "mrd" => *mrd,
        "lnc" => *lnc,
        "arc" => *arc,
        "cnj" => *cnj,
        "thm" => *thm,
        "crp" => *crp,
        "bsm" => *bsm,
        "arm" => *arm,
        "gsm" => *gsm,
        "ltw" => *ltw,
        "wvr" => *wvr,
        "alc" => *alc,
        "cul" => *cul,
        "min" => *min,
        "btn" => *btn,
        "fsh" => *fsh,
        "pld" => *pld,
        "mnk" => *mnk,
        "war" => *war,
        "drg" => *drg,
        "brd" => *brd,
        "whm" => *whm,
        "blm" => *blm,
        "acn" => *acn,
        "smn" => *smn,
        "sch" => *sch,
        "rog" => *rog,
        "nin" => *nin,
        "mch" => *mch,
        "drk" => *drk,
        "ast" => *ast,
        "sam" => *sam,
        "rdm" => *rdm,
        "blu" => *blu,
        "gnb" => *gnb,
        "dnc" => *dnc,
        "rpr" => *rpr,
        "sge" => *sge,
        "vpr" => *vpr,
        "pct" => *pct,
        _ => {
            tracing::warn!(job_acronym, "Unknown job acronym");
            false
        }
    }
}

/// Filter `data.items` to entries matching the given canonical job acronym.
/// When `market_only` is true, drops items without an `item_search_category`
/// (FFXIV's "not listable on the market board" flag).
///
/// Returned in **ascending `ItemId` order**. `data.items` is a HashMap whose
/// iteration order differs between the server's SSR process and the client's
/// WASM process (different `RandomState` seed). Downstream `For` rendering
/// and the `JobSetCard` grid both lay out children in iteration order, so a
/// divergence drives the view tree out of sync with the SSR DOM and tachys
/// panics at `hydration.rs:163` (`failed_to_cast_element`) on
/// `/items/jobset/<JOB>`. Sorting by `ItemId` pins one order across both
/// processes.
pub(crate) fn collect_job_items_sorted<'a>(
    data: &'a xiv_gen::Data,
    canonical_abbr: &str,
    market_only: bool,
) -> Vec<(&'a ItemId, &'a Item)> {
    let job_categories: HashSet<_> = data
        .class_job_categorys
        .iter()
        .filter(|(_id, c)| job_category_lookup(c, canonical_abbr))
        .map(|(id, _)| *id)
        .collect();
    let mut items: Vec<_> = data
        .items
        .iter()
        .filter(|(_id, item)| job_categories.contains(&ClassJobCategoryId(item.class_job_category)))
        .filter(|(_id, item)| !market_only || item.item_search_category > 0)
        .collect();
    items.sort_by_key(|(id, _)| id.0);
    items
}

#[component]
pub fn CategoryItems() -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    let params = use_params_map();
    let data = tracked_data();
    let items = Memo::new(move |_| {
        let cat = params()
            .get_str("category")
            .and_then(|cat| percent_encoding::percent_decode_str(cat).decode_utf8().ok())
            .and_then(|cat| {
                data.item_search_categorys
                    .iter()
                    .find(|(_id, category)| category.name == cat)
            })
            .map(|(id, _)| {
                let mut items: Vec<_> = data
                    .items
                    .iter()
                    .filter(|(_, item)| item.item_search_category == id.0)
                    .collect();
                // See note in `JobItems::items` — pin a stable order across
                // the SSR and CSR HashMap iterations to keep hydration in
                // sync.
                items.sort_by_key(|(id, _)| id.0);
                items
            });
        cat.unwrap_or_default()
    });
    let category_view_name = Memo::new(move |_| {
        params()
            .get("category")
            .as_ref()
            .and_then(|cat| percent_decode_str(cat).decode_utf8().ok())
            .map(|c| c.to_string())
            .unwrap_or_else(|| crate::i18n::t_string!(i18n, category_view_default).to_string())
    });
    view! {
        <MetaTitle title=move || crate::i18n::t_string!(i18n, item_explorer_title).to_string().replace("%name%", &category_view_name()) />
        <MetaDescription text=move || crate::i18n::t_string!(i18n, category_list_desc).to_string().replace("%category%", &category_view_name()) />
        <h3 class="text-xl">{category_view_name}</h3>
        <ItemList items />
    }
    .into_any()
}

#[component]
pub fn JobItems() -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    let params = use_params_map();
    let data = tracked_data();
    let (non_market, set_non_market) = query_signal::<bool>("show-non-market");
    let market_only = Signal::derive(move || !non_market().unwrap_or_default());
    let set_market_only =
        SignalSetter::map(move |market: bool| set_non_market((!market).then_some(true)));
    let items = Memo::new(move |_| {
        // decode, normalize, and map to a known job abbreviation if possible
        let raw = match params().get("jobset") {
            Some(p) => p.clone(),
            None => return vec![],
        };
        let decoded = percent_encoding::percent_decode_str(&raw)
            .decode_utf8()
            .map(|s| s.to_string())
            .unwrap_or(raw.clone());
        let lower = decoded.to_lowercase();

        // try to resolve to a canonical abbreviation (fallback: decoded input)
        let canonical_abbr = data
            .class_jobs
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
            .unwrap_or(decoded.clone());

        collect_job_items_sorted(data, &canonical_abbr, market_only())
    });
    let job_set = Memo::new(move |_| {
        params()
            .get("jobset")
            .as_ref()
            .and_then(|s| percent_encoding::percent_decode_str(s).decode_utf8().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| crate::i18n::t_string!(i18n, job_set_default).to_string())
    });

    // Split the job's items into named gear sets (rendered as
    // condensed cards) and an ungrouped remainder (sortable list).
    // The grouping operates on projections so the rest of `ItemList`
    // can stay untouched.
    let grouping = Memo::new(move |_| {
        let job_items = items();
        let projections: Vec<GroupableItem> = job_items
            .iter()
            .filter(|(_, item)| item.level_item > 0)
            .map(|(id, item)| GroupableItem {
                id: **id,
                name: item.name.clone(),
                ilvl: item.level_item,
            })
            .collect();

        let (groups, _ungrouped) = group_into_sets(projections);
        // Items placed into a set don't render as individual cards.
        // Everything else — items the grouping rejected AND items we
        // skipped up front (level_item == 0, non-equipment) — flows
        // through to the regular sortable list below the set cards.
        let in_a_group: std::collections::HashSet<i32> = groups
            .iter()
            .flat_map(|g| g.items.iter().map(|i| i.id.0))
            .collect();
        let ungrouped_items: Vec<_> = job_items
            .iter()
            .filter(|(id, _)| !in_a_group.contains(&id.0))
            .copied()
            .collect();

        (groups, ungrouped_items)
    });

    let groups = Memo::new(move |_| grouping.with(|(g, _)| g.clone()));
    let ungrouped_items = Memo::new(move |_| grouping.with(|(_, ungrouped)| ungrouped.clone()));

    let jobset_param = Memo::new(move |_| {
        params()
            .get("jobset")
            .as_ref()
            .map(|s| s.to_string())
            .unwrap_or_default()
    });

    view! {
        <MetaTitle title=move || crate::i18n::t_string!(i18n, item_explorer_title).to_string().replace("%name%", &job_set()) />
        <MetaDescription text=move || crate::i18n::t_string!(i18n, job_set_list_desc).to_string().replace("%job%", &job_set()) />
        <h3 class="text-xl">{job_set}</h3>
        <div class="flex flex-row items-center gap-2">
            <Toggle
                checked=market_only
                set_checked=set_market_only
                checked_label=t_string!(i18n, item_explorer_filtering_unmarketable).to_string()
                unchecked_label=t_string!(i18n, item_explorer_showing_all).to_string()
            />
        </div>

        // Set cards: one per detected gear set, sized matching the
        // ItemList grid so the rows align when both are present.
        {move || {
            let gs = groups.get();
            if gs.is_empty() {
                ().into_any()
            } else {
                let jobset = jobset_param.get();
                view! {
                    <div class="mt-4">
                        <h4 class="text-xs font-bold uppercase tracking-wider text-[color:var(--color-text-muted)] mb-2">
                            {t!(i18n, job_set_card_section_heading)}
                        </h4>
                        <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-4 gap-4">
                            {gs.into_iter().map(|group| {
                                view! { <JobSetCard group=group jobset=jobset.clone() /> }
                            }).collect::<Vec<_>>()}
                        </div>
                    </div>
                }
                .into_any()
            }
        }}

        <ItemList items=ungrouped_items />
    }
    .into_any()
}

#[component]
pub fn DefaultItems() -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <MetaTitle title=t_string!(i18n, item_explorer_default_title).to_string() />
        <MetaDescription text=t_string!(i18n, item_explorer_default_desc).to_string() />
        <div class="flex flex-col">
            <div>{t!(i18n, item_explorer_default_instruction)}</div>
            <div>
                {t!(i18n, item_explorer_default_sort_info)}
            </div>
            <div>""</div>
        </div>
    }
    .into_any()
}

#[derive(PartialEq, PartialOrd, Copy, Clone)]
enum ItemSortOption {
    ItemLevel,
    Price,
    Name,
    Key,
}

impl FromStr for ItemSortOption {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "ilvl" => ItemSortOption::ItemLevel,
            "price" => ItemSortOption::Price,
            "name" => ItemSortOption::Name,
            "key" => ItemSortOption::Key,
            _ => return Err(()),
        })
    }
}

impl Display for ItemSortOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let val = match self {
            ItemSortOption::ItemLevel => "ilvl",
            ItemSortOption::Price => "price",
            ItemSortOption::Name => "name",
            ItemSortOption::Key => "key",
        };
        f.write_str(val)
    }
}

#[derive(PartialEq, PartialOrd, Copy, Clone)]
enum SortDirection {
    Asc,
    Desc,
}

impl FromStr for SortDirection {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "asc" => SortDirection::Asc,
            "dsc" => SortDirection::Desc,
            _ => return Err(()),
        })
    }
}

impl Display for SortDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let val = match self {
            SortDirection::Asc => "asc",
            SortDirection::Desc => "desc",
        };
        f.write_str(val)
    }
}

/// Sortable column header for the results list. Clicking sets `sort` to
/// `value` and resets the page; clicking the already-active column
/// toggles between descending (the default — `dir` removed) and
/// ascending. Href construction mirrors `QueryButton` so all other query
/// params (`world`, `per_page`, ...) survive.
#[component]
fn SortHeader(
    /// value written to the `sort` query key
    value: &'static str,
    /// active when no `sort` param is present
    #[prop(optional)]
    default: bool,
    children: Children,
) -> impl IntoView {
    let Location {
        pathname, query, ..
    } = use_location();
    let is_active = Signal::derive(move || {
        query.with(|q| {
            let current = q.get_str("sort");
            current == Some(value) || (default && current.is_none())
        })
    });
    let is_asc = Signal::derive(move || query.with(|q| q.get_str("dir") == Some("asc")));
    let href = move || {
        let mut query = query();
        query.remove("page");
        query.remove("sort");
        query.insert("sort".to_string(), value.to_string());
        let flip_to_asc = is_active.get() && !is_asc.get();
        query.remove("dir");
        if flip_to_asc {
            query.insert("dir".to_string(), "asc".to_string());
        }
        format!("{}{}", pathname(), query.to_query_string())
    };
    view! {
        <div
            role="columnheader"
            aria-sort=move || {
                if is_active() {
                    if is_asc() { "ascending" } else { "descending" }
                } else {
                    "none"
                }
            }
        >
            <a
                href=href
                class="flex items-center gap-1 hover:text-brand-300 transition-colors"
            >
                {children()}
                <span class="w-3 inline-block">
                    {move || {
                        if is_active() {
                            if is_asc() { "↑" } else { "↓" }
                        } else {
                            ""
                        }
                    }}
                </span>
            </a>
        </div>
    }
    .into_any()
}

#[component]
fn ItemList(items: Memo<Vec<(&'static ItemId, &'static Item)>>) -> impl IntoView {
    let i18n = use_i18n();
    let (page, _set_page) = query_signal::<i32>("page");
    let (direction, _set_direction) = query_signal::<SortDirection>("dir");
    let (sort, _set_sort) = query_signal::<ItemSortOption>("sort");

    let cheapest_prices = use_context::<CheapestPrices>().unwrap();
    let listings_resource = cheapest_prices.read_listings;
    let scope = use_context::<ExplorerPriceScope>()
        .expect("ItemList is always rendered inside ItemExplorer, which provides the scope");
    let scope_name = scope.name;
    let is_single_world = scope.is_single_world;

    // Defer the price-based filter + sort until after hydration.
    //
    // `sorted_items` previously read `listings_resource.get()` directly. On
    // SSR that resource is `None` at render time (the wrapping `<Suspense>`
    // never suspends — `.get()` doesn't subscribe-and-suspend the way
    // `.read()` does), so the SSR HTML reflects the ilvl fallback with NO
    // price filter applied. On the client, Leptos serialises the resolved
    // resource into the payload so `listings_resource.get()` returns
    // `Some(map)` immediately during hydration — which would make the first
    // CSR render apply the price filter (dropping items without listings)
    // AND sort by price. The resulting `<For>` children then mismatch the
    // SSR DOM in both count and order, and tachys' walker panics at
    // `hydration.rs:163`/`:195` (`failed_to_cast_element`). That's the
    // `?sort=price`/`?page=N` cluster in GlitchTip — issues 707
    // (`/items/jobset/DNC?page=7&sort=price`, 47 events), 156
    // (`/items/jobset/NIN?page=21&sort=price`, 18 events), 4951+5002
    // (`RefCell already borrowed` cascades from the same trace), plus the
    // category-page mirrors (4968/4969 on Dancer's Arms etc.).
    //
    // Gate the price map behind a signal that defaults to `false` and
    // flips to `true` from an `Effect` — `Effect::new` runs only on the
    // client (same idiom as `WasmLoadingIndicator`), and only AFTER the
    // initial view is rendered. So the SSR render and the first CSR
    // hydration render both see `hydrated == false`, both fall back to
    // the ilvl sort with all items included, and shapes/positions match.
    // A frame later the effect fires, the memo re-runs with the real
    // price map, and the `<For>` reactively reorders/filters — by which
    // point hydration is finished and tachys is no longer walking.
    let hydrated = RwSignal::new(false);
    Effect::new(move |_| {
        hydrated.set(true);
    });

    let sorted_items = Memo::new(move |_| {
        let direction = direction().unwrap_or(SortDirection::Desc);
        let item_property = sort().unwrap_or(ItemSortOption::ItemLevel);
        let price_map = if hydrated.get() {
            listings_resource.get().and_then(|r| r.ok())
        } else {
            None
        };
        items()
            .into_iter()
            .filter(|(id, _)| {
                if ItemSortOption::Price == item_property {
                    if let Some(map) = &price_map {
                        map.find_matching_listings(id.0).lowest_gil().is_some()
                    } else {
                        true
                    }
                } else {
                    true
                }
            })
            .sorted_by(|a, b| {
                let ((_, item_a), (_, item_b)) = match direction {
                    SortDirection::Asc => (a, b),
                    SortDirection::Desc => (b, a),
                };
                match item_property {
                    ItemSortOption::ItemLevel => item_a.level_item.cmp(&item_b.level_item),
                    ItemSortOption::Name => item_a.name.cmp(&item_b.name),
                    ItemSortOption::Price => {
                        if let Some(price_map) = &price_map {
                            let price_a = price_map
                                .find_matching_listings(item_a.key_id.0)
                                .lowest_gil();
                            let price_b = price_map
                                .find_matching_listings(item_b.key_id.0)
                                .lowest_gil();
                            price_a.cmp(&price_b)
                        } else {
                            item_a.level_item.cmp(&item_b.level_item)
                        }
                    }
                    ItemSortOption::Key => item_a.key_id.0.cmp(&item_b.key_id.0),
                }
            })
            .collect::<Vec<_>>()
    });

    // ⚡ Bolt Optimization: Replace Memo::new with Signal::derive for O(1) ops
    let items_len = Signal::derive(move || sorted_items.with(|i| i.len()));
    // Rows per page, clamped to the values the selector offers so a
    // hand-edited `?per_page=` can't produce a surprising page size.
    let (per_page_q, _) = query_signal::<usize>("per_page");
    let per_page = Signal::derive(move || match per_page_q().unwrap_or(50) {
        25 => 25,
        100 => 100,
        _ => 50,
    });
    let pages = Signal::derive(move || Pages::new(items_len(), per_page()));

    let filtered_items = Memo::new(move |_| {
        let page = pages
            .get()
            .with_offset((page().unwrap_or_default() - 1).try_into().unwrap_or(0));
        // `paginate::Pages::with_offset(out_of_range)` returns
        // `Page { start: 0, end: 0, length: 0 }`. Because we then index
        // with the *inclusive* range `start..=end`, that range degrades to
        // `0..=0` and silently surfaces `items[0]` instead of an empty
        // page — which then disagrees with the rest of the view (no active
        // pagination button, the "next page" CTA hidden, items_len/pages
        // saying 0) and makes tachys' hydration walker hit
        // `failed_to_cast_element` on `/items/jobset/<JOB>` deep-links
        // carried over from a different job set with more pages
        // (GlitchTip issues 4902/306/4911/3005/etc., URL pattern
        // `?page=35&sort=ilvl`). Bail out explicitly when the page is
        // empty so server and client render the same nothing.
        if page.is_empty() {
            return Vec::new();
        }
        sorted_items.with(|items| {
            items
                .get(page.start..=page.end)
                .unwrap_or_default()
                .to_vec()
        })
    });

    view! {
        <Suspense fallback=move || view! { <div class="flex justify-center p-10"><Loading /></div> }>
        <div class="flex flex-col gap-6">
            // Sort and Direction Controls - Floating / Sticky Bar
            <div class="flex flex-col sm:flex-row justify-between gap-4 p-4 rounded-xl panel items-center sticky top-[72px] lg:top-4 z-20 backdrop-blur-md bg-[color:var(--bg-panel)]/90 border border-white/5 shadow-lg">
                <div class="flex flex-row flex-wrap gap-2 items-center">
                    <span class="text-xs font-bold uppercase tracking-wider text-[color:var(--color-text-muted)] mr-2">{t!(i18n, item_explorer_sort_by)}</span>
                    <QueryButton
                        key="sort"
                        value="ilvl"
                        class="px-3 py-1.5 rounded-lg text-sm font-medium transition-colors text-[color:var(--color-text-muted)] hover:bg-white/5"
                        active_classes="px-3 py-1.5 rounded-lg text-sm font-medium !bg-brand-500/20 !text-brand-300 ring-1 ring-brand-500/50"
                        default=true
                    >
                        {t!(i18n, item_explorer_ilvl)}
                    </QueryButton>
                    <QueryButton
                        key="sort"
                        value="price"
                        class="px-3 py-1.5 rounded-lg text-sm font-medium transition-colors text-[color:var(--color-text-muted)] hover:bg-white/5"
                        active_classes="px-3 py-1.5 rounded-lg text-sm font-medium !bg-brand-500/20 !text-brand-300 ring-1 ring-brand-500/50"
                    >
                        {t!(i18n, item_explorer_price)}
                    </QueryButton>
                    <QueryButton
                        key="sort"
                        value="name"
                        class="px-3 py-1.5 rounded-lg text-sm font-medium transition-colors text-[color:var(--color-text-muted)] hover:bg-white/5"
                        active_classes="px-3 py-1.5 rounded-lg text-sm font-medium !bg-brand-500/20 !text-brand-300 ring-1 ring-brand-500/50"
                    >
                        {t!(i18n, item_explorer_name)}
                    </QueryButton>
                    <QueryButton
                        key="sort"
                        value="key"
                        class="px-3 py-1.5 rounded-lg text-sm font-medium transition-colors text-[color:var(--color-text-muted)] hover:bg-white/5"
                        active_classes="px-3 py-1.5 rounded-lg text-sm font-medium !bg-brand-500/20 !text-brand-300 ring-1 ring-brand-500/50"
                    >
                        {t!(i18n, item_explorer_added)}
                    </QueryButton>
                </div>
                <div class="flex flex-row gap-2 bg-black/20 p-1 rounded-lg">
                     <QueryButton
                        key="dir"
                        value="asc"
                        class="p-1.5 rounded text-[color:var(--color-text-muted)] hover:text-brand-200 transition-colors"
                        active_classes="p-1.5 rounded bg-white/10 !text-brand-300 shadow-sm"
                    >
                        <Icon icon=i::BiSortUpRegular width="20" height="20" />
                    </QueryButton>
                     <QueryButton
                        key="dir"
                        value="desc"
                        class="p-1.5 rounded text-[color:var(--color-text-muted)] hover:text-brand-200 transition-colors"
                        active_classes="p-1.5 rounded bg-white/10 !text-brand-300 shadow-sm"
                        default=true
                    >
                        <Icon icon=i::BiSortDownRegular width="20" height="20" />
                    </QueryButton>
                </div>
            </div>

            // Results list: one row per item so prices line up in a
            // scannable column. One responsive layout in three tiers:
            // below `lg` only icon/name/NQ/actions (the rest collapses
            // into a compact line under the name), `lg` adds iLvl/Lv/HQ,
            // `xl` adds vendor and world. The full column set can't come
            // in earlier than `xl` — the fixed columns plus the app
            // sidebar leave `1fr` with no room and the item name
            // collapses to zero width.
            <div role="table" class="panel rounded-xl border border-white/5 divide-y divide-white/5 overflow-hidden">
                <div
                    role="row"
                    class=move || {
                        if is_single_world.get() {
                            "hidden lg:grid lg:grid-cols-[2.5rem_minmax(6rem,1fr)_3.5rem_3rem_6.5rem_6.5rem_5rem] xl:grid-cols-[2.5rem_minmax(6rem,1fr)_3.5rem_3rem_6.5rem_6.5rem_6rem_5rem] items-center gap-x-3 px-3 py-2 text-xs font-bold uppercase tracking-wider text-[color:var(--color-text-muted)]"
                        } else {
                            "hidden lg:grid lg:grid-cols-[2.5rem_minmax(6rem,1fr)_3.5rem_3rem_6.5rem_6.5rem_5rem] xl:grid-cols-[2.5rem_minmax(6rem,1fr)_3.5rem_3rem_6.5rem_6.5rem_6rem_6.5rem_5rem] items-center gap-x-3 px-3 py-2 text-xs font-bold uppercase tracking-wider text-[color:var(--color-text-muted)]"
                        }
                    }
                >
                    <div role="columnheader"></div>
                    <SortHeader value="name">{t!(i18n, item_explorer_name)}</SortHeader>
                    <SortHeader value="ilvl" default=true>{t!(i18n, item_explorer_ilvl)}</SortHeader>
                    <div role="columnheader">{t!(i18n, item_explorer_col_equip_level)}</div>
                    <SortHeader value="price">{t!(i18n, nq)}</SortHeader>
                    <div role="columnheader">{t!(i18n, hq)}</div>
                    <div role="columnheader" class="hidden xl:block">
                        {t!(i18n, item_explorer_vendor)}
                    </div>
                    <div
                        role="columnheader"
                        class=move || {
                            if is_single_world.get() { "hidden" } else { "hidden xl:block" }
                        }
                    >
                        {t!(i18n, item_explorer_col_world)}
                    </div>
                    <div role="columnheader"></div>
                </div>
                <For
                    each=move || filtered_items.get()
                    key=|(id, item)| (id.0, item.name.clone())
                    children=move |(id, item)| {
                        let item_id = id.0;
                        view! {
                            <div
                                role="row"
                                class=move || {
                                    if is_single_world.get() {
                                        "grid grid-cols-[2.5rem_minmax(0,1fr)_auto_auto] lg:grid-cols-[2.5rem_minmax(6rem,1fr)_3.5rem_3rem_6.5rem_6.5rem_5rem] xl:grid-cols-[2.5rem_minmax(6rem,1fr)_3.5rem_3rem_6.5rem_6.5rem_6rem_5rem] items-center gap-x-3 px-3 py-2 hover:bg-white/5 transition-colors"
                                    } else {
                                        "grid grid-cols-[2.5rem_minmax(0,1fr)_auto_auto] lg:grid-cols-[2.5rem_minmax(6rem,1fr)_3.5rem_3rem_6.5rem_6.5rem_5rem] xl:grid-cols-[2.5rem_minmax(6rem,1fr)_3.5rem_3rem_6.5rem_6.5rem_6rem_6.5rem_5rem] items-center gap-x-3 px-3 py-2 hover:bg-white/5 transition-colors"
                                    }
                                }
                            >
                                <A href=move || format!("/item/{}/{}",
                                    scope_name.get(),
                                    item.key_id.0)
                                >
                                    <ItemIcon item_id=item.key_id.0 icon_size=IconSize::Small />
                                </A>
                                <div class="flex flex-col min-w-0">
                                    <A href=move || format!("/item/{}/{}",
                                        scope_name.get(),
                                        item.key_id.0)
                                        attr:class="font-medium leading-snug text-[color:var(--color-text)] truncate \
                                                   hover:text-brand-300 transition-colors \
                                                   hover:underline decoration-brand-300/30 underline-offset-4"
                                    >
                                        {item.name.as_str()}
                                    </A>
                                    // Compact metadata, only below `lg` where the
                                    // dedicated columns are hidden.
                                    <div class="flex lg:hidden items-center gap-2 text-xs text-[color:var(--color-text-muted)]">
                                        <span>{t!(i18n, item_explorer_ilvl_prefix)} " "{item.level_item}</span>
                                        <div>
                                            {if item.level_equip > 1 {
                                                view! {
                                                    <span>{t!(i18n, item_explorer_lv_prefix)} " "{item.level_equip}</span>
                                                }.into_any()
                                            } else {
                                                ().into_any()
                                            }}
                                        </div>
                                    </div>
                                </div>
                                <div role="cell" class="hidden lg:block text-sm text-[color:var(--color-text-muted)]">
                                    {item.level_item}
                                </div>
                                <div role="cell" class="hidden lg:block text-sm text-[color:var(--color-text-muted)]">
                                    {if item.level_equip > 1 {
                                        view! { <span>{item.level_equip}</span> }.into_any()
                                    } else {
                                        view! { <span>"—"</span> }.into_any()
                                    }}
                                </div>
                                <div role="cell" class="text-sm">
                                    <CheapestPrice item_id=*id show_hq=false show_world=false />
                                </div>
                                // Always emit a stable wrapper div so the SSR and CSR view
                                // trees agree on element shape/count for this slot (same
                                // tachys-hydration reasoning as the old card layout).
                                <div role="cell" class="hidden lg:block text-sm">
                                    {if item.can_be_hq {
                                        view! {
                                            <CheapestPrice item_id=*id show_hq=true show_world=false />
                                        }.into_any()
                                    } else {
                                        ().into_any()
                                    }}
                                </div>
                                <div role="cell" class="hidden xl:block text-sm">
                                    {if let Some(price) = crate::components::related_items::get_vendor_price(item_id) {
                                        view! { <Gil amount=price as i32 /> }.into_any()
                                    } else {
                                        ().into_any()
                                    }}
                                </div>
                                // World holding the cheapest listing. Gated behind the
                                // same `hydrated` flag as the price sort — SSR and the
                                // first CSR render both show nothing, keeping shapes
                                // in sync (see comment on `hydrated` above).
                                <div
                                    role="cell"
                                    class=move || {
                                        if is_single_world.get() {
                                            "hidden"
                                        } else {
                                            "hidden xl:block truncate text-sm text-[color:var(--color-text-muted)]"
                                        }
                                    }
                                >
                                    {move || {
                                        if !hydrated.get() {
                                            return ().into_any();
                                        }
                                        listings_resource
                                            .with(|data| {
                                                data.as_ref().and_then(|result| {
                                                    result.as_ref().ok().and_then(|map| {
                                                        let summary = map.find_matching_listings(item_id);
                                                        let best = match (summary.lq, summary.hq) {
                                                            (Some(lq), Some(hq)) => {
                                                                Some(if hq.price < lq.price { hq } else { lq })
                                                            }
                                                            (lq, hq) => lq.or(hq),
                                                        };
                                                        best.map(|listing| {
                                                            view! {
                                                                <WorldName id=AnySelector::World(listing.world_id) />
                                                            }
                                                            .into_any()
                                                        })
                                                    })
                                                })
                                            })
                                            .unwrap_or_else(|| ().into_any())
                                    }}
                                </div>
                                <div role="cell" class="flex items-center justify-end gap-1">
                                    <AddToList
                                        item_id=item_id
                                        class="flex items-center justify-center p-2 rounded hover:bg-white/10 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)] transition-colors"
                                    />
                                    <div class="p-1 rounded hover:bg-white/10 text-[color:var(--color-text-muted)] cursor-pointer" title=t_string!(i18n, item_explorer_copy_name).to_string()>
                                        <Clipboard clipboard_text=item.name.clone() />
                                    </div>
                                </div>
                            </div>
                        }
                        .into_any()
                    }
                />
            </div>

            // Pagination + rows per page
            <div class="flex flex-col sm:flex-row items-center justify-center gap-4 mt-6">
                 <div class="flex flex-wrap justify-center gap-2 p-2 rounded-xl bg-[color:var(--bg-panel)]/50 border border-white/5">
                    {move || {
                        pages.get()
                            .map(|page| {
                                view! {
                                    <QueryButton
                                        key="page"
                                        value=(page.offset + 1).to_string()
                                        class="w-10 h-10 flex items-center justify-center rounded-lg text-sm font-medium transition-all
                                               text-[color:var(--color-text-muted)] hover:bg-white/10 hover:text-brand-200"
                                        active_classes="w-10 h-10 flex items-center justify-center rounded-lg text-sm font-medium transition-all !bg-brand-500 !text-white shadow-lg shadow-brand-500/20 scale-105"
                                        default=page.offset == 0
                                    >
                                        {page.offset + 1}
                                    </QueryButton>
                                }
                            })
                            .collect::<Vec<_>>()
                    }}
                </div>
                <div class="flex items-center gap-2 p-2 rounded-xl bg-[color:var(--bg-panel)]/50 border border-white/5">
                    <span class="text-xs font-bold uppercase tracking-wider text-[color:var(--color-text-muted)]">
                        {t!(i18n, item_explorer_rows_per_page)}
                    </span>
                    <QueryButton
                        key="per_page"
                        value="25"
                        remove_queries=&["page"]
                        class="px-2.5 py-1.5 rounded-lg text-sm font-medium transition-colors text-[color:var(--color-text-muted)] hover:bg-white/5"
                        active_classes="px-2.5 py-1.5 rounded-lg text-sm font-medium !bg-brand-500/20 !text-brand-300 ring-1 ring-brand-500/50"
                    >
                        "25"
                    </QueryButton>
                    <QueryButton
                        key="per_page"
                        value="50"
                        default=true
                        remove_queries=&["page"]
                        class="px-2.5 py-1.5 rounded-lg text-sm font-medium transition-colors text-[color:var(--color-text-muted)] hover:bg-white/5"
                        active_classes="px-2.5 py-1.5 rounded-lg text-sm font-medium !bg-brand-500/20 !text-brand-300 ring-1 ring-brand-500/50"
                    >
                        "50"
                    </QueryButton>
                    <QueryButton
                        key="per_page"
                        value="100"
                        remove_queries=&["page"]
                        class="px-2.5 py-1.5 rounded-lg text-sm font-medium transition-colors text-[color:var(--color-text-muted)] hover:bg-white/5"
                        active_classes="px-2.5 py-1.5 rounded-lg text-sm font-medium !bg-brand-500/20 !text-brand-300 ring-1 ring-brand-500/50"
                    >
                        "100"
                    </QueryButton>
                </div>
            </div>
            // Next Page Big Button (if applicable)
             <QueryButton
                key="page"
                value=Signal::derive(move || (page().unwrap_or(1) + 1).to_string())
                class=Signal::derive(move || {
                    let pages = pages.get();
                    let page = page();
                    if pages.page_count() > page.unwrap_or(1).try_into().unwrap_or(1) {
                        "w-full py-4 rounded-xl text-center font-bold
                             border border-[color:var(--color-outline)]
                             hover:border-brand-300/60 hover:shadow-lg hover:translate-y-[-2px]
                             text-brand-300 transition-all duration-300 group"
                    } else {
                        "hidden"
                    }
                })
                active_classes=""
            >
                <div class="flex items-center justify-center gap-2">
                    <span>{t!(i18n, item_explorer_load_next_page)}</span>
                    <Icon icon=i::BiChevronRightRegular attr:class="group-hover:translate-x-1 transition-transform" />
                </div>
            </QueryButton>
            <div class="h-8" /> // Bottom spacing
        </div>
        </Suspense>
    }.into_any()
}

#[component]
pub fn ItemExplorer() -> impl IntoView {
    // Rescope prices for the whole explorer subtree: shadow the global
    // `CheapestPrices` context (keyed on the PRICE_ZONE cookie) with a
    // resource keyed on the page's own `?world=`-driven scope. Every
    // descendant (`ItemList`, `CheapestPrice`, `JobSetCard`,
    // `JobSetDetail`) picks the scoped resource up via `use_context`
    // without signature changes.
    let scope = use_explorer_price_scope();
    let scope_name = scope.name;
    // The root-level `CheapestPrices` (lib.rs) already loads the cookie
    // zone's listings on every page. Grab its handle *before* shadowing
    // so the common no-`?world=` case reuses that fetch instead of
    // issuing a duplicate request for the same zone.
    let global_prices = use_context::<CheapestPrices>();
    let (cookie_zone, _) = crate::global_state::home_world::get_price_zone();
    let cookie_zone_name = Signal::derive(move || {
        cookie_zone
            .get()
            .map(|z| z.get_name().to_string())
            .unwrap_or_else(|| "North-America".to_string())
    });
    let read_listings = Resource::new(
        move || (scope_name.get(), cookie_zone_name.get()),
        move |(world, cookie_world)| {
            let global_prices = global_prices.clone();
            async move {
                if let Some(global) = global_prices.filter(|_| world == cookie_world) {
                    return global.read_listings.await;
                }
                crate::api::get_cheapest_listings(&world)
                    .await
                    .map(|cheapest_prices| {
                        ultros_api_types::cheapest_listings::CheapestListingsMap::from(
                            cheapest_prices,
                        )
                    })
            }
        },
    );
    provide_context(CheapestPrices { read_listings });
    provide_context(scope);
    view! {
        <div class="flex flex-col min-h-[calc(100vh-56px)]">
            <div class="p-4 lg:p-8 max-w-[1600px] mx-auto w-full">
                <crate::routes::item_explorer_toolbar::ItemExplorerToolbar />
                <Outlet />
            </div>
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::collect_job_items_sorted;
    use paginate::Pages;

    /// Regression for the `?page=35` family of GlitchTip hydration
    /// panics on `/items/jobset/<JOB>`. `paginate::Pages::with_offset`
    /// returns `Page { start: 0, end: 0, length: 0 }` for any
    /// out-of-bounds offset — and the inclusive `start..=end` slice
    /// `0..=0` then surfaces the first item on what should be an empty
    /// page. `ItemList::filtered_items` must treat that page as empty
    /// rather than indexing into the slice.
    #[test]
    fn paginate_oob_offset_reports_empty_but_inclusive_range_is_not() {
        let pages = Pages::new(200, 50); // 4 valid pages (offsets 0..=3)
        let page = pages.with_offset(34); // ?page=35 → offset 34, far past end
        assert_eq!(page.length, 0, "OOB page must have length 0");
        assert!(page.is_empty());
        // The trap we have to guard against in `filtered_items`: the
        // inclusive range `start..=end` covers index 0 even though the
        // page is supposed to be empty. Don't fix this here, just
        // document it so the production code stays vigilant.
        assert_eq!(
            page.start..=page.end,
            0..=0,
            "OOB Page's start..=end range degrades to 0..=0 (includes index 0!)",
        );
        let items: Vec<i32> = (0..200).collect();
        assert_eq!(
            items.get(page.start..=page.end).unwrap_or_default(),
            &[0],
            "the dangerous behavior: items[0..=0] is items[0], not empty",
        );
    }

    #[test]
    fn collect_job_items_returns_ascending_ids() {
        // Hydration regression for tachys `hydration.rs:163`
        // (`failed_to_cast_element`) panics on `/items/jobset/<JOB>`:
        // `data.items` is a HashMap whose iteration order differs between
        // the server's SSR process and the client's WASM process, and the
        // downstream `For` / card-grid rendering is order-sensitive during
        // hydration. The helper must return a deterministic order so the
        // SSR DOM and client view tree match.
        let data = xiv_gen_db::data();
        let items = collect_job_items_sorted(data, "DNC", true);
        assert!(
            !items.is_empty(),
            "DNC should match a non-trivial number of items",
        );
        for w in items.windows(2) {
            assert!(
                w[0].0.0 < w[1].0.0,
                "items must be in strictly ascending ItemId order \
                 (hydration safety): {} >= {}",
                w[0].0.0,
                w[1].0.0,
            );
        }
    }

    #[test]
    fn test_job_filtering() {
        let data = xiv_gen_db::data();
        let jobs = &data.class_jobs;
        let visible_jobs: Vec<_> = jobs
            .iter()
            .filter(|(_, job)| job.job_index > 0 || job.doh_dol_job_index >= 0)
            .filter(|(_, job)| !job.abbreviation.is_empty() || !job.name.is_empty())
            .collect();

        println!("Visible jobs count: {}", visible_jobs.len());
        for (id, job) in &visible_jobs {
            let seg = if job.abbreviation.is_empty() {
                job.name.as_str()
            } else {
                job.abbreviation.as_str()
            };
            println!(
                "Visible: {} (ID: {}) Abbr: '{}' Seg: '{}'",
                job.name, id.0, job.abbreviation, seg
            );
            assert!(!seg.is_empty(), "Segment should not be empty");
        }

        assert!(
            visible_jobs.iter().any(|(_, j)| j.name == "samurai"),
            "Samurai should be visible."
        );
        assert!(
            visible_jobs.iter().any(|(_, j)| j.name == "carpenter"),
            "Carpenter should be visible."
        );
        assert!(
            !visible_jobs.iter().any(|(_, j)| j.name == "marauder"),
            "Marauder should not be visible."
        );
        // Ensure invalid jobs are filtered out
        assert!(
            !visible_jobs.iter().any(|(id, _)| id.0 == 99),
            "Job 99 should be filtered out"
        );
        assert!(
            !visible_jobs.iter().any(|(id, _)| id.0 == 44),
            "Job 44 should be filtered out"
        );
        assert!(
            !visible_jobs.iter().any(|(id, _)| id.0 == 45),
            "Job 45 should be filtered out"
        );
    }
}
