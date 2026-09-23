//! One presentation for calculation assumptions, separate from row predicates.
//! The registry still owns URL definitions; the strip and header shortcuts share
//! setters created once in the route owner, outside reactive rendering closures.
use leptos::prelude::*;

use super::window::{MarketWindow, MarketWindowControl};
use crate::{
    components::{
        term_badge::{TermBadge, TermRole},
        virtual_grid::{ColumnFilter, registry::FilterRegistry},
    },
    i18n::*,
    query_defaults::filter_query_signal,
};

#[derive(Clone)]
pub struct CalculationTerm {
    pub role: TermRole,
    pub label: String,
    pub column: Option<&'static str>,
    pub key: Option<&'static str>,
    /// The market this term is priced in, read-only: `Revenue · Aether`.
    /// For a term that follows another term's picker.
    pub place: Option<Signal<String>>,
    /// The market this term is priced in, picked on the chip itself:
    /// `Cost · [Aether ▾]`. Wins over `place`.
    pub place_select: Option<CalculationPlace>,
}

/// A market picked inline on a term chip, as the recipe analyzer's strip
/// does, instead of a separate scope control in the page header.
#[derive(Clone, Copy)]
pub struct CalculationPlace {
    pub value: Signal<String>,
    /// `(token, label, enabled)`. Labels name the place ("Aether"), not the
    /// tier, so the chip reads as the market it prices from.
    pub options: Signal<Vec<(&'static str, String, bool)>>,
    pub on_change: Callback<String>,
}

impl CalculationTerm {
    pub fn fixed(role: TermRole, label: String, column: Option<&'static str>) -> Self {
        Self {
            role,
            label,
            column,
            key: None,
            place: None,
            place_select: None,
        }
    }

    pub fn input(role: TermRole, key: &'static str, column: &'static str) -> Self {
        Self {
            role,
            label: String::new(),
            column: Some(column),
            key: Some(key),
            place: None,
            place_select: None,
        }
    }

    pub fn with_place(mut self, place: Signal<String>) -> Self {
        self.place = Some(place);
        self
    }

    pub fn with_place_select(mut self, place: CalculationPlace) -> Self {
        self.place_select = Some(place);
        self
    }
}

#[derive(Clone, Copy)]
struct Input {
    key: &'static str,
    raw: Memo<Option<String>>,
    set: SignalSetter<Option<String>>,
}

#[derive(Clone, Copy)]
pub struct Calculation {
    terms: StoredValue<Vec<CalculationTerm>>,
    inputs: StoredValue<Vec<Input>>,
    registry: FilterRegistry,
    /// The shared market subject's compatible input (e.g. the turn-in cost,
    /// never the leve's item reward or the FC project's completed-item value).
    pub market_input: Option<&'static str>,
}

impl Calculation {
    pub fn provide(
        registry: FilterRegistry,
        terms: Vec<CalculationTerm>,
        market_input: Option<&'static str>,
    ) -> Self {
        let inputs = terms
            .iter()
            .filter_map(|term| term.key)
            .map(|key| {
                let (raw, set) = filter_query_signal::<String>(key);
                Input { key, raw, set }
            })
            .collect();
        let calculation = Self {
            terms: StoredValue::new(terms),
            inputs: StoredValue::new(inputs),
            registry,
            market_input,
        };
        provide_context(calculation);
        calculation
    }

    fn definition(self, key: &str) -> Option<ColumnFilter> {
        self.registry
            .controls()
            .into_iter()
            .find(|filter| filter.calculation && filter.key == key)
    }

    pub fn value(self, key: &str) -> String {
        let raw = self.inputs.with_value(|inputs| {
            inputs
                .iter()
                .find(|input| input.key == key)
                .and_then(|input| input.raw.get())
        });
        self.definition(key)
            .map(|definition| {
                raw.filter(|value| definition.options.iter().any(|(token, _)| token == value))
                    .or(definition.default_value)
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    }

    pub fn set(self, key: &str, value: String) {
        let Some(definition) = self.definition(key) else {
            return;
        };
        if !definition.options.iter().any(|(token, _)| *token == value) {
            return;
        }
        self.inputs.with_value(|inputs| {
            if let Some(input) = inputs.iter().find(|input| input.key == key) {
                input
                    .set
                    .set((definition.default_value.as_ref() != Some(&value)).then_some(value));
            }
        });
    }

    pub fn term(self, column: &str) -> Option<CalculationTerm> {
        self.terms.with_value(|terms| {
            terms
                .iter()
                .find(|term| term.column == Some(column))
                .cloned()
        })
    }

    pub fn selected_label(self, key: &str) -> String {
        let value = self.value(key);
        self.definition(key)
            .and_then(|definition| {
                definition
                    .options
                    .into_iter()
                    .find(|(token, _)| *token == value)
                    .map(|(_, label)| label)
            })
            .unwrap_or_default()
    }
}

#[component]
pub fn CalculationStrip(calculation: Calculation, window: MarketWindow) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    view! {
        <div class="flex flex-wrap items-center gap-2" data-analyzer-price-controls>
            {calculation.terms.get_value().into_iter().map(|term| {
                view! {
                    <span class="filter-chip max-w-full flex-wrap" data-calculation-term=term.key>
                        <TermBadge role=term.role />
                        {match term.key {
                            None => term.label.into_any(),
                            Some(key) => view! {
                                <label class="inline-flex max-w-full flex-wrap items-center gap-1">
                                    <span>{move || calculation.definition(key).map(|d| d.label).unwrap_or_default()}</span>
                                    <select class="filter-chip-value max-w-full" data-calculation-input=key
                                        prop:value=move || calculation.value(key)
                                        on:change=move |event| calculation.set(key, event_target_value(&event))>
                                        {move || calculation.definition(key).map(|definition| definition.options.into_iter().map(|(value, label)| view! {
                                            <option value=value selected=move || calculation.value(key) == value>{label}</option>
                                        }).collect_view())}
                                    </select>
                                </label>
                            }.into_any(),
                        }}
                        {match (term.place_select, term.place) {
                            (Some(place), _) => Some(view! {
                                <span class="inline-flex max-w-full items-center gap-1" data-testid="analyzer-price-scope">
                                    <span class="filter-chip-label">"·"</span>
                                    <select class="filter-chip-value max-w-full"
                                        aria-label=move || t_string!(i18n, analyzer_price_scope).to_string()
                                        prop:value=move || place.value.get()
                                        on:change=move |event| place.on_change.run(event_target_value(&event))>
                                        {move || place.options.get().into_iter().map(|(value, label, enabled)| view! {
                                            <option value=value disabled=!enabled selected=move || place.value.get() == value>{label}</option>
                                        }).collect_view()}
                                    </select>
                                </span>
                            }.into_any()),
                            (None, Some(place)) => Some(view! {
                                <span class="filter-chip-label">"· " {move || place.get()}</span>
                            }.into_any()),
                            (None, None) => None,
                        }}
                    </span>
                }
            }).collect_view()}
            <MarketWindowControl window />
        </div>
    }
}

/// Native result headers share the strip's role and selected price label.
pub fn decorate_header(calculation: Option<Calculation>, id: &str, header: AnyView) -> AnyView {
    let Some((calculation, term)) = calculation.and_then(|c| c.term(id).map(|term| (c, term)))
    else {
        return header;
    };
    view! {
        <div class="flex flex-col min-w-0 w-full gap-1" data-calculation-column=id.to_string()>
            <div class="flex items-center gap-1 min-w-0"><TermBadge role=term.role />{header}</div>
            {term.key.map(|key| view! {
                <span class="text-[10px] font-normal text-[color:var(--color-text-muted)] truncate" title=move || calculation.selected_label(key)>{move || calculation.selected_label(key)}</span>
            })}
        </div>
    }.into_any()
}

/// A signal shortcut only changes the matching input's pricing methodology.
/// It does not claim that a single ingredient statistic equals total craft cost.
#[component]
pub fn UsePriceSignal(
    calculation: Calculation,
    key: &'static str,
    value: &'static str,
) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let aria = move || {
        calculation
            .definition(key)
            .map(|definition| {
                let signal = definition
                    .options
                    .iter()
                    .find(|(token, _)| *token == value)
                    .map(|(_, label)| label.as_str())
                    .unwrap_or_default();
                t_string!(
                    i18n,
                    calculation_use_signal,
                    signal = signal,
                    input = definition.label.as_str()
                )
                .to_string()
            })
            .unwrap_or_default()
    };
    view! {
        <button type="button" class="inline-flex items-center gap-1 rounded-full border border-[color:var(--color-outline)] px-2 py-1 text-[10px] disabled:opacity-60"
            data-use-price-signal=value aria-label=aria title=aria
            aria-pressed=move || (calculation.value(key) == value).to_string()
            disabled=move || calculation.value(key) == value
            on:click=move |event| { event.prevent_default(); event.stop_propagation(); calculation.set(key, value.to_string()); }>
            {t!(i18n, analyzer_use_pill)}
        </button>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer_kit::{filters::price_control, stat_columns::Window};

    #[test]
    fn formula_renders_once_and_tracks_window_without_reading_grid_columns() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let selected = RwSignal::new(Window::D7);
            let mut window = MarketWindow::new(Window::D7, &Window::ALL);
            window.selected = Memo::new(move |_| selected.get());
            let registry = FilterRegistry::provide(Vec::new(), Signal::derive(move || vec![
                price_control("revenue", "Sale estimate".into(), window, "Cheapest listing".into()),
            ]));
            let calculation = Calculation::provide(registry, vec![
                CalculationTerm::fixed(TermRole::Result, "Profit".into(), Some("profit")),
                CalculationTerm::input(TermRole::Revenue, "revenue", "price"),
            ], Some("revenue"));
            // The grid reads the selected assumption while resolving its
            // columns. Looking the assumption up through registry.entries()
            // would create a reactive cycle once these columns are registered.
            let columns = Memo::new(move |_| vec![crate::components::virtual_grid::GridColumn::new(
                "price", calculation.selected_label("revenue"), 150.0, false, true,
            )]);
            registry.register(columns.into());
            assert_eq!(columns.get()[0].label, "Cheapest listing");
            let html = view! { <CalculationStrip calculation window /> }.to_html();
            assert_eq!(html.matches("data-calculation-input=\"revenue\"").count(), 1);
            assert!(html.contains("term-badge-result"));
            assert!(html.contains("term-badge-add"));
            assert!(html.contains("Sale median (7d)"));
            let chips = view! { <crate::components::virtual_grid::registry::RegisteredFilterChips registry /> }.to_html();
            assert!(!chips.contains("data-registered-filter"));
            let menu = view! { <crate::components::virtual_grid::registry::RegisteredFilterMenu registry on_select=Callback::new(|_| {}) /> }.to_html();
            assert!(!menu.contains("data-add-filter=\"revenue\""));
            selected.set(Window::D30);
            let html = view! { <CalculationStrip calculation window /> }.to_html();
            assert!(html.contains("Sale median (30d)"));
            assert!(!html.contains("Sale median (7d)"));
        });
    }

    #[test]
    fn a_place_select_renders_inside_its_term_chip() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let window = MarketWindow::new(Window::D7, &Window::ALL);
            let registry = FilterRegistry::provide(Vec::new(), Signal::derive(Vec::new));
            let place = CalculationPlace {
                value: Signal::derive(|| "datacenter".to_string()),
                options: Signal::derive(|| {
                    vec![
                        ("world", "Gilgamesh".to_string(), true),
                        ("datacenter", "Aether".to_string(), true),
                        ("region", "North-America".to_string(), false),
                    ]
                }),
                on_change: Callback::new(|_: String| {}),
            };
            let calculation = Calculation::provide(
                registry,
                vec![
                    CalculationTerm::fixed(TermRole::Revenue, "Vendor price".into(), None)
                        .with_place(Signal::derive(|| "Aether".to_string())),
                    CalculationTerm::fixed(TermRole::Cost, "Listing".into(), Some("listing"))
                        .with_place_select(place),
                ],
                None,
            );
            let html = view! { <CalculationStrip calculation window /> }.to_html();
            // One picker, on the Listing chip, after its label.
            assert_eq!(
                html.matches("data-testid=\"analyzer-price-scope\"").count(),
                1
            );
            assert!(html.find("Listing").unwrap() < html.find("analyzer-price-scope").unwrap());
            assert!(html.contains("Gilgamesh") && html.contains("North-America"));
            assert!(html.contains("disabled"));
            // The read-only place prints beside Vendor price, before the
            // picker's own "Aether" option.
            let vendor = html.find("Vendor price").unwrap();
            let label = vendor + html[vendor..].find("Aether").unwrap();
            assert!(label < html.find("analyzer-price-scope").unwrap());
        });
    }
}
