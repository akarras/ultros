use leptos::prelude::*;

use crate::components::alert_rules_panel::AlertRulesPanel;
use crate::components::endpoints_panel::EndpointsPanel;
use crate::components::history_panel::HistoryPanel;

#[component]
pub fn Alerts() -> impl IntoView {
    let (tab, set_tab) = signal::<&'static str>("endpoints");

    let tab_btn = move |id: &'static str, label: &'static str| {
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
            <h1 class="text-2xl font-bold">"Notifications"</h1>
            <div class="flex gap-2">
                {tab_btn("endpoints", "Endpoints")}
                {tab_btn("rules", "Alert rules")}
                {tab_btn("history", "History")}
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
