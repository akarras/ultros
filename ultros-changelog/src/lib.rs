//! Player-facing release notes, compiled from one JSON file per change.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangelogCategory {
    Features,
    Improvements,
    BugFixes,
}

impl ChangelogCategory {
    pub const ALL: [Self; 3] = [Self::Features, Self::Improvements, Self::BugFixes];
}

/// Declaration order is display priority: high first, then medium, then low.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ChangelogImportance {
    High,
    Medium,
    Low,
}

/// One shipped change. All text is compiled into the binary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChangelogEntry {
    /// ISO YYYY-MM-DD, taken from the filename.
    pub date: &'static str,
    pub category: ChangelogCategory,
    pub importance: ChangelogImportance,
    pub title: &'static str,
    pub blurb: &'static str,
    pub link: Option<&'static str>,
}

// Newest date first, then importance, then filename for stable ties.
include!(concat!(env!("OUT_DIR"), "/changelog.rs"));

pub fn latest_changelog_date() -> &'static str {
    CHANGELOG.first().map(|entry| entry.date).unwrap_or("")
}

#[cfg(test)]
#[path = "../build.rs"]
mod build;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_entries_are_newest_first_then_importance() {
        assert!(!CHANGELOG.is_empty());
        assert!(CHANGELOG.windows(2).all(|pair| {
            pair[0].date > pair[1].date
                || (pair[0].date == pair[1].date && pair[0].importance <= pair[1].importance)
        }));
        assert_eq!(latest_changelog_date(), CHANGELOG[0].date);
    }
}
