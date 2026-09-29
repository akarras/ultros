# Grid filters: ranges, units and the column menu

Date: 2026-09-22 · Status: approved in chat, building in two PRs

## Why

The right-click column menu was a long column of spelled-out buttons, and
its filter editor was a bare native input. Two filter systems ran side by
side:

1. **Per-route parameters** (`?profit=`, `?roi=`, `?min-sales=`, `?next-sale=`,
   …, ~27 declarations). Each is one hard-coded threshold, so a user could
   only ever say "at least" or "at most".
2. **Typed column filters** in `?gf=`: one `{op, value}` per column, with a
   flat operator list where "at least", "at most" and "between" are three
   separate modes.

`FilterAlias` already bridges (1) into (2), but editors, labels and parsing
never converged. Time columns were the worst case: market *Last sold* was
formatted text (only equality/contains worked), flip finder's `last_sold` is
seconds since, recipe's is a unix timestamp, and duration chips printed raw
seconds (`< 86400`).

## Decisions

### Data model and URL format

- `GridMetric` and `ColumnFilter` carry a `unit: Unit` (`Plain`, `Gil`,
  `Percent`, `Rate`, `Seconds`, `Hours`, `Timestamp`), default `Plain`.
  `ValueKind` stays `Number | Text | Mixed`; the unit only affects parsing,
  display and relative bounds.
- One numeric operator: `FilterOp::Range`, `value = "min,max"` with either side
  empty. Bounds are inclusive.
- `Gte`, `Lte`, `Lt`, `Between` stay in the enum and keep their exact meaning
  (strict `lt` included). They are **not rewritten on read**; the editor and
  chips view them through `MetricFilter::range_sides()`, and any edit writes
  `Range`. This keeps every saved view, cookie and alias test unchanged.
- A timestamp side may be relative: `-7d` = "7 days ago". Relative sides are
  resolved with `MetricFilter::resolved(now)` before filtering.
- `?gf=` is only ever applied in the browser (the server stores it as an opaque
  string in saved-view cookies), so the format change needs no server work.

### Unit layer (`ultros-grid-core/src/units.rs`)

Pure functions, no Leptos:

| Function | Purpose |
|---|---|
| `parse_input(unit, text)` | typed field → URL token (`Ok(None)` = open end) |
| `format_input(unit, token)` | token → exact editor text (round-trips) |
| `format_chip(unit, token)` | token → compact chip text / within / at |
| `resolve_token(token, now)` | token → comparable number |

Parsing: amounts accept `,` `_` space grouping and `k`/`m`/`b`; commas always
group. Percent accepts a trailing `%`. Durations accept `w d h m s` (and long
forms), combinable; **a bare number is hours**. Timestamps accept a duration
and store it as a relative token.

### Hydration

Grid filters run during SSR. A relative bound resolved against two different
clocks could put a boundary row on different sides and break hydration.
`QueryGrid` therefore captures `now` in a `SharedValue` — only when the URL
already carries a relative bound, which both server and browser see alike —
and uses it until the filters change.

### Editor and menu

- Header: column name, sort ↑/↓ icon links (active one highlighted), ✕.
- Filter card:
  - numeric: Min – Max fields with the unit inside and a live "= …" reading
    underneath (errors inline, Apply refuses invalid input);
  - timestamp: *Within* (24h/3d/7d/30d chips + custom) or *Range*
    (after/before `datetime-local`, local zone noted);
  - text: is / is not / contains with a value (choices become a select);
  - mixed: a Range / Value switch;
  - "Has a value" / "No value" presence chips on every kind;
  - Apply (check icon, Enter works) and a clear icon when a filter is active.
- Toolbar icons: move left/right, insert before/after, hide.
- Width row: auto-fit, `px` field (saves on Enter/blur), reset width.
- Footer: "Reset column layout" as a quiet link.
- Every icon button carries its localized label as `aria-label` and `title`.

### Migration (PR 2)

- Remove numeric `ColumnFilter::new(key, _, true)` declarations whose key is
  already an alias (skipped at runtime anyway).
- Flip finder `sales`, `next-sale`; vendor resale `sales`: alias to their
  columns where the column exists, else leave as controls.
- Vendor sell: drop the duplicate `profit` control.
- Recipe `?last-sold=` (duration): alias to the `last-sold` timestamp column's
  min side as `-{duration}` (bare number keeps meaning days); keep its evidence
  fetch firing for a `gf` filter on that column.
- Market `market-last-sold` becomes a `Timestamp` number; its cell keeps the
  same formatted text.
- Assign units: gil (profit, ppd, prices, cost, VWAP), percent (roi, drift,
  trend), rate (sales/day, vel), seconds/hours (ages, sale time, cadence,
  hours between sales), plain (counts, item level).

Out of scope: consolidating the app's other compact-number formatters;
flagging ~10 calculation-changing controls as `calculation` (Clear all resets
them today) — tracked separately.

## Testing

- `units.rs`: parse/format round trips per unit, invalid input, grouping,
  multipliers, bare-hours durations, UTC dates.
- `metrics.rs`: range sides of legacy ops, inclusive open ranges, relative
  resolution against a fixed `now`.
- `filter.rs`: chip text per unit, SSR markup of each editor kind, labelled
  sort icons.
- PR 2: legacy URL → expected `gf` per migrated route.
- Browser: flip finder, recipe analyzer, vendor resale; dark theme; 375px;
  keyboard (Tab cycle, Esc, Enter).
