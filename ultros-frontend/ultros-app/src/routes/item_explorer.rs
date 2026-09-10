use crate::components::app_link::{AppLink, use_location_or_default};
use std::collections::HashSet;
use std::fmt::Display;
use std::str::FromStr;
use std::sync::Arc;

use crate::CheapestPrices;
use crate::analyzer_kit::filters::{register_filters, toggle_control};
use crate::analyzer_kit::market::{MarketGrid, use_market_data_on_demand};
use crate::analyzer_kit::stat_columns::{market_picker_options, shared_cols_in, toggle_shared_col};
use crate::analyzer_kit::window::MarketWindowControl;
use crate::components::clipboard::Clipboard;
use crate::components::control_bar::{
    ColumnOption, ControlBar, parse_visible_cols, serialize_visible_cols,
};
use crate::components::gil::Gil;
use crate::components::icon::Icon;
use crate::components::item_tooltip::ItemTooltip;
use crate::components::job_set_card::JobSetCard;
use crate::components::job_set_grouping::{GroupableItem, group_into_sets};
use crate::components::loading::Loading;
use crate::components::related_items::get_vendor_price;
use crate::components::sort_header::{SortColumn, SortDir, SortHeader, cmp_none_last};
use crate::components::toggle::Toggle;
use crate::components::virtual_grid::GridColumn;
use crate::components::virtual_grid::metrics::{GridMetric, GridValue};
use crate::components::world_name::WorldName;
use crate::components::{add_to_list::*, item_icon::*, meta::*};
use crate::global_state::local_world_data::use_world_helper;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use crate::query_defaults::{filter_query_signal, query_signal};
use crate::routes::item_explorer_filters::{
    COL_ACTIONS, COL_EQUIP_LEVEL, COL_HQ, COL_ITEM, COL_ITEM_LEVEL, COL_KEY, COL_LISTING, COL_NQ,
    COL_VENDOR, COL_WORLD, ExplorerFilters, ExplorerRow, FILTER_HQ, column_availability,
    explorer_filter_aliases, has_equip_level,
};
use crate::routes::item_explorer_scope::{ExplorerPriceScope, use_explorer_price_scope};
use crate::routes::item_explorer_toolbar::jobset_display_label;
use icondata as i;
use leptos::prelude::*;
use leptos::reactive::wrappers::write::SignalSetter;
use leptos_router::components::Outlet;
use leptos_router::hooks::use_params_map;
use percent_encoding::percent_decode_str;
use thousands::Separable;
use ultros_api_types::world_helper::AnySelector;
use xiv_gen::{
    ClassJobCategory, ClassJobCategoryId, ClassJobId, Item, ItemId, ItemSearchCategory,
    ItemSearchCategoryId,
};

/// Canonical, locale-independent acronym for a `ClassJob`, keyed on its id.
///
/// `ClassJob::abbreviation` is a *localized display string*: German calls
/// Pugilist "FST" (Faustkämpfer) and French "PUG", while `ClassJobCategory`'s
/// columns — the flags [`job_category_lookup`] matches on — are English
/// acronyms baked into the sheet schema and therefore identical in every
/// locale. Anything that uses an abbreviation as a *key* (a route param, a
/// role bucket, an href) must go through this table instead of reading
/// `abbreviation`, or it resolves on English data and nowhere else.
///
/// The ids are the `ClassJob` sheet's row ids and the order matches
/// [`job_category_lookup`]'s destructure exactly, because the
/// `ClassJobCategory` sheet's per-job columns are laid out in `ClassJob` id
/// order. `canonical_acronym_matches_english_abbreviation` pins the table
/// against the shipped English pack, so a game-data bump that renumbers or
/// adds a job fails the test rather than silently emptying a route.
pub(crate) fn canonical_job_acronym(id: ClassJobId) -> Option<&'static str> {
    // Ids 0..=42 line up with `ClassJobCategory`'s columns. "BST" (43) has a
    // `ClassJob` row but no category column in the shipped pack — it still
    // belongs here so role grouping and display resolve, even though nothing
    // can select its gear (see `only_beastmaster_lacks_a_category_column`).
    const ACRONYMS: [&str; 44] = [
        "ADV", "GLA", "PGL", "MRD", "LNC", "ARC", "CNJ", "THM", "CRP", "BSM", "ARM", "GSM", "LTW",
        "WVR", "ALC", "CUL", "MIN", "BTN", "FSH", "PLD", "MNK", "WAR", "DRG", "BRD", "WHM", "BLM",
        "ACN", "SMN", "SCH", "ROG", "NIN", "MCH", "DRK", "AST", "SAM", "RDM", "BLU", "GNB", "DNC",
        "RPR", "SGE", "VPR", "PCT", "BST",
    ];
    usize::try_from(id.0)
        .ok()
        .and_then(|i| ACRONYMS.get(i))
        .copied()
}

/// Resolve the `/items/jobset/:jobset` route param to a canonical,
/// locale-independent job acronym suitable for [`job_category_lookup`].
///
/// The canonical param is the **English acronym**, for the same reason
/// `/items/category/:category` is keyed on a numeric id (see
/// [`resolve_category_param`]): the server initializes `xiv_gen_db` to
/// `Language::En` and never swaps it, while the client `try_init`s the
/// visitor's locale *before* hydrating. A param keyed on the localized
/// `abbreviation` resolves on at most one of the two sides.
///
/// Matching the acronym table first means the canonical form resolves
/// identically in both processes regardless of which locale each loaded. The
/// localized `abbreviation`/`name` match is kept as a fallback so links minted
/// before this change (and the English full names in the search index) keep
/// resolving; it carries exactly the locale caveat it always had, the same
/// tradeoff `resolve_category_param` makes for legacy name-keyed category
/// links.
pub(crate) fn resolve_jobset_param(data: &xiv_gen::Data, raw_param: &str) -> Option<String> {
    let decoded = percent_decode_str(raw_param)
        .decode_utf8()
        .map(|s| s.to_string())
        .unwrap_or_else(|_| raw_param.to_string());

    // Locale-independent path: the param already is a canonical acronym.
    if let Some(canonical) = data
        .class_jobs
        .keys()
        .filter_map(|id| canonical_job_acronym(*id))
        .find(|acronym| acronym.eq_ignore_ascii_case(&decoded))
    {
        return Some(canonical.to_string());
    }

    // Legacy fallback: a localized abbreviation or a full job name. Resolves
    // against whichever locale *this* process loaded, so it is inherently
    // one-sided for non-English visitors — but it only ever fires for
    // non-canonical URLs, which today resolve to nothing at all.
    data.class_jobs
        .iter()
        .find(|(_id, job)| {
            job.abbreviation.eq_ignore_ascii_case(&decoded)
                || job.name.eq_ignore_ascii_case(&decoded)
        })
        .and_then(|(id, _job)| canonical_job_acronym(*id))
        .map(|acronym| acronym.to_string())
}

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

/// Resolve the `/items/category/:category` route param to its
/// [`ItemSearchCategory`].
///
/// The canonical param is the **numeric** `ItemSearchCategory` id, because ids
/// are locale-independent. `ItemSearchCategory::name` is a *localized display
/// string* ("Shields" / "盾" / "Schilde"), and the two renderers of a page do
/// not share a locale: the server initializes `xiv_gen_db` to `Language::En`
/// and never swaps it, while the client calls `try_init` with the visitor's
/// locale *before* hydrating. A name-keyed param therefore resolves on exactly
/// one of the two sides — SSR renders an empty list where the client builds a
/// full one (a chip minted by a non-English client), or the reverse (the
/// English links in the sitemap and the search index). Either way the client's
/// view tree disagrees with the SSR DOM and tachys panics in
/// `failed_to_cast_element`, the same class as #960.
///
/// The name match is kept as a fallback so links minted before the switch to
/// ids keep resolving; it carries exactly the locale caveat it always had.
/// Duplicate names (`Primary Tools` is both id 2 and id 3) are broken by
/// lowest id rather than by `HashMap` iteration order, so the fallback is at
/// least deterministic across the two processes.
pub(crate) fn resolve_category_param<'a>(
    data: &'a xiv_gen::Data,
    raw_param: &str,
) -> Option<&'a ItemSearchCategory> {
    let decoded = percent_decode_str(raw_param).decode_utf8().ok()?;
    if let Ok(id) = decoded.parse::<i32>() {
        return data.item_search_categorys.get(&ItemSearchCategoryId(id));
    }
    data.item_search_categorys
        .values()
        .filter(|category| category.name == decoded)
        .min_by_key(|category| category.key_id.0)
}

#[component]
pub fn CategoryItems() -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    let params = use_params_map();
    let data = tracked_data();
    let items = Memo::new(move |_| {
        let cat = params()
            .get_str("category")
            .and_then(|cat| resolve_category_param(data, cat))
            .map(|category| {
                let mut items: Vec<_> = data
                    .items
                    .iter()
                    .filter(|(_, item)| item.item_search_category == category.key_id.0)
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
        // Resolve the id back to a display name. Falls through to the raw
        // param for legacy name-keyed links that no longer resolve, so an
        // unrecognised category still names itself in the heading.
        params()
            .get_str("category")
            .and_then(|cat| resolve_category_param(data, cat))
            .map(|category| category.name.clone())
            .or_else(|| {
                params()
                    .get("category")
                    .as_ref()
                    .and_then(|cat| percent_decode_str(cat).decode_utf8().ok())
                    .map(|c| c.to_string())
            })
            .unwrap_or_else(|| crate::i18n::t_string!(i18n, category_view_default).to_string())
    });
    // Legacy name-keyed links still resolve (see `resolve_category_param`), so
    // the same category is reachable under several URLs. Point them all at the
    // id-keyed form so the duplicates consolidate instead of competing.
    let canonical_href = move || {
        let params = params();
        let raw = params.get_str("category").unwrap_or("");
        match resolve_category_param(data, raw) {
            Some(category) => format!("https://ultros.app/items/category/{}", category.key_id.0),
            None => format!("https://ultros.app/items/category/{raw}"),
        }
    };
    view! {
        <MetaCanonical href=canonical_href />
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
        // Resolve to the canonical English acronym. Reading `abbreviation`
        // straight off the matched job would hand `job_category_lookup` a
        // localized string it cannot match — see `resolve_jobset_param`.
        let raw = match params().get("jobset") {
            Some(p) => p.clone(),
            None => return vec![],
        };
        let canonical_abbr = match resolve_jobset_param(data, &raw) {
            Some(abbr) => abbr,
            None => return vec![],
        };

        collect_job_items_sorted(data, &canonical_abbr, market_only())
    });
    // Heading/meta text: the param is a canonical English acronym, so resolve
    // it back to the visitor's localized label the way `CategoryItems` does
    // with its numeric category id. Falls through to the raw param so an
    // unrecognised jobset still names itself.
    let job_set = Memo::new(move |_| {
        params()
            .get("jobset")
            .as_ref()
            .and_then(|raw| {
                jobset_display_label(data, raw).or_else(|| {
                    percent_decode_str(raw)
                        .decode_utf8()
                        .ok()
                        .map(|s| s.to_string())
                })
            })
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

    // `?show-non-market=` genuinely changes the item set, but the default
    // (marketable items only) is the representative view; canonicalising to it
    // keeps the toggled variant from being crawled as thin duplicate content.
    // Legacy name-keyed and localized-abbreviation links still resolve (see
    // `resolve_jobset_param`), so one job is reachable under several URLs —
    // point them all at the canonical acronym so the duplicates consolidate
    // instead of competing, exactly as `CategoryItems` does with its id.
    let canonical_href = move || {
        let raw = jobset_param.get();
        match resolve_jobset_param(data, &raw) {
            Some(acronym) => format!("https://ultros.app/items/jobset/{acronym}"),
            None => format!("https://ultros.app/items/jobset/{raw}"),
        }
    };

    view! {
        <MetaCanonical href=canonical_href />
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
        <MetaCanonical href="https://ultros.app/items" />
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

/// Every column the explorer can order by. One variant per column that
/// carries data — issue #1296: the table used to render eight columns and
/// sort by four of them, so "cheapest vendor minion" was a question the page
/// displayed the answer to but could not be asked.
///
/// The token each variant `Display`s is the `?sort=` value and is also the
/// grid column id (and the `?cols=` id for the optional ones), so the two
/// never need a translation table between them. The shared grid adds its
/// own `?sort=grid:<column>` form on top for every metric column; the native
/// tokens stay so every bookmark from before the grid keeps its order.
#[derive(PartialEq, Eq, PartialOrd, Copy, Clone, Debug)]
pub(crate) enum ItemSortOption {
    ItemLevel,
    EquipLevel,
    Price,
    HqPrice,
    Vendor,
    World,
    Name,
    Key,
}

/// Sort options in the order the sort menu offers them, which is also the
/// left-to-right order of the columns they sort.
const SORT_OPTIONS: [ItemSortOption; 8] = [
    ItemSortOption::Name,
    ItemSortOption::ItemLevel,
    ItemSortOption::EquipLevel,
    ItemSortOption::Price,
    ItemSortOption::HqPrice,
    ItemSortOption::Vendor,
    ItemSortOption::World,
    ItemSortOption::Key,
];

impl ItemSortOption {
    /// The optional column this sort reads, when it reads one. A sort whose
    /// column the current item set cannot fill is not offered — sorting
    /// minions by equip level would silently be a no-op.
    fn column(self) -> Option<&'static str> {
        Some(match self {
            ItemSortOption::ItemLevel => COL_ITEM_LEVEL,
            ItemSortOption::EquipLevel => COL_EQUIP_LEVEL,
            ItemSortOption::HqPrice => COL_HQ,
            ItemSortOption::Vendor => COL_VENDOR,
            ItemSortOption::World => COL_WORLD,
            // Name, NQ price and "added" have columns that are always on.
            _ => return None,
        })
    }

    /// The grid column whose header this sort sits on. "Added" has no
    /// column of its own at any width; the sort menu is its only control.
    fn for_column(id: &str) -> Option<Self> {
        Some(match id {
            COL_ITEM => ItemSortOption::Name,
            COL_ITEM_LEVEL => ItemSortOption::ItemLevel,
            COL_EQUIP_LEVEL => ItemSortOption::EquipLevel,
            COL_NQ => ItemSortOption::Price,
            COL_HQ => ItemSortOption::HqPrice,
            COL_VENDOR => ItemSortOption::Vendor,
            COL_WORLD => ItemSortOption::World,
            _ => return None,
        })
    }
}

impl FromStr for ItemSortOption {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "ilvl" => ItemSortOption::ItemLevel,
            "lv" => ItemSortOption::EquipLevel,
            "price" => ItemSortOption::Price,
            "hq" => ItemSortOption::HqPrice,
            "vendor" => ItemSortOption::Vendor,
            "world" => ItemSortOption::World,
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
            ItemSortOption::EquipLevel => "lv",
            ItemSortOption::Price => "price",
            ItemSortOption::HqPrice => "hq",
            ItemSortOption::Vendor => "vendor",
            ItemSortOption::World => "world",
            ItemSortOption::Name => "name",
            ItemSortOption::Key => "key",
        };
        f.write_str(val)
    }
}

/// The Item Explorer's list is item-level-descending until the visitor says
/// otherwise.
impl SortColumn for ItemSortOption {
    fn fallback() -> Self {
        ItemSortOption::ItemLevel
    }

    /// Numbers read best-first descending, which is the shared default. Text
    /// columns do not: a world list and an item list both want A first, and
    /// arriving at them reversed buries what the click asked for.
    fn default_dir(self) -> SortDir {
        match self {
            ItemSortOption::Name | ItemSortOption::World => SortDir::Asc,
            _ => SortDir::Desc,
        }
    }
}

/// Ids of the columns the visitor can switch off, as persisted in `?cols=`.
///
/// Deliberately the same tokens `ItemSortOption` writes to `?sort=`: the two
/// name the same column, and a second vocabulary for it would be one more
/// mapping to keep in step. The shared `market-*` ids ride in the same param
/// beside these; `shared_cols_in` reads them back out.
pub(crate) const COL_ID_ITEM_LEVEL: &str = COL_ITEM_LEVEL;
pub(crate) const COL_ID_EQUIP_LEVEL: &str = COL_EQUIP_LEVEL;
pub(crate) const COL_ID_HQ: &str = COL_HQ;
pub(crate) const COL_ID_VENDOR: &str = COL_VENDOR;
pub(crate) const COL_ID_WORLD: &str = COL_WORLD;

/// The switchable native columns, in grid order. Item, NQ price and the row
/// actions are not here: they carry the row's identity and its one
/// always-meaningful number, so there is nothing to gain from hiding them.
const OPTIONAL_COLUMNS: &[&str] = &[
    COL_ID_ITEM_LEVEL,
    COL_ID_EQUIP_LEVEL,
    COL_ID_HQ,
    COL_ID_VENDOR,
    COL_ID_WORLD,
];

/// Every optional native column is on by default; the ones the current item
/// set cannot fill are then left out of the grid by [`ColumnAvailability`],
/// so a category picks its own columns without the visitor touching the
/// picker. No shared market column is on by default: a bare category page
/// makes no `sale_stats` request at all.
const DEFAULT_COLUMNS: &[&str] = OPTIONAL_COLUMNS;

/// Apply a sort direction to an ordering.
///
/// The counterpart of [`cmp_none_last`] for columns whose value is always
/// there — it, not this, handles the ones that can be missing, because
/// reversing an ordering that already sank the empty rows would float them
/// back to the top.
fn ordered(dir: SortDir, ordering: std::cmp::Ordering) -> std::cmp::Ordering {
    match dir {
        SortDir::Asc => ordering,
        SortDir::Desc => ordering.reverse(),
    }
}

/// Order the rows in place by a native sort. Stable, so a price sort before
/// prices load (every key `None`) leaves the item-id order the rows arrived
/// in — the same order the server rendered.
///
/// `world_name` resolves a listing's world to its display name; the World
/// sort orders by that, and a row with no listing (or no world list) sorts
/// last in both directions.
fn sort_rows(
    rows: &mut [ExplorerRow],
    mode: ItemSortOption,
    dir: SortDir,
    world_name: impl Fn(i32) -> Option<String>,
) {
    // `cmp_none_last` for everything that can be absent: a row with no
    // listing, no vendor and no equip level belongs at the bottom in *both*
    // directions, not dragged to the top the moment the reader asks for
    // best-first.
    rows.sort_by(|a, b| match mode {
        ItemSortOption::ItemLevel => ordered(dir, a.item.level_item.cmp(&b.item.level_item)),
        ItemSortOption::EquipLevel => cmp_none_last(
            has_equip_level(a.item).then_some(a.item.level_equip),
            has_equip_level(b.item).then_some(b.item.level_equip),
            dir,
            i32::cmp,
        ),
        ItemSortOption::Name => ordered(dir, a.item.name.cmp(&b.item.name)),
        ItemSortOption::Price => cmp_none_last(a.nq, b.nq, dir, i32::cmp),
        ItemSortOption::HqPrice => cmp_none_last(a.hq, b.hq, dir, i32::cmp),
        ItemSortOption::Vendor => cmp_none_last(a.vendor, b.vendor, dir, u32::cmp),
        ItemSortOption::World => cmp_none_last(
            a.cheapest.and_then(|c| world_name(c.world_id)),
            b.cheapest.and_then(|c| world_name(c.world_id)),
            dir,
            String::cmp,
        ),
        ItemSortOption::Key => ordered(dir, a.item_id.cmp(&b.item_id)),
    });
}

/// `?cols=` after flipping one picker entry. Native ids are re-serialised in
/// table order; shared `market-*` ids are kept exactly where they were, so a
/// visitor who added a sale-history column and then hid the vendor column
/// does not lose the first change to the second.
///
/// Toggling a *shared* column with no `?cols=` in the URL has to write the
/// native defaults out too: once the param exists, the grid shows only the
/// optional columns it names.
fn toggled_cols(
    previous: Option<&str>,
    visible: &HashSet<&'static str>,
    id: &'static str,
) -> String {
    let native_defaults = || serialize_visible_cols(visible, OPTIONAL_COLUMNS);
    if OPTIONAL_COLUMNS.contains(&id) {
        let mut set = visible.clone();
        if !set.remove(id) {
            set.insert(id);
        }
        let mut ids: Vec<String> = serialize_visible_cols(&set, OPTIONAL_COLUMNS)
            .split(',')
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
            .collect();
        ids.extend(
            previous
                .unwrap_or("")
                .split(',')
                .filter(|t| !t.is_empty() && !OPTIONAL_COLUMNS.contains(t))
                .map(str::to_owned),
        );
        ids.join(",")
    } else {
        let base = previous.map(str::to_owned).unwrap_or_else(native_defaults);
        toggle_shared_col(Some(&base), "", id)
    }
}

/// A price cell: nothing before prices load, a dash for a loaded map with
/// no listing, otherwise the gil amount.
fn price_cell(price: Option<i32>, loaded: bool) -> AnyView {
    match (price, loaded) {
        (Some(price), _) => view! { <div class="text-sm"><Gil amount=price /></div> }.into_any(),
        (None, true) => {
            view! { <span class="text-sm text-[color:var(--color-text-muted)]">"\u{2014}"</span> }
                .into_any()
        }
        (None, false) => ().into_any(),
    }
}

#[component]
fn ItemList(items: Memo<Vec<(&'static ItemId, &'static Item)>>) -> impl IntoView {
    let i18n = use_i18n();
    let location = use_location_or_default();
    let grid_sort = Memo::new(move |_| {
        location
            .query
            .with(|q| q.get("sort").filter(|s| s.starts_with("grid:")))
    });
    let (direction, _set_direction) = query_signal::<SortDir>("dir");
    let (sort, _set_sort) = query_signal::<ItemSortOption>("sort");
    // The one legacy filter that is not a grid metric (see
    // `item_explorer_filters`): applied here, before the rows reach the grid.
    let (hq_only, _set_hq_only) = filter_query_signal::<bool>(FILTER_HQ);

    let cheapest_prices = use_context::<CheapestPrices>().unwrap();
    let listings_resource = cheapest_prices.read_listings;
    let scope = use_context::<ExplorerPriceScope>()
        .expect("ItemList is always rendered inside ItemExplorer, which provides the scope");
    let scope_name = scope.name;
    let is_single_world = scope.is_single_world;
    // Resolved once, here, rather than per comparison: the World sort needs a
    // display name for every listing it orders. `use_world_helper` is the
    // non-panicking read `WorldName` already makes — an absent or failed world
    // list simply leaves every row's world name `None`, which sorts them all
    // last instead of taking the page down.
    let worlds = StoredValue::new(use_world_helper().ok());
    let world_name = move |world_id: i32| -> Option<String> {
        worlds.with_value(|worlds| {
            worlds.as_ref().and_then(|worlds| {
                worlds
                    .lookup_selector(AnySelector::World(world_id))
                    .map(|world| world.get_name().to_string())
            })
        })
    };

    // Defer everything price-based until after hydration.
    //
    // On SSR the listings resource is `None` at render time (the wrapping
    // `<Suspense>` never suspends — `.get()` doesn't subscribe-and-suspend the
    // way `.read()` does), so the SSR HTML reflects the ilvl fallback with NO
    // price filter applied. On the client, Leptos serialises the resolved
    // resource into the payload so `listings_resource.get()` returns
    // `Some(map)` immediately during hydration — which would make the first
    // CSR render apply the price filter (dropping items without listings)
    // AND sort by price. The resulting row list then mismatches the SSR DOM
    // in both count and order, and tachys' walker panics at
    // `hydration.rs:163`/`:195` (`failed_to_cast_element`). That was the
    // `?sort=price`/`?page=N` cluster in GlitchTip (issues 707, 156, 4951,
    // 5002 and the category-page mirrors).
    //
    // Gate the price map behind a signal that defaults to `false` and flips
    // to `true` from an `Effect` — `Effect::new` runs only on the client
    // (same idiom as `WasmLoadingIndicator`), and only AFTER the initial
    // view is rendered. So the SSR render and the first CSR hydration render
    // both build rows with `prices_loaded == false`: price sorts fall back to
    // the stable id order, every price metric is `Pending` (a grid filter
    // keeps the row, a `grid:` sort waits), and shapes/positions match. A
    // frame later the effect fires, the memo re-runs with the real price
    // map, and the grid reactively reorders/filters — by which point
    // hydration is finished and tachys is no longer walking.
    let hydrated = RwSignal::new(false);
    Effect::new(move |_| {
        hydrated.set(true);
    });

    let price_map = Memo::new(move |_| {
        if hydrated.get() {
            listings_resource.get().and_then(|r| r.ok())
        } else {
            None
        }
    });

    // Which optional columns this item set can fill (#1296). Computed from
    // the whole, unfiltered set so filtering never changes the column layout
    // under the reader.
    let availability = Memo::new(move |_| {
        items.with(|items| {
            column_availability(items.iter().map(|(_, item)| *item), get_vendor_price)
        })
    });

    // `?cols=` — the visitor's own overrides. The grid applies the param to
    // every optional column itself (shared ones included); the page reads it
    // only for the toolbar picker's checkboxes.
    let (cols_param, set_cols_param) = query_signal::<String>("cols");
    let visible_cols = Memo::new(move |_| {
        parse_visible_cols(cols_param().as_deref(), OPTIONAL_COLUMNS, DEFAULT_COLUMNS)
    });
    let picker_visible = Memo::new(move |_| {
        let mut set = visible_cols.get();
        set.extend(shared_cols_in(cols_param().as_deref()));
        set
    });

    // `?sort=` can name a column this set does not have — a bookmark carried
    // from a gear category to a minion one. Fall back rather than ordering by
    // a value every row shares, which reads as "sorting did nothing".
    //
    // Availability, not `?cols=`, is what gates a sort: switching a column off
    // is about screen space, and a visitor who hides the vendor column has not
    // said they no longer want the cheapest vendor item first.
    let active_sort = Memo::new(move |_| {
        let fallback = if availability.get().item_level {
            ItemSortOption::fallback()
        } else {
            ItemSortOption::Name
        };
        match sort() {
            Some(requested)
                if requested.column().is_none_or(|column| match column {
                    COL_ID_WORLD => !is_single_world.get(),
                    column => availability.get().has(column),
                }) =>
            {
                requested
            }
            _ => fallback,
        }
    });
    let default_dir = Signal::derive(move || {
        if grid_sort.get().is_some() {
            SortDir::Desc
        } else {
            active_sort.get().default_dir()
        }
    });
    let active_dir = Signal::derive(move || direction().unwrap_or_else(|| default_dir.get()));
    // What the sortable headers read: the sort *in effect*, so a bookmark's
    // inapplicable `?sort=` paints the arrow on the fallback column that is
    // actually ordering the rows.
    let header_sort = Signal::derive(move || Some(active_sort.get()));

    let rows = Memo::new(move |_| {
        let price_map = price_map.get();
        let filters = ExplorerFilters {
            hq_only: hq_only().unwrap_or_default(),
        };
        let mut rows = items()
            .into_iter()
            .filter(|(_, item)| filters.matches(item))
            .map(|(id, item)| {
                ExplorerRow::build(id.0, item, price_map.as_ref(), get_vendor_price(id.0))
            })
            .collect::<Vec<_>>();
        sort_rows(&mut rows, active_sort.get(), active_dir.get(), world_name);
        rows
    });

    // Sale statistics on demand: nothing is requested until a visible shared
    // column, a `?cols=` entry, a `?gf=` filter (legacy aliases included) or
    // a `?sort=grid:` target needs a window. `MarketGrid` registers those
    // needs itself.
    let market = use_market_data_on_demand(scope_name);

    let filters = register_filters(
        explorer_filter_aliases(),
        Signal::derive(move || {
            vec![toggle_control(
                FILTER_HQ,
                t_string!(i18n, item_explorer_filter_hq_only).to_string(),
            )]
        }),
    );

    let column_label = move |id: &str| -> String {
        match id {
            COL_ITEM => t_string!(i18n, item_explorer_name).to_string(),
            COL_ID_ITEM_LEVEL => t_string!(i18n, item_explorer_ilvl).to_string(),
            COL_ID_EQUIP_LEVEL => t_string!(i18n, item_explorer_col_equip_level).to_string(),
            COL_NQ => t_string!(i18n, nq).to_string(),
            COL_ID_HQ => t_string!(i18n, hq).to_string(),
            COL_ID_VENDOR => t_string!(i18n, item_explorer_vendor).to_string(),
            COL_ID_WORLD => t_string!(i18n, item_explorer_col_world).to_string(),
            _ => String::new(),
        }
    };

    // The grid's column table. A column the set cannot fill is not merely
    // hidden — `?cols=` would put it back — it is absent, and the toolbar
    // picker explains why.
    let grid_columns = Signal::derive(move || {
        let sort = active_sort.get();
        let ascending = active_dir.get() == SortDir::Asc;
        let availability = availability.get();
        let single_world = is_single_world.get();
        let column = move |id: &'static str, width: f64, optional: bool| {
            let col = GridColumn::new(id, column_label(id), width, optional, true);
            match ItemSortOption::for_column(id) {
                Some(mode) => col.sorted(sort == mode, ascending),
                None => col,
            }
        };
        let mut columns = vec![column(COL_ITEM, 330.0, false)];
        if availability.has(COL_ID_ITEM_LEVEL) {
            columns.push(column(COL_ID_ITEM_LEVEL, 90.0, true));
        }
        if availability.has(COL_ID_EQUIP_LEVEL) {
            columns.push(column(COL_ID_EQUIP_LEVEL, 80.0, true));
        }
        columns.push(column(COL_NQ, 140.0, false));
        if availability.has(COL_ID_HQ) {
            columns.push(column(COL_ID_HQ, 140.0, true));
        }
        if availability.has(COL_ID_VENDOR) {
            columns.push(column(COL_ID_VENDOR, 120.0, true));
        }
        // The world column has no data of its own to be missing — it is the
        // multi-world scope that gives it a reason to exist.
        if !single_world {
            columns.push(column(COL_ID_WORLD, 140.0, true));
        }
        let mut actions = GridColumn::new(COL_ACTIONS, String::new(), 96.0, false, true);
        actions.auto_fit = false;
        columns.push(actions);
        columns
    });

    // Every native value the grid can filter or sort by. Price-backed metrics
    // are `Pending` until the gate flips (see `ExplorerRow`). The shared
    // `market-listing` id is overridden for the same reason: the legacy
    // `max-price` / `listed` keys alias onto it.
    let native_metrics: Vec<GridMetric<ExplorerRow>> = vec![
        GridMetric::text(COL_ITEM, |row: &ExplorerRow| {
            GridValue::Text(row.item.name.clone())
        }),
        GridMetric::number(COL_ITEM_LEVEL, |row: &ExplorerRow| {
            GridValue::Number(f64::from(row.item.level_item))
        }),
        GridMetric::number(COL_EQUIP_LEVEL, |row: &ExplorerRow| {
            GridValue::Number(f64::from(row.item.level_equip))
        }),
        GridMetric::number(COL_NQ, |row: &ExplorerRow| row.price_value(false)),
        GridMetric::number(COL_HQ, |row: &ExplorerRow| row.price_value(true)),
        GridMetric::number(COL_VENDOR, |row: &ExplorerRow| {
            row.vendor
                .map_or(GridValue::Missing, |v| GridValue::Number(f64::from(v)))
        }),
        GridMetric::text(COL_WORLD, move |row: &ExplorerRow| {
            if !row.prices_loaded {
                return GridValue::Pending;
            }
            row.cheapest
                .and_then(|c| world_name(c.world_id))
                .map_or(GridValue::Missing, GridValue::Text)
        }),
        GridMetric::number(COL_KEY, |row: &ExplorerRow| {
            GridValue::Number(f64::from(row.item_id))
        }),
        GridMetric::number(COL_LISTING, |row: &ExplorerRow| row.listing_value()),
    ];

    // ---- Control bar wiring -------------------------------------------
    //
    // Result count and sort control on row 1, one chip per active filter on
    // row 2, everything unset folded into `+ Filter`. The chips and the menu
    // come from the shared registry now (#1351): the legacy keys are aliases
    // of grid filters, `hq-only` a registered control.

    let sort_label = move |option: ItemSortOption| -> String {
        match option {
            ItemSortOption::ItemLevel => t_string!(i18n, item_explorer_ilvl).to_string(),
            ItemSortOption::EquipLevel => {
                t_string!(i18n, item_explorer_col_equip_level).to_string()
            }
            ItemSortOption::Price => t_string!(i18n, item_explorer_price).to_string(),
            ItemSortOption::HqPrice => t_string!(i18n, item_explorer_col_hq_price).to_string(),
            ItemSortOption::Vendor => t_string!(i18n, item_explorer_vendor).to_string(),
            ItemSortOption::World => t_string!(i18n, item_explorer_col_world).to_string(),
            ItemSortOption::Name => t_string!(i18n, item_explorer_name).to_string(),
            ItemSortOption::Key => t_string!(i18n, item_explorer_added).to_string(),
        }
    };

    // Only offer to sort by a column this set can fill. The sort menu is the
    // only control for "Added", which has no column, and the one place a
    // phone can reach a sort without a header menu.
    let sort_options = Memo::new(move |_| {
        SORT_OPTIONS
            .iter()
            .copied()
            .filter(|option| match option.column() {
                None => true,
                Some(COL_ID_WORLD) => !is_single_world.get(),
                Some(column) => availability.get().has(column),
            })
            .collect::<Vec<_>>()
    });

    // Not `use_location()`: that is an `expect`, and this component renders
    // inside a `<Suspense>` whose owner can be gone by the time the fragment
    // resolves (see `components::app_link`).
    #[cfg(feature = "hydrate")]
    let navigate = leptos_router::hooks::use_navigate();
    // Rewrite several query params in one navigation. The sort control uses
    // it to move `?sort=` and drop `?dir=` without pushing two entries.
    // `navigate` only exists client-side, and so does every path that reaches
    // this callback — it runs from a control's `on:change` / `on:click`.
    #[allow(unused_variables)]
    let set_query_params = Callback::new(move |params: Vec<(&'static str, Option<String>)>| {
        let mut query = location.query.get_untracked();
        for (key, value) in params {
            query.remove(key);
            if let Some(value) = value {
                query.insert(key, value);
            }
        }
        #[cfg(feature = "hydrate")]
        navigate(
            &format!(
                "{}{}{}",
                location.pathname.get_untracked(),
                query.to_query_string(),
                location.hash.get_untracked()
            ),
            leptos_router::NavigateOptions {
                replace: true,
                scroll: false,
                ..Default::default()
            },
        );
    });

    // A column the set cannot fill stays in the picker, greyed, with the
    // reason: ticking it back on would produce a column of blanks, and
    // dropping the entry entirely would leave the reader wondering where the
    // column they know went. The shared sale-history columns follow, grouped
    // by window, exactly as the Flip Finder lists them.
    let column_options = Memo::new(move |_| {
        let mut options = OPTIONAL_COLUMNS
            .iter()
            .copied()
            .map(|id| {
                let (disabled, hint) = if id == COL_ID_WORLD && is_single_world.get() {
                    (
                        true,
                        Some(t_string!(i18n, item_explorer_column_single_world).to_string()),
                    )
                } else if !availability.get().has(id) {
                    (
                        true,
                        Some(t_string!(i18n, item_explorer_column_unavailable).to_string()),
                    )
                } else {
                    (false, None)
                };
                ColumnOption {
                    id,
                    label: match id {
                        COL_ID_HQ => t_string!(i18n, item_explorer_col_hq_price).to_string(),
                        id => column_label(id),
                    },
                    group: None,
                    disabled,
                    hint,
                }
            })
            .collect::<Vec<_>>();
        options.extend(market_picker_options(market.window.selected.get()));
        options
    });

    let toggle_column = Callback::new(move |id: &'static str| {
        let next = toggled_cols(
            cols_param.get_untracked().as_deref(),
            &visible_cols.get_untracked(),
            id,
        );
        set_cols_param.set(Some(next));
    });
    let reset_columns = Callback::new(move |_| set_cols_param.set(None));

    let item_href = move |item_id: i32| format!("/item/{}/{item_id}", scope_name.get());

    view! {
        <Suspense fallback=move || view! { <div class="flex justify-center p-10"><Loading /></div> }>
        <div class="flex flex-col gap-6">
            // Sort, filters, the columns picker and the market window, in
            // the bar every analyzer tool uses. The sort control lives here
            // as well as on the column headers because "Added" has no
            // column of its own, and a phone has no hover to reach a
            // header menu with.
            <ControlBar sticky=false
                summary=move || {
                    view! {
                        <span class="text-sm font-semibold text-[color:var(--color-text)] whitespace-nowrap truncate">
                            {move || t!(i18n, item_explorer_results_count, n = move || filters.row_count())}
                        </span>
                    }
                    .into_any()
                }
                actions=move || {
                    view! {
                        <label class="flex items-center gap-1.5 min-w-0">
                            <span class="hidden xl:inline text-xs font-bold uppercase tracking-wider text-[color:var(--color-text-muted)] whitespace-nowrap">
                                {t!(i18n, item_explorer_sort_by)}
                            </span>
                            <select
                                class="input input-sm min-w-0"
                                aria-label=t_string!(i18n, item_explorer_sort_by).to_string()
                                prop:value=move || grid_sort.get().unwrap_or_else(|| active_sort.get().to_string())
                                on:change=move |ev| {
                                    // Drop `?dir=` with the column so the new
                                    // one arrives in its own default
                                    // direction — landing on Name descending
                                    // buries exactly what the click asked
                                    // for. Same rule `SortHeader` applies.
                                    set_query_params
                                        .run(vec![
                                            ("sort", Some(event_target_value(&ev))),
                                            ("dir", None),
                                        ]);
                                }
                            >
                                {move || grid_sort.get().map(|token| {
                                    let id = token.strip_prefix("grid:").unwrap_or_default();
                                    let label = column_options.with(|options| options.iter().find(|option| option.id == id).map(|option| option.label.clone()))
                                        .unwrap_or_else(|| column_label(id));
                                    view! { <option value=token selected=true>{label}</option> }
                                })}
                                {move || {
                                    sort_options
                                        .get()
                                        .into_iter()
                                        .map(|option| {
                                            let token = option.to_string();
                                            view! {
                                                <option
                                                    value=token.clone()
                                                    selected=move || grid_sort.get().is_none() && active_sort.get() == option
                                                >
                                                    {sort_label(option)}
                                                </option>
                                            }
                                        })
                                        .collect_view()
                                }}
                            </select>
                        </label>
                        <button
                            class="sticky-bar-button"
                            aria-label=move || {
                                if active_dir.get() == SortDir::Asc {
                                    t_string!(i18n, grid_query_ascending).to_string()
                                } else {
                                    t_string!(i18n, grid_query_descending).to_string()
                                }
                            }
                            on:click=move |_| {
                                // Omitted when it matches the column's own
                                // default, so the common case stays a clean
                                // `?sort=` — the same contract
                                // `SortHeader::sort_href` keeps.
                                let next = active_dir.get_untracked().flipped();
                                let value = (next != default_dir.get_untracked())
                                    .then(|| next.to_string());
                                set_query_params.run(vec![("dir", value)]);
                            }
                        >
                            {move || {
                                if active_dir.get() == SortDir::Asc {
                                    view! { <Icon icon=i::BiSortUpRegular width="20" height="20" /> }
                                } else {
                                    view! { <Icon icon=i::BiSortDownRegular width="20" height="20" /> }
                                }
                            }}
                        </button>
                        <MarketWindowControl window=market.window />
                    }
                    .into_any()
                }
                columns=column_options
                visible_columns=picker_visible
                on_toggle_column=toggle_column
                on_reset_columns=reset_columns
                empty_label=Signal::derive(move || {
                    t_string!(i18n, item_explorer_no_active_filters).to_string()
                })
            />

            // Results grid: one row per item so prices line up in a
            // scannable column. Every visible column renders at every
            // width; a narrow screen scrolls the grid sideways rather than
            // hiding columns by breakpoint.
            <MarketGrid
                id="item-explorer-grid"
                label=t_string!(i18n, item_explorer_title_main).to_string()
                show_saved_views=false
                row_height=40.0
                market
                each=rows
                columns=grid_columns
                metrics=native_metrics
                key=|row: &ExplorerRow| row.item_id
                subject=Arc::new(|row: &ExplorerRow| row.market_subject())
                header=move |id| {
                    let label = column_label(id);
                    match ItemSortOption::for_column(id) {
                        Some(mode) => view! {
                            <SortHeader mode label sort_mode=header_sort sort_dir=direction />
                        }
                        .into_any(),
                        None => label.into_any(),
                    }
                }
                view=move |row: ExplorerRow, id| {
                    let item_id = row.item_id;
                    let item = row.item;
                    let content = match id {
                        COL_ITEM => view! {
                            <ItemTooltip item_id=item_id class="shrink-0">
                                <AppLink href=move || item_href(item_id)>
                                    <ItemIcon item_id=item_id icon_size=IconSize::Small />
                                </AppLink>
                            </ItemTooltip>
                            <AppLink href=move || item_href(item_id)
                                attr:class="font-medium leading-snug text-[color:var(--color-text)] truncate \
                                           hover:text-brand-300 transition-colors \
                                           hover:underline decoration-brand-300/30 underline-offset-4"
                            >
                                {item.name.as_str()}
                            </AppLink>
                        }
                        .into_any(),
                        COL_ITEM_LEVEL => view! {
                            <span class="text-sm text-[color:var(--color-text-muted)] tabular-nums">{item.level_item}</span>
                        }
                        .into_any(),
                        COL_EQUIP_LEVEL => if has_equip_level(item) {
                            view! { <span class="text-sm text-[color:var(--color-text-muted)] tabular-nums">{item.level_equip}</span> }.into_any()
                        } else {
                            view! { <span class="text-sm text-[color:var(--color-text-muted)]">"—"</span> }.into_any()
                        },
                        // Prices come from the row, the same numbers the
                        // grid sorts and filters on. Both are `None` until
                        // the gate flips, so the server and the first client
                        // render both draw an empty cell; a loaded map with
                        // nothing listed draws a dash.
                        COL_NQ => price_cell(row.nq, row.prices_loaded),
                        COL_HQ => if item.can_be_hq {
                            price_cell(row.hq, row.prices_loaded)
                        } else {
                            ().into_any()
                        },
                        COL_VENDOR => row
                            .vendor
                            .map(|price| view! { <div class="text-sm"><Gil amount=price as i32 /></div> }.into_any())
                            .unwrap_or_else(|| ().into_any()),
                        // `cheapest` is `None` until the gate flips, so the
                        // server and the first client render both show
                        // nothing here (see `hydrated` above).
                        COL_WORLD => row
                            .cheapest
                            .map(|listing| view! {
                                <span class="truncate text-sm text-[color:var(--color-text-muted)]">
                                    <WorldName id=AnySelector::World(listing.world_id) />
                                </span>
                            }
                            .into_any())
                            .unwrap_or_else(|| ().into_any()),
                        COL_ACTIONS => view! {
                            <AddToList
                                item_id=item_id
                                class="flex items-center justify-center p-2 rounded hover:bg-white/10 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)] transition-colors"
                            />
                            <div class="p-1 rounded hover:bg-white/10 text-[color:var(--color-text-muted)] cursor-pointer" title=t_string!(i18n, item_explorer_copy_name).to_string()>
                                <Clipboard clipboard_text=item.name.clone() />
                            </div>
                        }
                        .into_any(),
                        _ => ().into_any(),
                    };
                    let class = if matches!(id, COL_ITEM | COL_ACTIONS) {
                        "px-3 flex h-full items-center gap-2 min-w-0 w-full"
                    } else {
                        "px-3 flex h-full items-center justify-end min-w-0 w-full"
                    };
                    view! { <div class=class>{content}</div> }.into_any()
                }
                measure=move |row: &ExplorerRow, id| {
                    let gil = |price: Option<i32>| {
                        (price.map(|p| p.separate_with_commas()).unwrap_or_default(), 42.0)
                    };
                    match id {
                        COL_ITEM => (row.item.name.clone(), 60.0),
                        COL_ITEM_LEVEL => (row.item.level_item.to_string(), 24.0),
                        COL_EQUIP_LEVEL => (row.item.level_equip.to_string(), 24.0),
                        COL_NQ => gil(row.nq),
                        COL_HQ => gil(row.hq),
                        COL_VENDOR => gil(row.vendor.map(|v| v as i32)),
                        COL_WORLD => (
                            row.cheapest.and_then(|c| world_name(c.world_id)).unwrap_or_default(),
                            24.0,
                        ),
                        _ => (String::new(), 96.0),
                    }
                }
            />
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
        <div class="flex flex-col min-h-screen">
            <div class="main-content p-2 sm:p-6 w-full">
                <crate::routes::item_explorer_toolbar::ItemExplorerToolbar />
                <Outlet />
            </div>
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::{
        COL_ACTIONS, COL_ITEM, DEFAULT_COLUMNS, ItemSortOption, OPTIONAL_COLUMNS, SORT_OPTIONS,
        canonical_job_acronym, collect_job_items_sorted, resolve_category_param,
        resolve_jobset_param, sort_rows, toggled_cols,
    };
    use crate::components::sort_header::{SortColumn, SortDir};
    use crate::routes::item_explorer_filters::{CheapestListing, ExplorerRow};
    use crate::routes::item_explorer_toolbar::{job_chip_slug, job_chips_sorted_in};
    use std::collections::HashSet;
    use std::str::FromStr;
    use xiv_gen::Language;

    /// `?sort=` is the only channel between a bookmark and the sort in effect,
    /// so every variant has to survive the round trip. A variant added to the
    /// enum but not to `FromStr` would silently fall back to item level.
    #[test]
    fn every_sort_option_round_trips_through_the_url() {
        for option in SORT_OPTIONS {
            let token = option.to_string();
            assert_eq!(
                ItemSortOption::from_str(&token),
                Ok(option),
                "?sort={token} does not parse back to the option that wrote it",
            );
        }
    }

    /// The sort menu offers every column the table can order by. Adding a
    /// variant without listing it here would leave "Added" — which has no
    /// column header — unreachable.
    #[test]
    fn the_sort_menu_offers_every_sort_option() {
        for option in [
            ItemSortOption::ItemLevel,
            ItemSortOption::EquipLevel,
            ItemSortOption::Price,
            ItemSortOption::HqPrice,
            ItemSortOption::Vendor,
            ItemSortOption::World,
            ItemSortOption::Name,
            ItemSortOption::Key,
        ] {
            assert!(SORT_OPTIONS.contains(&option), "{option} is not offered");
        }
    }

    /// A sort keyed on an optional column has to name a *real* column id, or
    /// the availability check silently reads `true` for it (the `_ => true`
    /// arm of `ColumnAvailability::has`) and the explorer offers a sort that
    /// orders by nothing. And every header-backed sort maps back to the
    /// column it sits on, so the grid's header arrow and the sort menu agree.
    #[test]
    fn sorts_reference_real_columns_and_headers_round_trip() {
        for option in SORT_OPTIONS {
            if let Some(column) = option.column() {
                assert!(
                    OPTIONAL_COLUMNS.contains(&column),
                    "{option} sorts on unknown column {column:?}",
                );
                assert_eq!(ItemSortOption::for_column(column), Some(option));
            }
        }
        assert_eq!(
            ItemSortOption::for_column(COL_ITEM),
            Some(ItemSortOption::Name)
        );
        assert_eq!(ItemSortOption::for_column(COL_ACTIONS), None);
        assert_eq!(ItemSortOption::for_column("market-sale-median"), None);
    }

    /// The columns picker starts from the full native set; availability, not
    /// the default, is what takes a column away. No shared market column is
    /// on by default, so a bare category page requests no statistics.
    #[test]
    fn every_optional_native_column_is_on_by_default_and_no_shared_one_is() {
        assert_eq!(DEFAULT_COLUMNS, OPTIONAL_COLUMNS);
        assert!(!DEFAULT_COLUMNS.iter().any(|c| c.starts_with("market-")));
    }

    /// `?cols=` carries native and shared ids side by side. Flipping one
    /// kind must never drop the other, and the first shared toggle on a bare
    /// URL has to spell out the native defaults or the grid hides them.
    #[test]
    fn toggling_columns_preserves_the_other_kind() {
        let all: HashSet<&'static str> = OPTIONAL_COLUMNS.iter().copied().collect();
        assert_eq!(
            toggled_cols(None, &all, "market-sale-median"),
            "ilvl,lv,hq,vendor,world,market-sale-median"
        );
        let with_shared = "ilvl,lv,hq,vendor,world,market-sale-median";
        assert_eq!(
            toggled_cols(Some(with_shared), &all, "vendor"),
            "ilvl,lv,hq,world,market-sale-median"
        );
        let mut without_vendor = all.clone();
        without_vendor.remove("vendor");
        assert_eq!(
            toggled_cols(
                Some("ilvl,lv,hq,world,market-sale-median"),
                &without_vendor,
                "vendor"
            ),
            with_shared
        );
        assert_eq!(
            toggled_cols(Some(with_shared), &all, "market-sale-median"),
            "ilvl,lv,hq,vendor,world"
        );
        assert_eq!(toggled_cols(Some(""), &HashSet::new(), "hq"), "hq");
    }

    fn row(item: &'static xiv_gen::Item, nq: Option<i32>, world_id: i32) -> ExplorerRow {
        ExplorerRow {
            item_id: item.key_id.0,
            item,
            nq,
            hq: None,
            cheapest: nq.map(|price| CheapestListing {
                price,
                hq: false,
                world_id,
            }),
            vendor: None,
            prices_loaded: true,
        }
    }

    /// Rows without a value sort last in both directions, and a price sort
    /// before prices load (every key `None`) is a no-op that keeps the id
    /// order the server rendered.
    #[test]
    fn native_sorts_keep_missing_values_last_and_are_stable() {
        let en = xiv_gen_db::data_for(Language::En);
        let mut items: Vec<_> = en
            .items
            .values()
            .filter(|item| item.item_search_category == 10)
            .collect();
        items.sort_by_key(|item| item.key_id.0);
        let (a, b, c) = (items[0], items[1], items[2]);
        let mut rows = vec![row(a, None, 0), row(b, Some(200), 7), row(c, Some(100), 9)];
        sort_rows(&mut rows, ItemSortOption::Price, SortDir::Desc, |_| None);
        assert_eq!(
            rows.iter().map(|r| r.item_id).collect::<Vec<_>>(),
            [b.key_id.0, c.key_id.0, a.key_id.0]
        );
        sort_rows(&mut rows, ItemSortOption::Price, SortDir::Asc, |_| None);
        assert_eq!(
            rows.iter().map(|r| r.item_id).collect::<Vec<_>>(),
            [c.key_id.0, b.key_id.0, a.key_id.0]
        );
        let names = |id: i32| Some(if id == 7 { "Zalera" } else { "Adamantoise" }.to_string());
        sort_rows(&mut rows, ItemSortOption::World, SortDir::Asc, names);
        assert_eq!(
            rows.iter().map(|r| r.item_id).collect::<Vec<_>>(),
            [c.key_id.0, b.key_id.0, a.key_id.0]
        );

        let mut unloaded: Vec<_> = [a, b, c]
            .into_iter()
            .map(|item| ExplorerRow::build(item.key_id.0, item, None, None))
            .collect();
        sort_rows(&mut unloaded, ItemSortOption::Price, SortDir::Desc, |_| {
            None
        });
        assert_eq!(
            unloaded.iter().map(|r| r.item_id).collect::<Vec<_>>(),
            [a.key_id.0, b.key_id.0, c.key_id.0]
        );
    }

    /// Text columns read A-first; numbers read best-first, which is largest.
    /// Getting this backwards buries exactly the rows a fresh click asked for.
    #[test]
    fn text_columns_default_to_ascending_and_numbers_to_descending() {
        assert_eq!(ItemSortOption::Name.default_dir(), SortDir::Asc);
        assert_eq!(ItemSortOption::World.default_dir(), SortDir::Asc);
        for option in [
            ItemSortOption::ItemLevel,
            ItemSortOption::EquipLevel,
            ItemSortOption::Price,
            ItemSortOption::HqPrice,
            ItemSortOption::Vendor,
            ItemSortOption::Key,
        ] {
            assert_eq!(option.default_dir(), SortDir::Desc, "{option}");
        }
    }

    const ALL_LOCALES: [Language; 7] = [
        Language::En,
        Language::Ja,
        Language::De,
        Language::Fr,
        Language::Cn,
        Language::Ko,
        Language::Tc,
    ];

    /// Pins the hardcoded acronym table against the shipped English pack. A
    /// game-data bump that renumbers, adds, or removes a job fails here
    /// instead of silently emptying `/items/jobset/*`.
    #[test]
    fn canonical_acronym_matches_english_abbreviation() {
        let en = xiv_gen_db::data_for(Language::En);
        for job in job_chips_sorted_in(en) {
            assert_eq!(
                canonical_job_acronym(job.key_id),
                Some(job.abbreviation.as_str()),
                "job id {} ({}) is missing or wrong in the acronym table",
                job.key_id.0,
                job.name,
            );
        }
    }

    /// The bug. `ClassJobCategory`'s columns are English acronyms baked into
    /// the sheet schema, but `ClassJob::abbreviation` is a localized display
    /// string — so a German client's own "FST" chip is a key that
    /// `job_category_lookup` matches nothing against, on *either* side of the
    /// SSR/CSR split. The result was a nav link to a permanently empty page.
    #[test]
    fn a_localized_job_abbreviation_is_not_a_usable_category_key() {
        let en = xiv_gen_db::data_for(Language::En);
        let de = xiv_gen_db::data_for(Language::De);

        // Find a job German actually renames, so the assertion is not vacuous.
        let (localized, canonical) = job_chips_sorted_in(de)
            .into_iter()
            .find_map(|job| {
                let canonical = canonical_job_acronym(job.key_id)?;
                (job.abbreviation != canonical)
                    .then(|| (job.abbreviation.clone(), canonical.to_string()))
            })
            .expect("German renames at least one job abbreviation");

        assert!(
            collect_job_items_sorted(de, &localized, false).is_empty(),
            "the localized abbreviation {localized:?} must not resolve — it is \
             the dead-link bug, and if it ever does the acronym table is moot",
        );
        assert!(
            !collect_job_items_sorted(de, &canonical, false).is_empty(),
            "the canonical acronym {canonical:?} resolves against German data",
        );
        // Same key, same items, either side of the SSR/CSR locale split.
        assert_eq!(
            collect_job_items_sorted(de, &canonical, false)
                .iter()
                .map(|(id, _)| id.0)
                .collect::<Vec<_>>(),
            collect_job_items_sorted(en, &canonical, false)
                .iter()
                .map(|(id, _)| id.0)
                .collect::<Vec<_>>(),
        );
    }

    /// Jobs that no `ClassJobCategory` row can select, because the sheet has
    /// no column for them. These render an empty `/items/jobset/*` page in
    /// *every* locale, English included — a gap in the shipped game-data pack
    /// (`ClassJobCategory` predates Beastmaster), not something the acronym
    /// table can repair. Pinned so a data bump that widens or closes the gap
    /// surfaces in review instead of quietly changing which pages are dead.
    #[test]
    fn only_beastmaster_lacks_a_category_column() {
        let en = xiv_gen_db::data_for(Language::En);
        let unselectable: Vec<&str> = job_chips_sorted_in(en)
            .into_iter()
            .filter_map(|job| canonical_job_acronym(job.key_id))
            .filter(|acronym| {
                !en.class_job_categorys
                    .values()
                    .any(|c| super::job_category_lookup(c, acronym))
            })
            .collect();
        assert_eq!(unselectable, vec!["BST"]);
    }

    /// The fix, end to end: the href the job nav actually mints resolves to a
    /// non-empty item list in every locale. Pre-fix this minted the localized
    /// abbreviation and failed for German and French.
    #[test]
    fn every_job_chip_link_resolves_to_items_in_every_locale() {
        let en = xiv_gen_db::data_for(Language::En);
        for lang in ALL_LOCALES {
            let data = xiv_gen_db::data_for(lang);
            for job in job_chips_sorted_in(data) {
                // Dead for everyone regardless of locale keying — covered by
                // `only_beastmaster_lacks_a_category_column`. Keyed on the id
                // so the exclusion is itself locale-independent.
                if canonical_job_acronym(job.key_id) == Some("BST") {
                    continue;
                }
                let slug = job_chip_slug(job);
                let resolved = resolve_jobset_param(data, &slug).unwrap_or_else(|| {
                    panic!("{lang:?}: chip link /items/jobset/{slug} resolves to no job")
                });
                // The URL is a key, so every locale must read it as the same
                // job. Item *sets* are deliberately not compared across
                // locales: the CN/KO/TC packs track an older game version and
                // genuinely hold different items — a separate and far broader
                // SSR-locale issue that a route key cannot address.
                assert_eq!(
                    Some(resolved.as_str()),
                    canonical_job_acronym(job.key_id),
                    "{lang:?}: /items/jobset/{slug} resolves to the wrong job",
                );
                assert!(
                    !collect_job_items_sorted(data, &resolved, false).is_empty(),
                    "{lang:?}: /items/jobset/{slug} ({}) renders zero items",
                    job.name,
                );
            }
        }
        // English is the SSR locale, so it must resolve every slug a client of
        // any locale can mint — that is the pairing that actually hydrates.
        for lang in ALL_LOCALES {
            for job in job_chips_sorted_in(xiv_gen_db::data_for(lang)) {
                let slug = job_chip_slug(job);
                assert!(
                    resolve_jobset_param(en, &slug).is_some(),
                    "English SSR cannot resolve {slug}, minted by a {lang:?} client",
                );
            }
        }
    }

    /// Lowest-id category that the toolbar actually links to (`category`
    /// 1..=4), so the locale tests below key off a real navigable page
    /// rather than a hardcoded id that a game-data bump could retire.
    fn first_linkable_category(data: &xiv_gen::Data) -> &xiv_gen::ItemSearchCategory {
        data.item_search_categorys
            .values()
            .filter(|cat| (1..=4).contains(&cat.category))
            .min_by_key(|cat| cat.key_id.0)
            .expect("game data must have at least one linkable item search category")
    }

    /// The bug this route key was changed to avoid. The server renders SSR
    /// with English game data while the client hydrates with the visitor's
    /// locale, so a *name*-keyed param resolves on exactly one of the two
    /// sides — SSR emits an empty list where the client builds a full one,
    /// and tachys panics walking the mismatch.
    #[test]
    fn a_localized_category_name_only_resolves_in_its_own_locale() {
        let en = xiv_gen_db::data_for(Language::En);
        let ja = xiv_gen_db::data_for(Language::Ja);
        let sample = first_linkable_category(en);
        let ja_name = ja
            .item_search_categorys
            .get(&sample.key_id)
            .expect("the same id exists in every locale")
            .name
            .clone();
        assert_ne!(
            ja_name, sample.name,
            "sanity: this category's display name must actually differ between locales",
        );

        // A chip minted by a Japanese client carries the Japanese name...
        assert!(
            resolve_category_param(ja, &ja_name).is_some(),
            "the minting locale resolves its own name",
        );
        // ...but SSR answers that URL with English data and finds nothing.
        assert!(
            resolve_category_param(en, &ja_name).is_none(),
            "English SSR cannot resolve a Japanese category name — the mismatch",
        );
    }

    /// The fix: an id resolves to the same category in every locale, so SSR
    /// and the client agree on the item list regardless of who minted the
    /// link.
    #[test]
    fn a_category_id_resolves_identically_in_every_locale() {
        let en = xiv_gen_db::data_for(Language::En);
        let expected = first_linkable_category(en).key_id;
        let param = expected.0.to_string();
        for lang in [
            Language::En,
            Language::Ja,
            Language::De,
            Language::Fr,
            Language::Cn,
            Language::Ko,
            Language::Tc,
        ] {
            let data = xiv_gen_db::data_for(lang);
            assert_eq!(
                resolve_category_param(data, &param).map(|cat| cat.key_id),
                Some(expected),
                "id param must resolve to the same category under {lang:?}",
            );
        }
    }

    /// Links minted before the switch to ids are percent-encoded localized
    /// names; they must keep working for the locale that minted them.
    #[test]
    fn legacy_percent_encoded_name_params_still_resolve() {
        let en = xiv_gen_db::data_for(Language::En);
        let sample = en
            .item_search_categorys
            .values()
            .filter(|cat| (1..=4).contains(&cat.category) && cat.name.contains(' '))
            .min_by_key(|cat| cat.key_id.0)
            .expect("at least one linkable category name contains a space");
        let encoded = sample.name.replace(' ', "%20").replace('\'', "%27");
        assert_eq!(
            resolve_category_param(en, &encoded).map(|cat| cat.key_id),
            Some(sample.key_id),
        );
    }

    /// `Primary Tools` is the name of both id 2 and id 3. The name fallback
    /// must not pick between them by `HashMap` iteration order, or it
    /// reintroduces the very SSR/CSR divergence the id key removes.
    #[test]
    fn duplicate_category_names_resolve_to_the_lowest_id() {
        let en = xiv_gen_db::data_for(Language::En);
        let mut by_name: std::collections::HashMap<&str, Vec<i32>> =
            std::collections::HashMap::new();
        for cat in en.item_search_categorys.values() {
            by_name
                .entry(cat.name.as_str())
                .or_default()
                .push(cat.key_id.0);
        }
        let Some((name, ids)) = by_name
            .iter()
            .filter(|(name, ids)| ids.len() > 1 && !name.is_empty())
            .min_by_key(|(name, _)| *name)
        else {
            // No duplicate names in this data revision — nothing to pin.
            return;
        };
        let lowest = ids.iter().copied().min().expect("non-empty");
        assert_eq!(
            resolve_category_param(en, name).map(|cat| cat.key_id.0),
            Some(lowest),
            "duplicate name {name:?} must resolve deterministically to the lowest id",
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
