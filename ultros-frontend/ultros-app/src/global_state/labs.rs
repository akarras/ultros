//! Experiments a player can switch on before they become the default.
//! A cookie, not localStorage: the analyzers render on the server, so a
//! client-only flag would hydrate a different page than it served.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

pub const LABS_COOKIE: &str = "LABS";

/// Graduated recipe market model. Keep its token readable in old cookies,
/// URLs and column metadata; the feature is now available to every player.
pub const LAB_ANALYZER_RECIPE: &str = "analyzer-recipe";

pub struct LabInfo {
    pub token: &'static str,
}

/// Every live experiment. Adding one here is what makes it appear in
/// Settings; deleting it is part of shipping the feature. Each entry's
/// comment names when it is deleted (a struct field for that would have
/// no non-test reader, which `-D warnings` rejects).
pub const LABS: &[LabInfo] = &[];

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
    token == LAB_ANALYZER_RECIPE || LABS.iter().any(|l| l.token == token)
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
    /// the empty set. The graduated market model no longer needs a flag.
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
    fn graduated_recipe_model_has_no_settings_toggle() {
        assert!(!LABS.iter().any(|lab| lab.token == LAB_ANALYZER_RECIPE));
        let legacy: Labs = LAB_ANALYZER_RECIPE.parse().unwrap();
        assert!(legacy.has(LAB_ANALYZER_RECIPE));
        assert_eq!(legacy.to_string(), LAB_ANALYZER_RECIPE);
    }

    #[test]
    fn the_experiment_list_stays_short() {
        assert!(LABS.len() <= 3, "keep the experiment list short");
    }
}
