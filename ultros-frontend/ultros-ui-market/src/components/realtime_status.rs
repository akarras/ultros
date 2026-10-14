use crate::components::tooltip::Tooltip;
use crate::i18n::t_string;
use crate::i18n_fallback::use_i18n_or_default;
use chrono::{DateTime, Utc};
use leptos::prelude::*;

#[derive(Debug, PartialEq, Eq)]
pub struct StatusInfo {
    pub dot_class: &'static str,
    pub label_key: &'static str,
}

pub fn get_status_info(status: &str) -> StatusInfo {
    match status {
        "live" => StatusInfo {
            dot_class: "bg-green-400",
            label_key: "list_view_live_status_live",
        },
        "reconnecting" => StatusInfo {
            dot_class: "bg-amber-400 animate-pulse",
            label_key: "list_view_live_status_reconnecting",
        },
        "offline" => StatusInfo {
            dot_class: "bg-gray-500",
            label_key: "list_view_live_status_offline",
        },
        _ => StatusInfo {
            dot_class: "bg-amber-400 animate-pulse",
            label_key: "list_view_live_status_connecting",
        },
    }
}

#[component]
pub fn RealtimeStatus(
    #[prop(into)] status: Signal<String>,
    #[prop(into)] last_update: Signal<Option<DateTime<Utc>>>,
    /// Use a state symbol below `md` in space-constrained rows, while keeping
    /// the text label available to assistive technology at every width.
    #[prop(optional)]
    compact: bool,
) -> impl IntoView {
    let i18n = use_i18n_or_default();
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

    let status_label = Signal::derive(move || match status.get().as_str() {
        "live" => t_string!(i18n, list_view_live_status_live).to_string(),
        "reconnecting" => t_string!(i18n, list_view_live_status_reconnecting).to_string(),
        "offline" => t_string!(i18n, list_view_live_status_offline).to_string(),
        _ => t_string!(i18n, list_view_live_status_connecting).to_string(),
    });
    let tooltip_text = Signal::derive(move || {
        let updated = updated_label.get();
        if updated.is_empty() {
            status_label.get()
        } else {
            format!("{} · {updated}", status_label.get())
        }
    });
    view! {
        <Tooltip tooltip_text>
            <span class="inline-flex items-center gap-2 rounded-lg border border-[color:var(--color-outline)] px-2 py-1 text-xs text-[color:var(--color-text-muted)]"
                role="status" aria-atomic="true" data-testid="realtime-status-indicator" data-status=move || status.get()>
                <span aria-hidden="true" class=move || format!("relative inline-flex rounded-full h-2 w-2 {}", get_status_info(&status.get()).dot_class)></span>
                {compact.then(|| view! { <span class="md:hidden" aria-hidden="true">{move || match status.get().as_str() { "live" => "✓", "offline" => "×", _ => "↻" }}</span> })}
                <span class=if compact { "sr-only md:not-sr-only" } else { "" }>{move || status_label.get()}</span>
            </span>
        </Tooltip>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_status_info_live() {
        let info = get_status_info("live");
        assert_eq!(info.dot_class, "bg-green-400");
        assert_eq!(info.label_key, "list_view_live_status_live");
    }

    #[test]
    fn test_get_status_info_reconnecting() {
        let info = get_status_info("reconnecting");
        assert_eq!(info.dot_class, "bg-amber-400 animate-pulse");
        assert_eq!(info.label_key, "list_view_live_status_reconnecting");
    }

    #[test]
    fn test_get_status_info_offline() {
        let info = get_status_info("offline");
        assert_eq!(info.dot_class, "bg-gray-500");
        assert_eq!(info.label_key, "list_view_live_status_offline");
    }

    #[test]
    fn test_get_status_info_unknown() {
        let info = get_status_info("unknown");
        assert_eq!(info.dot_class, "bg-amber-400 animate-pulse");
        assert_eq!(info.label_key, "list_view_live_status_connecting");
    }

    #[test]
    fn test_get_status_info_empty() {
        let info = get_status_info("");
        assert_eq!(info.dot_class, "bg-amber-400 animate-pulse");
        assert_eq!(info.label_key, "list_view_live_status_connecting");
    }
}
