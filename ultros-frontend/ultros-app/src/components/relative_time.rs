use chrono::{NaiveDateTime, Utc};
use leptos::*;
use timeago::Formatter;

#[component]
pub fn RelativeToNow(timestamp: NaiveDateTime) -> impl IntoView {
    let duration = Utc::now().naive_utc() - timestamp;
    let delta = duration
        .to_std()
        .ok()
        .map(|duration| Formatter::new().convert(duration))
        .unwrap_or("now".to_string());
    view! {<span>{delta}</span>}
}