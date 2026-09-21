# Cheapest-listings payload Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop inlining the ~1.16 MB cheapest-listings map into every SSR page, fetch it lazily only on pages that render prices, and shrink the API payload with a columnar JSON shape.

**Architecture:** A new `CheapestListingsColumnar` wire type (struct-of-arrays) is served by `/api/v1/cheapest/{world}?format=columnar` and converted back to the existing `CheapestListings` at the fetch boundary, so consumers keep their in-memory types. The root `CheapestPrices` context becomes a `LocalResource` (never serialized, always pending on SSR) created lazily on the first `demand()` under the root owner, so only pages with a price cell fetch it.

**Tech Stack:** Rust, Leptos 0.8 (`LocalResource`), axum, serde. Spec: `docs/superpowers/specs/2026-09-21-cheapest-listings-payload-design.md`.

## Global Constraints

- Run `./check_ci.sh` (fmt + clippy `-D warnings`) before every commit. Redirect and check `$?` explicitly: `./check_ci.sh > "$SCRATCH/ci.log" 2>&1; echo "REAL_EXIT=$?"`.
- No new user-facing strings (no i18n work needed). Do not add any.
- Legacy `/api/v1/cheapest/{world}` response shape must remain byte-for-byte the same when `format` is absent or not `columnar`.
- Windows build env for the `ultros` crate: prepend `/c/Strawberry/perl/bin:/c/Strawberry/c/bin:` to `PATH`, set `OPENSSL_RUST_USE_NASM=0`, and set `CARGO_PROFILE_DEV_DEBUG=0` (the `ultros` test binary otherwise overflows the 4 GiB rlib limit). Use the shared target dir if one is configured (`cargo metadata --format-version 1 | jq -r .target_directory`).
- Consumers' `hydrated` gates stay exactly as they are; this change must not alter SSR HTML for the components (other than removing the serialized resource).

> **Deviation recorded during execution:** Task 4's first draft gated the fetcher on a
> `wanted` signal with a never-resolving future. That deadlocks `AsyncDerived` (it awaits
> the current future inside its notification loop), which showed up as the item explorer
> never fetching when its scope matched the cookie zone. The shipped version creates the
> `LocalResource` lazily on first `demand()` under the root owner instead; the spec's §4
> describes the final shape.

---

### Task 1: `CheapestListingsColumnar` wire type

**Files:**
- Modify: `ultros-api-types/src/cheapest_listings.rs` (add type after `CheapestListings`, ~line 18; add tests in the existing `mod tests`)

**Interfaces:**
- Produces: `pub struct CheapestListingsColumnar { pub item_id: Vec<i32>, pub hq: Vec<bool>, pub price: Vec<i32>, pub world_id: Vec<i32> }` with `impl From<CheapestListings> for CheapestListingsColumnar` and `impl From<CheapestListingsColumnar> for CheapestListings`. Used by Task 2 (server) and Task 3 (frontend fetch).

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `ultros-api-types/src/cheapest_listings.rs`:

```rust
    fn item(item_id: i32, hq: bool, cheapest_price: i32, world_id: i32) -> CheapestListingItem {
        CheapestListingItem {
            item_id,
            hq,
            cheapest_price,
            world_id,
        }
    }

    #[test]
    fn columnar_round_trips_rows_in_order() {
        let rows = CheapestListings {
            cheapest_listings: vec![item(2, false, 12, 73), item(2, true, 300, 74), item(5, false, 7, 34)],
        };
        let columnar = CheapestListingsColumnar::from(rows.clone());
        assert_eq!(columnar.item_id, vec![2, 2, 5]);
        assert_eq!(columnar.hq, vec![false, true, false]);
        assert_eq!(columnar.price, vec![12, 300, 7]);
        assert_eq!(columnar.world_id, vec![73, 74, 34]);
        assert_eq!(CheapestListings::from(columnar), rows);
    }

    #[test]
    fn columnar_serializes_as_four_arrays() {
        let columnar = CheapestListingsColumnar::from(CheapestListings {
            cheapest_listings: vec![item(2, true, 300, 74)],
        });
        let json = serde_json::to_string(&columnar).unwrap();
        assert_eq!(
            json,
            r#"{"item_id":[2],"hq":[true],"price":[300],"world_id":[74]}"#
        );
        let back: CheapestListingsColumnar = serde_json::from_str(&json).unwrap();
        assert_eq!(back, columnar);
    }

    #[test]
    fn columnar_empty_round_trips() {
        let empty = CheapestListingsColumnar::default();
        assert_eq!(CheapestListings::from(empty).cheapest_listings, vec![]);
        assert_eq!(
            CheapestListingsColumnar::from(CheapestListings::default()),
            CheapestListingsColumnar::default()
        );
    }

    #[test]
    fn columnar_mismatched_lengths_truncate_to_shortest() {
        let columnar = CheapestListingsColumnar {
            item_id: vec![1, 2, 3],
            hq: vec![false, true],
            price: vec![10, 20, 30],
            world_id: vec![7, 8, 9],
        };
        let rows = CheapestListings::from(columnar);
        assert_eq!(rows.cheapest_listings, vec![item(1, false, 10, 7), item(2, true, 20, 8)]);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ultros-api-types columnar`
Expected: compile error, `CheapestListingsColumnar` not found.

- [ ] **Step 3: Add the type and conversions**

Insert after the `CheapestListings` struct definition (after line ~18):

```rust
/// Struct-of-arrays wire form of [`CheapestListings`] — what
/// `/api/v1/cheapest/{world}?format=columnar` returns. Row `i` is
/// `(item_id[i], hq[i], price[i], world_id[i])`. The server emits rows in
/// `(item_id, hq)` order, which is what makes the columns compress well
/// (~83 KB brotli vs ~150 KB for the row-of-objects shape on a 24k-row
/// region), but decoding does not depend on the order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CheapestListingsColumnar {
    pub item_id: Vec<i32>,
    pub hq: Vec<bool>,
    pub price: Vec<i32>,
    pub world_id: Vec<i32>,
}

impl From<CheapestListings> for CheapestListingsColumnar {
    fn from(value: CheapestListings) -> Self {
        let n = value.cheapest_listings.len();
        let mut out = Self {
            item_id: Vec::with_capacity(n),
            hq: Vec::with_capacity(n),
            price: Vec::with_capacity(n),
            world_id: Vec::with_capacity(n),
        };
        for row in value.cheapest_listings {
            out.item_id.push(row.item_id);
            out.hq.push(row.hq);
            out.price.push(row.cheapest_price);
            out.world_id.push(row.world_id);
        }
        out
    }
}

impl From<CheapestListingsColumnar> for CheapestListings {
    /// Zips the four columns. A malformed payload with unequal column
    /// lengths degrades to the rows every column has rather than panicking.
    fn from(value: CheapestListingsColumnar) -> Self {
        let cheapest_listings = value
            .item_id
            .into_iter()
            .zip(value.hq)
            .zip(value.price)
            .zip(value.world_id)
            .map(|(((item_id, hq), cheapest_price), world_id)| CheapestListingItem {
                item_id,
                hq,
                cheapest_price,
                world_id,
            })
            .collect();
        Self { cheapest_listings }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ultros-api-types columnar`
Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add ultros-api-types/src/cheapest_listings.rs
git commit -m "feat(api-types): CheapestListingsColumnar struct-of-arrays wire type

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 2: Server serves `?format=columnar`

**Files:**
- Modify: `ultros/src/web/api/cheapest_per_world.rs` (whole file, ~55 lines)

**Interfaces:**
- Consumes: `CheapestListingsColumnar` from Task 1; `crate::analyzer_service::CheapestListings` (`pub(crate) item_map: BTreeMap<ItemKey, CheapestListingValue>`), `ItemKey { item_id: i32, hq: bool }`, `CheapestListingValue { price: i32, world_id: i32 }`.
- Produces: `GET /api/v1/cheapest/{world}?format=columnar` → JSON `CheapestListingsColumnar`. No new route; the existing `.route("/api/v1/cheapest/{world}", get(cheapest_per_world))` in `ultros/src/web.rs:3498` is unchanged.

- [ ] **Step 1: Write the failing test**

Append to `ultros/src/web/api/cheapest_per_world.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer_service::{CheapestListingValue, ItemKey};

    #[test]
    fn columnar_from_map_emits_equal_length_columns_in_key_order() {
        let mut listings = CheapestListings::default();
        for (item_id, hq, price, world_id) in [(5, false, 7, 34), (2, true, 300, 74), (2, false, 12, 73)] {
            listings.item_map.insert(
                ItemKey { item_id, hq },
                CheapestListingValue { price, world_id },
            );
        }
        let columnar = columnar_from_map(&listings);
        assert_eq!(columnar.item_id, vec![2, 2, 5]);
        assert_eq!(columnar.hq, vec![false, true, false]);
        assert_eq!(columnar.price, vec![12, 300, 7]);
        assert_eq!(columnar.world_id, vec![73, 74, 34]);
    }

    #[test]
    fn format_query_only_matches_columnar() {
        assert!(CheapestFormat { format: Some("columnar".into()) }.is_columnar());
        assert!(!CheapestFormat { format: Some("Columnar".into()) }.is_columnar());
        assert!(!CheapestFormat { format: Some("json".into()) }.is_columnar());
        assert!(!CheapestFormat { format: None }.is_columnar());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run (Git Bash, from repo root):
```bash
export PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:$PATH" OPENSSL_RUST_USE_NASM=0 CARGO_PROFILE_DEV_DEBUG=0
cargo test -p ultros --lib cheapest_per_world
```
Expected: compile error, `columnar_from_map` / `CheapestFormat` not found.

- [ ] **Step 3: Implement the format switch**

Replace the whole file with:

```rust
use std::{sync::Arc, time::Duration};

use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use axum_extra::headers::{CacheControl, HeaderMapExt};
use serde::{Deserialize, Serialize};
use ultros_api_types::cheapest_listings::CheapestListingsColumnar;
use ultros_db::world_data::world_cache::{AnySelector, WorldCache};

use crate::{
    analyzer_service::{AnalyzerService, CheapestListings},
    web::error::WebError,
};

#[derive(Serialize, Debug)]
struct CheapestListingData {
    item_id: i32,
    hq: bool,
    cheapest_price: i32,
    world_id: i32,
}

#[derive(Debug, Serialize)]
pub(crate) struct CheapestPerWorld {
    cheapest_listings: Vec<CheapestListingData>,
}

/// `?format=columnar` selects the struct-of-arrays shape
/// ([`CheapestListingsColumnar`]) the site fetches. Anything else — including
/// no `format` at all — keeps the legacy row-of-objects shape for outside
/// consumers of the public endpoint.
#[derive(Debug, Deserialize)]
pub(crate) struct CheapestFormat {
    pub(crate) format: Option<String>,
}

impl CheapestFormat {
    fn is_columnar(&self) -> bool {
        self.format.as_deref() == Some("columnar")
    }
}

/// Four equal-length columns in `item_map` iteration order — `(item_id, hq)`
/// ascending, since `ItemKey` is the `BTreeMap` key. Sorted ids are what let
/// the columns compress ~2x better than the row shape.
fn columnar_from_map(listings: &CheapestListings) -> CheapestListingsColumnar {
    let n = listings.item_map.len();
    let mut out = CheapestListingsColumnar {
        item_id: Vec::with_capacity(n),
        hq: Vec::with_capacity(n),
        price: Vec::with_capacity(n),
        world_id: Vec::with_capacity(n),
    };
    for (key, value) in listings.item_map.iter() {
        out.item_id.push(key.item_id);
        out.hq.push(key.hq);
        out.price.push(value.price);
        out.world_id.push(value.world_id);
    }
    out
}

fn rows_from_map(listings: &CheapestListings) -> CheapestPerWorld {
    CheapestPerWorld {
        cheapest_listings: listings
            .item_map
            .iter()
            .map(|(i, v)| CheapestListingData {
                item_id: i.item_id,
                hq: i.hq,
                cheapest_price: v.price,
                world_id: v.world_id,
            })
            .collect(),
    }
}

pub(crate) async fn cheapest_per_world(
    State(analyzer): State<AnalyzerService>,
    State(world_cache): State<Arc<WorldCache>>,
    Path(world): Path<String>,
    Query(format): Query<CheapestFormat>,
) -> Result<impl IntoResponse, WebError> {
    let value = world_cache.lookup_value_by_name(&world)?;
    let selector = AnySelector::from(&value);
    let mut response: Response = if format.is_columnar() {
        let columnar = analyzer
            .read_cheapest_items(&selector, columnar_from_map)
            .await?;
        Json(columnar).into_response()
    } else {
        let rows = analyzer.read_cheapest_items(&selector, rows_from_map).await?;
        Json(rows).into_response()
    };
    response
        .headers_mut()
        .typed_insert(CacheControl::new().with_max_age(Duration::from_secs(15)));
    Ok(response)
}
```

Then append the `#[cfg(test)] mod tests` block from Step 1 at the bottom.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ultros --lib cheapest_per_world` (same env exports as Step 2)
Expected: 2 passed.

- [ ] **Step 5: Run CI check and commit**

```bash
./check_ci.sh > "$SCRATCH/ci.log" 2>&1; echo "REAL_EXIT=$?"; tail -30 "$SCRATCH/ci.log"
git add ultros/src/web/api/cheapest_per_world.rs
git commit -m "feat(api): /api/v1/cheapest/{world}?format=columnar struct-of-arrays response

Legacy row shape stays the default for outside consumers.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 3: Frontend fetches the columnar shape

**Files:**
- Modify: `ultros-frontend/ultros-frontend-core/src/api.rs:16` (import) and `:176-191` (two fetchers)

**Interfaces:**
- Consumes: `CheapestListingsColumnar` from Task 1; the `?format=columnar` endpoint from Task 2.
- Produces: unchanged signatures `get_cheapest_listings(world_name: &str) -> AppResult<CheapestListings>` and `get_cheapest_listings_live(world_name: &str, refresh_version: u64) -> AppResult<CheapestListings>`.

- [ ] **Step 1: Update the import**

Change line 16 from
```rust
    cheapest_listings::{CheapestListings, CheapestListingsMap},
```
to
```rust
    cheapest_listings::{CheapestListings, CheapestListingsColumnar, CheapestListingsMap},
```

- [ ] **Step 2: Rewrite the two fetchers**

Replace lines 174-191 (`/// Get analyzer data` through the end of `get_cheapest_listings_live`) with:

```rust
/// Cheapest listing per `(item, hq)` for a world/DC/region. Fetches the
/// columnar wire shape (roughly half the bytes of the row shape after
/// compression) and converts at the boundary so callers keep the row type.
pub async fn get_cheapest_listings(world_name: &str) -> AppResult<CheapestListings> {
    fetch_api::<CheapestListingsColumnar>(&format!(
        "/api/v1/cheapest/{world_name}?format=columnar"
    ))
    .await
    .map(CheapestListings::from)
}

pub async fn get_cheapest_listings_live(
    world_name: &str,
    refresh_version: u64,
) -> AppResult<CheapestListings> {
    if refresh_version == 0 {
        get_cheapest_listings(world_name).await
    } else {
        fetch_api::<CheapestListingsColumnar>(&format!(
            "/api/v1/cheapest/{world_name}?format=columnar&rt={refresh_version}"
        ))
        .await
        .map(CheapestListings::from)
    }
}
```

- [ ] **Step 3: Compile-check the frontend crates**

Run: `cargo check -p ultros-frontend-core --features ssr && cargo check -p ultros-frontend-core --no-default-features --features hydrate`
(If the crate's feature names differ, use `grep -n '^\[features\]' -A10 ultros-frontend/ultros-frontend-core/Cargo.toml` to find them.)
Expected: both succeed.

- [ ] **Step 4: Run CI check and commit**

```bash
./check_ci.sh > "$SCRATCH/ci.log" 2>&1; echo "REAL_EXIT=$?"; tail -30 "$SCRATCH/ci.log"
git add ultros-frontend/ultros-frontend-core/src/api.rs
git commit -m "perf(frontend): fetch cheapest listings in columnar form

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 4: Lazy client-only `CheapestPrices` and its consumers

This task must land as one commit: changing the struct breaks every consumer until they are updated.

**Files:**
- Modify: `ultros-frontend/ultros-frontend-core/src/global_state/cheapest_prices.rs` (whole file)
- Modify: `ultros-frontend/ultros-ui-game/src/components/cheapest_price.rs:57`
- Modify: `ultros-frontend/ultros-ui-game/src/components/job_set_card.rs:241`
- Modify: `ultros-frontend/ultros-app/src/routes/job_set_detail.rs:437`
- Modify: `ultros-frontend/ultros-app/src/routes/item_view.rs:476,518-521`
- Modify: `ultros-frontend/ultros-ui-crafting/src/components/related_items.rs:160,187,461`
- Modify: `ultros-frontend/ultros-app/src/routes/item_explorer.rs:800,1403-1429`

**Interfaces:**
- Consumes: `get_cheapest_listings` from Task 3, `CheapestListingsMap`, `get_price_zone()` from `super::home_world`, `crate::error::AppError` (re-exported in `ultros-frontend-core`).
- Produces:
  - `pub struct CheapestPrices` — `Clone + Copy`, fields private.
  - `CheapestPrices::new() -> Self` (root, lazy).
  - `CheapestPrices::already_demanded(listings: LocalResource<Result<CheapestListingsMap, AppError>>) -> Self` (for scoped shadowing).
  - `CheapestPrices::demand(&self) -> LocalResource<Result<CheapestListingsMap, AppError>>`.
  - `LocalResource<T>` is `Copy`; `.with(|data: &Option<T>| …)` and `.get() -> Option<T>` read exactly like the old `Resource`. `.await` yields `T` (client only).

- [ ] **Step 1: Rewrite `cheapest_prices.rs`**

Replace the whole file with:

```rust
use leptos::prelude::*;
use ultros_api_types::cheapest_listings::CheapestListingsMap;

use crate::{api::get_cheapest_listings, error::AppError};

use super::home_world::get_price_zone;

pub type CheapestListingsResource = LocalResource<Result<CheapestListingsMap, AppError>>;

/// The cheapest listing per `(item, hq)` for the visitor's price zone,
/// shared by every price cell on the site.
///
/// Two deliberate properties:
///
/// * **Client-only.** This is a [`LocalResource`], so the server never fetches
///   or serializes it. The row-of-objects map is ~1.2 MB of JSON; as a plain
///   `Resource` it was inlined into every SSR page even though every consumer
///   already gates its read behind a post-hydration signal (the `hydrated`
///   idiom from #740) and so never used it on the server.
/// * **Lazy.** Nothing is fetched until a consumer calls [`demand`]. Pages
///   without a price cell (lists, alerts, settings, …) never pay for it; pages
///   with one fetch it once per zone for the SPA session.
///
/// [`demand`]: CheapestPrices::demand
#[derive(Clone, Copy)]
pub struct CheapestPrices {
    listings: CheapestListingsResource,
    wanted: RwSignal<bool>,
}

impl Default for CheapestPrices {
    fn default() -> Self {
        Self::new()
    }
}

impl CheapestPrices {
    pub fn new() -> Self {
        let (zone, _) = get_price_zone();
        let wanted = RwSignal::new(false);
        let listings = LocalResource::new(move || {
            // Both reads are tracked: flipping `wanted` re-runs the fetcher,
            // which drops the never-resolving future below and issues the
            // real request; a zone change refetches as before.
            let wanted = wanted.get();
            let zone = zone.get();
            async move {
                if !wanted {
                    // Nothing on this page has asked for prices: stay pending
                    // (consumers read `None` and show their skeleton) and
                    // never hit the network.
                    std::future::pending::<()>().await;
                }
                let zone_name = zone.as_ref().map(|w| w.get_name()).unwrap_or("North-America");
                get_cheapest_listings(zone_name)
                    .await
                    .map(CheapestListingsMap::from)
            }
        });
        Self { listings, wanted }
    }

    /// Wraps a resource some scope built itself (e.g. the item explorer's
    /// `?world=` override) so descendants' [`demand`] calls are no-ops.
    ///
    /// [`demand`]: CheapestPrices::demand
    pub fn already_demanded(listings: CheapestListingsResource) -> Self {
        Self {
            listings,
            wanted: RwSignal::new(true),
        }
    }

    /// Marks the map as needed on this page and returns the resource. Call
    /// once at component setup (not inside a render closure) and read the
    /// returned handle from there; it is `Copy`.
    pub fn demand(&self) -> CheapestListingsResource {
        if !self.wanted.get_untracked() {
            self.wanted.set(true);
        }
        self.listings
    }
}
```

- [ ] **Step 2: `cheapest_price.rs`**

Line 57, change
```rust
    let Some(cheapest) = use_context::<CheapestPrices>().map(|prices| prices.read_listings) else {
```
to
```rust
    let Some(cheapest) = use_context::<CheapestPrices>().map(|prices| prices.demand()) else {
```

- [ ] **Step 3: `job_set_card.rs`**

Line 241, change
```rust
    let read_listings = cheapest_prices.map(|c| c.read_listings);
```
to
```rust
    let read_listings = cheapest_prices.map(|c| c.demand());
```

- [ ] **Step 4: `job_set_detail.rs`**

Line 437, change
```rust
    let default_zone_listings = cheapest_prices.map(|p| p.read_listings);
```
to
```rust
    let default_zone_listings = cheapest_prices.map(|p| p.demand());
```
Also update the comment at lines 480-482 that says "reads the shared `CheapestPrices` `read_listings` resource" to say "reads the shared `CheapestPrices` resource (via `demand()`)".

- [ ] **Step 5: `item_view.rs`**

Line 476, change
```rust
    let cheapest_prices = use_context::<CheapestPrices>();
```
to
```rust
    let cheapest_listings = use_context::<CheapestPrices>().map(|prices| prices.demand());
```
Lines 518-523, change
```rust
                                let summary = if hydrated.get() {
                                    cheapest_prices.as_ref().and_then(|prices| {
                                        prices.read_listings.with(|r| {
                                            let map = r.as_ref().and_then(|r| r.as_ref().ok());
                                            map.map(|map| map.find_matching_listings(item_id()))
                                        })
                                    })
```
to
```rust
                                let summary = if hydrated.get() {
                                    cheapest_listings.and_then(|listings| {
                                        listings.with(|r| {
                                            let map = r.as_ref().and_then(|r| r.as_ref().ok());
                                            map.map(|map| map.find_matching_listings(item_id()))
                                        })
                                    })
```

- [ ] **Step 6: `related_items.rs`**

Line 160 (in `RecipePriceEstimate`), change
```rust
    let cheapest_prices = use_context::<CheapestPrices>().unwrap();
```
to
```rust
    let cheapest_listings = use_context::<CheapestPrices>().unwrap().demand();
```
Line 187, change
```rust
                cheapest_prices.read_listings.with(|prices| {
```
to
```rust
                cheapest_listings.with(|prices| {
```

For the second site (the profitability `<Suspense>` around line 450-461), the resource is currently obtained *inside* the render closure. Hoist it: directly after line 397 (`let profit_hydrated = RwSignal::new(false);`) and its `Effect::new` block (ends ~line 400) add

```rust
    let profit_listings = use_context::<CheapestPrices>().unwrap().demand();
```

then change line 461 from
```rust
                        use_context::<CheapestPrices>().unwrap().read_listings.with(|data| {
```
to
```rust
                        profit_listings.with(|data| {
```

- [ ] **Step 7: `item_explorer.rs` — `ItemList`**

Line 800, change
```rust
    let listings_resource = cheapest_prices.read_listings;
```
to
```rust
    let listings_resource = cheapest_prices.demand();
```
Update the comment block at lines 826-828 ("On the client, Leptos serialises the resolved resource into the payload so `listings_resource.get()` returns `Some(map)` immediately during hydration") to: "On the client the resource is a `LocalResource` that resolves after hydration, but a cached SPA navigation can already have it loaded, so `listings_resource.get()` may return `Some(map)` during the first CSR render". The gate logic that follows is unchanged.

- [ ] **Step 8: `item_explorer.rs` — `ItemExplorer` scoped resource**

Replace lines 1403-1429 (from `let global_prices = use_context::<CheapestPrices>();` through `provide_context(CheapestPrices { read_listings });`) with:

```rust
    let global_prices = use_context::<CheapestPrices>();
    let (cookie_zone, _) = crate::global_state::home_world::get_price_zone();
    let cookie_zone_name = Signal::derive(move || {
        cookie_zone
            .get()
            .map(|z| z.get_name().to_string())
            .unwrap_or_else(|| "North-America".to_string())
    });
    // Client-only like the root resource: the explorer gates every price read
    // behind hydration, so the server has nothing to fetch or serialize.
    let read_listings = LocalResource::new(move || {
        let world = scope_name.get();
        let cookie_world = cookie_zone_name.get();
        async move {
            if let Some(global) = global_prices.filter(|_| world == cookie_world) {
                return global.demand().await;
            }
            crate::api::get_cheapest_listings(&world)
                .await
                .map(ultros_api_types::cheapest_listings::CheapestListingsMap::from)
        }
    });
    provide_context(CheapestPrices::already_demanded(read_listings));
```

- [ ] **Step 9: Build both targets and run frontend tests**

Run:
```bash
cargo check -p ultros-app --features ssr
cargo check -p ultros-app --no-default-features --features hydrate --target wasm32-unknown-unknown
cargo test -p ultros-frontend-core -p ultros-ui-game -p ultros-ui-crafting
```
(Feature names: confirm with `grep -n '^\[features\]' -A12 ultros-frontend/ultros-app/Cargo.toml`.)
Expected: all succeed. If `demand()` is flagged by clippy for `&self` on a `Copy` type (`trivially_copy_pass_by_ref`), that lint is allow-by-default; do not change the signature.

- [ ] **Step 10: Run CI check and commit**

```bash
./check_ci.sh > "$SCRATCH/ci.log" 2>&1; echo "REAL_EXIT=$?"; tail -30 "$SCRATCH/ci.log"
git add ultros-frontend
git commit -m "perf(frontend): cheapest listings become a lazy client-only LocalResource

The root CheapestPrices resource was a plain Resource, so Leptos inlined the
~1.2 MB cheapest map into every SSR page (home, lists, analyzers...) even
though every consumer already defers its read past hydration. LocalResource
is never serialized and always pending on the server; a wanted signal makes
it lazy so only pages with a price cell fetch it.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 5: End-to-end verification against a local serve

**Files:** none modified. Scratch output goes in `$SCRATCH`.

- [ ] **Step 1: Build and start the app**

Use the `run` skill (or the project's existing `.claude/launch.json` entry) to build with `cargo leptos` and serve on port 8080. Kill anything already on 8080 first (`netstat -ano | grep :8080`). Wait for the analyzer warm-up log line before testing (`/api/v1/cheapest/...` returns 503 "Still warming up" until then).

- [ ] **Step 2: Verify the HTML no longer carries the map**

```bash
for p in / /list /item/North-America/4 /items/jobset/WAR; do
  curl -s -H 'Accept-Encoding: identity' "http://localhost:8080$p" -o "$SCRATCH/page.html" -w "$p %{size_download} bytes\n"
  echo "  id_hq keys: $(grep -o '_true\|_false' "$SCRATCH/page.html" | wc -l)   RESOLVED_RESOURCES refs: $(grep -o '__RESOLVED_RESOURCES\[[0-9]*\] = "{\\"Ok\\":{\\"map\\"' "$SCRATCH/page.html" | wc -l)"
done
```
Expected: `id_hq keys: 0` for every page (prod today is 23942). Home page well under 300 KB raw.

- [ ] **Step 3: Verify both API shapes**

```bash
curl -s 'http://localhost:8080/api/v1/cheapest/North-America' | head -c 120; echo
curl -s 'http://localhost:8080/api/v1/cheapest/North-America?format=columnar' -o "$SCRATCH/col.json" -D - | grep -i cache-control
python -c "import json;d=json.load(open('$SCRATCH/col.json'));print({k:len(v) for k,v in d.items()}); print(sorted(zip(d['item_id'],d['hq']))==list(zip(d['item_id'],d['hq'])))"
```
Expected: first line starts with `{"cheapest_listings":[{"item_id":`; `cache-control: max-age=15`; four equal lengths; `True` (sorted).

- [ ] **Step 4: Browser checks**

Open in the built-in browser and use `read_network_requests` with `urlPattern: "cheapest"` after each load:
1. `/` — prices appear in Recently Viewed after hydration (visit an item page first so there is something recent); exactly one request to `/api/v1/cheapest/North-America?format=columnar`.
2. `/list`, `/alerts`, `/settings` — zero `cheapest` requests.
3. `/item/North-America/4` — zone-savings pill renders; one `cheapest` request.
4. `/items?world=Aether` (explorer with a non-cookie scope) — one request, for `Aether`, none for `North-America`.
5. `read_console_messages` with `onlyErrors: true` on each of the above: no hydration panics or `RefCell already borrowed`.

- [ ] **Step 5: Record results**

Note the before/after page sizes and request counts in the PR description (Task 6).

---

### Task 6: PR

- [ ] **Step 1: Push and open the PR**

```bash
git push -u origin claude/global-listings-optimization-2a5097
gh pr create --title "perf: lazy client-only cheapest listings + columnar API payload" --body-file "$SCRATCH/pr.md"
```

`$SCRATCH/pr.md` contents:

```markdown
## Summary
- `/api/v1/cheapest/{world}?format=columnar` — struct-of-arrays JSON, ~83 KB br vs ~150 KB for the row shape on North-America. Legacy shape unchanged and still the default.
- The root `CheapestPrices` resource is now a lazy `LocalResource`. It was a plain `Resource`, so Leptos inlined the full 1.16 MB map into **every** SSR page (`/` was 1.28 MB raw / 202 KB gzip) even though every consumer already defers reading it until after hydration. Pages without a price cell no longer fetch it at all.
- Consumers obtain the handle via `CheapestPrices::demand()`; in-memory types are unchanged.

## Measurements
<before/after table from Task 5>

## Test plan
- [x] `cargo test -p ultros-api-types`, `cargo test -p ultros --lib cheapest_per_world`
- [x] `./check_ci.sh`
- [x] Local serve: 0 `id_hq` keys in `/`, `/list`, `/item/...`, `/items/jobset/...` HTML
- [x] Browser: prices render on `/`, item view, explorer; no `cheapest` fetch on `/list`, `/alerts`, `/settings`; no console errors

Spec: `docs/superpowers/specs/2026-09-21-cheapest-listings-payload-design.md`

🤖 Generated with [Claude Code](https://claude.com/claude-code)
```
