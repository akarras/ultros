use std::borrow::Cow;

use leptos::*;
use xiv_gen::{ClassJobId, ItemSearchCategoryId};

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
            // view! {cx, <i class=format!("icon xiv-ItemCategory_{}", class_job.abbreviation)></i>}
            view! {cx, <ClassJobIcon id=class_job.key_id/>}.into_view(cx)
        } else {
            let value: Cow<str> = match id.0 {
                // singular armory items
                30..=38 | 40..=41 => {
                    format!("icon xiv-Armoury_{}", category.name.replace(" ", "_")).into()
                }
                // plural armoury
                42 | 39 => format!(
                    "icon xiv-Armoury_{}",
                    category.name.remove_last().replace(" ", "_").to_string()
                )
                .into(),
                55 => "icon xiv-ItemCategory_Part".into(),
                44 => "icon xiv-ItemCategory_CUL".into(),
                46 => {
                    // why
                    "icon xiv-ItemCategory_fisher".into()
                }
                47 => "icon xiv-ItemCategory_MIN".into(),
                48 => "icon xiv-ItemCategory_BSM".into(),
                49 => "icon xiv-ItemCategory_BTN".into(),
                50 => "icon xiv-ItemCategory_WVR".into(),
                51 => "icon xiv-ItemCategory_LTW".into(),
                53 => "icon xiv-ItemCategory_ALC".into(),
                // Removes the last character from the item character
                17 | 44..=51 | 53..=55 | 58..=59 | 72 | 75..=78 | 82 => format!(
                    "icon xiv-ItemCategory_{}",
                    category.name.remove_last().replace(" ", "_").to_string()
                )
                .into(),
                79 => "icon xiv-ItemCategory_Airship".into(),
                80 => "icon xiv-ItemCategory_Orchestrion_Roll".into(),
                // removes the other Items- from the thing?
                81 => format!(
                    "icon xiv-ItemCategory_{}",
                    category
                        .name
                        .replace(" Items", "")
                        .replace("-", "")
                        .to_string()
                )
                .into(),
                90 => {
                    // oh yes, category 90 is icon 85?
                    "icon xiv-item_category_085".into()
                }
                _ => format!(
                    "icon xiv-ItemCategory_{}",
                    category.name.replace(" ", "_").replace("-", "")
                )
                .into(),
            };
            view! {cx, <i class=value.as_ref()></i>}.into_view(cx)
        }
    })
}

trait StrExt {
    fn remove_last(&self) -> &str;
}

impl StrExt for str {
    fn remove_last(&self) -> &str {
        match self.char_indices().next_back() {
            Some((i, _)) => &self[..i],
            None => self,
        }
    }
}

#[component]
pub fn ClassJobIcon(cx: Scope, id: ClassJobId) -> impl IntoView {
    view! {cx, <i class=format!("icon xiv-class_job_{:03}", id.0)></i>}
}
