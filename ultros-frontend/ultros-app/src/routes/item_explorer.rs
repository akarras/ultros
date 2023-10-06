use std::{collections::HashSet, str::FromStr};

use crate::components::{
    ad::Ad, cheapest_price::*, fonts::*, meta::*, small_item_display::*, tooltip::*,
};
use crate::CheapestPrices;
use itertools::Itertools;
use leptos::*;
use leptos_router::*;
use paginate::Pages;
use urlencoding::{decode, encode};
use xiv_gen::{ClassJobCategory, Item, ItemId};

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
        <div class="flex flex-row flex-wrap text-2xl p-2">
        {categories.into_iter()
            .map(|(_, name, id)| view! {
                <Tooltip tooltip_text=Oco::from(name.as_str())>
                    <A href=["/items/category/", &encode(name)].concat()>
                        <ItemSearchCategoryIcon id=*id />
                    </A>
                </Tooltip>
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
    view! {<div class="flex flex-wrap text-2xl p-2">
        {jobs.into_iter()
            // .filter(|(_id, job)| job.class_job_parent.0 == 0)
            .map(|(_id, job)| view!{<A href=["/items/jobset/", &job.abbreviation].concat()>
            // {&job.abbreviation}
            <Tooltip tooltip_text=Oco::from(job.name_english.as_str())>
                <ClassJobIcon id=job.key_id />
            </Tooltip>
        </A>}).collect::<Vec<_>>()}
    </div>}
}

#[component]
pub fn CategoryItems() -> impl IntoView {
    let params = use_params_map();
    let data = xiv_gen_db::data();
    let items = create_memo(move |_| {
        let cat = params()
            .get("category")
            .and_then(|cat| decode(cat).ok())
            .and_then(|cat| {
                data.item_search_categorys
                    .iter()
                    .find(|(_id, category)| category.name == cat)
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
            .and_then(|cat| decode(cat).ok())
            .map(|c| c.to_string())
            .unwrap_or("Category View".to_string())
            .to_string()
    });
    view! {
    <MetaTitle title=category_view_name/>
    <MetaDescription text=move || ["List of items for the item category ", &category_view_name()].concat()/>
    <h3 class="text-xl">{category_view_name}</h3>
    <ItemList items />}
}

#[component]
pub fn JobItems() -> impl IntoView {
    let params = use_params_map();
    let data = xiv_gen_db::data();
    let (market_only, set_market_only) = create_signal(true);
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
        <MetaTitle title=job_set/>
        <MetaDescription text=move || ["All items equippable by ", &job_set()].concat()/>
        <h3 class="text-xl">{job_set}</h3>
    <div class="flex-row">
        <label for="marketable-only">"Marketable Only"</label>
        <input type="checkbox" prop:checked=market_only name="market-only" on:change=move |_e| {
            set_market_only(!market_only())
        } />
    </div>
    <ItemList items />}
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

/// A button that sets the query property to the given value
#[component]
pub fn QueryButton(
    #[prop(into)] query_name: TextProp,
    /// default state classes
    #[prop(into)]
    class: TextProp,
    /// classes that will replace the main classes when this is active
    #[prop(into)]
    active_classes: TextProp,
    #[prop(into)] value: TextProp,
    #[prop(optional)] default: bool,
    children: Box<dyn Fn() -> Fragment>,
) -> impl IntoView {
    let Location {
        pathname, query, ..
    } = use_location();
    let query_1 = query_name.clone();
    let value_1 = value.clone();
    let is_active = move || {
        let query_name = query_1.get();
        let value = value_1.get();
        query
            .with(|q| q.get(&query_name).map(|query_value| query_value == &value))
            .unwrap_or(default)
    };
    view! { <a class=move || if is_active() { active_classes.get() } else { class.get() }.to_string() href=move || {
        let mut query = query();
        let _ = query.insert(query_name.get().to_string(), value.get().to_string());
        format!("{}{}", pathname(), query.to_query_string())
    }>{children}</a> }
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
        items()[page.start..=page.end].to_vec()
    };
    view! {
    <div class="flex flex-row">
        <QueryButton query_name="dir" value="asc" class="p-1 !text-violet-200 hover:text-violet-600" active_classes="p-1 !text-violet-500">
            "ASC"
        </QueryButton>
        <QueryButton query_name="dir" value="desc" class="p-1 !text-violet-200 hover:text-violet-600" active_classes="p-1 !text-violet-500" default=true>
            "DESC"
        </QueryButton>
        <QueryButton query_name="sort" value="key" class="p-1 !text-violet-200 hover:text-violet-600" active_classes="p-1 !text-violet-500">
            "ADDED"
        </QueryButton>
        <QueryButton query_name="sort" value="price" class="p-1 !text-violet-200 hover:text-violet-600" active_classes="p-1 !text-violet-500">
            "PRICE"
        </QueryButton>
        <QueryButton query_name="sort" value="name" class="p-1 !text-violet-200 hover:text-violet-600" active_classes="p-1 !text-violet-500">
            "NAME"
        </QueryButton>
        <QueryButton query_name="sort" value="ilvl" class="p-1 !text-violet-200 hover:text-violet-600" active_classes="p-1 !text-violet-500" default=true>
            "ILVL"
        </QueryButton>
    </div>
    <div class="flex flex-row flex-wrap">
        {pages().into_iter().map(|page| {
            view!{
                <QueryButton query_name="page" value=(page.offset + 1).to_string() class="p-1 !text-violet-200 hover:text-violet-600" active_classes="p-1 !text-violet-500" default=page.offset == 0>
                    {page.offset + 1}
                </QueryButton>
            }
        }).collect::<Vec<_>>()}
    </div>
    <For
        each=items
        key=|(id, item)| (id.0, &item.name)
        children=|(id, item)| view!{<div class="grid xl:grid-cols-4 grid-flow-row gap-1">
            <div class="xl:col-span-2"><SmallItemDisplay item=item /></div>
            <CheapestPrice item_id=*id show_hq=false />
            {item.can_be_hq.then(|| view!{<CheapestPrice item_id=*id show_hq=true />})}
        </div> }/>}
}

#[component]
pub fn ItemExplorer() -> impl IntoView {
    view! {
        <div class="main-content">
            <div class="mx-auto container flex flex-col md:flex-row items-start">
                <div class="flex grow flex-row items-start">
                    <div class="flex flex-col sm:text-3xl text-lg max-w-sm shrink">
                        "Weapons"
                        <CategoryView category=1 />
                        "Armor"
                        <CategoryView category=2 />
                        "Items"
                        <CategoryView category=3 />
                        "Housing"
                        <CategoryView category=4 />
                        "Job Set"
                        <JobsList />
                    </div>
                    <div class="flex flex-col grow">
                        <Outlet />
                    </div>
                </div>
                <div class="w-40">
                    <Ad class="h-96 md:h-[50vh]"/>
                </div>
            </div>
        </div>
    }
}
