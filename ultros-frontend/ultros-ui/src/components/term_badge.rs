//! The formula's own arithmetic as a legend: `=` result, `+` revenue,
//! `−` tax, `−` cost. Palette-safe (brand tokens only) and readable by
//! screen readers through an sr-only role name.
//!
//! Role is encoded by fill weight, never by hue: solid for the result,
//! brand-tinted for what adds, neutral for what subtracts. Hue is not
//! available as a channel here — the app ships 18 brand palettes across a
//! light and a dark theme, and a fixed green/red pair would clash with most
//! of them. Legibility comes from contrast instead, and the operator shape
//! still carries the meaning on its own.

use leptos::prelude::*;

use crate::i18n::*;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TermRole {
    Result,
    Revenue,
    Tax,
    Cost,
}

impl TermRole {
    fn chip_class(self) -> &'static str {
        match self {
            TermRole::Result => "term-badge term-badge-result",
            TermRole::Revenue => "term-badge term-badge-add",
            TermRole::Tax | TermRole::Cost => "term-badge term-badge-sub",
        }
    }

    /// The operator as `<path d=…>` data on a 12×12 grid. Stroked rather
    /// than typed: a 10px `font-mono` `−` or `=` is a 1px hairline that
    /// washes out at this size and depends on whichever mono face the
    /// browser falls back to, while a round-capped stroke renders the same
    /// everywhere and scales with the chip.
    fn strokes(self) -> &'static [&'static str] {
        match self {
            TermRole::Result => &["M2.5 4.5h7", "M2.5 7.5h7"],
            TermRole::Revenue => &["M6 2.5v7", "M2.5 6h7"],
            TermRole::Tax | TermRole::Cost => &["M2.5 6h7"],
        }
    }
}

#[component]
pub fn TermBadge(role: TermRole) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let name = move || match role {
        TermRole::Result => t_string!(i18n, formula_role_result).to_string(),
        TermRole::Revenue => t_string!(i18n, formula_role_revenue).to_string(),
        TermRole::Tax => t_string!(i18n, formula_role_tax).to_string(),
        TermRole::Cost => t_string!(i18n, formula_role_cost).to_string(),
    };
    view! {
        <span class=role.chip_class()>
            <svg
                aria-hidden="true"
                viewBox="0 0 12 12"
                fill="none"
                stroke="currentColor"
                stroke-width="1.75"
                stroke-linecap="round"
            >
                {role
                    .strokes()
                    .iter()
                    .map(|d| view! { <path d=*d /> })
                    .collect_view()}
            </svg>
            <span class="sr-only">{name}</span>
        </span>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `TermBadge` builds an I18nContext (which spawns an Effect), so the
    /// executor and the context both have to be standing before it renders —
    /// same scaffolding as `components/sort_header.rs`'s tests.
    fn render(role: TermRole) -> String {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            view! { <TermBadge role=role /> }.to_html()
        })
    }

    #[test]
    fn each_role_renders_its_own_chip_and_stroked_operator() {
        for (role, class, strokes) in [
            (TermRole::Result, "term-badge-result", 2),
            (TermRole::Revenue, "term-badge-add", 2),
            (TermRole::Cost, "term-badge-sub", 1),
            (TermRole::Tax, "term-badge-sub", 1),
        ] {
            let html = render(role);
            assert!(html.contains(class), "{role:?}: {html}");
            // Stroked, not typed: no font-size class, and one <path> per
            // stroke of the operator.
            assert_eq!(html.matches("<path").count(), strokes, "{role:?}: {html}");
            assert!(html.contains("aria-hidden=\"true\""), "{role:?}: {html}");
            assert!(html.contains("sr-only"), "{role:?}: {html}");
        }
    }

    #[test]
    fn add_and_subtract_read_apart_without_a_hue() {
        // The two chips a user has to tell apart at a glance differ in fill
        // weight, so the distinction survives all 18 palettes and both
        // themes. If these ever collapse to the same class the badges are
        // relying on the glyph alone.
        assert_ne!(
            TermRole::Revenue.chip_class(),
            TermRole::Cost.chip_class(),
            "revenue and cost must not share a chip"
        );
    }
}
