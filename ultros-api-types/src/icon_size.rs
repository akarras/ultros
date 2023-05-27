use std::fmt::Display;

use serde::{Deserialize, Serialize};

#[derive(Hash, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Copy, Deserialize, Serialize)]
pub enum IconSize {
    Small,
    Medium,
    Large,
}

impl IconSize {
    pub fn get_class(&self) -> &'static str {
        match self {
            IconSize::Small => "icon-small",
            IconSize::Medium => "icon-medium",
            IconSize::Large => "icon-large",
        }
    }

    pub fn get_px_size(&self) -> u32 {
        match self {
            IconSize::Small => 30,
            IconSize::Medium => 40,
            IconSize::Large => 80,
        }
    }
}

impl Display for IconSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IconSize::Small => write!(f, "Small"),
            IconSize::Medium => write!(f, "Medium"),
            IconSize::Large => write!(f, "Large"),
        }
    }
}
