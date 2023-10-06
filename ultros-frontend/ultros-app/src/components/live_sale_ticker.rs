use super::gil::*;
use super::item_icon::*;
use super::loading::*;
use super::relative_time::RelativeToNow;
use chrono::NaiveDateTime;
use leptos::*;
use leptos_router::*;
use std::collections::VecDeque;
use xiv_gen::ItemId;

use crate::global_state::home_world::get_homeworld;
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
            <ItemIcon item_id icon_size=IconSize::Small />
            <div class="flex flex-row">{&item.name}</div>
        </div>
    })
}

#[component]
pub fn LiveSaleTicker() -> impl IntoView {
    let sales = create_rw_signal::<VecDeque<SaleView>>(VecDeque::new());
    let (homeworld, _) = get_homeworld();
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
                log::info!("live sale");
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
                }
            }
        });
    });

    view! {
        <Suspense fallback=move || view!{<Loading />}>
            {move || {
                if homeworld().is_none() {
                    view!{
                        <div class="flex flex-col">
                            <h3 class="text-xl">"No homeworld set"</h3>
                            <div>
                                "No homeworld is set currently. Go to "<A href="/settings">"Settings"</A>" to set your homeworld."
                            </div>
                        </div>
                    }.into_view()
                } else {
                    view!{
                        <div class="flex flex-col">
                            <h3 class="text-xl">{move || format!("recent sales on {}", homeworld().map(|world| world.name).unwrap_or_default())}</h3>
                            <div>
                                <For each=sales
                                    key=|sale| sale.sold_date
                                    let:sale>
                                    <A href=move || format!("/item/{}/{}", homeworld().map(|world| world.name).unwrap_or_default(), sale.item_id)>
                                        <div class="flex flex-col gap-1 whitespace-nowrap text-white bg-neutral-950 hover:bg-neutral-800 transition-colors">
                                            <div class="flex flex-row">
                                                <Item item_id=sale.item_id />
                                            </div>
                                            <div class="flex flex-row gap-5">
                                                {sale.hq.then(|| "HQ")}
                                                <Gil amount=sale.price />
                                                <RelativeToNow timestamp=sale.sold_date />
                                            </div>
                                        </div>
                                    </A>
                                </For>
                        </div>
                    </div>}.into_view()
                }

            }}

    </Suspense>
    }
}
