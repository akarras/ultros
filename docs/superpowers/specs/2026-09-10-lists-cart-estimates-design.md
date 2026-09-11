# Lists 2.0: quantity-aware cart estimates and honest supply states

Date: 2026-09-10. Parent: #1427 (Track C). Issues: #1431, #1432. Epic: #1372.
Status: contracts settled here because #1428 recorded none; Track B renders
row markup against the types below.

## Outcome

While a player builds a list, every row carries an estimated cost for the
units they still need, and the cart shows one estimated total. When prices
are missing, stale, loading, or failed, the estimate says so instead of
looking free or fully priced. The estimate is a market observation, never a
reservation, and it is distinct from Shop's whole-stack purchase plan.

## Contracts

These answer the questions #1428 was meant to settle for Track C.

- **Requested versus remaining.** The estimate prices the *remaining*
  quantity: `needed − owned`, clamped to zero, exactly as
  `remaining_quantity` and the legacy "Estimated Remaining Cost" do. Both
  `requested` and `remaining` are on the line result so a row can show
  either. A row with nothing remaining is `Acquired` and contributes zero.
- **NQ / HQ / Any.** NQ prices only NQ listings, HQ only HQ listings, Any
  prices both together. Any never prefers a quality; it takes the cheapest
  units regardless.
- **Pricing scope.** The estimator prices whatever listings it is handed.
  Account lists receive the server's listings for the list's own
  world/datacenter/region scope, filtered by the page's excluded worlds and
  datacenters. Device lists receive the listings the player looked up for
  the scope they picked in Shop. The result carries the scope the listings
  were served for so the presentation can name it.
- **Arithmetic.** Cheapest units first. Matching listings are sorted by
  unit price (then larger stacks, then listing id, for a stable order) and
  units are taken from each until the remaining quantity is covered. The
  line total is the sum of `unit price × units taken`. Partial stacks are
  allowed: this is the cost of the cheapest *units* on the board, not a
  purchasable basket. Shop keeps whole-stack cost and surplus; the two are
  labelled differently and never mixed.
- **Supply coverage.** When matching units fall short, the line is
  `PartialSupply` (or `NoSupply` at zero). Its total covers only the priced
  units and it reports the unpriced units. The cart total sums the priced
  units and is marked incomplete whenever any line is short. A short line
  is never shown as zero-cost or omitted.
- **Safe gil arithmetic.** Quantities and unit prices are `i32`, so every
  product and every per-line sum fits `i64`. Cart-level sums use
  `checked_add`; on overflow the total saturates and the cart is flagged
  `saturated`, which the presentation reports rather than showing a wrapped
  number.
- **Freshness.** `PriceFeed` describes the listings behind the estimate:
  `Loading` (nothing yet), `Missing { NotRequested | Failed }` (no prices
  and why), or `Observed { fetched_at, refresh_failed }`. `fetched_at` is
  the client-clock instant the listing response arrived — for account
  lists the REST read (including refetches the market socket triggers), for
  device lists the Shop lookup. It is *not* an ingest time and is never
  derived from a listing's own `timestamp` (a seller review time). A failed
  refresh keeps the previous observation and sets `refresh_failed`, so the
  cart shows "prices from <time>; refresh failed" rather than pretending
  the failure produced fresh prices. Offline editing keeps whatever was
  last observed and stays editable; there is no age threshold that flips an
  estimate to "stale" on its own.
- **Late responses.** A lookup that returns after the player changed scope
  or list is discarded: device lookups carry a generation counter, account
  fetches already pass `request_is_current`, and the account listings cache
  is keyed by the scope the document had when it was fetched, so a scope
  edit is a cache miss instead of serving the old scope's prices.

## Components

- `ultros-calc/src/list_estimate.rs` — pure, Leptos-free:
  `LineRequest`, `LineEstimate`, `LineStatus`, `CartEstimate`, `Coverage`,
  `PriceFeed`, `estimate_line`, `estimate_cart`, `PriceFeed::after_fetch`,
  and `fixtures` for the UI track. Deterministic unit tests live here.
- `ultros-app` glue — `ListWorkspaceSource` gains `market: Signal<PriceFeed>`
  and `scope_label: Signal<Option<String>>`. `ListBuildWorkspace` derives a
  `Memo<CartEstimate>` from `rows` and renders `ListEstimateSummary`: the
  total, a one-line status, and a details line. Row markup is unchanged
  (Track B owns it); the line estimates are available on the memo.
- Account page (`list_view_sync.rs`): records `fetched_at` on every
  successful listings fetch, keeps the previous observation with
  `refresh_failed` on a failed one, and keys the cache by document scope.
- Device page (`guest_lists.rs`): the Shop lookup drives the feed and
  guards late responses by generation.

## Status text

Compact line (always visible), by priority:

1. `Loading` → "Loading prices…"
2. `Missing(NotRequested)` → "Look up prices in Shop to estimate this list."
3. `Missing(Failed)` → "Prices unavailable right now. You can keep editing."
4. saturated → "Too large to total exactly."
5. coverage `None` → "No listed prices for these items yet."
6. coverage `Partial` → "{priced} of {lines} items priced · {units} units
   without a listed price"
7. coverage `Empty` → "Nothing left to buy."
8. otherwise → "Cheapest listed units for what you still need."

Detail line: "Prices fetched {relative time}" / "Prices fetched {time};
refresh failed" and, when known, "Prices for {scope}". Both lines carry
"Estimates, not reservations." semantics through the basis sentence.

## Testing

`cargo test -p ultros-calc` covers: multiple quantities and qualities, Any
taking mixed qualities, partially acquired rows, acquired rows, mixed
priced/partial/unpriced carts, zero supply, listing depth (cheapest stack
too small), item-id mismatch, scope change (same rows, different listing
sets), overflow boundaries (single-line maximum fits; three maximum lines
saturate), feed transitions (success, failure after success, failure with
nothing cached), and generation guards. `./check_ci.sh` and a fresh
`cargo leptos build` validate the app crate; the Labs list page is checked
in a browser for the summary rendering.

## Out of scope

Row redesign and the compact row's line-total cell (#1434), deletion
feedback (#1436), Shop changes (#1437/#1438), the legacy per-world
`ListSummary` panel (moves with #1434), and any Labs promotion.
