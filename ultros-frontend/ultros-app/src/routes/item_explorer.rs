use std::borrow::Cow;
use std::fmt::Display;
use std::{collections::HashSet, str::FromStr};

use crate::CheapestPrices;
use crate::components::ad::Ad;
use crate::components::clipboard::Clipboard;
use crate::components::query_button::QueryButton;
use crate::components::toggle::Toggle;
use crate::components::{
    add_to_list::*, cheapest_price::*, fonts::*, meta::*, small_item_display::*,
};
use icondata as i;
use itertools::Itertools;
use leptos::either::Either;
use leptos::prelude::*;
use leptos::reactive::wrappers::write::SignalSetter;
use leptos::text_prop::TextProp;
use leptos_icons::*;
use leptos_router::components::A;
use leptos_router::components::Outlet;
use leptos_router::hooks::{query_signal, use_location, use_params_map};
use leptos_router::location::Url;
use paginate::Pages;
use percent_encoding::percent_decode_str;
use xiv_gen::{ClassJobCategory, Item, ItemId};

#[component]
fn SideMenuButton<T>(href: String, children: TypedChildrenFn<T>) -> impl IntoView
where
    T: RenderHtml + 'static,
{
    let children = children.into_inner();
    view! {
        <APersistQuery href remove_values=&["page", "menu-open"]>
            <div class="flex items-center gap-1 px-1 py-1 rounded-lg
            transition-colors duration-200
            panel font-medium
            text-[color:var(--color-text)] hover:text-[color:var(--brand-fg)]
            relative group">


                {children()}
            </div>
        </APersistQuery>
    }
}

/// Displays buttons of categories
#[component]
fn CategoryView(category: u8) -> impl IntoView {
    let data = xiv_gen_db::data();
    let search_categories = &data.item_search_categorys;
    // let item_ui_category = &data.item_ui_categorys;
    let mut categories = search_categories
        .iter()
        .filter(|(_, cat)| cat.category == category)
        .map(|(id, cat)| {
            // lookup the ID for the map
            (cat.order, &cat.name, id)
        })
        .collect::<Vec<_>>();
    categories.sort_by_key(|(order, _, _)| *order);
    view! {
        <div class="flex flex-col text-xl">
            {categories
                .into_iter()
                .map(|(_, name, id)| {
                    view! {
                        <SideMenuButton href=["/items/category/", &name.replace("/", "%2F")]
                            .concat()>
                            <ItemSearchCategoryIcon id=*id />
                            {name.as_str()}
                        </SideMenuButton>
                    }
                })
                .collect::<Vec<_>>()}
        </div>
    }
}

/// Return true if the given acronym is in the given class job category
fn job_category_lookup(class_job_category: &ClassJobCategory, job_acronym: &str) -> bool {
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

#[component]
fn JobsList() -> impl IntoView {
    let jobs = &xiv_gen_db::data().class_jobs;
    let mut jobs: Vec<_> = jobs.iter().collect();
    jobs.sort_by_key(|(_, job)| job.ui_priority);
    view! {
        <div class="flex flex-col text-xl">
            {jobs
                .into_iter()
                .filter(|(_id, job)| job.class_job_parent.0 != 0)
                .map(|(_id, job)| {
                    let seg = if job.abbreviation.is_empty() { job.name.as_str() } else { job.abbreviation.as_str() };
                    let href = ["/items/jobset/", &seg.replace("/", "%2F")].concat();
                    view! {
                        <SideMenuButton href=href>
                            <ClassJobIcon id=job.key_id />
                            {seg}
                        </SideMenuButton>
                    }
                })
                .collect::<Vec<_>>()}
        </div>
    }
}

#[component]
pub fn CategoryItems() -> impl IntoView {
    let params = use_params_map();
    let data = xiv_gen_db::data();
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
                data.items
                    .iter()
                    .filter(|(_, item)| item.item_search_category == *id)
                    .collect::<Vec<_>>()
            });
        cat.unwrap_or_default()
    });
    let category_view_name = Memo::new(move |_| {
        params()
            .get("category")
            .as_ref()
            .and_then(|cat| percent_decode_str(cat).decode_utf8().ok())
            .unwrap_or(Cow::from("Category View"))
            .to_string()
    });
    view! {
        <MetaTitle title=move || format!("{} - Item Explorer", category_view_name()) />
        <MetaDescription text=move || {
            ["List of items for the item category ", &category_view_name()].concat()
        } />
        <h3 class="text-xl">{category_view_name}</h3>
        <ItemList items />
    }
    .into_any()
}

#[component]
pub fn JobItems() -> impl IntoView {
    let params = use_params_map();
    let data = xiv_gen_db::data();
    let (non_market, set_non_market) = query_signal::<bool>("show-non-market");
    let market_only = Memo::new(move |_| !non_market().unwrap_or_default());
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

        // lookup jobs that match the resolved acronym for the given job set
        let job_categories: HashSet<_> = data
            .class_job_categorys
            .iter()
            .filter(|(_id, job_category)| job_category_lookup(job_category, &canonical_abbr))
            .map(|(id, _)| *id)
            .collect();

        let market_only = market_only();
        let job_items: Vec<_> = data
            .items
            .iter()
            .filter(|(_id, item)| job_categories.contains(&item.class_job_category))
            .filter(|(_id, item)| !market_only || item.item_search_category.0 > 0)
            .collect();
        job_items
    });
    let job_set = Memo::new(move |_| {
        params()
            .get("jobset")
            .as_ref()
            .and_then(|s| percent_encoding::percent_decode_str(s).decode_utf8().ok())
            .map(|s| s.to_string())
            .unwrap_or("Job Set".to_string())
    });
    view! {
        <MetaTitle title=move || format!("{} - Item Explorer", job_set()) />
        <MetaDescription text=move || ["All items equippable by ", &job_set()].concat() />
        <h3 class="text-xl">{job_set}</h3>
        <div class="flex flex-row items-center gap-2">
            <Toggle
                checked=market_only
                set_checked=set_market_only
                checked_label="Filtering Unmarketable Items"
                unchecked_label="Showing all items"
            />
        </div>
        <ItemList items />
    }
    .into_any()
}

#[component]
pub fn DefaultItems() -> impl IntoView {
    view! {
        <MetaTitle title="Items Explorer" />
        <MetaDescription text="Lookup items by their category. Similar to the market board categories that are visible in Final Fantasy 14. Find the cheapest minions, or find that new piece of glamour for your Summoner." />
        <div class="flex flex-col">
            <div>"Choose a category from the menu to explore items."</div>
            <div>
                "Once you choose a category, you will be able to sort the items by price, date added, alphabetically, or by item level."
            </div>
            <div>""</div>
        </div>
    }.into_any()
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

/// A URL that copies the existing query string but replaces the path
#[component]
pub fn APersistQuery<T>(
    #[prop(into)] href: TextProp,
    children: TypedChildren<T>,
    #[prop(optional)] remove_values: &'static [&'static str],
) -> impl IntoView
where
    T: IntoView,
{
    let location = use_location();
    let query = location.query;
    let path = location.pathname;
    let href_2 = href.clone();
    let query = Memo::new(move |_| {
        let mut query = query();
        for value in remove_values {
            query.remove(value);
        }
        query
    });
    let url = move || format!("{}{}", href_2.get(), query().to_query_string());
    let is_active = Memo::new(move |_| {
        let link_path = href.get();

        path.with(|path| &Url::escape(&link_path) == path)
    });
    view! {
        <a aria-current=move || is_active.get().then_some("page") href=url>
            {children.into_inner()().into_view()}
        </a>
    }
}

#[component]
fn ItemList(items: Memo<Vec<(&'static ItemId, &'static Item)>>) -> impl IntoView {
    let (page, _set_page) = query_signal::<i32>("page");
    let (direction, _set_direction) = query_signal::<SortDirection>("dir");
    let (sort, _set_sort) = query_signal::<ItemSortOption>("sort");

    let cheapest_prices = use_context::<CheapestPrices>().unwrap();

    let items_len = Memo::new(move |_| items.with(|i| i.len()));
    let pages = move || Pages::new(items_len(), 50);

    view! {
        <div class="flex flex-col gap-4">
            // Sort and Direction Controls
            <div class="flex flex-col sm:flex-row justify-between gap-2">
                <div class="flex flex-row flex-wrap gap-1">
                    <QueryButton
                        key="sort"
                        value="key"
                        class="p-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--brand-fg)]"
                        active_classes="p-1 text-[color:var(--brand-fg)] underline"
                    >
                        <div class="flex flex-row items-center gap-1">
                            <Icon icon=i::BiCalendarAltRegular />
                            <span class="hidden sm:inline">"ADDED"</span>
                        </div>
                    </QueryButton>
                    <QueryButton
                        key="sort"
                        value="price"
                        class="p-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--brand-fg)]"
                        active_classes="p-1 text-[color:var(--brand-fg)] underline"
                    >
                        <div class="flex flex-row items-center gap-1">
                            <Icon icon=i::ImPriceTag />
                            <span class="hidden sm:inline">"PRICE"</span>
                        </div>
                    </QueryButton>
                    <QueryButton
                        key="sort"
                        value="name"
                        class="p-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--brand-fg)]"
                        active_classes="p-1 text-[color:var(--brand-fg)] underline"
                    >
                        "NAME"
                    </QueryButton>
                    <QueryButton
                        key="sort"
                        value="ilvl"
                        class="p-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--brand-fg)]"
                        active_classes="p-1 !text-brand-300"
                        default=true
                    >
                        "ILVL"
                    </QueryButton>
                </div>
                <div class="flex flex-row gap-1">
                    <QueryButton
                        key="dir"
                        value="asc"
                        class="p-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--brand-fg)]"
                        active_classes="p-1 text-[color:var(--brand-fg)] underline"
                    >
                        <div class="flex flex-row items-center gap-1">
                            <Icon icon=i::BiSortUpRegular />
                            <span class="hidden sm:inline">"ASC"</span>
                        </div>
                    </QueryButton>
                    <QueryButton
                        key="dir"
                        value="desc"
                        class="p-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--brand-fg)]"
                        active_classes="p-1 text-[color:var(--brand-fg)] underline"
                        default=true
                    >
                        <div class="flex flex-row items-center gap-1">
                            <Icon icon=i::BiSortDownRegular />
                            <span class="hidden sm:inline">"DESC"</span>
                        </div>
                    </QueryButton>
                </div>
            </div>

            // Pagination
            <div class="flex flex-row flex-wrap gap-1">
                {move || {
                    pages()
                        .map(|page| {
                            view! {
                                <QueryButton
                                    key="page"
                                    value=(page.offset + 1).to_string()
                                    class="p-1 min-w-[2rem] text-center !text-brand-200 hover:text-brand-300"
                                    active_classes="p-1 text-[color:var(--brand-fg)] underline"
                                    default=page.offset == 0
                                >
                                    {page.offset + 1}
                                </QueryButton>
                            }
                        })
                        .collect::<Vec<_>>()
                }}
            </div>

            // Item List
            <div class="flex flex-col gap-2">
                <Suspense>
                    {move || {
                        let items = Memo::new(move |_| {
                            let direction = direction().unwrap_or(SortDirection::Desc);
                            let item_property = sort().unwrap_or(ItemSortOption::ItemLevel);
                            let price_map = cheapest_prices
                                .read_listings
                                .get()
                                .and_then(|r| r.ok());
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
                                        ItemSortOption::ItemLevel => {
                                            item_a.level_item.0.cmp(&item_b.level_item.0)
                                        }
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
                                                item_a.level_item.0.cmp(&item_b.level_item.0)
                                            }
                                        }
                                        ItemSortOption::Key => item_a.key_id.0.cmp(&item_b.key_id.0),
                                    }
                                })
                                .collect::<Vec<_>>()
                        });
                        let items = move || {
                            let page = pages()
                                .with_offset(
                                    (page().unwrap_or_default() - 1).try_into().unwrap_or(0),
                                );
                            items
                                .with(|items| {
                                    items.get(page.start..=page.end).unwrap_or_default().to_vec()
                                })
                        };
                        // filter items without a price if we're sorting by price
                        // TODO lookup price data for this case
                        // now take a subslice of the items
                        view! {
                            <For
                                each=items
                                key=|(id, item)| (id.0, &item.name)
                                children=|(id, item)| {
                                    view! {
                                        <div class="grid grid-cols-1 md:grid-cols-12 gap-3 p-4 rounded-xl
                                        panel
                                        transition-colors duration-200
                                        items-start md:items-center text-base md:text-lg">
                                            // Item Info Section
                                            <div class="md:col-span-8 flex flex-row items-center gap-2 min-w-0 w-full">
                                                // Added container with min-w-0
                                                <div class="flex-1 min-w-0 flex flex-row items-center gap-3">
                                                    <SmallItemDisplay item=item />
                                                    <span class="hidden md:inline text-[color:var(--color-text-muted)] whitespace-nowrap">
                                                        "min level: "{item.level_equip}
                                                    </span>
                                                    <Clipboard clipboard_text=item.name.clone() />
                                                </div>
                                            </div>
                                            // Prevent shrinking of add button
                                            <div class="md:col-span-2 shrink-0 flex justify-start md:justify-center w-full">
                                                <AddToList item_id=id.0 />
                                            </div>


                                            // Normal Quality Price
                                            <div class="md:col-span-2 flex flex-col md:flex-row justify-between md:justify-end items-start md:items-center gap-2 md:gap-4 w-full">
                                                <div class="flex flex-row items-center gap-2 whitespace-nowrap">
                                                    <span class="text-[color:var(--color-text-muted)] md:hidden">"NQ: "</span>
                                                    <CheapestPrice item_id=*id show_hq=false />
                                                </div>
                                                {move || {
                                                    if item.can_be_hq {
                                                        Either::Left(
                                                            view! {
                                                                <div class="flex flex-row items-center gap-2 whitespace-nowrap">
                                                                    <span class="text-[color:var(--color-text-muted)] md:hidden">"HQ: "</span>
                                                                    <CheapestPrice item_id=*id show_hq=true />
                                                                </div>
                                                            },
                                                        )
                                                    } else {
                                                        Either::Right(view! { <div /> })
                                                    }
                                                }}
                                            </div>
                                        </div>
                                    }
                                        .into_any()
                                }
                            />
                        }
                    }}
                </Suspense>
            </div>
            // Next Page Button
            <QueryButton
                key="page"
                value=Signal::derive(move || (page().unwrap_or(1) + 1).to_string())
                class=Signal::derive(move || {
                    let pages = pages();
                    let page = page();
                    if pages.page_count() > page.unwrap_or(1).try_into().unwrap_or(1) {
                        "px-4 py-2 rounded-lg text-center
                             bg-brand-900/40 border border-brand-400/20
                             hover:bg-brand-800/40 hover:border-brand-400/30
                             text-brand-300 transition-all duration-200"
                    } else {
                        "hidden"
                    }
                })
                active_classes="p-1 !text-brand-500"
            >
                <div class="flex items-center justify-center gap-2">
                    <span>"Next Page:"</span>
                    <span class="font-bold">{page().unwrap_or(1) + 1}</span>
                    <Icon icon=i::BiChevronRightRegular />
                </div>
            </QueryButton>
        </div>
    }.into_any()
}

#[component]
fn CategorySection(
    title: &'static str,
    #[prop(optional)] category: Option<u8>,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    view! {
        <div class="p-4 space-y-4">
            <h2 class="text-xl font-bold text-brand-200">{title}</h2>
            {category.map(|cat| view! { <CategoryView category=cat /> })}
            {children.map(|c| c())}
        </div>
    }
    .into_any()
}

#[component]
pub fn ItemExplorer() -> impl IntoView {
    let (menu_open, set_open) = query_signal("menu-open");
    let menu_open = Memo::new(move |_| menu_open().unwrap_or(false));
    const BASE_CLASSES: &str =
        "btn-secondary flex items-center gap-1 text-xs sm:text-sm font-medium";
    const OPEN_CLASSES: &str = "bg-[color:color-mix(in_srgb,var(--brand-ring)_22%,transparent)]";
    const CLOSED_CLASSES: &str = "";
    let button_classes = move || {
        if menu_open() {
            [BASE_CLASSES, OPEN_CLASSES].concat()
        } else {
            [BASE_CLASSES, CLOSED_CLASSES].concat()
        }
    };
    let menu_closed = Signal::derive(move || !menu_open());
    view! {
        <div class="main-content p-3 md:p-6">
            <div class="container mx-auto max-w-7xl">
                // Toggle Button
                <A
                    attr:class=button_classes
                    href=move || if menu_open() { "?" } else { "?menu-open=true" }.to_string()
                >
                    <div class="relative w-6 h-6 items-center">
                        <div
                            class="absolute inset-0 transition-all duration-300
                            text-[color:var(--color-text)] hover:text-[color:var(--brand-fg)] aria-current:text-[color:var(--brand-fg)]"
                            class=(["opacity-0", "rotate-90", "scale-0"], menu_closed)
                        >
                            <Icon icon=i::BiXRegular />
                        </div>
                        <div
                            class="absolute inset-0 transition-all duration-300"
                            class=(["opacity-100", "rotate-0", "scale-100"], menu_open)
                        >
                            <Icon icon=i::BiMenuRegular />
                        </div>
                    </div>
                    <span class="font-extrabold">
                        {move || if menu_open() { "Close Categories" } else { "Browse Categories" }}
                    </span>
                </A>

                <div class="relative mt-4">
                    // Mobile Overlay
                    {move || {
                        if menu_open() {
                            Either::Left(
                                view! {
                                    <div
                                        class="fixed inset-0 z-40 md:hidden bg-[color:color-mix(in_srgb,var(--color-text)_30%,transparent)]"
                                        on:click=move |_| set_open.set(Some(false))
                                    />
                                },
                            )
                        } else {
                            Either::Right(view! { <div /> })
                        }
                    }} // Sidebar
                    <div
                        class="fixed md:absolute top-0 bottom-0 left-0 z-50
                        w-[92vw] sm:w-[85vw] md:w-80 transition-all duration-300 ease-in-out
                        panel
                        min-h-screen"
                        class=("translate-x-0", move || menu_open())
                        class=("-translate-x-[105%]", move || !menu_open())
                        class=("opacity-0", move || !menu_open())
                        class=("opacity-100", move || menu_open())
                    >


                        // Content container with fade edges
                        <div class="relative h-full">


                            // Main scrollable content
                            <div class="h-full overflow-y-auto overflow-x-hidden
                            scrollbar-thin">
                                <div class="space-y-1 p-2">
                                    <CategorySection title="Weapons" category=1 />
                                    <CategorySection title="Armor" category=2 />
                                    <CategorySection title="Items" category=3 />
                                    <CategorySection title="Housing" category=4 />
                                    <CategorySection title="Job Sets">
                                        <JobsList />
                                    </CategorySection>
                                </div>
                            </div>


                        </div>
                    </div> // Main Content Area
                    <div
                        class="transition-all duration-300"
                        class=("md:ml-[21rem]", move || menu_open())
                    >
                        <div class="space-y-6">
                            <Ad class="w-full h-24 rounded-xl overflow-hidden" />
                            <div class="p-6 rounded-xl panel">
                                <h1 class="text-2xl font-bold text-brand-200 mb-4">
                                    "Item Explorer"
                                </h1>
                                <Outlet />
                            </div>
                            <Ad class="w-full max-h-72 rounded-xl overflow-hidden" />
                        </div>
                    </div>
                </div>
            </div>
        </div>
    }
    .into_any()
}
