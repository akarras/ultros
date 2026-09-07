# Recipe planner: travel frontier cards

Date: 2026-09-07. Follows PR #1319 (travel-weighted routes).

## Problem

#1319 replaced the fixed "Stay home / Up to N worlds / Full scope" tiers with
the top five distinct routes ranked by gil plus a travel weight. That made the
cards rigid in a different way: several cards can share the same travel shape
at slightly different prices, the "Stay home" baseline only appears when it
happens to rank, the savings line is looked up from the cards so it vanishes
with the baseline, and the cheapest plan can fall off the end when it needs
many hops. The comparison lost its story.

## Model

The cards are the **Pareto frontier over (travel, gil)**, read left to right
from the shortest travel to the longest.

- **Travel shape** is `Travel { dc_hops, world_hops }` relative to the worlds
  already on the itinerary (home plus locked stops), as today.
- **Travel distance** orders shapes: the gil-equivalent travel cost from the
  craft-options weights (`dc_hops * dc_hop + world_hops * world_hop`), tie-
  broken by `(dc_hops, world_hops)`. This keeps the axis consistent with the
  ranking and tunable in Planner settings.
- **One card per shape**: the best plan for that shape, where best is
  `(missing, cost, worlds)` ascending.
- **Frontier**: walking shapes by ascending distance, a shape is kept only if
  its best plan strictly improves on every kept card to its left, where
  improvement is fewer missing units, or equal missing and lower gil. Every
  step right is therefore a real gain; a dearer-and-further route never shows.
- **Baseline** is the plan for the already-visited set. It is always card one
  even if it is incomplete or if no travel route beats it.
- **Cheapest** is always the rightmost card. The full-scope plan (every
  allowed world) is evaluated unconditionally, and allowing more worlds never
  raises gil, so the cheapest plan is always found and is by construction the
  last frontier point, however many hops it takes.
- **Cap of five cards.** When the frontier is longer, keep the baseline and
  the cheapest unconditionally, then fill the remaining slots with the
  frontier points whose marginal saving over the card kept to their left is
  largest, preserving distance order. Recompute marginal savings against the
  kept set once, not iteratively; this is a display trim, not a ranking.
- **Shared route** links that name a world set not on the frontier are still
  computed and shown as a pinned card, as today. It is inserted into the
  card row by travel distance, labelled "Shared route · ...", and never
  carries a badge or counts as the cheapest.
- Ranking for **default selection** is unchanged: absent `route`, the selected
  card is the frontier card with the lowest `rank` (missing, effective, cost,
  worlds). Legacy `visits` still selects the best-ranked card with at most
  that many extra worlds, now over the frontier cards.

## Engine changes (`ultros-app/src/recipe_planner.rs`)

- `TravelWeights::distance(&self, travel: Travel) -> i64` (saturating).
- `compare_routes` keeps its search (beam, single additions, DC seeds, full
  scope) and replaces the "collapse by world set, top five by rank" step with
  the frontier collapse above. It returns `RouteComparison`:

  ```rust
  pub struct RouteComparison {
      /// Frontier cards in ascending travel distance; `cards[0]` is the
      /// baseline (no new travel) and the last card is the cheapest found.
      pub cards: Vec<ShoppingPlan>,
  }
  impl RouteComparison {
      pub fn baseline(&self) -> Option<&ShoppingPlan>;
      pub fn cheapest(&self) -> Option<&ShoppingPlan>;
      /// Index of the card `rank` prefers: the default selection.
      pub fn best_value(&self) -> Option<usize>;
  }
  ```

  `ROUTE_LIMIT` stays 5 and is the card cap. `rank` is unchanged and public.
- A free function `frontier(plans: impl IntoIterator<Item = ShoppingPlan>,
  weights: &TravelWeights, limit: usize) -> Vec<ShoppingPlan>` does the
  collapse, so it is unit-testable without market data.

### Engine tests

- Two plans with the same shape keep only the cheaper.
- A further-and-dearer plan is dropped; a further-and-cheaper plan is kept.
- The baseline is kept when incomplete and when nothing beats it.
- A complete plan is kept to the right of a cheaper incomplete one.
- Cap: seven frontier points trim to five, keeping the first, the last, and
  the three largest marginal savings, in distance order.
- Distance ordering: 1 DC hop (10,000) sorts after 3 world hops (6,000) at
  default weights and before them when `dc_hop` is set to 1,000.
- `best_value` picks by `rank`, which may be neither the first nor the last.

## UI changes (`ultros-app/src/routes/recipe_view.rs`)

- Heading: "Stay home, or travel?" Subheading: "Shortest trip on the left,
  cheapest on the right · best-found".
- Card layout, each card unchanged in size:
  - Label: existing hop-shape label ("Stay home" / "No new travel" /
    "3× world hop" / "1× DC hop → 2× world hop"). The pinned shared card
    prefixes "Shared route · ".
  - Badges on the label row: **Best value** (brand colour) on
    `best_value()`, **Cheapest** (muted) on `cheapest()`, the last frontier
    card. When both apply to one card show "Best value · Cheapest". A
    single-card frontier shows no badges.
  - Headline: raw gil.
  - Stops line: world list grouped by DC, as today; when more than six worlds,
    show the first six and "+N more".
  - Subtext, a pure function `saving_line(baseline, plan) -> SavingLine`
    rendered per variant:
    - `Baseline` when `plan` is the baseline: "Everything from your starting
      world", or the easter egg **"Just stay home."** when the frontier has a
      single card.
    - `Saved(gil)` green: "{gil} saved vs staying home" when both complete
      and cheaper. On the frontier this is every non-baseline complete card
      once the baseline is complete.
    - `Completes` green: "Completes the recipe" when the baseline is short
      and the card is complete.
    - `Partial(missing)` amber: existing "{n} units unavailable · partial
      cost".
- Selection, `route` and `visits` URL behaviour, "Not here", locks, and the
  summary aside are unchanged apart from reading `cards` from
  `RouteComparison`. The default selection uses `best_value()` rather than
  index 0.
- Strings: every new user-facing string is a `leptos-i18n` key in all seven
  locales (`recipe_planner_route_heading` and `_subheading` updated;
  `recipe_planner_route_best_value`, `_cheapest`, `_saved_vs_home`,
  `_completes`, `_just_stay_home`, `_more_worlds`). The currently hardcoded
  "saved vs home" string moves under `_saved_vs_home`.
- The in-page "calculation details" paragraph and `docs/recipe-planner.md`
  describe the frontier: one card per travel shape, each cheaper than the
  last, cheapest always shown.

### UI tests

- `saving_line` covers the four variants and the easter-egg predicate.
- `legacy_visits` test updated to run over frontier-ordered cards.
- E2E (`integration/recipe-planner.cjs`): the first card contains "Stay
  home"; exactly one card carries the "Cheapest" badge; with the two-world
  fixture a hop card exists and carries the green saved-vs-home line; cards
  remain pairwise distinct; the existing total-follows-selection and reload
  assertions stay.

## Out of scope

Modelling travel as time or teleport fees; counting vendor stops as travel;
changing the search itself; any change to the itinerary, locks, or Not here.
