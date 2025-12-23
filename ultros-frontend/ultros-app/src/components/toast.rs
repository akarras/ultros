use crate::components::icon::Icon;
use crate::global_state::toasts::{Toast, ToastLevel, use_toast};
use icondata as i;
use leptos::leptos_dom::helpers::set_timeout;
use leptos::prelude::*;

#[component]
pub fn ToastItem(toast: Toast) -> impl IntoView {
    let toasts = use_toast().expect("Toast context not found");
    let (is_exiting, set_is_exiting) = signal(false);

    let base_class = "flex items-center gap-3 w-full max-w-sm p-4 rounded-lg shadow-lg border text-sm animate-in slide-in-from-bottom-2 fade-in duration-300";
    let color_class = match toast.level {
        ToastLevel::Info => {
            "bg-[color:var(--color-background-elevated)] border-[color:var(--color-outline)] text-[color:var(--color-text)]"
        }
        ToastLevel::Success => "bg-green-500/10 border-green-500/20 text-green-400",
        ToastLevel::Warning => "bg-yellow-500/10 border-yellow-500/20 text-yellow-400",
        ToastLevel::Error => "bg-red-500/10 border-red-500/20 text-red-400",
    };

    let icon = match toast.level {
        ToastLevel::Info => i::BsInfoCircle,
        ToastLevel::Success => i::BsCheckCircle,
        ToastLevel::Warning => i::BsExclamationTriangle,
        ToastLevel::Error => i::BsExclamationCircle,
    };

    let exit_class = move || {
        if is_exiting() {
            "animate-out slide-out-to-right fade-out duration-300"
        } else {
            ""
        }
    };

    let message = toast.message.clone();
    let id = toast.id;

    view! {
        <div
            class=move || format!("{} {} {}", base_class, color_class, exit_class())
            role="alert"
        >
            <Icon icon width="1.2em" height="1.2em" />
            <div class="flex-1">{message}</div>
            <button
                class="opacity-70 hover:opacity-100 transition-opacity"
                aria-label="Close"
                on:click=move |_| {
                    set_is_exiting(true);
                    set_timeout(move || {
                        toasts.remove(id);
                    }, std::time::Duration::from_millis(300));
                }
            >
                <Icon icon=i::BsX width="1.2em" height="1.2em" />
            </button>
        </div>
    }
}

#[component]
pub fn ToastContainer() -> impl IntoView {
    let toasts = use_toast();

    view! {
        <div class="fixed bottom-0 right-0 p-4 sm:p-6 z-[100] flex flex-col gap-2 pointer-events-none">
            <div class="flex flex-col gap-2 items-end pointer-events-auto">
                <Show when=move || toasts.is_some()>
                    <For
                        each=move || toasts.unwrap().0.get()
                        key=|toast| toast.id
                        children=|toast| view! { <ToastItem toast /> }
                    />
                </Show>
            </div>
        </div>
    }
}
