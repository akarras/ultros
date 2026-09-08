//! Persistent "Ultros has been updated" banner. Shown once
//! `AppUpdate::pending` is set (see `global_state::app_update`) until the
//! user reloads or dismisses it. A dedicated element rather than a toast:
//! toasts auto-dismiss and have no action button.

use crate::components::icon::Icon;
use crate::global_state::app_update::use_app_update;
use crate::i18n::{t, t_string, use_i18n};
use icondata as i;
use leptos::prelude::*;

/// Full page reload. No-op on the server.
pub(crate) fn reload_page() {
    #[cfg(not(feature = "ssr"))]
    {
        let _ = window().location().reload();
    }
}

#[component]
pub fn UpdateBanner() -> impl IntoView {
    let i18n = use_i18n();
    let update = use_app_update();
    let visible = move || update.is_some_and(|u| u.banner_visible());
    let dismiss = move |_| {
        if let Some(update) = update {
            update.dismissed.set(true);
        }
    };

    view! {
        <Show when=visible>
            <div
                role="status"
                aria-live="polite"
                class="fixed bottom-0 inset-x-0 sm:inset-x-auto sm:left-4 sm:bottom-4 z-[90] flex items-center gap-3 p-4 sm:rounded-lg shadow-lg border text-sm bg-[color:var(--color-background-elevated)] border-[color:var(--color-outline)] text-[color:var(--color-text)]"
            >
                <Icon icon=i::BsArrowClockwise width="1.2em" height="1.2em" aria_hidden=true />
                <span class="flex-1">{t!(i18n, update_banner_message)}</span>
                <button class="btn-primary" on:click=move |_| reload_page()>
                    {t!(i18n, update_banner_reload)}
                </button>
                <button
                    class="opacity-70 hover:opacity-100 transition-opacity focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--brand-ring)] rounded"
                    aria-label=t_string!(i18n, update_banner_dismiss)
                    on:click=dismiss
                >
                    <Icon icon=i::BsX width="1.2em" height="1.2em" aria_hidden=true />
                </button>
            </div>
        </Show>
    }
}
