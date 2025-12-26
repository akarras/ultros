use super::gil::*;
use super::item_icon::*;
use super::relative_time::RelativeToNow;
use crate::components::icon::Icon;
use chrono::NaiveDateTime;
use icondata as i;
use leptos::prelude::*;
use leptos::task::spawn_local;
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
            <span class="text-[color:var(--color-text)]">{item.name.as_str()}</span>
        </div>
    })
}

#[component]
pub fn LiveSaleTicker() -> impl IntoView {
    let (done_loading, set_done_loading) = signal(false);
    let sales = RwSignal::<VecDeque<SaleView>>::new(VecDeque::new());
    let (homeworld, _) = use_home_world();
    let retrigger = RwSignal::new(false);
    // auto-trigger initial load and refresh on homeworld changes
    Effect::new({
        move |_| {
            let hw = homeworld();
            if hw.is_some() {
                sales.update(|s| s.clear());
                set_done_loading(false);
                retrigger.set(true);
            }
        }
    });
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
            #[allow(clippy::collapsible_if)]
            if let Some(world) = hw_2.map(|h| h.name) {
                #[allow(clippy::collapsible_if)]
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
        retrigger.set(false);
    });

    view! {
        <div class="p-6 rounded-xl panel">
            // No homeworld set warning
            <div class="space-y-4" class:hidden=move || homeworld.with(|w| w.is_some())>
                <h3 class="text-xl font-bold text-[color:var(--color-text)]">"No Homeworld Set"</h3>
                <div class="text-[color:var(--color-text-muted)]">
                    "No homeworld is currently set. Go to "
                    <A
                        href="/settings"
                        attr:class="text-[color:var(--brand-fg)] hover:underline transition-colors"
                    >
                        "Settings"
                    </A> " to set your homeworld."
                </div>
            </div>

            // Sales ticker content
            <div class="space-y-4" class:hidden=move || homeworld.with(|w| w.is_none())>
                <div class="flex items-center justify-between">
                    <h3 class="text-xl font-bold text-[color:var(--color-text)]">
                        "Recent Sales on "
                        <span class="text-[color:var(--color-text)]">
                            {move || homeworld().map(|world| world.name).unwrap_or_default()}
                        </span>
                    </h3>
                    <button
                        class="text-sm text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)] transition-colors
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
                scrollbar-thin">
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
                                <div class="card p-3 transition-colors duration-200 group">
                                    <div class="flex items-center gap-4 w-full transform transition-transform duration-200 group-hover:translate-x-1">
                                        <ItemIcon item_id=sale.item_id icon_size=IconSize::Medium />

                                        <div class="flex flex-col min-w-0 flex-1">
                                            <div class="flex items-center gap-2">
                                                <Item item_id=sale.item_id />
                                                {sale
                                                    .hq
                                                    .then(|| {
                                                        view! {
                                                            <span class="px-1.5 py-0.5 rounded text-xs bg-[color:color-mix(in_srgb,var(--brand-ring)_18%,transparent)] text-[color:var(--brand-fg)]">
                                                                "HQ"
                                                            </span>
                                                        }
                                                    })}
                                            </div>
                                            <div class="flex items-center gap-4 text-sm text-[color:var(--color-text-muted)]">
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
