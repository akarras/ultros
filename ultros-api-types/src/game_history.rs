//! Patch/expansion release calendar — the chart's milestone source.
//!
//! FFXIV's data files carry no release dates, so this is a checked-in seed
//! table: patch dates are append-only historical facts, so the table can
//! never become *wrong*, only incomplete (~4 appended rows a year). Any
//! future poller is an optimisation over this seed, never a dependency.
//!
//! CN and KR run separate game versions on their own schedules and are
//! seeded at expansion granularity (their sub-patch calendars live on the
//! operators' notice archives; see the source notes on each seed). A track
//! may be incomplete at the granularity level, but must never have a
//! *mid-timeline* gap at the granularity it claims — a missing middle
//! expansion would silently mislabel whole eras. Edits to the seed should
//! be treated as needing review: a wrong date misattributes price moves.
//!
//! Sources (verified 2026-07-31): Global — ffxiv.consolegameswiki.com
//! /wiki/Patches. CN — zh.wikipedia FFXIV version table + official
//! actff1.web.sdo.com version pages (2.0 uses the 2014-08-25 open-test
//! date, when the CN economy went live; the 2015-04-01 公测 relaunch kept
//! characters). KR — ff14.co.kr notice archive via press coverage; KR
//! updates run simultaneously with Global from 7.5 (2026-04-28) onward,
//! so future KR rows simply mirror Global's.

use std::sync::LazyLock;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// Which patch calendar a region follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PatchTrack {
    Global,
    China,
    Korea,
}

impl PatchTrack {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::China => "china",
            Self::Korea => "korea",
        }
    }
}

/// One released patch on one track.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GamePatch {
    pub track: PatchTrack,
    /// `PatchMark` convention: 700 = 7.0, 715 = 7.15, 655 = 6.55.
    pub version: u16,
    pub released: NaiveDate,
    /// ExVersion index: 0 = ARR, 1 = HW, 2 = SB, 3 = ShB, 4 = EW, 5 = DT.
    pub ex_version: u8,
}

/// `(version, (year, month, day))` — Global track. `ex_version` and
/// `PatchTrack` are derived, keeping the hand-entered surface minimal.
/// Sorted by release date; append new rows at the end.
const GLOBAL_SEED: &[(u16, (i32, u32, u32))] = &[
    (200, (2013, 8, 27)),
    (210, (2013, 12, 17)),
    (220, (2014, 3, 27)),
    (230, (2014, 7, 8)),
    (235, (2014, 8, 18)),
    (240, (2014, 10, 28)),
    (245, (2014, 12, 8)),
    (250, (2015, 1, 20)),
    (255, (2015, 3, 30)),
    (300, (2015, 6, 23)),
    (310, (2015, 11, 10)),
    (315, (2015, 12, 15)),
    (320, (2016, 2, 23)),
    (325, (2016, 3, 28)),
    (330, (2016, 6, 7)),
    (335, (2016, 7, 18)),
    (340, (2016, 9, 27)),
    (345, (2016, 10, 31)),
    (350, (2017, 1, 17)),
    (355, (2017, 2, 27)),
    (400, (2017, 6, 20)),
    (410, (2017, 10, 10)),
    (415, (2017, 11, 20)),
    (420, (2018, 1, 30)),
    (425, (2018, 3, 12)),
    (430, (2018, 5, 22)),
    (435, (2018, 7, 3)),
    (440, (2018, 9, 18)),
    (445, (2018, 11, 5)),
    (450, (2019, 1, 8)),
    (455, (2019, 2, 11)),
    (500, (2019, 7, 2)),
    (505, (2019, 7, 29)),
    (510, (2019, 10, 29)),
    (515, (2019, 12, 9)),
    (520, (2020, 2, 18)),
    (525, (2020, 4, 6)),
    (530, (2020, 8, 11)),
    (535, (2020, 10, 12)),
    (540, (2020, 12, 8)),
    (545, (2021, 2, 1)),
    (550, (2021, 4, 13)),
    (555, (2021, 5, 24)),
    (600, (2021, 12, 7)),
    (605, (2022, 1, 3)),
    (610, (2022, 4, 12)),
    (615, (2022, 6, 6)),
    (620, (2022, 8, 23)),
    (625, (2022, 10, 17)),
    (630, (2023, 1, 10)),
    (635, (2023, 3, 6)),
    (640, (2023, 5, 23)),
    (645, (2023, 7, 17)),
    (650, (2023, 10, 3)),
    (655, (2024, 1, 16)),
    (700, (2024, 7, 2)),
    (705, (2024, 7, 30)),
    (710, (2024, 11, 12)),
    (715, (2024, 12, 17)),
    (720, (2025, 3, 25)),
    (725, (2025, 5, 27)),
    (730, (2025, 8, 5)),
    (735, (2025, 10, 7)),
    (740, (2025, 12, 16)),
    (745, (2026, 3, 3)),
    (750, (2026, 4, 28)),
    (755, (2026, 7, 28)),
];

/// Chinese server — expansion launches (complete at that granularity).
/// Sub-patches follow their own cadence; append from the 盛趣 notice
/// archive when sourcing them.
const CHINA_SEED: &[(u16, (i32, u32, u32))] = &[
    (200, (2014, 8, 25)),
    (300, (2015, 11, 19)),
    (400, (2017, 9, 26)),
    (500, (2019, 10, 15)),
    (600, (2022, 3, 16)),
    (700, (2024, 9, 27)),
];

/// Korean server — expansion launches from 3.0 (the 2.0-era leading gap is
/// safe: no bands draw before the first row). 7.5 marks the switch to
/// simultaneous Global/KR updates; rows after it mirror Global's dates.
const KOREA_SEED: &[(u16, (i32, u32, u32))] = &[
    (300, (2016, 6, 14)),
    (400, (2017, 12, 19)),
    (500, (2019, 12, 3)),
    (600, (2022, 5, 10)),
    (700, (2024, 12, 3)),
    (750, (2026, 4, 28)),
];

/// The whole calendar, all tracks — Global at patch granularity (majors +
/// x.x5 point patches), CN/KR at expansion granularity. See module docs.
pub static GAME_PATCHES: LazyLock<Vec<GamePatch>> = LazyLock::new(|| {
    let expand = |track: PatchTrack, seed: &[(u16, (i32, u32, u32))]| {
        seed.iter()
            .map(|&(version, (y, m, d))| GamePatch {
                track,
                version,
                released: NaiveDate::from_ymd_opt(y, m, d)
                    .expect("seed dates are hand-checked calendar dates"),
                ex_version: (version / 100 - 2) as u8,
            })
            .collect::<Vec<_>>()
    };
    let mut all = expand(PatchTrack::Global, GLOBAL_SEED);
    all.extend(expand(PatchTrack::China, CHINA_SEED));
    all.extend(expand(PatchTrack::Korea, KOREA_SEED));
    all
});

/// Region name → patch track. Data with a Global default, NOT a hardcoded
/// exhaustive match: `update_datacenters` creates regions from whatever
/// `RegionName` values Universalis reports, so a region we have never seen
/// must fall back to Global rather than fail or render nothing.
pub fn track_for_region(region_name: &str) -> PatchTrack {
    match region_name {
        "中国" => PatchTrack::China,
        "한국" => PatchTrack::Korea,
        _ => PatchTrack::Global,
    }
}

pub fn patches_for_track(track: PatchTrack) -> impl Iterator<Item = &'static GamePatch> {
    GAME_PATCHES.iter().filter(move |p| p.track == track)
}

/// Which milestone marks a zoom level can carry — the same level-of-detail
/// principle as the bucket ladder. A fixed marker set is either a picket
/// fence at four years or an empty rail at thirty days.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkTier {
    /// > 2 years visible: expansion launches only.
    ExpansionsOnly,
    /// 6 months – 2 years: + major patches (x.0–x.5).
    Major,
    /// 30 days – 6 months: + point patches (x.x5).
    Point,
    /// < 30 days: nothing, usually.
    None,
}

const DAY: i64 = 86_400;

pub fn mark_tier(span_secs: i64) -> MarkTier {
    match span_secs {
        s if s > 730 * DAY => MarkTier::ExpansionsOnly,
        s if s > 182 * DAY => MarkTier::Major,
        s if s >= 30 * DAY => MarkTier::Point,
        _ => MarkTier::None,
    }
}

/// The patches worth marking for a window of `span_secs` on `track`.
pub fn visible_patches(track: PatchTrack, span_secs: i64) -> Vec<&'static GamePatch> {
    let tier = mark_tier(span_secs);
    patches_for_track(track)
        .filter(|p| match tier {
            MarkTier::ExpansionsOnly => p.version.is_multiple_of(100),
            MarkTier::Major => p.version.is_multiple_of(10),
            MarkTier::Point => true,
            MarkTier::None => false,
        })
        .collect()
}

/// `700` → `"7.0"`, `715` → `"7.15"`, `705` → `"7.05"` — the PatchMark
/// convention, rendered the way players write it.
pub fn version_label(version: u16) -> String {
    let major = version / 100;
    let minor = version % 100;
    if minor.is_multiple_of(10) {
        format!("{major}.{}", minor / 10)
    } else {
        format!("{major}.{minor:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_names_map_to_tracks_with_a_global_fallback() {
        assert_eq!(track_for_region("中国"), PatchTrack::China);
        assert_eq!(track_for_region("한국"), PatchTrack::Korea);
        assert_eq!(track_for_region("North-America"), PatchTrack::Global);
        assert_eq!(track_for_region("Europe"), PatchTrack::Global);
        assert_eq!(
            track_for_region("Some Region Universalis Invents Tomorrow"),
            PatchTrack::Global
        );
    }

    #[test]
    fn seed_is_sorted_and_unique_per_track() {
        let patches = &*GAME_PATCHES;
        for pair in patches.windows(2) {
            if pair[0].track == pair[1].track {
                assert!(
                    pair[0].released < pair[1].released,
                    "seed must be strictly sorted by date within a track: {:?} then {:?}",
                    pair[0],
                    pair[1]
                );
                assert_ne!(pair[0].version, pair[1].version, "duplicate version");
            }
        }
    }

    #[test]
    fn ex_version_is_consistent_with_the_major_number() {
        for p in GAME_PATCHES.iter() {
            assert_eq!(
                p.ex_version as u16,
                p.version / 100 - 2,
                "ex_version drifted for {}",
                p.version
            );
            assert!(p.ex_version <= 5, "unknown expansion index");
        }
    }

    #[test]
    fn mark_tiers_follow_the_documented_spans() {
        assert_eq!(mark_tier(4 * 365 * DAY), MarkTier::ExpansionsOnly);
        assert_eq!(mark_tier(365 * DAY), MarkTier::Major);
        assert_eq!(mark_tier(90 * DAY), MarkTier::Point);
        assert_eq!(mark_tier(7 * DAY), MarkTier::None);
    }

    #[test]
    fn visible_patches_filter_by_tier() {
        let expansions = visible_patches(PatchTrack::Global, 4 * 365 * DAY);
        assert!(expansions.iter().all(|p| p.version % 100 == 0));
        assert_eq!(expansions.len(), 6, "one launch per expansion, 2.0..=7.0");

        let majors = visible_patches(PatchTrack::Global, 365 * DAY);
        assert!(majors.iter().all(|p| p.version % 10 == 0));
        assert!(majors.len() > expansions.len());

        let all = visible_patches(PatchTrack::Global, 90 * DAY);
        assert_eq!(all.len(), GLOBAL_SEED.len());
        assert!(all.windows(2).all(|w| w[0].released < w[1].released));

        assert!(visible_patches(PatchTrack::Global, 7 * DAY).is_empty());
        assert_eq!(
            visible_patches(PatchTrack::China, 4 * 365 * DAY).len(),
            6,
            "CN seeded at expansion granularity, 2.0..=7.0"
        );
        assert_eq!(
            visible_patches(PatchTrack::Korea, 4 * 365 * DAY).len(),
            5,
            "KR seeded at expansion granularity from 3.0"
        );
    }

    #[test]
    fn no_track_has_a_mid_timeline_expansion_gap() {
        // A missing MIDDLE expansion would silently mislabel whole eras
        // (everything after the gap would wear the earlier expansion's
        // band). Leading gaps are fine — nothing draws before the first
        // row. So: within each track, expansion launches must be
        // consecutive from the track's first one.
        for track in [PatchTrack::Global, PatchTrack::China, PatchTrack::Korea] {
            let launches: Vec<u8> = patches_for_track(track)
                .filter(|p| p.version.is_multiple_of(100))
                .map(|p| p.ex_version)
                .collect();
            for pair in launches.windows(2) {
                assert_eq!(
                    pair[1],
                    pair[0] + 1,
                    "{track:?} skips an expansion between {} and {}",
                    pair[0],
                    pair[1]
                );
            }
        }
    }

    #[test]
    fn version_labels_render_the_player_convention() {
        assert_eq!(version_label(700), "7.0");
        assert_eq!(version_label(710), "7.1");
        assert_eq!(version_label(715), "7.15");
        assert_eq!(version_label(705), "7.05");
        assert_eq!(version_label(655), "6.55");
        assert_eq!(version_label(200), "2.0");
    }

    #[test]
    fn game_patch_round_trips_through_json() {
        let p = GAME_PATCHES[0];
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<GamePatch>(&json).unwrap(), p);
        assert!(
            json.contains("\"global\""),
            "track serialises lowercase: {json}"
        );
    }
}
