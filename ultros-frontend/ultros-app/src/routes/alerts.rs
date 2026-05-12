use leptos::prelude::*;

use crate::components::alert_rules_panel::AlertRulesPanel;
use crate::components::endpoints_panel::EndpointsPanel;
use crate::components::history_panel::HistoryPanel;
use crate::i18n::{t, t_string, use_i18n};

#[component]
pub fn Alerts() -> impl IntoView {
    let i18n = use_i18n();
    let (tab, set_tab) = signal::<&'static str>("endpoints");

    let tab_btn = move |id: &'static str, label: String| {
        view! {
            <button
                class=move || if tab.get() == id { "btn" } else { "btn-ghost" }
                on:click=move |_| set_tab.set(id)
            >
                {label}
            </button>
        }
    };

    view! {
        <div class="p-4 space-y-6">
            <h1 class="text-2xl font-bold">{t!(i18n, alerts_page_heading)}</h1>

            <div class="rounded-lg border border-brand-500/30 bg-brand-500/5 p-4 flex flex-col gap-2">
                <p class="font-semibold text-brand-200">"Prefer Discord-native alerts?"</p>
                <p class="text-sm text-[color:var(--color-text-muted)]">
                    "Run "
                    <code class="rounded bg-black/40 px-1.5 py-0.5">"/ffxiv retainer add_undercut_alert"</code>
                    " in any channel where the bot is installed. "
                    <a href="/bot#getting-started" class="text-brand-300 underline hover:text-brand-200">
                        "See the bot guide →"
                    </a>
                </p>
            </div>

            <div class="flex gap-2">
                {tab_btn("endpoints", t_string!(i18n, alerts_tab_endpoints).to_string())}
                {tab_btn("rules", t_string!(i18n, alerts_tab_rules).to_string())}
                {tab_btn("history", t_string!(i18n, alerts_tab_history).to_string())}
            </div>
            <div>
                <Show when=move || tab.get() == "endpoints">
                    <EndpointsPanel />
                </Show>
                <Show when=move || tab.get() == "rules">
                    <AlertRulesPanel />
                </Show>
                <Show when=move || tab.get() == "history">
                    <HistoryPanel />
                </Show>
            </div>
        </div>
    }
}
