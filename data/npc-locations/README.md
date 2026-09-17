# NPC locations and leve issuers

NPC positions and zone names now live in the game-data packs (`data/xiv-db`),
not here. `game-data-pack` reads every placement out of the client's `.lgb`
layout files (`icon-extract::lgb`), folds the ones the app can show — gil-shop
vendors and leve issuers — into `Data::npc_placements`, and records that same
set in `data/npc-placements.json` so a CSV-only rebuild (`--skip-icons`, no
game install) keeps them. Zone names come from the `PlaceName`, `Map` and
`TerritoryType` sheets in the pack, so they are translated like everything
else. The maps the pins sit on are `data/maps/maps.tar.zst`.

Measured against client 2026.09.15: 465 NPCs placed, versus 193 from the
`Level` sheet the previous JSON snapshot relied on. Every gil vendor without a
placement is either spawned inside a player estate (housing servants) or a
seasonal stall.

## `leve-issuers.json`

The one hand-kept input. `Leve.LevelLevemete -> Level.Object` names the NPC a
leve is *delivered* to, not the one who offers it: leve 21, "In with the New",
links to Maisenta (1001276) while Gontrant (1000101) issues it. No sheet lists
which leves each levemete offers, so the mapping is normalised from
[Teamcraft's handmade levemete table](https://github.com/ffxiv-teamcraft/ffxiv-teamcraft/blob/d2b7bd064181288225d110ae45480d4cd27e4f08/libs/data/src/lib/handmade/levemetes.ts)
(MIT, retained in `TEAMCRAFT-LICENSE`) as `{"npc_leves": {"<npc id>": [leve
ids]}}`, and `game-data-pack` inverts it into `Data::leve_issuers`. At that
revision it covers 1,622 of 1,757 named leves; the rest render "Quest giver
unavailable". This is curated community data, not a claim that every quest
has been checked in-game.

The browser regression probe is `integration/item-source-nav.cjs`; it compares
NPC details before and after hydration.
