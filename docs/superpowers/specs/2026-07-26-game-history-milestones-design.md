# Game history service and patch milestone bands

**Status:** design
**Date:** 2026-07-26
**Sequence:** 4 of 4 (chart revamp). Structurally independent — could ship first.

## Problem

Prices move because the game changes. A new gear tier in 7.2 collapses demand
for the previous tier; a crafting recipe change moves a material overnight. The
chart shows the collapse but gives no way to attribute it, so a crafter has to
already know the patch calendar to read their own data.

The blocker is that **FFXIV's data files contain no release dates**. Verified
against the `ffxiv-datamining` submodule:

- `ExVersion.csv` — expansion ids and names, six rows. No dates.
- `PatchMark.csv` — a `SubCategory` column carrying patch numbers ×100
  (`330` = 3.3, `430` = 4.3). No dates.
- `Item.csv` — no `Patch` column. XIVAPI derives item-to-patch by diffing sheet
  row counts across client versions; it is not shipped in the CSVs.

And a second problem: **CN and KR run separate game versions on their own
schedules**, months behind Global. A single global patch calendar would draw
wrong bands for those regions. `ExVersion.csv` cannot detect this — the CN and
TC dumps list the same six expansions as EN (in a different column order), so
there is no version-skew signal in the game data at all.

## Goals

- Mark patch and expansion boundaries on the chart, per region track.
- Ship without depending on any external service being reachable.
- Add no new i18n keys for expansion names.

## Non-goals

- No in-game event tracking (Moogle Treasure Trove, Make It Rain). Not in game
  data, no reliable machine-readable source, ongoing manual burden. Explicitly
  out.
- No item-to-patch attribution ("this item was added in 7.2"). Interesting, but
  it needs the sheet-diffing that XIVAPI does and is a separate problem.

## Design

### Data model

```rust
pub enum PatchTrack { Global, China, Korea }

pub struct GamePatch {
    pub track: PatchTrack,
    /// 700 = 7.0, 715 = 7.15. Matches PatchMark's SubCategory convention.
    pub version: u16,
    pub released: NaiveDate,
    /// Index into ExVersion, for the localized expansion name.
    pub ex_version: u8,
}
```

Versions are language-neutral numbers. Names come from game data — see below.

### Region to track

The mapping must be **data with a Global default**, not a hardcoded match on
region names. `update_datacenters` creates regions from whatever `RegionName`
values Universalis reports, so the region list is dynamic; a region we have never
seen must fall back to Global rather than fail or render nothing.

| Track | Regions |
|---|---|
| China | `中国` |
| Korea | `한국` |
| Global | everything else, including unrecognised |

### Expansion names cost nothing

`ExVersion` is an existing xiv-gen feature (`"ex_version"` is in the generated
feature list) that is simply not enabled in the workspace `Cargo.toml` today.
Enabling it adds one six-row table to the rkyv blob.

Because xiv-gen data is already built per-language for all seven locales,
`ExVersion.Name` gives "Dawntrail" on `en` and ダークトレイル on `ja` for free.
Joining `GamePatch.ex_version` to it means **zero new i18n keys for expansion
names** — which matters given the seven-locale rule in `CLAUDE.md`. Point patch
labels are bare numbers ("7.2"), which need no translation either.

Only the surrounding UI strings (the overlay toggle label, the disabled-state
reason) are new keys.

### Seed first, poll second

The table ships as a checked-in Rust const array, complete through the current
patch. Patch dates are append-only historical facts: a checked-in table can
never become *wrong*, only incomplete. Appending ~4 rows a year is a one-line
diff.

This makes any poller an optimisation rather than a dependency — the feature
works before a poller exists, and an upstream going down or away never breaks
the chart.

`xiv-gen` is the wrong home for the table. `extra.toml` turns out to be a
generated feature-list dump, not a config for extra data, and there is no
existing hook for non-game tables — teaching `build.rs` to read a repo-local CSV
to express 60 constants would be more machinery than the problem deserves. The
table lives in a small module in `ultros-api-types` instead, so both the server
and the WASM chart can read it without a round trip.

**Deferred:** a poller in the shape of the existing Universalis client, writing
to a Postgres `game_patch` table that the endpoint prefers over the seed when
populated. Before building it, verify which upstreams actually carry CN/KR
dates — candidate sources (XIVAPI's patch list, community patch repositories,
Lodestone news) are overwhelmingly Global-only. If none carry CN/KR, the poller
automates the easy two-thirds and leaves the manual burden exactly where it
already was, which may not be worth the moving parts.

### Endpoint

`GET /api/v1/game-history` returns the whole table for all tracks. It is a few
KB, changes ~4 times a year, and is not per-item — so it is served with a long
`Cache-Control` and fetched once per session rather than per chart.

An optional `?track=` filter exists for callers that want one track, but the
default is everything, because the client may need to detect the multi-track
case described below.

### Rendering: patch bands

Background tint bands, one per **patch** (not per expansion):

- Hue inherited from the patch's expansion, so all 7.x bands share a hue and
  eras read at a glance.
- Alternating lightness between consecutive patches within an expansion, so
  neighbours separate without a line between them.
- A single edge line at expansion boundaries only.
- Label at each band's centre: patch number when the band is wide enough,
  expansion name on the row beneath.

Nothing is drawn over the data, so bands compose with candles and range ribbons
identically. The crafter question — "what did 7.2 do to this price?" — becomes
answerable by eye: find the 7.2 band, read the step at its left edge.

**Density mode is the exception.** A tinted background beneath a sequential
color ramp will misread as data. In Density, bands degrade to boundary lines
only.

Bands render as `Node::Rect` behind everything else, so they need no new scene
primitive.

### Density of marks follows zoom

The same level-of-detail principle as bucket widths. A fixed marker set is
either a picket fence at four years or an empty rail at thirty days.

| Visible span | Marked |
|---|---|
| > 2 years | expansion launches only |
| 6 months – 2 years | + major patches (x.0–x.5) |
| 30 days – 6 months | + point patches (x.x5) |
| < 30 days | nothing, usually |

### Multi-track scopes

At Region-level grouping a chart can show North-America and 中国 simultaneously —
two different patch timelines over one x-axis, with no correct single answer.

Rule: bands follow the viewed scope's track. When the visible series span more
than one track, **milestones turn off** and the caption line says why. Picking a
winner would silently mislabel half the chart, which is worse than showing
nothing.

### Two free milestones

Both come from ClickHouse at no extra query cost, folded into spec 1's
aggregate:

- **First recorded sale** — `min(sold_date)` for the item in scope.
- **Coverage start** — the earliest sale we hold for the scope's worlds at all.

These matter because they are honesty markers. A flat early stretch is either a
stable market or missing data, and the chart currently cannot tell you which. A
"4 year" view that implies four years of history we do not have is a
correctness problem, not a cosmetic one.

Rendered as a hatched or dimmed region before coverage start, distinct from
patch bands.

## Testing

Pure logic:

- Region name → track: `中国` → China, `한국` → Korea, `North-America` → Global,
  and an invented region name → Global (the fallback guard).
- The seed table is sorted, has no duplicate `(track, version)`, and every
  `ex_version` resolves against `ExVersion`.
- Every version in the seed's Global track appears in `PatchMark`'s
  `SubCategory` set — a cross-check against game data that catches typos in
  hand-entered version numbers.
- Mark selection at each zoom tier returns the documented subset.
- A series set spanning two tracks yields "milestones off".

Rendering:

- Bands tile the visible window with no gaps and no overlaps.
- Consecutive patches within an expansion differ in lightness; patches in
  different expansions differ in hue.
- Density mode emits boundary lines and zero `Rect` band nodes.
- Bands sit behind every data node in the scene's draw order.

i18n:

- Expansion labels resolve from `ExVersion` per locale, and the ja locale
  renders the Japanese name — the guard for the zero-new-keys claim.

## Risks

- **The seed table is hand-entered**, so a wrong date draws a band in the wrong
  place and misattributes a price move. The `PatchMark` cross-check catches bad
  version numbers but not bad dates. Worth sourcing carefully once and treating
  edits as needing review.
- **Enabling the `ex_version` feature** grows the rkyv blob for all seven
  languages. Six rows, so the impact should be negligible, but it is worth
  measuring since the blob is embedded.
- **CN/KR dates are the least verifiable part** of this and the most likely to
  go stale, since they are also the least likely to be automated.

## Open questions

- Should bands be on by default or opt-in? Leaning on by default at wide zooms
  where they are informative and few, off under 30 days where they are absent
  anyway — which is what the LOD table produces naturally, so possibly the
  question answers itself.
- Is there value in marking the *current* patch's start distinctly, since "since
  the last patch" is a common window a user would want to select? Possibly a
  shortcut on the timeline slicer rather than a band treatment.
