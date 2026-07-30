use leptos::prelude::*;

use crate::api::get_login;
use crate::components::alert_rules_panel::AlertRulesPanel;
use crate::components::endpoints_panel::EndpointsPanel;
use crate::components::history_panel::HistoryPanel;
use crate::components::loading::Loading;
use crate::components::meta::{MetaDescription, MetaRobotsNoIndex, MetaTitle};
use crate::components::tool_help::ActionableEmptyState;
use crate::i18n::{t, t_string, use_i18n};

#[component]
pub fn Alerts() -> impl IntoView {
    let i18n = use_i18n();
    let login = Resource::new(|| (), |_| async move { get_login().await });
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
        <MetaTitle title=move || t_string!(i18n, alerts_meta_title).to_string() />
        <MetaDescription text=move || t_string!(i18n, alerts_meta_desc).to_string() />
        <MetaRobotsNoIndex />
        <div class="p-4 space-y-6">
            <h1 class="text-2xl font-bold">{t!(i18n, alerts_page_heading)}</h1>

            <Suspense fallback=move || view! { <Loading /> }>
                {move || match login.get() {
                    None => view! { <Loading /> }.into_any(),
                    Some(Err(_)) => {
                        view! {
                            <ActionableEmptyState
                                title=t!(i18n, alerts_empty_title)
                                body=t!(i18n, alerts_empty_body)
                                action_href="/login?next=/alerts"
                                action_label=t!(i18n, sign_in_discord)
                                action_external=true
                            />
                        }.into_any()
                    }
                    Some(Ok(_)) => {
                        view! {
                            <div class="rounded-lg border border-brand-500/30 bg-brand-500/5 p-4 flex flex-col gap-2 animate-fade-in">
                                <p class="font-semibold text-brand-200">{t!(i18n, alerts_prefer_discord_alerts)}</p>
                                <p class="text-sm text-[color:var(--color-text-muted)]">
                                    "Run "
                                    <code class="rounded bg-black/40 px-1.5 py-0.5">"/ffxiv retainer add_undercut_alert"</code>
                                    " in any channel where the bot is installed. "
                                    <a href="/bot#getting-started" class="text-brand-300 underline hover:text-brand-200">
                                        "See the bot guide →"
                                    </a>
                                </p>
                            </div>

                            <div class="flex gap-2 mt-4">
                                {tab_btn("endpoints", t_string!(i18n, alerts_tab_endpoints).to_string())}
                                {tab_btn("rules", t_string!(i18n, alerts_tab_rules).to_string())}
                                {tab_btn("history", t_string!(i18n, alerts_tab_history).to_string())}
                            </div>
                            <div class="mt-4">
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
                        }.into_any()
                    }
                }}
            </Suspense>
        </div>
    }
}
