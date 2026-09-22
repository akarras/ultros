# Analyzer consistency implementation

This follows the production and source audit in [report.md](report.md). It consolidates the existing grid stack; each analyzer keeps its domain calculations.

## Shared contracts

- Column definitions own native sort tokens, initial direction, picker labels, hints and availability. Header links, menus and legacy URLs resolve through the same metric registry. Domain comparators preserve Recipe's evidence tiers and missing-price policy.
- Sort resolution memoizes only ordering metadata, independently of window-dependent labels and picker text. Recipe comparisons avoid per-comparison reactive subscriptions; asynchronous column measurements run outside tracking because the measurement version already owns their dependencies.
- Column search includes labels, groups and hints. Follow-window statistics and fixed-window statistics have distinct picker descriptions. Item names keep a bounded initial width while manual resizing remains available.
- Shared and Flip view menus anchor to the full toolbar on phones, keeping their name and save controls inside the viewport. See the verified [mobile column search](21-implementation-mobile-columns.png) and [mobile Views menu](23-implementation-mobile-views-after.png).
- Currency Exchange also removes its old “Full Results” heading and enclosing panel ([#1492](https://github.com/akarras/ultros/issues/1492)). Views lives beside Columns and filters in the shared toolbar; the local World picker replaces the redundant home-world assumption note. See the [desktop](24-currency-shared-toolbar-desktop.png) and [mobile](25-currency-shared-toolbar-mobile.png) fixture captures.
- Explicit URLs take precedence over a chosen default, which takes precedence over the remembered last view, then the recommendation. An explicit version marker keeps an empty saved view empty. Recommendations and unrestricted views are always recoverable; named layouts retain their existing storage.
- Default filters are ordinary editable filters. Recommendations are tool-specific rather than imposing the same demand cutoff on every activity.

| Tool | Recommended eligibility |
| --- | --- |
| Recipe | At least one sale/day, positive profit, hide confirmed suspicious listing valuations |
| Venture | Positive return, historical outlier filtering, no extreme current-listing flag |
| Leve | Positive profit and historical outlier filtering |
| FC Crafting | Positive profit, complete ingredient prices, no extreme current-listing flag |
| Vendor Resale | Positive profit, predicted sale within one day, exclude suspicious listing valuations |
| Vendor Sell | Positive NPC arbitrage profit |
| Scrip Sources | Complete ingredient prices; efficiency ordering |
| Flip Finder | Existing realistic default: minimum purchase 5,000 gil, daily cadence, 30% ROI |
| Currency | Positive item value; no extreme current-listing flag |
| Trends | At least three recorded sales, exclude suspicious sales, no extreme current-listing flag |
| Item Explorer | No extreme current-listing flag |

## Listing evidence

The new shared assessment and Recipe guard flag a current price strictly above 50 times the matching sale median, using the same market scope, quality and selected history window. Vendor Resale retains its existing NQ recent-sale-sample guard; Trends retains its backend assessment of suspicious sales. These are conservative heuristics, not profitability guarantees. Missing comparison history remains unverified; failed and pending feeds stay distinct from measured zero.

Vendor Sell retains break-even and losing candidates so its visible profit filter controls eligibility. Vendor Resale and Trends place their existing suspicious-data guards explicitly in Recommended; clearing those filters or choosing Unrestricted removes them.

Recipe checks the listing valuation actually used by its pricing calculation. Other shared filters assess the current board independently of the chosen valuation basis. A historical-price custom view can remove this filter. Fixed NPC rewards and ingredient-cost tools do not inherit a resale-price guard.

## Scope and provenance

Venture, Leve, FC Crafting, Scrip Sources and Vendor Sell share a URL-backed World / Datacenter / Region price-scope control. Region remains the fallback for existing links. Native recent-sale samples continue to describe the selected world and are labeled separately from shared exact-quality market statistics.

Currency has a local world selector, so configuring a global home world is no longer required. Currency, Trends and Vendor Resale retain world-based sales calculations because their existing backend calculations reject aggregate scopes. Flip Finder retains a target sale world and purchase-location filters. Recipe retains its separate purchase and revenue scope roles; the explorer retains its existing world/DC/region picker.

New English labels use the existing English fallback in locales that do not yet have translations.

## Validation

Regression coverage includes unknown versus suspicious evidence, the strict 50× boundary, actual Recipe pricing, scope/quality matching, default preference precedence, explicit empty views, literal percent escapes in saved views, picker metadata/search, sort aliases/directions, domain comparators, and deterministic ties. Browser coverage is in `integration/analyzer-consistency.cjs`.

Commands, results, and the Windows debug-archive workaround are recorded in the [validation report](../../qa/analyzer-consistency-2026-09-21.md).

Validation after integrating the latest main is recorded separately in the [rebase validation report](../../qa/analyzer-rebase-2026-09-22.md).
