use super::gil::*;
use super::item_icon::*;
use super::relative_time::RelativeToNow;
use crate::components::icon::Icon;
use crate::global_state::xiv_data::tracked_data;
use chrono::NaiveDateTime;
use icondata as i;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use std::collections::VecDeque;
use xiv_gen::ItemId;

use crate::components::skeleton::BoxSkeleton;
use crate::global_state::home_world::use_home_world;
use crate::i18n::*;
use crate::ws::realtime::{RealtimeSubscription, use_realtime};
use ultros_api_types::websocket::{EventType, FilterPredicate, ServerClient, SocketMessageType};

#[derive(Clone)]
pub(crate) struct SaleView {
    pub(crate) item_id: i32,
    pub(crate) price: i32,
    pub(crate) sold_date: NaiveDateTime,
    pub(crate) hq: bool,
}

#[component]
fn Item(item_id: i32) -> impl IntoView {
    let item = tracked_data().items.get(&ItemId(item_id))?;
    Some(view! {
        <div class="flex items-center">
            <span class="text-[color:var(--color-text)]">{item.name.as_str()}</span>
        </div>
    })
}

#[component]
pub fn LiveSaleTicker() -> impl IntoView {
    let i18n = use_i18n();
    let (done_loading, set_done_loading) = signal(false);
    let sales = RwSignal::<VecDeque<SaleView>>::new(VecDeque::new());
    let (homeworld, _) = use_home_world();
    let retrigger = RwSignal::new(false);
    let live_subscription = StoredValue::new(None::<RealtimeSubscription>);
    let realtime = use_realtime();
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
        let hw_1 = homeworld();
        let hw_2 = homeworld();
        if !retrigger.get() {
            return;
        }
        live_subscription.update_value(|sub| *sub = None);
        if let (Some(sale), Some(realtime)) = (
            hw_1.map(|h| ultros_api_types::world_helper::AnySelector::World(h.id)),
            realtime.clone(),
        ) {
            let sub = realtime.subscribe_market(
                FilterPredicate::World(sale),
                SocketMessageType::Sales,
                move |message| match message {
                    ServerClient::Sales(EventType::Added(add)) => {
                        let _ = sales.try_update(|sales| {
                            for (sale, _) in add.sales {
                                sales.push_front(SaleView {
                                    item_id: sale.sold_item_id,
                                    price: sale.price_per_item,
                                    sold_date: sale.sold_date,
                                    hq: sale.hq,
                                });
                            }
                            use itertools::Itertools;
                            sales
                                .make_contiguous()
                                .sort_by_key(|sale| std::cmp::Reverse(sale.sold_date));
                            *sales = sales
                                .iter()
                                .unique_by(|sale| (sale.item_id, sale.hq))
                                .take(8)
                                .cloned()
                                .collect();
                        });
                    }
                    ServerClient::Stale { .. } => retrigger.set(true),
                    _ => {}
                },
            );
            live_subscription.set_value(Some(sub));
        }
        spawn_local(async move {
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
    on_cleanup(move || {
        live_subscription.update_value(|sub| *sub = None);
    });

    view! {
        <div class="py-2">
            // No homeworld set warning
            <div class="space-y-3" class:hidden=move || homeworld.with(|w| w.is_some())>
                <h3 class="dashboard-section-title">{t!(i18n, live_sale_no_homeworld_title)}</h3>
                <div class="text-sm text-[color:var(--color-text-muted)]">
                    {t!(i18n, live_sale_no_homeworld_prefix)}
                    <A
                        href="/settings"
                        attr:class="text-[color:var(--accent)] hover:underline transition-colors"
                    >
                        {t!(i18n, settings)}
                    </A>
                    {t!(i18n, live_sale_no_homeworld_suffix)}
                </div>
            </div>

            // Sales ticker content — vertical timeline. Each entry gets a
            // glowing dot anchored to a vertical accent line on the left,
            // matching the dashboard mockup.
            <div class="" class:hidden=move || homeworld.with(|w| w.is_none())>
                <div class="flex items-baseline justify-between mb-3">
                    <h3 class="dashboard-section-title">
                        {t!(i18n, live_sale_recent_sales_on)}
                        <span class="text-[color:var(--color-text)] normal-case tracking-normal ml-1">
                            {move || homeworld().map(|world| world.name).unwrap_or_default()}
                        </span>
                    </h3>
                    <button
                        class="text-xs text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)] transition-colors flex items-center gap-1"
                        on:click=move |_| {
                            sales.update(|s| s.clear());
                            set_done_loading(false);
                            retrigger.set(true);
                        }
                    >
                        <Icon icon=i::BiRefreshRegular aria_hidden=true />
                        {t!(i18n, refresh)}
                    </button>
                </div>

                <div class="relative max-h-[480px] overflow-y-auto overflow-x-hidden scrollbar-thin pl-4">
                    // Vertical accent rail running the height of the timeline.
                    <div class="absolute left-1 top-1 bottom-1 w-px bg-[color:color-mix(in_srgb,var(--accent)_45%,transparent)]" aria-hidden="true" />
                    <Show
                        when=done_loading
                        fallback=move || {
                            view! {
                                <div class="h-[300px] animate-pulse">
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
                                <div class="relative pl-4 py-2 group rounded-r hover:bg-[color:color-mix(in_srgb,var(--accent)_6%,transparent)] transition-colors">
                                    // Glowing timeline dot
                                    <span
                                        aria-hidden="true"
                                        class="absolute -left-[3px] top-[1.05rem] w-2 h-2 rounded-full bg-[color:var(--accent)] shadow-[0_0_8px_var(--accent-glow)] group-hover:scale-125 transition-transform"
                                    />
                                    <div class="flex items-center gap-3 w-full">
                                        <ItemIcon item_id=sale.item_id icon_size=IconSize::Small />
                                        <div class="flex flex-col min-w-0 flex-1">
                                            <div class="flex items-center gap-2">
                                                <Item item_id=sale.item_id />
                                                {sale
                                                    .hq
                                                    .then(|| {
                                                        view! {
                                                            <span class="px-1.5 py-0.5 rounded text-[10px] bg-[color:color-mix(in_srgb,var(--brand-ring)_18%,transparent)] text-[color:var(--brand-fg)]">
                                                                "HQ"
                                                            </span>
                                                        }
                                                    })}
                                            </div>
                                            <div class="flex items-center gap-3 text-xs text-[color:var(--color-text-muted)]">
                                                <span class="font-mono"><Gil amount=sale.price /></span>
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
