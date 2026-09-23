//! Sell and craft verdicts at the top of the item page.
//!
//! The arithmetic lives in `ultros_calc::verdict`; this module adapts the
//! page's listings payload and contexts into it and renders two compact
//! cards inside `#overview`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use leptos::prelude::*;
use leptos_router::location::Url;
use ultros_api_types::cheapest_listings::CheapestListingMapKey;
use ultros_api_types::world::World;
use ultros_api_types::world_helper::{AnySelector, OwnedResult, WorldHelper};
use ultros_api_types::{ActiveListing, CurrentlyShownItem, Retainer, SaleHistory};
use ultros_calc::verdict::{
    BoardVerdict, BuyPrice, BuySource, CraftVerdict, FloorWarning, ListingSample, SaleRate,
    SaleSample, SellVerdict, buy_price, craft_unit_cost, craft_verdict, headline_hq, sell_verdict,
};
use xiv_gen::{ItemId, Recipe};

use crate::components::crafting_cost::{
    CraftingCostOptions, EmptyOnHand, IngredientsIter, OnHand, ShardsMode, compute_cost,
    vendor_price_map,
};
use crate::components::gil::Gil;
use crate::components::on_hand_input::{LocalOnHand, OnHandMap};
use crate::components::related_items::{get_vendor_price, is_shard_item};
use crate::components::skeleton::SingleLineSkeleton;
use crate::error::AppError;
use crate::global_state::LocalWorldData;
use crate::global_state::cheapest_prices::{CheapestListingsResource, CheapestPrices};
use crate::global_state::cookies::Cookies;
use crate::global_state::craft_options::{self, CraftOptions};
use crate::global_state::home_world::{get_price_zone, use_home_world};
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::{t, t_string};
use crate::routes::item_view::{get_or_default, with_or};
use crate::routes::item_view_sections::Section;

type ListingRows = Vec<(ActiveListing, Arc<Retainer>)>;

const CARD_CLASS: &str = "item-surface flex min-w-0 flex-col gap-1.5 p-3 text-sm";
const MUTED: &str = "text-[color:var(--color-text-muted)]";
/// The craft card's rows, in order, with the height each reserves. Its
/// skeleton renders the same rows at the same heights, so swapping the card
/// in after hydration doesn't move the page.
const CRAFT_ROWS: [(&str, &str); 6] = [
    ("heading", "min-h-6"),
    ("craft", "min-h-[29px]"),
    ("buy", "min-h-[29px]"),
    ("verdict", "min-h-[26px]"),
    ("notes", "min-h-4"),
    ("link", "min-h-4"),
];

/// The height class [`CRAFT_ROWS`] reserves for `slot`.
fn row_min_h(slot: &str) -> &'static str {
    CRAFT_ROWS
        .iter()
        .find(|(name, _)| *name == slot)
        .map(|(_, min_h)| *min_h)
        .unwrap_or_default()
}
/// Lines in a typical sell card, for its loading skeleton.
const SELL_SKELETON_ROWS: usize = 7;

/// The world whose board the sell card describes: the page's world, else the
/// home world when it is in scope. An excluded world is never used.
pub(crate) fn sell_world(
    page_world: Option<i32>,
    scope_worlds: &[i32],
    excluded: &HashSet<i32>,
    home_world: Option<i32>,
) -> Option<i32> {
    page_world
        .or_else(|| home_world.filter(|home| scope_worlds.contains(home)))
        .filter(|world| !excluded.contains(world))
}

/// Every recipe whose result is `item_id`, in id order.
pub(crate) fn output_recipes(item_id: i32) -> Vec<&'static Recipe> {
    let mut recipes: Vec<&'static Recipe> = tracked_data()
        .recipes
        .values()
        .filter(|recipe| recipe.item_result == item_id)
        .collect();
    recipes.sort_by_key(|recipe| recipe.key_id.0);
    recipes
}

/// How many levels of intermediate crafts the craft verdict considers.
const SUBCRAFT_DEPTH: u8 = 2;

/// Recipes for the items up to `depth` levels below `roots`, keyed by the
/// item they make — the `recipes_by_output` map `compute_cost` walks for
/// sub-crafts, limited to what these recipes can reach.
pub(crate) fn subcraft_recipes(
    roots: &[&'static Recipe],
    depth: u8,
) -> HashMap<ItemId, Vec<&'static Recipe>> {
    let all = &tracked_data().recipes;
    let mut index: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
    let mut frontier: HashSet<ItemId> = roots
        .iter()
        .flat_map(|recipe| IngredientsIter::new(recipe).map(|(item, _)| item))
        .collect();
    for _ in 0..depth {
        if frontier.is_empty() {
            break;
        }
        let found: Vec<&'static Recipe> = all
            .values()
            .filter(|recipe| {
                let output = ItemId(recipe.item_result);
                frontier.contains(&output) && !index.contains_key(&output)
            })
            .collect();
        frontier = found
            .iter()
            .flat_map(|recipe| IngredientsIter::new(recipe).map(|(item, _)| item))
            .collect();
        for recipe in found {
            index
                .entry(ItemId(recipe.item_result))
                .or_default()
                .push(recipe);
        }
    }
    index
}

/// The vendor anchor `real_price` uses against laundered sales — the same
/// value `RealPriceSummary` passes, so both show the same real price.
fn laundering_vendor_price(item_id: i32) -> Option<i32> {
    tracked_data()
        .items
        .get(&ItemId(item_id))
        .map(|item| item.price_mid as i32)
        .filter(|price| *price > 0)
}

fn sale_samples(sales: &[SaleHistory]) -> Vec<SaleSample> {
    sales
        .iter()
        .map(|sale| SaleSample {
            world_id: sale.world_id,
            price_per_item: sale.price_per_item,
            quantity: sale.quantity,
            hq: sale.hq,
            sold_at: sale.sold_date.and_utc().timestamp(),
        })
        .collect()
}

fn quality_chip(hq: bool) -> AnyView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    view! {
        <span class="rounded border border-[color:var(--color-outline)] px-1 text-[10px] font-bold leading-4 text-[color:var(--color-text-muted)]">
            {if hq { t!(i18n, hq).into_any() } else { t!(i18n, nq).into_any() }}
        </span>
    }
    .into_any()
}

fn one_decimal(value: f64) -> String {
    format!("{value:.1}")
}

fn whole_percent(value: f64) -> String {
    format!("{value:.0}")
}

#[component]
pub(crate) fn ItemVerdicts(
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
    #[prop(into)] filtered_listings: Signal<ListingRows>,
    #[prop(into)] excluded_worlds: Signal<HashSet<i32>>,
    world: Memo<String>,
    item_id: Memo<i32>,
) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let craftable = Memo::new(move |_| !output_recipes(get_or_default(&item_id)).is_empty());
    view! {
        <section
            class="@container mt-4"
            aria-label=move || t_string!(i18n, item_verdict_region_label).to_string()
        >
            <div class="grid grid-cols-1 gap-3 @min-[40rem]:grid-cols-2">
                <SellVerdictCard listing_resource filtered_listings excluded_worlds world item_id />
                <Show when=move || get_or_default(&craftable)>
                    <CraftVerdictCard listing_resource item_id />
                </Show>
            </div>
        </section>
    }
}

/// What the sell card's `<Transition>` body reads. Built once per card so
/// the body can be a plain function (and tested after disposal).
#[derive(Clone)]
struct SellCardInputs {
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
    filtered_listings: Signal<ListingRows>,
    excluded_worlds: Signal<HashSet<i32>>,
    world: Memo<String>,
    item_id: Memo<i32>,
    world_data: Option<Arc<WorldHelper>>,
    home_world: Option<Signal<Option<World>>>,
    /// `now`-dependent text (sale rate, days of stock, ETA) waits for
    /// hydration so the server and the first client render agree.
    hydrated: RwSignal<bool>,
}

#[component]
fn SellVerdictCard(
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
    filtered_listings: Signal<ListingRows>,
    excluded_worlds: Signal<HashSet<i32>>,
    world: Memo<String>,
    item_id: Memo<i32>,
) -> impl IntoView {
    let inputs = SellCardInputs {
        listing_resource,
        filtered_listings,
        excluded_worlds,
        world,
        item_id,
        world_data: use_context::<LocalWorldData>().and_then(|data| data.0.ok()),
        // The item page always provides `Cookies`; the guard keeps the card
        // buildable in a bare owner (see `item_verdicts_builds_without_contexts`).
        home_world: use_context::<Cookies>().map(|_| use_home_world().0),
        hydrated: RwSignal::new(false),
    };
    let hydrated = inputs.hydrated;
    Effect::new(move |_| hydrated.set(true));

    view! {
        <Transition fallback=move || sell_card_skeleton()>
            {move || render_sell_card(&inputs)}
        </Transition>
    }
}

fn sell_card_skeleton() -> AnyView {
    view! {
        <div class=CARD_CLASS aria-hidden="true">
            {(0..SELL_SKELETON_ROWS).map(|_| view! { <SingleLineSkeleton /> }).collect_view()}
        </div>
    }
    .into_any()
}

fn render_sell_card(inputs: &SellCardInputs) -> AnyView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    // Every read goes through a `try_*` accessor: the server can walk this
    // body after the card's owner is gone (see `with_or` in item_view.rs).
    let show_rate = inputs.hydrated.try_get().unwrap_or(false);
    let world_data = inputs.world_data.as_ref();
    with_or(&inputs.listing_resource, ().into_any(), |data_ref| {
        let Some(Ok(data)) = data_ref.as_ref() else {
            return ().into_any();
        };
        let scope_name = Url::unescape(&get_or_default(&inputs.world));
        let scope = world_data.and_then(|helper| helper.lookup_world_by_name(&scope_name));
        let page_world = scope.as_ref().and_then(|s| s.as_world().map(|w| w.id));
        let scope_worlds: Vec<i32> = scope
            .as_ref()
            .map(|s| s.all_worlds().map(|w| w.id).collect())
            .unwrap_or_default();
        let home = inputs
            .home_world
            .and_then(|signal| with_or(&signal, None, |w| w.as_ref().map(|w| w.id)));
        let excluded = get_or_default(&inputs.excluded_worlds);
        let Some(world_id) = sell_world(page_world, &scope_worlds, &excluded, home) else {
            return view! {
                <div class=CARD_CLASS data-testid="sell-verdict">
                    <p class=MUTED>{t!(i18n, item_verdict_sell_pick_world)}</p>
                </div>
            }
            .into_any();
        };
        let world_name = world_data
            .and_then(|helper| helper.lookup_selector(AnySelector::World(world_id)))
            .map(|w| w.get_name().to_string())
            .unwrap_or_default();
        let listings: Vec<ListingSample> = get_or_default(&inputs.filtered_listings)
            .iter()
            .map(|(listing, _)| ListingSample {
                world_id: listing.world_id,
                price_per_unit: listing.price_per_unit,
                quantity: listing.quantity,
                hq: listing.hq,
            })
            .collect();
        let verdict = sell_verdict(
            &listings,
            &sale_samples(&data.sales),
            world_id,
            laundering_vendor_price(get_or_default(&inputs.item_id)),
            chrono::Utc::now().timestamp(),
        );
        sell_card_body(verdict, world_name, scope_name, show_rate)
    })
}

fn sell_card_body(
    verdict: SellVerdict,
    world_name: String,
    scope_name: String,
    show_rate: bool,
) -> AnyView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let heading =
        t_string!(i18n, item_verdict_sell_heading, world = world_name.as_str()).to_string();
    let board = match verdict.board {
        Some(board) => board_lines(board, verdict.rate, show_rate),
        None => {
            let quality = if verdict.hq {
                t_string!(i18n, hq).to_string()
            } else {
                t_string!(i18n, nq).to_string()
            };
            view! {
                <p class=MUTED>
                    {t_string!(i18n, item_verdict_no_listings, quality = quality.as_str(), world = world_name.as_str()).to_string()}
                </p>
            }
            .into_any()
        }
    };
    let real = match verdict.real_price {
        Some(real) => view! {
            <div class="flex flex-wrap items-center gap-x-1.5">
                <span class="text-blue-300">{t!(i18n, real_price)}</span>
                <Gil amount=real />
                {verdict.real_price_scope_wide.then(|| view! {
                    <span class=MUTED>
                        {t_string!(i18n, item_verdict_real_price_across, scope = scope_name.as_str()).to_string()}
                    </span>
                })}
            </div>
        }
        .into_any(),
        None => view! { <p class=MUTED>{t!(i18n, item_verdict_no_recent_sales)}</p> }.into_any(),
    };
    view! {
        <div class=CARD_CLASS data-testid="sell-verdict">
            <div class="flex items-center justify-between gap-2">
                <h2 class="text-base font-bold text-brand-200">{heading}</h2>
                {quality_chip(verdict.hq)}
            </div>
            {board}
            {real}
        </div>
    }
    .into_any()
}

fn board_lines(board: BoardVerdict, rate: SaleRate, show_rate: bool) -> AnyView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let wall = board.wall;
    let patient = board.patient.map(|patient| {
        let eta = board
            .patient_eta_days
            .filter(|_| show_rate)
            .map(|days| t_string!(i18n, item_verdict_eta_days, days = one_decimal(days)).to_string());
        view! {
            <div class="flex items-baseline justify-between gap-2">
                <span class=MUTED>{t!(i18n, item_verdict_patient)}</span>
                <span class="flex flex-wrap items-baseline justify-end gap-x-1.5">
                    <span class="font-bold"><Gil amount=patient /></span>
                    <span class=MUTED>
                        {t_string!(i18n, item_verdict_patient_hint, units = wall.units.to_string()).to_string()}
                    </span>
                    {eta.map(|eta| view! { <span class=MUTED>"· "{eta}</span> })}
                </span>
            </div>
        }
    });
    let gap = board.gap.map(|gap| {
        view! {
            <div class=format!("flex flex-wrap items-center gap-x-1.5 {MUTED}")>
                <span>{t_string!(i18n, item_verdict_gap, percent = whole_percent(gap.percent)).to_string()}</span>
                <span>"→"</span>
                <Gil amount=gap.price />
            </div>
        }
    });
    // The row is there from the first render; its `now`-dependent text only
    // fills in after hydration, so the card never grows a line.
    let stock = match (show_rate, rate, board.days_of_stock) {
        (false, _, _) => view! {
            <p class=MUTED data-slot="stock" aria-hidden="true">"\u{a0}"</p>
        }
        .into_any(),
        (true, SaleRate::UnitsPerDay(_), Some(days)) => view! {
            <p class=MUTED data-slot="stock">
                {t_string!(i18n, item_verdict_days_of_stock, days = one_decimal(days)).to_string()}
            </p>
        }
        .into_any(),
        (true, _, _) => view! {
            <p class=MUTED data-slot="stock">{t!(i18n, item_verdict_too_few_sales)}</p>
        }
        .into_any(),
    };
    let warning = board.warning.map(|warning| {
        let text = match warning {
            FloorWarning::UnderRealPrice { percent } => t_string!(
                i18n,
                item_verdict_floor_under_real,
                percent = whole_percent(percent)
            )
            .to_string(),
            FloorWarning::AboveRecentSales => t_string!(i18n, item_verdict_floor_above_real).to_string(),
        };
        view! {
            <span class="self-start rounded-full border border-amber-300/40 bg-amber-500/10 px-2 py-0.5 text-xs text-amber-200">
                {text}
            </span>
        }
    });
    view! {
        <div class="flex items-baseline justify-between gap-2">
            <span class=MUTED>{t!(i18n, item_verdict_fast)}</span>
            <span class="flex flex-wrap items-baseline justify-end gap-x-1.5">
                <span class="font-bold"><Gil amount=board.fast /></span>
                <span class=MUTED>{t!(i18n, item_verdict_fast_hint)}</span>
            </span>
        </div>
        {patient}
        <div class="my-1 border-t border-[color:var(--color-outline)]"></div>
        <div class=format!("flex flex-wrap items-center gap-x-1.5 {MUTED}")>
            <span>{t!(i18n, item_verdict_wall)}":"</span>
            <span>
                {t_string!(
                    i18n,
                    item_verdict_wall_counts,
                    listings = wall.listings.to_string(),
                    units = wall.units.to_string()
                ).to_string()}
            </span>
            <Gil amount=wall.low />
            {(wall.high != wall.low).then(|| view! { <span>"–"</span><Gil amount=wall.high /> })}
        </div>
        {gap}
        {stock}
        {warning}
    }
    .into_any()
}

/// What the craft card's `<Transition>` body reads (see [`SellCardInputs`]).
#[derive(Clone, Copy)]
struct CraftCardInputs {
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
    item_id: Memo<i32>,
    cheapest: Option<CheapestListingsResource>,
    options: Option<Memo<Option<CraftOptions>>>,
    price_zone: Option<Signal<Option<OwnedResult>>>,
    on_hand_map: Option<OnHandMap>,
    /// `CheapestPrices` is a client-only resource: the card is a skeleton on
    /// the server and during hydration (the #740 idiom).
    hydrated: RwSignal<bool>,
}

#[component]
fn CraftVerdictCard(
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
    item_id: Memo<i32>,
) -> impl IntoView {
    let cookies = use_context::<Cookies>();
    let inputs = CraftCardInputs {
        listing_resource,
        item_id,
        cheapest: use_context::<CheapestPrices>().map(|prices| prices.demand()),
        options: cookies.as_ref().map(|cookies| {
            cookies
                .use_cookie_typed::<_, CraftOptions>(craft_options::COOKIE_NAME)
                .0
        }),
        price_zone: cookies.map(|_| get_price_zone().0),
        on_hand_map: use_context::<OnHandMap>(),
        hydrated: RwSignal::new(false),
    };
    let hydrated = inputs.hydrated;
    Effect::new(move |_| hydrated.set(true));

    view! {
        <Transition fallback=move || craft_card_skeleton()>
            {move || render_craft_card(inputs)}
        </Transition>
    }
}

/// The craft card's rows, each a skeleton line, so swapping in the real card
/// after hydration does not move the page (see `craft_skeleton_has_the_card_rows`).
fn craft_card_skeleton() -> AnyView {
    view! {
        <div class=CARD_CLASS aria-hidden="true">
            {CRAFT_ROWS
                .iter()
                .map(|(slot, min_h)| {
                    view! {
                        <div class=format!("flex items-center {min_h}") data-slot=*slot>
                            <SingleLineSkeleton />
                        </div>
                    }
                })
                .collect_view()}
        </div>
    }
    .into_any()
}

fn render_craft_card(inputs: CraftCardInputs) -> AnyView {
    let item = get_or_default(&inputs.item_id);
    // Every read goes through a `try_*` accessor (see `render_sell_card`).
    let hq = with_or(&inputs.listing_resource, None, |data_ref| {
        data_ref
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .map(|data| headline_hq(&sale_samples(&data.sales), laundering_vendor_price(item)))
    });
    let Some(hq) = hq else {
        return ().into_any();
    };
    if !inputs.hydrated.try_get().unwrap_or(false) {
        return craft_card_skeleton();
    }
    let Some(cheapest) = inputs.cheapest else {
        return ().into_any();
    };
    with_or(&cheapest, None, |prices| {
        let prices = prices.as_ref()?.as_ref().ok()?;
        let opts = inputs
            .options
            .and_then(|cookie| cookie.try_get().flatten())
            .unwrap_or_default();
        let shards = if opts.exclude_shards {
            ShardsMode::ExcludeShards
        } else {
            ShardsMode::IncludeMarket
        };
        let recipes = output_recipes(item);
        let index = subcraft_recipes(&recipes, SUBCRAFT_DEPTH);
        let (craft_unit, unpriced) = recipes
            .iter()
            .map(|recipe| {
                // A fresh on-hand snapshot per run: compute_cost consumes it.
                let local = LocalOnHand::from_map(
                    inputs
                        .on_hand_map
                        .and_then(|map| map.0.try_get())
                        .unwrap_or_default(),
                );
                let empty = EmptyOnHand;
                let on_hand: &dyn OnHand = if opts.use_on_hand { &local } else { &empty };
                let cost_options = CraftingCostOptions {
                    require_hq: hq,
                    max_subcraft_depth: SUBCRAFT_DEPTH,
                    shards,
                    on_hand,
                    vendor_prices: Some(vendor_price_map()),
                };
                let breakdown = compute_cost(recipe, prices, &index, &cost_options, &is_shard_item);
                (
                    craft_unit_cost(breakdown.cost, recipe.amount_result),
                    breakdown.unpriced_market_lines,
                )
            })
            // Fully priced runs first, then the cheapest.
            .min_by_key(|&(unit, unpriced)| (unpriced > 0, unit))?;
        let market = |hq| {
            prices
                .map
                .get(&CheapestListingMapKey { item_id: item, hq })
                .map(|listing| listing.price)
        };
        let buy = buy_price(
            hq,
            market(true),
            market(false),
            get_vendor_price(item).map(|price| price as i32),
        );
        let zone = inputs
            .price_zone
            .and_then(|zone| zone.try_get().flatten())
            .map(|zone| zone.get_name().to_string())
            .unwrap_or_else(|| "North-America".to_string());
        let verdict = craft_verdict(craft_unit, buy.map(|buy| buy.price), unpriced);
        Some(craft_card_body(
            hq,
            craft_unit,
            buy,
            verdict,
            zone,
            opts.exclude_shards,
        ))
    })
    .unwrap_or_else(|| ().into_any())
}

fn craft_card_body(
    hq: bool,
    craft_unit: i32,
    buy: Option<BuyPrice>,
    verdict: CraftVerdict,
    zone: String,
    crystals_excluded: bool,
) -> AnyView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let buy_label = match buy.map(|buy| buy.source) {
        Some(BuySource::Vendor) => t_string!(i18n, item_verdict_buy_vendor).to_string(),
        _ => t_string!(i18n, item_verdict_buy_zone, zone = zone.as_str()).to_string(),
    };
    let nq_fallback = matches!(
        buy.map(|buy| buy.source),
        Some(BuySource::Market { nq_fallback: true })
    );
    let chip = |class: &'static str, label: AnyView, gil: Option<(i32, f64)>| {
        view! {
            <span class=format!("inline-flex items-center gap-1 self-start rounded-full border px-2 py-0.5 text-xs font-semibold {class}")>
                {label}
                {gil.map(|(gil, percent)| view! {
                    <Gil amount=gil />
                    <span>"("{whole_percent(percent)}"%)"</span>
                })}
            </span>
        }
        .into_any()
    };
    let verdict_view = match verdict {
        CraftVerdict::CraftSaves { gil, percent } => chip(
            "border-emerald-400/40 bg-emerald-500/10 text-emerald-200",
            t!(i18n, item_verdict_craft_saves).into_any(),
            Some((gil, percent)),
        ),
        CraftVerdict::BuyCheaper { gil, percent } => chip(
            "border-red-400/40 bg-red-500/10 text-red-200",
            t!(i18n, item_verdict_buy_cheaper).into_any(),
            Some((gil, percent)),
        ),
        CraftVerdict::AboutEven => chip(
            "border-[color:var(--color-outline)] text-[color:var(--color-text-muted)]",
            t!(i18n, item_verdict_about_even).into_any(),
            None,
        ),
        CraftVerdict::UnpricedIngredients => {
            view! { <p class=MUTED>{t!(i18n, item_verdict_incomplete)}</p> }.into_any()
        }
        CraftVerdict::NoBuyPrice => {
            view! { <p class=MUTED>{t!(i18n, item_verdict_no_buy_price)}</p> }.into_any()
        }
    };
    view! {
        <div class=CARD_CLASS data-testid="craft-verdict">
            <div class=format!("flex items-center justify-between gap-2 {}", row_min_h("heading")) data-slot="heading">
                <h2 class="text-base font-bold text-brand-200">{t!(i18n, item_verdict_craft_heading)}</h2>
                {quality_chip(hq)}
            </div>
            <div class=format!("flex items-baseline justify-between gap-2 {}", row_min_h("craft")) data-slot="craft">
                <span class=MUTED>{t!(i18n, item_verdict_craft_unit)}</span>
                <span class="font-bold"><Gil amount=craft_unit /></span>
            </div>
            <div class=format!("flex items-baseline justify-between gap-2 {}", row_min_h("buy")) data-slot="buy">
                <span class=format!("flex items-center gap-1 {MUTED}")>
                    {buy_label}
                    {nq_fallback.then(|| quality_chip(false))}
                </span>
                <span class="font-bold">
                    {match buy {
                        Some(buy) => view! { <Gil amount=buy.price /> }.into_any(),
                        None => t!(i18n, no_data).into_any(),
                    }}
                </span>
            </div>
            <div class=format!("flex {}", row_min_h("verdict")) data-slot="verdict">{verdict_view}</div>
            <p class=format!("text-xs {MUTED} {}", row_min_h("notes")) data-slot="notes">
                {t!(i18n, item_verdict_incl_subcrafts)}
                {crystals_excluded.then(|| view! { " · "{t!(i18n, item_verdict_crystals_excluded)} })}
            </p>
            <a class=format!("self-start text-xs underline text-brand-300 hover:text-brand-200 {}", row_min_h("link")) href=Section::Sources.href() data-slot="link">
                {t!(i18n, item_verdict_recipe_link)}" ↓"
            </a>
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sell_world_prefers_the_page_world() {
        assert_eq!(
            sell_world(Some(40), &[40], &HashSet::new(), Some(41)),
            Some(40)
        );
    }

    #[test]
    fn sell_world_uses_the_home_world_inside_the_scope() {
        assert_eq!(
            sell_world(None, &[40, 41, 42], &HashSet::new(), Some(41)),
            Some(41)
        );
    }

    #[test]
    fn sell_world_is_none_when_home_is_outside_the_scope_or_unset() {
        assert_eq!(sell_world(None, &[40, 41], &HashSet::new(), Some(99)), None);
        assert_eq!(sell_world(None, &[40, 41], &HashSet::new(), None), None);
    }

    #[test]
    fn sell_world_ignores_excluded_worlds() {
        let excluded = HashSet::from([41]);
        assert_eq!(sell_world(None, &[40, 41], &excluded, Some(41)), None);
        assert_eq!(sell_world(Some(41), &[41], &excluded, None), None);
    }

    /// Mirrors `real_price_summary_builds_without_an_i18n_context` in
    /// `item_view.rs`: the cards read i18n and every context through
    /// non-panicking accessors, so building them in a bare owner (no i18n,
    /// no cookies, no world data) must not panic.
    #[test]
    fn item_verdicts_builds_without_contexts() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            let listing_resource = Resource::new(|| (), |_| async { Err(AppError::ParamMissing) });
            let filtered_listings: Signal<Vec<(ActiveListing, Arc<Retainer>)>> =
                Signal::derive(Vec::new);
            let excluded_worlds: Signal<HashSet<i32>> = Signal::derive(HashSet::new);
            let world = Memo::new(|_| "Gilgamesh".to_string());
            let item_id = Memo::new(|_| 5057);
            let _ = view! {
                <ItemVerdicts listing_resource filtered_listings excluded_worlds world item_id />
            };
        });
    }

    #[test]
    fn output_recipes_finds_the_recipes_that_make_an_item() {
        let recipe = tracked_data()
            .recipes
            .values()
            .find(|recipe| recipe.item_result > 0)
            .expect("game data has recipes");
        let found = output_recipes(recipe.item_result);
        assert!(found.iter().any(|r| r.key_id == recipe.key_id));
        assert!(found.iter().all(|r| r.item_result == recipe.item_result));
    }

    /// A recipe with an ingredient that is itself craftable.
    fn recipe_with_craftable_ingredient() -> (&'static Recipe, ItemId) {
        let data = tracked_data();
        data.recipes
            .values()
            .find_map(|recipe| {
                IngredientsIter::new(recipe)
                    .map(|(ingredient, _)| ingredient)
                    .find(|ingredient| {
                        data.recipes
                            .values()
                            .any(|sub| ItemId(sub.item_result) == *ingredient)
                    })
                    .map(|ingredient| (recipe, ingredient))
            })
            .expect("game data has a recipe with a craftable ingredient")
    }

    #[test]
    fn subcraft_recipes_indexes_craftable_ingredients() {
        let (recipe, ingredient) = recipe_with_craftable_ingredient();
        let index = subcraft_recipes(&[recipe], 1);
        let subs = index.get(&ingredient).expect("ingredient is indexed");
        assert!(subs.iter().all(|sub| ItemId(sub.item_result) == ingredient));
        assert!(index.len() < tracked_data().recipes.len());
    }

    #[test]
    fn subcraft_recipes_at_depth_zero_is_empty() {
        let (recipe, _) = recipe_with_craftable_ingredient();
        assert!(subcraft_recipes(&[recipe], 0).is_empty());
    }

    /// `data-slot` markers in render order: the rows a card's layout is made of.
    fn slots(html: &str) -> Vec<&str> {
        html.split("data-slot=\"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .collect()
    }

    fn with_i18n<T>(f: impl FnOnce() -> T) -> T {
        let _ = any_spawner::Executor::init_futures_executor();
        Owner::new().with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            f()
        })
    }

    fn sample_board() -> BoardVerdict {
        let listing = ListingSample {
            world_id: 1,
            price_per_unit: 100,
            quantity: 2,
            hq: false,
        };
        sell_verdict(&[listing], &[], 1, None, 0)
            .board
            .expect("one listing")
    }

    /// The days-of-stock line depends on `now`, so it only fills in after
    /// hydration — but its row must be there from the first render, or the
    /// card grows a line and shifts the page.
    #[test]
    fn stock_line_keeps_its_slot_before_hydration() {
        with_i18n(|| {
            let board = sample_board();
            let before = board_lines(board, SaleRate::TooFewSales, false).to_html();
            let after = board_lines(board, SaleRate::TooFewSales, true).to_html();
            assert!(slots(&before).contains(&"stock"), "{before}");
            assert_eq!(slots(&before), slots(&after));
        });
    }

    /// The craft card renders a skeleton on the server and during hydration;
    /// it must have the same rows as the card it turns into.
    #[test]
    fn craft_skeleton_has_the_card_rows() {
        with_i18n(|| {
            let buy = Some(BuyPrice {
                price: 20,
                source: BuySource::Vendor,
            });
            let card = craft_card_body(
                false,
                18,
                buy,
                CraftVerdict::AboutEven,
                "North-America".to_string(),
                true,
            )
            .to_html();
            let skeleton = craft_card_skeleton().to_html();
            assert!(!slots(&card).is_empty(), "{card}");
            assert_eq!(slots(&skeleton), slots(&card));
        });
    }

    #[test]
    fn no_buy_price_does_not_blame_ingredients() {
        with_i18n(|| {
            let html = craft_card_body(
                false,
                18,
                None,
                CraftVerdict::NoBuyPrice,
                "North-America".to_string(),
                true,
            )
            .to_html();
            assert!(
                !html.contains("Some ingredients have no listings"),
                "{html}"
            );
            assert!(html.contains("No listing to compare with"), "{html}");
        });
    }

    /// Mirrors `item_view_listing_reads_survive_a_disposed_owner`: the server
    /// can walk a `<Transition>` body after the owner that created its
    /// signals was cleaned up (GlitchTip #6831/#6864). The card bodies must
    /// degrade to nothing rather than panic and truncate the SSR response.
    #[test]
    fn card_renders_survive_a_disposed_owner() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        let (sell, craft) = owner.with(|| {
            let listing_resource = Resource::new(|| (), |_| async { Err(AppError::ParamMissing) });
            let hydrated = RwSignal::new(true);
            let item_id = Memo::new(|_| 5057);
            let sell = SellCardInputs {
                listing_resource,
                filtered_listings: Signal::derive(Vec::new),
                excluded_worlds: Signal::derive(HashSet::new),
                world: Memo::new(|_| "Gilgamesh".to_string()),
                item_id,
                world_data: None,
                home_world: None,
                hydrated,
            };
            let craft = CraftCardInputs {
                listing_resource,
                item_id,
                cheapest: None,
                options: None,
                price_zone: None,
                on_hand_map: None,
                hydrated,
            };
            (sell, craft)
        });
        owner.cleanup();
        // The body then runs under the fresh, empty owner `ScopedFuture`
        // substitutes, not under no owner at all.
        Owner::new().with(|| {
            let _ = render_sell_card(&sell);
            let _ = render_craft_card(craft);
        });
    }

    /// Each row's `min-h-*` class on the element carrying `data-slot`.
    fn row_heights(html: &str) -> Vec<(String, Option<String>)> {
        html.split('<')
            .filter(|tag| tag.contains("data-slot=\""))
            .map(|tag| {
                let slot = tag
                    .split("data-slot=\"")
                    .nth(1)
                    .and_then(|s| s.split('"').next());
                let min_h = tag
                    .split("class=\"")
                    .nth(1)
                    .and_then(|s| s.split('"').next())
                    .and_then(|class| class.split(' ').find(|c| c.starts_with("min-h-")));
                (
                    slot.unwrap_or_default().to_string(),
                    min_h.map(str::to_string),
                )
            })
            .collect()
    }

    /// Same rows is not enough: a 12px skeleton bar per row left the card
    /// ~70px taller than its skeleton at phone width. Every row reserves the
    /// same minimum height in both.
    #[test]
    fn craft_skeleton_rows_reserve_the_card_row_heights() {
        with_i18n(|| {
            let card = craft_card_body(
                false,
                18,
                None,
                CraftVerdict::NoBuyPrice,
                "North-America".to_string(),
                true,
            )
            .to_html();
            let skeleton = craft_card_skeleton().to_html();
            let card_rows = row_heights(&card);
            assert!(
                card_rows.iter().all(|(_, min_h)| min_h.is_some()),
                "{card_rows:?}"
            );
            assert_eq!(row_heights(&skeleton), card_rows);
        });
    }
}
