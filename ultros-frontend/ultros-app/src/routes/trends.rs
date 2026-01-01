use leptos::prelude::*;
use leptos_router::{
    NavigateOptions,
    hooks::{use_navigate, use_params_map},
};
use std::sync::Arc;
use ultros_api_types::{icon_size::IconSize, trends::TrendItem};

use crate::{
    api::get_trends,
    components::{
        add_to_list::AddToList,
        clipboard::Clipboard,
        gil::Gil,
        item_icon::ItemIcon,
        meta::{MetaDescription, MetaTitle},
        skeleton::BoxSkeleton,
        virtual_scroller::VirtualScroller,
        world_picker::WorldOnlyPicker,
    },
    global_state::LocalWorldData,
};

#[component]
fn TrendsTable(
    #[prop(into)] items: Signal<Vec<TrendItem>>,
    #[prop(into)] world: Signal<String>,
) -> impl IntoView {
    let items_with_index = Memo::new(move |_| {
        items.with(|items| {
            items
                .iter()
                .copied()
                .enumerate()
                .collect::<Vec<(usize, TrendItem)>>()
        })
    });

    let world_arc = Memo::new(move |_| Arc::new(world.get()));

    view! {
        <div class="rounded-2xl overflow-x-auto panel content-visible contain-layout contain-paint will-change-scroll forced-layer">
            <VirtualScroller
                viewport_height=720.0
                row_height=40.0
                overscan=8
                header_height=48.0
                variable_height=false
                header=view! {
                    <div class="flex flex-row align-top h-12 bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)] font-semibold text-[color:var(--brand-fg)]" role="rowgroup">
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
                    </div>
                }.into_any()
                each=items_with_index.into()
                key=move |(index, item): &(usize, TrendItem)| (*index, item.item_id, item.hq)
                view=move |(index, item): (usize, TrendItem)| {
                    let world = world_arc.get();
                    let item_id = item.item_id;
                    let item_data = xiv_gen_db::data().items.get(&xiv_gen::ItemId(item_id));
                    let item_name = item_data.map(|i| i.name.as_str()).unwrap_or("Unknown Item").to_string();
                    let icon_loading = if index < 20 { "eager" } else { "" };

                    let classes = if (index % 2) == 0 {
                        "flex flex-row items-center flex-nowrap h-10 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_6%,transparent)] transition-colors"
                    } else {
                        "flex flex-row items-center flex-nowrap h-10 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_8%,transparent)] transition-colors"
                    };

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
            nav(&format!("/trends/{world}"), NavigateOptions::default());
        }
    });

    view! {
        <div class="flex flex-col md:flex-row items-center gap-2">
            <label class="text-[color:var(--brand-fg)] font-semibold">"Select World:"</label>
            <div class="w-full md:w-auto min-w-[200px]">
                <WorldOnlyPicker
                    current_world=current_world.into()
                    set_current_world=set_current_world.into()
                />
            </div>
        </div>
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

    let trends = Resource::new(world, move |w| async move {
        if w.is_empty() {
            return Ok(None);
        }
        get_trends(&w).await.map(Some)
    });

    view! {
        <MetaTitle title="Market Trends - Ultros" />
        <MetaDescription text="View market trends for Final Fantasy 14 items, including high velocity, rising prices, and falling prices." />

        <div class="main-content p-6">
            <div class="container mx-auto max-w-7xl">
                <div class="flex flex-col gap-8">
                    // Header Section
                    <div class="panel p-8 rounded-2xl">
                        <div class="flex flex-col md:flex-row justify-between items-start md:items-center gap-4 mb-4">
                            <div>
                                <h1 class="text-3xl font-bold text-[color:var(--brand-fg)] mb-2">
                                    "Market Trends"
                                </h1>
                                <p class="text-lg text-[color:var(--color-text)]/90">
                                    "Analyze item price movements and sales velocity for " <span class="font-semibold text-brand-300">{world}</span>
                                </p>
                            </div>
                            <TrendsWorldNavigator />
                        </div>

                        // Tab Selection
                        <div class="flex flex-wrap gap-2 mt-6">
                            <button
                                class=move || if selected_tab.get() == TrendTab::Velocity {
                                    "btn bg-brand-600 hover:bg-brand-500 text-white border-none"
                                } else {
                                    "btn btn-outline text-[color:var(--color-text)] hover:bg-[color:var(--brand-ring)] hover:border-transparent"
                                }
                                on:click=move |_| set_selected_tab.set(TrendTab::Velocity)
                            >
                                "High Velocity"
                            </button>
                            <button
                                class=move || if selected_tab.get() == TrendTab::Rising {
                                    "btn bg-brand-600 hover:bg-brand-500 text-white border-none"
                                } else {
                                    "btn btn-outline text-[color:var(--color-text)] hover:bg-[color:var(--brand-ring)] hover:border-transparent"
                                }
                                on:click=move |_| set_selected_tab.set(TrendTab::Rising)
                            >
                                "Rising Prices"
                            </button>
                            <button
                                class=move || if selected_tab.get() == TrendTab::Falling {
                                    "btn bg-brand-600 hover:bg-brand-500 text-white border-none"
                                } else {
                                    "btn btn-outline text-[color:var(--color-text)] hover:bg-[color:var(--brand-ring)] hover:border-transparent"
                                }
                                on:click=move |_| set_selected_tab.set(TrendTab::Falling)
                            >
                                "Falling Prices"
                            </button>
                        </div>
                    </div>

                    // Content
                    <div class="min-h-[500px]">
                        <Suspense fallback=BoxSkeleton>
                            {move || match trends.get() {
                                Some(Ok(Some(data))) => {
                                    // Use a memo to derive items based on selected_tab, so we don't re-create the table component
                                    let items = Memo::new(move |_| {
                                        match selected_tab.get() {
                                            TrendTab::Velocity => data.high_velocity.clone(),
                                            TrendTab::Rising => data.rising_price.clone(),
                                            TrendTab::Falling => data.falling_price.clone(),
                                        }
                                    });

                                    // Check if current list is empty
                                    let is_empty = move || items.with(|i| i.is_empty());

                                    view! {
                                        <Show when=move || !is_empty() fallback=|| view! {
                                            <div class="text-xl text-[color:var(--color-text)] text-center p-8 bg-brand-900/20 rounded-2xl border border-white/10">
                                                "No trends data available for this category."
                                            </div>
                                        }>
                                            <TrendsTable items=items world=Signal::derive(move || world()) />
                                        </Show>
                                    }.into_any()
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
        </div>
    }
}
