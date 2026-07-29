use crate::analysis::SalesCadence;
use crate::i18n::*;
use crate::sales_cadence::get_sales_cadence_display;
use leptos::prelude::*;

#[component]
pub fn SalesCadenceBadge(
    cadence: SalesCadence,
    sales_per_day: f32,
    #[prop(optional)] compact: bool,
) -> impl IntoView {
    let i18n = use_i18n();
    let display = get_sales_cadence_display(cadence, sales_per_day);
    let full_label = display.format_label(i18n);
    let text = if compact {
        display.format_compact(i18n)
    } else {
        full_label.clone()
    };

    view! {
        <span
            title=compact.then(|| full_label.clone())
            class=move || {
                let padding = if compact { "px-1.5" } else { "px-2" };
                format!("inline-flex items-center py-0.5 rounded-full text-xs font-semibold border whitespace-nowrap max-w-full overflow-hidden {} {}", padding, display.tone.css_classes())
            }
        >
            {text}
        </span>
    }
}
