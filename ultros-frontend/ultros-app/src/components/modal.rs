use std::rc::Rc;

use leptos::*;
use leptos_animation::*;
use leptos_use::use_window_scroll;

#[component]
pub fn Modal(
    children: Rc<dyn Fn() -> Fragment>,
    #[prop(into)] set_visible: SignalSetter<bool>,
) -> impl IntoView {
    let (_x, y) = use_window_scroll();
    let y = create_animated_signal(move || y.get().into(), tween_default);
    view! {
        <Portal>
            <div class="absolute top-0 bottom-0 left-0 right-0 bg-neutral-950 bg-opacity-25 z-40" on:click=move |_| set_visible(false)>
                <div class="flex flex-col mx-auto from-black to-violet-950 bg-gradient-to-br p-10 left-0 right-0 xl:w-[500px] w-screen z-50 rounded-xl shadow-md" style=move || format!("margin-top: {}px", y() + 50.0) on:click=move |e| {
                    e.stop_propagation();
                }>
                    <div class="self-end ml-auto cursor-pointer hover:text-neutral-200" on:click=move |_| set_visible(false) on:focusout=move |_| set_visible(false)>"CLOSE"</div>
                    {children()}
                </div>
            </div>
        </Portal>
    }
}
