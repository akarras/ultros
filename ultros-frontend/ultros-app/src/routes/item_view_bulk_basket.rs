//! The item page's bulk basket: what a full order of N units costs on each
//! world, buying whole listings.
//!
//! At quantity 1 it is a single slim row so casual visitors scroll straight
//! past it; raising the quantity expands the per-world comparison.

use crate::components::gil::GilIcon;
use crate::components::listing_quality::ListingQuality;
use crate::components::world_name::WorldName;
use crate::error::AppError;
use crate::global_state::use_world_helper;
use crate::i18n::{t, t_string};
// Not `use_i18n()`: everything below renders inside a `<Transition>`, which
// on the server can build under the empty owner `ScopedFuture` substitutes
// for a disposed one (GlitchTip #7289/#7294).
use crate::i18n_fallback::use_i18n_or_default;
use crate::query_defaults::filter_query_signal;
use crate::routes::item_view::with_or;
use leptos::prelude::*;
use std::sync::Arc;
use thousands::Separable;
use ultros_api_types::world_helper::AnySelector;
use ultros_api_types::{ActiveListing, CurrentlyShownItem, Retainer};
use ultros_calc::bulk_basket::{Basket, BulkBaskets, bulk_baskets};

type ListingRows = Vec<(ActiveListing, Arc<Retainer>)>;

/// Shared with `recipe_view`'s craft quantity: `?quantity=` is "how many".
const QUANTITY_PARAM: &str = "quantity";
/// Past this the planner's solver only estimates anyway, and no market board
/// holds more of one item in practice.
const MAX_QUANTITY: i64 = 9_999;
const PRESETS: [i64; 2] = [99, 999];
/// Worlds shown before "show all"; a region can list dozens.
const COLLAPSED_WORLDS: usize = 8;

fn clamp_quantity(quantity: i64) -> i64 {
    quantity.clamp(1, MAX_QUANTITY)
}

/// Gil amount that may exceed `i32` (a full order of an expensive item), so
/// it can't go through [`crate::components::gil::Gil`].
#[component]
fn GilTotal(amount: i64, #[prop(optional)] approximate: bool) -> impl IntoView {
    view! {
        <span class="inline-flex items-center justify-end gap-1">
            <GilIcon />
            {approximate.then_some("≈")}
            {amount.separate_with_commas()}
        </span>
    }
}

#[component]
fn QuantityStepper(quantity: Signal<i64>, set_quantity: Callback<i64>) -> impl IntoView {
    let i18n = use_i18n_or_default();
    let step_class = "h-8 w-8 flex items-center justify-center text-lg leading-none text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)] disabled:opacity-40 disabled:cursor-not-allowed";
    view! {
        <div class="flex flex-wrap items-center gap-2">
            <div class="inline-flex items-center overflow-hidden rounded-md border border-[color:var(--color-outline)]">
                <button
                    type="button"
                    class=step_class
                    aria-label=move || t_string!(i18n, bulk_basket_decrease_aria).to_string()
                    // Braced: a bare `<`/`>` in an attribute ends the tag in `view!`.
                    disabled=move || { quantity.get() <= 1 }
                    on:click=move |_| set_quantity.run(quantity.get_untracked() - 1)
                >
                    "−"
                </button>
                <input
                    type="number"
                    inputmode="numeric"
                    min="1"
                    max=MAX_QUANTITY.to_string()
                    class="h-8 w-16 bg-transparent text-center text-sm tabular-nums [appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none focus:outline-none"
                    aria-label=move || t_string!(i18n, bulk_basket_quantity_aria).to_string()
                    prop:value=move || quantity.get().to_string()
                    on:input=move |ev| {
                        // An emptied box is mid-edit, not a request for 1.
                        if let Ok(value) = event_target_value(&ev).trim().parse::<i64>() {
                            set_quantity.run(value);
                        }
                    }
                />
                <button
                    type="button"
                    class=step_class
                    aria-label=move || t_string!(i18n, bulk_basket_increase_aria).to_string()
                    disabled=move || { quantity.get() >= MAX_QUANTITY }
                    on:click=move |_| set_quantity.run(quantity.get_untracked() + 1)
                >
                    "+"
                </button>
            </div>
            {PRESETS
                .into_iter()
                .map(|preset| {
                    view! {
                        <button
                            type="button"
                            aria-pressed=move || (quantity.get() == preset).to_string()
                            class=move || {
                                [
                                    "rounded-md border px-2 py-1 text-xs tabular-nums transition-colors",
                                    if quantity.get() == preset {
                                        "border-[color:var(--brand-ring)] bg-[color:var(--brand-bg)] text-[color:var(--brand-fg)] font-bold"
                                    } else {
                                        "border-[color:var(--color-outline)] text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]"
                                    },
                                ]
                                    .join(" ")
                            }
                            on:click=move |_| set_quantity.run(preset)
                        >
                            {preset}
                        </button>
                    }
                })
                .collect_view()}
        </div>
    }
}

/// The numeric cells shared by world rows and the cross-world row.
fn basket_cells(basket: &Basket) -> impl IntoView + use<> {
    let i18n = use_i18n_or_default();
    let approximate = basket.approximate();
    let cost = basket.cost();
    let unit = basket.unit_price();
    let listings = basket.listings;
    let retainers = basket.retainers;
    let received = basket.received();
    let missing = basket.missing();
    view! {
        <td class="px-2 py-1.5 align-middle text-right font-semibold" title=move || {
            approximate.then(|| t_string!(i18n, bulk_basket_approximate).to_string())
        }>
            <GilTotal amount=cost approximate />
        </td>
        <td class="px-2 py-1.5 align-middle text-right">
            {unit.map(|unit| view! { <GilTotal amount=unit /> })}
        </td>
        <td class="px-2 py-1.5 align-middle text-right">{listings}</td>
        <td class="px-2 py-1.5 align-middle text-right">{retainers}</td>
        <td class="px-2 py-1.5 align-middle text-right">
            {if missing > 0 {
                view! {
                    <span class="font-semibold text-amber-300">
                        {t!(i18n, bulk_basket_short_by, count = missing)}
                    </span>
                }
                    .into_any()
            } else {
                view! { <span>{received}</span> }.into_any()
            }}
        </td>
    }
}

#[component]
fn BasketTable(baskets: BulkBaskets, show_all: RwSignal<bool>) -> impl IntoView {
    let i18n = use_i18n_or_default();
    let world_name = move |world: i32| {
        use_world_helper()
            .ok()
            .and_then(|data| {
                data.lookup_selector(AnySelector::World(world))
                    .map(|value| value.get_name().to_string())
            })
            .unwrap_or_default()
    };
    let world_count = baskets.worlds.len();
    let cross = baskets.cross_world.map(|cross| {
        let summary = match cross.versus {
            Some((world, saving)) => t_string!(
                i18n,
                bulk_basket_cross_saves,
                amount = saving.separate_with_commas(),
                world = world_name(world)
            )
            .to_string(),
            None => t_string!(i18n, bulk_basket_cross_fills).to_string(),
        };
        let worlds = cross.basket.worlds.len();
        let names = cross
            .basket
            .worlds
            .iter()
            .enumerate()
            .map(|(index, world)| {
                view! {
                    {(index > 0).then_some(", ")}
                    <WorldName id=AnySelector::World(*world) />
                }
            })
            .collect_view();
        view! {
            <tr
                class="border-b border-[color:var(--color-outline)] bg-emerald-500/10"
                data-testid="bulk-basket-cross-world"
            >
                <td class="px-2 py-1.5 align-middle text-left">
                    <div class="font-semibold text-emerald-200">
                        {t!(i18n, bulk_basket_cross_world, count = worlds)}
                    </div>
                    <div class="text-xs text-[color:var(--color-text-muted)]">{names}</div>
                    <div class="text-xs text-emerald-200/90">{summary}</div>
                </td>
                {basket_cells(&cross.basket)}
            </tr>
        }
    });
    let rows = baskets
        .worlds
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            let short = !row.basket.complete();
            view! {
                <tr
                    class="border-b border-[color:var(--color-outline)]/60 last:border-b-0"
                    class:opacity-60=short
                    class:hidden=move || { index >= COLLAPSED_WORLDS && !show_all.get() }
                >
                    <td class="px-2 py-1.5 align-middle text-left">
                        <WorldName id=AnySelector::World(row.world) />
                    </td>
                    {basket_cells(&row.basket)}
                </tr>
            }
        })
        .collect_view();
    view! {
        <div class="overflow-x-auto">
            <table class="w-full min-w-[34rem] text-sm tabular-nums" data-testid="bulk-basket-table">
                <thead class="text-xs text-[color:var(--color-text-muted)]">
                    <tr class="border-b border-[color:var(--color-outline)]">
                        <th class="px-2 py-1 text-left font-medium">{t!(i18n, world)}</th>
                        <th class="px-2 py-1 text-right font-medium">{t!(i18n, bulk_basket_col_total)}</th>
                        <th class="px-2 py-1 text-right font-medium">{t!(i18n, bulk_basket_col_unit)}</th>
                        <th class="px-2 py-1 text-right font-medium">{t!(i18n, bulk_basket_col_listings)}</th>
                        <th class="px-2 py-1 text-right font-medium">{t!(i18n, retainers)}</th>
                        <th class="px-2 py-1 text-right font-medium">{t!(i18n, bulk_basket_col_receive)}</th>
                    </tr>
                </thead>
                <tbody>{cross}{rows}</tbody>
            </table>
        </div>
        {(world_count > COLLAPSED_WORLDS)
            .then(|| {
                view! {
                    <button
                        type="button"
                        class="mt-1 text-xs font-semibold text-[color:var(--color-text-muted)] underline hover:text-[color:var(--color-text)]"
                        aria-expanded=move || show_all.get().to_string()
                        on:click=move |_| show_all.update(|all| *all = !*all)
                    >
                        {move || {
                            if show_all.get() {
                                t!(i18n, bulk_basket_show_less).into_any()
                            } else {
                                t!(i18n, bulk_basket_show_all, count = world_count).into_any()
                            }
                        }}
                    </button>
                }
            })}
    }
}

#[component]
pub fn BulkBasket(
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
    #[prop(into)] filtered_listings: Signal<ListingRows>,
    #[prop(into)] quality: Signal<ListingQuality>,
) -> impl IntoView {
    let i18n = use_i18n_or_default();
    // `?quantity=`, absent means 1. Only written above 1 so a casual visit
    // keeps a clean URL, and SSR renders the same collapsed/expanded shape the
    // client hydrates.
    let (quantity_param, set_quantity_param) = filter_query_signal::<i64>(QUANTITY_PARAM);
    let quantity = Signal::derive(move || clamp_quantity(quantity_param.get().unwrap_or(1)));
    let set_quantity = Callback::new(move |value: i64| {
        let value = clamp_quantity(value);
        set_quantity_param.set((value > 1).then_some(value));
    });
    let expanded = move || quantity.get() > 1;
    // Lives here, not in the table, so a quantity change keeps it.
    let show_all = RwSignal::new(false);
    // Infallible reads only, like `filtered_listings`: a panic inside a memo
    // body leaves it permanently poisoned (GlitchTip #6865).
    let baskets = Memo::new(move |_| {
        let needed = quantity.try_get().unwrap_or(1);
        if needed <= 1 {
            return BulkBaskets::default();
        }
        let quality = quality.try_get().unwrap_or_default();
        with_or(&filtered_listings, BulkBaskets::default(), |rows| {
            bulk_baskets(
                needed,
                rows.iter()
                    .map(|(listing, _)| listing)
                    .filter(|listing| quality.matches(listing.hq)),
            )
        })
    });

    view! {
        <Transition fallback=move || ()>
            {move || {
                // Suspend on `listing_resource` itself (see ListingsPanel):
                // `filtered_listings` alone doesn't subscribe this Transition,
                // and an SSR/CSR shape mismatch trips the tachys hydration
                // panic behind GlitchTip #6831.
                if !listing_resource.with(|r| matches!(r, Some(Ok(_)))) {
                    return ().into_any();
                }
                // Everything quantity-dependent is its own reactive child: if
                // this body read `quantity`, each keystroke would remount the
                // stepper and drop focus from the input.
                view! {
                    <section
                        class="item-surface mt-4 px-3 py-2 sm:px-4"
                        class:py-3=expanded
                        aria-labelledby="bulk-basket-title"
                        data-testid="bulk-basket"
                    >
                        <div class="flex flex-wrap items-center gap-x-4 gap-y-2">
                            <h2
                                id="bulk-basket-title"
                                class="text-base font-bold text-[color:var(--color-text)]"
                            >
                                {t!(i18n, bulk_basket_title)}
                            </h2>
                            <QuantityStepper quantity set_quantity />
                            <p class="text-xs text-[color:var(--color-text-muted)]">
                                {move || {
                                    if expanded() {
                                        t!(i18n, bulk_basket_whole_stacks).into_any()
                                    } else {
                                        t!(i18n, bulk_basket_hint).into_any()
                                    }
                                }}
                            </p>
                        </div>
                        {move || {
                            if !expanded() {
                                return ().into_any();
                            }
                            let baskets = baskets.get();
                            if baskets.worlds.is_empty() {
                                view! {
                                    <p role="status" class="mt-2 py-2 text-sm text-[color:var(--color-text-muted)]">
                                        {t!(i18n, bulk_basket_no_listings)}
                                    </p>
                                }
                                    .into_any()
                            } else {
                                view! {
                                    <div class="mt-2">
                                        <BasketTable baskets show_all />
                                    </div>
                                }
                                    .into_any()
                            }
                        }}
                    </section>
                }
                    .into_any()
            }}
        </Transition>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantity_is_clamped_to_the_stepper_range() {
        assert_eq!(clamp_quantity(0), 1);
        assert_eq!(clamp_quantity(-5), 1);
        assert_eq!(clamp_quantity(99), 99);
        assert_eq!(clamp_quantity(1_000_000), MAX_QUANTITY);
    }

    /// Same hazard as `real_price_summary_builds_without_an_i18n_context`:
    /// building the block with no i18n (or router) context must not panic.
    #[test]
    fn bulk_basket_builds_without_an_i18n_context() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            let listing_resource = Resource::new(|| (), |_| async { Err(AppError::ParamMissing) });
            let filtered_listings = Signal::derive(ListingRows::new);
            let quality = Signal::derive(|| ListingQuality::All);
            let _ = view! { <BulkBasket listing_resource filtered_listings quality /> };
        });
    }
}
