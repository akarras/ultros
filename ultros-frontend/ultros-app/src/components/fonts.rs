use leptos::*;
use xiv_gen::ItemSearchCategoryId;

#[component]
pub fn ItemSearchCategoryIcon(cx: Scope, id: ItemSearchCategoryId) -> impl IntoView {
    // the css names match with the english name of the category
    // if there's a class job, use the abbreviation instead
    let data = &xiv_gen_db::decompress_data();
    let categories = &data.item_search_categorys;
    let class_jobs = &data.class_jobs;
    categories.get(&id).map(|category| {
        let class_job = category.class_job;
        if let Some(class_job) = class_jobs.get(&class_job) {
            view! {cx, <i class=format!("icon xiv-ItemCategory_{}", class_job.abbreviation)></i>}
        } else {
            match id.0 {
                31..=42 => {
                    view! {cx, <i class=format!("icon xiv-Armoury_{}", category.name.replace(" ", "_").replace("", ""))></i>}
                }
                _ => {
                    view! {cx, <i class=format!("icon xiv-ItemCategory_{}", category.name.replace(" ", "_").replace("", ""))></i>}
                }
            }
            
        }
    })
}
