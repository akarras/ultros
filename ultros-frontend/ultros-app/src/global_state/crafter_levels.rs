use serde::{Deserialize, Serialize};
use std::{fmt::Display, str::FromStr};

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
            carpenter: 100,
            blacksmith: 100,
            armorer: 100,
            goldsmith: 100,
            leatherworker: 100,
            weaver: 100,
            alchemist: 100,
            culinarian: 100,
        }
    }
}

impl Display for CrafterLevels {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", serde_json::to_string(self).unwrap_or_default())
    }
}

impl FromStr for CrafterLevels {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}
