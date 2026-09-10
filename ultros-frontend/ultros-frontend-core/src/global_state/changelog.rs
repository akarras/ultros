//! Tracks which changelog entries a visitor has already seen, so the sidebar
//! can show a small dot when something new has shipped.

use cookie::{Cookie, SameSite, time::Duration};
use leptos::{
    prelude::*,
    reactive::wrappers::write::{IntoSignalSetter, SignalSetter},
};

use super::cookies::{Cookies, get_now};
use ultros_changelog::latest_changelog_date;

const CHANGELOG_SEEN_COOKIE: &str = "CHANGELOG_SEEN";

/// Reads and writes the ISO-8601 date of the newest changelog entry the
/// visitor has seen. Follows the home-world cookie pattern: a year-long,
/// site-wide `Lax` cookie so the server can render the same state the client
/// will.
pub fn use_changelog_seen() -> (Signal<Option<String>>, SignalSetter<Option<String>>) {
    let cookies = use_context::<Cookies>().unwrap();
    let (cookie, set_cookie) = cookies.get_cookie(CHANGELOG_SEEN_COOKIE);
    let seen = Memo::new(move |_| cookie().map(|cookie| cookie.value().to_string()));
    let set_seen = move |date: Option<String>| {
        let cookie = date.map(|date| {
            let mut cookie = Cookie::new(CHANGELOG_SEEN_COOKIE, date);
            cookie.set_same_site(SameSite::Lax);
            cookie.set_secure(Some(true));
            cookie.set_path("/");
            cookie.set_expires(get_now() + Duration::days(365));
            cookie
        });
        set_cookie(cookie);
    };
    (seen.into(), set_seen.into_signal_setter())
}

/// Whether there is a changelog entry newer than the one the visitor last saw.
///
/// Both arguments are ISO-8601 `YYYY-MM-DD` strings, which sort
/// lexicographically in the same order they sort chronologically — that
/// equivalence is why the build validates zero-padded calendar dates in the
/// changelog fragments.
///
/// A visitor with no cookie gets no dot: they have never seen *any* entry, and
/// nagging a first-time visitor about a feature they are already looking at
/// for the first time is noise. [`use_whats_new_indicator`] seeds the cookie
/// for them instead, so the next thing that ships does show up.
pub fn has_unseen_entries(seen: Option<&str>, latest: &str) -> bool {
    match seen {
        Some(seen) => seen < latest,
        None => false,
    }
}

/// Drives the sidebar's what's-new dot.
///
/// The returned signal is `false` during SSR and during the first client
/// render, and only becomes true once an `Effect` has run — effects are
/// client-only and run after hydration, so the server HTML and the client's
/// first pass always agree on whether the dot is in the DOM. This is the same
/// `hydrated` gate used for relative times and cheapest prices.
pub fn use_whats_new_indicator() -> Signal<bool> {
    let (seen, set_seen) = use_changelog_seen();
    let hydrated = RwSignal::new(false);
    Effect::new(move |_| {
        // First visit: record where they started so the dot marks what ships
        // *next* rather than the whole backlog.
        if seen.get_untracked().is_none() {
            set_seen(Some(latest_changelog_date().to_string()));
        }
        hydrated.set(true);
    });
    Signal::derive(move || {
        hydrated.get()
            && seen.with(|seen| has_unseen_entries(seen.as_deref(), latest_changelog_date()))
    })
}

/// Marks every current entry as seen. Called by the changelog page itself, so
/// reading the page is what clears the dot.
pub fn use_mark_changelog_seen() {
    let (_seen, set_seen) = use_changelog_seen();
    Effect::new(move |_| {
        set_seen(Some(latest_changelog_date().to_string()));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unseen_when_an_entry_is_newer_than_the_cookie() {
        assert!(has_unseen_entries(Some("2026-07-01"), "2026-08-01"));
    }

    #[test]
    fn nothing_unseen_when_caught_up() {
        assert!(!has_unseen_entries(Some("2026-08-01"), "2026-08-01"));
    }

    /// A cookie from the future (entries were removed, or clocks disagreed)
    /// must not resurrect the dot forever.
    #[test]
    fn nothing_unseen_when_the_cookie_is_ahead() {
        assert!(!has_unseen_entries(Some("2026-09-01"), "2026-08-01"));
    }

    #[test]
    fn first_time_visitors_get_no_dot() {
        assert!(!has_unseen_entries(None, "2026-08-01"));
    }

    /// Garbage in the cookie (hand-edited, or a format we no longer write)
    /// must not panic or permanently pin the dot on.
    #[test]
    fn unparseable_cookie_is_just_an_older_date() {
        assert!(has_unseen_entries(Some(""), "2026-08-01"));
        assert!(!has_unseen_entries(Some("not-a-date"), "2026-08-01"));
    }
}
