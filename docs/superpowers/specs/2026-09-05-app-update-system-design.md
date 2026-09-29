# App update detection

**Date:** 2026-09-05
**Status:** Approved, ready for implementation plan

## Problem

Ultros is a Leptos app with client-side routing. Once a tab has loaded the
wasm bundle it can live for days without a full page load, while the server
behind it is redeployed. A stale tab then talks to a newer server: API
payloads may have gained or lost fields, endpoints may have moved, and the
user sees deserialize errors, blank panels, or silently wrong data. A manual
refresh fixes it, but nothing tells the user to refresh.

The goal is for the client to notice it is behind the server and get itself
back onto the current build with as little disruption as possible.

## Current behaviour

- `ultros/build.rs` and `ultros-frontend/ultros-app/build.rs` each run
  `git rev-parse --short HEAD` and emit `GIT_HASH`, falling back to `dirty`.
  Both build scripts only declare `rerun-if-changed` on themselves (and
  `Cargo.toml` for the app), so the embedded hash goes stale after a commit
  until something else forces the build script to rerun. In a long dev session
  the two crates can even disagree.
- The JS/wasm/CSS bundle is served from `/pkg/<GIT_HASH>/` with
  `Cache-Control: public, max-age=86400, immutable` (release builds only), so
  a new deploy never collides with a cached old bundle.
- SSR HTML is `Cache-Control: private, no-store`, so browsers and Cloudflare
  never hold an HTML page that points at a retired `/pkg/<hash>/`.
- Every client request goes through `fetch_api`, `post_api`, or `delete_api`
  in `ultros-frontend/ultros-app/src/api.rs`, built on `gloo_net`. Nothing
  inspects response headers today.
- No version information crosses the wire in either direction.
- Cloudflare proxies `ultros.app` with default caching: only static file
  types (`.js`, `.css`, `.wasm`, images, fonts) are cached. HTML and `/api`
  are not. There is no Cloudflare API integration in the codebase.
- `ultros/static/search.js` and `ultros/static/retainer.js` are served under
  `/static/` with a 24h cache and an unversioned URL, but nothing in the
  codebase references them.
- Production is a single machine. `fly.toml` is a leftover from an earlier
  hosting setup and is no longer used.

## Design

### Header

The server stamps every response with the commit it was built from:

```
x-ultros-commit: 283f84e5
```

The name is lowercase and hyphenated rather than `ULTROS_COMMIT`. Underscored
header names are legal but nginx-family proxies drop them by default, and the
`http` crate's `HeaderName::from_static` requires lowercase.

The header name lives once, as a constant in `ultros-api-types`
(`ultros_api_types::app_version::APP_COMMIT_HEADER`), so the server and the
client cannot drift.

**Server.** One `SetResponseHeaderLayer::overriding` in the global layer stack
of `start_web` in `ultros/src/web.rs`, beside the existing
`X-Frame-Options` / `X-Content-Type-Options` / `Strict-Transport-Security`
layers, with the value `env!("GIT_HASH")`. Because it sits at the outermost
layer it covers SSR HTML, the JSON API, static files, the `/pkg/` bundle, and
error responses. The layer is built by a small named function so a unit test
can mount it on a throwaway router.

### Client detection

New module `ultros-frontend/ultros-app/src/global_state/app_update.rs`.

- `pub const CLIENT_COMMIT: &str = env!("GIT_HASH");`
- `pub fn update_pending(client: &str, server: Option<&str>) -> bool` is the
  whole decision, pure and unit-tested:
  - `server` is `None` or empty → `false` (older server, proxy error page,
    connection dropped mid-deploy).
  - either side is `dirty` → `false` (local builds without git; never nag).
  - otherwise `client != server`.
- `AppUpdate` context: `pending: RwSignal<Option<String>>` holding the first
  differing server commit seen, and `dismissed: RwSignal<bool>`.
  `provide_app_update_context()` is called from `AppInner` alongside the other
  `provide_*` calls.
- Because the fetch helpers run inside `spawn_local` and are not guaranteed a
  reactive owner, the client build also mirrors `pending` into a
  `thread_local!` slot, populated by `provide_app_update_context()`. This
  mirror is compiled only for the hydrate build. The SSR build must not keep
  per-request state in a thread-local (see the `StoredValue::new_local`
  incident in memory), so on the server `observe_server_commit` is a no-op.
- `pub fn observe_server_commit(header: Option<&str>)`: if
  `update_pending(CLIENT_COMMIT, header)` and `pending` is still `None`, set
  it. Once set it is never cleared or replaced until the page reloads. That
  makes the state monotonic: a proxy hiccup or a transient error page cannot
  flip it back, and nothing can cause repeated banners.

**Fetch helpers.** The client-side (`not(feature = "ssr")`) `fetch_api`,
`post_api`, and `delete_api` read
`response.headers().get(APP_COMMIT_HEADER)` right after `send()` resolves and
pass it to `observe_server_commit` before reading the body. This runs on error
statuses too; a 500 from the new server still carries the header. The SSR
variants are untouched: the server calling itself is always the same build.

### Banner

New component `ultros-frontend/ultros-app/src/components/update_banner.rs`,
rendered in `AppInner` next to `<ToastContainer />`. It is a dedicated
element rather than a toast because toasts have no action buttons and
auto-dismiss.

- Visible when `pending.get().is_some() && !dismissed.get()`.
- Fixed at the bottom-left on desktop, full width at the bottom on small
  screens, above page content, below the toast stack's z-index so a toast can
  still appear over it. Uses the existing surface tokens
  (`--color-background-elevated`, `--color-outline`) and the existing button
  classes.
- Content: a refresh icon, the message, a primary **Reload** button that
  calls `window.location.reload()`, and a close button that sets
  `dismissed`. `role="status"` with `aria-live="polite"`.
- Strings, added to all seven locale files with real translations:
  - `update_banner_message`: "Ultros has been updated."
  - `update_banner_reload`: "Reload"
  - `update_banner_dismiss` (aria-label): "Dismiss"
- Dismissing hides the banner only. The pending state stays, so the
  reload-on-navigation behaviour below still fires.

### Reload on next navigation

A small hydrate-only component next to `SentryRouteTag` inside `<Router>`,
`ReloadWhenStale`. It runs an `Effect` on `use_location().pathname` and keeps
the previous value. When the pathname changes and `pending` is `Some`, it
calls `window.location.reload()`. The router has already pushed the new URL,
so the reload lands on the page the user asked for, served with fresh HTML and
the current `/pkg/<hash>/` bundle.

Only `pathname` is watched. Query-string changes (filters, sort, world
pickers) keep the user on the same page and do not trigger a reload.

The effect skips its first run so a page that loads already-stale (not
possible in practice, but cheap to guard) does not reload in a loop.

### Build script freshness

Both `ultros/build.rs` and `ultros-frontend/ultros-app/build.rs` add:

```
cargo:rerun-if-changed=<git rev-parse --git-path HEAD>
cargo:rerun-if-changed=<git rev-parse --git-path logs/HEAD>
```

`--git-path` resolves correctly inside worktrees (where `.git` is a file) and
the reflog at `logs/HEAD` is appended on every commit and checkout, so the
hash tracks the tree without parsing symbolic refs. When git is unavailable the
lines are simply not emitted and the `dirty` fallback behaves as today.

`xiv-gen/build.rs` keeps its current behaviour on purpose: rerunning it
re-parses all game data CSVs, which is far too expensive to trigger per commit.

### Cloudflare

No purge step. With default caching only static file types are cached, and
everything that changes per deploy is already addressed by a hashed path
(`/pkg/<hash>/`, `/static/data/<version>/`). HTML and `/api` bypass the cache.
The header this design adds is on uncached responses, so Cloudflare never
serves a stale value of it for HTML or API calls; a cached `.wasm` may carry an
old header value but those responses are never read by the fetch helpers.

The two unversioned scripts that a purge would have existed for,
`ultros/static/search.js` and `ultros/static/retainer.js`, are unreferenced
and are deleted instead.

If a Cache Everything rule is ever added, the right follow-up is a startup
hook gated on `CLOUDFLARE_ZONE_ID` and `CLOUDFLARE_API_TOKEN` that purges by
URL. It is out of scope here.

### Cleanup

- Delete `fly.toml`.
- Delete `ultros/static/search.js` and `ultros/static/retainer.js`.

## Error handling

- Missing header, empty header, or a dropped connection: `update_pending`
  returns `false`; nothing happens.
- Local dev with `dirty` on either side: never flags.
- Server restarts mid-deploy: requests fail with network errors, which already
  surface as `AppError`; no header means no version decision.
- `location.reload()` failing is not a case the app can recover from; the
  banner remains and the user can reload manually.

## Testing

- **Unit, `update_pending`**: equal hashes → false; different → true; `None`
  and `""` → false; `dirty` on either side → false.
- **Unit, header layer**: mount the layer function on a one-route `axum`
  router, send a request with `tower::ServiceExt::oneshot`, assert the
  header equals `env!("GIT_HASH")` on a 200 and on a 404.
- **Unit, banner strings**: the i18n build already fails if a key is missing
  from any locale.
- **Manual**: with the app running, `curl -sI http://localhost:8080/` and
  `curl -sI http://localhost:8080/api/v1/world_data` both show
  `x-ultros-commit`.
- **Manual stale-tab check**: open the app, make an empty commit
  (`git commit --allow-empty`), rebuild and restart with `cargo leptos serve`.
  In the old tab, trigger any fetch: the banner appears. Navigate to another
  page: the tab does a full reload and the banner is gone. This also proves
  the `rerun-if-changed` change works, since without it the server would keep
  the old hash.
- **E2E**: `./scripts/run_e2e.sh` stays green (banner hidden by default, no
  console errors).

## Out of scope

- Polling a version endpoint and websocket-pushed version announcements.
- Cloudflare purge integration.
- Version checks on the SSR-side fetch helpers.
- Multi-machine rolling-deploy tolerance beyond the monotonic first-wins
  state, which already prevents flapping.
