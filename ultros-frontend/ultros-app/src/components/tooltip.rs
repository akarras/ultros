use leptos::*;

#[component]
pub fn Tooltip(
    cx: Scope,
    tooltip_text: String,
    children: Box<dyn Fn(Scope) -> Fragment>,
) -> impl IntoView {
    view! {
        cx,
        <div class="tooltip">
            {children(cx)}
            <div class="tooltip-text">{tooltip_text}</div>
        </div>
    }
}
