# Item page refactor design

**Date:** 2026-08-29
**Status:** Approved (chart below both tables; side-by-side tables at xl+;
separate PR from the compact-tool-header change)

## Problem

The item view (`ultros-frontend/ultros-app/src/routes/item_view.rs`) front-loads
content unrelated to the marketboard: a tall hero with the full item-stats
grid always visible, a Discord command chip nobody uses, and a large
"Cheapest Found" card — all above the sale-history chart, which itself sits
above the active-listings and sale-history tables users actually came for.

## Design

### 1. Remove the Discord command chip

Delete the `DiscordCommandChip` call from the hero and the component itself
(plus any i18n keys used only by it, from **all seven** locale files). The
bot has its own page (`/bot`); the item page stops mixing user stories.

### 2. Item details behind an accordion

The item-level badge + `ItemStats` grid (the Defense/Vitality/… block) move
into a collapsed disclosure labeled "Item details" under the title block.
Hero becomes: icon, `h1` name + clipboard, category link, and the
AddToList / Universalis / Garlandtools buttons.

- Collapsed by default on all viewports.
- New i18n key (e.g. `item_view_item_details`) added to all seven locales
  with real translations.
- SSR-deterministic: default state is static, no hydration mismatch risk.

### 3. Compact "Cheapest Found" strip

`MarketStatsPanel`'s tall card becomes a single wrapping row of stat chips:

- HQ cheapest, NQ cheapest, real price (median), active listings count +
  sales velocity, and the realtime Live badge.
- The crafting-recipe sub-card becomes an inline "Craft ~X gil" chip linking
  to the crafting/related section.
- `DecisionHeader` freshness badges stay, adjacent to the strip.

### 4. Reorder `ListingsContent`

New order:

1. `FlipRouteCard` + compact cheapest strip (`#overview`)
2. `ListingsPanel` — active listings (`#listings`)
3. `SalesDetails` — sale history table (start of `#history`)
4. `ChartWrapper` — sale history chart (`#history` wraps the table + chart
   together, so the History nav link lands on the table first)
5. `WorldMarketShare`
6. Ad

`SectionNav` link order updates to match the new visual order. Anchor ids
stay stable so deep links keep working.

### 5. Side-by-side tables on large monitors

At `xl:` and up, `ListingsPanel` and `SalesDetails` render in a
`grid grid-cols-1 xl:grid-cols-2` with `minmax(0,1fr)` columns — active
listings left, sale history right. Stacked (listings first) below `xl`.

- Breakpoint is deliberately `xl`, **not** `lg`: the sidebar makes a 1024px
  viewport no wider than 768px of content.
- Each table keeps its own `overflow-x-auto` wrapper as the escape hatch;
  beware the known popover-clipping interaction if either panel hosts
  dropdowns.
- Unequal panel heights are fine; no scroll syncing.

## Out of scope

No API/data changes. Realtime subscriptions, resources, and the
Suspense/Transition structure are untouched — components are reordered and
restyled only.

## Testing

- `./check_ci.sh` (fmt + clippy).
- Visual pass at 375px, 768px, 1280px (xl side-by-side), and wide desktop.
- Verify `#overview` / `#listings` / `#history` / `#related` anchors and
  `SectionNav` still scroll to the right places.
- Verify the accordion is collapsed on SSR and toggles client-side.
- Verify realtime updates still land in both tables after the reorder.
