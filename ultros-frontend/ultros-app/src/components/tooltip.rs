use leptos::*;

#[component]
pub fn Tooltip(
    #[prop(into)] tooltip_text: MaybeSignal<Oco<'static, str>>,
    children: Box<dyn Fn() -> Fragment>,
) -> impl IntoView {
    view! {

        <div class="tooltip">
            {children()}
            <div class="top">{move || tooltip_text().to_string()}</div>
        </div>
    }
}
