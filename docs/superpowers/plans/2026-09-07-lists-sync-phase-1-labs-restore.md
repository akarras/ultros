# Lists Sync Phase 1: Labs Restore Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring back the Labs experiment toggle with one token, `lists-sync`, and split the `/list/:id` route so the toggle picks a (for now identical) `ListViewSync` component. Ships alone with no user-visible change unless the toggle is on.

**Architecture:** The Labs module is restored from git history (`bc2b1df2^`, deleted by PR #1305) with the retired `analyzer-recipe` token replaced by `lists-sync`. A `LabsSettings` section returns to the Settings page. A new `ListRoute` component reads `use_lab(LAB_LISTS_SYNC)` and renders `ListViewSync` or `ListView`; the cookie is server-visible so SSR and hydration agree.

**Tech Stack:** Leptos 0.8 (nightly signals), leptos-i18n, cookie-backed `Cookies` context, Puppeteer e2e under `integration/`.

## Global Constraints

- Spec: `docs/superpowers/specs/2026-09-07-lists-local-first-sync-design.md`, section 7.
- Every user-facing string goes through `leptos-i18n`; every new key is added to all seven locale files (`en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`) with a real translation.
- Run `./check_ci.sh` before every commit (fmt + clippy `-D warnings` + tests). Log to the session scratchpad, not `/tmp`.
- The non-Labs list page stays byte-for-byte unchanged in this phase.
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

---

## File structure

| File | Responsibility |
|---|---|
| `ultros-frontend/ultros-app/src/global_state/labs.rs` (create) | `Labs` cookie type, `LABS` registry, `use_lab(token)` signal |
| `ultros-frontend/ultros-app/src/global_state/mod.rs` (modify) | register `pub mod labs;` |
| `ultros-frontend/ultros-app/src/routes/settings.rs` (modify) | `LabsSettings` section, `lab_title`, `lab_desc`, mount in `Settings` |
| `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json` (modify) | six keys |
| `ultros-frontend/ultros-app/src/routes/list_view_sync.rs` (create) | `ListViewSync` placeholder and `ListRoute` switch |
| `ultros-frontend/ultros-app/src/routes/mod.rs` (modify) | register `pub mod list_view_sync;` |
| `ultros-frontend/ultros-app/src/lib.rs` (modify) | route `:id` renders `ListRoute` |
| `integration/list-flow.cjs` (modify) | optional `LABS_COOKIE` env to run the flow under Labs |

---

### Task 1: Restore the Labs module with the `lists-sync` token

**Files:**
- Create: `ultros-frontend/ultros-app/src/global_state/labs.rs`
- Modify: `ultros-frontend/ultros-app/src/global_state/mod.rs`

**Interfaces:**
- Produces: `pub const LABS_COOKIE: &str`, `pub const LAB_LISTS_SYNC: &str = "lists-sync"`, `pub const LABS: &[LabInfo]`, `pub struct Labs { pub enabled: BTreeSet<String> }` with `FromStr`/`Display`/`has(&str)`, `pub fn use_lab(token: &'static str) -> Signal<bool>`.

- [ ] **Step 1: Write the module with its tests**

```rust
//! Experiments a player can switch on before they become the default.
//! A cookie, not localStorage: the list page renders on the server, so a
//! client-only flag would hydrate a different page than it served.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use leptos::prelude::*;
use leptos_router::hooks::use_query_map;

use super::cookies::Cookies;

pub const LABS_COOKIE: &str = "LABS";

/// The local-first list document: lists live in the browser and sync through
/// the server. Remains opt-in until convergence has soaked on shared lists.
pub const LAB_LISTS_SYNC: &str = "lists-sync";

pub struct LabInfo {
    pub token: &'static str,
}

/// Every live experiment. Adding one here is what makes it appear in
/// Settings; deleting it is part of shipping the feature.
pub const LABS: &[LabInfo] = &[
    // Remove when the lists sync layer is promoted (spec section 7).
    LabInfo {
        token: LAB_LISTS_SYNC,
    },
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Labs {
    pub enabled: BTreeSet<String>,
}

impl Labs {
    pub fn has(&self, token: &str) -> bool {
        self.enabled.contains(token)
    }
}

fn is_known(token: &str) -> bool {
    LABS.iter().any(|l| l.token == token)
}

impl FromStr for Labs {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self {
            enabled: s
                .split(',')
                .map(str::trim)
                .filter(|t| !t.is_empty() && is_known(t))
                .map(String::from)
                .collect(),
        })
    }
}

impl fmt::Display for Labs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.enabled.iter().cloned().collect::<Vec<_>>().join(","))
    }
}

/// Read the server-visible cookie and optional tester URL override together.
/// A memo prevents unrelated query changes from rebuilding the page.
pub fn use_lab(token: &'static str) -> Signal<bool> {
    let cookie = use_context::<Cookies>().map(|c| c.use_cookie_typed::<_, Labs>(LABS_COOKIE).0);
    let query = use_query_map();
    Memo::new(move |_| {
        let from_cookie = cookie.is_some_and(|c| c.get().is_some_and(|l| l.has(token)));
        let from_url = query.with(|q| {
            q.get("labs")
                .and_then(|v| v.parse::<Labs>().ok())
                .is_some_and(|l| l.has(token))
        });
        from_cookie || from_url
    })
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labs_cookie_round_trips_known_tokens_only() {
        let labs: Labs = "lists-sync,bogus,,lists-sync".parse().unwrap();
        assert_eq!(labs.enabled.len(), 1);
        assert!(labs.has(LAB_LISTS_SYNC));
        assert_eq!(labs.to_string(), "lists-sync");
        let empty: Labs = "".parse().unwrap();
        assert!(!empty.has(LAB_LISTS_SYNC));
        assert_eq!(empty.to_string(), "");
    }

    /// Retired analyzer tokens are gone, not aliased: a stale cookie parses to
    /// the empty set.
    #[test]
    fn the_retired_analyzer_tokens_no_longer_parse() {
        let old: Labs = "analyzer-recipe,analyzer-ledger".parse().unwrap();
        assert!(old.enabled.is_empty(), "{old:?}");
    }

    #[test]
    fn every_lab_token_is_listed_once() {
        let mut tokens: Vec<&str> = LABS.iter().map(|l| l.token).collect();
        tokens.sort_unstable();
        tokens.dedup();
        assert_eq!(tokens.len(), LABS.len());
    }

    #[test]
    fn lists_sync_is_an_opt_in_experiment() {
        assert!(LABS.iter().any(|lab| lab.token == LAB_LISTS_SYNC));
        assert!(!Labs::default().has(LAB_LISTS_SYNC));
    }

    #[test]
    fn the_experiment_list_stays_short() {
        assert!(LABS.len() <= 3, "keep the experiment list short");
    }
}
```

- [ ] **Step 2: Register the module**

In `ultros-frontend/ultros-app/src/global_state/mod.rs`, add after `pub mod home_world;`:

```rust
pub mod labs;
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p ultros-app --lib global_state::labs`
Expected: 5 passed.

- [ ] **Step 4: Commit**

```bash
git add ultros-frontend/ultros-app/src/global_state/labs.rs ultros-frontend/ultros-app/src/global_state/mod.rs
git commit -m "feat(labs): restore the Labs module with a lists-sync token

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Settings section and locale keys

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/settings.rs` (imports at lines 1-25, `Settings` body at lines 340-361)
- Modify: `ultros-frontend/ultros-app/locales/en.json`, `fr.json`, `de.json`, `ja.json`, `cn.json`, `ko.json`, `tc.json`

**Interfaces:**
- Consumes: `LABS`, `LABS_COOKIE`, `Labs`, `LAB_LISTS_SYNC` from Task 1; `Cookies::use_cookie_typed`; `components::toggle::Toggle` (props `checked`, `set_checked`, `checked_label`, `unchecked_label`).

- [ ] **Step 1: Add the six locale keys to every locale**

Add to each file's top-level object (alphabetical placement is not enforced; put them next to each other). English:

```json
"labs_title": "Labs",
"labs_desc": "Features still being finished. Turn one on to try it before everyone gets it.",
"labs_on": "On",
"labs_off": "Off",
"labs_lists_sync_title": "Lists: local-first sync",
"labs_lists_sync_desc": "Your lists live in this browser and sync through the server: edits apply instantly, keep working offline, and merge with everyone sharing the list. Adds undo and redo (Ctrl+Z, Ctrl+Shift+Z) on the list page."
```

French:

```json
"labs_title": "Labo",
"labs_desc": "Fonctionnalités encore en cours de finalisation. Activez-en une pour l’essayer avant qu’elle ne soit accessible à tout le monde.",
"labs_on": "Activé",
"labs_off": "Désactivé",
"labs_lists_sync_title": "Listes : synchronisation locale d’abord",
"labs_lists_sync_desc": "Vos listes vivent dans ce navigateur et se synchronisent via le serveur : les modifications s’appliquent instantanément, continuent de fonctionner hors ligne et fusionnent avec celles des autres membres de la liste. Ajoute annuler et rétablir (Ctrl+Z, Ctrl+Maj+Z) sur la page de liste."
```

German:

```json
"labs_title": "Experimente",
"labs_desc": "Funktionen, die noch fertiggestellt werden. Aktiviere eine, um sie auszuprobieren, bevor sie für alle verfügbar ist.",
"labs_on": "An",
"labs_off": "Aus",
"labs_lists_sync_title": "Listen: lokale Synchronisierung",
"labs_lists_sync_desc": "Deine Listen leben in diesem Browser und werden über den Server synchronisiert: Änderungen greifen sofort, funktionieren auch offline und werden mit allen zusammengeführt, die die Liste teilen. Bringt Rückgängig und Wiederholen (Strg+Z, Strg+Umschalt+Z) auf die Listenseite."
```

Japanese:

```json
"labs_title": "ラボ",
"labs_desc": "まだ開発中の機能です。全員に公開される前に、オンにして試すことができます。",
"labs_on": "オン",
"labs_off": "オフ",
"labs_lists_sync_title": "リスト：ローカル優先の同期",
"labs_lists_sync_desc": "リストはこのブラウザに保存され、サーバー経由で同期されます。編集は即座に反映され、オフラインでも動作し、リストを共有している全員の変更と統合されます。リストページに元に戻す／やり直し（Ctrl+Z、Ctrl+Shift+Z）が追加されます。"
```

Simplified Chinese:

```json
"labs_title": "实验室",
"labs_desc": "仍在完善中的功能。开启其中一项，可在全员开放前抢先体验。",
"labs_on": "开",
"labs_off": "关",
"labs_lists_sync_title": "清单：本地优先同步",
"labs_lists_sync_desc": "清单保存在此浏览器中并通过服务器同步：编辑即时生效，离线也能继续使用，并与共享该清单的所有人合并。清单页面新增撤销与重做（Ctrl+Z、Ctrl+Shift+Z）。"
```

Korean:

```json
"labs_title": "실험실",
"labs_desc": "아직 다듬는 중인 기능입니다. 모두에게 공개되기 전에 켜서 먼저 사용해 보세요.",
"labs_on": "켬",
"labs_off": "끔",
"labs_lists_sync_title": "목록: 로컬 우선 동기화",
"labs_lists_sync_desc": "목록이 이 브라우저에 저장되고 서버를 통해 동기화됩니다. 편집이 즉시 반영되고, 오프라인에서도 계속 작동하며, 목록을 공유하는 모든 사람의 변경과 병합됩니다. 목록 페이지에 실행 취소와 다시 실행(Ctrl+Z, Ctrl+Shift+Z)이 추가됩니다."
```

Traditional Chinese:

```json
"labs_title": "實驗室",
"labs_desc": "仍在完善中的功能。開啟其中一項，可在全員開放前搶先體驗。",
"labs_on": "開",
"labs_off": "關",
"labs_lists_sync_title": "清單：本機優先同步",
"labs_lists_sync_desc": "清單保存在此瀏覽器中並透過伺服器同步：編輯即時生效，離線也能繼續使用，並與共享該清單的所有人合併。清單頁面新增復原與重做（Ctrl+Z、Ctrl+Shift+Z）。"
```

- [ ] **Step 2: Add the `LabsSettings` component to `settings.rs`**

Add `use leptos_i18n::I18nContext;` to the imports (after `use leptos::task::spawn_local;`). Then add above `pub fn Settings`:

```rust
#[component]
fn LabsSettings() -> impl IntoView {
    use crate::global_state::labs::{LABS, LABS_COOKIE, Labs};
    // Shipping the last experiment deletes its `LABS` entry; an empty
    // "Labs" box with nothing to toggle would outlive it.
    if LABS.is_empty() {
        return ().into_any();
    }
    let cookies = use_context::<Cookies>().unwrap();
    let (labs, set_labs) = cookies.use_cookie_typed::<_, Labs>(LABS_COOKIE);
    let i18n = use_i18n();
    view! {
        <div class="panel p-6 rounded-xl" data-testid="labs-settings">
            <h3 class="text-2xl font-bold text-[color:var(--brand-fg)] mb-2">{t!(i18n, labs_title)}</h3>
            <p class="text-sm text-[color:var(--color-text-muted)] mb-4">{t!(i18n, labs_desc)}</p>
            <div class="flex flex-col gap-4">
                {LABS.iter().map(|lab| {
                    let token = lab.token;
                    view! {
                        <div class="grid md:grid-cols-3 gap-4 items-center" data-testid=format!("lab-{token}")>
                            <div class="col-span-2">
                                <div class="font-semibold text-[color:var(--color-text)]">{lab_title(i18n, token)}</div>
                                <div class="text-sm text-[color:var(--color-text-muted)]">{lab_desc(i18n, token)}</div>
                            </div>
                            <Toggle
                                checked=Signal::derive(move || labs().unwrap_or_default().has(token))
                                set_checked=(move |checked: bool| {
                                    let mut current = labs.get_untracked().unwrap_or_default();
                                    if checked { current.enabled.insert(token.to_string()); } else { current.enabled.remove(token); }
                                    // Always write the set, even when it is empty: the shared cookie
                                    // helper's removal path does not carry the path/SameSite/Secure
                                    // attributes the write used, so a delete is silently ignored by the
                                    // browser and the lab could never be switched off. An empty set
                                    // serializes to `LABS=` and parses back to "nothing enabled".
                                    set_labs(Some(current));
                                }).into_signal_setter()
                                checked_label=t_string!(i18n, labs_on)
                                unchecked_label=t_string!(i18n, labs_off)
                            />
                        </div>
                    }
                }).collect_view()}
            </div>
        </div>
    }
    .into_any()
}

fn lab_title(i18n: I18nContext<Locale, I18nKeys>, token: &str) -> String {
    match token {
        crate::global_state::labs::LAB_LISTS_SYNC => {
            t_string!(i18n, labs_lists_sync_title).to_string()
        }
        _ => token.to_string(),
    }
}

fn lab_desc(i18n: I18nContext<Locale, I18nKeys>, token: &str) -> String {
    match token {
        crate::global_state::labs::LAB_LISTS_SYNC => {
            t_string!(i18n, labs_lists_sync_desc).to_string()
        }
        _ => String::new(),
    }
}
```

- [ ] **Step 3: Mount it**

In `Settings`, change

```rust
                <ThemePicker />
                <AdChoice />
```

to

```rust
                <ThemePicker />
                <AdChoice />
                <LabsSettings />
```

- [ ] **Step 4: Build the frontend crate and run its tests**

Run: `cargo check -p ultros-app --features hydrate --no-default-features` then `cargo test -p ultros-app --lib`
Expected: both succeed; the i18n build step reports no missing keys for any locale.

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/settings.rs ultros-frontend/ultros-app/locales
git commit -m "feat(labs): Settings section with the lists-sync toggle

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: Route split with a placeholder `ListViewSync`

**Files:**
- Create: `ultros-frontend/ultros-app/src/routes/list_view_sync.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/mod.rs`
- Modify: `ultros-frontend/ultros-app/src/lib.rs:542` (the `:id` route) and the `routes::{...}` import block at lines 50-70

**Interfaces:**
- Consumes: `use_lab`, `LAB_LISTS_SYNC` (Task 1); `routes::list_view::ListView`.
- Produces: `pub fn ListRoute() -> impl IntoView` (the route view), `pub fn ListViewSync() -> impl IntoView` (replaced wholesale in Phase 4).

- [ ] **Step 1: Write the module**

```rust
//! `/list/:id` behind the `lists-sync` Labs toggle.
//!
//! Phase 1 ships the switch with an identical page on both sides so the
//! toggle, cookie and route wiring can be exercised end to end before the
//! local-first document exists. Phase 4 replaces `ListViewSync`.

use leptos::prelude::*;

use crate::global_state::labs::{LAB_LISTS_SYNC, use_lab};
use crate::routes::list_view::ListView;

/// The Labs page. Identical to `ListView` until Phase 4.
#[component]
pub fn ListViewSync() -> impl IntoView {
    view! { <ListView /> }
}

/// Picks the page for `/list/:id`. The `LABS` cookie is server-visible, so
/// the server and the hydrating client make the same choice.
#[component]
pub fn ListRoute() -> impl IntoView {
    let sync = use_lab(LAB_LISTS_SYNC);
    move || {
        if sync.get() {
            view! { <ListViewSync /> }.into_any()
        } else {
            view! { <ListView /> }.into_any()
        }
    }
}
```

- [ ] **Step 2: Register and route**

In `routes/mod.rs`, add after `pub mod list_view;`:

```rust
pub mod list_view_sync;
```

In `lib.rs`, in the `routes::{ ... }` import block add `list_view_sync::ListRoute,` after `list_view::*,`, and change

```rust
                            <Route path=path!(":id") view=ListView />
```

to

```rust
                            <Route path=path!(":id") view=ListRoute />
```

- [ ] **Step 3: Build both halves**

Run: `cargo check -p ultros-app` and `cargo check -p ultros-app --features hydrate --no-default-features`
Expected: both succeed.

- [ ] **Step 4: Run the e2e list flow once as a smoke check**

Run: `./scripts/run_e2e.sh` with `LEPTOS_FEATURES="test-auth"` per `AGENTS.md`, or the targeted probe `npm --prefix integration run test:list-flow` against a running test-auth server.
Expected: passes unchanged (the route renders `ListView` without the cookie).

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/list_view_sync.rs ultros-frontend/ultros-app/src/routes/mod.rs ultros-frontend/ultros-app/src/lib.rs
git commit -m "feat(lists): route /list/:id through a Labs switch

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: Let the e2e list flow run under Labs

**Files:**
- Modify: `integration/list-flow.cjs` (the `login` helper at lines 28-37)

**Interfaces:**
- Produces: env `LABS_COOKIE` — when set, every logged-in page in the flow carries `LABS=<value>`.

- [ ] **Step 1: Set the cookie after login**

Replace the `login` function with:

```js
async function login(page, baseUrl, user) {
  const url = new URL("/test/login", baseUrl);
  url.searchParams.set("user_id", String(user.id));
  url.searchParams.set("username", user.username);
  url.searchParams.set("redirect", "/list");
  const resp = await page.goto(url.toString(), { waitUntil: "domcontentloaded" });
  if (!resp || resp.status() >= 400) {
    throw new Error(`test login failed for ${user.username}: ${resp ? resp.status() : -1}`);
  }
  // Opt this session into a Labs experiment. The cookie is server-visible,
  // so SSR and hydration agree; the page reloads below to pick it up.
  if (process.env.LABS_COOKIE) {
    await page.setCookie({ name: "LABS", value: process.env.LABS_COOKIE, url: baseUrl, path: "/" });
    await page.goto(new URL("/list", baseUrl).toString(), { waitUntil: "domcontentloaded" });
  }
}
```

- [ ] **Step 2: Run the flow both ways**

Run against a test-auth server:

```bash
cd integration && BASE_URL=http://127.0.0.1:8080 npm run test:list-flow
cd integration && BASE_URL=http://127.0.0.1:8080 LABS_COOKIE=lists-sync npm run test:list-flow
```

Expected: both pass. With the cookie, the Settings page shows the Labs box (verify by hand once: `data-testid="labs-settings"` present at `/settings`).

- [ ] **Step 3: Run CI checks and commit**

Run: `./check_ci.sh > "$SCRATCHPAD/ci.log" 2>&1; echo "REAL_EXIT=$?"`
Expected: `REAL_EXIT=0`.

```bash
git add integration/list-flow.cjs
git commit -m "test(e2e): LABS_COOKIE opts the list flow into an experiment

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

## Self-review

- Spec coverage: section 7 (Labs restore, token, route split, settings section, URL override) is covered by Tasks 1-3; the "runs twice, with and without the Labs cookie" test requirement from section 8 is prepared by Task 4.
- Placeholders: none. Every string has all seven locales.
- Type consistency: `use_lab(&'static str) -> Signal<bool>` is used identically in Tasks 1 and 3; `Toggle` props match the existing component.
