# Item view layout revamp

Date: 2026-07-26
Route: `/item/:world/:id` — `ultros-frontend/ultros-app/src/routes/item_view.rs`

## Problem

The item page is a single vertical stack of ~13 blocks running eight to ten
screens on desktop. Four concrete failures:

1. **Listings dominate.** `HighQualityTable` and `LowQualityTable` render
   identical columns in two separate uncapped tables, each with its own
   "Show more" that expands to *every* listing. On a liquid item this inserts
   many screens of table between the reader and everything below it.
2. **No way to jump.** Nothing but the world menu is sticky. Finding the recipe
   panel or the sale history means scrolling past everyone else's content.
   Anchors (`#listings`, `#history`, `#crafting-recipes`) exist and are already
   linked from callouts, but there is no visible navigation that uses them.
3. **Market share sits too high.** `WorldMarketShare` renders third, above the
   chart, but "where is the supply" is a research question — it is not what
   most visitors came for.
4. **Item stats truncate.** `ItemStats` uses `lg:grid-cols-4` with `gap-x-8`
   inside the `minmax(320px,1.2fr)` grid track declared on the header, leaving
   roughly 40px per stat name. "Vitality" renders as "Vi…".

The deeper problem behind 2 and 3: the page serves four distinct jobs — buying,
pricing a sale, deciding craft-vs-buy, and researching a market — in roughly
equal measure. Any single linear ordering is wrong for three of them.

## Approach

Introduce a **lens**: a Buy / Sell / Craft / Research selector that resequences
the page and changes what the top of it headlines.

The lens is **reorder-only**. Every section renders in every lens; nothing is
hidden, collapsed, or removed from the DOM. The lens changes CSS `order` and
the content of one hero block. This keeps the full page visible to search, to
Ctrl-F, and to readers who do not notice the control exists.

Because the lens does not shorten the page, in-page navigation and bounded
section heights carry the rest of the fix. They are not optional extras.

### Why CSS order

`item_view.rs` has a long history of tachys hydration panics under
out-of-order streaming SSR — GlitchTip #6831, #6864, #6865, and the
`with_or` / `get_or_default` helpers and the `hydrated`-flag idiom exist
solely to work around them. See the comment blocks at `item_view.rs:43-61`,
`item_view.rs:696-724`, and `listings_table.rs:51-73`.

Reordering with CSS `order` on a flex column adds **no new hydration
surface**: DOM order is identical on server and client in every lens, no
component re-mounts, and no resource refetches. Lens switching is a class
change. The verdict hero does vary per lens, but it is driven by a query
parameter that resolves identically on both sides, so it stays deterministic.

Accepted cost: tab order follows DOM order, not visual order. The jump-nav
mitigates this for keyboard users, and DOM order equals Buy order — the
default and the majority case.

## Sections

Nine sections, always rendered. **DOM order is Buy order** — the numbering
below *is* the source order, so the default lens needs no `order` overrides at
all.

| # | Section | Source today |
|---|---------|--------------|
| S1 | Verdict hero | `DecisionHeader` + `MarketStatsPanel`, merged |
| S2 | Bulk basket | new |
| S3 | Listings | `HighQualityTable` + `LowQualityTable`, merged |
| S4 | Price history chart | `ChartWrapper` |
| S5 | Sources | `VendorItems`, `ExchangeSources`, `LeveSources` |
| S6 | Related items | `RelatedItems`' item grid |
| S7 | Sale history + insights | `SalesDetails` |
| S8 | Market share | `WorldMarketShare` |
| S9 | Crafting recipes | `RelatedItems`' recipe block |

`DatacenterExclusionControls` stops being a section and becomes a popover on
the S3 header.

### Order per lens

| Lens | Order |
|------|-------|
| Buy (default) | S1 S2 S3 S4 S5 S6 S7 S8 S9 — DOM order, no overrides |
| Sell | S1 S3 S7 S4 S8 S5 S6 S2 S9 |
| Craft | S1 S9 S5 S3 S4 S6 S7 S8 S2 |
| Research | S1 S4 S8 S7 S3 S5 S6 S2 S9 |

### Verdict hero per lens

One hero card plus a quiet key/value column. This replaces the current
four-tile strip, where NQ, HQ, real price and listing count all carry equal
weight and therefore none of them reads as the answer.

| Lens | Headline | Supporting line | Data source |
|------|----------|-----------------|-------------|
| Buy | Cheapest NQ + world | cross-world savings | `cheapest_listing_for_quality`, `cheapest_savings_verdict` — both exist |
| Sell | Real price | undercut wall + days of stock | `real_price()` exists; wall and days-of-stock are new (see below) |
| Craft | Craft cost vs market, with margin | which is cheaper | `compute_cost()` and `min(listings)` both exist, never compared |
| Research | 30d VWAP vs spot | confidence band + sample size | `ItemStatsVariant.vwap_30d` / `.confidence_band` — fetched today, only the band is shown |

The key/value column carries the other three numbers plus the source callout
(`Craft for ~N`), so craft-vs-buy is answerable without scrolling in any lens.

## Chrome

Two tiers.

**Tier 1** — the existing `WorldMenu` pills, unchanged and *not* sticky. These
are roughly 30 crawlable `<a href>` links to sibling worlds on a page with a
canonical tag and per-world URLs; they stay as markup.

**Tier 2** — a slim single row that becomes sticky once tier 1 scrolls out of
view. Contents: compact scope dropdown, lens selector, jump-nav.

- Scope dropdown reuses `components/world_picker.rs::WorldPicker`, already used
  by the item explorer toolbar, alert drawers, and lists.
- Jump-nav links to the section anchors and highlights the current section via
  `IntersectionObserver`.

Rejected: stacking lens and jump-nav rows onto the existing three-row world
menu (chrome reaches ~45% of viewport height on a laptop), and replacing the
world pills outright with the dropdown (deletes the crawlable links).

## Lens state

Carried in the URL as `?lens=buy|sell|craft|research`, read through
`use_query_map` alongside the existing `exclude-worlds` param. Absent or
unrecognised means Buy.

- Makes "here's what to list this at" shareable.
- No canonical risk: `item_view.rs:1866` already emits
  `https://ultros.app/item/{id}` with no world and no query, so every lens
  already collapses to one canonical URL.
- The lens must survive world switching — `WorldButton`'s `href` needs to
  preserve the current query string.

Cookie persistence is explicitly out of scope; revisit if users ask for it.

## Bounded listings

`ListingsTable` gains a max height with an internal scroller and a
`position: sticky` header row. "Show more" expands the row set inside that
box; the page height does not change.

HQ and NQ merge into one table with an All / HQ / NQ segmented filter. This
halves the section's height, removes the second "Show more", and makes
cross-quality price comparison possible — today it requires scrolling between
two tables. HQ rows keep a quality marker so the merged view stays readable.

The `<For>` / dynamic-sibling constraint documented at `listings_table.rs:51-73`
must be preserved: the "show more" row stays inside a `{ move || … }` block so
it supplies the marker node bounding the keyed list. The segmented filter must
not reintroduce a static element directly after the `<For>`.

## New computations

All three are derived from data the page already fetches.

**Bulk basket (S2).** Listings carry `quantity` and nothing reads it. Given a
target quantity, walk each world's price-ascending ladder and total the
cheapest basket, reporting per-world total, average unit price, retainer count,
and short-fall when a world cannot fill the order. The cheapest single listing
is frequently the wrong answer for anyone buying in bulk.

Placement: position 2, with the quantity stepper defaulting to **1** so it
renders as one compact row. Raising the quantity expands it into the per-world
comparison.

**Undercut wall + days of stock (Sell hero).** The wall is the price-ascending
run of listings on the viewer's home world, plus the gap above it. Days of
stock is `listing count ÷ sales per day` — two numbers the page already renders
separately in `MarketStatsPanel` and never divides.

**Launder suspicion.** `ItemStatsVariant.launder_suspicion` is fetched by
`ChartWrapper` via `get_item_stats` and never surfaced. Show it as a warning
chip on the Sell and Research heroes above a threshold.

## Stat truncation fix

`components/stats_display.rs:119`: drop `lg:grid-cols-4` to two columns and
reduce `gap-x-8`. Contained entirely within `ItemStats`; no layout change to
the header grid at `item_view.rs:1930`.

## Phasing

**Phase 1 — no lens.** Stat fix, merged bounded listings table, two-tier sticky
chrome with jump-nav, market share moved to the bottom. Ships all four reported
problems fixed. Nothing here is throwaway; all of it is load-bearing for the
lens.

**Phase 2 — lens machinery.** `?lens=` parsing, CSS order map, per-lens verdict
hero, lens preserved across world switches.

**Phase 3 — new blocks.** Bulk basket, undercut wall, days of stock, launder
suspicion, VWAP vs spot.

Each phase is independently shippable.

## Constraints

- Every new user-facing string goes through `leptos-i18n` and must be added
  with a real translation to all seven locale files (`en`, `fr`, `de`, `ja`,
  `cn`, `ko`, `tc`). See `CLAUDE.md`.
- `./check_ci.sh` before every commit.
- Pure functions (basket walk, undercut wall, days of stock, lens parsing,
  order mapping) get unit tests alongside the existing tests at the foot of
  `item_view.rs`.
- No regression in the hydration workarounds cited above; new reactive reads on
  this route use the `with_or` / `get_or_default` accessors.

## Out of scope

- Cookie or account-level lens persistence.
- Changing the world menu's information architecture beyond adding the compact
  dropdown to tier 2.
- Redesigning the chart, recipe panels, or related-items grid internals. They
  move; they do not change.
