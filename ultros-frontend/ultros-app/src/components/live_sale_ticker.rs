use super::gil::*;
use super::item_icon::*;
use super::relative_time::RelativeToNow;
use chrono::NaiveDateTime;
use leptos::*;
use leptos_router::*;
use std::collections::VecDeque;
use xiv_gen::ItemId;

use crate::components::skeleton::BoxSkeleton;
use crate::global_state::home_world::use_home_world;
#[cfg(not(feature = "ssr"))]
use crate::ws::live_data::live_sales;

#[derive(Clone)]
pub(crate) struct SaleView {
    pub(crate) item_id: i32,
    pub(crate) price: i32,
    pub(crate) sold_date: NaiveDateTime,
    pub(crate) hq: bool,
}

#[component]
fn Item(item_id: i32) -> impl IntoView {
    let item = xiv_gen_db::data().items.get(&ItemId(item_id))?;
    Some(view! {
        <div class="flex flex-row">

            <div class="flex flex-row">{&item.name}</div>
        </div>
    })
}

#[component]
pub fn LiveSaleTicker() -> impl IntoView {
    let (done_loading, set_done_loading) = create_signal(false);
    let sales = create_rw_signal::<VecDeque<SaleView>>(VecDeque::new());
    let (homeworld, _) = use_home_world();
    create_effect(move |_| {
        #[cfg(not(feature = "ssr"))]
        let hw_1 = homeworld();
        #[cfg(not(feature = "ssr"))]
        let hw_2 = homeworld();
        spawn_local(async move {
            #[cfg(not(feature = "ssr"))]
            if let Some(sale) =
                hw_1.map(|h| ultros_api_types::world_helper::AnySelector::World(h.id))
            {
                live_sales(sales, sale).await.unwrap();
            }
        });
        spawn_local(async move {
            #[cfg(not(feature = "ssr"))]
            if let Some(world) = hw_2.map(|h| h.name) {
                if let Ok(recent_sales) = crate::api::get_recent_sales_for_world(&world).await {
                    use itertools::Itertools;
                    let first_sales = recent_sales
                        .sales
                        .into_iter()
                        .flat_map(|sale| {
                            sale.sales.first().map(|sale_data| SaleView {
                                item_id: sale.item_id,
                                price: sale_data.price_per_unit,
                                sold_date: sale_data.sale_date,
                                hq: sale.hq,
                            })
                        })
                        .sorted_by_key(|s| std::cmp::Reverse(s.sold_date))
                        .take(8);

                    sales.update(|s| {
                        for sale in first_sales {
                            s.push_back(sale);
                        }
                    });
                    set_done_loading(true);
                }
            }
            set_done_loading(true);
        });
    });

    view! {
        <div class="flex flex-col" class:hidden=move || homeworld.with(|w| w.is_some())>
            <h3 class="text-xl">"No homeworld set"</h3>
            <div>
                "No homeworld is set currently. Go to "<A href="/settings">"Settings"</A>" to set your homeworld."
            </div>
        </div>
        <div class="flex flex-col" class:hidden=move || homeworld.with(|w| w.is_none())>
            <h3 class="text-xl">"recent sales on "{move || homeworld().map(|world| world.name).unwrap_or_default()}</h3>
            <div class="gap-1">
                <Show when=done_loading fallback=move || view!{ <div class="h-[416px]"><BoxSkeleton/></div> } >
                    <For each=sales
                        key=|sale| sale.sold_date
                        let:sale>
                        <A href=move || format!("/item/{}/{}", homeworld().map(|world| world.name).unwrap_or_default(), sale.item_id)>
                            <div class="flex flex-row gap-1 p-1 whitespace-nowrap text-white bg-neutral-950 hover:bg-neutral-800 transition-colors">
                                <ItemIcon item_id=sale.item_id icon_size=IconSize::Medium />
                                <div class="flex flex-col">
                                    <div class="flex flex-row gap-5">
                                        <Item item_id=sale.item_id />
                                        {sale.hq.then(|| "HQ")}
                                    </div>
                                    <div class="flex flex-row gap-5 text-sm">
                                        <Gil amount=sale.price />
                                        <RelativeToNow timestamp=sale.sold_date />
                                    </div>
                                </div>
                            </div>
                        </A>
                    </For>
                </Show>
            </div>
        </div>
    }
}
