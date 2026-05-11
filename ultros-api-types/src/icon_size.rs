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
            IconSize::Small => 25,
            IconSize::Medium => 40,
            IconSize::Large => 60,
        }
    }

    pub fn get_size_px(&self) -> &'static str {
        match self {
            IconSize::Small => "25px",
            IconSize::Medium => "40px",
            IconSize::Large => "60px",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_strings_match_size() {
        assert_eq!(IconSize::Small.get_class(), "icon-small");
        assert_eq!(IconSize::Medium.get_class(), "icon-medium");
        assert_eq!(IconSize::Large.get_class(), "icon-large");
    }

    #[test]
    fn px_sizes_are_ordered_ascending() {
        assert!(IconSize::Small.get_px_size() < IconSize::Medium.get_px_size());
        assert!(IconSize::Medium.get_px_size() < IconSize::Large.get_px_size());
    }

    #[test]
    fn size_px_string_matches_get_px_size() {
        for size in [IconSize::Small, IconSize::Medium, IconSize::Large] {
            let expected = format!("{}px", size.get_px_size());
            assert_eq!(size.get_size_px(), expected);
        }
    }

    #[test]
    fn display_renders_variant_name() {
        assert_eq!(IconSize::Small.to_string(), "Small");
        assert_eq!(IconSize::Medium.to_string(), "Medium");
        assert_eq!(IconSize::Large.to_string(), "Large");
    }

    #[test]
    fn serde_roundtrip_preserves_value() {
        for size in [IconSize::Small, IconSize::Medium, IconSize::Large] {
            let s = serde_json::to_string(&size).unwrap();
            let back: IconSize = serde_json::from_str(&s).unwrap();
            assert_eq!(size, back);
        }
    }

    #[test]
    fn ordering_is_small_lt_medium_lt_large() {
        assert!(IconSize::Small < IconSize::Medium);
        assert!(IconSize::Medium < IconSize::Large);
    }
}
