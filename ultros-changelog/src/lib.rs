//! Player-facing release notes, compiled from one JSON file per change.
//!
//! The history is *server-only*. Two hundred-odd entries of prose is a real
//! slice of the wasm bundle and the changelog page is one of the least-visited
//! routes, so the full table is compiled in behind the `history` feature and
//! served over `/api/v1/changelog`. What the client keeps is the pair of dates
//! the sidebar's what's-new dot compares against, emitted by `build.rs` as two
//! `&str` consts.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangelogCategory {
    Features,
    Improvements,
    BugFixes,
}

impl ChangelogCategory {
    pub const ALL: [Self; 3] = [Self::Features, Self::Improvements, Self::BugFixes];
}

/// Declaration order is display priority: high first, then medium, then low.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangelogImportance {
    High,
    Medium,
    Low,
}

/// One shipped change.
///
/// `Cow` so the one type serves both ends of `/api/v1/changelog`: the
/// generated server-side table is entirely `Cow::Borrowed` over string
/// literals in the binary, and the client deserializes owned copies.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChangelogEntry {
    /// ISO YYYY-MM-DD, taken from the filename.
    pub date: Cow<'static, str>,
    pub category: ChangelogCategory,
    pub importance: ChangelogImportance,
    pub title: Cow<'static, str>,
    pub blurb: Cow<'static, str>,
    pub link: Option<Cow<'static, str>>,
    /// Behind a Labs toggle: shown with a badge, excluded from what's-new.
    pub labs: bool,
}

// `CHANGELOG`: newest date first, then importance, then filename for stable
// ties. Only compiled with the `history` feature — the wasm client fetches
// this list instead of carrying it.
#[cfg(feature = "history")]
include!(concat!(env!("OUT_DIR"), "/changelog.rs"));

mod dates {
    include!(concat!(env!("OUT_DIR"), "/dates.rs"));
}

/// The newest day anything shipped, Labs entries included. Reading the
/// changelog page records this, so opening the page clears the dot even when
/// the only new entries are Labs ones.
pub const LATEST_CHANGELOG_DATE: &str = dates::LATEST;

/// The newest day with a change everyone can use. Labs entries are skipped so
/// a Labs-only release does not light up the what's-new indicator for players
/// who have not opted in.
pub const LATEST_ANNOUNCED_CHANGELOG_DATE: &str = dates::LATEST_ANNOUNCED;

#[cfg(test)]
#[path = "../build.rs"]
mod build;

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "history")]
    #[test]
    fn compiled_entries_are_newest_first_then_importance() {
        assert!(!CHANGELOG.is_empty());
        assert!(CHANGELOG.windows(2).all(|pair| {
            pair[0].date > pair[1].date
                || (pair[0].date == pair[1].date && pair[0].importance <= pair[1].importance)
        }));
        assert_eq!(LATEST_CHANGELOG_DATE, CHANGELOG[0].date);
    }

    #[cfg(feature = "history")]
    #[test]
    fn announced_date_skips_labs_entries() {
        assert!(LATEST_ANNOUNCED_CHANGELOG_DATE <= LATEST_CHANGELOG_DATE);
        assert!(
            CHANGELOG
                .iter()
                .any(|entry| entry.date == LATEST_ANNOUNCED_CHANGELOG_DATE && !entry.labs)
        );
        assert!(
            CHANGELOG
                .iter()
                .take_while(|entry| entry.date.as_ref() > LATEST_ANNOUNCED_CHANGELOG_DATE)
                .all(|entry| entry.labs)
        );
    }

    /// The wire format keeps the vocabulary the change files are written in,
    /// so a payload stays readable and the enums can gain variants without
    /// the client and server disagreeing about ordinals.
    #[test]
    fn entries_round_trip_as_the_json_the_change_files_use() {
        let entry = ChangelogEntry {
            date: Cow::Borrowed("2026-09-22"),
            category: ChangelogCategory::BugFixes,
            importance: ChangelogImportance::Medium,
            title: Cow::Borrowed("Fixed a thing"),
            blurb: Cow::Borrowed("It works now."),
            link: Some(Cow::Borrowed("/changelog")),
            labs: false,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains(r#""category":"bug_fixes""#), "{json}");
        assert!(json.contains(r#""importance":"medium""#), "{json}");
        assert_eq!(
            serde_json::from_str::<ChangelogEntry>(&json).unwrap(),
            entry
        );
    }
}
