# Retainer pages: live data

Date: 2026-09-07

## Goal

Make `/retainers/listings` and `/retainers/undercuts` update themselves when
the market changes, using the websocket client the item and list pages already
use, and show the same `RealtimeStatus` pill those pages show.

`/retainers/edit` and the public `/retainer/{id}` page are unchanged.

## Existing pieces

- `ultros-frontend/ultros-app/src/ws/realtime.rs` — `RealtimeClient` with
  `subscribe_market(filter, SocketMessageType, handler)`. Subscriptions are
  dropped by dropping the returned `RealtimeSubscription`. On the server side
  (`ssr` feature) every call is a no-op.
- `ultros_api_types::websocket::FilterPredicate` — `World`, `Items`, `And`,
  `Or`, plus `.and()` / `.or()` builders.
- `ultros-frontend/ultros-app/src/components/realtime_status.rs` —
  `RealtimeStatus { status: Signal<String>, last_update: Signal<Option<DateTime<Utc>>> }`.
  Status strings are `"connecting" | "live" | "reconnecting" | "offline"`.
- `ultros-frontend/ultros-app/src/routes/list_view.rs` lines ~398–460 — the
  refetch-on-event pattern this design copies: an `Effect` rebuilds the
  subscription whenever the resource resolves, and the handler calls
  `resource.refetch()`.

Listing events are deltas (`Added` / `Removed` / `Updated` carry only the
listings that changed), so the client cannot recompute "cheapest listing on
this world" from an event alone. That is why the pages refetch rather than
patch in place.

## Design

### `routes/retainer_live.rs` (new module)

Owns all live wiring for both pages. Public surface:

```rust
/// A (world_id, item_id) pair one of the user's retainers currently lists.
pub(crate) type ListedPair = (i32, i32);

/// `None` when there is nothing to subscribe to.
pub(crate) fn retainer_market_filter(pairs: &[ListedPair]) -> Option<FilterPredicate>;

/// True when `message` should trigger a refetch for a page showing `pairs`.
pub(crate) fn is_retainer_update_relevant(message: &ServerClient, pairs: &[ListedPair]) -> bool;

/// Signals the page hands to `RealtimeStatus`.
pub(crate) struct RetainerLive {
    pub status: Signal<String>,
    pub last_update: Signal<Option<DateTime<Utc>>>,
}

/// Wire a page's resource to the realtime client.
/// `pairs` is derived from the resource's current value (`None` while loading
/// or errored). `refetch` re-runs the resource.
pub(crate) fn use_retainer_live(
    pairs: Signal<Option<Vec<ListedPair>>>,
    refetch: impl Fn() + Clone + 'static,
) -> RetainerLive;
```

`retainer_market_filter` builds
`Items(<unique item ids>) AND (World(w1) OR World(w2) OR ...)` over the unique
worlds in `pairs`. Both pages use the same filter; the listings page's refetch
is cheap enough that a narrower retainer-name filter is not worth it.

`is_retainer_update_relevant` returns true for a `Listings` event whose
`(world_id, item_id)` is in `pairs`, true for `Stale` when `pairs` is non-empty,
false otherwise. (Server-side filtering already narrows events; this is the
client-side guard, same role as `is_list_market_update_relevant`.)

`use_retainer_live`:

- Holds the current `RealtimeSubscription` in a `StoredValue<Option<_>>` and
  clears it in `on_cleanup`.
- An `Effect` reads `pairs`. When `use_realtime()` is `None` it sets status
  `"offline"` and returns. When `pairs` is `None` or empty it drops any
  subscription and returns. Otherwise it drops the old subscription and
  subscribes with `retainer_market_filter`, `SocketMessageType::Listings`.
- Handler:
  - `Subscribed` → status `"live"`.
  - `Listings(_)` relevant per `is_retainer_update_relevant` → status `"live"`,
    `last_update = now`, schedule a debounced refetch.
  - `Stale` / `Error` → status `"reconnecting"`, `last_update = now`, refetch
    immediately (cancelling any pending debounced one).
  - anything else → ignored.
- Debounce: trailing 1.5 s using `gloo_timers::callback::Timeout`, stored in a
  `StoredValue<Option<Timeout>>`; a new event replaces the pending timeout.
  The timer is wasm-only (`#[cfg(not(feature = "ssr"))]`); on the server the
  whole hook is inert because `RealtimeClient` is inert there.

### Page changes (`routes/retainers.rs`)

Both `RetainerListings` and `RetainerUndercuts`:

- Derive `pairs: Signal<Option<Vec<ListedPair>>>` from their existing
  `retainers` resource (`Some(Ok(_))` → collect `(listing.world_id,
  listing.item_id)` over every retainer's listings). Pass
  `move || retainers.refetch()` as `refetch`.
- The undercuts page must watch **every** listing the user has, not only the
  rows currently undercut — an item that is cheapest now and gets undercut
  later would otherwise never trigger a refetch. `get_retainer_undercuts`
  therefore returns `UndercutReport { undercuts, listed: Vec<(i32, i32)> }`,
  where `listed` is the full set taken before the cheapest-filter runs.
- Render `<RealtimeStatus status last_update />` in the title row, right-aligned
  (`flex flex-wrap items-center justify-between gap-3`, the undercuts page
  already has that row; the listings page gets the same wrapper).
- Remove the `retainers_data_notice` paragraph.

### i18n

No new keys. `retainers_data_notice` is deleted from all seven locale files
(`en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`).

## Testing

- Unit tests in `retainer_live.rs` (native, no wasm):
  - `retainer_market_filter`: empty → `None`; one pair → `Items([i]) AND
    World(w)`; pairs across two worlds and a repeated item → unique ids, an
    `Or` of both worlds.
  - `is_retainer_update_relevant`: matching `Listings` → true; `Listings` for
    another world or item → false; `Stale` with pairs → true, with none →
    false; `Sales` → false.
- `./check_ci.sh` (fmt, clippy, tests).
- Manual check on prod after deploy: pill shows "Live" on both pages, and
  relisting an item in-game updates the undercuts table without a reload.
  A logged-in retainer view cannot be exercised against local data.

## Out of scope

In-place patching of rows, per-row "just changed" highlighting, sales events,
and any change to `/retainers/edit` or `/retainer/{id}`.
