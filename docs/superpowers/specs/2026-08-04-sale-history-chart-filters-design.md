# Sale history chart: scope-aware defaults, legible ranges, shareable URLs

**Date:** 2026-08-04
**Surface:** the price/sale-history chart on the item view page
(`ultros-frontend/ultros-app/src/routes/item_view.rs`,
`components/price_history_chart.rs`, `components/chart_toolbar.rs`)

## Problem

Four independent defects and gaps in the same control surface:

1. **The grouping default ignores the viewed scope.** `item_view.rs` hardcodes
   `signal(GroupLevel::World)`. The corrective `Effect` in
   `price_history_chart.rs` only fires when the current group is *invalid* for
   the scope — and `World` is valid at every scope. So a region page overlays
   ~70 world lines when the user asked to look at a region.

2. **The time-range label omits the year at every zoom level.**
   `format_timeline_ts` is hardcoded to `%m-%d %H:%M`. A chart spanning
   2023-02-21 → 2026-07-05 renders as `02-21 18:00 - 07-05 18:00`, which reads
   as a four-month window in the current year.

3. **No quick ranges.** The only range control is a single "Full range"
   button; every other window requires dragging the slicer.

4. **No chart state is shareable.** `mode`, `group`, `view`, `hq`, the
   overlays, the world filter and the time window are all plain `signal()`s.
   A customised chart cannot be linked or bookmarked.

## Non-goals

- Persisting hover/crosshair position.
- Changing what the chart draws, how series are bucketed, or any server-side
  query behaviour. This is entirely a control-surface and URL-state change.
- Truncating a long `show` expression on a region page. The base-picking rule
  below bounds it to roughly half the series; that is sufficient.

---

## Part 1 — Grouping defaults to the scope's own level

`available_group_levels` already returns levels broadest-first:

| Scope page | Levels offered |
|---|---|
| Region | `[Region, Datacenter, World]` |
| Datacenter | `[Datacenter, World]` |
| World | `[World]` |

The default becomes `options.first()`. `group` stops being a plain signal and
becomes a **derived read** over the URL param:

```rust
let group = Signal::derive(move || {
    group_param.get()
        .filter(|g| available_levels.get().contains(g))
        .unwrap_or_else(|| scope_default(&helper, &world.get()))
});
```

Consequences of this shape, all of them wanted:

- A shared `?group=region` link opened on a **world** page degrades to `World`
  rather than requesting a grouping the scope cannot serve.
- Navigating region → world re-derives with no write and no mount-time race.
- The corrective `Effect` at `price_history_chart.rs:779` becomes unreachable
  and is **deleted**, removing a write-back loop.

**Cost:** `set_group` and `set_mode` change from `WriteSignal<T>` to
`SignalSetter<T>` on both `PriceHistoryChart` and `ChartToolbar`.

---

## Part 2 — Span-adaptive range label

`format_timeline_ts` takes the selected span and picks a format from it:

| Selected span | Format | Renders as |
|---|---|---|
| ≥ 2 years | `%Y-%m` | `2023-02 – 2026-07` |
| ≥ 30 days | `%Y-%m-%d` | `2026-06-05 – 2026-07-05` |
| < 30 days | `%Y-%m-%d %H:%M` | `2026-07-01 09:00 – 2026-07-05 18:00` |

Under 30 days keeps the clock **and** gains the year: a 7-day drag into 2023 is
precisely where the current format misleads most.

The `< 30 days` string is long for the truncating container, so the label
element also carries a `title` with the full unambiguous range.

The same helper feeds the chart's `aria-label`
(`price_history_chart.rs:1266`), so the screen-reader announcement is fixed by
the same change.

Pure function; extend the existing `test_format_timeline_ts`.

---

## Part 3 — Quick-range buttons

The lone "Full range" button becomes a segmented row:

```
[ 7d ] [ 1mo ] [ 1y ] [ All ]
```

`All` is today's full-range behaviour (`set_selected_range(None)`).

**Anchored to `now`.** A preset sets `(now - N, now)`. `normalize_time_range`
already clamps to the available domain, so `1y` on an item with six months of
history correctly yields those six months rather than erroring.

**Disabled when the window would be empty** — i.e. `domain.1 < now - N`, the
dead-item case — with a tooltip naming the reason. The chart never goes blank
from a preset click, and the user learns why the button is unavailable.

**Active state is exact, not inferred.** A preset click writes `?range=1mo`, so
the pressed button is read straight off the param. There is no guessing whether
a dragged window "is" 30 days.

**No hydration risk.** `TimelineSlicer` renders inside
`<Show when=available_domain.is_some()>`, and `available_domain` derives from a
`LocalResource` that is `None` during SSR. Nothing `now`-dependent is ever
rendered server-side, so SSR and the first client render cannot diverge.

New i18n keys, all seven locales (`en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`):
`chart_range_7d`, `chart_range_1mo`, `chart_range_1y`, `chart_range_all`,
`chart_range_unavailable`.

---

## Part 4 — URL persistence

New module `components/chart_query.rs`: pure encode/parse helpers, plus
`FromStr`/`Display` for `ChartMode` and `GroupLevel`.

All reads and writes go through `filter_query_signal`
(`replace: true, scroll: false`). A plain `query_signal` would push a history
entry and scroll the page to the top on every toolbar click — filters are not
navigation.

### Parameters

| Param | Values | Absent means |
|---|---|---|
| `mode` | `price` · `candles` · `range` · `density` | `price` |
| `group` | `region` · `datacenter` · `world` | scope default (Part 1) |
| `view` | `overlay` · `grid` | `overlay` |
| `hq` | `true` | off |
| `range` | `7d` · `1mo` · `1y` | ⎫ full range |
| `from` + `to` | epoch seconds | ⎭ |
| `overlays` | `avg,trend,qty,pct,patches` or `none` | `avg` + `patches` |
| `show` | visibility expression (below) | everything visible |
| `sort` | `name` · `change` | `name` (grid only) |
| `cellscale` | `true` | off (grid only) |

### Time window: relative when possible, absolute when dragged

A preset click writes `?range=1mo`, so the link keeps meaning "the last month"
indefinitely. A slicer drag has no relative meaning, so it writes absolute
`?from=…&to=…` epoch seconds. On read, `range` takes precedence; then
`from`/`to`; then full range.

Relative ranges resolve against `now` **once per mount**, not continuously — a
chart does not need to slide in real time.

### `show` — a visibility expression

Named `show`, not `hide`: `hide=all` would read as "hide everything" while
meaning the opposite.

```
show := base ("," item)*
base := "all" | "none"
item := ("+" | "-")? name
```

- Under base `all`, a bare or `-`-prefixed name **excludes**. Under `none`, a
  bare or `+`-prefixed name **includes**. The sign is optional (the base
  implies polarity) but is what we emit — self-documenting beats terse.
- **The base may be omitted**: `?show=Gilgamesh` implies `all`, so a bare list
  still means "hide these". Convenient for hand-editing.
- Names match **case-insensitively**; canonical casing is emitted.
- On write, the encoder **picks whichever base is shorter** (hidden ≤ visible →
  `all,-…`, otherwise `none,+…`; ties favour `all`). This bounds the parameter
  to `ceil(n/2) + 1` tokens.
- Deltas are emitted alphabetically, so URLs are stable and diff-friendly.

Examples:

```
?show=all,-Gilgamesh                  everything except Gilgamesh
?show=none,+Sargatanas,+Gilgamesh     only those two
?show=Gilgamesh                       implicit `all` base — hide Gilgamesh
```

#### Two safety rules this syntax requires

**Fail open on a stale `none` link, but not on a deliberate hide-all.** The
series set depends on the group level, so a `?show=none,+Gilgamesh` link
authored at World grouping carries names matching nothing at Region grouping —
and `none` plus zero matches is a blank chart. An empty chart from a stale
link is indistinguishable from a bug. Therefore: **a `none` base whose
include-deltas all miss is discarded and everything is shown instead.** A bare
`?show=none` with *no* deltas is not stale in that sense — it's an explicit,
unambiguous "hide everything" and is honoured as written; only a `none` base
with unmatched deltas is treated as a broken link. The `all` base has no such
hazard (unmatched exclusions are inert), which is the second reason ties
favour `all`.

**`+` is safe only because of how leptos_router decodes.** It uses
`decodeURIComponent` / `percent_decode` (`leptos_router::location::Url::unescape`),
**not** form-urlencoded decoding, so `+` stays a literal `+` rather than
becoming a space. Under a form-decoding parser this syntax would silently
corrupt every `none,+…` link. This reasoning goes in a comment beside the
parser, with a test pinning the behaviour so a later "fix" to the decoder
cannot quietly break shared links.

### Separator

Comma, for both `show` and `overlays`. `ParamsMap::to_query_string` escapes
values via `Url::escape` (`encodeURIComponent` client-side), so generated links
carry `%2C`:

```
generated: ?show=all%2C-Gilgamesh%2C-Sargatanas&overlays=avg%2Ctrend
typed:     ?show=all,-Gilgamesh,-Sargatanas          (parses identically)
```

This round-trips correctly and copy-pastes correctly; only the generated form
is visually noisy. Accepted deliberately over a cleaner-escaping separator
(`.`) because comma is what anyone hand-editing would reach for first.

### Two structural decisions

**Nothing is seeded on mount.** An absent param means "use the default",
computed at read time. Seeding would dirty every URL on arrival, and this
repo has twice been bitten by mount-time query seeding losing races against the
world picker's rebuild (see the `WorldNavigator` seed race).

**Each component owns its own params.** `PriceHistoryChart` reads
`view` / `overlays` / `show` / `sort` / `cellscale` directly rather than
receiving them as props; otherwise its signature grows from 8 props to ~20.
The route component keeps only what gates a fetch: `mode`, `group`, `hq`, and
the time window. Because nothing is seeded, a `Suspense` remount is harmless —
reads and writes are idempotent against the current URL.

`overlays` uses a `none` sentinel rather than an empty value, so "all overlays
off" survives the round trip instead of parsing back as the default.

---

## Testing

Unit tests, all on pure functions (no reactive `Owner` required — a bare
`RwSignal::new` in an `ultros-app` test panics with "no Arena is active"):

- `format_timeline_ts` across all three span tiers, including a sub-30-day
  window in a past year.
- `GroupLevel` / `ChartMode` `FromStr` ↔ `Display` round trips, plus
  case-insensitive parsing.
- `show` expression: both bases; explicit and omitted signs; omitted base;
  case-insensitive matching; shorter-base selection at the boundary and on a
  tie; alphabetical delta ordering.
- `show` fail-open when the expression resolves to zero visible series.
- `show` parses a literal `+` (the decoder-behaviour pin described above).
- `overlays` round trip including the `none` sentinel.
- Range params: `range` takes precedence over `from`/`to`; preset disable
  predicate against a domain ending before the window.

**Known verification gap:** on the local debug build every `query_signal` URL
*write* is inert while reads still work. The parse/encode layer is fully
covered by the unit tests above, but end-to-end round-trip behaviour (click a
control → URL updates → reload restores it) cannot be confirmed locally in
debug. It needs a release build or prod. This will be stated plainly in the PR
rather than claimed as verified.

`./check_ci.sh` before commit, per `CLAUDE.md`. Note that clippy never lints
`#[cfg(feature = "hydrate")]` blocks, so any hydrate-gated code added here
is unlinted by CI.

## Files touched

| File | Change |
|---|---|
| `components/chart_query.rs` | **new** — param encode/parse, `show` grammar, tests |
| `routes/item_view.rs` | `mode`/`group`/`hq`/range become URL-derived; scope default |
| `components/price_history_chart.rs` | adaptive label, range buttons, own params, delete dead Effect |
| `components/chart_toolbar.rs` | `SignalSetter` prop types |
| `ultros-frontend/ultros-charts/src/data/grouping.rs` | `FromStr`/`Display` for `GroupLevel` |
| `ultros-frontend/ultros-charts/src/charts/mod.rs` | `FromStr`/`Display` for `ChartMode` |
| `locales/{en,fr,de,ja,cn,ko,tc}.json` | 5 new range-button keys |
