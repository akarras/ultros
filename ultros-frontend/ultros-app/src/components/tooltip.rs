use leptos::*;

#[component]
pub fn Tooltip(tooltip_text: String, children: Box<dyn Fn() -> Fragment>) -> impl IntoView {
    view! {

        <div class="tooltip">
            {children()}
            {(!tooltip_text.is_empty()).then(|| {
                view!{<div class="top">{tooltip_text}</div>}
            })}
        </div>
    }
}