use std::{cmp::Reverse, collections::HashSet};

use crate::components::{
    cheapest_price::*, fonts::*, meta::*, small_item_display::*, tooltip::*, virtual_scroller::*,
};
use leptos::*;
use leptos_router::*;
use urlencoding::{decode, encode};
use xiv_gen::{ClassJobCategory, Item, ItemId};

/// Displays buttons of categories
#[component]
fn CategoryView(cx: Scope, category: u8) -> impl IntoView {
    let data = xiv_gen_db::decompress_data();
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
    view! {cx,
        <div class="flex flex-row flex-wrap">
        {categories.into_iter()
            .map(|(_, name, id)| view! {cx,
                <Tooltip tooltip_text=name.to_string()>
                    <A  href=format!("/items/category/{}", encode(name))>
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
            return false;
        }
    }
}

#[component]
fn JobsList(cx: Scope) -> impl IntoView {
    let jobs = &xiv_gen_db::decompress_data().class_jobs;
    let mut jobs: Vec<_> = jobs.iter().collect();
    jobs.sort_by_key(|(_, job)| job.ui_priority);
    view! {cx, <div class="flex-wrap" style="padding: 5px;">
        {jobs.into_iter()
            // .filter(|(_id, job)| job.class_job_parent.0 == 0)
            .map(|(_id, job)| view!{cx, <A href=format!("/items/jobset/{}", job.abbreviation)>
            // {&job.abbreviation}
            <Tooltip tooltip_text=job.name_english.clone()>
                <ClassJobIcon id=job.key_id />
            </Tooltip>
        </A>}).collect::<Vec<_>>()}
    </div>}
}

#[component]
pub fn CategoryItems(cx: Scope) -> impl IntoView {
    let params = use_params_map(cx);
    let data = xiv_gen_db::decompress_data();
    let items = create_memo(cx, move |_| {
        let cat = params()
            .get("category")
            .and_then(|cat| decode(&cat).ok())
            .and_then(|cat| {
                data.item_search_categorys
                    .iter()
                    .find(|(_id, category)| category.name == cat)
            })
            .map(|(id, _)| {
                let mut items = data
                    .items
                    .iter()
                    .filter(|(_, item)| item.item_search_category == *id)
                    .collect::<Vec<_>>();
                items.sort_by_key(|(_, item)| Reverse(item.level_item.0));
                items
            });
        cat.unwrap_or_default()
    });
    let category_view_name = create_memo(cx, move |_| {
        params()
            .get("category")
            .as_ref()
            .map(|c| c.as_str())
            .unwrap_or("Category View")
            .to_string()
    });
    view! {cx,
    <MetaTitle title=category_view_name/>
    <MetaDescription text=move || format!("List of items for the item category {}", category_view_name())/>
    <ItemList items />}
}

#[component]
pub fn JobItems(cx: Scope) -> impl IntoView {
    let params = use_params_map(cx);
    let data = xiv_gen_db::decompress_data();
    let (market_only, set_market_only) = create_signal(cx, true);
    let items = create_memo(cx, move |_| {
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
        let mut job_items: Vec<_> = data
            .items
            .iter()
            .filter(|(_id, item)| job_categories.contains(&item.class_job_category))
            .filter(|(_id, item)| !market_only || item.item_search_category.0 > 0)
            .collect();

        job_items.sort_by_key(|(_, item)| Reverse(item.level_item.0));
        job_items
    });
    view! {cx,
        <MetaTitle title=move || format!("{}", params().get("jobset").as_ref().map(|s| s.as_str()).unwrap_or("Job Set"))/>
        <MetaDescription text=move || format!("All items equippable by {}", params().get("jobset").as_ref().map(|s| s.as_str()).unwrap_or_default())/>
    <div class="flex-row">
        <label for="marketable-only">"Marketable Only"</label>
        <input type="checkbox" prop:checked=market_only name="market-only" on:change=move |_e| {
            set_market_only(!market_only())
        } />
    </div>
    <ItemList items />}
}

#[component]
fn ItemList(cx: Scope, items: Memo<Vec<(&'static ItemId, &'static Item)>>) -> impl IntoView {
    view! {cx,
    <VirtualScroller
        each=items.into()
        key=|(id, item)| (id.0, &item.name)
        view=|cx, (id, item)| view!{cx, <div class="flex-row" style="min-width: 500px;">
            <SmallItemDisplay item=item />
            <CheapestPrice item_id=*id hq=None />
        </div> } viewport_height=800.0 row_height=27.3/>}
}

#[component]
pub fn ItemExplorer(cx: Scope) -> impl IntoView {
    view! {cx,
        <div class="container">
            <div class="main-content flex">
                <div class="flex-column" style="width: 250px; font-size: 1.2em">
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
                <div class="flex-column" style="flex-grow: 1;">
                    <Outlet/>
                </div>
            </div>
        </div>
    }
}
