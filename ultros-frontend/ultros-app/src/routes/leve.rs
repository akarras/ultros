use crate::{
    components::{
        gil::*, item_icon::*, meta::*,
    },
    global_state::home_world::use_home_world,
};
use leptos::prelude::*;
use leptos_router::{hooks::use_params_map, components::A};
use xiv_gen::{LeveId};

#[component]
pub fn LeveView() -> impl IntoView {
    let params = use_params_map();
    let id = move || {
        params
            .with(|p| p.get("id").clone())
            .and_then(|id| id.parse::<i32>().ok())
            .unwrap_or(0)
    };

    let data = xiv_gen_db::data();
    let leves = &data.leves;
    let craft_leves = &data.craft_leves;
    let items = &data.items;

    // Find Leve
    let leve = Memo::new(move |_| {
        let leve_id = LeveId(id());
        leves.get(&leve_id)
    });

    // Find associated CraftLeve (if any)
    let craft_leve = Memo::new(move |_| {
        let l_id = LeveId(id());
        craft_leves.values().find(|cl| cl.leve == l_id)
    });

    let turn_in_items = Memo::new(move |_| {
        if let Some(cl) = craft_leve.get() {
            let mut items_list = Vec::new();
            let list = [
                (cl.item_0, cl.item_count_0),
                (cl.item_1, cl.item_count_1),
                (cl.item_2, cl.item_count_2),
                (cl.item_3, cl.item_count_3),
            ];
            for (item_id, count) in list {
                if item_id.0 > 0 && count > 0 {
                    if let Some(item) = items.get(&item_id) {
                        items_list.push((item, count));
                    }
                }
            }
            items_list
        } else {
            Vec::new()
        }
    });

    let (home_world, _) = use_home_world();

    view! {
        <div class="flex flex-col gap-4">
            {move || {
                if let Some(leve) = leve.get() {
                    let name = leve.name.as_str();
                    let description = leve.description.as_str();
                    let level = leve.class_job_level;
                    let job_category = data.class_job_categorys.get(&leve.class_job_category).map(|c| c.name.as_str()).unwrap_or("");

                    let gil_reward = leve.gil_reward;
                    let xp_reward = leve.exp_reward;

                    view! {
                        <MetaTitle title=format!("{} - Levequest", name) />
                        <MetaDescription text=format!("Levequest details for {}: {}", name, description) />

                        <div class="panel p-4 flex flex-col gap-4">
                            <div class="flex flex-row gap-4 items-center border-b border-white/10 pb-4">
                                <div class="w-16 h-16 bg-white/5 rounded-lg flex items-center justify-center text-3xl">
                                    <span class="icon-[fa6-solid--book-open] text-brand-300"></span>
                                </div>
                                <div class="flex flex-col">
                                    <h1 class="text-2xl font-bold">{name}</h1>
                                    <span class="text-[color:var(--color-text-muted)]">{job_category} " Lv. " {level}</span>
                                </div>
                            </div>

                            <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                                <div class="flex flex-col gap-2">
                                    <h2 class="font-bold text-lg">"Details"</h2>
                                     <div class="flex flex-row gap-2">
                                        <span class="text-[color:var(--color-text-muted)]">"Description:"</span>
                                        <p>{description}</p>
                                    </div>
                                </div>

                                <div class="flex flex-col gap-2">
                                    <h2 class="font-bold text-lg">"Rewards"</h2>
                                    <div class="flex flex-row gap-2 items-center">
                                        <span class="text-[color:var(--color-text-muted)]">"Gil:"</span>
                                        <Gil amount=gil_reward as i32 />
                                    </div>
                                    <div class="flex flex-row gap-2">
                                        <span class="text-[color:var(--color-text-muted)]">"XP:"</span>
                                        <span>{xp_reward}</span>
                                    </div>
                                </div>
                            </div>

                            <Show when=move || turn_in_items.with(|l| !l.is_empty())>
                                <div class="flex flex-col gap-2 pt-4 border-t border-white/10">
                                    <h2 class="font-bold text-lg">"Turn-in Items"</h2>
                                    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-2">
                                        <For
                                            each=move || turn_in_items.get()
                                            key=move |(item, _)| item.key_id
                                            children=move |(item, count)| {
                                                let item_id = item.key_id.0;
                                                let name = item.name.to_string();
                                                let world = home_world.get().map(|w| w.name).unwrap_or("North-America".to_string());
                                                view! {
                                                    <A href=format!("/item/{}/{}", world, item_id) attr:class="flex flex-row items-center gap-2 p-2 rounded hover:bg-white/5 transition-colors">
                                                        <ItemIcon item_id=item_id icon_size=IconSize::Small />
                                                        <div class="flex flex-col">
                                                            <span class="font-medium">{name}</span>
                                                            <span class="text-xs text-[color:var(--color-text-muted)]">"x" {count}</span>
                                                        </div>
                                                    </A>
                                                }
                                            }
                                        />
                                    </div>
                                </div>
                            </Show>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <div class="p-8 text-center text-[color:var(--color-text-muted)]">
                            "Levequest not found."
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}
