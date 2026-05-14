use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
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
        gil::Gil,
        item_icon::ItemIcon,
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
    let i18n = use_i18n();
    let items = Memo::new(move |_| {
        items
            .iter()
            .cloned()
            .enumerate()
            .collect::<Vec<(usize, TrendItem)>>()
    });

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
                each=items.into()
                key=move |(index, item): &(usize, TrendItem)| (*index, item.item_id, item.hq)
                view=move |(index, item): (usize, TrendItem)| {
                    let world = world.clone();
                    let item_id = item.item_id;
                    let item_data = tracked_data().items.get(&xiv_gen::ItemId(item_id));
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
                                    Some(view! { <span class="px-2 py-0.5 rounded-full text-xs font-semibold border text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)]">{t!(i18n, hq)}</span> })
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
    let i18n = use_i18n();
    let params = use_params_map();
    let world = move || params.with(|params| params.get("world").unwrap_or_default());
    let (selected_tab, set_selected_tab) = signal(TrendTab::Velocity);

    let trends = ArcResource::new(world, move |w| async move {
        if w.is_empty() {
            return Ok(None);
        }
        get_trends(&w).await.map(Some)
    });

    view! {
        <MetaTitle title=t_string!(i18n, trends_meta_title).to_string() />
        <MetaDescription text=t_string!(i18n, trends_meta_desc).to_string() />

        <div class="main-content p-6">
            <div class="flex flex-col gap-8">
                <ToolHeader
                    title=t_string!(i18n, market_trends).to_string()
                    summary=t_string!(i18n, trends_tool_summary).to_string()
                    context=t_string!(i18n, trends_tool_context).to_string()
                    help_href="/help/market-trends"
                    help_body=t_string!(i18n, trends_tool_help).to_string()
                />

                // Filter toolbar
                <Toolbar>
                    <ToolbarField label=t_string!(i18n, world).to_string()>
                        <TrendsWorldNavigator />
                    </ToolbarField>
                    <ToolbarField label=t_string!(i18n, filter_category_label).to_string()>
                        <ToolbarPills>
                            <button
                                aria-pressed=move || (selected_tab.get() == TrendTab::Velocity).to_string()
                                on:click=move |_| set_selected_tab.set(TrendTab::Velocity)
                            >
                                {t!(i18n, trends_tab_high_velocity)}
                            </button>
                            <button
                                aria-pressed=move || (selected_tab.get() == TrendTab::Rising).to_string()
                                on:click=move |_| set_selected_tab.set(TrendTab::Rising)
                            >
                                {t!(i18n, trends_tab_rising)}
                            </button>
                            <button
                                aria-pressed=move || (selected_tab.get() == TrendTab::Falling).to_string()
                                on:click=move |_| set_selected_tab.set(TrendTab::Falling)
                            >
                                {t!(i18n, trends_tab_falling)}
                            </button>
                        </ToolbarPills>
                    </ToolbarField>
                </Toolbar>

                // Metric explainers
                <div class="grid grid-cols-1 md:grid-cols-3 gap-3">
                    <MetricExplainer label=t_string!(i18n, trends_tab_high_velocity).to_string() explanation=t_string!(i18n, trends_explanation_high_velocity).to_string() />
                    <MetricExplainer label=t_string!(i18n, trends_tab_rising).to_string() explanation=t_string!(i18n, trends_explanation_rising).to_string() />
                    <MetricExplainer label=t_string!(i18n, trends_tab_falling).to_string() explanation=t_string!(i18n, trends_explanation_falling).to_string() />
                </div>

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
