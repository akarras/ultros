//! The formula ledger as a row of chips: `[=] Profit / unit  [+] revenue
//! · place  [−] 5% tax  [−] cost · place`. A term is fixed (static chip)
//! or selectable (a native `<select>` inside the chip writing one URL
//! param). Inline for the row under "Sell on"; Stacked for popovers.

use leptos::prelude::*;

use crate::components::term_badge::{TermBadge, TermRole};
use crate::i18n::*;

pub struct StripSelect {
    pub value: Signal<String>,
    pub options: Vec<(&'static str, String)>,
    pub on_change: Callback<String>,
    pub aria: String,
}

pub struct StripTerm {
    pub role: TermRole,
    pub label: Signal<String>,
    /// "· Gilgamesh" / "· Aether".
    pub place: Option<Signal<String>>,
    pub select: Option<StripSelect>,
    /// A second select for the place (Buy from).
    pub place_select: Option<StripSelect>,
    /// Show the amber dot: the numbers fell back to the listing.
    pub degraded: Signal<bool>,
}

impl StripTerm {
    pub fn fixed(role: TermRole, label: Signal<String>) -> Self {
        Self {
            role,
            label,
            place: None,
            select: None,
            place_select: None,
            degraded: Signal::derive(|| false),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum StripLayout {
    Inline,
    Stacked,
}

fn select_view(s: StripSelect) -> AnyView {
    let StripSelect {
        value,
        options,
        on_change,
        aria,
    } = s;
    view! {
        <select
            class="filter-chip-value"
            aria-label=aria
            prop:value=move || value.get()
            on:change=move |ev| on_change.run(event_target_value(&ev))
        >
            {options.into_iter().map(|(val, lab)| view! {
                <option value=val selected=move || value.get() == val>{lab}</option>
            }).collect_view()}
        </select>
    }
    .into_any()
}

#[component]
pub fn FormulaStrip(terms: Vec<StripTerm>, layout: StripLayout) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let container = match layout {
        StripLayout::Inline => "flex flex-wrap items-center gap-2",
        StripLayout::Stacked => "flex flex-col items-stretch gap-1.5",
    };
    view! {
        <div class=container>
            {terms.into_iter().map(|term| {
                let chip_class = if term.select.is_some() { "filter-chip" } else { "filter-chip bg-transparent" };
                let degraded = term.degraded;
                view! {
                    <span class=chip_class>
                        <TermBadge role=term.role />
                        {match term.select {
                            Some(s) => select_view(s),
                            None => view! { <span>{move || term.label.get()}</span> }.into_any(),
                        }}
                        {term.place.map(|p| view! { <span class="filter-chip-label">"· " {move || p.get()}</span> })}
                        {term.place_select.map(select_view)}
                        <span
                            class=move || if degraded.get() { "inline-block w-1.5 h-1.5 rounded-full bg-amber-300" } else { "hidden" }
                            role="img"
                            title=move || degraded.get().then(|| t_string!(i18n, formula_degraded_listing_fallback).to_string())
                            aria-label=move || degraded.get().then(|| t_string!(i18n, formula_degraded_listing_fallback).to_string())
                        ></span>
                    </span>
                }
            }).collect_view()}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_terms_render_static_chips_and_select_terms_render_selects() {
        // `TermBadge` builds an I18nContext (spawns an Effect) and `<Gil>`
        // reads it: stand up the executor and the context, as
        // components/list/filter_row.rs's tests do.
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let terms = vec![
                StripTerm::fixed(
                    TermRole::Result,
                    Signal::derive(|| "Profit / unit".to_string()),
                ),
                StripTerm {
                    role: TermRole::Revenue,
                    label: Signal::derive(|| "Cheapest listing".to_string()),
                    place: Some(Signal::derive(|| "Gilgamesh".to_string())),
                    select: Some(StripSelect {
                        value: Signal::derive(|| "listing-min".to_string()),
                        options: vec![
                            ("listing-min", "Cheapest listing".into()),
                            ("sale-median", "Sale median (7d)".into()),
                        ],
                        on_change: Callback::new(|_| {}),
                        aria: "Change revenue signal".into(),
                    }),
                    place_select: None,
                    degraded: Signal::derive(|| false),
                },
            ];
            let html = view! { <FormulaStrip terms=terms layout=StripLayout::Inline /> }.to_html();
            assert_eq!(html.matches("<select").count(), 1, "{html}");
            assert!(html.contains("Profit / unit"), "{html}");
            assert!(html.contains("Gilgamesh"), "{html}");
            assert!(
                html.contains("aria-label=\"Change revenue signal\""),
                "{html}"
            );
        });
    }
}
