use super::gil::*;
use super::item_icon::*;
use super::relative_time::RelativeToNow;
use chrono::NaiveDateTime;
use icondata as i;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_icons::Icon;
use leptos_router::components::A;
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
        <div class="flex items-center">
            <span class="text-gray-200">{item.name.as_str()}</span>
        </div>
    })
}

#[component]
pub fn LiveSaleTicker() -> impl IntoView {
    let (done_loading, set_done_loading) = signal(false);
    let sales = RwSignal::<VecDeque<SaleView>>::new(VecDeque::new());
    let (homeworld, _) = use_home_world();
    let retrigger = RwSignal::new(false);
    Effect::new(move |_| {
        #[cfg(not(feature = "ssr"))]
        let hw_1 = homeworld();
        #[cfg(not(feature = "ssr"))]
        let hw_2 = homeworld();
        if !retrigger.get() {
            return;
        }
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
        let _retrigger = retrigger.set(false);
    });

    view! {
        <div class="p-6 rounded-xl bg-gradient-to-br from-violet-950/20 to-violet-900/20
        border border-white/10 backdrop-blur-sm">
            // No homeworld set warning
            <div class="space-y-4" class:hidden=move || homeworld.with(|w| w.is_some())>
                <h3 class="text-xl font-bold text-amber-200">"No Homeworld Set"</h3>
                <div class="text-gray-300">
                    "No homeworld is currently set. Go to "
                    <A
                        href="/settings"
                        attr:class="text-amber-200 hover:text-amber-100 transition-colors"
                    >
                        "Settings"
                    </A> " to set your homeworld."
                </div>
            </div>

            // Sales ticker content
            <div class="space-y-4" class:hidden=move || homeworld.with(|w| w.is_none())>
                <div class="flex items-center justify-between">
                    <h3 class="text-xl font-bold text-amber-200">
                        "Recent Sales on "
                        <span class="text-gray-200">
                            {move || homeworld().map(|world| world.name).unwrap_or_default()}
                        </span>
                    </h3>
                    <button
                        class="text-sm text-gray-400 hover:text-amber-200 transition-colors
                        flex items-center gap-2"
                        on:click=move |_| {
                            sales.update(|s| s.clear());
                            set_done_loading(false);
                            retrigger.set(true);
                        }
                    >
                        <Icon icon=i::BiRefreshRegular />
                        "Refresh"
                    </button>
                </div>

                <div class="space-y-2 max-h-[400px] overflow-y-auto overflow-x-hidden
                scrollbar-thin scrollbar-thumb-violet-600/50 scrollbar-track-transparent">
                    <Show
                        when=done_loading
                        fallback=move || {
                            view! {
                                <div class="h-[400px] animate-pulse">
                                    <BoxSkeleton />
                                </div>
                            }
                        }
                    >
                        <For each=sales key=|sale| sale.sold_date let:sale>
                            <A href=move || {
                                format!(
                                    "/item/{}/{}",
                                    homeworld().map(|world| world.name).unwrap_or_default(),
                                    sale.item_id,
                                )
                            }>
                                <div class="flex items-center gap-4 p-3 rounded-lg
                                bg-violet-950/30 border border-white/5
                                hover:bg-violet-900/30 hover:border-white/10
                                transition-all duration-200 group">
                                    <div class="flex items-center gap-4 w-full transform transition-transform duration-200 group-hover:translate-x-1">
                                        <ItemIcon item_id=sale.item_id icon_size=IconSize::Medium />

                                        <div class="flex flex-col min-w-0 flex-1">
                                            <div class="flex items-center gap-2">
                                                <Item item_id=sale.item_id />
                                                {sale
                                                    .hq
                                                    .then(|| {
                                                        view! {
                                                            <span class="px-1.5 py-0.5 rounded text-xs bg-amber-500/20 text-amber-200">
                                                                "HQ"
                                                            </span>
                                                        }
                                                    })}
                                            </div>
                                            <div class="flex items-center gap-4 text-sm text-gray-400">
                                                <Gil amount=sale.price />
                                                <RelativeToNow timestamp=sale.sold_date />
                                            </div>
                                        </div>
                                    </div>
                                </div>
                            </A>
                        </For>
                    </Show>
                </div>
            </div>
        </div>
    }.into_any()
}
