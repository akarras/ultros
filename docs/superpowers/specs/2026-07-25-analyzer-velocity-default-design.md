# Default sale-velocity filter on the analyzer tools

**Date:** 2026-07-25
**Status:** Approved, ready for implementation

## Problem

A user who opens `/analyzer` or `/vendor-resale` for the first time sees every
item that clears the profit threshold, including items that sell once a month.
Those rows look like the best flips on the page — high profit, high ROI — and
they are the ones a newcomer is most likely to buy and then sit on. The tools
already have a sale-velocity filter that fixes this, but it is off by default and
buried behind "More Filters," so the people who need it most never find it.

## Goal

A bare visit to any of the four analyzer tools lands on a velocity-filtered view,
and the control that produced that view is visible without opening a drawer.

## Scope

Four pages, two equivalent filters:

| Page | Param seeded | Meaning |
|---|---|---|
| `/analyzer` | `next-sale=1d` | avg sale duration < 1 day |
| `/vendor-resale` | `next-sale=1d` | avg sale duration < 1 day |
| `/recipe-analyzer` | `min-sales=1` | at least 1 sale/day |
| `/fc-crafting-analyzer` | `min-sales=1` | at least 1 sale/day |

Leve Analyzer and Venture Analyzer have no sale-velocity filter and are out of
scope. No filter logic changes; no new locale keys.

## Design

### Seeding the default

New module `ultros-frontend/ultros-app/src/query_defaults.rs`, a sibling of
`math.rs` and `freshness.rs`, exposing one helper. On mount, if the query
parameter is absent from the URL, the helper writes the default value. Each of
the four routes calls it once, next to its existing `query_signal` declaration.

Everything downstream is untouched. The input box, the `Next Sale ≤ 1d` chip and
its X button, and Clear All all keep operating on the same query signal, and the
URL stays honest and shareable.

Two details, both from `leptos_router` 0.8.14's `query_signal`:

- Its default `NavigateOptions` is `replace: false, scroll: true`. Seeding with
  those would push a history entry — one wasted back press before the user
  leaves the page — and scroll the window to top on load. The helper uses
  `query_signal_with_options` with `replace: true, scroll: false`.
- The helper reads the parameter untracked, so the effect has no reactive
  dependencies and fires exactly once per mount. Clearing the filter mid-session
  sticks; it is not re-seeded.

### Absent vs. explicitly empty

The seed fires only when the parameter is **absent**. A link that carries the
parameter is honored verbatim, which gives shared links a way to say "no limit":

- `?next-sale=` — present but unparseable, so `predicted_time` is `None` and the
  filter is skipped.
- `?min-sales=0` — every item has `daily_sales >= 0`.

Both forms already arise naturally when a user empties the input box.

Seeding cannot instead key off "the URL has no query string at all": the side
nav links to these routes with `?world={world}` already attached.

### Lifting the control out of "More Filters"

**Vendor Resale** — the secondary toolbar holds nothing but the Max Sale Time
field. Lifting it empties the drawer, so the `show_more` signal, the
"More Filters" button, and the secondary `<Toolbar>` are all deleted.

**Analyzer** — the drawer also holds Min Profit/Day, Min Buy, Last Sold Within,
and Show Suspicious, so the button stays; only Max Sale Time moves up.

Placement in both: immediately after "Min Sales" in the primary toolbar. The two
are the page's "how well does this actually sell" controls, and it puts the
seeded `1d` in a newcomer's line of sight, which is what makes the default
legible rather than mysterious.

Recipe Analyzer and FC Crafting Analyzer have no drawer — `min-sales` is already
in their primary row.

### Bundled fixes

- The four velocity params move to `query_signal_with_options` with
  `replace: true, scroll: false` for **all** writes, not just seeding. Today
  every keystroke in a duration or min-sales box pushes a history entry and
  scrolls the page to top.
- `vendor_resale.rs` has a hardcoded `placeholder="e.g. 7d 12h"` inside the block
  being moved; it becomes `t_string!(i18n, analyzer_placeholder_7d_12h)`.
  `vendor_resale.rs` already borrows analyzer keys such as
  `analyzer_tooltip_duration_format`, so no new locale entries are needed.

## Accepted consequence

The next-sale filter drops items whose `avg_sale_duration` is unknown —
`analyzer.rs:595` and `vendor_resale.rs:307` both end in `unwrap_or(false)`.
Making the filter a default therefore hides no-data items by default, and on
Vendor Resale, where `sale_summary` is an `Option`, that may shorten the list
noticeably.

The semantics stay as they are. An item with no sales data is not a good
recommendation for a newcomer, which is the audience this default serves. The
mitigation is visibility: the chip renders with a working X, and the control now
sits in the primary toolbar, so a user who wants the long tail can get it in one
click.

## Out of scope

`"More Filters"` / `"Fewer Filters"` are hardcoded English in `analyzer.rs`,
which violates the no-hardcoded-strings rule in CLAUDE.md. Vendor Resale's copy
disappears with the button. Analyzer's needs two new keys across seven locales
and is unrelated to this change; it gets its own task.

## Verification

- `./check_ci.sh` (fmt + clippy).
- Drive the app and confirm:
  - A bare `/analyzer` lands on `?next-sale=1d`, with the chip showing and the
    Max Sale Time field visible in the primary toolbar without expanding
    "More Filters".
  - The chip's X removes the filter and it stays removed while navigating within
    the page.
  - Back from a seeded page goes to the previous *page*, not to the unfiltered
    URL, and loading the page does not scroll the window.
  - `?next-sale=` renders unfiltered.
  - `/vendor-resale` no longer shows a "More Filters" button.
  - `/recipe-analyzer` and `/fc-crafting-analyzer` land on `?min-sales=1` with
    `1` in the Min Daily Sales box.
