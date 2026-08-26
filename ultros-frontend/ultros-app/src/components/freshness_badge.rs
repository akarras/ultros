use crate::freshness::get_freshness_verdict_display;
use crate::i18n_fallback::use_i18n_or_default;
use chrono::Duration;
use leptos::prelude::*;
use ultros_api_types::freshness::FreshnessVerdict;

#[component]
pub fn FreshnessBadge(
    verdict: FreshnessVerdict,
    age: Option<Duration>,
    #[prop(optional)] compact: bool,
) -> impl IntoView {
    let i18n = use_i18n_or_default();
    let display = get_freshness_verdict_display(verdict, age);

    view! {
        <span
            title=display.tooltip(i18n)
            class=move || {
                let padding = if compact { "px-1.5" } else { "px-2" };
                format!("inline-flex items-center py-0.5 rounded-full text-xs font-semibold border {} {}", padding, display.tone.css_classes())
            }
        >
            {display.format_label(i18n)}
        </span>
    }
}
