use crate::components::meta::{MetaDescription, MetaTitle};
use crate::{
    api::get_cheapest_listings,
    components::{
        gil::*, item_icon::*, query_button::QueryButton, skeleton::BoxSkeleton,
        virtual_scroller::*, world_picker::WorldOnlyPicker,
    },
    global_state::{LocalWorldData, home_world::use_home_world},
};
use itertools::Itertools;
use leptos::{either::Either, prelude::*};
use leptos_router::{
    NavigateOptions,
    hooks::{query_signal, use_location, use_navigate, use_query_map},
};
use std::{cmp::Reverse, collections::HashSet, sync::Arc};
use ultros_api_types::{
    cheapest_listings::{CheapestListings, CheapestListingsMap},
    world_helper::AnyResult,
};

#[derive(Clone, Debug, PartialEq)]
struct VentureProfitData {
    task_level: i32,
    item_id: i32,
    quantity: i32,
    market_price: i32,
    profit: i32,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum SortMode {
    Profit,
    Level,
}

impl std::str::FromStr for SortMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "profit" => Ok(SortMode::Profit),
            "level" => Ok(SortMode::Level),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for SortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let val = match self {
            SortMode::Profit => "profit",
            SortMode::Level => "level",
        };
        f.write_str(val)
    }
}

#[component]
fn VentureAnalyzerTable(
    global_cheapest_listings: CheapestListings,
    world: Signal<String>,
) -> impl IntoView {
    let prices = CheapestListingsMap::from(global_cheapest_listings);
    let data = xiv_gen_db::data();
    let items = &data.items;
    let retainer_tasks = &data.retainer_tasks;
    let retainer_task_normals = &data.retainer_task_normals;

    let (sort_mode, _set_sort_mode) = query_signal::<SortMode>("sort");
    let (minimum_profit, set_minimum_profit) = query_signal::<i32>("profit");
    let query = use_query_map();
    let location = use_location();
    let nav = use_navigate();

    let categories = Memo::new(move |_| {
        retainer_tasks
            .values()
            .filter(|t| !t.is_random)
            .map(|t| t.class_job_category)
            .unique()
            .filter_map(|id| {
                data.class_job_categorys
                    .get(&id)
                    .map(|c| (id, c.name.as_str().to_string()))
            })
            .sorted_by(|a, b| a.1.cmp(&b.1))
            .collect::<Vec<_>>()
    });

    let selected_jobs_set = Memo::new(move |_| {
        query.with(|q| {
            q.get("jobs")
                .map(|s| s.split(',').map(|s| s.to_string()).collect::<HashSet<_>>())
                .unwrap_or_default()
        })
    });

    let toggle_job = move |job_name: String| {
        let mut current = selected_jobs_set.get();
        if current.contains(&job_name) {
            current.remove(&job_name);
        } else {
            current.insert(job_name);
        }

        let mut q = query.get_untracked();
        if current.is_empty() {
            q.remove("jobs");
        } else {
            q.insert("jobs".to_string(), current.into_iter().join(","));
        }

        let qs = q.to_query_string();
        nav(
            &format!("{}{}", location.pathname.get(), qs),
            NavigateOptions::default(),
        );
    };

    let selected_category_ids = Memo::new(move |_| {
        let selected_names = selected_jobs_set.get();
        if selected_names.is_empty() {
            return None;
        }
        let ids: HashSet<_> = categories
            .get()
            .iter()
            .filter(|(_, name)| selected_names.contains(name))
            .map(|(id, _)| *id)
            .collect();
        Some(ids)
    });

    let computed_data = Memo::new(move |_| {
        let mut results = Vec::new();
        let selected_ids = selected_category_ids.get();

        // Iterate over RetainerTasks to find normal ventures
        for (_task_id, task) in retainer_tasks.iter() {
            if task.is_random {
                continue;
            }

            #[allow(clippy::collapsible_if)]
            if let Some(ids) = &selected_ids {
                if !ids.contains(&task.class_job_category) {
                    continue;
                }
            }

            // Check if `task.task` (RowId) corresponds to a RetainerTaskNormal
            // We need to cast RowId to RetainerTaskNormalId?
            // Since RowId is just u16 wrapper, and RetainerTaskNormalId is i32 wrapper.
            let normal_id = xiv_gen::RetainerTaskNormalId(task.task.0 as i32);

            if let Some(normal_task) = retainer_task_normals.get(&normal_id) {
                let item_id = normal_task.item;
                if item_id.0 == 0 {
                    continue;
                }

                let quantity = normal_task.quantity_0 as i32; // taking base quantity
                if quantity == 0 {
                    continue;
                }

                let task_level = task.retainer_level as i32;

                // Market Price
                let market_price_summary = prices.find_matching_listings(item_id.0);
                let market_price = market_price_summary.lowest_gil().unwrap_or(0);

                if market_price == 0 {
                    continue;
                }

                let venture_cost_gil = 0; // Placeholder

                let revenue = market_price * quantity;
                let profit = revenue - venture_cost_gil;

                if let Some(min) = minimum_profit()
                    && profit < min
                {
                    continue;
                }

                results.push(VentureProfitData {
                    task_level,
                    item_id: item_id.0,
                    quantity,
                    market_price,
                    profit,
                });
            }
        }

        // Sort
        match sort_mode().unwrap_or(SortMode::Profit) {
            SortMode::Profit => results.sort_by_key(|d| Reverse(d.profit)),
            SortMode::Level => results.sort_by_key(|d| Reverse(d.task_level)),
        }

        results
            .into_iter()
            .take(100)
            .map(Arc::new)
            .enumerate()
            .collect::<Vec<_>>()
    });

    view! {
        <div class="flex flex-col gap-6">
            <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
                <div class="panel p-6 flex flex-col w-full bg-[color:var(--color-background-elevated)] bg-opacity-100 z-20">
                    <h3 class="font-bold text-xl mb-2 text-[color:var(--brand-fg)]">"Filter by Job"</h3>
                    <div class="flex flex-wrap gap-2">
                        {move || {
                            let selected = selected_jobs_set.get();
                            categories
                                .get()
                                .into_iter()
                                .map(|(_id, name)| {
                                    let is_selected = selected.contains(&name);
                                    let name_clone = name.clone();
                                    let toggle_job = toggle_job.clone();
                                    view! {
                                        <button
                                            class=move || {
                                                if is_selected {
                                                    "px-3 py-1 rounded-full text-xs font-bold bg-brand-600 text-white transition-colors border border-brand-500"
                                                } else {
                                                    "px-3 py-1 rounded-full text-xs font-bold bg-[color:var(--color-base)] hover:bg-[color:var(--brand-ring)]/20 text-[color:var(--color-text)] transition-colors border border-[color:var(--color-outline)]"
                                                }
                                            }
                                            on:click=move |_| toggle_job(name_clone.clone())
                                        >
                                            {name}
                                        </button>
                                    }
                                })
                                .collect_view()
                        }}
                    </div>
                </div>
                <div class="panel p-6 flex flex-col w-full bg-[color:var(--color-background-elevated)] bg-opacity-100 z-20">
                    <h3 class="font-bold text-xl mb-2 text-[color:var(--brand-fg)]">"Minimum Profit"</h3>
                    <p class="mb-4 text-[color:var(--color-text-muted)]">"Set the minimum profit margin"</p>
                    <div class="flex flex-col gap-2">
                        <div class="text-brand-300">
                            {move || {
                                minimum_profit()
                                    .map(|profit| Either::Left(view! { <Gil amount=profit /> }))
                                    .unwrap_or(Either::Right("---"))
                            }}
                        </div>
                        <input
                            class="input"
                            min=0
                            step=1000
                            type="number"
                            prop:value=minimum_profit
                            on:input=move |input| {
                                let value = event_target_value(&input);
                                if let Ok(profit) = value.parse::<i32>() {
                                    set_minimum_profit(Some(profit))
                                } else if value.is_empty() {
                                    set_minimum_profit(None);
                                }
                            }
                        />
                    </div>
                </div>
            </div>

            <div class="rounded-2xl overflow-x-auto panel content-visible contain-layout contain-paint will-change-scroll forced-layer">
                <VirtualScroller
                    viewport_height=720.0
                    row_height=60.0
                    overscan=8
                    header_height=64.0
                    variable_height=false
                    header=view! {
                        <div class="flex flex-row align-top h-16 bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]" role="rowgroup">
                             <div role="columnheader" class="w-84 p-4">"Venture / Item"</div>
                             <div role="columnheader" class="w-30 p-4">
                                <QueryButton
                                    class="!text-brand-300 hover:text-brand-200"
                                    active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                    key="sort"
                                    value="profit"
                                >
                                    "Profit"
                                </QueryButton>
                             </div>
                             <div role="columnheader" class="w-30 p-4">"Unit Price"</div>
                             <div role="columnheader" class="w-30 p-4 hidden md:block">
                                <QueryButton
                                    class="!text-brand-300 hover:text-brand-200"
                                    active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                    key="sort"
                                    value="level"
                                >
                                    "Level"
                                </QueryButton>
                             </div>
                        </div>
                    }.into_any()
                    each=computed_data.into()
                    key=move |(index, data): &(usize, Arc<VentureProfitData>)| (*index, data.item_id)
                    view=move |(index, data): (usize, Arc<VentureProfitData>)| {
                        let item_id = data.item_id;
                        let item = items.get(&xiv_gen::ItemId(item_id)).map(|i| i.name.as_str()).unwrap_or("Unknown");

                        let classes = if (index % 2) == 0 {
                            "flex flex-row items-center flex-nowrap h-15 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_6%,transparent)] transition-colors"
                        } else {
                            "flex flex-row items-center flex-nowrap h-15 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_8%,transparent)] transition-colors"
                        };

                        view! {
                            <div class=classes role="row-group">
                                <div role="cell" class="px-4 py-2 flex flex-row w-84 items-center gap-2">
                                     <a
                                        class="flex flex-row items-center gap-2 hover:text-brand-300 transition-colors truncate overflow-x-clip w-full"
                                        href=format!("/item/{}/{}", world(), item_id)
                                    >
                                        <div class="shrink-0">
                                            <ItemIcon item_id=item_id icon_size=IconSize::Small />
                                        </div>
                                        <div class="flex flex-col truncate">
                                            <span class="font-semibold">{item}</span>
                                            <span class="text-xs text-[color:var(--color-text-muted)] truncate">
                                                "x" {data.quantity}
                                            </span>
                                        </div>
                                    </a>
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right">
                                    <Gil amount=data.profit />
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right">
                                    <Gil amount=data.market_price />
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right hidden md:block">
                                    <span class="text-xs text-[color:var(--color-text-muted)]">
                                        "Lv " {data.task_level}
                                    </span>
                                </div>
                            </div>
                        }.into_any()
                    }
                />
             </div>
        </div>
    }
}

#[component]
pub fn VentureAnalyzer() -> impl IntoView {
    let query = use_query_map();
    let (home_world, _) = use_home_world();
    let nav = use_navigate();

    let region = Memo::new(move |_| {
        let worlds = use_context::<LocalWorldData>()
            .expect("Worlds should always be populated here")
            .0
            .unwrap();
        // Default to home world region or North-America
        let world_name = query
            .with(|p| p.get("world").clone())
            .or_else(|| home_world.get().map(|w| w.name))
            .unwrap_or_else(|| "North-America".to_string());

        worlds
            .lookup_world_by_name(&world_name)
            .map(|world| {
                let region = worlds.get_region(world);
                AnyResult::Region(region).get_name().to_string()
            })
            .unwrap_or_else(|| "North-America".to_string())
    });

    let global_cheapest_listings = ArcResource::new(region, move |region: String| async move {
        get_cheapest_listings(&region).await
    });

    let worlds = use_context::<LocalWorldData>()
        .expect("Should always have local world data")
        .0
        .unwrap();

    let initial_world = query.with_untracked(|p| {
        let binding = p.get("world");
        let world = binding.as_deref().unwrap_or_default();
        worlds
            .lookup_world_by_name(world)
            .and_then(|w| w.as_world().cloned())
    });

    let (selected_world, set_selected_world) = signal(initial_world);

    // If no world is selected initially, try to use home world
    Effect::new(move |_| {
        if selected_world.get_untracked().is_none()
            && let Some(home) = home_world.get()
        {
            set_selected_world(Some(home));
        }
    });

    // When selected world changes, update the URL
    Effect::new(move |_| {
        if let Some(world) = selected_world.get() {
            let world_name = world.name;
            let current_query = query.get_untracked();
            let world_matches = current_query
                .get("world")
                .map(|s| s == world_name)
                .unwrap_or(false);

            if !world_matches {
                let mut query_string = format!("?world={}", world_name);
                for (k, v) in current_query.into_iter() {
                    if k != "world" {
                        query_string.push_str(&format!("&{}={}", k, v));
                    }
                }
                nav(&query_string, NavigateOptions::default());
            }
        }
    });

    view! {
        <div class="flex flex-col gap-4 h-full">
            <MetaTitle title="Venture Analyzer - Ultros" />
            <MetaDescription text="Analyze Retainer Ventures for profitability" />

            <div class="flex flex-col gap-4 p-4 bg-brand-900/50 rounded-lg border border-brand-800">
                <div class="flex flex-row justify-between items-center">
                    <h1 class="text-2xl font-bold text-brand-100">"Venture Analyzer"</h1>
                </div>

                <div class="flex flex-col md:flex-row items-center gap-2">
                    <label class="text-[color:var(--brand-fg)] font-semibold">"Select World for Prices:"</label>
                    <div class="w-full md:w-auto">
                        <WorldOnlyPicker
                            current_world=selected_world.into()
                            set_current_world=set_selected_world.into()
                        />
                    </div>
                </div>

                <Suspense fallback=move || view! { <BoxSkeleton /> }>
                    {move || {
                        let listings = global_cheapest_listings.get();
                        match listings {
                            Some(Ok(listings)) => {
                                view! {
                                    <VentureAnalyzerTable
                                        global_cheapest_listings=listings
                                        world=region.into()
                                    />
                                }.into_any()
                            }
                            Some(Err(e)) => {
                                view! {
                                    <div class="text-red-400">
                                        "Error loading listings: " {e.to_string()}
                                    </div>
                                }.into_any()
                            }
                            _ => {
                                view! { <BoxSkeleton /> }.into_any()
                            }
                        }
                    }}
                </Suspense>
            </div>
        </div>
    }
}
