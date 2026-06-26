use crate::components::tooltip::Tooltip;
use crate::i18n::{t_string, use_i18n};
use chrono::{DateTime, Utc};
use leptos::prelude::*;

#[component]
pub fn RealtimeStatus(
    #[prop(into)] status: Signal<String>,
    #[prop(into)] last_update: Signal<Option<DateTime<Utc>>>,
) -> impl IntoView {
    let i18n = use_i18n();
    #[allow(unused_variables)]
    let (clock_tick, set_clock_tick) = signal(0_u32);

    #[cfg(not(feature = "ssr"))]
    {
        use gloo_timers::callback::Interval;
        let interval = Interval::new(1_000, move || {
            set_clock_tick.update(|n| *n = n.wrapping_add(1));
        });
        interval.forget();
    }

    let updated_label = Signal::derive(move || {
        let _ = clock_tick.get();
        let Some(t) = last_update.get() else {
            return String::new();
        };
        let now = Utc::now();
        let secs = now.signed_duration_since(t).num_seconds().max(0);
        if secs < 2 {
            t_string!(i18n, list_view_updated_just_now).to_string()
        } else {
            t_string!(i18n, list_view_updated_seconds_ago, seconds = secs).to_string()
        }
    });

    move || {
        let status_key = status.get();
        let (dot_class, label_key): (&'static str, &'static str) = match status_key.as_str() {
            "live" => ("bg-green-400", "list_view_live_status_live"),
            "reconnecting" => (
                "bg-amber-400 animate-pulse",
                "list_view_live_status_reconnecting",
            ),
            "offline" => ("bg-gray-500", "list_view_live_status_offline"),
            _ => (
                "bg-amber-400 animate-pulse",
                "list_view_live_status_connecting",
            ),
        };

        let status_label = match label_key {
            "list_view_live_status_live" => t_string!(i18n, list_view_live_status_live).to_string(),
            "list_view_live_status_reconnecting" => {
                t_string!(i18n, list_view_live_status_reconnecting).to_string()
            }
            "list_view_live_status_offline" => {
                t_string!(i18n, list_view_live_status_offline).to_string()
            }
            _ => t_string!(i18n, list_view_live_status_connecting).to_string(),
        };

        let status_key_for_view = status_key.clone();
        let status_label_clone = status_label.clone();
        let tooltip_text = Signal::derive(move || {
            let updated = updated_label.get();
            if updated.is_empty() {
                status_label_clone.clone()
            } else {
                format!("{status_label_clone} · {updated}")
            }
        });
        view! {
            <Tooltip tooltip_text=tooltip_text>
                <span
                    class="inline-flex items-center gap-2 rounded-lg border border-[color:var(--color-outline)] px-2 py-1 text-xs text-[color:var(--color-text-muted)]"
                    data-testid="realtime-status-indicator"
                    data-status=status_key_for_view.clone()
                >
                    <span class="relative flex h-2 w-2">
                        {if status_key == "live" {
                            view! {
                                <span class="animate-ping absolute inline-flex h-full w-full rounded-full bg-green-400 opacity-75"></span>
                            }
                                .into_any()
                        } else {
                            ().into_any()
                        }}
                        <span class=format!("relative inline-flex rounded-full h-2 w-2 {}", dot_class)></span>
                    </span>
                    <span>{status_label.clone()}</span>
                </span>
            </Tooltip>
        }
    }
}
