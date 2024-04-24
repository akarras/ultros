use leptos::{html::Div, *};
use leptos_use::{use_element_bounding, use_window_scroll, UseElementBoundingReturn};

#[component]
pub fn Tooltip(
    #[prop(into)] tooltip_text: MaybeSignal<Oco<'static, str>>,
    children: Box<dyn Fn() -> Fragment>,
) -> impl IntoView {
    let is_hover = create_rw_signal(false);
    let tooltip_text = match tooltip_text {
        MaybeSignal::Static(s) => Signal::derive(move || s.clone()),
        MaybeSignal::Dynamic(d) => d,
    };
    let target = create_node_ref::<Div>();
    let UseElementBoundingReturn { bottom, x, y, .. } = use_element_bounding(target);

    use_window_scroll();

    let tooltip = {
        move || {
            is_hover.get().then(move || {
                let desired_position = move || {
                    let x = x();
                    let mut y = y();
                    if y - 100.0 < 0.0 {
                        y = bottom() + 25.0;
                    } else {
                        y -= 50.0;
                    }
                    format!("top: {}px; left: {}px;", y, x)
                };
                view! {
                    <Portal>
                        <div class="fixed bg-violet-950 rounded-xl p-2 z-50" style=desired_position>
                            {move || tooltip_text().to_string()}
                        </div>
                    </Portal>
                }
            })
        }
    };
    view! {

        <div class="tooltip" on:mouseover=move |_| { is_hover.set(true); } on:mouseout=move |_| { is_hover.set(false); } node_ref=target>
            {children()}
            {tooltip}
        </div>
    }
}
