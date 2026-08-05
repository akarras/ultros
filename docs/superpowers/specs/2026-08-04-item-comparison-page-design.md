# Item comparison ("flip verification") mode — design

**Date:** 2026-08-04
**Status:** Approved (brainstorm with Aaron)

## Purpose

When a user clicks through from a flip-finder row, the item page should answer
"is this flip still real?" — what it costs to buy on the source world, what it
actually sells for on the destination world, and the profit after tax. A
decision aid layered onto the existing item view, not a separate page.

## URL contract

```
/item/{SellWorld}/{item_id}?compare-buy-from={BuyWorld}
```

- `compare-buy-from` names the **buy** world; the page's path scope is the
  **sell** side, exactly as today.
- `item_href` (item_view_scope.rs) already carries the query string across
  world switches, so changing the sell world keeps the comparison alive. Its
  doc comment should be updated to name this param alongside
  `exclude-worlds`.
- Unknown buy world, buy world == the page's world scope, or a non-world page
  scope (DC/region path) → the card does not render. No error state.
- Dismissing the card removes the param using the `filter_query_signal`
  pattern (NOT plain `query_signal` — that scrolls to top and pushes a
  history entry).
- The world name in the param is percent-encoded/decoded the same way as the
  path scope (`Url::unescape`, matched against `lookup_world_by_name`).

## Data strategy (chosen approach)

**Second client fetch of the buy world.** When the param resolves to a valid
world different from the page scope, create a `Resource` keyed on
`(item_id, buy_world)` calling the existing `get_listings`
(`/api/v1/listings/{BuyWorld}/{item_id}`). The sell side (floor listings,
recent sales, freshness) is already on the page from the normal listings
resource.

Rejected alternatives: a new combined server endpoint (new API surface,
duplicated logic — YAGNI) and linking to the DC-scoped page (breaks for
cross-DC flips; the flip-finder is region-aware).

## The hero comparison card

Rendered above the listings (in the `DecisionHeader` region) when active.
Title frames the direction unambiguously: **"Flip route: Jenova → Gilgamesh"**
(localized, with world names interpolated). Dismissible via an X that clears
the param.

Three columns, stacking vertically on mobile:

1. **Buy on {BuyWorld}** — cheapest listing(s) with price × quantity, NQ/HQ
   handled the same way the flip-finder does, plus a freshness badge for the
   buy world's board (`last_updated` from its payload).
2. **Sell on {SellWorld}** — estimated sale price using the *same math as the
   flip-finder row* (median of recent sales, capped by the current world
   floor), plus sales velocity ("~N sales/day" from recent sale timestamps).
3. **Verdict** — profit per unit after 5% tax, and total for the cheapest
   stack. The tax is stated explicitly ("after 5% tax") so the number is
   trustworthy.

If the buy world currently has no listings, the card still renders with an
honest "no listings on {BuyWorld} right now" buy column — the user came to
verify a flip; "it's gone" is the answer. While the buy-side fetch is in
flight, the card shows a skeleton (SSR-deterministic widths).

## Shared math

Extract the flip-finder's estimate/profit computation (median-of-recent-sales
capped by world floor; `estimated * 0.95 - buy_price`) into a shared helper
used by both `analyzer.rs` and the card, so the card can never disagree with
the row the user clicked. Velocity derivation is shared the same way.

## Entry points

1. **Flip-finder rows:** the item link gains
   `?compare-buy-from={cheapest_world_name}` (the row already knows the
   cheapest world id; resolve to a name via world data).
2. **Item page itself:** the existing `SavingsVerdict` banner is dead code in
   practice — it needs a single-world scope *and* a multi-world listings
   payload, but a world-scoped listings request returns only that world
   (`world_cache.rs` `get_all_worlds_in`, `AnyResult::World => vec![id]`).
   Replace that path: when the page is world-scoped and the price-zone-scoped
   `CheapestPrices` map (already loaded globally; zone = the user's home
   DC/region from `get_price_zone`) shows a meaningfully
   cheaper world (reuse `MEANINGFUL_CROSS_WORLD_SAVINGS_GIL`), render the
   savings line with a **Compare** action that sets `?compare-buy-from=`.
   The `SavingsVerdict` machinery and its tests are removed/adapted in the
   same change. (In scope per brainstorm; it is the same surface.)

## i18n

Every card string goes through `leptos-i18n`, keys prefixed `item_compare_*`,
added to all 7 locale files with real translations (per CLAUDE.md).

## Error handling

- Buy-side fetch error → card renders the buy column in an error/unavailable
  state; the rest of the page is unaffected.
- All signal reads inside Suspense/Transition bodies use the existing
  `with_or`/`get_or_default` guards (disposed-signal panic class).
- The card must not introduce any HashMap-iteration-ordered DOM (SSR/CSR
  determinism rule).

## Testing

- Unit tests for the shared estimate/profit helper (fixtures mirroring the
  existing `SavingsVerdict` tests; wrap signal-touching tests in
  `Owner::new()`).
- Tests for param parsing/validation (unknown world, same-world, DC scope).
- `item_href` tests extended to show `compare-buy-from` is carried across
  world switches.
- Card rendering states (active / no-listings / invalid param) verified via
  the existing viewport-comparison recipe against prod CSS where practical.

## Out of scope

- Quantity-aware "buy through the stack" math (per-unit + cheapest-stack
  total only, matching the flip-finder).
- A general "compare with…" world picker on the item page.
- Multi-world comparison (more than one buy world at once).
