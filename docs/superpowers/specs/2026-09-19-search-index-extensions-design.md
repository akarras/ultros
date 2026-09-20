# Search index extensions: currencies (fix), NPCs, ventures, scrip sources

**Date:** 2026-09-19
**Status:** approved design, not yet planned

## Problem

The global search (`Cmd/Ctrl+K`) is backed by a tantivy index built in RAM at
startup from the game-data pack (`ultros/src/search_service.rs`). It holds
marketable items, item search categories, job gear sets, "currencies", recipes
and a hardcoded list of tool/help pages. Three gaps:

1. **Currencies are wrong.** `SpecialShop.item` maps to the `Item[{}].Item[0]`
   column — the item the player *receives* — and the indexer only reads that
   field. It never touches `item_cost_*`. So the "currency" docs are received
   items that happen to sit in the Currency UI category ("Blue Crafters' Scrip
   Token" shows up) while real currencies ("Wolf Mark", "Fae Fancy") never do.
   The Currency Exchange page (`CurrencySelection` in
   `routes/currency_exchange.rs`) uses `shop_items()` with the proper
   cost/receive split and gets it right; the indexer "adapted" that logic
   instead of sharing it and drifted.
2. **Vendors, ventures and scrip turn-ins are not searchable at all.** Each has
   a page (`/npc/:id`, `/venture-analyzer`, `/scrip-sources`), but the latter
   two are tables with no per-row deep link, so there is nowhere for a hit to
   land.
3. **Several tools are missing from the static tool list** (Venture Analyzer,
   Scrip Sources, Vendor Resale, FC Crafting Analyzer, Trends, Leve Analyzer).

Recipes are already indexed (issue #1384) and are not changed by this work,
except that their ranking gets the "kind nudge" below like every other type.

## Goals

- Every currency the Currency Exchange page lists is findable by name.
- Every gil-shop NPC, every non-random venture reward and every crafted scrip
  turn-in is findable by name and lands on something useful.
- An item stays the top result for its own name; its recipe (and venture /
  scrip-source rows) remain visible below it, and adding a type word to the
  query ("iron ingot recipe", "coeurl skin venture") lifts that type to the top.
- One source of truth per record type, shared by the page that displays the
  records and the indexer, so the currency drift cannot recur.

## Non-goals

- Leve-issuer NPCs (they have placements but no page).
- Gatherer scrip sources (the Scrip Sources page cannot price gathered
  collectables and shows an "unsupported" empty state for them).
- Reverse lookups ("what can I buy with X", "what is X an ingredient of").
- Translating game-data subtitles (zone names, `MIN Lv. 30`). The pack is
  English game data everywhere else in search too.
- A type-prefix query language (`npc:merchant`). The kind nudge covers the
  need without new syntax.

## Design

### 1. Index documents

Schema additions in `SearchService::new`: one new searchable text field
`kind` (tokenizer `en_stem`, stored not required). Existing fields keep their
roles: `title` (searchable, high boost), `category` (searchable, low boost),
`display_category` (subtitle only), `type`, `url`, `icon_id`.

| type | one doc per | title | `category` (searchable) | subtitle | url | icon_id |
|---|---|---|---|---|---|---|
| `currency` (fixed) | item id from `exchange_currencies(data)` | item name | — | UI category name | `/currency-exchange/:id` | item id |
| `npc` | `ENpcResident` id appearing in any `Data::gil_shop_npcs` value | `singular`, title-cased | zone of first placement via `placement_label` (empty if none) | same zone | `/npc/:id` | 0 |
| `venture` | distinct reward item from `venture_rewards(data)`, lowest task id per item | item name | — | `{ClassJobCategory.name} Lv. {retainer_level}` e.g. `MIN Lv. 30` | `/venture-analyzer?item=:id` | item id |
| `scrip source` | crafted turn-in from `scrip_turn_ins(data)` (gatherer scrip types skipped) | item name | — | `{amount} {scrip name}` e.g. `12 Purple Crafters' Scrip` | `/scrip-sources?item=:id` | item id |
| `scrip source` | each crafter scrip currency (Orange Crafters', Purple Crafters') | scrip item name | — | `Scrip sources` | `/scrip-sources?scrip=OrangeCrafters` / `PurpleCrafters` | scrip item id |

`kind` field contents per type (the nudge words; see §3):

| type | kind terms |
|---|---|
| item | `item` |
| category | `category` |
| job equipment | `job gear equipment` |
| currency | `currency exchange` |
| recipe | `recipe craft crafting` |
| venture | `venture retainer` |
| npc | `npc vendor shop merchant` |
| scrip source | `scrip collectable turnin` |

Notes:

- The crafter scrips get a `scrip source` doc (earn it → sources page). They
  are *not* exchange currencies — no special shop takes them for a marketable
  item, so neither the exchange page nor the `currency` docs list them
  (verified against the pack; the only scrip that is one is Skybuilders').
  Gatherer scrips get no doc of either kind.
- The scrip currency items are resolved by matching `ScripType` to the scrip
  item in `data.items` by name (the same way the indexer already finds Gil and
  MGP); a test pins that each crafter `ScripType` resolves to exactly one item.
- `ENpcResident.singular` is stored lowercase ("merchant & mender"). Title-case
  it for display; the `en_stem` tokenizer lowercases anyway so search is
  unaffected. NPCs with no placement still get a doc with an empty zone — they
  have a page.
- Venture dedupe mirrors the recipe dedupe: several tasks can reward the same
  item at different levels; index the lowest task id so the list holds one row
  per item.
- The `category` field for NPC zones uses the existing low boosts (exact 1.0,
  fuzzy 0.1), so "merchant limsa" disambiguates without zone names crowding
  item results.

### 2. Shared source helpers

New module `ultros-frontend/ultros-app/src/game_sources.rs` (`pub`), holding
pure functions over `&xiv_gen::Data` with no Leptos dependency:

- `exchange_currencies(&Data) -> Vec<ItemId>` — extracted from
  `CurrencySelection` (special-shop rows whose received item is marketable →
  cost items → UI category ∈ {100 Currency, 61 Miscellany, 63 Other}, by id →
  minus Gil/MGP → unique, sorted by id). The page calls it.
- `venture_rewards(&Data) -> Vec<VentureReward>` with
  `VentureReward { task: RetainerTaskId, item: ItemId, quantity: i32, level: u8, class_job_category: ClassJobCategoryId }`
  — extracted from the venture analyzer's task loop (non-random tasks whose
  `RetainerTaskNormal` has a non-zero item and quantity). The page keeps its
  pricing loop but iterates this instead of walking the sheets itself.
- `scrip_turn_ins(&Data) -> Vec<ScripTurnIn>` and `ScripType` (with
  `from_filter_key`, the new inverse `filter_key() -> &'static str`, and
  `is_gatherer`) — moved out of `routes/scrip_sources.rs` and made `pub`.

`ultros` already depends on `ultros-app` (SSR feature), so the indexer imports
these directly. Existing tests for the moved logic move with it.

### 3. Ranking

`type_weight` (multiplier on the raw tantivy score, applied in `rank` before
the top-10 cut):

| type | weight |
|---|---|
| item, currency, category, job equipment, npc | 1.0 |
| recipe | 0.8 (unchanged) |
| scrip source | 0.75 |
| venture | 0.7 |

Rationale: a venture or scrip turn-in always shares its title with an item
and/or recipe doc, so it must sit below both by default; NPC names almost
never collide with item names, so there is nothing to demote against.

Kind nudge: the exact parser adds the `kind` field (boost 1.0, no fuzzy) for
*recall* only — it keeps "bastard sword recipe" from treating "recipe" as a
dead word so the recipe doc stays inside the candidate window. The ordering
is decided in `rank`: a type whose kind word appears in the query gets
`NUDGE_WEIGHT` = 1.5 instead of its `type_weight`, so it overtakes an equally
good item match (1.0) while a much better title match still wins. A BM25
boost alone cannot do this — a term shared by ten thousand documents never
outscores a rare title term, however large the boost (measured: at 2.0 the
recipe stayed below the item for every sampled craft).

`SEARCH_CANDIDATES` 30 → 60. With up to five docs per title, 30 raw
candidates can be exhausted by lookalike items before the weighted sort ever
sees the venture row.

### 4. Deep-link reveal (grid)

New optional prop on `VirtualGrid`
(`ultros-frontend/ultros-ui-grid/src/components/virtual_grid/mod.rs`):

```rust
#[prop(optional, into)] reveal_index: Option<Signal<Option<usize>>>,
```

An effect watches it; when it becomes `Some(i)` and `i < row count`, the grid
calls its existing `reveal(i + 1, active_col)` — the same scroll-into-view and
active-row styling keyboard navigation uses (`reveal` takes 1-based rows, 0
being the header). Forwarded verbatim through `QueryGrid` and `MarketGrid` as
optional props; no other caller changes.

On `/venture-analyzer` and `/scrip-sources`:

- Read `?item=` with `query_signal::<i32>("item")`. It is a navigation target,
  not a filter — `filter_query_signal` would be wrong here.
- Keep the final row list the grid hands back through the existing `on_rows`
  callback in a signal.
- `reveal_index = rows.iter().position(|row| row.item_id == item)`.

If the row is filtered out (a saved default view with `profit=50000`, say),
nothing highlights. The row is one filter-clear away and the URL stays
honest; the page does not auto-clear the user's filters.

### 5. Search box

(`ultros-frontend/ultros-ui-shell/src/components/search_box.rs`)

- **Type labels through i18n.** Replace the raw `result_type` in the subtitle
  with a lookup: `search_type_item`, `search_type_category`,
  `search_type_job_equipment`, `search_type_currency`, `search_type_recipe`,
  `search_type_venture`, `search_type_npc`, `search_type_scrip_source`,
  `search_type_tool`, `search_type_page`, `search_type_help`, in all seven
  locales. Unknown types fall back to the raw string. Subtitles from game data
  stay untranslated (non-goal).
- **Icons**: `npc` → `FaUserSolid`-style person icon (distinct from the job
  icon if one is available), `venture` → the retainer/venture icon already used
  in the side nav, `scrip source` → `FaCoinsSolid`. Item-backed types keep the
  item icon via `icon_id`.
- **Static tool list**: add Venture Analyzer (`/venture-analyzer`), Scrip
  Sources (`/scrip-sources`), Vendor Resale (`/vendor-resale`), FC Crafting
  Analyzer (`/fc-crafting-analyzer`), Trends (`/trends`), Leve Analyzer
  (`/leve-analyzer`). The static list's titles are pre-existing hardcoded
  English; translating them is a separate change and not part of this one.
- **Empty-state hints**: one extra line covering vendors and ventures so people
  discover the new coverage (new locale keys, all seven locales).

### 6. Testing

Backend (`search_service.rs`, same style as the existing tests: a real index
from embedded data, deterministic samples via `spread`):

- Currency: every id `exchange_currencies()` returns is findable by name
  within the ten rows as a `currency` doc; "Wolf Mark" and "Fae Fancy"
  specifically (the reported regression); "Blue Crafters' Scrip Token" is
  *not* a `currency` doc.
- NPC: a spread of gil-shop NPCs findable by name; for an NPC whose name is
  shared across zones, `"<name> <zone word>"` ranks that zone's NPC first.
- Venture: a spread of reward items findable within ten rows as `venture`;
  `"<name>"` lists `item` above `venture`; `"<name> venture"` lists `venture`
  first.
- Scrip: a spread of crafted turn-ins findable as `scrip source`; gatherer
  turn-ins produce no `scrip source` doc; each crafter `ScripType` resolves
  to exactly one scrip item and its doc lands on `?scrip=<key>`.
- Recipe nudge: for a spread of craftable marketable items, `"<name>"` lists
  `item` above `recipe` (existing test) and `"<name> recipe"` lists `recipe`
  first (new).
- `kind` boost calibration: the nudge tests above are what pin the 2.0 value;
  if a future weight change breaks them, the boost is the knob.

Shared helpers (`game_sources.rs`): the moved tests, plus
`exchange_currencies` contains Wolf Mark and Fae Fancy and excludes Gil/MGP.

Frontend:

- Unit test for the reveal-index derivation (rows + item → `Some(i)`; absent →
  `None`).
- E2E screenshot of `/venture-analyzer?item=<id>` and `/scrip-sources?item=<id>`
  showing the highlighted row scrolled into view (`integration/`).

### 7. Files touched

- `ultros/src/search_service.rs` — new doc types, `kind` field, weights,
  candidates, tests.
- `ultros-frontend/ultros-app/src/game_sources.rs` (new) and `lib.rs` (mod).
- `ultros-frontend/ultros-app/src/routes/currency_exchange.rs`,
  `venture_analyzer.rs`, `scrip_sources.rs` — call the shared helpers; the
  latter two add `?item=` reveal.
- `ultros-frontend/ultros-ui-grid/src/components/virtual_grid/mod.rs`,
  `query_grid.rs`; `ultros-frontend/ultros-app/src/analyzer_kit/market.rs` —
  `reveal_index` prop.
- `ultros-frontend/ultros-ui-shell/src/components/search_box.rs` — labels,
  icons, static tools, hints.
- `ultros-frontend/ultros-i18n/locales/{en,fr,de,ja,cn,ko,tc}.json` — new keys.
