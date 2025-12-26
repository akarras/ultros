use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use ultros_api_types::icon_size::IconSize;
use ultros_api_types::trends::TrendItem;

use crate::{api::get_trends, components::item_icon::ItemIcon};

#[component]
pub fn Trends() -> impl IntoView {
    let params = use_params_map();
    let world = move || params.with(|params| params.get("world").unwrap_or_default());
    let (selected_tab, set_selected_tab) = signal("velocity".to_string());

    let trends = Resource::new(world, move |w| async move {
        if w.is_empty() {
            return Ok(None);
        }
        get_trends(&w).await.map(Some)
    });

    view! {
        <div class="p-4">
            <h1 class="text-2xl font-bold mb-4">"Market Trends for " {world}</h1>

            <Suspense fallback=move || view! { <div class="loading">"Loading trends..."</div> }>
                {move || match trends.get() {
                    Some(Ok(Some(data))) => view! {
                        <div class="tabs tabs-boxed mb-4">
                            <a class="tab" class:tab-active=move || selected_tab.get() == "velocity" on:click=move |_| set_selected_tab.set("velocity".to_string())>"High Velocity"</a>
                            <a class="tab" class:tab-active=move || selected_tab.get() == "rising" on:click=move |_| set_selected_tab.set("rising".to_string())>"Rising Prices"</a>
                            <a class="tab" class:tab-active=move || selected_tab.get() == "falling" on:click=move |_| set_selected_tab.set("falling".to_string())>"Falling Prices"</a>
                        </div>

                        {move || match selected_tab.get().as_str() {
                            "velocity" => view! { <TrendList items=data.high_velocity.clone() world=world() /> }.into_any(),
                            "rising" => view! { <TrendList items=data.rising_price.clone() world=world() /> }.into_any(),
                            "falling" => view! { <TrendList items=data.falling_price.clone() world=world() /> }.into_any(),
                            _ => view! { <div>"Select a tab"</div> }.into_any(),
                        }}
                    }.into_any(),
                    Some(Ok(None)) => view! { <div>"No data available."</div> }.into_any(),
                    Some(Err(e)) => view! { <div class="text-red-500">{format!("Error loading trends: {}", e)}</div> }.into_any(),
                    None => view! { <div>"Loading..."</div> }.into_any(),
                }}
            </Suspense>
        </div>
    }
}

#[component]
fn TrendList(items: Vec<TrendItem>, world: String) -> impl IntoView {
    view! {
        <div class="overflow-x-auto">
            <table class="table w-full">
                <thead>
                    <tr>
                        <th>"Item"</th>
                        <th>"Price"</th>
                        <th>"Avg Price"</th>
                        <th>"Weekly Sales"</th>
                    </tr>
                </thead>
                <tbody>
                    <For
                        each=move || items.clone()
                        key=|item| (item.item_id, item.hq)
                        children=move |item| {
                            let world_clone = world.clone();
                            view! {
                                <tr>
                                    <td class="flex items-center gap-2">
                                        <ItemIcon item_id=item.item_id icon_size=IconSize::Small />
                                        <a href=format!("/item/{}/{}", world_clone, item.item_id) class="link link-hover">
                                            {
                                                let item_name = if let Some(item_data) = xiv_gen_db::data().items.get(&xiv_gen::ItemId(item.item_id)) {
                                                    item_data.name.clone()
                                                } else {
                                                    "Unknown Item".to_string()
                                                };
                                                item_name
                                            }
                                            {if item.hq { " (HQ)" } else { "" }}
                                        </a>
                                    </td>
                                    <td>{item.price.to_string()} " gil"</td>
                                    <td>{format!("{:.0}", item.average_sale_price)} " gil"</td>
                                    <td>{format!("{:.1}", item.sales_per_week)}</td>
                                </tr>
                            }
                        }
                    />
                </tbody>
            </table>
        </div>
    }
}
