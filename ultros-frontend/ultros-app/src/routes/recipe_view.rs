//! Public, URL-backed recipe planning. Account access is only used by Save.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use futures::{StreamExt, stream};
use leptos::prelude::*;
use leptos_router::hooks::{use_params_map, use_query_map};
use ultros_api_types::{CurrentlyShownItem, list::ListItem, world_helper::AnySelector};
use xiv_gen::{ItemId, RecipeId, RecipeLevelTableId};

use crate::api::{bulk_add_item_to_list, get_listings, get_lists, get_login};
use crate::components::{
    clipboard::Clipboard,
    crafting_cost::{IngredientsIter, vendor_price_map},
    item_icon::{IconSize, ItemIcon},
    meta::{MetaDescription, MetaTitle},
};
use crate::global_state::{home_world::use_home_world, use_world_helper, xiv_data::tracked_data};
use crate::query_defaults::filter_query_signal;
use crate::recipe_planner::{self as planner, Material, Offer, Recipe};

pub(crate) fn recipe_href(
    id: i32,
    world: &str,
    query: &leptos_router::params::ParamsMap,
) -> String {
    format!("/recipe/{id}{}", market_query(world, query))
}

fn market_query(world: &str, query: &leptos_router::params::ParamsMap) -> String {
    use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
    let mut url = format!("?world={}", utf8_percent_encode(world, NON_ALPHANUMERIC));
    for key in [
        "buy-scope",
        "require-hq",
        "subcrafts",
        "shards-exclude",
        "lang",
    ] {
        if let Some(value) = query.get(key) {
            url.push_str(&format!(
                "&{key}={}",
                utf8_percent_encode(&value, NON_ALPHANUMERIC)
            ));
        }
    }
    url
}

fn resolve_market_query(
    mut query: leptos_router::params::ParamsMap,
    world: String,
    scope: String,
) -> leptos_router::params::ParamsMap {
    // ParamsMap::insert appends another value for an existing key. Replace
    // scope aliases and cookie defaults so a shared link has one concrete world.
    query.replace("world", world);
    query.replace("buy-scope", scope);
    query
}

fn pairs(raw: Option<String>) -> BTreeMap<i32, i64> {
    raw.unwrap_or_default()
        .split(',')
        .take(128)
        .filter_map(|p| {
            let (id, amount) = p.split_once(':')?;
            let id: i32 = id.parse().ok()?;
            let amount: i64 = amount.parse().ok()?;
            (id > 0 && (0..=1_000_000_000).contains(&amount)).then_some((id, amount))
        })
        .collect()
}

fn write_pair(raw: Option<String>, id: i32, value: i64) -> Option<String> {
    let mut values = pairs(raw);
    if value == 0 {
        values.remove(&id);
    } else {
        values.insert(id, value);
    }
    (!values.is_empty()).then(|| {
        values
            .into_iter()
            .map(|(k, v)| format!("{k}:{v}"))
            .collect::<Vec<_>>()
            .join(",")
    })
}

fn item_name(id: i32) -> String {
    tracked_data()
        .items
        .get(&ItemId(id))
        .map(|i| i.name.clone())
        .unwrap_or_else(|| format!("Item {id}"))
}

fn gil(amount: i64) -> String {
    use thousands::Separable;
    format!("{} gil", amount.separate_with_commas())
}

fn job(recipe: &xiv_gen::Recipe) -> String {
    let jobs = ["CRP", "BSM", "ARM", "GSM", "LTW", "WVR", "ALC", "CUL"];
    let level = tracked_data()
        .recipe_level_tables
        .get(&RecipeLevelTableId(recipe.recipe_level_table))
        .map(|r| r.class_job_level)
        .unwrap_or(0);
    format!(
        "{} · Lv. {level} · Yields {}",
        jobs.get(recipe.craft_type as usize).unwrap_or(&"Crafter"),
        recipe.amount_result
    )
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct MarketData {
    scope: String,
    ids: Vec<i32>,
    items: BTreeMap<i32, CurrentlyShownItem>,
    failed: BTreeSet<i32>,
}

#[component]
pub fn RecipeView() -> impl IntoView {
    let params = use_params_map();
    let id = Memo::new(move |_| params.with(|p| p.get("id").and_then(|v| v.parse::<i32>().ok())));
    view! {
        <For each=move || vec![id.get()] key=|id| *id children=move |id| {
            match id.and_then(|id| tracked_data().recipes.get(&RecipeId(id))) {
                Some(recipe) => view! { <RecipePage recipe /> }.into_any(),
                None => view! { <crate::routes::not_found::NotFound /> }.into_any(),
            }
        } />
    }
}

#[component]
fn RecipePage(recipe: &'static xiv_gen::Recipe) -> impl IntoView {
    let (qty, set_qty) = filter_query_signal::<i64>("quantity");
    let (world, set_world) = filter_query_signal::<String>("world");
    let (buy_scope, set_buy_scope) = filter_query_signal::<String>("buy-scope");
    let (craft, set_craft) = filter_query_signal::<String>("craft");
    let (owned, set_owned) = filter_query_signal::<String>("owned");
    let (hq, set_hq) = filter_query_signal::<bool>("require-hq");
    let (output_hq, set_output_hq) = filter_query_signal::<bool>("output-hq");
    let (shards, set_shards) = filter_query_signal::<bool>("shards-exclude");
    let (visits, set_visits) = filter_query_signal::<usize>("visits");
    let (subcrafts, _) = filter_query_signal::<bool>("subcrafts");
    let quantity = Memo::new(move |_| qty.get().unwrap_or(1).clamp(1, 9999));
    let (home, _) = use_home_world();
    let helper = StoredValue::new(use_world_helper().ok());
    let worlds = helper.with_value(|h| {
        h.as_ref()
            .map(|h| {
                h.iter()
                    .filter_map(|w| w.as_world().cloned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    let fallback_world = worlds.first().map(|w| w.name.clone()).unwrap_or_default();
    let selected_world = Memo::new(move |_| {
        let requested = world
            .get()
            .filter(|w| !w.is_empty())
            .or_else(|| home.get().map(|w| w.name));
        helper
            .with_value(|h| {
                h.as_ref()
                    .and_then(|h| requested.as_ref().and_then(|w| h.lookup_world_by_name(w)))
                    .and_then(|w| w.all_worlds().next())
                    .map(|w| w.name.clone())
            })
            .unwrap_or_else(|| fallback_world.clone())
    });
    let home_id = Memo::new(move |_| {
        helper.with_value(|h| {
            h.as_ref()
                .and_then(|h| h.lookup_world_by_name(&selected_world.get()))
                .and_then(|w| w.as_world())
                .map(|w| w.id)
                .unwrap_or(0)
        })
    });
    let scope_kind = Memo::new(move |_| match buy_scope.get().as_deref() {
        Some("world") => "world",
        Some("region") => "region",
        Some(_) => "datacenter",
        None => helper
            .with_value(|h| {
                h.as_ref()
                    .and_then(|h| world.get().and_then(|raw| h.lookup_world_by_name(&raw)))
                    .filter(|w| w.as_region().is_some())
                    .map(|_| "region")
            })
            .unwrap_or("datacenter"),
    });
    let scope = Memo::new(move |_| {
        helper
            .with_value(|h| {
                let h = h.as_ref()?;
                let name = selected_world.get();
                let w = h.lookup_world_by_name(&name)?.as_world()?;
                Some(match scope_kind.get() {
                    "world" => name,
                    "region" => h.get_region(w.into()).name.clone(),
                    _ => h
                        .lookup_selector(AnySelector::Datacenter(w.datacenter_id))?
                        .get_name()
                        .to_string(),
                })
            })
            .unwrap_or_default()
    });
    let world_name = move |id: i32| {
        helper.with_value(|h| {
            h.as_ref()
                .and_then(|h| h.lookup_selector(AnySelector::World(id)))
                .map(|w| w.get_name().to_string())
                .unwrap_or_else(|| format!("World {id}"))
        })
    };
    let catalog = StoredValue::new(
        tracked_data()
            .recipes
            .values()
            .map(|r| {
                (
                    r.key_id.0,
                    Recipe {
                        id: r.key_id.0,
                        output: r.item_result,
                        yield_amount: i64::from(r.amount_result),
                        ingredients: IngredientsIter::new(r)
                            .filter(|(id, n)| id.0 > 0 && *n > 0)
                            .map(|(id, n)| (id.0, i64::from(n)))
                            .collect(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>(),
    );
    let root = StoredValue::new(catalog.with_value(|c| c[&recipe.key_id.0].clone()));
    let materials = Memo::new(move |_| {
        let choices = pairs(craft.get())
            .into_iter()
            .filter_map(|(k, v)| i32::try_from(v).ok().map(|v| (k, v)))
            .collect();
        let excluded = if shards.get().unwrap_or(false) {
            tracked_data()
                .items
                .values()
                .filter(|i| i.item_search_category == 59)
                .map(|i| i.key_id.0)
                .collect()
        } else {
            BTreeSet::new()
        };
        catalog.with_value(|c| {
            root.with_value(|r| {
                planner::expand(
                    r,
                    quantity.get(),
                    c,
                    &choices,
                    &pairs(owned.get()),
                    &excluded,
                )
            })
        })
    });
    let refresh = RwSignal::new(0_u32);
    let fetch_ids = Memo::new(move |_| {
        let mut ids: BTreeSet<_> = materials
            .get()
            .unwrap_or_default()
            .iter()
            .filter(|m| m.recipe.is_none() && m.remaining() > 0)
            .map(|m| m.item)
            .collect();
        ids.insert(recipe.item_result);
        ids.into_iter().collect::<Vec<_>>()
    });
    // Client-only: render a deterministic skeleton on SSR and the first hydrate.
    // Four in-flight requests at most; changing quantity reuses the same payload.
    let market = LocalResource::new(move || {
        let scope = scope.get();
        let ids = fetch_ids.get();
        refresh.track();
        async move {
            if scope.is_empty() {
                return Arc::new(MarketData::default());
            }
            let responses: Vec<_> = stream::iter(ids.iter().copied())
                .map(|id| {
                    let scope = scope.clone();
                    async move { (id, get_listings(id, &scope).await) }
                })
                .buffer_unordered(4)
                .collect()
                .await;
            let mut data = MarketData {
                scope,
                ids,
                ..Default::default()
            };
            for (id, response) in responses {
                match response {
                    Ok(item) => {
                        data.items.insert(id, item);
                    }
                    Err(_) => {
                        data.failed.insert(id);
                    }
                }
            }
            Arc::new(data)
        }
    });
    let loaded = Memo::new(move |_| {
        market
            .get()
            .filter(|m| m.scope == scope.get() && m.ids == fetch_ids.get())
    });
    let offers = Memo::new(move |_| {
        loaded
            .get()
            .map(|m| {
                m.items
                    .iter()
                    .map(|(id, item)| {
                        let require_hq = hq.get().unwrap_or(false)
                            && tracked_data()
                                .items
                                .get(&ItemId(*id))
                                .is_some_and(|i| i.can_be_hq);
                        (
                            *id,
                            item.listings
                                .iter()
                                .filter(|(l, _)| !require_hq || l.hq)
                                .map(|(l, _)| Offer {
                                    id: l.id,
                                    world: l.world_id,
                                    quantity: i64::from(l.quantity),
                                    price: i64::from(l.price_per_unit),
                                })
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default()
    });
    let vendors = Memo::new(move |_| {
        vendor_price_map()
            .iter()
            .filter(|(id, _)| {
                !hq.get().unwrap_or(false)
                    || !tracked_data()
                        .items
                        .get(&ItemId(**id))
                        .is_some_and(|i| i.can_be_hq)
            })
            .map(|(id, p)| (*id, i64::from(*p)))
            .collect::<BTreeMap<_, _>>()
    });
    let plans = Memo::new(move |_| {
        if loaded.get().is_none() || home_id.get() == 0 {
            return Vec::new();
        }
        materials
            .get()
            .map(|m| planner::compare_routes(&m, &offers.get(), &vendors.get(), home_id.get()))
            .unwrap_or_default()
    });
    let selected = Memo::new(move |_| plans.get().get(visits.get().unwrap_or(4).min(4)).cloned());
    let finished = Memo::new(move |_| {
        let data = loaded.get()?;
        let item = data.items.get(&recipe.item_result)?;
        let require_hq = output_hq.get().unwrap_or(false);
        let offers: Vec<_> = item
            .listings
            .iter()
            .filter(|(l, _)| l.hq == require_hq)
            .map(|(l, _)| Offer {
                id: l.id,
                world: l.world_id,
                quantity: i64::from(l.quantity),
                price: i64::from(l.price_per_unit),
            })
            .collect();
        let demand = [Material {
            item: recipe.item_result,
            needed: quantity.get(),
            ..Default::default()
        }];
        planner::compare_routes(
            &demand,
            &BTreeMap::from([(recipe.item_result, offers)]),
            &BTreeMap::new(),
            home_id.get(),
        )
        .get(visits.get().unwrap_or(4).min(4))
        .and_then(|p| p.purchases.get(&recipe.item_result))
        .cloned()
    });
    let query = use_query_map();
    let resolved_query = Memo::new(move |_| {
        resolve_market_query(
            query.get(),
            selected_world.get(),
            scope_kind.get().to_string(),
        )
    });
    let share_url = Signal::derive(move || {
        format!(
            "https://ultros.app/recipe/{}{}",
            recipe.key_id.0,
            resolved_query.get().to_query_string()
        )
    });
    let show_save = RwSignal::new(false);
    let title = format!("{} · Recipe planner", item_name(recipe.item_result));
    let checklist = RwSignal::new(BTreeSet::<(i32, i32)>::new());
    // Purchase identities include stack, world and price in the memo; resetting
    // on a changed plan prevents a tick from claiming a new purchase is complete.
    Effect::new(move |_| {
        selected.track();
        checklist.set(BTreeSet::new());
    });
    let copy_plan = Signal::derive(move || {
        let mut text = format!(
            "{} × {}\n{}\n",
            item_name(recipe.item_result),
            quantity.get(),
            share_url.get()
        );
        if let Some(plan) = selected.get() {
            for (id, p) in &plan.purchases {
                for o in &p.offers {
                    text.push_str(&format!(
                        "{}: {} × {} @ {} = {}\n",
                        world_name(o.world),
                        item_name(*id),
                        o.quantity,
                        gil(o.price),
                        gil(o.quantity * o.price)
                    ));
                }
                if p.vendor_quantity > 0 {
                    text.push_str(&format!(
                        "Vendor: {} × {}\n",
                        item_name(*id),
                        p.vendor_quantity
                    ));
                }
                if p.missing() > 0 {
                    text.push_str(&format!("Missing: {} × {}\n", item_name(*id), p.missing()));
                }
            }
            text.push_str(&format!("Planned spend: {}\n", gil(plan.cost)));
        }
        for m in materials
            .get()
            .unwrap_or_default()
            .iter()
            .rev()
            .filter(|m| m.crafts > 0)
        {
            text.push_str(&format!(
                "Craft {}: {} operations\n",
                item_name(m.item),
                m.crafts
            ));
        }
        text
    });
    view! {
        <MetaTitle title=title.clone() />
        <MetaDescription text="Plan a recipe without signing in. Compare full-stack ingredient costs, craft intermediates, and see what each extra world visit saves." />
        <div class="space-y-5 pb-12" data-testid="recipe-planner">
            <nav aria-label="Recipe navigation" class="flex flex-wrap gap-2 text-sm text-[color:var(--color-text-muted)]">
                <a class="hover:text-brand-300" href=move || format!("/recipe-analyzer{}",market_query(&selected_world.get(),&resolved_query.get()))>"Recipe Analyzer"</a>
                <span aria-hidden="true">"/"</span><span>"Recipe planner"</span>
            </nav>
            <header class="panel rounded-xl p-4 sm:p-5 flex flex-wrap items-center gap-4">
                <ItemIcon item_id=recipe.item_result icon_size=IconSize::Medium />
                <div class="flex-1 min-w-[12rem]"><h1 class="text-xl sm:text-2xl font-bold">{item_name(recipe.item_result)}</h1><p class="text-sm text-[color:var(--color-text-muted)]">{job(recipe)}</p></div>
                <a class="btn-secondary text-sm" href=move ||format!("/item/{}/{}",selected_world.get(),recipe.item_result)>"View item market"</a>
                <div class="flex items-center gap-2 text-sm"><span>"Share plan"</span><Clipboard clipboard_text=share_url /></div>
            </header>
            <section aria-label="Plan settings" class="panel rounded-xl p-4 flex flex-wrap gap-4 items-end">
                <label class="text-sm space-y-1"><span class="block text-[color:var(--color-text-muted)]">"Items to make"</span><input aria-label="Items to make" class="input w-28" type="number" min="1" max="9999" value=move ||quantity.get() prop:value=move ||quantity.get() on:change=move |e|set_qty.set(event_target_value(&e).parse::<i64>().ok().map(|n|n.clamp(1,9999))) /></label>
                <label class="text-sm space-y-1"><span class="block text-[color:var(--color-text-muted)]">"Starting world"</span><select aria-label="Starting world" class="input" prop:value=move ||selected_world.get() on:change=move |e|set_world.set(Some(event_target_value(&e)))>{worlds.into_iter().map(|w| { let name=w.name; let selected_name=name.clone(); view!{<option value=name.clone() selected=move ||selected_world.get()==selected_name>{name.clone()}</option>} }).collect_view()}</select></label>
                <label class="text-sm space-y-1"><span class="block text-[color:var(--color-text-muted)]">"Buy from"</span><select aria-label="Buy from" class="input" prop:value=move ||scope_kind.get() on:change=move |e|set_buy_scope.set(Some(event_target_value(&e)))><option value="world" selected=move ||scope_kind.get()=="world">"Home world"</option><option value="datacenter" selected=move ||scope_kind.get()=="datacenter">"Datacenter"</option><option value="region" selected=move ||scope_kind.get()=="region">"Region"</option></select></label>
                <label class="flex items-center gap-2 text-sm pb-2"><input type="checkbox" checked=move ||hq.get().unwrap_or(false) prop:checked=move ||hq.get().unwrap_or(false) on:change=move |e|set_hq.set(Some(event_target_checked(&e))) />"HQ ingredients only"</label>
                <label class="flex items-center gap-2 text-sm pb-2"><input type="checkbox" checked=move ||shards.get().unwrap_or(false) prop:checked=move ||shards.get().unwrap_or(false) on:change=move |e|set_shards.set(Some(event_target_checked(&e))) />"Exclude crystals"</label>
                <button class="btn-secondary text-sm" on:click=move |_|refresh.update(|n|*n=n.wrapping_add(1))>"Refresh prices"</button>
            </section>
            <div class="flex flex-wrap items-center gap-3 text-sm text-[color:var(--color-text-muted)]">
                <label class="flex items-center gap-2">"Or set crafts"<input aria-label="Number of crafts" class="input w-24" type="number" min="1" max=9999_i64.div_euclid(i64::from(recipe.amount_result.max(1))).max(1) value=move ||(quantity.get()+i64::from(recipe.amount_result.max(1))-1)/i64::from(recipe.amount_result.max(1)) prop:value=move ||(quantity.get()+i64::from(recipe.amount_result.max(1))-1)/i64::from(recipe.amount_result.max(1)) on:change=move |e|{ if let Ok(n)=event_target_value(&e).parse::<i64>() {set_qty.set(Some(n.max(1).saturating_mul(i64::from(recipe.amount_result.max(1))).clamp(1,9999)));} } /></label>
                <span>"Changing crafts updates the desired output quantity."</span>
            </div>
            <Show when=move ||home_id.get()==0><p role="alert" class="panel rounded-xl p-4 text-amber-300">"World data is unavailable. Recipe ingredients still work; reload the page to retry market planning."</p></Show>
            <Show when=move ||loaded.get().is_some_and(|d| !d.failed.is_empty())><p role="alert" class="panel rounded-xl p-4 text-amber-300">"Some ingredient markets could not be loaded. Costs may be incomplete. Refresh prices to retry."</p></Show>
            <Show when=move ||selected.get().is_some_and(|p|p.missing>0)><p role="status" class="panel rounded-xl p-4 text-amber-300">"This plan has missing materials. The amount shown covers available purchases only; it is not the full cost to finish the recipe."</p></Show>
            <Show when=move ||subcrafts.get().unwrap_or(false) && craft.get().is_none()><p class="text-sm text-[color:var(--color-text-muted)]">"Your analyzer estimate included subcrafts. Choose which ingredients to craft below to price whole batches and their shopping stops."</p></Show>
            {move ||materials.get().err().map(|error|view!{<p role="alert" class="panel rounded-xl p-4 text-amber-300">{error}<button class="btn-secondary ml-3" on:click=move |_|set_craft.set(None)>"Reset craft choices"</button></p>})}
            <section aria-label="World visit comparison" class="space-y-2">
                <div class="flex flex-wrap justify-between gap-2"><h2 class="font-semibold text-lg">"Is another world worth the trip?"</h2><span class="text-sm text-[color:var(--color-text-muted)]">"Best-found plans · additional worlds beyond home"</span></div>
                <div class="grid grid-cols-2 xl:grid-cols-5 gap-3">
                <Suspense fallback=move ||view!{<div class="panel rounded-xl p-5 animate-pulse">"Loading ingredient markets…"</div>}>
                    {move || {
                        let plans=plans.get();
                        let baseline=plans.first().cloned();
                        let previous=plans.clone();
                        plans.into_iter().enumerate().map(|(index,p)| {
                            let label=["Stay home","Up to 1 world","Up to 2 worlds","Up to 3 worlds","Full scope"][index];
                            let saving=baseline.as_ref().filter(|b|b.missing==0 && p.missing==0).map(|b|b.cost-p.cost);
                            let incremental=index.checked_sub(1).and_then(|i|previous.get(i)).filter(|b|b.missing==0 && p.missing==0).map(|b|b.cost-p.cost);
                            view!{<button class="panel rounded-xl p-4 text-left space-y-1 hover:border-brand-500 focus-visible:ring-2 focus-visible:ring-brand-400" class:border-brand-400=move ||visits.get().unwrap_or(4).min(4)==index aria-pressed=move ||(visits.get().unwrap_or(4).min(4)==index).to_string() on:click=move |_|set_visits.set(Some(index))>
                                <span class="block text-sm text-[color:var(--color-text-muted)]">{label}</span><strong class="block text-xl tabular-nums">{gil(p.cost)}</strong>
                                <span class="block text-xs">{if p.missing>0 {format!("{} units unavailable · partial cost",p.missing)} else {format!("{} additional worlds",p.worlds.len())}}</span>
                                {saving.map(|s|view!{<span class="block text-xs text-emerald-400">{format!("{} saved vs home",gil(s))}</span>})}
                                {incremental.filter(|s|*s>0).map(|s|view!{<span class="block text-xs">{format!("{} extra savings",gil(s))}</span>})}
                            </button>}
                        }).collect_view()
                    }}
                </Suspense>
                </div>
            </section>
            <div class="grid grid-cols-1 xl:grid-cols-[minmax(0,1fr)_22rem] gap-5 items-start">
                <section aria-label="Ingredients" class="panel rounded-xl overflow-hidden min-w-0">
                    <div class="p-4 border-b border-[color:var(--color-outline)]"><h2 class="text-lg font-semibold">"Build your recipe"</h2><p class="text-sm text-[color:var(--color-text-muted)]">"Choose Buy or a recipe to craft. Shared ingredients are combined; owned quantities apply once."</p></div>
                    <div class="divide-y divide-[color:var(--color-outline)]">
                        <For each=move || { materials.get().unwrap_or_default().into_iter().filter(|m|m.item!=recipe.item_result).collect::<Vec<_>>() } key=|m|m.item children=move |line| {
                            let id=line.item;
                            let current=Memo::new(move |_|materials.get().unwrap_or_default().into_iter().find(|m|m.item==id).unwrap_or_default());
                            let alternatives=catalog.with_value(|c|c.values().filter(|r|r.output==id).map(|r|r.id).collect::<Vec<_>>());
                            view!{
                                <div class="p-4 space-y-3" data-testid=format!("material-{id}")>
                                    <div class="flex items-center gap-3"><ItemIcon item_id=id icon_size=IconSize::Small /><a class="font-medium hover:text-brand-300 flex-1" href=move ||format!("/item/{}/{id}",selected_world.get())>{item_name(id)}</a><span class="text-sm tabular-nums">{move ||format!("{} needed",current.get().needed)}</span></div>
                                    <div class="flex flex-wrap items-center gap-3">
                                        <label class="text-sm"><span class="sr-only">{format!("Source for {}",item_name(id))}</span><select class="input text-sm max-w-full" aria-label=format!("Source for {}",item_name(id)) prop:value=move ||current.get().recipe.unwrap_or(0).to_string() on:change=move |e|set_craft.set(write_pair(craft.get_untracked(),id,event_target_value(&e).parse().unwrap_or(0)))><option value="0" selected=move ||current.get().recipe.is_none()>"Buy"</option>{alternatives.into_iter().map(|rid|view!{<option value=rid.to_string() selected=move ||current.get().recipe==Some(rid)>{tracked_data().recipes.get(&RecipeId(rid)).map(|r|format!("Craft · {}",job(r))).unwrap_or_default()}</option>}).collect_view()}</select></label>
                                        <label class="flex gap-2 items-center text-sm text-[color:var(--color-text-muted)]">"Already have"<input class="input w-24" aria-label=format!("Already have {}",item_name(id)) type="number" min="0" max="1000000000" value=move ||pairs(owned.get()).get(&id).copied().unwrap_or(0) prop:value=move ||pairs(owned.get()).get(&id).copied().unwrap_or(0) on:change=move |e|set_owned.set(write_pair(owned.get_untracked(),id,event_target_value(&e).parse::<i64>().unwrap_or(0).clamp(0,1_000_000_000))) /></label>
                                        <span class="text-sm tabular-nums text-brand-300">{move || {
                                            let m=current.get();
                                            if m.recipe.is_some(){format!("{} crafts · {} left over",m.crafts,m.surplus)}else{selected.get().and_then(|p|p.purchases.get(&id).cloned()).map(|p|format!("{} · buy {} · {} missing",gil(p.cost),p.quantity,p.missing())).unwrap_or_else(||if m.remaining()==0{"Owned".into()}else{"Loading…".into()})}
                                        }}</span>
                                    </div>
                                    {move ||current.get().recipe.and_then(|rid| catalog.with_value(|c|c.get(&rid).cloned())).map(|r|view!{
                                        <details open class="rounded-lg bg-[color:var(--color-background)] border border-brand-700/30 p-3 text-sm"><summary class="cursor-pointer text-brand-300">"Materials for this ingredient"</summary><ul class="mt-2 space-y-1">{r.ingredients.into_iter().map(|(child,n)|view!{<li>{move ||format!("{} × {}",n*current.get().crafts,item_name(child))}</li>}).collect_view()}</ul></details>
                                    })}
                                </div>
                            }
                        } />
                    </div>
                </section>
                <aside class="panel rounded-xl p-5 space-y-4 xl:sticky xl:top-4" aria-label="Plan summary">
                    <h2 class="text-lg font-semibold">"Your crafting plan"</h2>
                    <div><span class="text-sm text-[color:var(--color-text-muted)]">"Planned purchase spend"</span><p class="text-3xl font-bold tabular-nums" data-testid="plan-total">{move ||selected.get().map(|p|gil(p.cost)).unwrap_or_else(||"Loading…".into())}</p></div>
                    <p class="text-sm text-[color:var(--color-text-muted)]">{move ||selected.get().filter(|p|p.missing==0).map(|p|format!("{} per requested item, rounded up",gil((p.cost+quantity.get()-1)/quantity.get())) )}</p>
                    <p class="text-sm">{move ||materials.get().ok().map(|m|format!("{} crafting operations · {} finished items · {} extra output",m.iter().map(|m|m.crafts).sum::<i64>(),quantity.get(),m.first().map(|m|m.surplus).unwrap_or(0)))}</p>
                    <p class="text-sm">{move ||selected.get().map(|p|format!("{} additional worlds · {} units missing",p.worlds.len(),p.missing))}</p>
                    <p class="text-xs text-[color:var(--color-text-muted)]">"Whole stacks included. Owned materials reduce cash spend; leftovers have no assumed resale value. Vendor prices assume access. Travel time and teleport fees are excluded."</p>
                    <Show when=move ||selected.get().is_some_and(|p|p.approximate)><p class="text-xs text-amber-300">"Large batch: stack selection is a best-found estimate."</p></Show>
                    <div class="border-t border-[color:var(--color-outline)] pt-3 space-y-2"><label class="flex items-center gap-2 text-sm"><input type="checkbox" checked=move ||output_hq.get().unwrap_or(false) prop:checked=move ||output_hq.get().unwrap_or(false) on:change=move |e|set_output_hq.set(Some(event_target_checked(&e))) />"Compare with HQ finished items"</label><p class="text-sm">{move ||finished.get().map(|p|if p.missing()>0{format!("Buy finished: {} units unavailable",p.missing())}else{format!("Buy finished in {}: {}",scope.get(),gil(p.cost))})}</p></div>
                    <p class="text-sm font-medium text-brand-300">{move ||selected.get().zip(finished.get()).filter(|(p,f)|p.missing==0 && f.missing()==0).map(|(p,f)|if f.cost>=p.cost{format!("Crafting saves {} in purchase spend",gil(f.cost-p.cost))}else{format!("Buying finished saves {}",gil(p.cost-f.cost))})}</p>
                    <p class="text-xs text-[color:var(--color-text-muted)]">"Finished-item purchases use the same buying scope and world-visit limit."</p>
                    <div class="flex flex-wrap items-center gap-2 text-sm"><span>"Copy shopping plan"</span><Clipboard clipboard_text=copy_plan /></div>
                    <button class="btn-primary w-full" disabled=move ||selected.get().is_none() on:click=move |_|show_save.set(true)>"Add remaining materials to a list"</button>
                    <p class="text-xs text-[color:var(--color-text-muted)]">"No account needed to plan or share. Sign in only to save to a list."</p>
                </aside>
            </div>
            <section class="space-y-3" aria-label="Shopping itinerary"><h2 class="text-lg font-semibold">"Shopping itinerary"</h2>
                <p class="text-sm text-[color:var(--color-text-muted)]">"Grouped by datacenter and world. Confirm availability before travelling; prices can change."</p>
                {move || {
                    let Some(plan)=selected.get() else{return view!{<p>"Loading shopping stops…"</p>}.into_any()};
                    let mut stops:BTreeMap<(String,String,i32),Vec<(i32,Offer)>>=BTreeMap::new();
                    for (id,p) in &plan.purchases {for o in &p.offers {
                        let dc=helper.with_value(|h|h.as_ref().and_then(|h|h.lookup_selector(AnySelector::World(o.world)).and_then(|w|w.as_world()).and_then(|w|h.lookup_selector(AnySelector::Datacenter(w.datacenter_id)))).map(|dc|dc.get_name().to_string()).unwrap_or_default());
                        stops.entry((dc,world_name(o.world),o.world)).or_default().push((*id,o.clone()));
                    }}
                    view!{<div class="grid gap-3 lg:grid-cols-2">{stops.into_iter().map(|((dc,name,_),rows)| {
                        let total=rows.iter().map(|(_,o)|o.price*o.quantity).sum::<i64>();
                        view!{<div class="panel rounded-xl p-4 space-y-3"><div class="flex justify-between gap-2"><h3 class="font-semibold">{format!("{dc} · {name}")}</h3><span class="tabular-nums">{gil(total)}</span></div>{rows.into_iter().map(|(id,o)|{
                            let key=(id,o.id);
                            let label=format!("{} × {} · {} each · {}",item_name(id),o.quantity,gil(o.price),gil(o.price*o.quantity));
                            view!{<label class="flex items-start gap-2 text-sm"><input type="checkbox" class="mt-1" prop:checked=move ||checklist.with(|s|s.contains(&key)) on:change=move |e|checklist.update(|s|{if event_target_checked(&e){s.insert(key);}else{s.remove(&key);}}) /><span>{label}</span></label>}
                        }).collect_view()}</div>}
                    }).collect_view()}
                    {plan.purchases.into_iter().filter(|(_,p)|p.vendor_quantity>0 || p.missing()>0).map(|(id,p)|view!{<div class="panel rounded-xl p-4 text-sm"><strong>{item_name(id)}</strong><p>{format!("Vendor: {} · Still missing: {}",p.vendor_quantity,p.missing())}</p></div>}).collect_view()}</div>}.into_any()
                }}
            </section>
            <section class="panel rounded-xl p-4 space-y-3" aria-label="Crafting order"><h2 class="text-lg font-semibold">"Craft in this order"</h2><ol class="list-decimal list-inside space-y-2 text-sm">{move ||materials.get().unwrap_or_default().into_iter().rev().filter(|m|m.crafts>0).map(|m|view!{<li>{format!("{} · {} crafts · {} extra",item_name(m.item),m.crafts,m.surplus)}</li>}).collect_view()}</ol></section>
            <details class="text-xs text-[color:var(--color-text-muted)]"><summary class="cursor-pointer">"Price freshness and calculation details"</summary><div class="mt-2 space-y-1"><p>"One-world comparisons check every world in your scope. Two- and three-world comparisons search promising combinations; they are best-found routes, not guaranteed global minima. Full-scope cost may involve datacenter travel. Only market worlds are counted; vendor stops are separate."</p>{move ||loaded.get().map(|d| {
                let mut lines=Vec::new();
                for (id,item) in &d.items {let oldest=item.last_updated.iter().map(|u|u.updated_at).min();lines.push(format!("{}: {}",item_name(*id),oldest.map(|t|format!("oldest world update {t} UTC")).unwrap_or_else(||"freshness unknown".into())));}
                for id in &d.failed {lines.push(format!("{}: market request failed — refresh to retry",item_name(*id)));}
                lines.into_iter().map(|line|view!{<p>{line}</p>}).collect_view()
            })}</div></details>
            <Show when=move ||show_save.get()><SavePlan materials=materials hq=hq set_visible=show_save /></Show>
        </div>
    }
}

#[component]
fn SavePlan(
    materials: Memo<Result<Vec<Material>, String>>,
    hq: Memo<Option<bool>>,
    set_visible: RwSignal<bool>,
) -> impl IntoView {
    let lists = LocalResource::new(move || async move {
        get_login().await?;
        get_lists().await
    });
    let action = Action::new(move |id: &i32| {
        let id = *id;
        let items = materials
            .get_untracked()
            .unwrap_or_default()
            .into_iter()
            .filter(|m| m.recipe.is_none() && m.remaining() > 0)
            .map(|m| ListItem {
                id: 0,
                item_id: m.item,
                list_id: id,
                hq: if hq.get_untracked().unwrap_or(false)
                    && tracked_data()
                        .items
                        .get(&ItemId(m.item))
                        .is_some_and(|i| i.can_be_hq)
                {
                    Some(true)
                } else {
                    None
                },
                quantity: Some(m.remaining() as i32),
                acquired: None,
                target_price: None,
            })
            .collect();
        async move { bulk_add_item_to_list(id, items).await }
    });
    view! {<crate::components::modal::Modal set_visible=SignalSetter::map(move |v|set_visible.set(v))><div class="space-y-4"><h2 class="text-xl font-semibold">"Save remaining materials"</h2>
        <Suspense fallback=move ||view!{<p>"Loading your lists…"</p>}>{move ||lists.get().map(|result| match result {
            Ok(lists)=>view!{<div class="space-y-2">{lists.into_iter().map(|list|view!{<button class="btn-secondary w-full" disabled=move ||action.pending().get() on:click=move |_|{action.dispatch(list.id);}>{list.name}</button>}).collect_view()}<a href="/list" class="block text-brand-300">"Manage or create lists"</a></div>}.into_any(),
            Err(_)=>view!{<p>"Sign in to save this plan to a list. Your plan is preserved in its link."</p><a href="/login" rel="external" class="btn-primary">"Sign in"</a>}.into_any(),
        })}</Suspense>
        {move ||action.value().get().map(|result|view!{<p role="status">{if result.is_ok(){"Materials added to your list."}else{"Could not save materials. Please try again."}}</p>})}
        <button class="btn-secondary" on:click=move |_|set_visible.set(false)>"Close"</button>
    </div></crate::components::modal::Modal>}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_choices_round_trip_and_reject_invalid_quantities() {
        let raw = write_pair(None, 42, 7);
        assert_eq!(pairs(raw.clone()), BTreeMap::from([(42, 7)]));
        assert_eq!(write_pair(raw, 42, 0), None);
        assert!(pairs(Some("1:-1,2:1000000001,garbage".into())).is_empty());
    }

    #[test]
    fn shared_market_replaces_scope_aliases_without_duplicate_values() {
        let mut query = leptos_router::params::ParamsMap::new();
        query.insert("world", "North-America".into());
        query.insert("buy-scope", "invalid".into());
        query.insert("owned", "42:7".into());
        let resolved = resolve_market_query(query, "Gilgamesh".into(), "region".into());
        assert_eq!(resolved.get("world").as_deref(), Some("Gilgamesh"));
        assert_eq!(resolved.get("owned").as_deref(), Some("42:7"));
        assert_eq!(resolved.to_query_string().matches("world=").count(), 1);
    }
}
