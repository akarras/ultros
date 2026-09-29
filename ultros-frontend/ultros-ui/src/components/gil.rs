use leptos::prelude::*;
use thousands::Separable;

/// Currency marker: prices must not add nested buttons or tab stops.
#[component]
pub fn GilIcon() -> impl IntoView {
    view! { <img src="/static/images/gil.webp" alt="gil" class="h-5 w-5" /> }
}

#[component]
pub fn Gil(#[prop(into)] amount: Signal<i32>) -> impl IntoView {
    view! {
        <div class="flex flex-row items-center">
            <GilIcon />
            <div>{move || amount().separate_with_commas()}</div>
        </div>
    }
}

/// Visibility class for the gil icon. The icon is hidden rather than
/// removed so the element shape stays constant - see [`GilOrDash`].
fn get_gil_or_dash_icon_class(has_amount: bool) -> &'static str {
    if has_amount { "inline-flex" } else { "hidden" }
}

/// Class for the value slot - muted while the amount is unknown.
fn get_gil_or_dash_value_class(has_amount: bool) -> &'static str {
    if has_amount {
        ""
    } else {
        "text-[color:var(--color-text-muted)]"
    }
}

/// The gil amount with thousands separators, or the placeholder.
fn format_gil_or_dash(amount: Option<i32>) -> String {
    amount
        .map(|t| t.separate_with_commas())
        .unwrap_or_else(|| "—".to_string())
}

/// Render a gil amount when present, falling back to an em-dash placeholder
/// when `amount` is `None` — without changing the element shape.
///
/// Switching between `<Gil>` (`<div><button/><div/></div>`) and a bare
/// `<span>"—"</span>` via `into_any()` triggered tachys hydration mismatches
/// at `hydration.rs:163` (`failed_to_cast_element`) on `/items/jobset/<JOB>`:
/// if the server resolved the cheapest-listings resource and rendered a
/// `<Gil>` but the client briefly evaluated the `None` arm (or vice versa),
/// the dynamic-block child had a different root tag than the SSR DOM and the
/// hydration walker panicked. Always emitting the same `<div>` + icon + value
/// shape removes that class of mismatch entirely — the icon is just hidden
/// via CSS when the value is unknown, so the SSR and CSR view trees agree on
/// element types/positions regardless of resource state.
#[component]
pub fn GilOrDash(#[prop(into)] amount: Signal<Option<i32>>) -> impl IntoView {
    let icon_class = move || get_gil_or_dash_icon_class(amount().is_some());
    let value_class = move || get_gil_or_dash_value_class(amount().is_some());
    view! {
        <div class="flex flex-row items-center">
            <span class=icon_class>
                <GilIcon />
            </span>
            <div class=value_class>
                {move || format_gil_or_dash(amount())}
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_gil_or_dash_icon_class() {
        assert_eq!(get_gil_or_dash_icon_class(true), "inline-flex");
        assert_eq!(get_gil_or_dash_icon_class(false), "hidden");
    }

    #[test]
    fn test_get_gil_or_dash_value_class() {
        assert_eq!(get_gil_or_dash_value_class(true), "");
        assert_eq!(
            get_gil_or_dash_value_class(false),
            "text-[color:var(--color-text-muted)]"
        );
    }

    #[test]
    fn test_format_gil_or_dash() {
        assert_eq!(format_gil_or_dash(Some(1234)), "1,234");
        assert_eq!(format_gil_or_dash(None), "—");
        assert_eq!(format_gil_or_dash(Some(0)), "0");
        assert_eq!(format_gil_or_dash(Some(-50)), "-50");
    }
}
