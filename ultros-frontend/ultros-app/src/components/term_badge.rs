//! The formula's own arithmetic as a legend: `=` result, `+` revenue,
//! `−` tax, `−` cost. Palette-safe (brand tokens only) and readable by
//! screen readers through an sr-only role name.

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
    pub fn glyph(self) -> &'static str {
        match self {
            TermRole::Result => "=",
            TermRole::Revenue => "+",
            TermRole::Tax | TermRole::Cost => "−",
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
        <span class="inline-flex items-center justify-center w-4 h-4 rounded border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_20%,transparent)] text-[color:var(--brand-fg)] font-mono text-[10px] font-bold leading-none shrink-0">
            <span aria-hidden="true">{role.glyph()}</span>
            <span class="sr-only">{name}</span>
        </span>
    }
}
