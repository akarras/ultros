# Lazy routes + wasm-split pilot — design

Date: 2026-09-21. Branch: `claude/lazyroutes-wasm-shrink-6ce732`.

## Goal

Shrink the wasm the browser must download before the app hydrates, by moving
the analyzer family and the Lists cluster into lazily-loaded wasm chunks.
Pilot scope: prove the mechanism end to end (build, serve, hydrate, navigate,
preload) and report measured sizes, on two clusters only.

## Why these two clusters

From the 2026-09-20 attribution of the production `ultros.wasm`: framework
code (~25%) must stay in the main module; Lists (`list_view_sync`,
`guest_lists`, `list_view`, `lists`, loro ≈ 0.8 MB) ≈ 17%; analyzer family
≈ 14%. Every other route is 1–3.5%. Two clusters is enough to measure a real
win and small enough to bisect a hydration regression.

## Toolchain (already pinned, no bumps)

- `leptos_router 0.8.15`: `#[lazy_route] impl LazyRoute for T`,
  `Lazy::<T>::new()` as a `<Route view=…>`, `LazyRoute::preload()`.
- `leptos 0.8.20`: `#[lazy]` → `wasm_split_helpers::wasm_split`;
  `HydrationScripts` reads `<site_pkg_dir>/__wasm_split_manifest.json`;
  `leptos_integration_utils` injects `<link rel=preload>` for the chunks the
  rendered route touched.
- `cargo-leptos 0.3.1` (local + Dockerfile): `--split` → `wasm_split_cli_support 0.2.0`.

## Components

### 1. Route wrappers — `ultros-frontend/ultros-app/src/routes/lazy.rs` (new)

One unit struct per lazy route with a `#[lazy_route] impl LazyRoute`:

```rust
pub struct AnalyzerRoute;
#[lazy_route]
impl LazyRoute for AnalyzerRoute {
    fn data() -> Self { Self }
    fn view(_: Self) -> AnyView { view! { <Analyzer /> }.into_any() }
}
```

Routes converted (view fn → wrapper):

| path | component | wrapper |
|---|---|---|
| `flip-finder` | `Analyzer` | `AnalyzerRoute` |
| `flip-finder/:world` | `AnalyzerWorldView` | `AnalyzerWorldRoute` |
| `vendor-resale` | `VendorResale` | `VendorResaleRoute` |
| `vendor-resale/:world` | `VendorWorldView` | `VendorWorldRoute` |
| `vendor-sell/:world?` | `VendorSell` | `VendorSellRoute` |
| `recipe-analyzer/:world?` | `RecipeAnalyzer` | `RecipeAnalyzerRoute` |
| `fc-crafting-analyzer[/ :world]` | `FCCraftingAnalyzer` | `FcCraftingRoute` |
| `leve-analyzer/:world?` | `LeveAnalyzer` | `LeveRoute` |
| `scrip-sources/:world?` | `ScripSources` | `ScripRoute` |
| `venture-analyzer/:world?` | `VentureAnalyzer` | `VentureRoute` |
| `list` (parent) | `Lists` | `ListsRoute` |
| `list/:id` | `ListRoute` | `ListViewRoute` |
| `list/` (index) | `EditLists` | `EditListsRoute` |
| `list/device/:device_id` | `GuestListRoute` | `GuestListLazyRoute` |
| `list/invite/:invite_id` | `ListInviteAccept` | `ListInviteRoute` |

`data()` is empty: the components already own their resources. The two
`/analyzer*` redirect closures stay eager. `ParentRoute` accepts the same
`ChooseView`, so the Lists parent is lazy too; `<Outlet/>` inside it is
unaffected.

### 1b. Client entry point — `ultros-client/src/lib.rs`

`hydrate_body` is synchronous and panics on a direct load of a lazy route
(`lazy routes not supported with hydrate_body(); use hydrate_lazy() instead`,
`nested_router.rs:494`): the router must fetch that route's chunk before it
can walk the SSR DOM. The client already hydrates inside a `spawn_local`
task, so it awaits `leptos::mount::hydrate_from_async(body, app)` there
instead — unlike `hydrate_lazy`, which spawns and returns, this keeps the
`ultros:hydrated` boot event firing only after hydration finished. The
offline-guest path (`mount_to_body`, CSR) already spawns lazy loaders
asynchronously and is unchanged.

Async hydration has a hazard of its own: while the router awaits the chunk,
the executor runs effects queued by the already-hydrated shell (`Effect::new`
is not gated on hydration), and any of them can move state the not-yet-
hydrated route body reads — the URL normalisation / restored-view logic did
exactly that in the analyzer E2E, producing an intermittent
`tachys hydration.rs:163` panic. Every `hydrate_async` in the chain only
forwards to inner futures, so the loader is the sole suspension point. The
client therefore awaits `lazy::preload_for_path(location.pathname)` *before*
hydrating; with the chunk resident the router's await resolves without
yielding and the walk is as atomic as `hydrate_body` was. The path table is
hand-maintained (unit test pins its shapes); an unmatched path just falls
back to the router's own fetch.

### 2. Nav hover preload — `components/side_nav.rs`

`SideNavItem` gains `#[prop(optional)] preload: Option<fn()>`. On
`mouseenter` and `focus` (hydrate builds only, `#[cfg(feature = "hydrate")]`)
it calls the fn, which does `spawn_local(<XRoute as LazyRoute>::preload())`.
Wired for the analyzer entries (each to its own world route wrapper) and
`/list` (to `ListsRoute` + `EditListsRoute`). `Lazy` memoises successful
loads so repeated hovers are free; failed loads retry.

### 3. Build & serve

- `Dockerfile` frontend step: add `--split`.
- New `scripts/post_split.sh` (run in the Dockerfile after the frontend
  build, and locally after `cargo leptos build --release --split`):
  1. In every `target/site/pkg/__wasm_split*.js`, rewrite the hard-coded
     `from "/pkg/ultros.js"` to `from "./ultros.js"` — the generated loader
     assumes an unhashed `/pkg/` root, we serve `/pkg/<git hash>/`, and a
     second module instance would try to init a second wasm.
  2. Move `target/site/pkg` → `target/site/pkg/<GIT_HASH>` so the server's
     existing `site_pkg_dir = "pkg/<hash>"` override matches disk. That makes
     `HydrationScripts` find `__wasm_split_manifest.json` and lets the SSR
     response carry `<link rel=preload>` for the current route's chunks.
- `ultros/src/leptos.rs`: compute `bundle_filepath` from the overridden
  `site_pkg_dir` (`./{site_root}/pkg/{git_hash}`) instead of the raw config
  value. Everything else (`pkg_service`, `precompressed_br`, immutable
  cache-control) already applies to the chunk files because `--precompress`
  runs after split.
- Local dev (`cargo leptos watch`, no `--split`): `Lazy<T>` routes still
  work — `view()` is just an async fn that resolves immediately. The move
  step is what makes local layout match prod; `post_split.sh` is optional
  locally and the server falls back gracefully (no preload links) if the
  manifest is absent.

### 4. Verification

- Sizes: `ultros.wasm` raw + brotli before/after on the same commit, plus the
  chunk list. Recorded in the PR body and memory.
- Browser (local `cargo leptos build --release --split` + server):
  - `/` hydrates with no console errors.
  - navigate to `/flip-finder/<world>` and `/list`: chunk `.wasm` fetched
    once, grid/list renders.
  - hard-reload on `/list` and `/leve-analyzer`: SSR HTML present, page
    hydrates, `<link rel=preload … .wasm>` for the route's chunk is in
    `<head>`.
  - hover a nav entry, then click: no new chunk fetch at click time.
- `./check_ci.sh`, existing `ultros/src/leptos.rs` pkg tests.
- `scripts/run_e2e.sh` as the regression net for touched routes.

### 5. Failure modes / rollback

- Chunk load failure: `wasm_split_helpers` logs to console, the route
  renders nothing, next navigation retries. Accepted for the pilot.
- Rollback: remove `--split` (and the post-split script) from the
  Dockerfile. The `Lazy<T>` routes remain valid unsplit.

## Measured (2026-09-21, same source, only `--split` differs)

| | raw | brotli |
|---|---|---|
| unsplit `ultros.wasm` | 16,687,640 | 3,457,098 |
| split main `ultros.wasm` | 12,418,209 (−25.6%) | 2,945,413 (−14.8%) |

Per-route loads (files / brotli bytes, from the manifest): analyzer world
37 / 167 KB, recipe analyzer 45 / 209 KB, list view 30 / 616 KB, edit lists
11 / 164 KB, leve 39 / 108 KB. The splitter emits ~100 shared `chunk_N.wasm`
files, most under 2 KB, so a first visit to an analyzer costs 30–45 small
requests. They are fetched in parallel (and preloaded from `<head>` on a
direct load, or on nav hover), which is fine over HTTP/2 through Cloudflare,
but a chunk-merging threshold upstream would be the next win.

## Out of scope

Moving resources into `data()`; preload on non-nav links; making every route
lazy; a router-level hover prefetch.
