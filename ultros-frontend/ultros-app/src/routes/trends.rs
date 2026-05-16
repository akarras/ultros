use crate::global_state::xiv_data::tracked_data;
use leptos::prelude::*;
use leptos_router::{
    NavigateOptions,
    hooks::{use_navigate, use_params_map},
};
use ultros_api_types::{icon_size::IconSize, trends::TrendItem};

use crate::{
    api::get_trends,
    components::{
        add_to_list::AddToList,
        clipboard::Clipboard,
        confidence_badge::ConfidenceBadge,
        gil::Gil,
        item_icon::ItemIcon,
        market_heat::MarketHeat,
        market_movers::MarketMovers,
        meta::{MetaDescription, MetaTitle},
        skeleton::BoxSkeleton,
        tool_help::*,
        toolbar::{Toolbar, ToolbarField, ToolbarPills},
        virtual_scroller::VirtualScroller,
        world_picker::WorldOnlyPicker,
    },
    global_state::LocalWorldData,
};

#[component]
fn TrendsTable(items: Vec<TrendItem>, world: String) -> impl IntoView {
    let items = Memo::new(move |_| {
        items
            .iter()
            .cloned()
            .enumerate()
            .collect::<Vec<(usize, TrendItem)>>()
    });

    view! {
        <div class="overflow-x-auto content-visible contain-layout contain-paint will-change-scroll forced-layer">
            <VirtualScroller
                viewport_height=720.0
                row_height=40.0
                overscan=8
                header_height=48.0
                variable_height=false
                header=view! {
                    <div class="flex flex-row align-top h-12 border-b border-[color:var(--line)] font-semibold text-[10px] uppercase tracking-[0.14em] text-[color:var(--color-text-muted)]" role="rowgroup">
                        <div role="columnheader" class="w-[40px] px-2 py-3 text-center">
                            "HQ"
                        </div>
                        <div role="columnheader" class="w-84 px-4 py-3">
                            "Item"
                        </div>
                        <div role="columnheader" class="w-32 px-4 py-3 text-right">
                            "Price"
                        </div>
                        <div role="columnheader" class="w-32 px-4 py-3 text-right">
                            "Avg Price"
                        </div>
                        <div role="columnheader" class="w-32 px-4 py-3 text-right">
                            "Sales/Week"
                        </div>
                        <div role="columnheader" class="w-28 px-4 py-3 text-center">
                            "Quality"
                        </div>
                    </div>
                }.into_any()
                each=items.into()
                key=move |(index, item): &(usize, TrendItem)| (*index, item.item_id, item.hq)
                view=move |(index, item): (usize, TrendItem)| {
                    let world = world.clone();
                    let item_id = item.item_id;
                    let item_data = tracked_data().items.get(&xiv_gen::ItemId(item_id));
                    let item_name = item_data.map(|i| i.name.as_str()).unwrap_or("Unknown Item").to_string();
                    let icon_loading = if index < 20 { "eager" } else { "" };

                    // Single hairline divider between rows — no zebra
                    // striping, no panel background, in line with the new
                    // dashboard aesthetic.
                    let classes = "flex flex-row items-center flex-nowrap h-10 border-b border-[color:var(--line)] hover:bg-[color:color-mix(in_srgb,var(--accent)_8%,transparent)] transition-colors";

                    view! {
                        <div class=classes role="row-group">
                            <div role="cell" class="px-2 py-2 w-[40px] flex items-center justify-center">
                                {if item.hq {
                                    Some(view! { <span class="px-2 py-0.5 rounded-full text-xs font-semibold border text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)]">"HQ"</span> })
                                } else {
                                    None
                                }}
                            </div>
                            <div role="cell" class="px-4 py-2 flex flex-row w-84 items-center gap-2">
                                <a
                                    class="flex flex-row items-center gap-2 hover:text-brand-300 transition-colors truncate overflow-x-clip w-full text-[color:var(--color-text)]"
                                    href=format!("/item/{}/{item_id}", world)
                                >
                                    <div class="shrink-0">
                                        <ItemIcon item_id icon_size=IconSize::Small loading=icon_loading />
                                    </div>
                                    {item_name.clone()}
                                </a>
                                <AddToList item_id />
                                <Clipboard clipboard_text=item_name />
                            </div>
                            <div role="cell" class="px-4 py-2 w-32 text-right flex items-center justify-end">
                                <Gil amount=item.price />
                            </div>
                            <div role="cell" class="px-4 py-2 w-32 text-right flex items-center justify-end">
                                <Gil amount=item.average_sale_price as i32 />
                            </div>
                            <div role="cell" class="px-4 py-2 w-32 text-right flex items-center justify-end text-[color:var(--color-text)]">
                                {format!("{:.1}", item.sales_per_week)}
                            </div>
                            <div role="cell" class="px-4 py-2 w-28 flex items-center justify-center">
                                <ConfidenceBadge band=item.confidence_band sample_size=item.sample_size_30d />
                            </div>
                        </div>
                    }.into_any()
                }
            />
        </div>
    }
}

#[component]
fn TrendsWorldNavigator() -> impl IntoView {
    let nav = use_navigate();
    let params = use_params_map();
    let worlds = use_context::<LocalWorldData>()
        .expect("Should always have local world data")
        .0;

    let initial_world = params.with_untracked(|p| {
        let world = p.get_str("world").unwrap_or_default();
        if let Ok(w_data) = &worlds {
            w_data
                .lookup_world_by_name(world)
                .and_then(|w| w.as_world().cloned())
        } else {
            None
        }
    });

    let (current_world, set_current_world) = signal(initial_world);

    Effect::new(move |_| {
        if let Some(world) = current_world() {
            let world = world.name;
            nav(
                &format!("/trends/{world}"),
                NavigateOptions {
                    scroll: false,
                    ..Default::default()
                },
            );
        }
    });

    view! {
        <WorldOnlyPicker
            current_world=current_world.into()
            set_current_world=set_current_world.into()
        />
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TrendTab {
    Velocity,
    Rising,
    Falling,
}

#[component]
pub fn Trends() -> impl IntoView {
    let params = use_params_map();
    let world = move || params.with(|params| params.get("world").unwrap_or_default());
    let (selected_tab, set_selected_tab) = signal(TrendTab::Velocity);

    let trends = ArcResource::new(world, move |w| async move {
        if w.is_empty() {
            return Ok(None);
        }
        get_trends(&w).await.map(Some)
    });

    // For MarketHeat / MarketMovers signal compat — they take an
    // Option<String> so they can no-op when no world is selected.
    let world_signal: Signal<Option<String>> = Signal::derive(move || {
        let w = world();
        if w.is_empty() { None } else { Some(w) }
    });

    view! {
        <MetaTitle title="Market Trends - Ultros" />
        <MetaDescription text="View market trends for Final Fantasy 14 items, including high velocity, rising prices, and falling prices." />

        <div class="main-content p-6">
            <div class="flex flex-col gap-6 max-w-7xl mx-auto">
                <ToolHeader
                    title="Market Trends"
                    summary="Review high-velocity, rising-price, and falling-price items for the selected world."
                    context="Trends are directional signals. Open an item page before treating a movement as a buying decision."
                    help_href="/help/market-trends"
                    help_body="Market Trends groups items by recent sales and price movement. High velocity is best for demand checks; rising and falling prices are prompts to investigate item history."
                />

                // Filter toolbar
                <Toolbar>
                    <ToolbarField label="World">
                        <TrendsWorldNavigator />
                    </ToolbarField>
                    <ToolbarField label="Category">
                        <ToolbarPills>
                            <button
                                aria-pressed=move || (selected_tab.get() == TrendTab::Velocity).to_string()
                                on:click=move |_| set_selected_tab.set(TrendTab::Velocity)
                            >
                                "High Velocity"
                            </button>
                            <button
                                aria-pressed=move || (selected_tab.get() == TrendTab::Rising).to_string()
                                on:click=move |_| set_selected_tab.set(TrendTab::Rising)
                            >
                                "Rising Prices"
                            </button>
                            <button
                                aria-pressed=move || (selected_tab.get() == TrendTab::Falling).to_string()
                                on:click=move |_| set_selected_tab.set(TrendTab::Falling)
                            >
                                "Falling Prices"
                            </button>
                        </ToolbarPills>
                    </ToolbarField>
                </Toolbar>

                // Market Heat band (gated on a selected world). Gives a
                // quick read on category-level sentiment before the detail
                // table.
                {move || world_signal.with(|w| w.is_some()).then(|| view! {
                    <MarketHeat world=world_signal />
                })}

                // Market Movers — wider view than the home page rail. The
                // tabbed component handles its own rising/falling/volume
                // bucket so it complements (not duplicates) the
                // TrendTab filter below.
                {move || world_signal.with(|w| w.is_some()).then(|| view! {
                    <MarketMovers world=world_signal />
                })}

                // Trend detail table. The MarketMovers strip above gives
                // the at-a-glance picture; this is the deep-dive surface
                // with full price, sales/week, and confidence columns.
                <section class="dashboard-section">
                    <h2 class="dashboard-section-title mb-2">
                        {move || match selected_tab.get() {
                            TrendTab::Velocity => "High Velocity",
                            TrendTab::Rising => "Rising Prices",
                            TrendTab::Falling => "Falling Prices",
                        }}
                    </h2>
                </section>

                // Content
                <div class="min-h-[500px]">
                    <Suspense fallback=BoxSkeleton>
                        {move || match trends.get() {
                            Some(Ok(Some(data))) => {
                                let items = match selected_tab.get() {
                                    TrendTab::Velocity => data.high_velocity,
                                    TrendTab::Rising => data.rising_price,
                                    TrendTab::Falling => data.falling_price,
                                };

                                if items.is_empty() {
                                    view! {
                                        <div class="text-xl text-[color:var(--color-text)] text-center p-8 bg-brand-900/20 rounded-2xl border border-white/10">
                                            "No trends data available for this category."
                                        </div>
                                    }.into_any()
                                } else {
                                    view! { <TrendsTable items=items world=world() /> }.into_any()
                                }
                            },
                            Some(Ok(None)) => view! {
                                <div class="text-xl text-[color:var(--color-text)] text-center p-8 bg-brand-900/20 rounded-2xl border border-white/10">
                                    "Please select a valid world."
                                </div>
                            }.into_any(),
                            Some(Err(e)) => view! {
                                <div class="text-xl text-red-400 text-center p-8 bg-red-950/20 rounded-2xl border border-red-500/30">
                                    {format!("Error loading trends: {}", e)}
                                </div>
                            }.into_any(),
                            None => view! { <BoxSkeleton /> }.into_any(),
                        }}
                    </Suspense>
                </div>
            </div>
        </div>
    }
}
