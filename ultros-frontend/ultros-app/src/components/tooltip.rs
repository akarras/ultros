use leptos::*;

#[component]
pub fn Tooltip(
    cx: Scope,
    tooltip_text: ReadSignal<String>,
    children: Box<dyn Fn(Scope) -> Fragment>,
) -> impl IntoView {
    view! {
        cx,
        <div class="tooltip">
            {children(cx)
                .nodes
                .into_iter()
                .map(|child| view! { cx, <li>{child}</li> })
                .collect::<Vec<_>>()}
            <div class="tooltip-text">{move || tooltip_text()}</div>
        </div>
    }
}
