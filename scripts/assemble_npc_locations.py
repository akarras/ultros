#!/usr/bin/env python3
"""Assemble an English location research snapshot from manifest-pinned CSVs.

This deliberately does not change the application's rkyv schema. Run without
--source-dir to fetch pinned inputs, or supply an already downloaded CSV folder.
"""

import argparse
import csv
import hashlib
import io
import json
from pathlib import Path
import tomllib
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
SHEETS = (
    "Level", "Map", "PlaceName", "TerritoryType", "ENpcBase", "ENpcResident",
    "GilShop", "GilShopItem", "TopicSelect", "PreHandler", "Leve",
    "GuildleveAssignment",
)


def map_coordinate(position, scale, offset):
    """World X/Z -> map X/Y; preserve precision until presentation.

    Formula: xivapi/ffxiv-datamining/docs/MapCoordinates.md.
    World Y is elevation, not the second map coordinate.
    """
    if scale <= 0:
        raise ValueError("Map scale must be positive")
    factor = scale / 100
    return 41 / factor * (((position + offset) * factor + 1024) / 2048) + 1


def index(rows):
    return {int(row["#"]): row for row in rows}


def referenced_shops(event, shops, topics, handlers, seen=None):
    """Resolve direct shops and menu wrappers, tolerating cyclic handlers."""
    seen = set() if seen is None else seen
    if not event or event in seen:
        return set()
    seen.add(event)
    found = {event} if event in shops else set()
    if event in topics:
        for i in range(10):
            found |= referenced_shops(int(topics[event][f"Shop[{i}]"]), shops,
                                      topics, handlers, seen)
    if event in handlers:
        found |= referenced_shops(int(handlers[event]["Target"]), shops,
                                  topics, handlers, seen)
    return found


def assemble(tables, issuer_mapping):
    maps = index(tables["Map"])
    places = index(tables["PlaceName"])
    residents = index(tables["ENpcResident"])
    territories = index(tables["TerritoryType"])
    levels = index(tables["Level"])
    shops = index(tables["GilShop"])
    topics = index(tables["TopicSelect"])
    handlers = index(tables["PreHandler"])
    assignments = index(tables["GuildleveAssignment"])
    leve_issuers = {}
    for npc_id, leve_ids in issuer_mapping.items():
        if int(npc_id) not in residents:
            raise ValueError(f"Unknown leve issuer NPC {npc_id}")
        for leve_id in leve_ids:
            leve_issuers.setdefault(leve_id, set()).add(int(npc_id))

    def place(key):
        return places.get(int(key), {}).get("Name", "")

    def location(row):
        # Level.Type=8 is an event NPC placement; other Object values can
        # reference unrelated sheets and must not be treated as NPC IDs.
        if int(row["Type"]) != 8:
            return None
        map_row = maps.get(int(row["Map"]))
        territory = territories.get(int(row["Territory"]))
        if not map_row or not territory or not int(row["Map"]) or not int(row["Territory"]):
            return None
        scale = int(map_row["SizeFactor"])
        if scale <= 0:
            return None
        return {
            "level_id": int(row["#"]), "map_id": int(row["Map"]),
            "territory_id": int(row["Territory"]),
            "zone": place(territory["PlaceName"]),
            "map_name": place(map_row["PlaceName"]),
            "subarea": place(map_row["PlaceNameSub"]),
            "x": round(map_coordinate(float(row["X"]), scale, int(map_row["OffsetX"])), 6),
            "y": round(map_coordinate(float(row["Z"]), scale, int(map_row["OffsetY"])), 6),
        }

    vendor_shops = {}
    issuers = {}
    for npc in tables["ENpcBase"]:
        npc_id = int(npc["#"])
        events = {int(npc[f"ENpcData[{i}]"]) for i in range(32)} - {0}
        resolved = set()
        for event in events:
            resolved |= referenced_shops(event, shops, topics, handlers)
        if resolved:
            vendor_shops[npc_id] = sorted(resolved)
        assignment_ids = sorted(events & assignments.keys())
        if assignment_ids:
            issuers[npc_id] = assignment_ids

    leve_links = {}
    for leve in tables["Leve"]:
        if not leve["Name"]:
            continue
        level = levels.get(int(leve["LevelLevemete"]))
        npc_id = int(level["Object"]) if level and int(level["Type"]) == 8 else None
        leve_links[int(leve["#"])] = {
            "name": leve["Name"],
            # This is not a verified issuer relation. Craft leves may point
            # to their delivery recipient (e.g. In with the New -> Maisenta).
            "linked_npc_id": npc_id if npc_id in residents else None,
            "level_levemete_id": int(leve["LevelLevemete"]),
            "location": location(level) if level else None,
            "issued_place": place(leve["PlaceNameIssued"]),
            "issuer_npc_ids": sorted(leve_issuers[int(leve["#"])]) if int(leve["#"]) in leve_issuers else None,
        }

    wanted = set(vendor_shops) | set(issuers) | set(map(int, issuer_mapping)) | {
        leve["linked_npc_id"] for leve in leve_links.values()
        if leve["linked_npc_id"] is not None
    }
    npcs = {
        npc_id: {"name": residents.get(npc_id, {}).get("Singular", ""),
                 "gil_shop_ids": vendor_shops.get(npc_id, []),
                 "guildleve_assignment_ids": issuers.get(npc_id, []),
                 "locations": []}
        for npc_id in sorted(wanted)
    }
    for level_id in sorted(levels):
        row = levels[level_id]
        npc_id = int(row["Object"])
        if npc_id in npcs and (resolved := location(row)) is not None:
            npcs[npc_id]["locations"].append(resolved)

    missing = sorted(npc_id for npc_id in vendor_shops if not npcs[npc_id]["locations"])
    items = {}
    for row in tables["GilShopItem"]:
        shop_id = int(row["#"].split(".")[0])
        item_id = int(row["Item"])
        if item_id:
            items.setdefault(item_id, set()).add(shop_id)
    return {
        "npcs": npcs,
        "shops": {key: shops[key]["Name"] for key in sorted(shops)},
        "item_gil_shops": {key: sorted(value) for key, value in sorted(items.items())},
        "leves": dict(sorted(leve_links.items())),
        "coverage": {
            "vendor_npcs": len(vendor_shops),
            "vendor_npcs_with_locations": len(vendor_shops) - len(missing),
            "vendor_npcs_without_locations": missing,
            "assignment_npcs": len(issuers),
            "assignment_npcs_with_locations": sum(bool(npcs[key]["locations"]) for key in issuers),
            "named_leves": len(leve_links),
            "leves_with_linked_npc": sum(row["linked_npc_id"] is not None for row in leve_links.values()),
            "leves_with_linked_location": sum(row["location"] is not None for row in leve_links.values()),
            "mapped_leve_issuers": sum(row["issuer_npc_ids"] is not None for row in leve_links.values()),
        },
    }


def runtime_data(snapshot):
    """Only ship UI fields, deduplicating placements with identical labels/coords."""
    npcs = {}
    for npc_id, npc in snapshot["npcs"].items():
        locations = set()
        for loc in npc["locations"]:
            label = loc["zone"] or loc["map_name"]
            if loc["subarea"] and loc["subarea"] != label:
                label += " · " + loc["subarea"]
            if label:
                locations.add((label, loc["x"], loc["y"]))
        npcs[npc_id] = {"name": npc["name"], "locations": [
            {"label": label, "x": x, "y": y} for label, x, y in sorted(locations)
        ]}
    return {"npcs": npcs, "leve_issuers": {
        leve_id: leve["issuer_npc_ids"] for leve_id, leve in snapshot["leves"].items()
        if leve["issuer_npc_ids"]
    }}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-dir", type=Path)
    parser.add_argument("--output", type=Path, default=ROOT / "data/npc-locations/en.json")
    parser.add_argument("--runtime-output", type=Path, default=ROOT / "data/npc-locations/runtime.json")
    args = parser.parse_args()
    manifest = tomllib.loads((ROOT / "data/manifest.toml").read_text())
    source = manifest["sources"]["ffxiv-datamining"]
    base = source["url"].removesuffix(".git").replace("github.com", "raw.githubusercontent.com")
    tables, hashes = {}, {}
    for sheet in SHEETS:
        if args.source_dir:
            raw = (args.source_dir / f"{sheet}.csv").read_bytes()
        else:
            with urllib.request.urlopen(f"{base}/{source['sha']}/csv/en/{sheet}.csv", timeout=60) as response:
                raw = response.read()
        hashes[sheet] = hashlib.sha256(raw).hexdigest()
        tables[sheet] = list(csv.DictReader(io.StringIO(raw.decode("utf-8-sig"))))
    issuer_source = json.loads((ROOT / "data/npc-locations/leve-issuers.json").read_text())
    result = {"schema_version": 1, "language": "en", "source": source,
              "issuer_source": issuer_source["source"],
              "input_sha256": hashes, **assemble(tables, issuer_source["npc_leves"])}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    args.runtime_output.parent.mkdir(parents=True, exist_ok=True)
    args.runtime_output.write_text(json.dumps(runtime_data(result), ensure_ascii=False, separators=(",", ":")) + "\n")
    print(json.dumps({key: len(value) if isinstance(value, list) else value
                      for key, value in result["coverage"].items()}, indent=2))


if __name__ == "__main__":
    main()
