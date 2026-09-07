# Item-page NPC locations

`en.json` is an English research snapshot assembled from the exact
`ffxiv-datamining` revision in `data/manifest.toml`. Source revision and SHA-256
hashes of every CSV input are embedded in the file. `runtime.json` is its compact
UI projection (about 76 KB uncompressed), embedded in both the server and WASM
client. It contains only NPC names/locations and explicit leve issuer mappings.
It is parsed once, with no live requests and no changes to the rkyv pack format.
The generator writes both files; tests reject a stale runtime projection.

Vendor cards show known locations and retain their external NPC links. Leve
cards show mapped quest givers and their locations. Missing records use explicit
unavailable labels. Multiple distinct placements are retained in stable order.
NPC names use the current game-data locale; zone/subarea names currently remain
English. UI labels are translated for all seven supported locales.

Regenerate with Python 3.11+ (standard library only):

```sh
python3 scripts/assemble_npc_locations.py
python3 -m unittest discover -s scripts -p 'test_assemble_npc_locations.py'
```

For offline regeneration, pass `--source-dir /path/to/csv/en`. That folder must
contain the pinned versions of the twelve sheets listed in the script; compare
the resulting input hashes with the checked-in snapshot. Local inputs are not
automatically verified against upstream. Output has no timestamp and uses stable
ordering, so the same inputs produce identical bytes.

## Contents and measured coverage

- Item -> gil shops -> NPCs, including direct shops and nested TopicSelect /
  PreHandler menus. This resolver handles more wrapper paths than the current UI.
- 629 gil-shop NPCs; 193 have usable event-NPC placements in Level (30.7%). The
  other 436 IDs are explicitly listed in `coverage.vendor_npcs_without_locations`.
  These counts cover resolved NPC records, not a guarantee of active, accessible
  vendors or every shop type.
- NPC locations include Level, map, and territory IDs, English zone/map/subarea
  names, and map X/Y coordinates. Multiple placements are preserved. Coordinates
  use world X/Z, map scale and offsets, following
  [the upstream conversion](https://github.com/xivapi/ffxiv-datamining/blob/master/docs/MapCoordinates.md).
  Stored precision is six decimal places; presentation should format separately.
- All 1,757 named leves have a resolvable `LevelLevemete` NPC and location.
- 36 NPCs directly reference GuildleveAssignment event handlers; all 36 have
  locations. These are candidate issuing NPCs, not per-leve issuer mappings.
- The pinned supplemental mapping supplies issuers for 1,622 of 1,757 named
  leves. The remaining 135 use the unavailable fallback.

## Levequest roles must remain distinct

`Leve.LevelLevemete -> Level.Object -> ENpcResident` is **not sufficient to
identify the NPC who offers each quest**. For example, leve 21, “In with the New”,
links to Maisenta (1001276), while Gontrant (1000101) is identified separately as
an assignment NPC. The [quest reference](https://ffxiv.consolegameswiki.com/wiki/In_with_the_New)
also distinguishes Gontrant as the giver and Maisenta as the recipient.
Do not label Maisenta as the quest giver based on this join.

The snapshot calls this field `linked_npc_id`, preserves the original Level ID
and issued place name. It fills `issuer_npc_ids` from the supplemental mapping,
leaving it null when unknown. The
GuildleveAssignment sheet identifies handlers and unlock quests, but does not
provide an explicit list of offered leve IDs. We do not infer issuers from zone
or proximity.

`leve-issuers.json` is normalized from
[Teamcraft's handmade levemete mapping](https://github.com/ffxiv-teamcraft/ffxiv-teamcraft/blob/d2b7bd064181288225d110ae45480d4cd27e4f08/libs/data/src/lib/handmade/levemetes.ts).
At that revision it assigns 1,622 of our 1,757 named leve IDs to 28 NPC IDs,
leaving 135 unmatched. All referenced issuer NPCs are included in the snapshot.
The source URL includes the exact revision, and its MIT license is retained in
`TEAMCRAFT-LICENSE`. Regression tests distinguish Gontrant (giver) from Maisenta
(recipient) for leve 21. This is curated community data, not a claim that every
quest has been independently checked in-game. Teamcraft's
[leve extractor](https://github.com/ffxiv-teamcraft/ffxiv-teamcraft/blob/d2b7bd064181288225d110ae45480d4cd27e4f08/apps/data-extraction/src/extractors/leves.extractor.ts)
also uses LevelLevemete's map as the delivery place for its data.

The same revision's
[NPC dataset](https://github.com/ffxiv-teamcraft/ffxiv-teamcraft/blob/d2b7bd064181288225d110ae45480d4cd27e4f08/libs/data/src/lib/json/npcs.json)
has position objects for 433 of our 629 vendor NPC IDs. Combined with our Level
records, that would cover 455 IDs (72.3%), before checking coordinate validity or
conflicts. It has not been imported. Its `zoneid` must not be assumed to be a
TerritoryType ID: for example NPC 1000391 has zoneid 53 and map 3, whereas our
Map sheet maps map 3 to territory 133. It also stores one position per NPC,
unlike the multiple placements retained here.

## Coverage improvements

1. Supplement Level placements: it misses most resolved gil vendors, including
   ordinary city vendors. Evaluate game-client placement extraction or a pinned
   supplemental dataset; measure coverage and preserve source provenance.
2. Fill the remaining quest issuer relationships independently of delivery NPC
   relationships. Include city-issued courier leves and expansion hubs in checks.
3. Handle event/housing/story-dependent locations and duplicate placements. A
   coordinate is not proof that an NPC or shop is currently accessible.
4. Add localized place names and consider integrating the records into the
   language packs when updating `xiv-gen` and `game-data-pack` together.

The browser regression probe is `integration/item-source-nav.cjs`; it compares
NPC details before and after hydration. Run it against a fresh worktree server.
