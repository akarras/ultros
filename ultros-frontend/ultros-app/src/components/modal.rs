use icondata as i;
use leptos::{portal::Portal, prelude::*, reactive::wrappers::write::SignalSetter};
// use leptos_animation::*;
use leptos_hotkeys::use_hotkeys;
use leptos_icons::*;
#[cfg(feature = "hydrate")]
use leptos_use::use_window_scroll;

#[component]
pub fn Modal<T>(
    children: TypedChildrenFn<T>,
    #[prop(into)] set_visible: SignalSetter<bool>,
) -> impl IntoView
where
    T: Render + RenderHtml + Send + 'static,
{
    #[cfg(feature = "hydrate")]
    let (_x, _y) = use_window_scroll();
    // let y = create_animated_signal(move || y.get().into(), tween_default);

    use_hotkeys!(("escape") => move |_| {
        set_visible(false);
    });
    let children = children.into_inner();
    view! {
        <Portal>
            <div
                class="fixed inset-0 z-40 bg-[color:color-mix(in_srgb,_var(--color-text)_40%,_var(--color-background))] flex items-start sm:items-center justify-center p-6
                transition-opacity duration-300 ease-in-out
                animate-fade-in"
                on:click=move |_| set_visible(false)
            >
                <div
                    class="flex flex-col mx-auto max-w-2xl w-[95%] sm:w-[500px]
                    panel rounded-2xl shadow-xl
                    backdrop-blur-md
                    p-6 z-50
                    animate-slide-in"

                    on:click=move |e| {
                        e.stop_propagation();
                    }
                >
                    <div class="flex justify-end mb-2">
                        <button
                            class="p-2 rounded-lg hover:bg-[color:color-mix(in_srgb,_var(--brand-ring)_20%,_transparent)]
                            text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]
                            transition-colors duration-200
                            focus:outline-none focus:ring-2 focus:ring-[color:var(--brand-ring)]"
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
