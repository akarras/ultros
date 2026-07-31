//! Opinionated defaults for URL-backed filters.
//!
//! The analyzer tools land first-time visitors on a sale-velocity-filtered view
//! instead of a list topped by items that sell once a month. The default lives
//! in the URL rather than in the filter logic, so chips, Clear All, and shared
//! links all keep behaving exactly as they do for a hand-typed filter.

use std::str::FromStr;

use leptos::prelude::*;
use leptos_router::NavigateOptions;
use leptos_router::hooks::query_signal_with_options;

/// Default ceiling on predicted time to next sale: items that sell at least
/// once a day. Parsed with `humantime`, same as anything typed into the box.
pub const DEFAULT_MAX_SALE_TIME: &str = "1d";

/// The same velocity floor, expressed as the crafting analyzers' daily-sales
/// metric rather than as a duration.
pub const DEFAULT_MIN_DAILY_SALES: f32 = 1.0;

/// Navigation options for filter query params.
///
/// `query_signal`'s defaults (`replace: false`, `scroll: true`) mean every
/// keystroke in a filter box pushes a history entry and yanks the window back
/// to the top. Filters are not navigation.
fn filter_nav_options() -> NavigateOptions {
    NavigateOptions {
        replace: true,
        scroll: false,
        ..Default::default()
    }
}

/// A [`query_signal`](leptos_router::hooks::query_signal) for a filter param,
/// using [`filter_nav_options`].
pub fn filter_query_signal<T>(key: &'static str) -> (Memo<Option<T>>, SignalSetter<Option<T>>)
where
    T: FromStr + ToString + PartialEq + Send + Sync + 'static,
{
    query_signal_with_options::<T>(key, filter_nav_options())
}

/// Write `default` into the URL if `key` is absent when this mounts.
///
/// Seeding fires only when the param is *absent*, so a link that carries the
/// param is honored verbatim — `?next-sale=` (unparseable) and `?min-sales=0`
/// both mean "no limit", and both are what the input box produces when a user
/// empties it.
///
/// Call this from the **route** component. Anything rendered inside a
/// `Suspense`/resource closure remounts whenever its resource changes — a live
/// market refetch, a world switch — and seeding there would silently reinstate
/// a filter the user had just cleared. The route component mounts once per
/// navigation, which is the granularity a default wants.
pub fn seed_query_default<T>(key: &'static str, default: T)
where
    T: FromStr + ToString + PartialEq + Clone + Send + Sync + 'static,
{
    let (value, set_value) = filter_query_signal::<T>(key);
    Effect::new(move |_| {
        if value.get_untracked().is_none() {
            set_value.set(Some(default.clone()));
        }
    });
}

/// Seed a whole set of defaults in one navigation, but only when the URL
/// carries none of `suppressing_keys`.
///
/// One navigation rather than one [`seed_query_default`] per key: separate
/// seeds are separate effects, and an earlier one changing the URL makes a
/// later one's "is my key absent?" check race against router state — with a
/// presence *predicate* (not just per-key absence) that race would corrupt
/// the outcome, not just reorder it.
///
/// Same rule as [`seed_query_default`]: call from the **route** component,
/// never from inside a `Suspense` closure.
pub fn seed_query_defaults_when_unfiltered(
    suppressing_keys: &'static [&'static str],
    defaults: &'static [(&'static str, &'static str)],
) {
    let query = leptos_router::hooks::use_query_map();
    let location = leptos_router::hooks::use_location();
    let navigate = leptos_router::hooks::use_navigate();
    Effect::new(move |_| {
        let mut map = query.get_untracked();
        if suppressing_keys.iter().any(|k| map.get_str(k).is_some()) {
            return;
        }
        for (key, value) in defaults {
            map.insert(key.to_string(), value.to_string());
        }
        let path = location.pathname.get_untracked();
        navigate(
            &format!("{path}{}", map.to_query_string()),
            filter_nav_options(),
        );
    });
}

#[cfg(test)]
mod test {
    use super::*;

    /// The seeded value goes through the same `humantime` parse as anything
    /// typed into the box, and an unparseable duration doesn't error — it just
    /// leaves `predicted_time` as `None`, i.e. no filter at all. A typo in the
    /// constant would silently undo the default, so pin it.
    #[test]
    fn default_max_sale_time_parses_to_one_day() {
        assert_eq!(
            humantime::parse_duration(DEFAULT_MAX_SALE_TIME).expect("default must parse"),
            std::time::Duration::from_secs(60 * 60 * 24),
        );
    }
}
