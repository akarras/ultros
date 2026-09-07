use crate::components::app_link::{AppLink, use_location_or_default};
use std::fmt::Display;
use std::{collections::HashSet, str::FromStr};

use crate::CheapestPrices;
use crate::components::clipboard::Clipboard;
use crate::components::control_bar::{
    ColumnOption, ControlBar, FilterOption, parse_visible_cols, serialize_visible_cols,
};
use crate::components::data_table::{Column, ColumnHeader, DataTableGrid, TrackWidths};
use crate::components::filter_chip::{FilterChip, committed_value};
use crate::components::gil::Gil;
use crate::components::icon::Icon;
use crate::components::item_tooltip::ItemTooltip;
use crate::components::job_set_card::JobSetCard;
use crate::components::job_set_grouping::{GroupableItem, group_into_sets};
use crate::components::loading::Loading;
use crate::components::query_button::QueryButton;
use crate::components::related_items::get_vendor_price;
use crate::components::sort_header::{SortColumn, SortDir, SortableHeaderCell, cmp_none_last};
use crate::components::toggle::Toggle;
use crate::components::world_name::WorldName;
use crate::components::{add_to_list::*, cheapest_price::*, item_icon::*, meta::*};
use crate::global_state::local_world_data::use_world_helper;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use crate::query_defaults::query_signal;
use crate::routes::item_explorer_filters::{
    CheapestPrice, ExplorerFilters, column_availability, has_equip_level,
};
use crate::routes::item_explorer_scope::{ExplorerPriceScope, use_explorer_price_scope};
use crate::routes::item_explorer_toolbar::jobset_display_label;
use icondata as i;
use itertools::Itertools;
use leptos::prelude::*;
use leptos::reactive::wrappers::write::SignalSetter;
use leptos_router::components::Outlet;
use leptos_router::hooks::use_params_map;
use paginate::Pages;
use percent_encoding::percent_decode_str;
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
/// column id the columns picker persists in `?cols=`, so the two never need a
/// translation table between them.
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
            ItemSortOption::ItemLevel => COL_ID_ITEM_LEVEL,
            ItemSortOption::EquipLevel => COL_ID_EQUIP_LEVEL,
            ItemSortOption::HqPrice => COL_ID_HQ,
            ItemSortOption::Vendor => COL_ID_VENDOR,
            ItemSortOption::World => COL_ID_WORLD,
            // Name, NQ price and "added" have columns that are always on.
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

/// Re-sorting has to return to page 1: page 7 of a different ordering is a
/// different set of items, and an out-of-range page is what
/// `?page=35&sort=ilvl` deep-links used to hydrate-crash on.
const RESET_ON_SORT: &[&str] = &["page"];

/// One row of the explorer's table.
type ExplorerRow = (&'static ItemId, &'static Item);

/// Grid tracks and header-cell classes for the explorer's nine columns, in
/// DOM order.
///
/// Kept as plain data, and read by both the column list below and
/// `header_classes_match_their_tracks`, so the two can never drift: a header
/// cell that is visible at a breakpoint where its column owns no track pushes
/// every header to its right one track over and wraps the last one onto an
/// implicit second row, with the body rows still correct. The `xl`-only
/// columns are exactly where that is easy to get wrong.
type ExplorerColumn = (TrackWidths, &'static str);

const COL_ICON: ExplorerColumn = (TrackWidths::everywhere("2.5rem"), "");
const COL_NAME: ExplorerColumn = (
    TrackWidths::responsive(
        Some("minmax(0,1fr)"),
        Some("minmax(6rem,1fr)"),
        Some("minmax(6rem,1fr)"),
    ),
    "",
);
const COL_ITEM_LEVEL: ExplorerColumn = (TrackWidths::from_lg("3.5rem"), "");
const COL_EQUIP_LEVEL: ExplorerColumn = (TrackWidths::from_lg("3rem"), "");
const COL_NQ: ExplorerColumn = (
    TrackWidths::responsive(Some("auto"), Some("6.5rem"), Some("6.5rem")),
    "",
);
const COL_HQ: ExplorerColumn = (TrackWidths::from_lg("6.5rem"), "");
const COL_VENDOR: ExplorerColumn = (TrackWidths::from_xl("6rem"), "hidden xl:block");
const COL_WORLD: ExplorerColumn = (TrackWidths::from_xl("6.5rem"), "hidden xl:block");
const COL_ACTIONS: ExplorerColumn = (
    TrackWidths::responsive(Some("auto"), Some("5rem"), Some("5rem")),
    "",
);

#[cfg(test)]
const EXPLORER_COLUMNS: [ExplorerColumn; 9] = [
    COL_ICON,
    COL_NAME,
    COL_ITEM_LEVEL,
    COL_EQUIP_LEVEL,
    COL_NQ,
    COL_HQ,
    COL_VENDOR,
    COL_WORLD,
    COL_ACTIONS,
];

/// Ids of the columns the visitor can switch off, as persisted in `?cols=`.
///
/// Deliberately the same tokens `ItemSortOption` writes to `?sort=`: the two
/// name the same column, and a second vocabulary for it would be one more
/// mapping to keep in step.
pub(crate) const COL_ID_ITEM_LEVEL: &str = "ilvl";
pub(crate) const COL_ID_EQUIP_LEVEL: &str = "lv";
pub(crate) const COL_ID_HQ: &str = "hq";
pub(crate) const COL_ID_VENDOR: &str = "vendor";
pub(crate) const COL_ID_WORLD: &str = "world";

/// The switchable columns, in DOM order. Icon, name, NQ price and the row
/// actions are not here: they carry the row's identity and its one
/// always-meaningful number, so there is nothing to gain from hiding them.
const OPTIONAL_COLUMNS: &[&str] = &[
    COL_ID_ITEM_LEVEL,
    COL_ID_EQUIP_LEVEL,
    COL_ID_HQ,
    COL_ID_VENDOR,
    COL_ID_WORLD,
];

/// Every optional column is on by default; the ones the current item set
/// cannot fill are then dropped by [`ColumnAvailability`], so a category picks
/// its own columns without the visitor touching the picker.
const DEFAULT_COLUMNS: &[&str] = OPTIONAL_COLUMNS;

/// `?` keys the filter chips own. Listed once so `+ Filter` and `Clear all`
/// cannot drift from what the chips actually render.
pub(crate) const FILTER_NAME: &str = "q";
pub(crate) const FILTER_MIN_ILVL: &str = "min-ilvl";
pub(crate) const FILTER_MAX_ILVL: &str = "max-ilvl";
pub(crate) const FILTER_MIN_LV: &str = "min-lv";
pub(crate) const FILTER_MAX_PRICE: &str = "max-price";
pub(crate) const FILTER_VENDOR: &str = "vendor-only";
pub(crate) const FILTER_HQ: &str = "hq-only";
pub(crate) const FILTER_LISTED: &str = "listed";

/// Filter order in the `+ Filter` menu.
pub(crate) const ADDABLE_FILTERS: &[&str] = &[
    FILTER_NAME,
    FILTER_MIN_ILVL,
    FILTER_MAX_ILVL,
    FILTER_MIN_LV,
    FILTER_MAX_PRICE,
    FILTER_VENDOR,
    FILTER_HQ,
    FILTER_LISTED,
];

/// Filters that only mean something once a column has values behind it. A
/// minion category offers neither an equip-level floor nor an HQ-only switch.
fn filter_requires_column(filter: &str) -> Option<&'static str> {
    Some(match filter {
        FILTER_MIN_ILVL | FILTER_MAX_ILVL => COL_ID_ITEM_LEVEL,
        FILTER_MIN_LV => COL_ID_EQUIP_LEVEL,
        FILTER_HQ => COL_ID_HQ,
        FILTER_VENDOR => COL_ID_VENDOR,
        _ => return None,
    })
}

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

/// The explorer's filter state: one value to read, one setter to write.
#[derive(Copy, Clone)]
struct ExplorerFilterSignals {
    /// Everything the chips currently filter by, as one parsed value.
    values: Memo<ExplorerFilters>,
    /// Set (`Some`) or clear (`None`) one filter param.
    set_param: Callback<(&'static str, Option<String>)>,
    /// Rewrite several query params in one navigation. The sort control uses
    /// it to move `?sort=` and drop `?dir=` without pushing two entries.
    set_params: Callback<Vec<(&'static str, Option<String>)>>,
    /// Filter ids drawn as a chip right now, in [`ADDABLE_FILTERS`] order.
    active: Memo<Vec<&'static str>>,
    /// A filter just added from `+ Filter` and not yet given a value. It has
    /// no URL presence, so without this a freshly added chip would render and
    /// then vanish on the next reactive pass.
    pending: RwSignal<Option<&'static str>>,
    /// Drop every filter in one navigation.
    clear_all: Callback<()>,
}

/// Wire the filter chips to the URL.
///
/// Writes go through [`use_navigate`](leptos_router::hooks::use_navigate)
/// rather than `query_signal`'s setter for two reasons: a filter change has to
/// drop `?page=` in the *same* navigation (page 7 of one cut is not page 7 of
/// another — the same rule [`RESET_ON_SORT`] applies to sorting), and
/// `Clear all` has to drop eight params without pushing eight history entries.
fn use_explorer_filters() -> ExplorerFilterSignals {
    let (name, _) = query_signal::<String>(FILTER_NAME);
    let (min_ilvl, _) = query_signal::<i32>(FILTER_MIN_ILVL);
    let (max_ilvl, _) = query_signal::<i32>(FILTER_MAX_ILVL);
    let (min_lv, _) = query_signal::<i32>(FILTER_MIN_LV);
    let (max_price, _) = query_signal::<i32>(FILTER_MAX_PRICE);
    let (vendor_only, _) = query_signal::<bool>(FILTER_VENDOR);
    let (hq_only, _) = query_signal::<bool>(FILTER_HQ);
    let (listed_only, _) = query_signal::<bool>(FILTER_LISTED);

    let values = Memo::new(move |_| ExplorerFilters {
        name: name().and_then(|raw| committed_value(&raw)),
        min_ilvl: min_ilvl(),
        max_ilvl: max_ilvl(),
        min_lv: min_lv(),
        max_price: max_price(),
        vendor_only: vendor_only().unwrap_or_default(),
        hq_only: hq_only().unwrap_or_default(),
        listed_only: listed_only().unwrap_or_default(),
    });

    let pending = RwSignal::new(None::<&'static str>);
    let active = Memo::new(move |_| {
        let values = values.get();
        let pending = pending.get();
        ADDABLE_FILTERS
            .iter()
            .copied()
            .filter(|id| values.is_set(id) || pending == Some(*id))
            .collect::<Vec<_>>()
    });

    // Not `use_location()`: that is an `expect`, and this component renders
    // inside a `<Suspense>` whose owner can be gone by the time the fragment
    // resolves (see `components::app_link`).
    let location = use_location_or_default();
    #[cfg(feature = "hydrate")]
    let navigate = leptos_router::hooks::use_navigate();
    // `navigate` only exists client-side, and so does every path that reaches
    // these callbacks — they run from a chip's `on:change` / `on:click`.
    // A `Callback` rather than a plain closure: it is `Copy`, and the
    // `navigate` it captures under `hydrate` is not.
    #[allow(unused_variables)]
    let go = Callback::new(move |query: leptos_router::params::ParamsMap| {
        #[cfg(feature = "hydrate")]
        navigate(
            &format!(
                "{}{}",
                location.pathname.get_untracked(),
                query.to_query_string()
            ),
            leptos_router::NavigateOptions {
                replace: true,
                scroll: false,
                ..Default::default()
            },
        );
    });

    // Every write drops `?page=`: page 7 of one cut of the list is not page 7
    // of another, and an out-of-range page is what the `?page=N` deep-links in
    // GlitchTip used to hydrate-crash on. Same rule as [`RESET_ON_SORT`].
    let set_params = Callback::new(move |params: Vec<(&'static str, Option<String>)>| {
        let mut query = location.query.get_untracked();
        query.remove("page");
        for (key, value) in params {
            query.remove(key);
            if let Some(value) = value {
                query.insert(key, value);
            }
            if pending.get_untracked() == Some(key) {
                pending.set(None);
            }
        }
        go.run(query);
    });
    let set_param = Callback::new(move |(key, value): (&'static str, Option<String>)| {
        set_params.run(vec![(key, value)]);
    });

    let clear_all = Callback::new(move |_| {
        let mut query = location.query.get_untracked();
        for key in ADDABLE_FILTERS {
            query.remove(key);
        }
        query.remove("page");
        pending.set(None);
        go.run(query);
    });

    ExplorerFilterSignals {
        values,
        set_param,
        set_params,
        active,
        pending,
        clear_all,
    }
}

#[component]
fn ItemList(items: Memo<Vec<ExplorerRow>>) -> impl IntoView {
    let i18n = use_i18n();
    let (page, _set_page) = query_signal::<i32>("page");
    let (direction, _set_direction) = query_signal::<SortDir>("dir");
    let (sort, _set_sort) = query_signal::<ItemSortOption>("sort");

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
    //
    // The price *filters* (`?max-price=`, `?listed=`) ride the same gate,
    // through `CheapestPrice::NotLoaded` — see `item_explorer_filters`.
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
    // the whole, unfiltered set so paging and filtering never change the
    // column layout under the reader.
    let availability = Memo::new(move |_| {
        items.with(|items| {
            column_availability(items.iter().map(|(_, item)| *item), get_vendor_price)
        })
    });

    // `?cols=` — the visitor's own overrides on top of that. A column shows
    // when the set has data for it AND the visitor has not switched it off.
    let (cols_param, set_cols_param) = query_signal::<String>("cols");
    let visible_cols = Memo::new(move |_| {
        parse_visible_cols(cols_param().as_deref(), OPTIONAL_COLUMNS, DEFAULT_COLUMNS)
    });
    let column_on = move |id: &'static str| {
        availability.get().has(id) && visible_cols.with(|cols| cols.contains(id))
    };
    // The world column has no data of its own to be missing — it is the
    // multi-world scope that gives it a reason to exist.
    let world_column_on = Signal::derive(move || !is_single_world.get() && column_on(COL_ID_WORLD));

    let ExplorerFilterSignals {
        values: filter_values,
        set_param: set_filter_param,
        set_params: set_query_params,
        active: filter_active,
        pending: filter_pending,
        clear_all: clear_filters,
    } = use_explorer_filters();

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

    let sorted_items = Memo::new(move |_| {
        let item_property = active_sort.get();
        let direction = direction().unwrap_or_else(|| item_property.default_dir());
        let price_map = price_map.get();
        let filters = filter_values.get();
        // One lookup closure per key the sort or the filters need, so a row is
        // priced once per comparison rather than once per branch.
        // Borrowed, not moved: all three read the same map, and cloning a
        // whole listings map per closure would be per-render, per-column.
        let cheapest = |item_id: i32| match &price_map {
            None => CheapestPrice::NotLoaded,
            Some(map) => match map.find_matching_listings(item_id).lowest_gil() {
                Some(price) => CheapestPrice::Some(price),
                None => CheapestPrice::Missing,
            },
        };
        let quality_price = |item_id: i32, hq: bool| {
            price_map.as_ref().and_then(|map| {
                let summary = map.find_matching_listings(item_id);
                if hq { summary.hq } else { summary.lq }.map(|listing| listing.price)
            })
        };
        let world_of = |item_id: i32| {
            let listing = price_map
                .as_ref()?
                .find_matching_listings(item_id)
                .chosen(false)?;
            worlds.with_value(|worlds| {
                worlds.as_ref().and_then(|worlds| {
                    worlds
                        .lookup_selector(AnySelector::World(listing.world_id))
                        .map(|world| world.get_name().to_string())
                })
            })
        };
        items()
            .into_iter()
            .filter(|(id, item)| filters.matches(item, get_vendor_price(id.0), cheapest(id.0)))
            .sorted_by(|(_, item_a), (_, item_b)| {
                let (a, b) = (item_a.key_id.0, item_b.key_id.0);
                // `cmp_none_last` for everything that can be absent: a row
                // with no listing, no vendor and no equip level belongs at the
                // bottom in *both* directions, not dragged to the top the
                // moment the reader asks for best-first.
                match item_property {
                    ItemSortOption::ItemLevel => {
                        ordered(direction, item_a.level_item.cmp(&item_b.level_item))
                    }
                    ItemSortOption::EquipLevel => cmp_none_last(
                        has_equip_level(item_a).then_some(item_a.level_equip),
                        has_equip_level(item_b).then_some(item_b.level_equip),
                        direction,
                        i32::cmp,
                    ),
                    ItemSortOption::Name => ordered(direction, item_a.name.cmp(&item_b.name)),
                    ItemSortOption::Price => cmp_none_last(
                        quality_price(a, false),
                        quality_price(b, false),
                        direction,
                        i32::cmp,
                    ),
                    ItemSortOption::HqPrice => cmp_none_last(
                        quality_price(a, true),
                        quality_price(b, true),
                        direction,
                        i32::cmp,
                    ),
                    ItemSortOption::Vendor => cmp_none_last(
                        get_vendor_price(a),
                        get_vendor_price(b),
                        direction,
                        u32::cmp,
                    ),
                    ItemSortOption::World => {
                        cmp_none_last(world_of(a), world_of(b), direction, String::cmp)
                    }
                    ItemSortOption::Key => ordered(direction, a.cmp(&b)),
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

    // The nine columns, described once, in DOM order. This list replaces the
    // four hand-copied `grid-cols-[…]` class strings the table used to carry
    // — a header and a body copy in a single-world and a multi-world variant,
    // which had to stay character-identical with nothing to catch a drift
    // (issue #1080). `components/data_table.rs` derives all of them from
    // these tracks.
    //
    // Every explorer-specific context read stays here, in the page's own cell
    // closures: `listings_resource` (from `CheapestPrices`) and the price
    // scope's `scope_name` / `is_single_world`. The shared table knows about
    // none of them and cannot panic on a missing one.
    let columns: Vec<Column<ExplorerRow>> = vec![
        // Item icon.
        Column::new(
            COL_ICON.0,
            ColumnHeader::Empty,
            move |(id, _item): &ExplorerRow| {
                let item_id = id.0;
                view! {
                    <ItemTooltip item_id=item_id>
                        <AppLink href=move || format!("/item/{}/{}", scope_name.get(), item_id)>
                            <ItemIcon item_id=item_id icon_size=IconSize::Small />
                        </AppLink>
                    </ItemTooltip>
                }
                .into_any()
            },
        ),
        // Name, plus the compact metadata line that stands in for the
        // iLvl/Lv columns below `lg`.
        Column::new(
            COL_NAME.0,
            ColumnHeader::cell(move |class| {
                view! {
                    <SortableHeaderCell
                        mode=ItemSortOption::Name
                        label=t_string!(i18n, item_explorer_name).to_string()
                        class=class.unwrap_or_default()
                        sort_mode=sort
                        sort_dir=direction
                        reset_keys=RESET_ON_SORT
                    />
                }
                .into_any()
            }),
            move |(_id, item): &ExplorerRow| {
                let item = *item;
                view! {
                    <div class="flex flex-col min-w-0">
                        <AppLink href=move || format!("/item/{}/{}",
                            scope_name.get(),
                            item.key_id.0)
                            attr:class="font-medium leading-snug text-[color:var(--color-text)] truncate \
                                       hover:text-brand-300 transition-colors \
                                       hover:underline decoration-brand-300/30 underline-offset-4"
                        >
                            {item.name.as_str()}
                        </AppLink>
                        // Compact metadata, only below `lg` where the
                        // dedicated columns are hidden. Follows the same
                        // availability as those columns, so a minion row does
                        // not carry a lone "iLvl 0" on a phone either.
                        <div class="flex lg:hidden items-center gap-2 text-xs text-[color:var(--color-text-muted)]">
                            <div>
                                {move || column_on(COL_ID_ITEM_LEVEL).then(|| {
                                    view! {
                                        <span>{t!(i18n, item_explorer_ilvl_prefix)} " "{item.level_item}</span>
                                    }
                                })}
                            </div>
                            <div>
                                {move || (column_on(COL_ID_EQUIP_LEVEL) && item.level_equip > 1).then(|| {
                                    view! {
                                        <span>{t!(i18n, item_explorer_lv_prefix)} " "{item.level_equip}</span>
                                    }
                                })}
                            </div>
                        </div>
                    </div>
                }
                .into_any()
            },
        ),
        // Item level.
        Column::new(
            COL_ITEM_LEVEL.0,
            ColumnHeader::cell(move |class| {
                view! {
                    <SortableHeaderCell
                        mode=ItemSortOption::ItemLevel
                        label=t_string!(i18n, item_explorer_ilvl).to_string()
                        class=class.unwrap_or_default()
                        sort_mode=sort
                        sort_dir=direction
                        reset_keys=RESET_ON_SORT
                    />
                }
                .into_any()
            }),
            move |(_id, item): &ExplorerRow| {
                let level_item = item.level_item;
                view! {
                    <div role="cell" class="hidden lg:block text-sm text-[color:var(--color-text-muted)]">
                        {level_item}
                    </div>
                }
                .into_any()
            },
        )
        .visible(Signal::derive(move || column_on(COL_ID_ITEM_LEVEL))),
        // Equip level.
        Column::new(
            COL_EQUIP_LEVEL.0,
            ColumnHeader::cell(move |class| {
                view! {
                    <SortableHeaderCell
                        mode=ItemSortOption::EquipLevel
                        label=t_string!(i18n, item_explorer_col_equip_level).to_string()
                        class=class.unwrap_or_default()
                        sort_mode=sort
                        sort_dir=direction
                        reset_keys=RESET_ON_SORT
                    />
                }
                .into_any()
            }),
            move |(_id, item): &ExplorerRow| {
                let level_equip = item.level_equip;
                view! {
                    <div role="cell" class="hidden lg:block text-sm text-[color:var(--color-text-muted)]">
                        {if level_equip > 1 {
                            view! { <span>{level_equip}</span> }.into_any()
                        } else {
                            view! { <span>"—"</span> }.into_any()
                        }}
                    </div>
                }
                .into_any()
            },
        )
        .visible(Signal::derive(move || column_on(COL_ID_EQUIP_LEVEL))),
        // Cheapest NQ price.
        Column::new(
            COL_NQ.0,
            ColumnHeader::cell(move |class| {
                view! {
                    <SortableHeaderCell
                        mode=ItemSortOption::Price
                        label=t_string!(i18n, nq).to_string()
                        class=class.unwrap_or_default()
                        sort_mode=sort
                        sort_dir=direction
                        reset_keys=RESET_ON_SORT
                    />
                }
                .into_any()
            }),
            move |(id, _item): &ExplorerRow| {
                let id = **id;
                view! {
                    <div role="cell" class="text-sm">
                        <CheapestPrice item_id=id show_hq=false show_world=false />
                    </div>
                }
                .into_any()
            },
        ),
        // Cheapest HQ price. Always emits a stable wrapper div so the SSR and
        // CSR view trees agree on element shape/count for this slot (same
        // tachys-hydration reasoning as the old card layout).
        Column::new(
            COL_HQ.0,
            ColumnHeader::cell(move |class| {
                view! {
                    <SortableHeaderCell
                        mode=ItemSortOption::HqPrice
                        label=t_string!(i18n, hq).to_string()
                        class=class.unwrap_or_default()
                        sort_mode=sort
                        sort_dir=direction
                        reset_keys=RESET_ON_SORT
                    />
                }
                .into_any()
            }),
            move |(id, item): &ExplorerRow| {
                let id = **id;
                let can_be_hq = item.can_be_hq;
                view! {
                    <div role="cell" class="hidden lg:block text-sm">
                        {if can_be_hq {
                            view! {
                                <CheapestPrice item_id=id show_hq=true show_world=false />
                            }.into_any()
                        } else {
                            ().into_any()
                        }}
                    </div>
                }
                .into_any()
            },
        )
        .visible(Signal::derive(move || column_on(COL_ID_HQ))),
        // Vendor price.
        Column::new(
            COL_VENDOR.0,
            ColumnHeader::cell(move |class| {
                view! {
                    <SortableHeaderCell
                        mode=ItemSortOption::Vendor
                        label=t_string!(i18n, item_explorer_vendor).to_string()
                        class=class.unwrap_or_default()
                        sort_mode=sort
                        sort_dir=direction
                        reset_keys=RESET_ON_SORT
                    />
                }
                .into_any()
            }),
            move |(id, _item): &ExplorerRow| {
                let item_id = id.0;
                view! {
                    <div role="cell" class="hidden xl:block text-sm">
                        {if let Some(price) = crate::components::related_items::get_vendor_price(item_id) {
                            view! { <Gil amount=price as i32 /> }.into_any()
                        } else {
                            ().into_any()
                        }}
                    </div>
                }
                .into_any()
            },
        )
        .header_class(COL_VENDOR.1)
        .visible(Signal::derive(move || column_on(COL_ID_VENDOR))),
        // World holding the cheapest listing. Only exists when the scope
        // spans more than one world; when it doesn't, the cell stays in the
        // DOM as `hidden` and the column simply drops out of the derived
        // template, exactly as the two class-string variants did.
        Column::new(
            COL_WORLD.0,
            ColumnHeader::cell(move |class| {
                view! {
                    <SortableHeaderCell
                        mode=ItemSortOption::World
                        label=t_string!(i18n, item_explorer_col_world).to_string()
                        class=class.unwrap_or_default()
                        sort_mode=sort
                        sort_dir=direction
                        reset_keys=RESET_ON_SORT
                    />
                }
                .into_any()
            }),
            move |(id, _item): &ExplorerRow| {
                let item_id = id.0;
                view! {
                    <div
                        role="cell"
                        class=move || {
                            if world_column_on.get() {
                                "hidden xl:block truncate text-sm text-[color:var(--color-text-muted)]"
                            } else {
                                "hidden"
                            }
                        }
                    >
                        // Gated behind the same `hydrated` flag as the price
                        // sort — SSR and the first CSR render both show
                        // nothing, keeping shapes in sync (see the comment on
                        // `hydrated` above).
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
                }
                .into_any()
            },
        )
        .header_class(COL_WORLD.1)
        .visible(world_column_on),
        // Row actions.
        Column::new(
            COL_ACTIONS.0,
            ColumnHeader::Empty,
            move |(id, item): &ExplorerRow| {
                let item_id = id.0;
                let name = item.name.clone();
                view! {
                    <div role="cell" class="flex items-center justify-end gap-1">
                        <AddToList
                            item_id=item_id
                            class="flex items-center justify-center p-2 rounded hover:bg-white/10 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)] transition-colors"
                        />
                        <div class="p-1 rounded hover:bg-white/10 text-[color:var(--color-text-muted)] cursor-pointer" title=t_string!(i18n, item_explorer_copy_name).to_string()>
                            <Clipboard clipboard_text=name />
                        </div>
                    </div>
                }
                .into_any()
            },
        ),
    ];

    // ---- Control bar wiring -------------------------------------------
    //
    // The explorer used to carry a bespoke sticky bar: four `?sort=` buttons
    // and a direction pair, with no filters at all. It is the analyzer tools'
    // `ControlBar` now (#1296) — result count and sort control on row 1, one
    // chip per active filter on row 2, everything unset folded into
    // `+ Filter` so the bar costs the same height whether one filter is on or
    // none.

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
    // only sort control below `lg`, where the header row is hidden, so an
    // entry that silently does nothing costs more here than anywhere.
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
    let active_dir =
        Signal::derive(move || direction().unwrap_or_else(|| active_sort.get().default_dir()));

    let column_label = move |id: &str| -> String {
        match id {
            COL_ID_ITEM_LEVEL => t_string!(i18n, item_explorer_ilvl).to_string(),
            COL_ID_EQUIP_LEVEL => t_string!(i18n, item_explorer_col_equip_level).to_string(),
            COL_ID_HQ => t_string!(i18n, item_explorer_col_hq_price).to_string(),
            COL_ID_VENDOR => t_string!(i18n, item_explorer_vendor).to_string(),
            COL_ID_WORLD => t_string!(i18n, item_explorer_col_world).to_string(),
            _ => String::new(),
        }
    };

    // A column the set cannot fill stays in the picker, greyed, with the
    // reason: ticking it back on would produce a column of blanks, and
    // dropping the entry entirely would leave the reader wondering where the
    // column they know went.
    let column_options = Memo::new(move |_| {
        OPTIONAL_COLUMNS
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
                    label: column_label(id),
                    group: None,
                    disabled,
                    hint,
                }
            })
            .collect::<Vec<_>>()
    });

    let toggle_column = Callback::new(move |id: &'static str| {
        let mut cols = visible_cols.get_untracked();
        if !cols.remove(id) {
            cols.insert(id);
        }
        set_cols_param.set(Some(serialize_visible_cols(&cols, OPTIONAL_COLUMNS)));
    });
    let reset_columns = Callback::new(move |_| set_cols_param.set(None));

    let filter_label = move |id: &str| -> String {
        match id {
            FILTER_NAME => t_string!(i18n, item_explorer_filter_name).to_string(),
            FILTER_MIN_ILVL => t_string!(i18n, item_explorer_filter_min_ilvl).to_string(),
            FILTER_MAX_ILVL => t_string!(i18n, item_explorer_filter_max_ilvl).to_string(),
            FILTER_MIN_LV => t_string!(i18n, item_explorer_filter_min_lv).to_string(),
            FILTER_MAX_PRICE => t_string!(i18n, item_explorer_filter_max_price).to_string(),
            FILTER_VENDOR => t_string!(i18n, item_explorer_filter_vendor_only).to_string(),
            FILTER_HQ => t_string!(i18n, item_explorer_filter_hq_only).to_string(),
            FILTER_LISTED => t_string!(i18n, item_explorer_filter_listed_only).to_string(),
            _ => String::new(),
        }
    };

    // `+ Filter` offers what is neither on screen already nor meaningless for
    // this set — no HQ-only switch in a category with no HQ items.
    let filter_options = Memo::new(move |_| {
        let active = filter_active.get();
        ADDABLE_FILTERS
            .iter()
            .copied()
            .filter(|id| !active.contains(id))
            .filter(|id| {
                filter_requires_column(id).is_none_or(|column| availability.get().has(column))
            })
            .map(|id| FilterOption {
                id,
                label: filter_label(id),
            })
            .collect::<Vec<_>>()
    });

    // The three booleans commit straight to `true` — their chip's presence
    // *is* their value. The rest mount blank and editing, so the chip a click
    // produces is one the visitor can immediately type into.
    let add_filter = Callback::new(move |id: &'static str| match id {
        FILTER_VENDOR | FILTER_HQ | FILTER_LISTED => {
            set_filter_param.run((id, Some("true".to_string())))
        }
        id => filter_pending.set(Some(id)),
    });

    // A numeric chip is on screen when its param is set, or while it is the
    // one `+ Filter` just added and nothing has been typed into it yet.
    let number_chip_shown = move |key: &'static str, value: Option<i32>| {
        value.is_some() || filter_pending.get() == Some(key)
    };
    let commit_number = move |key: &'static str| {
        Callback::new(move |raw: Option<String>| {
            set_filter_param.run((
                key,
                raw.and_then(|raw| raw.trim().parse::<i32>().ok())
                    .map(|value| value.to_string()),
            ))
        })
    };

    view! {
        <Suspense fallback=move || view! { <div class="flex justify-center p-10"><Loading /></div> }>
        <div class="flex flex-col gap-6">
            // Sort, filters and the columns picker, in the bar every analyzer
            // tool uses. The sort control lives here rather than only on the
            // column headers because the header row is `hidden lg:grid`, so
            // below `lg` this is the only way to sort — and "Added" has no
            // column of its own at any width.
            <ControlBar
                summary=move || {
                    view! {
                        <span class="text-sm font-semibold text-[color:var(--color-text)] whitespace-nowrap truncate">
                            {move || t!(i18n, item_explorer_results_count, n = move || items_len.get())}
                        </span>
                    }
                    .into_any()
                }
                actions=move || {
                    view! {
                        <label class="flex items-center gap-1.5 min-w-0">
                            <span class="hidden md:inline text-xs font-bold uppercase tracking-wider text-[color:var(--color-text-muted)] truncate">
                                {t!(i18n, item_explorer_sort_by)}
                            </span>
                            <select
                                class="input input-sm min-w-0"
                                aria-label=t_string!(i18n, item_explorer_sort_by).to_string()
                                prop:value=move || active_sort.get().to_string()
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
                                {move || {
                                    sort_options
                                        .get()
                                        .into_iter()
                                        .map(|option| {
                                            let token = option.to_string();
                                            view! {
                                                <option
                                                    value=token.clone()
                                                    selected=move || active_sort.get() == option
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
                                let value = (next != active_sort.get_untracked().default_dir())
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
                    }
                    .into_any()
                }
                columns=column_options
                visible_columns=Signal::derive(move || visible_cols.get())
                on_toggle_column=toggle_column
                on_reset_columns=reset_columns
                available_filters=filter_options
                on_add_filter=add_filter
                on_clear_all=clear_filters
                empty_label=Signal::derive(move || {
                    t_string!(i18n, item_explorer_no_active_filters).to_string()
                })
                is_empty=Signal::derive(move || filter_active.get().is_empty())
            >
                {move || {
                    let name = filter_values.get().name;
                    (name.is_some() || filter_pending.get() == Some(FILTER_NAME))
                        .then(|| {
                            view! {
                                <FilterChip
                                    label=filter_label(FILTER_NAME)
                                    value=Signal::derive(move || filter_values.get().name)
                                    start_editing=filter_pending.get_untracked() == Some(FILTER_NAME)
                                    on_commit=Callback::new(move |raw: Option<String>| {
                                        set_filter_param.run((FILTER_NAME, raw))
                                    })
                                />
                            }
                        })
                }}
                {move || {
                    number_chip_shown(FILTER_MIN_ILVL, filter_values.get().min_ilvl)
                        .then(|| {
                            view! {
                                <FilterChip
                                    label=filter_label(FILTER_MIN_ILVL)
                                    value=Signal::derive(move || {
                                        filter_values.get().min_ilvl.map(|v| v.to_string())
                                    })
                                    numeric=true
                                    min="0"
                                    start_editing=filter_pending.get_untracked() == Some(FILTER_MIN_ILVL)
                                    on_commit=commit_number(FILTER_MIN_ILVL)
                                />
                            }
                        })
                }}
                {move || {
                    number_chip_shown(FILTER_MAX_ILVL, filter_values.get().max_ilvl)
                        .then(|| {
                            view! {
                                <FilterChip
                                    label=filter_label(FILTER_MAX_ILVL)
                                    value=Signal::derive(move || {
                                        filter_values.get().max_ilvl.map(|v| v.to_string())
                                    })
                                    numeric=true
                                    min="0"
                                    start_editing=filter_pending.get_untracked() == Some(FILTER_MAX_ILVL)
                                    on_commit=commit_number(FILTER_MAX_ILVL)
                                />
                            }
                        })
                }}
                {move || {
                    number_chip_shown(FILTER_MIN_LV, filter_values.get().min_lv)
                        .then(|| {
                            view! {
                                <FilterChip
                                    label=filter_label(FILTER_MIN_LV)
                                    value=Signal::derive(move || {
                                        filter_values.get().min_lv.map(|v| v.to_string())
                                    })
                                    numeric=true
                                    min="0"
                                    start_editing=filter_pending.get_untracked() == Some(FILTER_MIN_LV)
                                    on_commit=commit_number(FILTER_MIN_LV)
                                />
                            }
                        })
                }}
                {move || {
                    number_chip_shown(FILTER_MAX_PRICE, filter_values.get().max_price)
                        .then(|| {
                            view! {
                                <FilterChip
                                    label=filter_label(FILTER_MAX_PRICE)
                                    value=Signal::derive(move || {
                                        filter_values.get().max_price.map(|v| v.to_string())
                                    })
                                    numeric=true
                                    min="0"
                                    step="100"
                                    start_editing=filter_pending.get_untracked() == Some(FILTER_MAX_PRICE)
                                    on_commit=commit_number(FILTER_MAX_PRICE)
                                />
                            }
                        })
                }}
                // The three switches: the chip's presence is the value, so
                // there is nothing to type and `x` is the only edit.
                {move || {
                    filter_values.get().vendor_only.then(|| {
                        view! {
                            <FilterChip
                                label=filter_label(FILTER_VENDOR)
                                readonly=true
                                value=Signal::derive(|| None::<String>)
                                on_commit=Callback::new(move |_| set_filter_param.run((FILTER_VENDOR, None)))
                            />
                        }
                    })
                }}
                {move || {
                    filter_values.get().hq_only.then(|| {
                        view! {
                            <FilterChip
                                label=filter_label(FILTER_HQ)
                                readonly=true
                                value=Signal::derive(|| None::<String>)
                                on_commit=Callback::new(move |_| set_filter_param.run((FILTER_HQ, None)))
                            />
                        }
                    })
                }}
                {move || {
                    filter_values.get().listed_only.then(|| {
                        view! {
                            <FilterChip
                                label=filter_label(FILTER_LISTED)
                                readonly=true
                                value=Signal::derive(|| None::<String>)
                                on_commit=Callback::new(move |_| set_filter_param.run((FILTER_LISTED, None)))
                            />
                        }
                    })
                }}
            </ControlBar>

            // Results list: one row per item so prices line up in a
            // scannable column. One responsive layout in three tiers:
            // below `lg` only icon/name/NQ/actions (the rest collapses
            // into a compact line under the name), `lg` adds iLvl/Lv/HQ,
            // `xl` adds vendor and world. The full column set can't come
            // in earlier than `xl` — the fixed columns plus the app
            // sidebar leave `1fr` with no room and the item name
            // collapses to zero width.
            <DataTableGrid
                columns=columns
                rows=filtered_items
                key=|(id, item): &ExplorerRow| (id.0, item.name.clone())
                class="panel rounded-xl border border-white/5 divide-y divide-white/5 overflow-hidden"
                header_class="text-xs font-bold uppercase tracking-wider text-[color:var(--color-text-muted)]"
                row_class="hover:bg-white/5 transition-colors"
            />

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
        ADDABLE_FILTERS, DEFAULT_COLUMNS, EXPLORER_COLUMNS, ItemSortOption, OPTIONAL_COLUMNS,
        SORT_OPTIONS, canonical_job_acronym, collect_job_items_sorted, filter_requires_column,
        resolve_category_param, resolve_jobset_param,
    };
    use crate::components::data_table::check_header_class;
    use crate::components::sort_header::{SortColumn, SortDir};
    use crate::routes::item_explorer_filters::ExplorerFilters;
    use crate::routes::item_explorer_toolbar::{job_chip_slug, job_chips_sorted_in};
    use paginate::Pages;
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
    /// variant without listing it here would leave it unreachable below `lg`,
    /// where the sortable header row is hidden.
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
    /// orders by nothing.
    #[test]
    fn sorts_and_filters_reference_real_columns() {
        for option in SORT_OPTIONS {
            if let Some(column) = option.column() {
                assert!(
                    OPTIONAL_COLUMNS.contains(&column),
                    "{option} sorts on unknown column {column:?}",
                );
            }
        }
        for filter in ADDABLE_FILTERS {
            if let Some(column) = filter_requires_column(filter) {
                assert!(
                    OPTIONAL_COLUMNS.contains(&column),
                    "filter {filter:?} requires unknown column {column:?}",
                );
            }
        }
    }

    /// Every filter the `+ Filter` menu can add has to be one the chip row
    /// then draws, or adding it produces a URL param and no visible chip —
    /// a filter the visitor cannot see or clear.
    #[test]
    fn every_addable_filter_is_recognized() {
        let all_set = ExplorerFilters {
            name: Some("x".to_string()),
            min_ilvl: Some(1),
            max_ilvl: Some(2),
            min_lv: Some(3),
            max_price: Some(4),
            vendor_only: true,
            hq_only: true,
            listed_only: true,
        };
        for filter in ADDABLE_FILTERS {
            assert!(
                all_set.is_set(filter),
                "{filter:?} is never drawn as a chip"
            );
            assert!(
                !ExplorerFilters::default().is_set(filter),
                "{filter:?} reads as active with nothing set",
            );
        }
    }

    /// The columns picker starts from the full set; availability, not the
    /// default, is what takes a column away.
    #[test]
    fn every_optional_column_is_on_by_default() {
        assert_eq!(DEFAULT_COLUMNS, OPTIONAL_COLUMNS);
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

    /// Every header cell must be visible at exactly the breakpoints where its
    /// column owns a grid track.
    ///
    /// Counting tracks is not enough: the vendor column shipped for one commit
    /// with its tracks right (`xl` only) and its header class dropped, so at
    /// `lg` the header row had seven tracks and eight unhidden cells. VENDOR
    /// took the actions track, the actions header wrapped to an implicit
    /// second row, and the body rows — whose cells carry their own
    /// `hidden xl:block` — stayed correct, so header and body disagreed with
    /// nothing to catch it.
    #[test]
    fn header_classes_match_their_tracks() {
        for (index, (widths, header_class)) in EXPLORER_COLUMNS.iter().enumerate() {
            if let Err(problem) = check_header_class(widths, header_class) {
                panic!("column {index}: {problem}");
            }
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
