use std::borrow::Cow;
use std::{collections::HashSet, str::FromStr};

use crate::components::ad::Ad;
use crate::components::query_button::QueryButton;
use crate::components::toggle::Toggle;
use crate::components::{
    add_to_list::*, cheapest_price::*, fonts::*, meta::*, small_item_display::*,
};
use crate::CheapestPrices;
use icondata as i;
use itertools::Itertools;
use leptos::*;
use leptos_icons::*;
use leptos_router::*;
use log::info;
use paginate::Pages;
use percent_encoding::percent_decode_str;
use xiv_gen::{ClassJobCategory, Item, ItemId};

#[component]
fn SideMenuButton(href: String, children: Box<dyn Fn() -> Fragment>) -> impl IntoView {
    view! {
        <APersistQuery href remove_values=&["page", "menu-open"]>
            <div class="flex items-center gap-3 px-4 py-3 rounded-lg
                        transition-all duration-200
                        border border-transparent
                        hover:border-white/10
                        hover:bg-gradient-to-r hover:from-violet-800/20 hover:to-violet-700/10
                        active:from-violet-700/30 active:to-violet-600/20
                        text-gray-300 hover:text-amber-200
                        relative group">
                // Glossy highlight
                <div class="absolute inset-0 rounded-lg opacity-0 group-hover:opacity-100
                           transition-opacity duration-200
                           bg-gradient-to-b from-white/5 to-transparent
                           pointer-events-none"/>

                // Icon container with subtle glow
                <div class="relative">
                    <div class="absolute inset-0 rounded-full bg-violet-500/10 blur-sm
                               scale-150 opacity-0 group-hover:opacity-100
                               transition-opacity duration-200"/>
                    {children()}
                </div>
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
                            <ItemSearchCategoryIcon id=*id/>
                            {name}
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
        _ => {
            log::warn!("Unknown job acronym {job_acronym}");
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
                    view! {
                        <SideMenuButton href=["/items/jobset/", &job.abbreviation].concat()>
                            <ClassJobIcon id=job.key_id/>
                            // {&job.abbreviation}
                            {job.name_english.as_str()}
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
    let items = create_memo(move |_| {
        let cat = params()
            .get("category")
            .and_then(|cat| percent_encoding::percent_decode_str(cat).decode_utf8().ok())
            .and_then(|cat| {
                data.item_search_categorys
                    .iter()
                    .find(|(_id, category)| &category.name == &cat)
            })
            .map(|(id, _)| {
                let items = data
                    .items
                    .iter()
                    .filter(|(_, item)| item.item_search_category == *id)
                    .collect::<Vec<_>>();
                items
            });
        cat.unwrap_or_default()
    });
    let category_view_name = create_memo(move |_| {
        params()
            .get("category")
            .as_ref()
            .and_then(|cat| percent_decode_str(cat).decode_utf8().ok())
            .unwrap_or(Cow::from("Category View"))
            .to_string()
    });
    view! {
        <MetaTitle title=move || format!("{} - Item Explorer", category_view_name())/>
        <MetaDescription text=move || {
            ["List of items for the item category ", &category_view_name()].concat()
        }/>
        <h3 class="text-xl">{category_view_name}</h3>
        <ItemList items/>
    }
}

#[component]
pub fn JobItems() -> impl IntoView {
    let params = use_params_map();
    let data = xiv_gen_db::data();
    let (non_market, set_non_market) = create_query_signal::<bool>("show-non-market");
    let market_only = create_memo(move |_| !non_market().unwrap_or_default());
    let set_market_only =
        SignalSetter::map(move |market: bool| set_non_market((!market).then_some(true)));
    let items = create_memo(move |_| {
        let job_set = match params().get("jobset") {
            Some(p) => p.clone(),
            None => return vec![],
        };

        // let item_category_items = category
        // lookup jobs that match the acronym for the given job set
        let job_categories: HashSet<_> = data
            .class_job_categorys
            .iter()
            .filter(|(_id, job_category)| job_category_lookup(job_category, &job_set))
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
    let job_set = create_memo(move |_| {
        params()
            .get("jobset")
            .as_ref()
            .map(|s| s.as_str())
            .unwrap_or("Job Set")
            .to_string()
    });
    view! {
        <MetaTitle title=move || format!("{} - Item Explorer", job_set())/>
        <MetaDescription text=move || ["All items equippable by ", &job_set()].concat()/>
        <h3 class="text-xl">{job_set}</h3>
        <div class="flex-row">
            <Toggle
                checked=market_only
                set_checked=set_market_only
                checked_label="Filtering Unmarketable Items"
                unchecked_label="Showing all items"
            />
        </div>
        <ItemList items/>
    }
}

#[component]
pub fn DefaultItems() -> impl IntoView {
    view! {
        <MetaTitle title="Items Explorer"/>
        <MetaDescription text="Lookup items by their category. Similar to the market board categories that are visible in Final Fantasy 14. Find the cheapest minions, or find that new piece of glamour for your Summoner."/>
        <div class="flex flex-col">
            <div>"Choose a category from the menu to explore items."</div>
            <div>
                "Once you choose a category, you will be able to sort the items by price, date added, alphabetically, or by item level."
            </div>
            <div>""</div>
        </div>
    }
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

impl ToString for ItemSortOption {
    fn to_string(&self) -> String {
        match self {
            ItemSortOption::ItemLevel => "ilvl",
            ItemSortOption::Price => "price",
            ItemSortOption::Name => "name",
            ItemSortOption::Key => "key",
        }
        .to_string()
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

impl ToString for SortDirection {
    fn to_string(&self) -> String {
        match self {
            SortDirection::Asc => "asc",
            SortDirection::Desc => "desc",
        }
        .to_string()
    }
}

/// A URL that copies the existing query string but replaces the path
#[component]
pub fn APersistQuery(
    #[prop(into)] href: TextProp,
    children: Box<dyn Fn() -> Fragment>,
    #[prop(optional)] remove_values: &'static [&'static str],
) -> impl IntoView {
    let location = use_location();
    let query = location.query;
    let path = location.pathname;
    let href_2 = href.clone();
    let query = create_memo(move |_| {
        let mut query = query();
        for value in remove_values {
            query.remove(value);
        }
        query
    });
    let url = move || format!("{}{}", href_2.get(), query().to_query_string());
    let is_active = create_memo(move |_| {
        let link_path = href.get();

        path.with(|path| {
            info!("{link_path} {path}");
            &escape(&link_path) == path
        })
    });
    view! {
        <a aria-current=move || is_active.get().then(|| "page") href=url>
            {children}
        </a>
    }
}

#[component]
fn ItemList(items: Memo<Vec<(&'static ItemId, &'static Item)>>) -> impl IntoView {
    let (page, _set_page) = create_query_signal::<i32>("page");
    let (direction, _set_direction) = create_query_signal::<SortDirection>("dir");
    let (sort, _set_sort) = create_query_signal::<ItemSortOption>("sort");

    let cheapest_prices = use_context::<CheapestPrices>().unwrap();
    let items = create_memo(move |_| {
        let direction = direction().unwrap_or(SortDirection::Desc);
        let item_property = sort().unwrap_or(ItemSortOption::ItemLevel);
        let price_map = cheapest_prices.read_listings.get().and_then(|r| r.ok());
        items()
            .into_iter()
            .filter(|(id, _)| {
                if ItemSortOption::Price == item_property {
                    // filter items without a price if we're sorting by price
                    if let Some((_, map)) = &price_map {
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
                    ItemSortOption::ItemLevel => item_a.level_item.0.cmp(&item_b.level_item.0),
                    ItemSortOption::Name => item_a.name.cmp(&item_b.name),
                    // TODO lookup price data for this case
                    ItemSortOption::Price => {
                        if let Some((_, price_map)) = &price_map {
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
    let items_len = create_memo(move |_| items.with(|i| i.len()));
    let pages = move || Pages::new(items_len(), 50);
    let items = move || {
        // now take a subslice of the items
        let page = pages().with_offset((page().unwrap_or_default() - 1).try_into().unwrap_or(0));
        items.with(|items| {
            items
                .get(page.start..=page.end)
                .unwrap_or_default()
                .to_vec()
        })
    };
    view! {
        <div class="flex flex-col gap-4">
            // Sort and Direction Controls
            <div class="flex flex-col sm:flex-row justify-between gap-2">
                <div class="flex flex-row flex-wrap gap-1">
                    <QueryButton
                        query_name="sort"
                        value="key"
                        class="p-1 !text-violet-200 hover:text-violet-600"
                        active_classes="p-1 !text-violet-500"
                    >
                        <div class="flex flex-row items-center gap-1">
                            <Icon icon=i::BiCalendarAltRegular/>
                            <span class="hidden sm:inline">"ADDED"</span>
                        </div>
                    </QueryButton>
                    <QueryButton
                        query_name="sort"
                        value="price"
                        class="p-1 !text-violet-200 hover:text-violet-600"
                        active_classes="p-1 !text-violet-500"
                    >
                        <div class="flex flex-row items-center gap-1">
                            <Icon icon=i::ImPriceTag/>
                            <span class="hidden sm:inline">"PRICE"</span>
                        </div>
                    </QueryButton>
                    <QueryButton
                        query_name="sort"
                        value="name"
                        class="p-1 !text-violet-200 hover:text-violet-600"
                        active_classes="p-1 !text-violet-500"
                    >
                        "NAME"
                    </QueryButton>
                    <QueryButton
                        query_name="sort"
                        value="ilvl"
                        class="p-1 !text-violet-200 hover:text-violet-600"
                        active_classes="p-1 !text-violet-500"
                        default=true
                    >
                        "ILVL"
                    </QueryButton>
                </div>
                <div class="flex flex-row gap-1">
                    <QueryButton
                        query_name="dir"
                        value="asc"
                        class="p-1 !text-violet-200 hover:text-violet-600"
                        active_classes="p-1 !text-violet-500"
                    >
                        <div class="flex flex-row items-center gap-1">
                            <Icon icon=i::BiSortUpRegular/>
                            <span class="hidden sm:inline">"ASC"</span>
                        </div>
                    </QueryButton>
                    <QueryButton
                        query_name="dir"
                        value="desc"
                        class="p-1 !text-violet-200 hover:text-violet-600"
                        active_classes="p-1 !text-violet-500"
                        default=true
                    >
                        <div class="flex flex-row items-center gap-1">
                            <Icon icon=i::BiSortDownRegular/>
                            <span class="hidden sm:inline">"DESC"</span>
                        </div>
                    </QueryButton>
                </div>
            </div>

            // Pagination
            <div class="flex flex-row flex-wrap gap-1">
                {move || {
                    pages()
                        .into_iter()
                        .map(|page| {
                            view! {
                                <QueryButton
                                    query_name="page"
                                    value=(page.offset + 1).to_string()
                                    class="p-1 min-w-[2rem] text-center !text-violet-200 hover:text-violet-600"
                                    active_classes="p-1 !text-violet-500"
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
            <For
                each=items
                key=|(id, item)| (id.0, &item.name)
                children=|(id, item)| {
                    view! {
                        <div class="flex flex-col md:grid md:grid-cols-12 gap-2 p-3 rounded-lg
                                  border border-white/10 backdrop-blur-sm
                                  bg-gradient-to-br from-violet-950/20 to-violet-900/20
                                  hover:from-violet-900/30 hover:to-violet-800/30
                                  transition-all duration-200">
                            // Item Info Section
                            <div class="flex flex-row items-center justify-between md:col-span-5 gap-2 min-w-0"> // Added min-w-0
                                <div class="flex-1 min-w-0"> // Added container with min-w-0
                                    <SmallItemDisplay item=item/>
                                </div>
                                <div class="flex-shrink-0"> // Prevent shrinking of add button
                                    <AddToList item_id=id.0/>
                                </div>
                            </div>

                            // Normal Quality Price
                            <div class="md:col-span-3 flex flex-row md:justify-center items-center gap-2">
                                <span class="text-gray-400 md:hidden">"NQ: "</span>
                                <CheapestPrice item_id=*id show_hq=false/>
                            </div>

                            // High Quality Price (if available)
                            {move || {
                                if item.can_be_hq {
                                    view! {
                                        <div class="md:col-span-4 flex flex-row md:justify-center items-center gap-2">
                                            <span class="text-gray-400 md:hidden">"HQ: "</span>
                                            <CheapestPrice item_id=*id show_hq=true/>
                                        </div>
                                    }
                                } else {
                                    // Take up the space on desktop but don't show anything
                                    view! { <div class="md:col-span-4"></div> }
                                }
                            }}
                        </div>
                    }
                }
            />
            </div>

            // Next Page Button
            <QueryButton
                query_name="page"
                value=move || (page().unwrap_or(1) + 1).to_string()
                class=move || {
                    let pages = pages();
                    let page = page();
                    if pages.page_count() > page.unwrap_or(1).try_into().unwrap_or(1) {
                        "px-4 py-2 rounded-lg text-center
                             bg-violet-900/40 border border-violet-400/20
                             hover:bg-violet-800/40 hover:border-violet-400/30
                             text-amber-200 transition-all duration-200"
                    } else {
                        "hidden"
                    }
                }
                active_classes="p-1 !text-violet-500"
            >
                <div class="flex items-center justify-center gap-2">
                    <span>"Next Page:"</span>
                    <span class="font-bold">{page().unwrap_or(1) + 1}</span>
                    <Icon icon=i::BiChevronRightRegular/>
                </div>
            </QueryButton>
        </div>
    }
}

#[component]
fn CategorySection(
    title: &'static str,
    #[prop(optional)] category: Option<u8>,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    view! {
        <div class="p-4 space-y-4">
            <h2 class="text-xl font-bold text-amber-200">{title}</h2>
            {category.map(|cat| view! { <CategoryView category=cat/> })}
            {children.map(|c| c())}
        </div>
    }
}


#[component]
#[component]
pub fn ItemExplorer() -> impl IntoView {
    let (menu_open, set_open) = create_query_signal("menu-open");
    let menu_open = create_memo(move |_| menu_open().unwrap_or(false));
    const BASE_CLASSES: &str = "group px-4 py-2 rounded-lg flex items-center gap-2
                           transition-all duration-200 relative
                           border backdrop-blur-sm ";
    const OPEN_CLASSES: &str = "bg-violet-900/40 border-violet-400/20 text-amber-200";
    const CLOSED_CLASSES: &str = "bg-violet-950/20 border-white/10 text-gray-200 hover:text-amber-200";
    let button_classes = move || {
        if menu_open() {
            [BASE_CLASSES, OPEN_CLASSES].concat()
        } else {
            [BASE_CLASSES, CLOSED_CLASSES].concat()
        }
    };
    let menu_closed = Signal::derive(move || !menu_open());
    view! {
        <div class="main-content p-6">
            <div class="container mx-auto max-w-7xl">
                // Toggle Button
                <button
                    class=button_classes
                >
                    <div class="relative w-6 h-6">
                        <div class="absolute inset-0 transition-all duration-300"
                             class=("opacity-0", menu_closed)
                             class=("rotate-90", menu_closed)
                             class=("scale-0", menu_closed)>
                            <Icon icon=i::BiXRegular/>
                        </div>
                        <div class="absolute inset-0 transition-all duration-300"
                             class=("opacity-100", menu_open)
                             class=("rotate-0", menu_open)
                             class=("scale-100", menu_open)>
                            <Icon icon=i::BiMenuRegular/>
                        </div>
                    </div>
                    <span class="font-medium">
                        {move || if menu_open() { "Close Categories" } else { "Browse Categories" }}
                    </span>
                </button>

                <div class="relative mt-4">
                    // Mobile Overlay
                    {move || {
                        if menu_open() {
                            view! {
                                <div
                                    class="fixed inset-0 bg-black/50 backdrop-blur-sm z-40 md:hidden"
                                    on:click=move |_| set_open.set(Some(false))
                                />
                            }
                        } else {
                            view! { <div/> }
                        }
                    }}

                    // Sidebar
                    <div
                        class="fixed md:absolute top-0 bottom-0 left-0 z-50
                               w-[85vw] md:w-80 transition-all duration-300 ease-in-out
                               rounded-xl border border-white/10
                               bg-gradient-to-br from-violet-950/90 to-violet-900/80
                               backdrop-filter backdrop-blur-xl
                               min-h-screen"
                        class=("translate-x-0", move || menu_open())
                        class=("-translate-x-[105%]", move || !menu_open())
                        class=("opacity-0", move || !menu_open())
                        class=("opacity-100", move || menu_open())
                    >
                        // Glossy overlay effect
                        <div class="absolute inset-0 rounded-xl bg-gradient-to-br from-white/5 to-transparent pointer-events-none"/>

                        // Content container with fade edges
                        <div class="relative h-full">
                            // Top fade
                            <div class="absolute top-0 left-0 right-0 h-4 bg-gradient-to-b
                                        from-violet-950/50 to-transparent z-10 pointer-events-none"/>

                            // Main scrollable content
                            <div class="h-full overflow-y-auto overflow-x-hidden
                                       scrollbar-thin scrollbar-thumb-white/20 hover:scrollbar-thumb-violet-400/30
                                       scrollbar-track-transparent">
                                <div class="space-y-1 p-2">
                                    <CategorySection title="Weapons" category=1/>
                                    <CategorySection title="Armor" category=2/>
                                    <CategorySection title="Items" category=3/>
                                    <CategorySection title="Housing" category=4/>
                                    <CategorySection title="Job Sets">
                                        <JobsList/>
                                    </CategorySection>
                                </div>
                            </div>

                            // Bottom fade
                            <div class="absolute bottom-0 left-0 right-0 h-4 bg-gradient-to-t
                                        from-violet-950/50 to-transparent z-10 pointer-events-none"/>
                        </div>
                    </div>

                    // Main Content Area
                    <div
                        class="transition-all duration-300"
                        class=("md:ml-[21rem]", move || menu_open())
                    >
                        <div class="space-y-6">
                            <Ad class="w-full h-24 rounded-xl overflow-hidden"/>
                            <div class="p-6 rounded-xl bg-gradient-to-br from-violet-950/20 to-violet-900/20
                                      border border-white/10 backdrop-blur-sm">
                                <h1 class="text-2xl font-bold text-amber-200 mb-4">"Item Explorer"</h1>
                                <Outlet/>
                            </div>
                            <Ad class="w-full max-h-72 rounded-xl overflow-hidden"/>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    }
}
