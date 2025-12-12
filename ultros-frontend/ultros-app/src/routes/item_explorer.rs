use std::borrow::Cow;
use std::fmt::Display;
use std::{collections::HashSet, str::FromStr};

use crate::CheapestPrices;
use crate::components::clipboard::Clipboard;
use crate::components::query_button::QueryButton;
use crate::components::toggle::Toggle;
use crate::components::{add_to_list::*, cheapest_price::*, fonts::*, item_icon::*, meta::*};
use crate::global_state::home_world::get_price_zone;
use icondata as i;
use itertools::Itertools;
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
            <div class="flex items-center gap-3 px-3 py-2 rounded-md
            transition-all duration-200
            text-sm font-medium
            text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]
            hover:bg-white/5
            aria-[current]:text-brand-300 aria-[current]:bg-brand-500/10
            group">
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
                .filter(|(_id, job)| job.job_index > 0)
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
    let listings_resource = cheapest_prices.read_listings;
    let (price_zone, _) = get_price_zone();

    let sorted_items = Memo::new(move |_| {
        let direction = direction().unwrap_or(SortDirection::Desc);
        let item_property = sort().unwrap_or(ItemSortOption::ItemLevel);
        let price_map = listings_resource.get().and_then(|r| r.ok());
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
                    ItemSortOption::ItemLevel => item_a.level_item.0.cmp(&item_b.level_item.0),
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

    let items_len = Memo::new(move |_| sorted_items.with(|i| i.len()));
    let pages = Memo::new(move |_| Pages::new(items_len(), 50));

    let filtered_items = Memo::new(move |_| {
        let page = pages
            .get()
            .with_offset((page().unwrap_or_default() - 1).try_into().unwrap_or(0));
        sorted_items.with(|items| {
            items
                .get(page.start..=page.end)
                .unwrap_or_default()
                .to_vec()
        })
    });

    view! {
        <div class="flex flex-col gap-6">
            // Sort and Direction Controls - Floating / Sticky Bar
            <div class="flex flex-col sm:flex-row justify-between gap-4 p-4 rounded-xl panel items-center sticky top-[72px] lg:top-4 z-20 backdrop-blur-md bg-[color:var(--bg-panel)]/90 border border-white/5 shadow-lg">
                <div class="flex flex-row flex-wrap gap-2 items-center">
                    <span class="text-xs font-bold uppercase tracking-wider text-[color:var(--color-text-muted)] mr-2">"Sort By"</span>
                    <QueryButton
                        key="sort"
                        value="ilvl"
                        class="px-3 py-1.5 rounded-lg text-sm font-medium transition-colors text-[color:var(--color-text-muted)] hover:bg-white/5"
                        active_classes="px-3 py-1.5 rounded-lg text-sm font-medium !bg-brand-500/20 !text-brand-300 ring-1 ring-brand-500/50"
                        default=true
                    >
                        "iLvl"
                    </QueryButton>
                    <QueryButton
                        key="sort"
                        value="price"
                        class="px-3 py-1.5 rounded-lg text-sm font-medium transition-colors text-[color:var(--color-text-muted)] hover:bg-white/5"
                        active_classes="px-3 py-1.5 rounded-lg text-sm font-medium !bg-brand-500/20 !text-brand-300 ring-1 ring-brand-500/50"
                    >
                        "Price"
                    </QueryButton>
                    <QueryButton
                        key="sort"
                        value="name"
                        class="px-3 py-1.5 rounded-lg text-sm font-medium transition-colors text-[color:var(--color-text-muted)] hover:bg-white/5"
                        active_classes="px-3 py-1.5 rounded-lg text-sm font-medium !bg-brand-500/20 !text-brand-300 ring-1 ring-brand-500/50"
                    >
                        "Name"
                    </QueryButton>
                    <QueryButton
                        key="sort"
                        value="key"
                        class="px-3 py-1.5 rounded-lg text-sm font-medium transition-colors text-[color:var(--color-text-muted)] hover:bg-white/5"
                        active_classes="px-3 py-1.5 rounded-lg text-sm font-medium !bg-brand-500/20 !text-brand-300 ring-1 ring-brand-500/50"
                    >
                        "Added"
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

            // Item Grid
            <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-4 gap-4">
                <For
                    each=move || filtered_items.get()
                    key=|(id, item)| (id.0, item.name.clone())
                    children=move |(id, item)| {
                        view! {
                            <div class="group relative flex flex-col p-4 rounded-xl panel
                                        border border-white/5 hover:border-brand-500/30
                                        hover:shadow-lg hover:shadow-brand-500/5
                                        transition-all duration-300">
                                <div class="flex flex-row items-start gap-4 mb-4">
                                    <div class="shrink-0 relative">
                                         <A href=move || format!("/item/{}/{}",
                                            price_zone.get().as_ref().map(|z| z.get_name()).unwrap_or("North-America"),
                                            item.key_id.0)
                                         >
                                            <ItemIcon item_id=item.key_id.0 icon_size=IconSize::Medium />
                                         </A>
                                    </div>
                                    <div class="flex flex-col min-w-0 pt-0.5">
                                        <div class="flex items-center gap-2 mb-1.5 flex-wrap">
                                            <span class="text-xs font-bold px-1.5 py-0.5 rounded bg-white/10 text-[color:var(--color-text-muted)] whitespace-nowrap">
                                                "iLvl "{item.level_item.0}
                                            </span>
                                             {if item.level_equip > 1 {
                                                view! {
                                                    <span class="text-xs px-1.5 py-0.5 rounded bg-white/5 text-[color:var(--color-text-muted)] whitespace-nowrap">
                                                        "Lv "{item.level_equip}
                                                    </span>
                                                }.into_any()
                                            } else {
                                                view! { <span/> }.into_any()
                                            }}
                                        </div>
                                        <A href=move || format!("/item/{}/{}",
                                            price_zone.get().as_ref().map(|z| z.get_name()).unwrap_or("North-America"),
                                            item.key_id.0)
                                            attr:class="font-bold text-base leading-snug text-[color:var(--color-text)] \
                                                       group-hover:text-brand-300 transition-colors line-clamp-2 \
                                                       hover:underline decoration-brand-300/30 underline-offset-4"
                                         >
                                            {item.name.as_str()}
                                        </A>
                                    </div>
                                </div>
                                <div class="flex-1" />
                                <div class="flex flex-col gap-3 mt-2 pt-3 border-t border-white/5">
                                    <div class="flex flex-col gap-2 text-sm">
                                        <CheapestPrice item_id=*id show_hq=false label="NQ" />
                                        {if item.can_be_hq {
                                            view! {
                                                <CheapestPrice item_id=*id show_hq=true label="HQ" />
                                            }.into_any()
                                        } else {
                                            view! { <div/> }.into_any()
                                        }}
                                    </div>
                                    <div class="flex items-center gap-2 mt-1">
                                        <div class="flex-1">
                                            <AddToList
                                                item_id=id.0
                                                class="w-full flex items-center justify-center p-2 rounded hover:bg-white/10 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)] transition-colors"
                                            />
                                        </div>
                                        <div class="p-1 rounded hover:bg-white/10 text-[color:var(--color-text-muted)] cursor-pointer" title="Copy Name">
                                             <Clipboard clipboard_text=item.name.clone() />
                                        </div>
                                    </div>
                                </div>
                            </div>
                        }
                        .into_any()
                    }
                />
            </div>

            // Pagination
             <div class="flex justify-center mt-6">
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
                             bg-brand-900/40 border border-brand-400/20
                             hover:bg-brand-800/60 hover:border-brand-400/50 hover:shadow-lg hover:translate-y-[-2px]
                             text-brand-300 transition-all duration-300 group"
                    } else {
                        "hidden"
                    }
                })
                active_classes=""
            >
                <div class="flex items-center justify-center gap-2">
                    <span>"Load Next Page"</span>
                    <Icon icon=i::BiChevronRightRegular attr:class="group-hover:translate-x-1 transition-transform" />
                </div>
            </QueryButton>
            <div class="h-8" /> // Bottom spacing
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
        <details class="group/section" open>
            <summary class="flex items-center justify-between w-full px-2 py-2 cursor-pointer
                           text-xs font-bold uppercase tracking-wider text-[color:var(--color-text-muted)]
                           hover:text-[color:var(--color-text)] transition-colors select-none list-none">
                <span>{title}</span>
                <Icon icon=i::BiChevronDownRegular attr:class="transition-transform group-open/section:rotate-180" />
            </summary>
            <div class="pl-2 space-y-0.5 mt-1 border-l border-white/5 ml-2">
                {category.map(|cat| view! { <CategoryView category=cat /> })}
                {children.map(|c| c())}
            </div>
        </details>
    }
    .into_any()
}

#[component]
pub fn ItemExplorer() -> impl IntoView {
    let (menu_open, set_open) = query_signal("menu-open");
    let menu_open = Memo::new(move |_| menu_open().unwrap_or(false));

    view! {
        <div class="flex flex-col min-h-[calc(100vh-64px)]">
            // Mobile Header / Toggle
            <div class="lg:hidden p-4 border-b border-white/5 bg-[color:var(--bg-panel)] sticky top-0 z-30 flex items-center justify-between">
                <span class="font-bold text-lg">"Item Explorer"</span>
                <A
                    href=move || if menu_open() { "?" } else { "?menu-open=true" }.to_string()
                    attr:class="btn-secondary !p-2"
                >
                    <Icon icon=i::BiMenuRegular width="24" height="24" />
                </A>
            </div>

            <div class="flex flex-row grow relative">
                // Sidebar (Desktop Sticky / Mobile Drawer)
                <aside
                    class="fixed inset-y-0 left-0 z-40 bg-[color:var(--bg-panel)] border-r border-white/5
                           lg:static lg:block lg:z-auto w-[280px] shrink-0
                           transition-transform duration-300 ease-in-out"
                    class=("translate-x-0", move || menu_open())
                    class=("-translate-x-full", move || !menu_open())
                    class=("lg:translate-x-0", true)
                >
                    <div class="h-full overflow-y-auto scrollbar-thin p-4 space-y-6">
                        <div class="flex items-center justify-between lg:hidden mb-6">
                            <span class="font-bold text-xl">"Categories"</span>
                            <A href="?" attr:class="btn-ghost p-1">
                                <Icon icon=i::BiXRegular width="24" height="24" />
                            </A>
                        </div>

                        <div class="space-y-1">
                            <CategorySection title="Weapons" category=1 />
                            <CategorySection title="Armor" category=2 />
                            <CategorySection title="Items" category=3 />
                            <CategorySection title="Housing" category=4 />
                            <CategorySection title="Job Sets">
                                <JobsList />
                            </CategorySection>
                        </div>
                    </div>
                </aside>

                // Mobile Backend Backdrop
                {move || {
                    if menu_open() {
                        view! {
                            <div
                                class="fixed inset-0 bg-black/50 backdrop-blur-sm z-30 lg:hidden"
                                on:click=move |_| set_open.set(Some(false))
                            />
                        }.into_any()
                    } else {
                        view! { <div class="hidden" /> }.into_any()
                    }
                }}

                // Main Content Area
                <main class="flex-1 min-w-0 bg-[color:var(--bg-body)]">
                    <div class="p-4 lg:p-8 max-w-[1600px] mx-auto">
                        <Outlet />
                    </div>
                </main>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_job_filtering() {
        let data = xiv_gen_db::data();
        let jobs = &data.class_jobs;
        let visible_jobs: Vec<_> = jobs
            .iter()
            .filter(|(_id, job)| {
                let visible = job.job_index > 0;
                if !visible {
                    println!(
                        "Filtered out: {} (Parent: {})",
                        job.name, job.class_job_parent.0
                    );
                }
                visible
            })
            .collect();

        println!("Visible jobs count: {}", visible_jobs.len());
        for (_, job) in &visible_jobs {
            println!("Visible: {}", job.name);
        }

        assert!(
            !visible_jobs.is_empty(),
            "No jobs are visible! Filtering logic might be wrong."
        );
    }
}
