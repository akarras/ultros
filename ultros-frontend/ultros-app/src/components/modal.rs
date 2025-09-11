use icondata as i;
use leptos::{portal::Portal, prelude::*, reactive::wrappers::write::SignalSetter};
// use leptos_animation::*;
use leptos_hotkeys::use_hotkeys;
use leptos_icons::*;
use leptos_use::use_window_scroll;

#[component]
pub fn Modal<T>(
    children: TypedChildrenFn<T>,
    #[prop(into)] set_visible: SignalSetter<bool>,
) -> impl IntoView
where
    T: Render + RenderHtml + Send + 'static,
{
    let (_x, y) = use_window_scroll();
    // let y = create_animated_signal(move || y.get().into(), tween_default);

    use_hotkeys!(("escape") => move |_| {
        set_visible(false);
    });
    let children = children.into_inner();
    view! {
        <Portal>
            <div
                class="fixed inset-0 bg-black/60  z-40
                transition-opacity duration-300 ease-in-out
                animate-fade-in"
                on:click=move |_| set_visible(false)
            >
                <div
                    class="flex flex-col mx-auto max-w-2xl w-[95%] sm:w-[500px]
                    bg-gradient-to-br from-violet-950/70 via-black/60 to-violet-950/70
                    border border-white/10
                    rounded-2xl shadow-xl shadow-black/40
                    backdrop-blur-md
                    p-6 z-50
                    animate-slide-in"
                    style=move || format!("margin-top: {}px", y() + 50.0)
                    on:click=move |e| {
                        e.stop_propagation();
                    }
                >
                    <div class="flex justify-end mb-2">
                        <button
                            class="p-2 rounded-lg hover:bg-violet-800/20
                            text-gray-400 hover:text-violet-300
                            transition-colors duration-200
                            focus:outline-none focus:ring-2 focus:ring-violet-500/40"
                            on:click=move |_| set_visible(false)
                            on:focusout=move |_| set_visible(false)
                            aria-label="Close modal"
                        >
                            <Icon icon=i::CgClose width="1.5em" height="1.5em" />
                        </button>
                    </div>

                    <div class="relative">{children().into_view()}</div>
                </div>
            </div>
        </Portal>
    }
    .into_any()
}
