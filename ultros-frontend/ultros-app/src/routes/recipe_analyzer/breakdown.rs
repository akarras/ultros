//! The "why this cost" drawer: the selected row's own `CostBreakdown`,
//! line by line, beside the ledger its profit came from, plus the page's
//! add-to-craft-list entry point.
//!
//! Everything it shows is already on the row — the pricing pass keeps the
//! selected cost signal's run (`RecipeProfitData::breakdown`) — so opening
//! it never re-prices anything. The arithmetic lives in
//! [`breakdown_model`], a pure function, so the "these lines add up to the
//! Cost column" promise is tested without a DOM.

use super::{RecipeRow, short_signal};
use crate::analyzer_kit::formula::{PriceSignal, per_unit_cost};
use crate::analyzer_kit::stat_columns::Window;
use crate::components::add_recipe_to_list::AddRecipeToListModal;
use crate::components::crafting_cost::{CostBreakdown, PriceSource, ShardsMode, SubcraftInfo};
use crate::components::{gil::*, icon::Icon, item_icon::*};
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use icondata as i;
use leptos::prelude::*;
use leptos_i18n::I18nContext;
use thousands::Separable;
use xiv_gen::{ItemId, Recipe};

/// What a breakdown line's badge says about where its price came from.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum LineSource {
    Market,
    Vendor,
    Subcraft,
    /// Every unit came out of the user's on-hand stock.
    OnHand,
    /// A crystal under Exclude Crystals: shown, never counted.
    Excluded,
    /// Bought on a market no listing priced; costs 0 here, as the Cost
    /// column's "n unpriced" note says.
    Unpriced,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BreakdownLine {
    pub item_id: ItemId,
    pub quantity: i32,
    /// Units taken from on-hand stock. Shown as a note when the line is
    /// only partly covered; a fully covered line is `LineSource::OnHand`.
    pub on_hand: i32,
    pub unit_price: i32,
    pub source: LineSource,
    /// What this line adds to the craft's cost; `None` when it is not
    /// counted at all (an excluded crystal).
    pub total: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BreakdownModel {
    pub lines: Vec<BreakdownLine>,
    /// One execution of the recipe: the sum of the counted line totals.
    pub cost_per_craft: i32,
    pub yield_per_craft: i32,
    /// `cost_per_craft / yield`, the figure the Cost column shows.
    pub cost_per_unit: i32,
    /// The crystals' replacement value when Exclude Crystals left them
    /// out of the cost; `None` when they were counted or cost nothing.
    pub crystals_excluded: Option<i32>,
    pub on_hand_savings: Option<i32>,
}

fn clamp_i32(v: i64) -> i32 {
    v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// The drawer's figures from one `compute_cost` run. Mirrors
/// `compute_cost`'s own accounting: a line costs `used_from_market ×
/// unit_price`, on-hand units are free, and an excluded crystal is off the
/// books entirely.
pub(super) fn breakdown_model(
    b: &CostBreakdown,
    amount_result: i32,
    shards: ShardsMode,
) -> BreakdownModel {
    let excluding = shards == ShardsMode::ExcludeShards;
    let lines = b
        .ingredient_lines
        .iter()
        .map(|line| {
            let excluded = line.is_shard && excluding;
            let source = if excluded {
                LineSource::Excluded
            } else if line.used_from_market == 0 && line.used_from_on_hand > 0 {
                LineSource::OnHand
            } else {
                match line.source {
                    PriceSource::Market if line.unit_price == 0 => LineSource::Unpriced,
                    PriceSource::Market => LineSource::Market,
                    PriceSource::Vendor => LineSource::Vendor,
                    PriceSource::Subcraft => LineSource::Subcraft,
                }
            };
            BreakdownLine {
                item_id: line.item_id,
                quantity: line.needed_total,
                on_hand: line.used_from_on_hand,
                unit_price: line.unit_price,
                source,
                total: (!excluded).then(|| {
                    clamp_i32(i64::from(line.used_from_market) * i64::from(line.unit_price))
                }),
            }
        })
        .collect();
    BreakdownModel {
        lines,
        cost_per_craft: b.cost,
        yield_per_craft: amount_result.max(1),
        cost_per_unit: per_unit_cost(b.cost, amount_result),
        crystals_excluded: (excluding && b.shard_cost > 0).then_some(b.shard_cost),
        on_hand_savings: (b.on_hand_savings > 0).then_some(b.on_hand_savings),
    }
}

fn source_label(i18n: I18nContext<Locale, I18nKeys>, source: LineSource) -> String {
    match source {
        LineSource::Market => t_string!(i18n, recipe_breakdown_source_market),
        LineSource::Vendor => t_string!(i18n, recipe_breakdown_source_vendor),
        LineSource::Subcraft => t_string!(i18n, recipe_breakdown_source_subcraft),
        LineSource::OnHand => t_string!(i18n, recipe_breakdown_source_on_hand),
        LineSource::Excluded => t_string!(i18n, related_recipe_ingredient_excluded),
        LineSource::Unpriced => t_string!(i18n, recipe_breakdown_source_unpriced),
    }
    .to_string()
}

fn source_class(source: LineSource) -> &'static str {
    match source {
        LineSource::Market => "recipe-breakdown-badge",
        LineSource::Vendor => "recipe-breakdown-badge recipe-breakdown-badge-vendor",
        LineSource::Subcraft => "recipe-breakdown-badge recipe-breakdown-badge-subcraft",
        LineSource::OnHand => "recipe-breakdown-badge recipe-breakdown-badge-on-hand",
        LineSource::Excluded => "recipe-breakdown-badge recipe-breakdown-badge-muted",
        LineSource::Unpriced => "recipe-breakdown-badge recipe-breakdown-badge-warn",
    }
}

/// Move keyboard focus to the first element matching `selector`, if it is
/// on the page (a virtualized row's toggle may have scrolled away).
#[cfg(feature = "hydrate")]
fn focus_selector(selector: &str) {
    use wasm_bindgen::JsCast;
    if let Some(el) = document()
        .query_selector(selector)
        .ok()
        .flatten()
        .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
    {
        let _ = el.focus();
    }
}
#[cfg(not(feature = "hydrate"))]
fn focus_selector(_: &str) {}

/// The toggle in the Item cell: opens this row's breakdown, or closes it
/// when it is already the one showing.
#[component]
pub(super) fn BreakdownToggle(
    selected: RwSignal<Option<i32>>,
    recipe_id: i32,
    #[prop(into)] item_name: String,
) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let open = move || selected.get() == Some(recipe_id);
    view! {
        <button
            type="button"
            class="recipe-breakdown-toggle"
            data-recipe-breakdown-toggle=recipe_id
            aria-pressed=move || open().to_string()
            aria-label=move || t_string!(i18n, recipe_breakdown_open_aria, name = item_name.clone()).to_string()
            title=move || t_string!(i18n, recipe_breakdown_title).to_string()
            on:click=move |_| selected.update(|s| *s = if *s == Some(recipe_id) { None } else { Some(recipe_id) })
        >
            <Icon icon=i::FaReceiptSolid width="0.9em" height="0.9em" />
        </button>
    }
}

/// The drawer itself. Looks the selected recipe up in the sorted rows, so it
/// follows every recalculation and closes when the row leaves the result
/// set (a job filter, Hide suspicious, a scope with no price for it).
#[component]
pub(super) fn RecipeBreakdownDrawer(
    selected: RwSignal<Option<i32>>,
    rows: Memo<Vec<(usize, RecipeRow)>>,
    #[prop(into)] shards: Signal<ShardsMode>,
    #[prop(into)] require_hq: Signal<bool>,
    /// The effective revenue signal: what the column headers name.
    #[prop(into)]
    revenue_signal: Signal<PriceSignal>,
    #[prop(into)] revenue_place: Signal<String>,
    window: Window,
) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let row = Memo::new(move |_| {
        let id = selected.get()?;
        rows.with(|rows| {
            rows.iter()
                .find(|(_, row)| row.recipe.key_id.0 == id)
                .map(|(_, row)| row.clone())
        })
    });
    // A selection whose row vanished closes rather than showing stale data.
    Effect::new(move |_| {
        if selected.get().is_some() && row.with(Option::is_none) {
            selected.set(None);
        }
    });
    // Opening (or switching rows) puts focus in the drawer, so Escape and
    // Tab work from there; closing hands it back to the row's toggle.
    Effect::new(move |previous: Option<Option<i32>>| {
        let now = selected.get();
        if now.is_some() && previous.flatten() != now {
            request_animation_frame(|| focus_selector(".recipe-breakdown-close"));
        }
        now
    });
    let close = move || {
        if let Some(id) = selected.get_untracked() {
            selected.set(None);
            focus_selector(&format!("[data-recipe-breakdown-toggle=\"{id}\"]"));
        }
    };
    // The add-to-list modal lives outside the drawer's reactive body: that
    // body rebuilds whenever the row recomputes, and a modal inside it would
    // be rebuilt too, dropping every quantity the user had typed.
    let (modal_visible, set_modal_visible) = signal(false);
    let modal_recipe = RwSignal::new(None::<&'static Recipe>);

    let drawer = move || {
        let row = row.get()?;
        let items = &tracked_data().items;
        let recipe = row.recipe;
        let name = items
            .get(&ItemId(recipe.item_result))
            .map(|i| i.name.to_string())
            .unwrap_or_default();
        let model = breakdown_model(&row.breakdown, recipe.amount_result, shards.get());
        let lines = model
            .lines
            .iter()
            .map(|line| {
                let line_name = items
                    .get(&line.item_id)
                    .map(|i| i.name.to_string())
                    .unwrap_or_default();
                let partial_on_hand = (line.on_hand > 0 && line.source != LineSource::OnHand)
                    .then(|| {
                        t_string!(i18n, related_recipe_ingredient_on_hand, count = line.on_hand)
                            .to_string()
                    });
                let muted = matches!(line.source, LineSource::Excluded);
                let source = line.source;
                let unit_price = line.unit_price;
                let total = line.total;
                view! {
                    <tr class:recipe-breakdown-muted=muted data-breakdown-line=line.item_id.0>
                        <td>
                            <div class="flex items-center gap-2 min-w-0">
                                <ItemIcon item_id=line.item_id.0 icon_size=IconSize::Small />
                                <div class="flex flex-col min-w-0">
                                    <span class="truncate">{line_name}</span>
                                    <span class="flex flex-wrap items-center gap-1">
                                        <span class=source_class(source)>{source_label(i18n, source)}</span>
                                        {partial_on_hand.map(|note| view! {
                                            <span class="recipe-breakdown-badge recipe-breakdown-badge-on-hand">{note}</span>
                                        })}
                                    </span>
                                </div>
                            </div>
                        </td>
                        <td class="text-right tabular-nums">{line.quantity}</td>
                        <td class="text-right tabular-nums">
                            <div class="flex justify-end"><Gil amount=unit_price /></div>
                        </td>
                        <td class="text-right tabular-nums">
                            <div class="flex justify-end"><GilOrDash amount=total /></div>
                        </td>
                    </tr>
                }
            })
            .collect_view();

        let sub_crafts = sub_craft_lines(i18n, &row.breakdown.sub_crafts);
        let per_unit = (model.yield_per_craft > 1).then(|| {
            view! {
                <div class="recipe-breakdown-sum">
                    <span>{t_string!(i18n, recipe_breakdown_cost_per_unit, n = model.yield_per_craft).to_string()}</span>
                    <Gil amount=model.cost_per_unit />
                </div>
            }
        });
        let crystals = model.crystals_excluded.map(|gil| {
            view! {
                <div class="recipe-breakdown-note" data-breakdown-crystals-excluded=gil>
                    <span>{t_string!(i18n, recipe_breakdown_crystals_excluded).to_string()}</span>
                    <Gil amount=gil />
                </div>
            }
        });
        let savings = model.on_hand_savings.map(|gil| {
            view! {
                <div class="recipe-breakdown-note">
                    <span>{t_string!(i18n, recipe_breakdown_on_hand_savings).to_string()}</span>
                    <Gil amount=gil />
                </div>
            }
        });

        // The revenue assumption, in the formula strip's own words: the
        // signal (the cheapest listing when this row fell back to one), the
        // place, the priced quality, then the row's ledger.
        let signal = if row.fell_back() {
            PriceSignal::ListingMin
        } else {
            revenue_signal.get()
        };
        let quality = if row.stat_hq {
            t_string!(i18n, hq).to_string()
        } else {
            t_string!(i18n, nq).to_string()
        };
        let assumption = t_string!(
            i18n,
            recipe_breakdown_revenue_assumption,
            signal = short_signal(i18n, signal, window),
            place = revenue_place.get(),
            quality = quality
        )
        .to_string();
        let fell_back = row
            .fell_back()
            .then(|| view! { <p class="recipe-breakdown-caveat">{t!(i18n, recipe_breakdown_revenue_fell_back)}</p> });
        let ledger = match row.line {
            Some(line) => view! {
                <div class="recipe-breakdown-sum">
                    <span>{t!(i18n, recipe_breakdown_revenue)}</span>
                    <Gil amount=line.revenue />
                </div>
                <div class="recipe-breakdown-sum">
                    <span>"− " {t!(i18n, formula_term_tax)}</span>
                    <Gil amount=line.tax />
                </div>
                <div class="recipe-breakdown-sum">
                    <span>"− " {t!(i18n, recipe_breakdown_cost)}</span>
                    <Gil amount=line.cost />
                </div>
                <div class="recipe-breakdown-sum recipe-breakdown-total" data-breakdown-profit=line.profit>
                    <span>"= " {t!(i18n, formula_term_profit_per_unit)}</span>
                    <Gil amount=line.profit />
                </div>
            }
            .into_any(),
            None => view! {
                <p class="recipe-breakdown-caveat">{t!(i18n, analyzer_price_no_sales_title)}</p>
            }
            .into_any(),
        };

        Some(view! {
            <aside
                class="recipe-breakdown"
                data-recipe-breakdown=recipe.key_id.0
                aria-label=t_string!(i18n, recipe_breakdown_title).to_string()
                on:keydown=move |e| {
                    // The modal does not take focus, so its Escape can
                    // arrive here too: that one closes the modal alone.
                    if e.key() == "Escape" && !modal_visible.get_untracked() {
                        close();
                    }
                }
            >
                <header class="recipe-breakdown-header">
                    <ItemIcon item_id=recipe.item_result icon_size=IconSize::Medium />
                    <div class="min-w-0 flex-1">
                        <div class="text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]">
                            {t!(i18n, recipe_breakdown_title)}
                        </div>
                        <div class="font-bold truncate">{name}</div>
                    </div>
                    <button
                        type="button"
                        class="recipe-breakdown-close"
                        aria-label=t_string!(i18n, recipe_breakdown_close_aria).to_string()
                        on:click=move |_| close()
                    >
                        <Icon icon=i::AiCloseOutlined />
                    </button>
                </header>
                <div class="recipe-breakdown-body">
                    <button
                        type="button"
                        class="btn-primary w-full justify-center"
                        data-recipe-breakdown-add
                        on:click=move |_| {
                            modal_recipe.set(Some(recipe));
                            set_modal_visible(true);
                        }
                    >
                        <Icon icon=i::AiOrderedListOutlined />
                        <span>{t!(i18n, recipe_breakdown_add_to_craft_list)}</span>
                    </button>

                    <section>
                        <h3 class="recipe-breakdown-heading">{t!(i18n, recipe_breakdown_ingredients)}</h3>
                        <table class="recipe-breakdown-table">
                            <thead>
                                <tr>
                                    <th scope="col" class="text-left">{t!(i18n, recipe_breakdown_col_ingredient)}</th>
                                    <th scope="col" class="text-right">{t!(i18n, recipe_breakdown_col_qty)}</th>
                                    <th scope="col" class="text-right">{t!(i18n, recipe_breakdown_col_unit)}</th>
                                    <th scope="col" class="text-right">{t!(i18n, recipe_breakdown_col_total)}</th>
                                </tr>
                            </thead>
                            <tbody>{lines}</tbody>
                        </table>
                        <div class="recipe-breakdown-sum recipe-breakdown-total" data-breakdown-cost-per-craft=model.cost_per_craft>
                            <span>{t!(i18n, recipe_breakdown_cost_per_craft)}</span>
                            <Gil amount=model.cost_per_craft />
                        </div>
                        {per_unit}
                        {crystals}
                        {savings}
                        {sub_crafts}
                    </section>

                    <section>
                        <h3 class="recipe-breakdown-heading">{t!(i18n, recipe_breakdown_revenue)}</h3>
                        <p class="text-sm" data-breakdown-assumption>{assumption}</p>
                        {fell_back}
                        {ledger}
                    </section>
                </div>
            </aside>
        })
    };

    view! {
        {drawer}
        <Show when=modal_visible>
            {move || {
                modal_recipe.get_untracked().map(|recipe| {
                    view! {
                        <AddRecipeToListModal
                            recipe
                            initial_hq=require_hq.get_untracked()
                            set_visible=set_modal_visible
                        />
                    }
                })
            }}
        </Show>
    }
}

/// Every sub-craft the winning run made, nested ones included — the table
/// only shows the top level, so this is where a depth-two craft is said.
fn sub_craft_lines(
    i18n: I18nContext<Locale, I18nKeys>,
    sub_crafts: &[SubcraftInfo],
) -> Option<impl IntoView + use<>> {
    if sub_crafts.is_empty() {
        return None;
    }
    let items = &tracked_data().items;
    let rows = sub_crafts
        .iter()
        .map(|sub| {
            let name = items
                .get(&sub.item_id)
                .map(|i| i.name.to_string())
                .unwrap_or_default();
            t_string!(
                i18n,
                recipe_breakdown_sub_craft_row,
                count = sub.amount,
                name = name,
                gil = sub.unit_cost.separate_with_commas()
            )
            .to_string()
        })
        .map(|text| view! { <li>{text}</li> })
        .collect_view();
    Some(view! {
        <div class="recipe-breakdown-subcrafts">
            <span>{t!(i18n, recipe_breakdown_sub_crafts)}</span>
            <ul>{rows}</ul>
        </div>
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::crafting_cost::IngredientLine;

    fn line(
        item: i32,
        needed: i32,
        on_hand: i32,
        unit: i32,
        source: PriceSource,
        shard: bool,
    ) -> IngredientLine {
        IngredientLine {
            item_id: ItemId(item),
            needed_total: needed,
            used_from_on_hand: on_hand,
            used_from_market: needed - on_hand,
            unit_price: unit,
            is_shard: shard,
            source,
            world_id: 0,
        }
    }

    fn breakdown(lines: Vec<IngredientLine>, shards: ShardsMode) -> CostBreakdown {
        // The same accounting `compute_cost` does, so the fixture cannot
        // disagree with the function the model mirrors.
        let mut b = CostBreakdown::default();
        for l in &lines {
            let market = l.used_from_market * l.unit_price;
            let held = l.used_from_on_hand * l.unit_price;
            if l.is_shard {
                b.shard_cost += market + held;
                if shards == ShardsMode::IncludeMarket {
                    b.cost += market;
                    b.on_hand_savings += held;
                }
            } else {
                b.cost += market;
                b.on_hand_savings += held;
            }
        }
        b.ingredient_lines = lines;
        b
    }

    fn fixture(shards: ShardsMode) -> CostBreakdown {
        breakdown(
            vec![
                line(1, 3, 0, 120, PriceSource::Market, false),
                line(2, 2, 0, 20, PriceSource::Vendor, false),
                line(3, 2, 0, 300, PriceSource::Subcraft, false),
                line(4, 4, 4, 55, PriceSource::Market, false),
                line(5, 5, 2, 10, PriceSource::Market, false),
                line(6, 1, 0, 0, PriceSource::Market, false),
                line(7, 8, 0, 8, PriceSource::Market, true),
            ],
            shards,
        )
    }

    #[test]
    fn counted_line_totals_add_up_to_the_craft_cost() {
        for shards in [ShardsMode::ExcludeShards, ShardsMode::IncludeMarket] {
            let b = fixture(shards);
            let m = breakdown_model(&b, 1, shards);
            let sum: i32 = m.lines.iter().filter_map(|l| l.total).sum();
            assert_eq!(sum, m.cost_per_craft, "{shards:?}");
            assert_eq!(m.cost_per_craft, b.cost, "{shards:?}");
        }
    }

    #[test]
    fn badges_follow_the_line_that_was_priced() {
        let m = breakdown_model(
            &fixture(ShardsMode::ExcludeShards),
            1,
            ShardsMode::ExcludeShards,
        );
        let sources: Vec<LineSource> = m.lines.iter().map(|l| l.source).collect();
        assert_eq!(
            sources,
            [
                LineSource::Market,
                LineSource::Vendor,
                LineSource::Subcraft,
                LineSource::OnHand,
                LineSource::Market,
                LineSource::Unpriced,
                LineSource::Excluded,
            ]
        );
        // Fully held: free. Partly held: only the bought units cost.
        assert_eq!(m.lines[3].total, Some(0));
        assert_eq!((m.lines[4].on_hand, m.lines[4].total), (2, Some(30)));
        // An unpriced line costs 0, as the Cost column counts it.
        assert_eq!(m.lines[5].total, Some(0));
        // An excluded crystal is not counted at all.
        assert_eq!(m.lines[6].total, None);
    }

    #[test]
    fn crystals_are_reported_only_when_excluded() {
        let excluded = breakdown_model(
            &fixture(ShardsMode::ExcludeShards),
            1,
            ShardsMode::ExcludeShards,
        );
        assert_eq!(excluded.crystals_excluded, Some(64));
        let counted = breakdown_model(
            &fixture(ShardsMode::IncludeMarket),
            1,
            ShardsMode::IncludeMarket,
        );
        assert_eq!(counted.crystals_excluded, None);
        assert_eq!(counted.lines[6].source, LineSource::Market);
        assert_eq!(counted.lines[6].total, Some(64));
    }

    #[test]
    fn per_unit_divides_by_the_yield_like_the_cost_column() {
        let b = fixture(ShardsMode::ExcludeShards);
        let m = breakdown_model(&b, 3, ShardsMode::ExcludeShards);
        assert_eq!(m.yield_per_craft, 3);
        assert_eq!(m.cost_per_unit, per_unit_cost(b.cost, 3));
        // On-hand savings: 4×55 held outright plus 2×10 of a partial line.
        assert_eq!(m.on_hand_savings, Some(240));
    }
}
