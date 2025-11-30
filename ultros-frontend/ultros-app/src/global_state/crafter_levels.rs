use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrafterLevels {
    pub carpenter: i32,
    pub blacksmith: i32,
    pub armorer: i32,
    pub goldsmith: i32,
    pub leatherworker: i32,
    pub weaver: i32,
    pub alchemist: i32,
    pub culinarian: i32,
}

impl Default for CrafterLevels {
    fn default() -> Self {
        Self {
            carpenter: 0,
            blacksmith: 0,
            armorer: 0,
            goldsmith: 0,
            leatherworker: 0,
            weaver: 0,
            alchemist: 0,
            culinarian: 0,
        }
    }
}

impl ToString for CrafterLevels {
    fn to_string(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

impl FromStr for CrafterLevels {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}
