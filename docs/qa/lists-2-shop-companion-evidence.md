# Lists 2.0 Shop and companion: evidence matrix (#1438)

Reconciles the Shop and companion requirements of the Lists 2.0 product
design (`docs/superpowers/specs/2026-09-10-lists-2-product-design.md`,
sections "Shop" and "Shopping companion") and the checklist in #1438 against
the code as of PR #1442 (`claude/build-to-shop-handoff-gaps-d15ca6`, commit
`6fabe5e3`) plus this branch. It records which check exercises each row.
It does not verify the production deployment and it does not close the
physical game checks, which live in #1444.

Status: **implemented** (code exists and a listed check exercises it),
**partial** (code exists; a named part of the requirement is not met),
**missing** (no code), **unverified** (code exists; no listed check has
exercised it). "Unit" means `cargo test -p ultros-app list_shop`; "browser"
names a script under `integration/`. A row marked with a script is a claim
about what the script asserts, not a record of a passing run: the run log
for the reviewed build belongs in the "Runs" section below and is empty
until someone fills it in from an actual execution.

## Shop

| # | Requirement | Status | Code | Check |
|---|---|---|---|---|
| S1 | Uses the recipe planner's purchasing/travel engine | implemented | `list_shop.rs::scoped_candidates` → `ultros_calc::recipe_planner::{purchase, compare_routes}` | unit `actual_stack_cost_changes_cheapest_world` |
| S2 | Route choices: home world, fewest stops, lowest total cost | implemented | `candidates`; buttons `shop-home`, `shop-fewest`, `shop-cheapest` | browser `lists-v2.cjs`, `list-shop-handoff.cjs` (choose lowest cost); `shop-home` is disabled while no home world is set |
| S3 | Explain marginal travel value per extra world | partial | `list_shop_savings` compares fewest-stops against lowest-cost only | No per-hop ladder ("one extra world saves X; a third saves Y"). The recipe planner owns the left-to-right hop model under #1334; Shop should reuse it rather than grow a second one |
| S4 | Estimates come from observed listings, not reservations | implemented | copy `list_shop_listings_observed`, `list_shop_whole_stacks` | wording; shown inside the route comparison |
| S5 | Whole-stack purchase costs with surplus | implemented | `trip_totals`; `shop-totals`, `shop-estimate` | unit `whole_stack_totals_explain_the_difference_from_the_build_estimate`; browser journeys open `shop-estimate` |
| S6 | Respect quality and remaining quantities | implemented | `scoped_candidates` filters `hq` and plans `needed − acquired` | unit `overlapping_quality_rows_cannot_buy_the_same_listing_twice`, `foreign_quality_offer_does_not_hide_feasible_home_supply` |
| S7 | Respect allowed worlds/datacenters and travel constraints | partial | account lists: list scope plus the filter row's excluded worlds/datacenters (`filter_excluded`); device lists: the world/datacenter chosen for the price lookup | Exclusions are not exercised by a browser check. Datacenter hops only enter through `RouteContext.datacenters` weighting; there is no Shop-side control for travel constraints |
| S8 | Missing supply shown explicitly, never as zero cost | implemented | `ShoppingPlan.missing`; `list_shop_totals`, `list_shop_estimate_missing`, `shop-no-prices` | unit `no_supply_is_explicitly_missing`; browser `lists-v2.cjs` asserts `0 gil · 0 surplus · 5 missing` and the no-prices notice |
| S9 | Active trip grouped by world with stack sizes, expected cost, copy-name and partial-purchase controls | implemented | `snapshot`; `shop-stack`, `shop-stack-quantity`, `shop-stack-bought` | browser `list-shop-handoff.cjs` records 1 unit of a stack |
| S10 | Purchase progress immediately updates remaining work | implemented | `snapshot` reads the live `acquired` count against the frozen plan | unit `live_prices_do_not_move_active_trip_and_partial_purchase_updates_it`; browser `list-shop-handoff.cjs` (Build shows Owned = 1) |
| S11 | Undo for a purchase | partial | `on_undo` runs the document's undo | It undoes the **last document operation**, which is a Build edit if one happened after the purchase. A Shop-local reverse delta cannot replace it: `Edit::AddAcquired` with a negative delta is a no-op once the row has no room (`list_doc/adapter.rs`, `has_room`). Undo boundaries belong to Track A (#1430) |
| S12 | Freeze the active trip while market data changes | implemented | `Trip.source` snapshot | unit `live_prices_do_not_move_active_trip…`; browser journeys keep the trip across a Build edit |
| S13 | Reviewable refresh | implemented (PR #1442) | `refresh`, `Review`; `shop-drift`, `shop-review`, `shop-review-apply`, `shop-review-keep` | unit `a_review_leaves_the_active_trip_untouched_until_adopted`; browser journeys keep then adopt a refresh |
| S14 | "Listing is gone" replacement | implemented | `unavailable`; `shop-stack-gone` | unit `later_stack_is_disabled_until_earlier_stack_is_recorded` (replan excludes ids); browser `list-shop-handoff.cjs` (priced path) excludes a stack and adopts the reviewed replacement |
| S15 | Manual completion is the player's report, not proof | implemented | copy `list_shop_recorded`; `AutoMarkPurchases` is a separate, opt-in path | wording |
| S16 | Build → Shop handoff preserves quantities, quality, scope and acquired progress | implemented (PR #1442) | `shop_input` (account), `DeviceShop` input (device); Shop stays mounted, `shop-handoff` | browser journeys: 8 needed / 3 owned → "5 units left to buy"; account `?buy=true` server render |
| S17 | Guest and account parity | implemented | one `ListShop` component behind both routes | browser `lists-v2.cjs` (device) and `list-shop-handoff.cjs` (account) |
| S18 | Alternatives disclosed progressively | implemented | `<details>` route comparison below the checklist; `shop-estimate` details | browser journeys open the details |

## Shopping companion

| # | Requirement | Status | Code | Check |
|---|---|---|---|---|
| C1 | Compact reusable view for a second monitor, a normal window and Document Picture-in-Picture | implemented | `ultros/static/list-companion.mjs::openCompanion` | browser `list-companion.cjs`, `lists-v2.cjs` (fallback window) |
| C2 | Preferred action "Pop out shopping companion" | implemented | `open-shopping-companion` | browser `lists-v2.cjs` |
| C3 | Requested viewport ≈340×440, tolerant of clamping and resizing | partial | requests 360×480; fluid CSS, no fixed widths | resizing on a real window is **unverified** (#1444) |
| C4 | Content: world, progress, remaining items, quality, whole stacks, price, Copy name, Bought, Undo, Next world | implemented | `CompanionSnapshot`, `render()` | browser `list-companion.cjs` asserts every control and the action ids |
| C5 | Feature-detect PiP, open from the click, fall back to a window | implemented | `openCompanion` try/catch around `requestWindow` | browser scripts force the fallback; the real PiP path is **unverified** (headless Chromium has no PiP; #1444) |
| C6 | Real interactive HTML sharing the opener's document and actions; no second snapshot or socket | implemented | `dispatch()` calls the opener; the companion fetches nothing | browser `lists-v2.cjs` asserts no authenticated list writes |
| C7 | Cannot outlive the originating document; route change, sign-out, permission loss and deletion close it | implemented | `on_cleanup → browser::close`; the update effect closes on `!can_edit` or a changed list; `pagehide` | browser `lists-v2.cjs` (client-side navigation closes it). Sign-out, permission loss and deletion all reach the same `can_edit=false` path (`view_caps`) and are verified by reading, not by a browser check |
| C8 | Closing the companion does not lose edits | implemented | the opener owns all state | code review |
| C9 | Browser-owned chrome; no click-through overlay, input injection or global hotkey promise | implemented | copy `list_shop_keep_page`, `list_shop_regular_window` | wording |
| C10 | Normal-window fallback communicates that it is not always on top | implemented | `regularWindow` notice | browser `lists-v2.cjs` asserts the notice |
| C11 | Windows FFXIV borderless/windowed: visibility, focus, copy, marking purchases, DPI, resizing | **unverified** | — | Physical run required; tracked in #1444. Not claimable from browser automation |
| C12 | Exclusive fullscreen | **unverified, not advertised** | no copy claims it | Keep it that way until #1444 records evidence |
| C13 | Guest lists work in the companion | implemented | `DeviceShop` → `ListShop` | browser `lists-v2.cjs` |
| C14 | Labels follow the owner view's locale | implemented | `labels` map in the snapshot | browser `list-companion.cjs` (French labels) |

## #1438 checklist mapping

- Whole-stack costs, surplus, quality/scope constraints, missing supply: S5, S6, S7, S8.
- Stable itinerary: `planner::itinerary` orders stacks by (item, price, quantity, id) so recording one never reorders the rest; unit `later_stack_is_disabled_until_earlier_stack_is_recorded`.
- Listing-gone replacement, partial purchases, undo: S14, S9/S10, S11.
- Progressive disclosure and marginal travel savings: S18, S3 (coordinate with #1334).
- Guest/account parity, companion/opener shared state, cleanup, ordinary-window fallback: S17, C6/C8, C7, C1/C5/C10.
- Physical game testing: C11, C12, plus the real PiP path in C5 and resizing in C3, all in #1444.

## Runs

Record each execution against a named build here, with the command and the
exit status. An empty section means no run has been recorded for the
reviewed build; the tables above then describe coverage, not results.

| Date | Build | Command | Result |
|---|---|---|---|
| | | `cargo test -p ultros-app list_shop` | |
| | | `node integration/list-companion.cjs` | |
| | | `BASE_URL=… node integration/lists-v2.cjs` | |
| | | `BASE_URL=… node integration/list-shop-handoff.cjs` (test-auth build) | |
| | | `./check_ci.sh` | |

## Open items

- #1444 — physical FFXIV companion validation (C3, C5 real PiP, C11, C12).
- #1430 (Track A) — purchase-specific undo so "Undo purchase" cannot undo a Build edit (S11).
- #1334 — per-hop marginal savings ladder; Shop should reuse the planner's presentation rather than duplicate it (S3).
- Browser coverage still owed: excluded worlds/datacenters reaching Shop (S7); sign-out and permission-loss companion close (C7).
