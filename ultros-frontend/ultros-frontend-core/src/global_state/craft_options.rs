//! Shared craft-options cookie state. Consumed by the recipe analyzer,
//! FC analyzer, and item-page recipe panel in Tasks 7-9.
// TODO(Tasks 8-9): remove this allow once COOKIE_NAME gains callers in the analyzer routes.
#![allow(dead_code)]

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
    /// Gil-equivalent cost of one world hop inside a datacenter, used by the
    /// recipe planner's route ranking.
    #[serde(default = "default_world_hop_gil")]
    pub world_hop_gil: i64,
    /// Gil-equivalent cost of entering another datacenter.
    #[serde(default = "default_dc_hop_gil")]
    pub dc_hop_gil: i64,
}

fn default_exclude_shards() -> bool {
    true
}

fn default_world_hop_gil() -> i64 {
    crate::recipe_planner::TravelWeights::default().world_hop
}

fn default_dc_hop_gil() -> i64 {
    crate::recipe_planner::TravelWeights::default().dc_hop
}

/// Upper bound for either hop weight; anything larger is a typo, not a preference.
pub const MAX_HOP_GIL: i64 = 10_000_000;

impl CraftOptions {
    /// Planner travel weights, clamped so a hand-edited cookie cannot
    /// overflow the route score.
    pub fn travel_weights(&self) -> crate::recipe_planner::TravelWeights {
        crate::recipe_planner::TravelWeights {
            world_hop: self.world_hop_gil.clamp(0, MAX_HOP_GIL),
            dc_hop: self.dc_hop_gil.clamp(0, MAX_HOP_GIL),
        }
    }
}

impl Default for CraftOptions {
    fn default() -> Self {
        Self {
            require_hq: false,
            include_subcrafts: false,
            exclude_shards: true,
            use_on_hand: false,
            active_craft_list: None,
            world_hop_gil: default_world_hop_gil(),
            dc_hop_gil: default_dc_hop_gil(),
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
            world_hop_gil: 1_500,
            dc_hop_gil: 9_000,
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
        assert_eq!((parsed.world_hop_gil, parsed.dc_hop_gil), (2_000, 10_000));
    }

    #[test]
    fn travel_weights_round_trip_and_are_clamped() {
        let opts = CraftOptions {
            world_hop_gil: 500,
            dc_hop_gil: 25_000,
            ..Default::default()
        };
        let parsed: CraftOptions = opts.to_string().parse().unwrap();
        assert_eq!((parsed.world_hop_gil, parsed.dc_hop_gil), (500, 25_000));
        let parsed: CraftOptions =
            r#"{"world_hop_gil":-5,"dc_hop_gil":99999999999}"#.parse().unwrap();
        assert_eq!(parsed.travel_weights().world_hop, 0);
        assert_eq!(parsed.travel_weights().dc_hop, MAX_HOP_GIL);
    }
}
