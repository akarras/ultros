//! Experiments a player can switch on before they become the default.
//! A cookie, not localStorage: the analyzers render on the server, so a
//! client-only flag would hydrate a different page than it served.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use leptos::prelude::*;
use leptos_router::hooks::use_query_map;

use super::cookies::Cookies;

pub const LABS_COOKIE: &str = "LABS";

/// Recipe's experimental formula controls, price-signal columns, and travel
/// comparisons. Remains opt-in until its production validation is complete.
pub const LAB_ANALYZER_RECIPE: &str = "analyzer-recipe";

pub struct LabInfo {
    pub token: &'static str,
}

/// Every live experiment. Adding one here is what makes it appear in
/// Settings; deleting it is part of shipping the feature. Each entry's
/// comment names when it is deleted (a struct field for that would have
/// no non-test reader, which `-D warnings` rejects).
pub const LABS: &[LabInfo] = &[
    // Remove only after the recipe market model has passed production validation.
    LabInfo {
        token: LAB_ANALYZER_RECIPE,
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
/// A memo prevents unrelated grid query changes from rebuilding the model.
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
        let labs: Labs = "analyzer-recipe,bogus,,analyzer-recipe".parse().unwrap();
        assert_eq!(labs.enabled.len(), 1);
        assert!(labs.has(LAB_ANALYZER_RECIPE));
        assert_eq!(labs.to_string(), "analyzer-recipe");
        let empty: Labs = "".parse().unwrap();
        assert!(!empty.has(LAB_ANALYZER_RECIPE));
        assert_eq!(empty.to_string(), "");
    }

    /// The two tokens Phases C and D shipped are gone, not aliased: a
    /// stored cookie or a bookmarked `?labs=` holding one of them parses to
    /// the empty set. Testers opt in with the current experiment token.
    #[test]
    fn the_retired_analyzer_tokens_no_longer_parse() {
        let old: Labs = "analyzer-ledger,analyzer-signal-columns".parse().unwrap();
        assert!(old.enabled.is_empty(), "{old:?}");
        assert_eq!(old.to_string(), "");
    }

    #[test]
    fn every_lab_token_is_listed_once() {
        let mut tokens: Vec<&str> = LABS.iter().map(|l| l.token).collect();
        tokens.sort_unstable();
        tokens.dedup();
        assert_eq!(tokens.len(), LABS.len());
    }

    #[test]
    fn recipe_model_remains_an_opt_in_settings_experiment() {
        assert!(LABS.iter().any(|lab| lab.token == LAB_ANALYZER_RECIPE));
        assert!(!Labs::default().has(LAB_ANALYZER_RECIPE));
        let enabled: Labs = LAB_ANALYZER_RECIPE.parse().unwrap();
        assert!(enabled.has(LAB_ANALYZER_RECIPE));
        assert_eq!(enabled.to_string(), LAB_ANALYZER_RECIPE);
    }

    #[test]
    fn the_experiment_list_stays_short() {
        assert!(LABS.len() <= 3, "keep the experiment list short");
    }
}
