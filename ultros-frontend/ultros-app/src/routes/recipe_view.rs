//! Public, URL-backed recipe planning. Account access is only used by Save.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use futures::{StreamExt, stream};
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use ultros_api_types::{CurrentlyShownItem, list::ListItem, world_helper::AnySelector};
use xiv_gen::{ItemId, RecipeId, RecipeLevelTableId};

use crate::api::{bulk_add_item_to_list, get_listings, get_lists, get_login};
use crate::components::app_link::use_query_map_or_default;
use crate::components::{
    clipboard::Clipboard,
    crafting_cost::{CRYSTAL_SEARCH_CATEGORY, IngredientsIter, vendor_price_map},
    item_icon::{IconSize, ItemIcon},
    meta::{MetaDescription, MetaTitle},
};
use crate::global_state::craft_options::MAX_HOP_GIL;
use crate::global_state::{
    cookies::Cookies,
    craft_options::{self, CraftOptions},
    home_world::use_home_world,
    use_world_helper,
    xiv_data::tracked_data,
};
use crate::i18n::*;
use crate::query_defaults::filter_query_signal;
use crate::recipe_planner::{self as planner, Material, Offer, Recipe, Travel};

pub(crate) use ultros_ui_crafting::links::recipe_href;

fn resolve_market_query(
    mut query: leptos_router::params::ParamsMap,
    world: String,
    scope: String,
) -> leptos_router::params::ParamsMap {
    // ParamsMap::insert appends another value for an existing key. Replace
    // scope aliases and cookie defaults so a shared link has one concrete world.
    query.replace("world", world);
    query.replace("buy-scope", scope);
    // The hop-budget selector is gone; new links carry `route` instead.
    query.remove("visits");
    query
}

/// `item:world,...` pairs reported as "not here". Unlike `pairs`, an item can
/// appear with several worlds.
fn pair_set(raw: Option<String>) -> BTreeSet<(i32, i32)> {
    raw.unwrap_or_default()
        .split(',')
        .take(128)
        .filter_map(|p| {
            let (item, world) = p.split_once(':')?;
            let item: i32 = item.parse().ok()?;
            let world: i32 = world.parse().ok()?;
            (item > 0 && world > 0).then_some((item, world))
        })
        .collect()
}

fn write_pair_set(set: &BTreeSet<(i32, i32)>) -> Option<String> {
    (!set.is_empty()).then(|| {
        set.iter()
            .map(|(item, world)| format!("{item}:{world}"))
            .collect::<Vec<_>>()
            .join(",")
    })
}

const HOME_ROUTE: &str = "home";

/// The selected route: `None` when the link leaves the choice to the ranking,
/// an empty set for `home`, otherwise the non-home world ids to visit.
fn route_set(raw: Option<&str>) -> Option<BTreeSet<i32>> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    if raw == HOME_ROUTE {
        return Some(BTreeSet::new());
    }
    Some(
        raw.split(',')
            .take(32)
            .filter_map(|w| w.parse::<i32>().ok())
            .filter(|w| *w > 0)
            .collect(),
    )
}

fn write_route(worlds: &BTreeSet<i32>) -> String {
    if worlds.is_empty() {
        HOME_ROUTE.into()
    } else {
        worlds
            .iter()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// Legacy `visits=N` links: the best-ranked card visiting at most `visits`
/// extra worlds. `4` was "full scope" and means the best-ranked card overall.
fn legacy_visits(plans: &[planner::ShoppingPlan], visits: usize) -> Option<usize> {
    plans
        .iter()
        .enumerate()
        .filter(|(_, p)| visits >= 4 || p.worlds.len() <= visits)
        .min_by_key(|(_, p)| planner::rank(p))
        .map(|(i, _)| i)
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

/// Item ids in the "Crystals" search category, from `(item id, search
/// category)` pairs. Kept data-agnostic so the category can be pinned in a
/// test: 59 ("Catalysts") is the neighbouring category that silently turned
/// this toggle into a no-op once already.
fn crystal_item_ids(items: impl Iterator<Item = (i32, i32)>) -> BTreeSet<i32> {
    items
        .filter(|(_, category)| *category == CRYSTAL_SEARCH_CATEGORY)
        .map(|(id, _)| id)
        .collect()
}

/// "Missing" is a shortage, so it only appears when there is one.
fn purchase_summary(cost: i64, quantity: i64, missing: i64) -> String {
    let mut text = format!("{} · buy {quantity}", gil(cost));
    if missing > 0 {
        text.push_str(&format!(" · {missing} missing"));
    }
    text
}

/// Vendor top-up and shortage for one item; either part is omitted when zero.
fn vendor_summary(vendor_quantity: i64, missing: i64) -> String {
    let mut parts = Vec::new();
    if vendor_quantity > 0 {
        parts.push(format!("Vendor: {vendor_quantity}"));
    }
    if missing > 0 {
        parts.push(format!("Still missing: {missing}"));
    }
    parts.join(" · ")
}

fn plan_summary(route: &str, missing: i64) -> String {
    let mut text = route.to_string();
    if missing > 0 {
        text.push_str(&format!(" · {missing} units missing"));
    }
    text
}

/// The card's comparison line against the no-new-travel baseline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SavingLine {
    /// The baseline card itself; `alone` when no travel route beats it.
    Baseline { alone: bool },
    /// Complete on both sides and this card is cheaper by this much gil.
    Saved(i64),
    /// The baseline is short and this card completes the recipe.
    Completes,
    /// This card is still short by this many units.
    Partial(i64),
}

fn saving_line(
    baseline: &planner::ShoppingPlan,
    plan: &planner::ShoppingPlan,
    is_baseline: bool,
    cards: usize,
) -> Option<SavingLine> {
    if is_baseline {
        return Some(SavingLine::Baseline { alone: cards == 1 });
    }
    if plan.missing > 0 {
        return Some(SavingLine::Partial(plan.missing));
    }
    if baseline.missing > 0 {
        return Some(SavingLine::Completes);
    }
    (baseline.cost > plan.cost).then(|| SavingLine::Saved(baseline.cost - plan.cost))
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

/// What the card row renders: frontier cards, plus a pinned shared route
/// inserted by travel distance, and the badge positions after insertion.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Cards {
    plans: Vec<planner::ShoppingPlan>,
    pinned: Option<usize>,
    best_value: Option<usize>,
    cheapest: Option<usize>,
    /// The no-new-travel card every other card's saving line compares
    /// against. Always `plans[0]`: the frontier sorts the no-travel shape
    /// first by construction, and inserting a pinned card by travel distance
    /// (`partition_point` with `<=`) can only land it after cards sharing
    /// that shape's zero distance, never before index 0.
    baseline: planner::ShoppingPlan,
}

/// The frontier's rightmost card is the most complete plan found, but because
/// the frontier improves lexicographically on `(missing, cost)`, completing
/// the recipe can cost more gil than an earlier, incomplete card. Badge it
/// "Cheapest" only when its gil cost is actually the lowest among the
/// displayed cards; otherwise the "Completes the recipe" subtext already
/// tells the story, and a false "Cheapest" claim would mislead.
fn cheapest_badge(plans: &[planner::ShoppingPlan], candidate: Option<usize>) -> Option<usize> {
    let i = candidate?;
    let cost = plans.get(i)?.cost;
    plans
        .iter()
        .enumerate()
        .all(|(j, p)| j == i || cost <= p.cost)
        .then_some(i)
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
    // Legacy hop budget from old links; only read, and cleared by a card click.
    let (visits, set_visits) = filter_query_signal::<usize>("visits");
    let (route, set_route) = filter_query_signal::<String>("route");
    let (unavailable, set_unavailable) = filter_query_signal::<String>("unavailable");
    let (subcrafts, _) = filter_query_signal::<bool>("subcrafts");
    let i18n = use_i18n();
    let quantity = Memo::new(move |_| qty.get().unwrap_or(1).clamp(1, 9999));
    // The shared craft-options cookie carries the crystal default and the
    // planner's travel weights. An explicit URL value wins for crystals, the
    // same fallback the analyzer uses, so a link that omits the key does not
    // flip the meaning of the toggle.
    let craft_cookie = use_context::<Cookies>()
        .map(|cookies| cookies.use_cookie_typed::<_, CraftOptions>(craft_options::COOKIE_NAME));
    let options = Memo::new(move |_| {
        craft_cookie
            .and_then(|(read, _)| read.get())
            .unwrap_or_default()
    });
    let set_options = craft_cookie.map(|(_, set)| set);
    let exclude_crystals =
        Memo::new(move |_| shards.get().unwrap_or_else(|| options.get().exclude_shards));
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
    let datacenters = StoredValue::new(
        worlds
            .iter()
            .map(|w| (w.id, w.datacenter_id))
            .collect::<BTreeMap<i32, i32>>(),
    );
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
    let dc_name = move |id: i32| {
        helper.with_value(|h| {
            h.as_ref()
                .and_then(|h| {
                    h.lookup_selector(AnySelector::World(id))
                        .and_then(|w| w.as_world())
                        .and_then(|w| h.lookup_selector(AnySelector::Datacenter(w.datacenter_id)))
                })
                .map(|dc| dc.get_name().to_string())
                .unwrap_or_default()
        })
    };
    // "Aether · Gilgamesh, Adamantoise / Primal · Leviathan"
    let route_stops = move |worlds: &BTreeSet<i32>| {
        const SHOWN: usize = 6;
        let mut by_dc: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for w in worlds.iter().take(SHOWN) {
            by_dc.entry(dc_name(*w)).or_default().push(world_name(*w));
        }
        let mut text = by_dc
            .into_iter()
            .map(|(dc, worlds)| format!("{dc} · {}", worlds.join(", ")))
            .collect::<Vec<_>>()
            .join(" / ");
        if worlds.len() > SHOWN {
            text.push_str(" · ");
            text.push_str(
                &t_string!(
                    i18n,
                    recipe_planner_route_more_worlds,
                    count = worlds.len() - SHOWN
                )
                .to_string(),
            );
        }
        text
    };
    let route_label = move |travel: Travel, worlds_empty: bool| -> String {
        match (travel.dc_hops, travel.world_hops) {
            (0, 0) if worlds_empty => t_string!(i18n, recipe_planner_route_stay_home).to_string(),
            (0, 0) => t_string!(i18n, recipe_planner_route_no_new_travel).to_string(),
            (0, count) => {
                t_string!(i18n, recipe_planner_route_world_hops, count = count).to_string()
            }
            (dcs, worlds) => t_string!(
                i18n,
                recipe_planner_route_dc_and_world_hops,
                dcs = dcs,
                worlds = worlds
            )
            .to_string(),
        }
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
        let excluded = if exclude_crystals.get() {
            crystal_item_ids(
                tracked_data()
                    .items
                    .values()
                    .map(|i| (i.key_id.0, i.item_search_category)),
            )
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
    // Ticked itinerary lines are committed purchases. Each carries its own
    // listing snapshot, so it survives re-planning and a price refresh; it is
    // dropped only when its item is no longer something to buy.
    let locks = RwSignal::new(BTreeMap::<(i32, i32), Offer>::new());
    Effect::new(move |_| {
        let leaves: BTreeSet<i32> = materials
            .get()
            .unwrap_or_default()
            .iter()
            .filter(|m| m.recipe.is_none() && m.remaining() > 0)
            .map(|m| m.item)
            .collect();
        let stale = locks.with_untracked(|l| l.keys().any(|(item, _)| !leaves.contains(item)));
        if stale {
            locks.update(|l| l.retain(|(item, _), _| leaves.contains(item)));
        }
    });
    let context = Memo::new(move |_| {
        let mut locked: BTreeMap<i32, Vec<Offer>> = BTreeMap::new();
        locks.with(|l| {
            for ((item, _), offer) in l {
                locked.entry(*item).or_default().push(offer.clone());
            }
        });
        planner::RouteContext {
            home: home_id.get(),
            datacenters: datacenters.get_value(),
            weights: options.get().travel_weights(),
            locked,
            unavailable: pair_set(unavailable.get()),
        }
    });
    let comparison = Memo::new(move |_| {
        if loaded.get().is_none() || home_id.get() == 0 {
            return planner::RouteComparison::default();
        }
        materials
            .get()
            .map(|m| planner::compare_routes(&m, &offers.get(), &vendors.get(), &context.get()))
            .unwrap_or_default()
    });
    // Frontier cards plus, when a shared link names a route that is not on
    // the frontier, that route as a pinned card inserted by travel distance.
    // Badges follow the frontier's best-value and cheapest cards by identity.
    let cards = Memo::new(move |_| {
        let comparison = comparison.get();
        let best_worlds = comparison
            .best_value()
            .map(|i| comparison.cards[i].worlds.clone());
        let cheapest_worlds = comparison.cheapest().map(|p| p.worlds.clone());
        let baseline = comparison.baseline().cloned().unwrap_or_default();
        let mut plans = comparison.cards;
        let mut pinned = None;
        let unranked = route_set(route.get().as_deref())
            .filter(|set| !plans.is_empty() && !plans.iter().any(|p| p.worlds == *set));
        if let (Some(mut allowed), Ok(m)) = (unranked, materials.get()) {
            allowed.insert(home_id.get());
            let ctx = context.get();
            let plan = planner::shop(&m, &offers.get(), &vendors.get(), &allowed, &ctx);
            let key = (ctx.weights.distance(plan.travel), plan.travel);
            let at = plans.partition_point(|p| (ctx.weights.distance(p.travel), p.travel) <= key);
            plans.insert(at, plan);
            pinned = Some(at);
        }
        let find = |worlds: Option<BTreeSet<i32>>| {
            worlds.and_then(|w| plans.iter().position(|p| p.worlds == w))
        };
        let badges = if plans.len() > 1 {
            (
                find(best_worlds),
                cheapest_badge(&plans, find(cheapest_worlds)),
            )
        } else {
            (None, None)
        };
        Cards {
            plans,
            pinned,
            best_value: badges.0,
            cheapest: badges.1,
            baseline,
        }
    });
    let selected_index = Memo::new(move |_| {
        let cards = cards.get();
        if cards.plans.is_empty() {
            return None;
        }
        let default = cards.best_value.or(Some(0));
        if let Some(set) = route_set(route.get().as_deref()) {
            return cards
                .plans
                .iter()
                .position(|p| p.worlds == set)
                .or(cards.pinned)
                .or(default);
        }
        visits
            .get()
            .and_then(|v| legacy_visits(&cards.plans, v))
            .or(default)
    });
    let selected = Memo::new(move |_| {
        selected_index
            .get()
            .and_then(|i| cards.get().plans.get(i).cloned())
    });
    // Travel metadata changes on a tick even when all purchases stay the same.
    // Compare just the consistently ordered rows before notifying the itinerary.
    let stops = Memo::new(move |_| {
        selected.with(|plan| plan.as_ref().map(planner::itinerary).unwrap_or_default())
    });
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
        // Buy the finished item within the selected route: those stops are
        // being made anyway, so they carry no extra travel.
        let mut allowed = selected.get().map(|p| p.worlds).unwrap_or_default();
        allowed.insert(home_id.get());
        let ctx = planner::RouteContext {
            locked: BTreeMap::new(),
            ..context.get()
        };
        planner::shop(
            &demand,
            &BTreeMap::from([(recipe.item_result, offers)]),
            &BTreeMap::new(),
            &allowed,
            &ctx,
        )
        .purchases
        .remove(&recipe.item_result)
    });
    let query = use_query_map_or_default();
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
    let show_settings = RwSignal::new(false);
    let title = format!("{} · Recipe planner", item_name(recipe.item_result));
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
                <a class="hover:text-brand-300" href=move || ultros_ui_crafting::links::recipe_analyzer_href(&selected_world.get(), &resolved_query.get())>"Recipe Analyzer"</a>
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
                <label class="flex items-center gap-2 text-sm pb-2"><input type="checkbox" checked=move ||exclude_crystals.get() prop:checked=move ||exclude_crystals.get() on:change=move |e|set_shards.set(Some(event_target_checked(&e))) />"Exclude crystals"</label>
                <button class="btn-secondary text-sm" on:click=move |_|refresh.update(|n|*n=n.wrapping_add(1))>"Refresh prices"</button>
                <button class="btn-secondary text-sm" aria-label=move ||t_string!(i18n, recipe_planner_settings).to_string() on:click=move |_|show_settings.set(true)>{t!(i18n, recipe_planner_settings)}</button>
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
                <div class="flex flex-wrap justify-between gap-2"><h2 class="font-semibold text-lg">{t!(i18n, recipe_planner_route_heading)}</h2><span class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, recipe_planner_route_subheading)}</span></div>
                <div class="grid grid-cols-2 xl:grid-cols-5 gap-3">
                <Suspense fallback=move ||view!{<div class="panel rounded-xl p-5 animate-pulse">"Loading ingredient markets…"</div>}>
                    {move || {
                        let Cards{plans,pinned,best_value,cheapest,baseline}=cards.get();
                        let total=plans.len();
                        plans.into_iter().enumerate().map(|(index,p)| {
                            let mut label=route_label(p.travel,p.worlds.is_empty());
                            if pinned==Some(index) { label=format!("{} · {label}",t_string!(i18n, recipe_planner_shared_route)); }
                            let line=saving_line(&baseline,&p,index==0,total);
                            let stops=if p.worlds.is_empty() { t_string!(i18n, recipe_planner_route_home_only).to_string() } else { route_stops(&p.worlds) };
                            let badge=match (best_value==Some(index),cheapest==Some(index)) {
                                (true,true)=>Some((format!("{} · {}",t_string!(i18n, recipe_planner_route_best_value),t_string!(i18n, recipe_planner_route_cheapest)),true)),
                                (true,false)=>Some((t_string!(i18n, recipe_planner_route_best_value).to_string(),true)),
                                (false,true)=>Some((t_string!(i18n, recipe_planner_route_cheapest).to_string(),false)),
                                (false,false)=>None,
                            };
                            let worlds=p.worlds.clone();
                            view!{<button class="panel rounded-xl p-4 text-left space-y-1 hover:border-brand-500 focus-visible:ring-2 focus-visible:ring-brand-400" class:border-brand-400=move ||selected_index.get()==Some(index) aria-pressed=move ||(selected_index.get()==Some(index)).to_string() on:click=move |_|{ set_route.set(Some(write_route(&worlds))); set_visits.set(None); }>
                                <span class="flex flex-wrap items-center justify-between gap-2"><span class="text-sm text-[color:var(--color-text-muted)]">{label}</span>{badge.map(|(text,brand)|{
                                    let badge_class=if brand { "rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide bg-brand-500/20 text-brand-300" } else { "rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide bg-[color:var(--color-outline)] text-[color:var(--color-text-muted)]" };
                                    view!{<span class=badge_class data-testid="route-badge">{text}</span>}
                                })}</span>
                                <strong class="block text-xl tabular-nums">{gil(p.cost)}</strong>
                                <span class="block text-xs">{if p.missing > 0 {format!("{} units unavailable · partial cost",p.missing)} else {stops}}</span>
                                {match line {
                                    Some(SavingLine::Saved(s))=>Some(view!{<span class="block text-xs text-emerald-400">{t_string!(i18n, recipe_planner_route_saved_vs_home, gil = gil(s)).to_string()}</span>}),
                                    Some(SavingLine::Completes)=>Some(view!{<span class="block text-xs text-emerald-400">{t_string!(i18n, recipe_planner_route_completes).to_string()}</span>}),
                                    Some(SavingLine::Baseline{alone:true})=>Some(view!{<span class="block text-xs text-emerald-400">{t_string!(i18n, recipe_planner_route_just_stay_home).to_string()}</span>}),
                                    _=>None,
                                }}
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
                                            if m.recipe.is_some(){format!("{} crafts · {} left over",m.crafts,m.surplus)}else{selected.get().and_then(|p|p.purchases.get(&id).cloned()).map(|p|purchase_summary(p.cost,p.quantity,p.missing())).unwrap_or_else(||if m.remaining()==0{"Owned".into()}else{"Loading…".into()})}
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
                    <p class="text-sm">{move ||selected.get().map(|p|plan_summary(&route_label(p.travel,p.worlds.is_empty()),p.missing))}</p>
                    <p class="text-xs text-[color:var(--color-text-muted)]">"Whole stacks included. Owned materials reduce cash spend; leftovers have no assumed resale value. Vendor prices assume access. Travel time and teleport fees are excluded."</p>
                    <Show when=move ||selected.get().is_some_and(|p|p.approximate)><p class="text-xs text-amber-300">"Large batch: stack selection is a best-found estimate."</p></Show>
                    <div class="border-t border-[color:var(--color-outline)] pt-3 space-y-2"><label class="flex items-center gap-2 text-sm"><input type="checkbox" checked=move ||output_hq.get().unwrap_or(false) prop:checked=move ||output_hq.get().unwrap_or(false) on:change=move |e|set_output_hq.set(Some(event_target_checked(&e))) />"Compare with HQ finished items"</label><p class="text-sm">{move ||finished.get().map(|p|if p.missing()>0{format!("Buy finished: {} units unavailable",p.missing())}else{format!("Buy finished in {}: {}",scope.get(),gil(p.cost))})}</p></div>
                    <p class="text-sm font-medium text-brand-300">{move ||selected.get().zip(finished.get()).filter(|(p,f)|p.missing==0 && f.missing()==0).map(|(p,f)|if f.cost>=p.cost{format!("Crafting saves {} in purchase spend",gil(f.cost-p.cost))}else{format!("Buying finished saves {}",gil(p.cost-f.cost))})}</p>
                    <p class="text-xs text-[color:var(--color-text-muted)]">"Finished-item purchases use the same buying scope and the selected route's stops."</p>
                    <div class="flex flex-wrap items-center gap-2 text-sm"><span>"Copy shopping plan"</span><Clipboard clipboard_text=copy_plan /></div>
                    <button class="btn-primary w-full" disabled=move ||selected.get().is_none() on:click=move |_|show_save.set(true)>"Add remaining materials to a list"</button>
                    <p class="text-xs text-[color:var(--color-text-muted)]">"No account needed to plan or share. Sign in only to save to a list."</p>
                </aside>
            </div>
            <section class="space-y-3" aria-label="Shopping itinerary"><h2 class="text-lg font-semibold">"Shopping itinerary"</h2>
                <p class="text-sm text-[color:var(--color-text-muted)]">"Grouped by datacenter and world. Tick a line once bought: it stays in the plan while the rest re-plans. Confirm availability before travelling; prices can change."</p>
                {move || {
                    let reported=pair_set(unavailable.get());
                    (!reported.is_empty()).then(|| view!{<div class="flex flex-wrap items-center gap-2 text-sm" data-testid="unavailable-reports"><span class="text-[color:var(--color-text-muted)]">{t!(i18n, recipe_planner_reported_unavailable)}</span>
                        {reported.iter().map(|(item,world)|{ let (item,world)=(*item,*world); view!{<button class="btn-secondary text-xs" on:click=move |_|{ let mut set=pair_set(unavailable.get_untracked()); set.remove(&(item,world)); set_unavailable.set(write_pair_set(&set)); }>{format!("{} · {} ×",item_name(item),world_name(world))}</button>} }).collect_view()}
                        <button class="text-xs text-brand-300 hover:underline" on:click=move |_|set_unavailable.set(None)>{t!(i18n, recipe_planner_clear_unavailable)}</button></div>})
                }}
                <Show when=move ||selected.with(Option::is_some) fallback=||view!{<p>"Loading shopping stops…"</p>}>
                    <div class="grid gap-3 lg:grid-cols-2">
                    <For each=move ||stops.with(|stops|{
                        let mut worlds=stops.keys().copied().collect::<Vec<_>>();
                        worlds.sort_by_key(|world|(dc_name(*world),world_name(*world),*world));
                        worlds
                    }) key=|world|*world children=move |world|{
                        let rows=Memo::new(move |_|stops.with(|stops|stops.get(&world).cloned().unwrap_or_default()));
                        view!{<div class="panel rounded-xl p-4 space-y-3"><div class="flex justify-between gap-2"><h3 class="font-semibold">{format!("{} · {}",dc_name(world),world_name(world))}</h3><span class="tabular-nums">{move ||rows.with(|rows|gil(rows.iter().map(|(_,o)|o.price*o.quantity).sum::<i64>()))}</span></div>
                        <For each=move ||rows.get() key=|(id,o)|(*id,o.id,o.price,o.quantity) children=move |(id,o)|{
                            let key=(id,o.id);
                            let world=o.world;
                            let snapshot=o.clone();
                            let label=format!("{} × {} · {} each · {}",item_name(id),o.quantity,gil(o.price),gil(o.price*o.quantity));
                            view!{<div class="flex items-start gap-2 text-sm" data-testid=format!("stop-{id}-{world}") data-listing-id=o.id><label class="flex items-start gap-2 flex-1"><input type="checkbox" class="mt-1" prop:checked=move ||locks.with(|s|s.contains_key(&key)) on:change=move |e|locks.update(|s|{if event_target_checked(&e){s.insert(key,snapshot.clone());}else{s.remove(&key);}}) /><span>{label}</span></label>
                                <button class="btn-secondary text-xs shrink-0" disabled=move ||locks.with(|s|s.contains_key(&key)) on:click=move |_|{ let mut set=pair_set(unavailable.get_untracked()); set.insert((id,world)); set_unavailable.set(write_pair_set(&set)); locks.update(|s|s.retain(|(item,_),o|!(*item==id && o.world==world))); }>{t!(i18n, recipe_planner_not_here)}</button></div>}
                        } /></div>}
                    } />
                    {move ||selected.with(|plan|plan.as_ref().map(|plan|plan.purchases.iter().filter(|(_,p)|p.vendor_quantity>0 || p.missing()>0).map(|(id,p)|view!{<div class="panel rounded-xl p-4 text-sm"><strong>{item_name(*id)}</strong><p>{vendor_summary(p.vendor_quantity,p.missing())}</p></div>}).collect_view()))}
                    </div>
                </Show>
            </section>
            <section class="panel rounded-xl p-4 space-y-3" aria-label="Crafting order"><h2 class="text-lg font-semibold">"Craft in this order"</h2><ol class="list-decimal list-inside space-y-2 text-sm">{move ||materials.get().unwrap_or_default().into_iter().rev().filter(|m|m.crafts>0).map(|m|view!{<li>{format!("{} · {} crafts · {} extra",item_name(m.item),m.crafts,m.surplus)}</li>}).collect_view()}</ol></section>
            <details class="text-xs text-[color:var(--color-text-muted)]"><summary class="cursor-pointer">"Price freshness and calculation details"</summary><div class="mt-2 space-y-1"><p>"Route cards are the travel frontier: one card per travel shape, shortest trip on the left. Each card to the right completes more of the recipe or, when equally complete, costs less gil; the last card is the most complete plan found and, among equally complete plans, the cheapest. The full scope is always evaluated, so the frontier keeps the best plan found. Best value is the card the gil-plus-travel weighting prefers (adjustable in Planner settings). Adding a single world is checked exhaustively; larger routes search promising combinations, so they are best-found, not guaranteed global minima. Worlds already on your itinerary are free to revisit. Only market worlds are counted; vendor stops are separate."</p>{move ||loaded.get().map(|d| {
                let mut lines=Vec::new();
                for (id,item) in &d.items {let oldest=item.last_updated.iter().map(|u|u.updated_at).min();lines.push(format!("{}: {}",item_name(*id),oldest.map(|t|format!("oldest world update {t} UTC")).unwrap_or_else(||"freshness unknown".into())));}
                for id in &d.failed {lines.push(format!("{}: market request failed — refresh to retry",item_name(*id)));}
                lines.into_iter().map(|line|view!{<p>{line}</p>}).collect_view()
            })}</div></details>
            <Show when=move ||show_save.get()><SavePlan materials=materials hq=hq set_visible=show_save /></Show>
            <Show when=move ||show_settings.get()><PlannerSettings options=options set_options=set_options set_visible=show_settings /></Show>
        </div>
    }
}

/// Travel weights live in the shared craft-options cookie so they follow the
/// user across pages. Without a cookie context the inputs are read-only.
#[component]
fn PlannerSettings(
    options: Memo<CraftOptions>,
    set_options: Option<SignalSetter<Option<CraftOptions>>>,
    set_visible: RwSignal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let save = move |update: fn(&mut CraftOptions, i64), raw: String| {
        if let (Some(set), Ok(value)) = (set_options, raw.parse::<i64>()) {
            let mut next = options.get_untracked();
            update(&mut next, value.clamp(0, MAX_HOP_GIL));
            set.set(Some(next));
        }
    };
    let reset = move |_| {
        if let Some(set) = set_options {
            let defaults = CraftOptions::default();
            set.set(Some(CraftOptions {
                world_hop_gil: defaults.world_hop_gil,
                dc_hop_gil: defaults.dc_hop_gil,
                ..options.get_untracked()
            }));
        }
    };
    view! {<crate::components::modal::Modal set_visible=SignalSetter::map(move |v|set_visible.set(v))><div class="space-y-4"><h2 class="text-xl font-semibold">{t!(i18n, recipe_planner_settings)}</h2>
        <p class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, recipe_planner_hop_cost_help)}</p>
        <div class="flex flex-wrap gap-4">
            <label class="text-sm space-y-1"><span class="block text-[color:var(--color-text-muted)]">{t!(i18n, recipe_planner_world_hop_cost)}</span><input class="input w-32" type="number" min="0" max=MAX_HOP_GIL step="100" disabled=set_options.is_none() prop:value=move ||options.get().travel_weights().world_hop on:change=move |e|save(|o,v|o.world_hop_gil=v,event_target_value(&e)) /></label>
            <label class="text-sm space-y-1"><span class="block text-[color:var(--color-text-muted)]">{t!(i18n, recipe_planner_dc_hop_cost)}</span><input class="input w-32" type="number" min="0" max=MAX_HOP_GIL step="100" disabled=set_options.is_none() prop:value=move ||options.get().travel_weights().dc_hop on:change=move |e|save(|o,v|o.dc_hop_gil=v,event_target_value(&e)) /></label>
        </div>
        <div class="flex flex-wrap gap-2"><button class="btn-secondary" disabled=set_options.is_none() on:click=reset>{t!(i18n, recipe_planner_reset_defaults)}</button><button class="btn-secondary" on:click=move |_|set_visible.set(false)>{t!(i18n, grid_close)}</button></div>
    </div></crate::components::modal::Modal>}
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
    fn unavailable_pairs_round_trip_and_reject_garbage() {
        let mut set = BTreeSet::new();
        assert_eq!(write_pair_set(&set), None);
        set.insert((5, 63));
        set.insert((5, 79));
        set.insert((9, 63));
        let raw = write_pair_set(&set);
        assert_eq!(raw.as_deref(), Some("5:63,5:79,9:63"));
        assert_eq!(pair_set(raw), set);
        assert!(pair_set(Some("0:1,1:0,x:y,7".into())).is_empty());
    }

    #[test]
    fn route_param_encodes_home_and_sorted_world_ids() {
        assert_eq!(route_set(None), None);
        assert_eq!(route_set(Some("")), None);
        assert_eq!(route_set(Some("home")), Some(BTreeSet::new()));
        assert_eq!(route_set(Some("79,63")), Some(BTreeSet::from([63, 79])));
        assert_eq!(route_set(Some("63,junk")), Some(BTreeSet::from([63])));
        assert_eq!(write_route(&BTreeSet::new()), "home");
        assert_eq!(write_route(&BTreeSet::from([79, 63])), "63,79");
    }

    #[test]
    fn legacy_visits_selects_by_world_count_when_no_route_is_given() {
        let plan = |worlds: &[i32], effective: i64| planner::ShoppingPlan {
            worlds: worlds.iter().copied().collect(),
            effective,
            cost: effective,
            ..Default::default()
        };
        // Frontier order: home first, then by distance.
        let plans = vec![
            plan(&[], 300),
            plan(&[63], 200),
            plan(&[63, 79], 100),
            plan(&[63, 79, 80], 400),
        ];
        assert_eq!(legacy_visits(&plans, 0), Some(0));
        assert_eq!(legacy_visits(&plans, 1), Some(1));
        assert_eq!(legacy_visits(&plans, 2), Some(2));
        assert_eq!(legacy_visits(&plans, 4), Some(2));
        assert_eq!(legacy_visits(&[], 0), None);
    }

    #[test]
    fn saving_line_frames_each_card_against_the_baseline() {
        let plan = |cost: i64, missing: i64| planner::ShoppingPlan {
            cost,
            missing,
            ..Default::default()
        };
        let home = plan(1_000, 0);
        assert_eq!(
            saving_line(&home, &home, true, 1),
            Some(SavingLine::Baseline { alone: true })
        );
        assert_eq!(
            saving_line(&home, &home, true, 3),
            Some(SavingLine::Baseline { alone: false })
        );
        assert_eq!(
            saving_line(&home, &plan(800, 0), false, 2),
            Some(SavingLine::Saved(200))
        );
        assert_eq!(
            saving_line(&home, &plan(800, 2), false, 2),
            Some(SavingLine::Partial(2))
        );
        assert_eq!(
            saving_line(&plan(10, 4), &plan(900, 0), false, 2),
            Some(SavingLine::Completes)
        );
        // A pinned shared route can be dearer than home: no claim is made.
        assert_eq!(saving_line(&home, &plan(1_200, 0), false, 3), None);
    }

    #[test]
    fn cheapest_badge_is_none_when_the_frontiers_last_card_costs_more() {
        let plan = |cost: i64| planner::ShoppingPlan {
            cost,
            ..Default::default()
        };
        // An incomplete baseline at 10,000 gil, and a complete one-hop plan
        // at 40,000 gil: both are kept by the frontier in that order, but the
        // pricier complete plan must not be badged "Cheapest".
        let plans = vec![plan(10_000), plan(40_000)];
        assert_eq!(cheapest_badge(&plans, Some(1)), None);
        // When the rightmost card really is the cheapest, the badge holds.
        let cheaper = vec![plan(10_000), plan(4_000)];
        assert_eq!(cheapest_badge(&cheaper, Some(1)), Some(1));
        assert_eq!(cheapest_badge(&plans, None), None);
    }

    #[test]
    fn exclude_crystals_targets_the_crystal_search_category() {
        // 58 is "Crystals"; 59 is "Catalysts" (dark matter, glamour prisms).
        // The page once filtered on 59, which left every shard in the plan.
        let ids = crystal_item_ids([(2, 58), (5594, 59), (5, 58), (100, 1)].into_iter());
        assert_eq!(ids, BTreeSet::from([2, 5]));
    }

    #[test]
    fn purchase_summary_mentions_missing_only_when_short() {
        assert_eq!(purchase_summary(1_500, 3, 0), "1,500 gil · buy 3");
        assert_eq!(
            purchase_summary(1_500, 3, 2),
            "1,500 gil · buy 3 · 2 missing"
        );
    }

    #[test]
    fn plan_summary_mentions_missing_only_when_short() {
        assert_eq!(plan_summary("2 world hops", 0), "2 world hops");
        assert_eq!(plan_summary("Stay home", 4), "Stay home · 4 units missing");
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
