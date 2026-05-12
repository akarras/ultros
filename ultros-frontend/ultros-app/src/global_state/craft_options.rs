use serde::{Deserialize, Serialize};
use std::{fmt::Display, str::FromStr};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CraftOptions {
    #[serde(default)]
    pub require_hq: bool,
    #[serde(default)]
    pub include_subcrafts: bool,
    #[serde(default = "default_exclude_shards")]
    pub exclude_shards: bool,
    #[serde(default)]
    pub use_on_hand: bool,
    /// If set, on-hand is read from this list's `ListItem.acquired`.
    /// If None, on-hand uses LocalStorage.
    #[serde(default)]
    pub active_craft_list: Option<i32>,
}

fn default_exclude_shards() -> bool {
    true
}

impl Default for CraftOptions {
    fn default() -> Self {
        Self {
            require_hq: false,
            include_subcrafts: false,
            exclude_shards: true,
            use_on_hand: false,
            active_craft_list: None,
        }
    }
}

impl Display for CraftOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", serde_json::to_string(self).unwrap_or_default())
    }
}

impl FromStr for CraftOptions {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}

#[allow(dead_code)]
pub const COOKIE_NAME: &str = "CRAFT_OPTIONS";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_excludes_shards() {
        let opts = CraftOptions::default();
        assert!(opts.exclude_shards);
    }

    #[test]
    fn roundtrip_through_cookie() {
        let opts = CraftOptions {
            require_hq: true,
            include_subcrafts: true,
            exclude_shards: false,
            use_on_hand: true,
            active_craft_list: Some(42),
        };
        let s = opts.to_string();
        let parsed: CraftOptions = s.parse().unwrap();
        assert_eq!(opts, parsed);
    }

    #[test]
    fn missing_fields_get_defaults() {
        // Backward compat: a stale cookie with only one field should still parse.
        let parsed: CraftOptions = r#"{"require_hq":true}"#.parse().unwrap();
        assert!(parsed.require_hq);
        assert!(parsed.exclude_shards); // serde default kicks in
    }
}
