use chrono::{NaiveDateTime, Utc};
use leptos::prelude::*;
#[cfg(feature = "hydrate")]
use leptos_use::{UseIntervalReturn, use_interval};
use timeago::Formatter;

use crate::i18n::*;

#[component]
pub fn RelativeToNow(timestamp: NaiveDateTime) -> impl IntoView {
    let i18n = use_i18n();
    // this could probably be moved to a global state so we just have one interval for every clock
    #[cfg(feature = "hydrate")]
    let UseIntervalReturn { counter, .. } = use_interval(1000);
    // Defer the `Utc::now()`-based label until after hydration. SSR computes
    // the label at server-render time; the client re-runs the memo during
    // hydration with a `Utc::now()` that has advanced by network + render
    // latency, so the SSR text ("5 seconds ago") and the first CSR render
    // ("20 seconds ago") differ. Pure text-content differences are usually
    // tolerated by tachys, but `timeago::Formatter` can flip across
    // unit/format boundaries (e.g. "now" vs "1 second ago") which has
    // historically been part of the `/item/<world>/<id>` and
    // `/items/jobset/<JOB>` cluster cascades — same idiom as #725 (chart),
    // #719 (item-explorer), #712 (home `RecentItems`).
    //
    // During the initial SSR + first CSR render we emit the absolute UTC
    // timestamp (deterministic, identical on both sides). An `Effect` flips
    // `hydrated` post-render — effects only fire on the client and only
    // after the first render — and the memo re-runs with the real relative
    // label. Users see the absolute timestamp for one frame, then it
    // updates to the relative form.
    let hydrated = RwSignal::new(false);
    Effect::new(move |_| {
        hydrated.set(true);
    });
    let time_display = Memo::new(move |_| {
        #[cfg(feature = "hydrate")]
        let _counter = counter(); // just to make things tick
        if !hydrated.get() {
            return timestamp.format("%Y-%m-%d %H:%M UTC").to_string();
        }
        let duration = Utc::now().naive_utc() - timestamp;
        duration
            .to_std()
            .ok()
            .map(|duration| Formatter::new().convert(duration))
            .unwrap_or_else(|| t_string!(i18n, relative_time_now).to_string())
    });
    view! { <span>{time_display}</span> }.into_any()
}
