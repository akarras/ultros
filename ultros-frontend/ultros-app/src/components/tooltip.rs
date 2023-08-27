use leptos::*;

#[component]
pub fn Tooltip(tooltip_text: String, children: Box<dyn Fn() -> Fragment>) -> impl IntoView {
    view! {

        <div class="tooltip">
            {children()}
            <div class="top">{tooltip_text}</div>
        </div>
    }
}
