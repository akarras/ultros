use crate::components::{dialog::DialogSurface, icon::Icon};
use crate::i18n::{t_string, use_i18n};
use icondata as i;
use leptos::{portal::Portal, prelude::*, reactive::wrappers::write::SignalSetter};

#[component]
pub fn Modal<T>(
    children: TypedChildrenFn<T>,
    #[prop(into)] set_visible: SignalSetter<bool>,
    #[prop(optional, into)] max_width: Option<String>,
    #[prop(optional, into)] aria_label: Option<Signal<String>>,
) -> impl IntoView
where
    T: Render + RenderHtml + Send + 'static,
{
    let i18n = use_i18n();
    let children = StoredValue::new(children.into_inner());
    let max_width =
        StoredValue::new(max_width.unwrap_or_else(|| "max-w-2xl w-[95%] sm:w-[500px]".to_string()));
    view! {
        <Portal>
            <DialogSurface set_visible=set_visible class="task-dialog" aria_label=aria_label>
            <div
                // `overflow-y-auto` + the panel's `m-auto` (rather than
                // items-center) keep a modal taller than the viewport fully
                // reachable: auto margins center a short panel, and a tall one
                // scrolls with the overlay from its top edge — flex centering
                // would push the top above the viewport with no way to reach
                // it (the list settings drawer's Delete button was unclickable
                // at 900px-tall windows).
                class="fixed inset-0 z-[90] bg-black/60 flex justify-center overflow-y-auto p-3 sm:p-6
                transition-opacity duration-300 ease-in-out
                animate-fade-in"
                on:click=move |_| set_visible(false)
            >
                <div
                    class=format!("flex flex-col min-w-0 m-auto h-fit {max_width}
                    panel rounded-2xl shadow-xl
                    backdrop-blur-md
                    p-4 sm:p-6 z-50
                    animate-slide-in", max_width=max_width.get_value())
                    on:click=move |e| {
                        e.stop_propagation();
                    }
                >
                    <div class="flex justify-end mb-2">
                        <button
                            type="button"
                            class="p-2 rounded-lg hover:bg-[color:color-mix(in_srgb,_var(--brand-ring)_20%,_transparent)]
                            text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]
                            transition-colors duration-200
                            focus:outline-none focus:ring-2 focus:ring-[color:var(--brand-ring)]"
                            on:click=move |_| set_visible(false)
                            aria-label=t_string!(i18n, modal_aria_close)
                        >
                            <Icon icon=i::CgClose width="1.5em" height="1.5em" />
                        </button>
                    </div>

                    <div class="relative">{children.with_value(|children| children().into_view())}</div>
                </div>
            </div>
            </DialogSurface>
        </Portal>
    }
    .into_any()
}
