use chrono::{NaiveDateTime, Utc};
use leptos::*;
#[cfg(feature = "hydrate")]
use leptos_use::{use_interval, UseIntervalReturn};
use timeago::Formatter;

#[component]
pub fn RelativeToNow(timestamp: NaiveDateTime) -> impl IntoView {
    // this could probably be moved to a global state so we just have one interval for every clock
    #[cfg(feature = "hydrate")]
    let UseIntervalReturn { counter, .. } = use_interval(1000);
    let time_display = create_memo(move |_| {
        #[cfg(feature = "hydrate")]
        let _counter = counter(); // just to make things tick
        let duration = Utc::now().naive_utc() - timestamp;
        duration
            .to_std()
            .ok()
            .map(|duration| Formatter::new().convert(duration))
            .unwrap_or("now".to_string())
    });
    view! {<span>{time_display}</span>}
}
