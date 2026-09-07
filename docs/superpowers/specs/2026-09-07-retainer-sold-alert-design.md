# Retainer sold alert — design

Notify a user when one of their claimed retainers' listings sells. Universalis
never says "this listing sold"; it only emits `listings/remove` and a separate
`sales/add`. This feature infers a sale by pairing the two, and is tuned to
**miss a sale rather than report one that did not happen**.

## Evidence (2026-09-07)

Measured on a 15-minute capture of the full Universalis websocket (18.6k
removals, 14.7k sales) plus a read of the Universalis server source:

- A viewer's board upload carries listings and history together; Universalis
  publishes `sales/add` and `listings/remove` from that one snapshot as two
  detached tasks. 91% arrive within the same instant, 7% sale first, 2%
  removal first, worst observed gap 59 s. Both orders must be buffered.
- The buyer's own plugin POSTs a single-listing delete keyed on retainer,
  quantity and price, which reaches the websocket as a one-row
  `listings/remove` (Universalis PR #1442, merged 2026-08-28). The matching
  sale only arrives on the next board view.
- 23.8% of removals are reprices (same `listingID` re-added).
- `sales/add` is mostly stale history (median age at receipt 18 h). Ultros'
  `update_sales` already drops sales it has stored, so only newly inserted
  sales reach the history bus, but a first-time upload of an item's history
  still delivers old sales as "new".
- Of sales under 5 minutes old, 61% pair with a same-key removal. Of all
  pairs, 62.5% have one candidate removal, 21% several candidates from one
  retainer, 16.5% candidates from different retainers. Recall is capped by the
  source: a listing Universalis never saw before it sold has no removal event.

Match key: `(world_id, item_id, hq, price_per_unit, quantity)`. FFXIV has no
partial-stack purchase, so quantity matches exactly. A sale carries the buyer
but never the seller; a listing carries the retainer but never the buyer.
Hard bound: `active_listing.timestamp` is Universalis' `lastReviewTime`, the
retainer's last touch, and a sale of that listing has `sold_date >= timestamp`.

## Detection rule

A sold event fires for a user's retainer only when **all** of the following
hold:

1. A `listings/remove` event deleted a row whose `retainer_id` is one of the
   user's owned retainers.
2. No `listings/add` from the **same retainer** for the same
   `(world_id, item_id, hq, quantity)` arrives within the hold window.
   (Reprice exclusion.) The public `ActiveListing` type carries no Universalis
   `listingID` and is constructed as a literal in 37 places, so the check is
   keyed on the retainer instead. A retainer that sells one stack and relists
   an identical stack inside the window is skipped, which is the accepted
   failure direction.
3. A sale inserted by `update_sales` arrives on the history bus with the same
   match key, received within **300 s** either side of the removal.
4. `sale.sold_date >= removed_row.timestamp - 60 s` (clock-skew slack).
5. `now - sale.sold_date <= 24 h`. Older sales are history backfill.
6. Every removal with that match key received within the window, **from any
   retainer, owned or not**, belongs to the same retainer as the candidate.
   If a different retainer's listing with the same key was removed in the
   window, the sale is ambiguous and is dropped.

Each sale consumes exactly one pending removal (the earliest received). Three
identical listings from one retainer and two sales fire twice. Entries on both
sides expire after the window. When the window expires, an unmatched removal is
forgotten; it was a withdrawal, an ambiguous case, or a sale Universalis did
not witness.

No per-alert cooldown: each sale is a discrete event and suppressing one is a
miss with no benefit. The alert row keeps `cooldown_seconds` for schema
uniformity but the tracker ignores it.

## Backend

### Data

- New table `alert_retainer_sale (id, alert_id → alert.id ON DELETE CASCADE)`,
  mirroring `alert_retainer_undercut` minus the margin column. One row per
  alert; one alert per user is the expected shape but not enforced.
- No new event bus. The listener reloads its rules from the database on
  every event of the existing `alerts` bus (`alert::Model` add/update/remove),
  exactly as the price and list-update listeners do.
- New `alert_event` rows on each fire, with `item_id`, `matched_price` set to
  the sale's `price_per_item`, and `matched_listing_id` left null (the
  `active_listing` row is already deleted). `delivered`/`delivery_error` as
  the undercut listener records them.

### Matcher (`ultros/src/alerts/sold_matcher.rs`)

A pure `SoldMatcher` struct with no I/O, unit-testable from an event sequence.
There is **one matcher process-wide**, shared by every sold alert, holding the
union of all alerts' owned retainers. Pending removals from *every* retainer
are kept (needed for rule 6); at the measured ~15 removals/s and a 300 s
window that is ~4,500 small entries.

```
struct SoldMatcher {
    owned: HashSet<i32>,                                   // union across alerts
    pending_removals: HashMap<SaleKey, Vec<PendingRemoval>>, // all retainers
    pending_sales:    HashMap<SaleKey, Vec<PendingSale>>,
    window: TimeDelta,        // 300 s
    max_sale_age: TimeDelta,  // 24 h
    skew: TimeDelta,          // 60 s
}
fn on_removed(&mut self, listing: RemovedListing, now: DateTime<Utc>) -> Vec<SoldEvent>
fn on_added(&mut self, added: AddedListing, now: DateTime<Utc>)
fn on_sale(&mut self, sale: ObservedSale, now: DateTime<Utc>) -> Vec<SoldEvent>
fn expire(&mut self, now: DateTime<Utc>)
fn set_owned(&mut self, owned: HashSet<i32>)
```

- `on_removed`: push the row under its key with its receipt time, then run
  the match step for that key (a sale may already be waiting).
- `on_added`: delete every pending removal whose `(world, item, hq, quantity,
  retainer)` equals the added listing's (reprice).
- `on_sale`: drop the sale if older than `max_sale_age`; otherwise store it
  and run the match step. The step takes the oldest pending sale, collects
  candidate removals under the key received within `window` of the sale and
  satisfying rule 4, and: no candidates → the sale stays pending; candidates
  from more than one retainer → the sale is discarded, removals stay; one
  retainer → the earliest candidate is consumed and, if owned, a `SoldEvent
  { retainer_id, retainer_name, key, sold_at, buyer_name }` is emitted.
- `expire`: drop entries older than `window` from both maps.

`SoldEvent` carries no confidence field: after rule 6 every emitted event is
one the matcher stands behind.

### Listener (`ultros/src/alerts/sold_alert.rs`, `RetainerSaleListener`)

Same shape as `ListUpdateAlertListener`: a single task started by
`AlertManager`, selecting over the listings bus, the history bus, the
owned-retainer bus, the `alerts` bus, a stop channel, and a 30 s tick that
calls `expire`. Rules are `retainer_id → Vec<{alert_id, owner}>`, rebuilt from
`get_all_active_retainer_sale_alerts` plus each owner's retainer ids on every
`alerts` or owned-retainer event; the matcher's owned set is the union. On a
`SoldEvent` it formats the message, calls `dispatch_alert` for every alert
that owns that retainer, records an `alert_event` (`matched_price` = sale
price) and updates `last_fired_at`.

`AlertManager::start_manager` gains a `history: EventBus<SaleEventData>`
parameter, passed from `discord/mod.rs`.

Bus lag on either channel is handled with `handle_bus_recv` and simply
continues; a dropped event is a missed sale, which is the accepted failure
direction.

### Message

Title `Retainer sale: {item}`, body
`Your retainer {retainer} sold {quantity}× {item} for {price} gil each` with
`(HQ)` appended when HQ, followed by the total and a link; click URL
`/retainers/listings`.
Item name resolved through `xiv_gen_db` as the undercut message does. Discord
delivery is server-side English like the existing alerts.

### API

- `AlertTrigger::RetainerSold {}` (unit-like struct variant, serialized as
  `{"type":"retainer_sold"}`), handled in `create_alert` via a new
  `create_retainer_sold_alert_handler` that requires `endpoint_ids`, calls
  `db.create_retainer_sale_alert(owner, cooldown, &endpoint_ids)`, and
  publishes the new alert on the `alerts` bus.
- `list_alerts` appends rows from `get_user_retainer_sold_alerts`.
- `delete_alert` and `update_alert` (enabled, endpoint_ids) work through the
  shared `alert` row; deleting cascades to `alert_retainer_sale`, and the
  manager's `alerts` bus removal already tears down listeners by alert id.
- Discord: `/ffxiv retainer add_sale_alert` and `remove_sale_alert` beside the
  undercut pair, using `add_discord_retainer_sale_alert` (channel endpoint,
  same as `add_discord_retainer_alert`).

## Frontend

All strings via `leptos-i18n` with keys in every locale file.

- `AlertKind::Sold` in `alert_drawer.rs`. The kind toggle shows three options
  when no `preset_item` is given. The Sold form has no inputs beyond endpoint
  selection and a one-line explanation
  (`sold_alert_description`: fires when a claimed retainer's listing
  disappears and a matching sale is recorded; same-price listings from other
  sellers are skipped rather than guessed). `trigger_matches_kind` maps
  `RetainerSold` to `Sold`; the drawer's existing-alerts list renders it with
  `alerts_retainer_sold_rule`.
- `alert_rules_panel.rs`: `RetainerSold` row renders
  (`alerts_retainer_sold_rule`, "—", "—", "—"). The history tab needs no
  change; sold fires are ordinary `alert_event` rows.
- `/retainers/listings` (`RetainerListings` in `routes/retainers.rs`): a bell
  button beside the title, identical in markup to the one on the undercuts
  tab, opening `AlertDrawer` with `initial_kind=AlertKind::Sold`.
- `/alerts` hint block: mention `add_sale_alert` next to `add_undercut_alert`.

## Testing

Unit tests on `SoldMatcher` (no DB, no runtime):

1. Removal then matching sale fires once with the right retainer.
2. Sale then removal (reverse order) fires once.
3. Removal followed by an add from the same retainer for the same item,
   quality and quantity never fires (reprice).
4. Same key removed for two different retainers, one sale: no fire.
5. Same key removed three times for one retainer, two sales: fires twice.
6. Sale older than 24 h at receipt: no fire.
7. Sale `sold_date` before the removed row's `timestamp` (beyond slack): no
   fire.
8. Removal older than the window when the sale arrives: no fire.
9. `expire` drops both removals and sales older than the window.

Replay test: feed the 2026-09-07 capture JSONL (checked in under
`ultros/test_data/`, trimmed to a few hundred lines around known pairs) through
the matcher with a synthetic owned-retainer set and assert the fire count.

API tests follow the existing undercut handler tests in
`ultros/src/web/api/alerts.rs`. E2E: `integration/` screenshot of the listings
tab with the bell button and the drawer open on the Sold kind.

## Out of scope

- A "Sold" tab or ledger on `/retainers`; fires are visible on `/alerts`
  history. Candidate follow-up once fire volume is known.
- Global seller attribution on `sale_history` (approach B) and listing
  time-to-sell analytics (approach C). Both would reuse `SoldMatcher`.
- Any tunable on the alert (minimum gil, per-retainer selection).
