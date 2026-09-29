//! NPC placements, read out of the client's `.lgb` layout files.
//!
//! The `Level` sheet only places the NPCs that quests reference — 31% of the
//! gil vendors. Every NPC that stands in the world is placed by its zone's
//! layout files instead: `planner.lgb` for the original city vendors,
//! `planevent.lgb` for nearly everything since, `planlive.lgb` for a few.
//! Measured against client 2026.09.01: 445 of 629 vendor NPCs, and every one
//! not placed here is either spawned inside a player estate (housing
//! servants) or a seasonal stall.
//!
//! `planmap.lgb` is deliberately not read: its only NPC objects belong to the
//! graphics-benchmark scene, which stands Gridania's vendors in the Central
//! Shroud. Layers named `*benchmark*` are skipped for the same reason.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use anyhow::Context;
use icon_extract::GameInstall;
use icon_extract::lgb;
use serde::{Deserialize, Serialize};
use xiv_gen::{ENpcResidentId, LeveId, Map, MapId, NpcPlacement, TerritoryType, TerritoryTypeId};

/// Layout files that place event NPCs, in the order they are read.
const LAYOUT_FILES: [&str; 3] = ["planner.lgb", "planevent.lgb", "planlive.lgb"];

/// Two placements of one NPC closer than this on the same map are the same
/// spot (a layer variant, or the same object repeated).
const DUPLICATE_RADIUS: f32 = 0.05;

/// Every placement in the client, keyed by `ENpcBase` id.
pub type AllPlacements = HashMap<ENpcResidentId, Vec<NpcPlacement>>;

/// What the client walk found, for the run report.
pub struct Extraction {
    pub placements: AllPlacements,
    /// Distinct layout directories read.
    pub directories: usize,
    /// Files that were indexed but did not parse: `(path, reason)`. A handful
    /// of dungeon/field `planner.lgb` use a header variant that holds only
    /// level-design markers; anything else here is worth a look.
    pub unparsed: Vec<(String, String)>,
}

/// Walks every territory's layout files.
///
/// Instanced copies of a zone (quest battles, solo duties) reuse the zone's
/// layout directory under their own `TerritoryType` row, so each directory is
/// read once and attributed to the canonical row: the one whose `Name` is the
/// zone code the directory is named for, else the lowest id.
///
/// `resident_map` is `ENpcResident.Map`, the game's own hint for which of a
/// multi-map city's maps an NPC belongs to (the Ul'dah Merchant Strip); it is
/// used whenever it names a map of the territory being read.
pub fn extract(
    install: &GameInstall,
    territories: &[TerritoryType],
    maps: &HashMap<MapId, Map>,
    resident_map: &HashMap<ENpcResidentId, MapId>,
) -> anyhow::Result<Extraction> {
    let mut by_dir: BTreeMap<&str, &TerritoryType> = BTreeMap::new();
    for territory in territories {
        let Some((dir, code)) = territory.bg.rsplit_once('/') else {
            continue;
        };
        let entry = by_dir.entry(dir).or_insert(territory);
        let canonical = |t: &TerritoryType| t.name == code;
        if canonical(territory) || (!canonical(entry) && territory.key_id.0 < entry.key_id.0) {
            *entry = territory;
        }
    }

    let mut placements: AllPlacements = HashMap::new();
    let mut unparsed = Vec::new();
    let mut directories = 0;
    for (dir, territory) in &by_dir {
        let mut any = false;
        for file in LAYOUT_FILES {
            let path = format!("bg/{dir}/{file}");
            let bytes = match install.read_file(&path) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => continue,
                Err(e) => {
                    unparsed.push((path, format!("{e:#}")));
                    continue;
                }
            };
            any = true;
            let parsed = match lgb::parse(&bytes) {
                Ok(parsed) => parsed,
                Err(e) => {
                    unparsed.push((path, format!("{e:#}")));
                    continue;
                }
            };
            for layer in &parsed.layers {
                if layer.name.to_ascii_lowercase().contains("benchmark") {
                    continue;
                }
                for object in &layer.objects {
                    let Some(base) = object.enpc_base_id else {
                        continue;
                    };
                    let npc = ENpcResidentId(base as i32);
                    let Some(placement) = place(
                        territory,
                        maps,
                        resident_map.get(&npc).copied(),
                        object.translation,
                        layer.festival_id,
                    ) else {
                        continue;
                    };
                    placements.entry(npc).or_default().push(placement);
                }
            }
        }
        if any {
            directories += 1;
        }
    }
    for list in placements.values_mut() {
        dedupe(list);
    }
    Ok(Extraction {
        placements,
        directories,
        unparsed,
    })
}

/// Resolves one object's world position onto the territory's map.
fn place(
    territory: &TerritoryType,
    maps: &HashMap<MapId, Map>,
    resident_map: Option<MapId>,
    translation: [f32; 3],
    festival_id: u16,
) -> Option<NpcPlacement> {
    let map_id = resident_map
        .filter(|m| {
            maps.get(m)
                .is_some_and(|map| map.territory_type == territory.key_id.0)
        })
        .unwrap_or(MapId(territory.map));
    let map = maps.get(&map_id)?;
    if map.size_factor <= 0 {
        return None;
    }
    Some(NpcPlacement {
        map: map_id,
        territory: territory.key_id,
        x: xiv_gen::map_coordinate(translation[0], map.size_factor, map.offset_x),
        y: xiv_gen::map_coordinate(translation[2], map.size_factor, map.offset_y),
        festival_id,
    })
}

/// Drops repeated spots: same territory and map, within [`DUPLICATE_RADIUS`].
/// A permanent placement wins over a seasonal duplicate.
fn dedupe(list: &mut Vec<NpcPlacement>) {
    list.sort_by(|a, b| {
        (a.territory.0, a.map.0, a.festival_id)
            .cmp(&(b.territory.0, b.map.0, b.festival_id))
            .then(a.x.total_cmp(&b.x))
            .then(a.y.total_cmp(&b.y))
    });
    let mut kept: Vec<NpcPlacement> = Vec::with_capacity(list.len());
    for candidate in list.drain(..) {
        let duplicate = kept.iter().any(|k| {
            k.territory == candidate.territory
                && k.map == candidate.map
                && (k.x - candidate.x).abs() < DUPLICATE_RADIUS
                && (k.y - candidate.y).abs() < DUPLICATE_RADIUS
        });
        if !duplicate {
            kept.push(candidate);
        }
    }
    *list = kept;
}

/// On-disk form of the committed placements: `data/npc-placements.json`.
///
/// Only the NPCs the pack shows are written (vendors and leve issuers), so a
/// CSV-only rebuild on a machine without the game can fold the same
/// placements back in. Keyed by NPC id as a string because JSON object keys
/// are strings; rows are `[map, territory, x, y, festival]`.
#[derive(Debug, Serialize, Deserialize)]
pub struct PlacementsFile {
    /// `game/ffxivgame.ver` of the client these were read from.
    pub game_version: String,
    pub npcs: BTreeMap<String, Vec<PlacementRow>>,
}

/// One placement as written to the file: `[map, territory, x, y, festival]`.
pub type PlacementRow = (i32, i32, f32, f32, u16);

impl PlacementsFile {
    pub fn from_placements(game_version: &str, placements: &AllPlacements) -> Self {
        let npcs = placements
            .iter()
            .filter(|(_, list)| !list.is_empty())
            .map(|(npc, list)| {
                (
                    npc.0.to_string(),
                    list.iter()
                        .map(|p| {
                            (
                                p.map.0,
                                p.territory.0,
                                round(p.x),
                                round(p.y),
                                p.festival_id,
                            )
                        })
                        .collect(),
                )
            })
            .collect();
        PlacementsFile {
            game_version: game_version.to_string(),
            npcs,
        }
    }

    pub fn into_placements(self) -> anyhow::Result<AllPlacements> {
        self.npcs
            .into_iter()
            .map(|(npc, rows)| {
                let npc: i32 = npc
                    .parse()
                    .with_context(|| format!("npc id {npc:?} in npc-placements.json"))?;
                let rows = rows
                    .into_iter()
                    .map(|(map, territory, x, y, festival_id)| NpcPlacement {
                        map: MapId(map),
                        territory: TerritoryTypeId(territory),
                        x,
                        y,
                        festival_id,
                    })
                    .collect();
                Ok((ENpcResidentId(npc), rows))
            })
            .collect()
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let text = serde_json::to_string(self).context("serializing npc-placements.json")?;
        std::fs::write(path, text + "\n").with_context(|| format!("writing {}", path.display()))
    }
}

/// `data/npc-locations/leve-issuers.json`: Teamcraft's hand-kept levemete
/// table, normalised to `{"npc_leves": {"<npc id>": [leve ids]}}`, inverted
/// to leve -> issuers.
pub fn load_leve_issuers(path: &Path) -> anyhow::Result<HashMap<LeveId, Vec<ENpcResidentId>>> {
    #[derive(Deserialize)]
    struct File {
        npc_leves: BTreeMap<String, Vec<i32>>,
    }
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let file: File =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let mut issuers: HashMap<LeveId, Vec<ENpcResidentId>> = HashMap::new();
    for (npc, leves) in file.npc_leves {
        let npc: i32 = npc
            .parse()
            .with_context(|| format!("npc id {npc:?} in {}", path.display()))?;
        for leve in leves {
            issuers
                .entry(LeveId(leve))
                .or_default()
                .push(ENpcResidentId(npc));
        }
    }
    Ok(issuers)
}

/// Two decimals: what the game shows, and enough to keep the file stable
/// across float noise between runs.
fn round(v: f32) -> f32 {
    (v * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(id: i32, territory: i32, size_factor: i32) -> Map {
        Map {
            key_id: MapId(id),
            id: format!("m{id}/00"),
            size_factor,
            place_name_region: 0,
            place_name: 0,
            place_name_sub: 0,
            territory_type: territory,
            offset_x: 0,
            offset_y: 0,
        }
    }

    fn territory(id: i32, map: i32) -> TerritoryType {
        TerritoryType {
            key_id: TerritoryTypeId(id),
            name: "w1t2".into(),
            bg: "ffxiv/wil_w1/twn/w1t2/level/w1t2".into(),
            place_name: 0,
            map,
        }
    }

    #[test]
    fn resident_map_hint_wins_only_inside_its_territory() {
        let maps = HashMap::from([
            (MapId(13), map(13, 130, 200)),
            (MapId(14), map(14, 130, 200)),
            (MapId(99), map(99, 999, 200)),
        ]);
        let t = territory(130, 13);
        let p = place(&t, &maps, Some(MapId(14)), [0.0, 0.0, 0.0], 0).unwrap();
        assert_eq!(p.map, MapId(14));
        let p = place(&t, &maps, Some(MapId(99)), [0.0, 0.0, 0.0], 0).unwrap();
        assert_eq!(p.map, MapId(13));
        let p = place(&t, &maps, None, [0.0, 0.0, 0.0], 7).unwrap();
        assert_eq!((p.map, p.festival_id), (MapId(13), 7));
        // World origin is the centre of the map: coordinate 11.25 in a city.
        assert!((p.x - 11.25).abs() < 1e-4 && (p.y - 11.25).abs() < 1e-4);
    }

    #[test]
    fn unscaled_map_yields_no_placement() {
        let maps = HashMap::from([(MapId(1), map(1, 5, 0))]);
        assert!(place(&territory(5, 1), &maps, None, [1.0, 1.0, 1.0], 0).is_none());
    }

    #[test]
    fn dedupe_collapses_near_duplicates_and_prefers_permanent() {
        let p = |fest: u16, x: f32| NpcPlacement {
            map: MapId(1),
            territory: TerritoryTypeId(1),
            x,
            y: 5.0,
            festival_id: fest,
        };
        let mut list = vec![p(3, 10.0), p(0, 10.01), p(0, 12.0), p(0, 10.02)];
        dedupe(&mut list);
        let got: Vec<_> = list.iter().map(|p| (p.festival_id, p.x)).collect();
        assert_eq!(got, vec![(0, 10.01), (0, 12.0)]);
    }

    #[test]
    fn placements_file_round_trips() {
        let all: AllPlacements = HashMap::from([
            (
                ENpcResidentId(1000101),
                vec![NpcPlacement {
                    map: MapId(2),
                    territory: TerritoryTypeId(132),
                    x: 11.751,
                    y: 13.414,
                    festival_id: 0,
                }],
            ),
            (ENpcResidentId(5), vec![]),
        ]);
        let file = PlacementsFile::from_placements("2026.09.01.0000.0000", &all);
        assert_eq!(file.npcs.len(), 1, "empty lists are not written");
        let text = serde_json::to_string(&file).unwrap();
        let back: PlacementsFile = serde_json::from_str(&text).unwrap();
        let back = back.into_placements().unwrap();
        let p = back[&ENpcResidentId(1000101)][0];
        assert_eq!(
            (p.map, p.territory, p.festival_id),
            (MapId(2), TerritoryTypeId(132), 0)
        );
        assert_eq!((p.x, p.y), (11.75, 13.41));
    }
}
