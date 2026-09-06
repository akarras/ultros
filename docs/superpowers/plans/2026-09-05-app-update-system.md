# App Update Detection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a long-lived browser tab notice that the server has been redeployed and get itself back onto the current build with a banner and a full reload on the next navigation.

**Architecture:** The server stamps every response with `x-ultros-commit: <GIT_HASH>` via one global `tower-http` layer. The wasm client compares that header with its own baked-in `GIT_HASH` inside the four `gloo_net` fetch helpers, records the first mismatch in a monotonic signal, shows a persistent banner with a Reload button, and hard-reloads on the next pathname change. Both `build.rs` scripts learn to rerun when git HEAD moves so the embedded hash tracks commits.

**Tech Stack:** Rust, axum 0.8 + tower-http 0.6 (server), Leptos 0.8 + leptos_router + gloo_net + web-sys (client), leptos-i18n locale JSON files, `cargo leptos` for the wasm build.

**Spec:** `docs/superpowers/specs/2026-09-05-app-update-system-design.md`

**Repo rules that apply to every task:**
- Run `./check_ci.sh` before every commit (`cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`). Read the exit code directly: `./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log`.
- Clippy only checks the `ssr` feature. Client-only code under `#[cfg(not(feature = "ssr"))]` is compiled by `cargo leptos build`, so run that too in tasks that touch it.
- Every user-facing string goes through `leptos-i18n` and must be added to all seven locale files (`en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`).
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

---

## File structure

| Path | Responsibility |
| --- | --- |
| `ultros-api-types/src/app_version.rs` (new) | The single definition of the header name, shared by server and client. |
| `ultros-api-types/src/lib.rs` | Registers the new module. |
| `ultros/Cargo.toml` | Enables `tower`'s `util` feature so the test can use `ServiceExt::oneshot`. |
| `ultros/src/web.rs` | `app_commit_header_layer()` plus its use in the global layer stack, and its unit test. |
| `ultros/build.rs`, `ultros-frontend/ultros-app/build.rs` | Rerun the git-hash step when HEAD moves. |
| `ultros-frontend/ultros-app/src/global_state/app_update.rs` (new) | Pure mismatch decision, the `AppUpdate` context, the client-only thread-local mirror, and `observe_server_commit`. |
| `ultros-frontend/ultros-app/src/global_state/mod.rs` | Registers the new module. |
| `ultros-frontend/ultros-app/src/api.rs` | The four client fetch helpers report the header. |
| `ultros-frontend/ultros-app/src/components/update_banner.rs` (new) | `UpdateBanner` component and the `reload_page()` helper. |
| `ultros-frontend/ultros-app/src/components/mod.rs` | Registers the new component module. |
| `ultros-frontend/ultros-app/src/lib.rs` | Provides the context, mounts the banner, adds `ReloadWhenStale` inside `<Router>`. |
| `ultros-frontend/ultros-app/locales/*.json` | Three new keys per locale. |
| `fly.toml`, `ultros/static/search.js`, `ultros/static/retainer.js` | Deleted. |

---

### Task 1: Shared header-name constant

**Files:**
- Create: `ultros-api-types/src/app_version.rs`
- Modify: `ultros-api-types/src/lib.rs:1-2`

- [ ] **Step 1: Create the module**

```rust
//! Version handshake between the server and the wasm client.
//!
//! The server stamps every response with the commit it was built from so a
//! long-lived tab can tell when it is running an older bundle. See
//! `docs/superpowers/specs/2026-09-05-app-update-system-design.md`.

/// Response header carrying the server's short git hash (`GIT_HASH`).
///
/// Lowercase and hyphenated on purpose: `http::HeaderName::from_static`
/// requires lowercase, and nginx-style proxies drop underscored names.
pub const APP_COMMIT_HEADER: &str = "x-ultros-commit";
```

- [ ] **Step 2: Register it in `ultros-api-types/src/lib.rs`**

The module list is alphabetical. Insert after `pub mod alert;`:

```rust
pub mod alert;
pub mod app_version;
pub mod bootstrap;
```

- [ ] **Step 3: Check it compiles**

Run: `cargo check -p ultros-api-types`
Expected: `Finished` with no warnings.

- [ ] **Step 4: Commit**

```bash
git add ultros-api-types/src/app_version.rs ultros-api-types/src/lib.rs
git commit -m "feat(api-types): add APP_COMMIT_HEADER constant

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Server stamps every response with `x-ultros-commit`

**Files:**
- Modify: `ultros/Cargo.toml:46`
- Modify: `ultros/src/web.rs` (near line 1957 `start_web`, and the layer stack near line 2166)
- Test: inline `#[cfg(test)]` module in `ultros/src/web.rs`

- [ ] **Step 1: Enable `tower`'s `util` feature**

In `ultros/Cargo.toml` change

```toml
tower = "0.5.3"
```

to

```toml
tower = { version = "0.5.3", features = ["util"] }
```

`ServiceExt::oneshot` lives behind `util`. It is a no-op if another dependency already turned it on.

- [ ] **Step 2: Write the failing test**

Append to the bottom of `ultros/src/web.rs`:

```rust
#[cfg(test)]
mod app_commit_header_tests {
    use super::app_commit_header_layer;
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
        routing::get,
    };
    use tower::ServiceExt;
    use ultros_api_types::app_version::APP_COMMIT_HEADER;

    fn router() -> Router {
        Router::new()
            .route("/ok", get(|| async { "ok" }))
            .layer(app_commit_header_layer())
    }

    async fn header_for(uri: &str) -> (StatusCode, Option<String>) {
        let response = router()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let header = response
            .headers()
            .get(APP_COMMIT_HEADER)
            .map(|v| v.to_str().unwrap().to_string());
        (response.status(), header)
    }

    #[tokio::test]
    async fn stamps_success_responses() {
        let (status, header) = header_for("/ok").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(header.as_deref(), Some(env!("GIT_HASH")));
    }

    #[tokio::test]
    async fn stamps_not_found_responses() {
        let (status, header) = header_for("/missing").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(header.as_deref(), Some(env!("GIT_HASH")));
    }
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p ultros app_commit_header 2>&1 | tail -20`
Expected: compile error `cannot find function app_commit_header_layer in module super`.

- [ ] **Step 4: Add the layer function**

In `ultros/src/web.rs`, directly above `pub(crate) async fn start_web(state: WebState) {` add:

```rust
/// Stamps every response with the commit this binary was built from so a
/// stale wasm bundle can notice the server moved on. Outermost layer, so it
/// covers SSR HTML, the JSON API, static files, `/pkg/`, and error responses.
/// The client side lives in `ultros_app::global_state::app_update`.
fn app_commit_header_layer() -> SetResponseHeaderLayer<HeaderValue> {
    SetResponseHeaderLayer::overriding(
        axum::http::HeaderName::from_static(ultros_api_types::app_version::APP_COMMIT_HEADER),
        HeaderValue::from_static(env!("GIT_HASH")),
    )
}
```

`HeaderName::from_static` panics on an uppercase name; the constant is lowercase, and the test in Step 2 exercises it.

- [ ] **Step 5: Mount it in the global layer stack**

In `start_web`, the stack currently ends with:

```rust
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        ));
```

Change it to:

```rust
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        ))
        .layer(app_commit_header_layer());
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test -p ultros app_commit_header 2>&1 | tail -20`
Expected: `test result: ok. 2 passed`.

- [ ] **Step 7: Run CI checks and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```
Expected: `REAL_EXIT=0`.

```bash
git add ultros/Cargo.toml Cargo.lock ultros/src/web.rs
git commit -m "feat(web): stamp every response with x-ultros-commit

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: Client-side mismatch decision and `AppUpdate` context

**Files:**
- Create: `ultros-frontend/ultros-app/src/global_state/app_update.rs`
- Modify: `ultros-frontend/ultros-app/src/global_state/mod.rs:1`
- Test: inline `#[cfg(test)]` module in the new file

- [ ] **Step 1: Register the module**

In `ultros-frontend/ultros-app/src/global_state/mod.rs` the list is alphabetical. Insert at the top:

```rust
pub mod app_update;
pub(crate) mod cheapest_prices;
```

- [ ] **Step 2: Write the failing tests**

Create `ultros-frontend/ultros-app/src/global_state/app_update.rs` with only the tests:

```rust
//! Detects when the wasm bundle running in this tab is older than the
//! server it is talking to.
//!
//! The server stamps every response with `x-ultros-commit` (see
//! `app_commit_header_layer` in `ultros/src/web.rs`). The client fetch
//! helpers in `crate::api` pass that header to [`observe_server_commit`],
//! which records the first mismatch in [`AppUpdate::pending`]. The state is
//! monotonic: once set it stays set until the page reloads, so a proxy
//! hiccup or a transient error page can never flip it back or nag twice.
//!
//! Spec: `docs/superpowers/specs/2026-09-05-app-update-system-design.md`.

#[cfg(test)]
mod tests {
    use super::update_pending;

    #[test]
    fn same_commit_is_not_pending() {
        assert!(!update_pending("283f84e5", Some("283f84e5")));
    }

    #[test]
    fn different_commit_is_pending() {
        assert!(update_pending("283f84e5", Some("fdf6a8cb")));
    }

    #[test]
    fn missing_or_empty_header_is_not_pending() {
        assert!(!update_pending("283f84e5", None));
        assert!(!update_pending("283f84e5", Some("")));
        assert!(!update_pending("283f84e5", Some("   ")));
    }

    #[test]
    fn dirty_on_either_side_is_not_pending() {
        assert!(!update_pending("dirty", Some("283f84e5")));
        assert!(!update_pending("283f84e5", Some("dirty")));
        assert!(!update_pending("dirty", Some("dirty")));
    }

    #[test]
    fn header_whitespace_is_ignored() {
        assert!(!update_pending("283f84e5", Some(" 283f84e5 ")));
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p ultros-app app_update 2>&1 | tail -20`
Expected: compile error `cannot find function update_pending`.

- [ ] **Step 4: Write the implementation**

Insert the following above the `#[cfg(test)]` module in the same file:

```rust
use leptos::prelude::*;

/// Short git hash this wasm bundle was built from. `dirty` when the build
/// script could not run git.
pub const CLIENT_COMMIT: &str = env!("GIT_HASH");

const DIRTY: &str = "dirty";

/// Whether the server's reported commit means this client is stale.
///
/// Conservative on purpose: a missing or empty header (older server, proxy
/// error page, connection dropped mid-deploy) and a `dirty` hash on either
/// side (local build without git) both mean "don't know", which is treated
/// as "not stale" so the app never nags without evidence.
pub fn update_pending(client: &str, server: Option<&str>) -> bool {
    let Some(server) = server.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    if client == DIRTY || server == DIRTY {
        return false;
    }
    client != server
}

/// App-wide update state. Provided once in `AppInner`.
#[derive(Clone, Copy)]
pub struct AppUpdate {
    /// The first server commit seen that differs from [`CLIENT_COMMIT`].
    /// Never cleared until the page reloads.
    pub pending: RwSignal<Option<String>>,
    /// The user closed the banner. Reload-on-navigation still applies.
    pub dismissed: RwSignal<bool>,
}

impl AppUpdate {
    pub fn banner_visible(&self) -> bool {
        self.pending.get().is_some() && !self.dismissed.get()
    }
}

// The fetch helpers run inside `spawn_local` with no guaranteed reactive
// owner, so the client keeps a thread-local handle to the signal. wasm is
// single-threaded, so this is exactly one slot per tab. It is compiled out of
// the SSR build on purpose: a thread-local on the server would leak state
// across requests sharing a worker thread.
#[cfg(not(feature = "ssr"))]
thread_local! {
    static PENDING: std::cell::RefCell<Option<RwSignal<Option<String>>>> =
        const { std::cell::RefCell::new(None) };
}

pub fn provide_app_update_context() -> AppUpdate {
    let update = AppUpdate {
        pending: RwSignal::new(None),
        dismissed: RwSignal::new(false),
    };
    provide_context(update);
    #[cfg(not(feature = "ssr"))]
    PENDING.with(|slot| *slot.borrow_mut() = Some(update.pending));
    update
}

pub fn use_app_update() -> Option<AppUpdate> {
    use_context::<AppUpdate>()
}

/// Called by every client fetch helper with the raw `x-ultros-commit` header.
/// Records the first mismatch and ignores everything after that.
#[cfg(not(feature = "ssr"))]
pub fn observe_server_commit(header: Option<&str>) {
    if !update_pending(CLIENT_COMMIT, header) {
        return;
    }
    let Some(server) = header else {
        return;
    };
    PENDING.with(|slot| {
        if let Some(pending) = slot.borrow().as_ref() {
            if pending.get_untracked().is_none() {
                pending.set(Some(server.trim().to_string()));
            }
        }
    });
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p ultros-app app_update 2>&1 | tail -20`
Expected: `test result: ok. 5 passed`.

- [ ] **Step 6: Run CI checks and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```
Expected: `REAL_EXIT=0`. If clippy reports `provide_app_update_context` or `use_app_update` as dead code, that is expected until Task 5 wires them in; add `#[allow(dead_code)]` only if the run actually fails, and remove it again in Task 5.

```bash
git add ultros-frontend/ultros-app/src/global_state/app_update.rs ultros-frontend/ultros-app/src/global_state/mod.rs
git commit -m "feat(app): add AppUpdate state and update_pending decision

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: Fetch helpers report the server commit

**Files:**
- Modify: `ultros-frontend/ultros-app/src/api.rs` — the four `#[cfg(not(feature = "ssr"))]` helpers: `delete_api` (~line 696), `fetch_api` (~line 779), `post_api` (~line 876), `patch_api` (~line 931)

Each helper currently chains `.send().await?...text().await?` in one expression. Split the chain so the response is a local, and report its header before reading the body. Use the fully qualified paths shown; do not add a top-level `use` for `APP_COMMIT_HEADER` or `observe_server_commit`, because the SSR build would flag it unused.

- [ ] **Step 1: `fetch_api` (client variant)**

Replace

```rust
            let inner_impl = async move || -> AppResult<String> {
                let json: String = gloo_net::http::Request::get(&path)
                    // .abort_signal(abort_signal.as_ref())
                    .send()
                    .await
                    .inspect_err(|e| error!(error = %e, path, "Error making http request"))?
                    .text()
                    .await?;
                Ok(json)
            };
```

with

```rust
            let inner_impl = async move || -> AppResult<String> {
                let response = gloo_net::http::Request::get(&path)
                    // .abort_signal(abort_signal.as_ref())
                    .send()
                    .await
                    .inspect_err(|e| error!(error = %e, path, "Error making http request"))?;
                report_server_commit(&response);
                let json: String = response.text().await?;
                Ok(json)
            };
```

- [ ] **Step 2: `delete_api` (client variant)**

Replace

```rust
        let inner_impl = async move || -> AppResult<String> {
            let json: String = gloo_net::http::Request::delete(&path)
                .credentials(web_sys::RequestCredentials::Include)
                .send()
                .await
                .inspect_err(|e| {
                    error!("{}", e);
                })?
                .text()
                .await?;
            Ok(json)
        };
```

with

```rust
        let inner_impl = async move || -> AppResult<String> {
            let response = gloo_net::http::Request::delete(&path)
                .credentials(web_sys::RequestCredentials::Include)
                .send()
                .await
                .inspect_err(|e| {
                    error!("{}", e);
                })?;
            report_server_commit(&response);
            let json: String = response.text().await?;
            Ok(json)
        };
```

- [ ] **Step 3: `post_api` (client variant)**

Replace

```rust
            let json: String = gloo_net::http::Request::post(&path)
                .header("Content-Type", "application/json")
                .credentials(web_sys::RequestCredentials::Include)
                .body(body)
                .map_err(|e| anyhow::anyhow!("failed to set json body: {:?}", e))?
                .send()
                .await
                .inspect_err(|e| {
                    log::error!("{e}");
                })?
                .text()
                .await
                .inspect_err(|e| log::error!("{e}"))?;
            Ok(json)
```

with

```rust
            let response = gloo_net::http::Request::post(&path)
                .header("Content-Type", "application/json")
                .credentials(web_sys::RequestCredentials::Include)
                .body(body)
                .map_err(|e| anyhow::anyhow!("failed to set json body: {:?}", e))?
                .send()
                .await
                .inspect_err(|e| {
                    log::error!("{e}");
                })?;
            report_server_commit(&response);
            let json: String = response
                .text()
                .await
                .inspect_err(|e| log::error!("{e}"))?;
            Ok(json)
```

- [ ] **Step 4: `patch_api` (client variant)**

Replace

```rust
            let json: String = gloo_net::http::Request::patch(&path)
                .header("Content-Type", "application/json")
                .credentials(web_sys::RequestCredentials::Include)
                .body(body)
                .map_err(|e| anyhow::anyhow!("failed to set json body: {:?}", e))?
                .send()
                .await
                .inspect_err(|e| {
                    log::error!("{e}");
                })?
                .text()
                .await
                .inspect_err(|e| log::error!("{e}"))?;
            Ok(json)
```

with

```rust
            let response = gloo_net::http::Request::patch(&path)
                .header("Content-Type", "application/json")
                .credentials(web_sys::RequestCredentials::Include)
                .body(body)
                .map_err(|e| anyhow::anyhow!("failed to set json body: {:?}", e))?
                .send()
                .await
                .inspect_err(|e| {
                    log::error!("{e}");
                })?;
            report_server_commit(&response);
            let json: String = response
                .text()
                .await
                .inspect_err(|e| log::error!("{e}"))?;
            Ok(json)
```

- [ ] **Step 5: Add the shared helper**

Directly above the client `delete_api` (the first `#[cfg(not(feature = "ssr"))]` helper, ~line 696) add:

```rust
/// Feed the server's `x-ultros-commit` header to the update detector. Runs on
/// error statuses too: a 500 from a newer server still carries the header.
#[cfg(not(feature = "ssr"))]
fn report_server_commit(response: &gloo_net::http::Response) {
    let header = response
        .headers()
        .get(ultros_api_types::app_version::APP_COMMIT_HEADER);
    crate::global_state::app_update::observe_server_commit(header.as_deref());
}
```

- [ ] **Step 6: Build the wasm side to verify it compiles**

Run: `cargo leptos build 2>&1 | tail -15`
Expected: ends with `Finished` for both the server and the front-end; no errors mentioning `api.rs`. (First build is slow; that is normal.)

- [ ] **Step 7: Run CI checks and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```
Expected: `REAL_EXIT=0`.

```bash
git add ultros-frontend/ultros-app/src/api.rs
git commit -m "feat(app): report x-ultros-commit from every client fetch

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: Update banner with i18n strings

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/update_banner.rs`
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs` (alphabetical list, near `undercut_alert_drawer`)
- Modify: `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json` (end of file)
- Modify: `ultros-frontend/ultros-app/src/lib.rs:20-30` (imports), `:469` (providers), `:500` (mount)

- [ ] **Step 1: Add the locale keys**

Each locale file ends with `"item_view_quality_all": "<value>"` followed by `}`. Add a comma after that line and insert the three keys. The exact final lines per file:

`en.json`:
```json
    "item_view_quality_all": "All",
    "update_banner_message": "Ultros has been updated.",
    "update_banner_reload": "Reload",
    "update_banner_dismiss": "Dismiss"
}
```

`fr.json`:
```json
    "item_view_quality_all": "Toutes",
    "update_banner_message": "Ultros a été mis à jour.",
    "update_banner_reload": "Recharger",
    "update_banner_dismiss": "Fermer"
}
```

`de.json`:
```json
    "item_view_quality_all": "Alle",
    "update_banner_message": "Ultros wurde aktualisiert.",
    "update_banner_reload": "Neu laden",
    "update_banner_dismiss": "Schließen"
}
```

`ja.json`:
```json
    "item_view_quality_all": "すべて",
    "update_banner_message": "Ultrosが更新されました。",
    "update_banner_reload": "再読み込み",
    "update_banner_dismiss": "閉じる"
}
```

`cn.json`:
```json
    "item_view_quality_all": "全部",
    "update_banner_message": "Ultros 已更新。",
    "update_banner_reload": "重新加载",
    "update_banner_dismiss": "关闭"
}
```

`ko.json`:
```json
    "item_view_quality_all": "전체",
    "update_banner_message": "Ultros가 업데이트되었습니다.",
    "update_banner_reload": "새로고침",
    "update_banner_dismiss": "닫기"
}
```

`tc.json`:
```json
    "item_view_quality_all": "全部",
    "update_banner_message": "Ultros 已更新。",
    "update_banner_reload": "重新載入",
    "update_banner_dismiss": "關閉"
}
```

Verify every file is still valid JSON and has all three keys:

```bash
for f in ultros-frontend/ultros-app/locales/*.json; do python3 -c "import json,sys; d=json.load(open('$f')); assert all(k in d for k in ['update_banner_message','update_banner_reload','update_banner_dismiss']), '$f'"; done && echo OK
```
Expected: `OK`.

- [ ] **Step 2: Create the component**

`ultros-frontend/ultros-app/src/components/update_banner.rs`:

```rust
//! Persistent "Ultros has been updated" banner. Shown once
//! `AppUpdate::pending` is set (see `global_state::app_update`) until the
//! user reloads or dismisses it. A dedicated element rather than a toast:
//! toasts auto-dismiss and have no action button.

use crate::components::icon::Icon;
use crate::global_state::app_update::use_app_update;
use crate::i18n::{t, t_string, use_i18n};
use icondata as i;
use leptos::prelude::*;

/// Full page reload. No-op on the server.
pub(crate) fn reload_page() {
    #[cfg(not(feature = "ssr"))]
    {
        let _ = window().location().reload();
    }
}

#[component]
pub fn UpdateBanner() -> impl IntoView {
    let i18n = use_i18n();
    let update = use_app_update();
    let visible = move || update.is_some_and(|u| u.banner_visible());
    let dismiss = move |_| {
        if let Some(update) = update {
            update.dismissed.set(true);
        }
    };

    view! {
        <Show when=visible>
            <div
                role="status"
                aria-live="polite"
                class="fixed bottom-0 inset-x-0 sm:inset-x-auto sm:left-4 sm:bottom-4 z-[90] flex items-center gap-3 p-4 sm:rounded-lg shadow-lg border text-sm bg-[color:var(--color-background-elevated)] border-[color:var(--color-outline)] text-[color:var(--color-text)]"
            >
                <Icon icon=i::BsArrowClockwise width="1.2em" height="1.2em" aria_hidden=true />
                <span class="flex-1">{t!(i18n, update_banner_message)}</span>
                <button class="btn-primary" on:click=move |_| reload_page()>
                    {t!(i18n, update_banner_reload)}
                </button>
                <button
                    class="opacity-70 hover:opacity-100 transition-opacity focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--brand-ring)] rounded"
                    aria-label=t_string!(i18n, update_banner_dismiss)
                    on:click=dismiss
                >
                    <Icon icon=i::BsX width="1.2em" height="1.2em" aria_hidden=true />
                </button>
            </div>
        </Show>
    }
}
```

`z-[90]` keeps it under the toast stack (`z-[100]` in `toast.rs`) so a toast can still appear on top.

- [ ] **Step 3: Register the module**

In `ultros-frontend/ultros-app/src/components/mod.rs`, the list is alphabetical:

```rust
pub mod undercut_alert_drawer;
pub mod update_banner;
pub mod virtual_scroller;
```

- [ ] **Step 4: Provide the context and mount the banner in `lib.rs`**

Imports: in the `use crate::global_state::{ ... }` block add `app_update::provide_app_update_context`, and in the `use crate::{ components::{ ... } }` block add `update_banner::UpdateBanner`:

```rust
use crate::global_state::{
    app_update::provide_app_update_context, cheapest_prices::CheapestPrices,
    clipboard_text::GlobalLastCopiedText, cookies::Cookies,
    side_nav::provide_side_nav_settings, theme::provide_theme_settings,
    toasts::provide_toast_context, xiv_data::provide_xiv_data_revision,
};
use crate::{
    components::{
        app_shell::AppShell, on_hand_input::provide_on_hand_context, patreon::*, toast::*,
        tooltip::*, update_banner::UpdateBanner,
    },
```

(Run `cargo fmt --all` afterwards; it will reflow the block.)

In `AppInner`, after `provide_toast_context();` add:

```rust
    provide_toast_context();
    provide_app_update_context();
```

In the `AppInner` view, after `<ToastContainer />` add `<UpdateBanner />`:

```rust
            <ToastContainer />
            <UpdateBanner />
            <Router>
```

- [ ] **Step 5: Build both sides**

Run: `cargo leptos build 2>&1 | tail -15`
Expected: `Finished` for server and front-end. A missing locale key fails here with a leptos-i18n error naming the locale; fix the JSON and rebuild.

- [ ] **Step 6: Run CI checks and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```
Expected: `REAL_EXIT=0`. If Task 3 needed a temporary `#[allow(dead_code)]`, remove it now.

```bash
git add ultros-frontend/ultros-app/src/components/update_banner.rs ultros-frontend/ultros-app/src/components/mod.rs ultros-frontend/ultros-app/src/lib.rs ultros-frontend/ultros-app/locales/
git commit -m "feat(app): show an update banner when the server commit changes

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: Full reload on the next navigation while stale

**Files:**
- Modify: `ultros-frontend/ultros-app/src/lib.rs` — next to `SentryRouteTag` (~line 596) and its mount (~line 502)

- [ ] **Step 1: Add the component**

Directly below `fn SentryRouteTag` in `lib.rs` add:

```rust
/// Once an update is pending, the next client-side route change becomes a
/// full page load, so the user lands on the requested page with the current
/// wasm bundle. Only `pathname` is watched: query-string changes (filters,
/// sort, world pickers) keep the user on the page and never reload. Must be
/// mounted inside `<Router>` because `use_location()` needs router context.
#[component]
fn ReloadWhenStale() -> impl IntoView {
    #[cfg(feature = "hydrate")]
    {
        use crate::global_state::app_update::use_app_update;
        let location = leptos_router::hooks::use_location();
        let update = use_app_update();
        Effect::new(move |previous: Option<String>| {
            let path = location.pathname.get();
            // Skip the first run: the path the page loaded on is not a navigation.
            let navigated = previous.as_deref().is_some_and(|p| p != path);
            let stale = update.is_some_and(|u| u.pending.get_untracked().is_some());
            if navigated && stale {
                crate::components::update_banner::reload_page();
            }
            path
        });
    }
}
```

`pending` is read untracked on purpose: the effect must fire on navigation, not the moment the mismatch is detected.

- [ ] **Step 2: Mount it inside `<Router>`**

```rust
            <Router>
                <SentryRouteTag />
                <ReloadWhenStale />
                <AppShell>
```

- [ ] **Step 3: Build both sides**

Run: `cargo leptos build 2>&1 | tail -15`
Expected: `Finished` for server and front-end.

- [ ] **Step 4: Run CI checks and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```
Expected: `REAL_EXIT=0`.

```bash
git add ultros-frontend/ultros-app/src/lib.rs
git commit -m "feat(app): full reload on next navigation once an update is pending

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: Build scripts rerun when git HEAD moves

**Files:**
- Modify: `ultros/build.rs`
- Modify: `ultros-frontend/ultros-app/build.rs:32-48`

Both scripts only rerun on their own changes today, so `GIT_HASH` goes stale after a commit until something else forces a rebuild. `git rev-parse --git-path <name>` resolves correctly inside worktrees (where `.git` is a file). `logs/HEAD` is the reflog, appended on every commit and checkout, so watching it plus `HEAD` tracks the tree without parsing symbolic refs.

- [ ] **Step 1: `ultros/build.rs`**

Replace the whole file with:

```rust
use std::process::Command;

// Emit GIT_HASH for use via `env!("GIT_HASH")`. Falls back to "dirty" when git
// is unavailable, the working tree isn't a real git checkout (worktree pointer
// files don't resolve inside containers, archives have no .git, etc.), or
// `git rev-parse` otherwise fails. Replaces the `git-const` proc-macro which
// hard-panics in those cases.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    rerun_when_head_moves();
    let git_hash = git_stdout(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "dirty".to_string());
    println!("cargo:rustc-env=GIT_HASH={git_hash}");
}

// The hash must track commits, so rerun when HEAD or the reflog changes.
// `--git-path` resolves inside worktrees; when git is unavailable nothing is
// emitted and the "dirty" fallback behaves as before.
fn rerun_when_head_moves() {
    for name in ["HEAD", "logs/HEAD"] {
        if let Some(path) = git_stdout(&["rev-parse", "--git-path", name]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}

fn git_stdout(args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
```

- [ ] **Step 2: `ultros-frontend/ultros-app/build.rs`**

Replace the `emit_git_hash` function (and its comment) at the bottom of the file with:

```rust
// Emit GIT_HASH for use via `env!("GIT_HASH")`. Falls back to "dirty" when git
// is unavailable, the working tree isn't a real git checkout (worktree pointer
// files don't resolve inside containers, archives have no .git, etc.), or
// `git rev-parse` otherwise fails. Replaces the `git-const` proc-macro which
// hard-panics in those cases.
fn emit_git_hash() {
    rerun_when_head_moves();
    let git_hash = git_stdout(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "dirty".to_string());
    println!("cargo:rustc-env=GIT_HASH={git_hash}");
}

// The hash must track commits, so rerun when HEAD or the reflog changes.
// `--git-path` resolves inside worktrees; when git is unavailable nothing is
// emitted and the "dirty" fallback behaves as before.
fn rerun_when_head_moves() {
    for name in ["HEAD", "logs/HEAD"] {
        if let Some(path) = git_stdout(&["rev-parse", "--git-path", name]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}

fn git_stdout(args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
```

- [ ] **Step 3: Verify the build scripts emit the rerun lines**

```bash
cargo build -p ultros-app 2>&1 | tail -3
grep -h "rerun-if-changed" target/debug/build/ultros-app-*/output | sort -u
```
Expected: lines including `cargo:rerun-if-changed=.../HEAD` and `cargo:rerun-if-changed=.../logs/HEAD` (paths will point into `.git/worktrees/<name>/` when run from a worktree).

- [ ] **Step 4: Verify the hash tracks a commit**

```bash
grep -h "GIT_HASH" target/debug/build/ultros-app-*/output
git commit --allow-empty -q -m "tmp: probe build-script rerun" && git rev-parse --short HEAD
cargo build -p ultros-app 2>&1 | grep -c "Compiling ultros-app"
grep -h "GIT_HASH" target/debug/build/ultros-app-*/output
git reset -q --soft HEAD~1
```
Expected: the second `GIT_HASH` line shows the new short hash from `git rev-parse`, and the `grep -c` shows `1` (the crate recompiled). The `reset --soft` drops the probe commit without touching the tree.

- [ ] **Step 5: Run CI checks and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```
Expected: `REAL_EXIT=0`.

```bash
git add ultros/build.rs ultros-frontend/ultros-app/build.rs
git commit -m "build: rerun GIT_HASH build scripts when git HEAD moves

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 8: Remove Fly config and unreferenced static scripts

**Files:**
- Delete: `fly.toml`, `ultros/static/search.js`, `ultros/static/retainer.js`

- [ ] **Step 1: Confirm nothing references them**

```bash
grep -rn "search\.js\|retainer\.js\|fly\.toml" --include='*.rs' --include='*.js' --include='*.html' --include='*.yml' --include='*.toml' --include='Dockerfile' --include='*.sh' . 2>/dev/null | grep -v "/target/" | grep -v node_modules | grep -v "docs/superpowers"
```
Expected: no output.

- [ ] **Step 2: Delete and commit**

```bash
git rm -q fly.toml ultros/static/search.js ultros/static/retainer.js
git commit -m "chore: drop unused fly.toml and unreferenced static scripts

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 9: End-to-end verification

**Files:** none modified.

- [ ] **Step 1: Header present on real responses**

Start the app (`HOSTNAME=http://localhost:8080 cargo leptos serve`, needs `.env` with `DATABASE_URL`, `DISCORD_*`, `KEY`), then in another shell:

```bash
curl -sI http://localhost:8080/ | grep -i x-ultros-commit
curl -sI http://localhost:8080/api/v1/world_data | grep -i x-ultros-commit
curl -sI http://localhost:8080/this-does-not-exist | grep -i x-ultros-commit
```
Expected: each prints `x-ultros-commit: <short hash>` matching `git rev-parse --short HEAD`.

- [ ] **Step 2: Stale-tab behaviour**

1. With the server running, open `http://localhost:8080/` in a browser and navigate to any item page.
2. Stop the server. Run `git commit --allow-empty -m "tmp: simulate deploy"`, then start the server again with `cargo leptos serve`.
3. In the still-open tab, change a filter or open a world picker so the app makes a fetch. The banner "Ultros has been updated." appears bottom-left.
4. Click **Reload**: the page reloads and the banner is gone (the tab now runs the new bundle).
5. Repeat steps 2 and 3, then click the banner's close button. Navigate to another page via the side nav. The tab does a full page load and lands on that page without the banner.
6. `git reset --soft HEAD~1` twice to drop the probe commits.

- [ ] **Step 3: E2E harness stays green**

```bash
./scripts/run_e2e.sh
```
Expected: passes with no `console.error` / `pageerror` failures. The banner is hidden by default, so no route should change.

- [ ] **Step 4: Final CI check**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -5 /tmp/ci.log
```
Expected: `REAL_EXIT=0`.
