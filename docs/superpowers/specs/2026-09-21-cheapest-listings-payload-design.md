# Cheapest-listings payload: lazy client-only resource + columnar wire format

**Date:** 2026-09-21
**Status:** approved for planning

## Problem

`/api/v1/cheapest/{zone}` (the "global listings" resource, `CheapestPrices` in
`ultros-frontend/ultros-frontend-core/src/global_state/cheapest_prices.rs`) returns
~24k rows — 1.54 MB of JSON, ~145 KB gzip / ~151 KB br on the wire.

Measured on prod (2026-09-21, North-America):

| Page | HTML raw | of which cheapest map | HTML gzip |
|---|---|---|---|
| `/` | 1.28 MB | **1.16 MB** | 202 KB |
| `/list` | 1.25 MB | 1.16 MB | — |
| `/analyzer/Aether` | 1.25 MB | 1.16 MB | — |
| `/item/North-America/4` | 2.19 MB | 1.16 MB | — |

Two compounding causes:

1. **It is inlined into every SSR page.** `CheapestPrices::new()` is called at the app
   root (`ultros-app/src/lib.rs`, `provide_context(CheapestPrices::new())`) and uses a
   plain `Resource`. Leptos serializes every server-created `Resource` into
   `__RESOLVED_RESOURCES` whether or not anything reads it. Every consumer already gates
   its read behind a `hydrated` signal, so the SSR render never uses the data: the server
   does ~1 MB of serde + JS-string escaping per request for nothing, the browser downloads
   it before hydration can finish, then `JSON.parse`s and serde-deserializes it (through
   the custom `"id_hq"` string-key parser) into a `HashMap`. `ItemExplorer` shadows the
   context with a second plain `Resource` and has the same problem on explorer pages.
2. **The JSON shape is verbose.** Row-of-objects repeats four key names 24k times.

Encoder comparison on the real payload (24k rows, 32 worlds):

| Encoding | raw | gzip | br |
|---|---|---|---|
| current JSON objects | 1,541 KB | 145 KB | 151 KB |
| JSON array-of-arrays | 424 KB | 124 KB | 123 KB |
| **JSON columnar (struct-of-arrays, sorted)** | **376 KB** | **98 KB** | **83 KB** |
| postcard-style varint rows | 165 KB | 113 KB | 115 KB |
| binary columnar, delta ids, varint price, u8 world | 98 KB | 59 KB | 59 KB |

Generic binary encoders barely beat gzipped JSON (prices are high-entropy). Columnar
JSON gets most of the way to the purpose-built binary with zero new dependencies and stays
human-readable, so that is the format.

## Goals

- No page inlines the cheapest map into its HTML.
- Pages that never render a price never fetch it. Pages that do (item view, item
  explorer, job-set pages, recipe/related-items) fetch
  it once on hydration, then only refetch on zone change — same as today.
- The API fetch shrinks to ~83 KB br without changing the shape of the existing public
  endpoint.
- Consumers keep the in-memory types they use today (`CheapestListings`,
  `CheapestListingsMap`). No analyzer route or test fixture changes.

## Non-goals

- A binary wire format. Columnar JSON captures ~85% of the win; revisit only if the
  remaining ~25 KB matters later.
- Refactoring consumers to read the columnar shape directly. In-memory they want O(1)
  `(item_id, hq)` lookup or row iteration; converting 24k rows at the boundary is ~1 ms.
- Changing the analyzers' own scoped `/api/v1/cheapest` resources beyond what they gain
  for free from the smaller payload.
- Cloudflare cache rules for the endpoint (separate ops task; the `?format=columnar` URL
  is cache-key friendly).

## Design

### 1. Wire format: `?format=columnar`

`GET /api/v1/cheapest/{world}?format=columnar` returns

```json
{"item_id":[2,2,5,…],"hq":[false,true,false,…],"price":[12,300,7,…],"world_id":[73,74,34,…]}
```

- Four equal-length arrays, row `i` = `(item_id[i], hq[i], price[i], world_id[i])`.
- Rows are emitted in `(item_id, hq)` order. The analyzer already stores them in a
  `BTreeMap<ItemKey, _>` with that ordering, so this is free and it is what makes the
  columns compress well. The format does not *require* sorted input on decode.
- Without the query flag (or with any other value) the endpoint returns the existing
  `{"cheapest_listings":[{item_id,hq,cheapest_price,world_id},…]}` shape, unchanged, for
  outside consumers.
- Same `Cache-Control: max-age=15` on both shapes.

Server: `ultros/src/web/api/cheapest_per_world.rs` gains a `Query<CheapestFormat>`
extractor (`format: Option<String>`, matched against `"columnar"`) and builds the four
`Vec`s in one pass over `item_map` when requested.

### 2. Types: `ultros-api-types/src/cheapest_listings.rs`

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CheapestListingsColumnar {
    pub item_id: Vec<i32>,
    pub hq: Vec<bool>,
    pub price: Vec<i32>,
    pub world_id: Vec<i32>,
}

impl From<CheapestListings> for CheapestListingsColumnar { … }   // server side
impl From<CheapestListingsColumnar> for CheapestListings { … }   // client side
```

`From<CheapestListingsColumnar>` zips the four columns; if the lengths disagree it
truncates to the shortest (a malformed payload degrades to fewer rows rather than a
panic). Tests: round-trip equality on a small fixture, empty payload, and the
length-mismatch truncation.

The server handler serializes its own private `CheapestListingData` rows today; it will
instead build a `CheapestListingsColumnar` directly (it does not need the `From`, but the
type is shared so the JSON shape is defined in one place).

### 3. Frontend fetch: `ultros-frontend-core/src/api.rs`

`get_cheapest_listings` and `get_cheapest_listings_live` request
`/api/v1/cheapest/{world}?format=columnar` (the live variant appends `&rt=…`), deserialize
`CheapestListingsColumnar`, and return `CheapestListings` via `From`. Signatures are
unchanged, so every caller (root resource, item explorer, analyzers, retainer undercut)
gets the smaller payload with no edits.

### 4. Lazy, client-only `CheapestPrices`

```rust
#[derive(Clone, Copy)]
pub struct CheapestPrices {
    owner: StoredValue<Owner>,                              // app root owner
    listings: StoredValue<Option<LocalResource<Result<CheapestListingsMap, AppError>>>>,
}

impl CheapestPrices {
    pub fn new() -> Self                                    // root: no resource yet
    pub fn already_demanded(listings: LocalResource<…>) -> Self  // scoped shadowing
    /// Returns the shared resource, creating (and so fetching) it on the first
    /// call. Call at component setup, not inside a render closure.
    pub fn demand(&self) -> LocalResource<Result<CheapestListingsMap, AppError>>
}
```

`demand()` creates the `LocalResource` on first use, under the owner captured at
`new()` (the app root), so the resource outlives the component that demanded it and
is shared by every later consumer. The fetcher tracks the price-zone signal, so a zone
change refetches as before.

Why this shape:

- `LocalResource` is *always pending* during SSR and is never serialized. That is exactly
  the contract the consumers' `hydrated` gates already assume, so their SSR output is
  byte-identical to today and no hydration mismatch is introduced. It also means the
  server does zero work for this resource.
- Lazy *creation* rather than a "wanted" signal gated inside the fetcher. The first
  draft used a fetcher that returned a never-resolving future until a `wanted` signal
  flipped. That deadlocks: `reactive_graph`'s `AsyncDerived` task awaits the current
  future *inside* its notification loop, so once the pending future is being polled the
  signal change is never received. It only appeared to work when `demand()` ran
  synchronously during hydration (the task's `already_dirty` check discards the stale
  future before first poll); `ItemExplorer` demands from inside another resource's
  fetcher, after that window, and never got data.
- `LocalResource<T>` is `Copy` and reads as `Option<T>`, the same shape consumers use
  today (`.with(|data| data.as_ref()?.as_ref().ok()?)`), so consumer edits are confined
  to *where they obtain the handle*.
- The fields are private so a consumer cannot grab the resource without `demand()`.

`CheapestListingsMap` keeps its `Serialize`/`Deserialize` impls and the `"id_hq"` key
codec: analyzer routes still round-trip `CheapestListings` through their own resources
and existing tests cover the codec. Nothing in this change relies on them.

### 5. Consumers

Each obtains the resource with `demand()` at component setup instead of reading
`read_listings`:

| Site | Change |
|---|---|
| `ultros-ui-game/src/components/cheapest_price.rs` | `use_context::<CheapestPrices>().map(\|p\| p.demand())` |
| `ultros-ui-game/src/components/job_set_card.rs` | same |
| `ultros-app/src/routes/job_set_detail.rs` | same |
| `ultros-app/src/routes/item_view.rs` | `demand()` once outside the `Transition` closure; the closure reads the returned handle |
| `ultros-ui-crafting/src/components/related_items.rs` (two sites, one of which currently calls `use_context` inside a render closure) | `demand()` once at setup, capture the handle, read it in the closures |
| `ultros-app/src/routes/item_explorer.rs` `ItemList` | `demand()` |
| `ultros-app/src/routes/item_explorer.rs` `ItemExplorer` | scoped resource becomes a `LocalResource`; when the scope equals the cookie zone it does `global.demand().await`, otherwise fetches the scoped zone. It is wrapped as `CheapestPrices::already_demanded(listings)` so descendants' `demand()` calls hand back that resource |

`item_explorer.rs`'s shadowing constructor is the only place a second `CheapestPrices`
is built; give it a dedicated `CheapestPrices::already_demanded(listings)` constructor
rather than exposing the fields.

### 6. Error handling

- Fetch/deserialize failures surface as `Err(AppError)` inside the resource exactly as
  today; consumers already render skeletons/empty for `Err`.
- A length-mismatched columnar payload truncates to the shortest column (see §2) — the
  page shows fewer prices rather than crashing. The server always emits equal lengths.
- Unknown `format` values fall through to the legacy shape; no 400.

### 7. Testing

- Unit: `CheapestListingsColumnar` ⇄ `CheapestListings` round-trip, empty, and
  mismatched-length tests in `ultros-api-types`.
- Unit: server handler builds equal-length columns in `(item_id, hq)` order from a
  populated `CheapestListings` (pure function over `item_map`, tested without axum).
- `./check_ci.sh` clean.
- Manual, against a local serve:
  - `curl -H 'Accept-Encoding: identity' localhost:8080/ | grep -c _true` is `0` (was
    23942), and the same for `/list` and `/item/North-America/4`.
  - `curl 'localhost:8080/api/v1/cheapest/North-America'` still returns the legacy shape;
    `?format=columnar` returns four arrays of equal length.
  - In the browser: `/`, `/list`, `/alerts`, `/settings` issue no `cheapest` fetch (the
    home page's Recently Viewed rail prices via the sparklines endpoint, not this map); `/item/North-America/4` shows the zone-savings pill and one fetch;
    `/items/...?world=Aether` (explorer with non-cookie scope) shows one fetch for the
    scoped zone only.
  - No hydration warnings/panics in the console on any of the above.

## Expected outcome

Home page HTML drops from ~1.28 MB / 202 KB gzip to roughly 120 KB / 50 KB. Pages without
prices lose the 1.16 MB outright. The price fetch becomes a separate, cacheable ~83 KB br
request that no longer blocks hydration, and the server stops serializing a 1 MB string
per page render.
