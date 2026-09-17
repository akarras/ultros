//! Spike: measure how many gil-vendor NPCs the client's `.lgb` layout files
//! place, versus the `Level` sheet the current `data/npc-locations` snapshot
//! relies on (31%).
//!
//! cargo run --release -p icon-extract --example npc_positions -- <csv dir> <vendors.json> <out.json>
//!
//! `csv dir` holds the pinned `TerritoryType`, `Map`, `PlaceName` and
//! `ENpcResident` CSVs; `vendors.json` is `{"vendor_npcs": {"<id>": [shop..]}}`.

use icon_extract::lgb::{self, ASSET_EVENT_NPC};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Debug, Clone)]
struct MapRow {
    id: String,
    size_factor: f64,
    offset_x: f64,
    offset_y: f64,
    place_name: u32,
    place_name_sub: u32,
    territory: u32,
}

/// World X/Z -> map X/Y, per xivapi/ffxiv-datamining docs/MapCoordinates.md.
fn map_coordinate(position: f64, scale: f64, offset: f64) -> f64 {
    let factor = scale / 100.0;
    41.0 / factor * (((position + offset) * factor + 1024.0) / 2048.0) + 1.0
}

fn read_csv(path: &std::path::Path) -> Vec<HashMap<String, String>> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_path(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut header: Option<Vec<String>> = None;
    let mut rows = Vec::new();
    for rec in rdr.records() {
        let rec = rec.expect("csv record");
        match &header {
            None => {
                if rec.get(0) == Some("#") {
                    header = Some(rec.iter().map(str::to_string).collect());
                }
            }
            Some(h) => {
                if rec.get(0).and_then(|v| v.parse::<i64>().ok()).is_some() {
                    rows.push(
                        h.iter()
                            .cloned()
                            .zip(rec.iter().map(str::to_string))
                            .collect(),
                    );
                }
            }
        }
    }
    rows
}

#[derive(serde::Serialize)]
struct Placement {
    npc_id: u32,
    name: String,
    territory_id: u32,
    map_id: u32,
    zone: String,
    map_name: String,
    subarea: String,
    x: f64,
    y: f64,
    festival_id: u16,
    file: &'static str,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let csv_dir = std::path::Path::new(&args[1]);
    let vendors: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&args[2]).expect("vendors.json")).unwrap();
    let vendor_ids: BTreeSet<u32> = vendors["vendor_npcs"]
        .as_object()
        .unwrap()
        .keys()
        .map(|k| k.parse().unwrap())
        .collect();

    let places: HashMap<u32, String> = read_csv(&csv_dir.join("PlaceName.csv"))
        .into_iter()
        .map(|r| (r["#"].parse().unwrap(), r["Name"].clone()))
        .collect();
    let maps: HashMap<u32, MapRow> = read_csv(&csv_dir.join("Map.csv"))
        .into_iter()
        .map(|r| {
            (
                r["#"].parse().unwrap(),
                MapRow {
                    id: r["Id"].clone(),
                    size_factor: r["SizeFactor"].parse().unwrap(),
                    offset_x: r["OffsetX"].parse().unwrap(),
                    offset_y: r["OffsetY"].parse().unwrap(),
                    place_name: r["PlaceName"].parse().unwrap(),
                    place_name_sub: r["PlaceNameSub"].parse().unwrap(),
                    territory: r["TerritoryType"].parse().unwrap(),
                },
            )
        })
        .collect();
    let residents: HashMap<u32, (String, u32)> = read_csv(&csv_dir.join("ENpcResident.csv"))
        .into_iter()
        .map(|r| {
            (
                r["#"].parse().unwrap(),
                (r["Singular"].clone(), r["Map"].parse().unwrap_or(0)),
            )
        })
        .collect();
    let territories = read_csv(&csv_dir.join("TerritoryType.csv"));

    let install = icon_extract::GameInstall::discover(None).expect("install");
    eprintln!("client {}", install.version);
    let iw = install.ironworks();

    // `planmap.lgb` is left out: its only NPC placements are the graphics
    // benchmark scene (`LVD_benchmark_01`), which puts Gridania NPCs in the
    // Central Shroud. `bg.lgb` holds no NPCs.
    const FILES: [&str; 3] = ["planner.lgb", "planevent.lgb", "planlive.lgb"];
    let mut per_file: BTreeMap<&str, usize> = BTreeMap::new();
    let mut territories_read = 0usize;
    let mut parse_failures = Vec::new();
    // npc -> placements
    let mut placements: BTreeMap<u32, Vec<Placement>> = BTreeMap::new();
    let mut all_npc_objects = 0usize;

    // Instanced copies of a zone (quest battles, solo duties) reuse the zone's
    // layout directory under their own TerritoryType row. Read each directory
    // once and attribute it to the canonical row: the one whose Name is the
    // directory's zone code (`f1t1`), else the lowest id.
    let mut by_dir: BTreeMap<&str, &HashMap<String, String>> = BTreeMap::new();
    for t in &territories {
        let bg = &t["Bg"];
        let Some((dir, code)) = bg.rsplit_once('/') else {
            continue;
        };
        let entry = by_dir.entry(dir).or_insert(t);
        let canonical = |t: &HashMap<String, String>| t["Name"] == code;
        if canonical(t)
            || (!canonical(entry)
                && t["#"].parse::<u32>().unwrap() < entry["#"].parse::<u32>().unwrap())
        {
            *entry = t;
        }
    }
    for (dir, t) in &by_dir {
        let dir = *dir;
        let t = *t;
        let territory_id: u32 = t["#"].parse().unwrap();
        let main_map: u32 = t["Map"].parse().unwrap();
        let mut any = false;
        for file in FILES {
            let path = format!("bg/{dir}/{file}");
            let bytes = match iw.file::<Vec<u8>>(&path) {
                Ok(b) => b,
                Err(ironworks::Error::NotFound(_)) => continue,
                Err(e) => {
                    parse_failures.push(format!("{path}: read: {e}"));
                    continue;
                }
            };
            any = true;
            let parsed = match lgb::parse(&bytes) {
                Ok(p) => p,
                Err(e) => {
                    parse_failures.push(format!("{path}: {e:#}"));
                    continue;
                }
            };
            for layer in &parsed.layers {
                if layer.name.to_ascii_lowercase().contains("benchmark") {
                    continue;
                }
                for obj in &layer.objects {
                    let Some(base) = obj.enpc_base_id else {
                        continue;
                    };
                    debug_assert_eq!(obj.asset_type, ASSET_EVENT_NPC);
                    all_npc_objects += 1;
                    *per_file.entry(file).or_default() += 1;
                    // Prefer the resident's own Map when it belongs to this
                    // territory (multi-map cities), else the territory's map.
                    let resident = residents.get(&base);
                    let map_id = resident
                        .map(|r| r.1)
                        .filter(|m| maps.get(m).is_some_and(|mr| mr.territory == territory_id))
                        .unwrap_or(main_map);
                    let Some(map) = maps.get(&map_id) else {
                        continue;
                    };
                    if map.size_factor <= 0.0 {
                        continue;
                    }
                    let name = |id: u32| places.get(&id).cloned().unwrap_or_default();
                    placements.entry(base).or_default().push(Placement {
                        npc_id: base,
                        name: resident.map(|r| r.0.clone()).unwrap_or_default(),
                        territory_id,
                        map_id,
                        zone: name(t["PlaceName"].parse().unwrap()),
                        map_name: name(map.place_name),
                        subarea: name(map.place_name_sub),
                        x: map_coordinate(obj.translation[0] as f64, map.size_factor, map.offset_x),
                        y: map_coordinate(obj.translation[2] as f64, map.size_factor, map.offset_y),
                        festival_id: layer.festival_id,
                        file,
                    });
                    let _ = &map.id;
                }
            }
        }
        if any {
            territories_read += 1;
        }
    }

    // Dedupe identical placements (same npc, territory, ~same coordinate).
    for list in placements.values_mut() {
        list.sort_by(|a, b| {
            (a.territory_id, a.map_id)
                .cmp(&(b.territory_id, b.map_id))
                .then(a.x.partial_cmp(&b.x).unwrap())
                .then(a.y.partial_cmp(&b.y).unwrap())
        });
        list.dedup_by(|a, b| {
            a.territory_id == b.territory_id
                && a.map_id == b.map_id
                && (a.x - b.x).abs() < 0.05
                && (a.y - b.y).abs() < 0.05
        });
    }

    let covered: Vec<u32> = vendor_ids
        .iter()
        .copied()
        .filter(|id| placements.contains_key(id))
        .collect();
    let permanent: Vec<u32> = vendor_ids
        .iter()
        .copied()
        .filter(|id| {
            placements
                .get(id)
                .is_some_and(|p| p.iter().any(|x| x.festival_id == 0))
        })
        .collect();
    let missing: Vec<u32> = vendor_ids
        .iter()
        .copied()
        .filter(|id| !placements.contains_key(id))
        .collect();

    println!("territories with lgb files: {territories_read}");
    println!("event-npc objects total: {all_npc_objects}  per file: {per_file:?}");
    println!("parse/read failures: {}", parse_failures.len());
    for f in parse_failures.iter().take(10) {
        println!("  {f}");
    }
    println!("distinct NPCs placed: {}", placements.len());
    println!(
        "vendor NPCs: {}  placed: {} ({:.1}%)  placed on a permanent layer: {} ({:.1}%)  missing: {}",
        vendor_ids.len(),
        covered.len(),
        100.0 * covered.len() as f64 / vendor_ids.len() as f64,
        permanent.len(),
        100.0 * permanent.len() as f64 / vendor_ids.len() as f64,
        missing.len()
    );
    println!("\nsample placements:");
    for id in [
        1000101u32, 1000216, 1000218, 1001276, 1000391, 1002694, 1005425, 1027998,
    ] {
        match placements.get(&id) {
            Some(p) => {
                for x in p {
                    println!(
                        "  {id} {:<28} {:<26} {:<22} {:<16} X:{:.1} Y:{:.1} fest:{} {}",
                        x.name, x.zone, x.map_name, x.subarea, x.x, x.y, x.festival_id, x.file
                    );
                }
            }
            None => println!("  {id} (no placement)"),
        }
    }
    println!("\nmissing vendor NPCs (first 40):");
    for id in missing.iter().take(40) {
        let name = residents.get(id).map(|r| r.0.as_str()).unwrap_or("?");
        println!("  {id} {name}");
    }
    let multi: usize = covered
        .iter()
        .filter(|id| {
            let p = &placements[id];
            p.iter()
                .map(|x| x.territory_id)
                .collect::<BTreeSet<_>>()
                .len()
                > 1
        })
        .count();
    println!("\nvendor NPCs placed in more than one territory: {multi}");

    let out = serde_json::json!({
        "client_version": install.version,
        "vendor_npcs": vendor_ids.len(),
        "placed": covered.len(),
        "placed_permanent": permanent.len(),
        "missing": missing,
        "placements": vendor_ids.iter().filter_map(|id| placements.get(id).map(|p| (id.to_string(), p))).collect::<BTreeMap<_, _>>(),
    });
    std::fs::write(&args[3], serde_json::to_string_pretty(&out).unwrap()).expect("write out");
}
