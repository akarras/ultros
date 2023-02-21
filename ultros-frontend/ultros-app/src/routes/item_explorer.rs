use std::cmp::Reverse;

use crate::components::{cheapest_price::*, fonts::*, item_icon::*, tooltip::*};
use leptos::*;
use leptos_router::*;
use ultros_api_types::icon_size::IconSize;
use urlencoding::{decode, encode};
use xiv_gen::ClassJobCategory;

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
                    <A  href=format!("/items/{}", encode(name))>
                        <ItemSearchCategoryIcon id=*id />
                    </A>
                </Tooltip>
            })
            .collect::<Vec<_>>()}
        </div>
    }
}

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
    view!{cx, <div class="flex-wrap">
            {jobs.into_iter().map(|(_id, job)| view!{cx, <A href=format!("/items/jobset/{}", job.abbreviation)>
                {&job.abbreviation}
            </A>}).collect::<Vec<_>>()}
        </div>}
}

#[component]
pub fn ItemExplorer(cx: Scope) -> impl IntoView {
    let params = use_params_map(cx);
    let data = xiv_gen_db::decompress_data();
    view! {cx,
        <div class="container">
            <div class="main-content flex">
                <div class="flex-column" style="width: 250px; font-size: 2em">
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
                <div class="flex-column">
                    {move || {
                        let cat = params().get("category")?.clone();
                        let cat = decode(&cat).ok()?.into_owned();
                        let category = data.item_search_categorys.iter().find(|(_id, category)| category.name == cat);
                        category.map(|(id, _)| {
                            let mut items = data.items
                                .iter()
                                .filter(|(_, item)| item.item_search_category == *id)
                                .collect::<Vec<_>>();
                            items.sort_by_key(|(_, item)| Reverse(item.level_item.0));
                            items.into_iter().map(|(id, item)| view!{cx, <div class="flex-row">
                                    <ItemIcon item_id=id.0 icon_size=IconSize::Small />
                                    <a href=format!("/item/North-America/{}", id.0) style="width: 250px">{&item.name}</a>
                                    <span style="color: #f3a; width: 50px">{item.level_item.0}</span>
                                    <CheapestPrice item_id=*id hq=None />
                                </div>
                            })
                            .collect::<Vec<_>>()
                        })

                    }}
                </div>
            </div>
        </div>
    }
}
