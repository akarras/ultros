//! Experiments a player can switch on before they become the default.
//! A cookie, not localStorage: the list page renders on the server, so a
//! client-only flag would hydrate a different page than it served.

#![allow(dead_code)] // Public API used by Settings UI and route switches (later tasks)

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use leptos::prelude::*;
use leptos_router::hooks::use_query_map;

use super::cookies::Cookies;

pub const LABS_COOKIE: &str = "LABS";

/// The local-first list document: lists live in the browser and sync through
/// the server. Remains opt-in until convergence has soaked on shared lists.
pub const LAB_LISTS_SYNC: &str = "lists-sync";

pub struct LabInfo {
    pub token: &'static str,
}

/// Every live experiment. Adding one here is what makes it appear in
/// Settings; deleting it is part of shipping the feature.
pub const LABS: &[LabInfo] = &[
    // Remove when the lists sync layer is promoted (spec section 7).
    LabInfo {
        token: LAB_LISTS_SYNC,
    },
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Labs {
    pub enabled: BTreeSet<String>,
}

impl Labs {
    pub fn has(&self, token: &str) -> bool {
        self.enabled.contains(token)
    }
}

fn is_known(token: &str) -> bool {
    LABS.iter().any(|l| l.token == token)
}

impl FromStr for Labs {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self {
            enabled: s
                .split(',')
                .map(str::trim)
                .filter(|t| !t.is_empty() && is_known(t))
                .map(String::from)
                .collect(),
        })
    }
}

impl fmt::Display for Labs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.enabled.iter().cloned().collect::<Vec<_>>().join(","))
    }
}

/// Read the server-visible cookie and optional tester URL override together.
/// A memo prevents unrelated query changes from rebuilding the page.
pub fn use_lab(token: &'static str) -> Signal<bool> {
    let cookie = use_context::<Cookies>().map(|c| c.use_cookie_typed::<_, Labs>(LABS_COOKIE).0);
    let query = use_query_map();
    Memo::new(move |_| {
        let from_cookie = cookie.is_some_and(|c| c.get().is_some_and(|l| l.has(token)));
        let from_url = query.with(|q| {
            q.get("labs")
                .and_then(|v| v.parse::<Labs>().ok())
                .is_some_and(|l| l.has(token))
        });
        from_cookie || from_url
    })
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labs_cookie_round_trips_known_tokens_only() {
        let labs: Labs = "lists-sync,bogus,,lists-sync".parse().unwrap();
        assert_eq!(labs.enabled.len(), 1);
        assert!(labs.has(LAB_LISTS_SYNC));
        assert_eq!(labs.to_string(), "lists-sync");
        let empty: Labs = "".parse().unwrap();
        assert!(!empty.has(LAB_LISTS_SYNC));
        assert_eq!(empty.to_string(), "");
    }

    /// Retired analyzer tokens are gone, not aliased: a stale cookie parses to
    /// the empty set.
    #[test]
    fn the_retired_analyzer_tokens_no_longer_parse() {
        let old: Labs = "analyzer-recipe,analyzer-ledger".parse().unwrap();
        assert!(old.enabled.is_empty(), "{old:?}");
    }

    #[test]
    fn every_lab_token_is_listed_once() {
        let mut tokens: Vec<&str> = LABS.iter().map(|l| l.token).collect();
        tokens.sort_unstable();
        tokens.dedup();
        assert_eq!(tokens.len(), LABS.len());
    }

    #[test]
    fn lists_sync_is_an_opt_in_experiment() {
        assert!(LABS.iter().any(|lab| lab.token == LAB_LISTS_SYNC));
        assert!(!Labs::default().has(LAB_LISTS_SYNC));
    }

    #[test]
    fn the_experiment_list_stays_short() {
        assert!(LABS.len() <= 3, "keep the experiment list short");
    }
}
