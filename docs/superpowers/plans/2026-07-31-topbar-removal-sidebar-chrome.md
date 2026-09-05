# Top Bar Removal — Sidebar-Owned Chrome Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Delete the 56px application top bar, moving search into a keyboard-accessible overlay and the locale/theme/account controls into a drop-up at the bottom of the sidebar, with a three-slot fixed bottom bar on mobile.

**Architecture:** Four stages. Stages 1–3 are purely additive — each leaves a fully working app that temporarily carries duplicate controls. Stage 4 is the atomic flip: the top bar is deleted and the mobile bar appears in the same commit, because shipping either alone leaves phones with broken or doubled chrome.

**Tech Stack:** Rust, Leptos 0.8 (SSR + hydrate), `leptos_router`, `leptos_hotkeys`, `leptos-use`, `leptos-i18n`, Tailwind v4 (`@utility` syntax).

**Spec:** [`docs/superpowers/specs/2026-07-31-topbar-removal-sidebar-chrome-design.md`](../specs/2026-07-31-topbar-removal-sidebar-chrome-design.md)

## Global Constraints

- **No hardcoded user-facing strings.** Every user-facing string in `ultros-frontend/ultros-app/` goes through `leptos-i18n` — `t!(i18n, key)` in `view!`, `t_string!(i18n, key)` for attribute values. Console logs and dev-only messages may stay English.
- **New i18n keys go in all seven locale files** — `en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc` in `ultros-frontend/ultros-app/locales/`. Real translations, not English stubs. `leptos-i18n` will not compile if a key is missing from any locale.
- **Snake_case i18n keys**, grouped by feature prefix.
- **Run `./check_ci.sh` from the repo root before every commit.** It runs `cargo fmt --all -- --check` and `cargo clippy --all-targets -- -D warnings`.
- **Read `check_ci.sh`'s exit code correctly** — never pipe into `tail`/`grep` and read `$?`. Use: `./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log`
- **On Windows/MSVC the `ultros` binary's test target does not link.** Scope clippy with `-p ultros-app` locally if the full workspace build fails for that reason. This is pre-existing and unrelated to this work.
- **Do not reintroduce `overflow-x` on `body`** in `style/tailwind.css`. It makes `body` a scroll container and breaks every viewport-sticky and fixed element, including the new mobile bar. See the existing comment at `style/tailwind.css:119-125`.
- **Branch:** `claude/top-bar-sidebar-redesign-d938db`.

## Testing Reality — Read Before Starting

CI for this repo does **not** run `cargo test` (it is commented out in `rust.yml`). Green CI proves the code compiles and is clippy-clean; it does not prove behavior.

This plan is mostly Leptos view code, which has no meaningful unit-test surface in this codebase — there is no component test harness. So:

- **Genuine TDD applies to the state layer only** (Task 2). That code is a plain struct with plain functions, mirroring the existing tested pattern in `global_state/side_nav.rs:78-95`.
- **All other tasks are gated by `./check_ci.sh` plus explicit manual verification steps.** Those manual steps are written out per task and are not optional — they are the only thing standing between this work and a visual regression.
- **Do not invent component tests that assert nothing** to satisfy a TDD checkbox. If a task has no real test, its gate is the compile plus the listed manual checks.

`ultros-app`'s lib tests **do** link on Windows/MSVC, so `cargo test -p ultros-app` works for Task 2.

## File Structure

| File | Responsibility |
|---|---|
| `global_state/search_overlay.rs` *(new)* | Open/closed state for the search overlay; the single source of truth all three triggers share |
| `components/search_overlay.rs` *(new)* | The overlay shell — backdrop, positioning, Escape, focus. Wraps `SearchBox`, owns the `Cmd/Ctrl+K` hotkey |
| `components/account_menu.rs` *(new)* | The sidebar footer account row and its drop-up panel |
| `components/mobile_bar.rs` *(new)* | The three-slot fixed bottom bar, below 1024px only |
| `components/language_picker.rs` | Gains `LanguageAccordion`; loses `LanguageNavMenu` in Stage 4 |
| `components/side_nav.rs` | Nav order, Explorer promotion, footer composition |
| `components/app_shell.rs` | Composes shell; drops `TopBar`, adds `MobileBar` + `SearchOverlay` |
| `components/search_box.rs` | Loses its hotkey (moves to the overlay), gains an `autofocus` prop |
| `style/tailwind.css` | Shell grid, drop-up, mobile bar, account row; `.top-bar*` removed in Stage 4 |
| `components/top_bar.rs`, `components/apps_menu.rs` | **Deleted** in Stage 4 |

---

# Stage 1 — Search overlay (additive)

After this stage the top bar still renders its inline `SearchBox`, and the overlay is reachable via `Cmd/Ctrl+K`. Both work. Nothing is removed.

---

### Task 1: Add the three missing i18n keys

Only three keys in this whole design don't already exist. Everything else (`account`, `profile`, `settings`, `logout`, `login_with_discord`, `switch_language`, `language`, `item_explorer`, `home`, `close`, `items`, `side_nav_tools`, `side_nav_saved`, `side_nav_toggle_navigation`, `help_label`, `version`) is already present in all seven locales.

**Files:**
- Modify: `ultros-frontend/ultros-app/locales/en.json`
- Modify: `ultros-frontend/ultros-app/locales/fr.json`
- Modify: `ultros-frontend/ultros-app/locales/de.json`
- Modify: `ultros-frontend/ultros-app/locales/ja.json`
- Modify: `ultros-frontend/ultros-app/locales/cn.json`
- Modify: `ultros-frontend/ultros-app/locales/ko.json`
- Modify: `ultros-frontend/ultros-app/locales/tc.json`

**Interfaces:**
- Produces: i18n keys `search`, `menu`, `sign_in` — used by Tasks 3, 7, 9.

- [ ] **Step 1: Confirm the keys are actually missing**

```bash
cd ultros-frontend/ultros-app/locales
for k in search menu sign_in; do printf "%-12s" "$k"; grep -q "\"$k\"" en.json && echo "EXISTS" || echo "MISSING"; done
```

Expected: all three report `MISSING`. If any reports `EXISTS`, skip that key everywhere below and reuse the existing one.

- [ ] **Step 2: Add the keys to each locale**

Add these three entries to each file, keeping the file's existing alphabetical or grouped ordering and its existing indentation.

`en.json`:
```json
  "search": "Search",
  "menu": "Menu",
  "sign_in": "Sign in",
```

`fr.json`:
```json
  "search": "Rechercher",
  "menu": "Menu",
  "sign_in": "Se connecter",
```

`de.json`:
```json
  "search": "Suchen",
  "menu": "Menü",
  "sign_in": "Anmelden",
```

`ja.json`:
```json
  "search": "検索",
  "menu": "メニュー",
  "sign_in": "ログイン",
```

`cn.json`:
```json
  "search": "搜索",
  "menu": "菜单",
  "sign_in": "登录",
```

`ko.json`:
```json
  "search": "검색",
  "menu": "메뉴",
  "sign_in": "로그인",
```

`tc.json`:
```json
  "search": "搜尋",
  "menu": "選單",
  "sign_in": "登入",
```

- [ ] **Step 3: Verify every locale parses and has all three keys**

```bash
cd ultros-frontend/ultros-app/locales
for f in *.json; do python -c "import json,sys; json.load(open('$f',encoding='utf-8'))" && printf "%-10s parsed  " "$f"; for k in search menu sign_in; do grep -q "\"$k\"" "$f" || echo "MISSING $k in $f"; done; echo ok; done
```

Expected: every file prints `parsed  ok` and nothing reports `MISSING`.

- [ ] **Step 4: Verify the crate still compiles**

```bash
cargo check -p ultros-app
```

Expected: success. A missing key in any locale fails here with a `leptos_i18n` macro error naming the locale.

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-app/locales/
git commit -m "i18n: add search, menu and sign_in keys across all locales"
```

---

### Task 2: Search overlay state

A plain struct in context, mirroring `SideNavSettings`. Deliberately **not** cookie-persisted — an overlay must never be open on page load.

**Files:**
- Create: `ultros-frontend/ultros-app/src/global_state/search_overlay.rs`
- Modify: `ultros-frontend/ultros-app/src/global_state/mod.rs`

**Interfaces:**
- Produces:
  - `pub struct SearchOverlayState { pub open: RwSignal<bool> }` (derives `Clone, Copy`)
  - `pub fn provide_search_overlay_state() -> SearchOverlayState`
  - `pub fn use_search_overlay_state() -> SearchOverlayState`
  - `impl SearchOverlayState { pub fn toggle(&self); pub fn close(&self); }`
- Consumed by Tasks 3, 4, 7.

- [ ] **Step 1: Write the failing test**

Create `ultros-frontend/ultros-app/src/global_state/search_overlay.rs` containing **only** the test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_closed() {
        let state = SearchOverlayState::new();
        assert!(!state.open.get_untracked());
    }

    #[test]
    fn toggle_flips_open() {
        let state = SearchOverlayState::new();
        state.toggle();
        assert!(state.open.get_untracked());
        state.toggle();
        assert!(!state.open.get_untracked());
    }

    #[test]
    fn close_is_idempotent() {
        let state = SearchOverlayState::new();
        state.close();
        assert!(!state.open.get_untracked());
        state.toggle();
        state.close();
        state.close();
        assert!(!state.open.get_untracked());
    }
}
```

Register the module by adding this line to `ultros-frontend/ultros-app/src/global_state/mod.rs`, alongside the existing `pub mod side_nav;`:

```rust
pub mod search_overlay;
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test -p ultros-app search_overlay
```

Expected: FAIL to compile — `cannot find type SearchOverlayState in this scope`.

- [ ] **Step 3: Write the minimal implementation**

Insert above the test module in `search_overlay.rs`:

```rust
//! Search overlay open/closed state.
//!
//! Deliberately **not** persisted: an overlay that restored itself as open
//! on page load would trap the user behind a modal on every navigation.
//! Contrast [`SideNavSettings::collapsed`](super::side_nav::SideNavSettings),
//! which is cookie-backed on purpose.

use leptos::prelude::*;

/// Shared open state for the search overlay. Every trigger — the sidebar
/// row, the mobile bar button, and the `Cmd`/`Ctrl`+K hotkey — flips this
/// one signal, so they can never disagree about whether the overlay is up.
#[derive(Clone, Copy)]
pub struct SearchOverlayState {
    pub open: RwSignal<bool>,
}

impl SearchOverlayState {
    fn new() -> Self {
        Self {
            open: RwSignal::new(false),
        }
    }

    /// Flip the overlay open or closed.
    pub fn toggle(&self) {
        self.open.update(|v| *v = !*v);
    }

    /// Force the overlay closed. Safe to call when already closed.
    pub fn close(&self) {
        self.open.set(false);
    }
}

/// Provide `SearchOverlayState` into context if absent, and return it.
pub fn provide_search_overlay_state() -> SearchOverlayState {
    if let Some(existing) = use_context::<SearchOverlayState>() {
        return existing;
    }
    let state = SearchOverlayState::new();
    provide_context(state);
    state
}

/// Retrieve `SearchOverlayState` from context. Panics if not provided.
pub fn use_search_overlay_state() -> SearchOverlayState {
    use_context::<SearchOverlayState>().expect("SearchOverlayState not provided")
}
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test -p ultros-app search_overlay
```

Expected: PASS — 3 passed.

- [ ] **Step 5: Run CI checks**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

Expected: `REAL_EXIT=0`.

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/global_state/
git commit -m "feat(search): add search overlay state"
```

---

### Task 3: Move the hotkey out of SearchBox and build the overlay

`SearchBox` currently registers `Cmd/Ctrl+K` itself at `search_box.rs:234` and uses it to focus its own inline input. That only works because the top bar keeps one `SearchBox` permanently mounted. Once a second `SearchBox` exists inside the overlay, two instances would register the same combo.

So the hotkey moves to the overlay. The top bar's inline box keeps working by click for the rest of Stage 1–3; it is deleted in Stage 4.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/search_box.rs:233-239` (remove hotkey), and the component signature
- Create: `ultros-frontend/ultros-app/src/components/search_overlay.rs`
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs`
- Modify: `ultros-frontend/ultros-app/src/components/app_shell.rs`
- Modify: `style/tailwind.css`

**Interfaces:**
- Consumes: `use_search_overlay_state()`, `SearchOverlayState` (Task 2); i18n key `search` (Task 1)
- Produces: `#[component] pub fn SearchOverlay() -> impl IntoView`; `SearchBox` gains `#[prop(optional)] autofocus: bool`

- [ ] **Step 1: Remove the hotkey from `SearchBox` and add the autofocus prop**

In `search_box.rs`, delete this block (currently lines 233–239):

```rust
    // Hotkey to focus search (Cmd+K / Ctrl+K)
    use_hotkeys!(("MetaLeft+KeyK,ControlLeft+KeyK", "*") => move |_| {
        set_active(true);
        if let Some(input) = text_input.get() {
            let _ = input.focus();
        }
    });
```

Remove the now-unused import on line 9:

```rust
use leptos_hotkeys::use_hotkeys;
```

Leave `leptos_hotkeys::use_hotkeys_ref` (line 242) alone — it is a different import path and still used.

Change the component signature from:

```rust
pub fn SearchBox() -> impl IntoView {
```

to:

```rust
pub fn SearchBox(#[prop(optional)] autofocus: bool) -> impl IntoView {
```

Then, immediately after the `let keydown = move |e: KeyboardEvent| { ... };` closure ends (before the `view!` macro), add:

```rust
    // When mounted inside the overlay we want the caret in the field
    // immediately — the user pressed a key to get here. Effect, not a
    // render-time call: the input doesn't exist until after mount.
    if autofocus {
        Effect::new(move |_| {
            if let Some(input) = text_input.get() {
                let _ = input.focus();
                set_active(true);
            }
        });
    }
```

- [ ] **Step 2: Verify existing callers still compile**

```bash
cargo check -p ultros-app
```

Expected: success. `#[prop(optional)]` means the existing `<SearchBox />` in `top_bar.rs` needs no change.

- [ ] **Step 3: Create the overlay component**

Create `ultros-frontend/ultros-app/src/components/search_overlay.rs`:

```rust
//! Global search overlay.
//!
//! Owns the `Cmd`/`Ctrl`+K hotkey (moved here out of [`SearchBox`], which
//! could only handle it while a single instance was permanently mounted in
//! the old top bar).
//!
//! On mobile the input is anchored to the **top** of the sheet. This is
//! load-bearing, not cosmetic: iOS Safari shrinks the visual viewport but
//! not the layout viewport when the keyboard opens, so a bottom-anchored
//! input ends up behind the keyboard with no pure-CSS remedy.

use crate::components::search_box::SearchBox;
use crate::global_state::search_overlay::use_search_overlay_state;
use crate::i18n::{t_string, use_i18n};
use leptos::prelude::*;
use leptos_hotkeys::use_hotkeys;
use leptos_router::hooks::use_location;

#[component]
pub fn SearchOverlay() -> impl IntoView {
    let i18n = use_i18n();
    let state = use_search_overlay_state();
    let open = state.open;

    use_hotkeys!(("MetaLeft+KeyK,ControlLeft+KeyK", "*") => move |_| {
        open.update(|v| *v = !*v);
    });

    // Any navigation dismisses the overlay — selecting a result routes, and
    // leaving the sheet up over the destination would be a trap.
    let location = use_location();
    Effect::new(move |_| {
        let _ = location.pathname.get();
        open.set(false);
    });

    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Escape" {
            open.set(false);
        }
    };

    view! {
        <Show when=move || open.get()>
            <div
                class="search-overlay"
                role="dialog"
                aria-modal="true"
                aria-label=t_string!(i18n, search).to_string()
                on:keydown=on_keydown
            >
                <div
                    class="search-overlay-backdrop"
                    on:click=move |_| open.set(false)
                />
                <div class="search-overlay-panel">
                    <SearchBox autofocus=true />
                </div>
            </div>
        </Show>
    }
    .into_any()
}
```

- [ ] **Step 4: Register the module**

In `ultros-frontend/ultros-app/src/components/mod.rs`, add alongside the other `pub mod` lines:

```rust
pub mod search_overlay;
```

- [ ] **Step 5: Add the overlay CSS**

Append to `style/tailwind.css`, after the existing topbar block (which ends near line 1917):

```css
/* ----- Search overlay ----- */
.search-overlay {
    position: fixed;
    inset: 0;
    z-index: 80;
}
.search-overlay-backdrop {
    position: absolute;
    inset: 0;
    background-color: color-mix(in srgb, black 55%, transparent);
}
.search-overlay-panel {
    position: relative;
    z-index: 1;
    width: 100%;
    max-width: 560px;
    margin: 12vh auto 0;
    padding: 0 1rem;
}

/* Mobile: full-screen sheet, input pinned to the top so the virtual
   keyboard pushes content the way the browser expects. */
@media (max-width: 1023px) {
    .search-overlay-panel {
        max-width: none;
        margin: 0;
        padding: 0.75rem;
        height: 100dvh;
        background-color: var(--color-background);
    }
}
```

- [ ] **Step 6: Mount the overlay and provide its state**

In `ultros-frontend/ultros-app/src/components/app_shell.rs`, add the imports:

```rust
use crate::components::search_overlay::SearchOverlay;
use crate::global_state::search_overlay::provide_search_overlay_state;
```

Inside `AppShell`, immediately after `let nav = provide_side_nav_settings();`, add:

```rust
    provide_search_overlay_state();
```

And in the `view!`, add `<SearchOverlay />` as the last child of the shell `div`, after the ad rail:

```rust
            <div class="app-shell-ad-rail">
                <DesktopAdRail />
            </div>

            <SearchOverlay />
```

- [ ] **Step 7: Run CI checks**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

Expected: `REAL_EXIT=0`.

- [ ] **Step 8: Manual verification**

Run the app and confirm:
1. `Ctrl+K` (or `Cmd+K`) opens the overlay with the caret already in the field.
2. Typing shows results; clicking one navigates **and** closes the overlay.
3. `Escape` closes it.
4. Clicking the backdrop closes it.
5. `Ctrl+K` a second time while open closes it.
6. The top bar's inline search box still works by clicking into it.
7. At 375px width the sheet is full-screen with the input at the top.

- [ ] **Step 9: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/ style/tailwind.css
git commit -m "feat(search): add search overlay with Cmd/Ctrl+K"
```

---

# Stage 2 — Account drop-up (additive)

The top bar keeps its own language/theme/user controls throughout this stage. The sidebar gains a parallel set. Duplicated but fully functional.

---

### Task 4: Language accordion

`LanguageNavMenu` is a hover-driven drop-down that will be deleted in Stage 4. This adds a click-driven, inline-expanding variant for use inside the account drop-up. Both coexist during Stages 2–3.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/language_picker.rs`
- Modify: `style/tailwind.css`

**Interfaces:**
- Produces: `#[component] pub fn LanguageAccordion() -> impl IntoView`
- Reuses the existing private `LANGUAGE_OPTIONS` const and `reload_locale_data()` fn in the same file — do not duplicate either.

- [ ] **Step 1: Add the accordion component**

Append to `language_picker.rs`:

```rust
/// Language switcher as an inline accordion, for use inside the account
/// drop-up.
///
/// Deliberately not a flyout submenu: the sidebar doubles as the mobile
/// drawer below 1024px, and a hover-opened flyout has no touch equivalent.
#[component]
pub fn LanguageAccordion() -> impl IntoView {
    let i18n = use_i18n();
    let (expanded, set_expanded) = signal(false);
    let selected = Selector::new(move || i18n.get_locale());

    let set_language = move |new_locale: Locale| {
        i18n.set_locale(new_locale);
        reload_locale_data(new_locale);
        set_expanded(false);
    };

    view! {
        <button
            type="button"
            class="menu-item"
            aria-expanded=move || if expanded.get() { "true" } else { "false" }
            on:click=move |_| set_expanded.update(|v| *v = !*v)
        >
            <Icon icon=i::IoLanguage width="1.1em" height="1.1em" />
            <span class="ml-2">{t!(i18n, language)}</span>
            <span class="menu-item-trailing">
                {move || i18n.get_locale().as_str().to_uppercase()}
            </span>
        </button>

        <Show when=move || expanded.get()>
            <div class="menu-accordion" role="group" aria-label=t_string!(i18n, language).to_string()>
                {LANGUAGE_OPTIONS
                    .into_iter()
                    .map(|option| {
                        let selected_for_class = selected.clone();
                        let selected_for_aria = selected.clone();
                        let selected_for_show = selected.clone();
                        view! {
                            <button
                                type="button"
                                role="menuitemradio"
                                class=move || {
                                    if selected_for_class.selected(&option.locale) {
                                        "menu-item menu-item-selected"
                                    } else {
                                        "menu-item"
                                    }
                                }
                                aria-checked=move || selected_for_aria.selected(&option.locale).to_string()
                                on:click=move |_| set_language(option.locale)
                            >
                                <span class="menu-item-code">{option.locale.as_str()}</span>
                                <span class="ml-2 truncate">{option.native_name}</span>
                                <Show when=move || selected_for_show.selected(&option.locale)>
                                    <span class="menu-item-trailing">
                                        <Icon icon=i::BsCheckCircleFill width="0.9em" height="0.9em" />
                                    </span>
                                </Show>
                            </button>
                        }
                    })
                    .collect::<Vec<_>>()}
            </div>
        </Show>
    }
    .into_any()
}
```

- [ ] **Step 2: Add the shared menu CSS**

These classes are used by both `LanguageAccordion` and `AccountMenu` (Task 5). Append to `style/tailwind.css`:

```css
/* ----- Drop-up menu primitives (account menu + language accordion) ----- */
.menu-item {
    display: flex;
    align-items: center;
    width: 100%;
    padding: 0.4rem 0.55rem;
    border-radius: 0.375rem;
    color: var(--color-text);
    background: transparent;
    border: none;
    font-size: 0.85rem;
    text-align: left;
    text-decoration: none;
}
.menu-item:hover {
    background-color: color-mix(in srgb, var(--brand-ring) 18%, transparent);
}
.menu-item-selected {
    background-color: color-mix(in srgb, var(--brand-ring) 28%, transparent);
}
.menu-item-trailing {
    margin-left: auto;
    font-size: 0.7rem;
    font-weight: 700;
    color: var(--color-text-muted);
    display: inline-flex;
    align-items: center;
}
.menu-item-code {
    width: 1.75rem;
    flex: none;
    font-size: 0.65rem;
    font-weight: 700;
    text-transform: uppercase;
    color: var(--color-text-muted);
}
.menu-accordion {
    display: flex;
    flex-direction: column;
    gap: 0.05rem;
    padding-left: 0.85rem;
    max-height: 15rem;
    overflow-y: auto;
}
.menu-divider {
    height: 1px;
    background-color: var(--color-outline);
    margin: 0.3rem 0.15rem;
}
```

- [ ] **Step 3: Run CI checks**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

Expected: `REAL_EXIT=0`. `LanguageAccordion` is not yet mounted anywhere, so clippy may warn it is unused — if it does, that resolves in Task 5. If clippy fails the build on `dead_code`, proceed directly to Task 5 and commit them together rather than adding an `#[allow]`.

- [ ] **Step 4: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/language_picker.rs style/tailwind.css
git commit -m "feat(i18n): add inline language accordion for the account menu"
```

---

### Task 5: Account drop-up component

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/account_menu.rs`
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs`
- Modify: `ultros-frontend/ultros-app/src/components/theme_picker.rs:127-155` (add the `menu_item` prop)
- Modify: `style/tailwind.css`

**Interfaces:**
- Consumes: `LanguageAccordion` (Task 4); `QuickThemeToggle` from `components::theme_picker`; `crate::api::get_login`; i18n key `sign_in` (Task 1)
- Produces: `#[component] pub fn AccountMenu() -> impl IntoView`
- Consumed by Task 6.

Two behaviors differ deliberately from the `UserMenu` this replaces:

1. **Click to toggle**, not hover. `UserMenu` derives `is_open` from `use_element_hover` + `focusin` (`apps_menu.rs:199`), which has no touch equivalent — and this sidebar *is* the mobile drawer.
2. **The signed-out panel carries Language.** The old logged-out menu (`apps_menu.rs:322-336`) had no language entry because locale lived in a separate top-bar control. Omitting it here would strand non-English visitors.

- [ ] **Step 1: Create the component**

Create `ultros-frontend/ultros-app/src/components/account_menu.rs`:

```rust
//! Account row + drop-up for the sidebar footer.
//!
//! Replaces `UserMenu`. Two deliberate changes from that component:
//!
//! 1. Opens on **click**, not hover. The old hover trigger
//!    (`use_element_hover`) has no touch equivalent, and this sidebar is
//!    the mobile drawer below 1024px.
//! 2. The signed-out panel includes Language. Previously locale lived in a
//!    separate top-bar control, so the signed-out menu omitted it —
//!    carrying that omission over would leave non-English visitors with no
//!    switcher at all.

use crate::api::get_login;
use crate::components::icon::Icon;
use crate::components::language_picker::LanguageAccordion;
use crate::components::theme_picker::QuickThemeToggle;
use crate::i18n::{t, t_string, use_i18n};
use cfg_if::cfg_if;
use icondata as i;
use leptos::html;
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_location;

#[component]
pub fn AccountMenu() -> impl IntoView {
    let i18n = use_i18n();
    let (open, set_open) = signal(false);
    let user = Resource::new(move || {}, move |_| async move { get_login().await.ok() });
    let root_ref = NodeRef::<html::Div>::new();

    // Outside-click dismissal. Hydrate-only: there is no document to listen
    // to on the server, and the same cfg_if guard is used for
    // `use_element_hover` elsewhere in this codebase.
    cfg_if! {
        if #[cfg(feature = "hydrate")] {
            let _ = leptos_use::on_click_outside(root_ref, move |_| set_open(false));
        }
    }

    // Dismiss on navigation, so the panel never survives a route change.
    let location = use_location();
    Effect::new(move |_| {
        let _ = location.pathname.get();
        set_open(false);
    });

    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Escape" {
            set_open(false);
        }
    };

    view! {
        <div class="side-nav-account" node_ref=root_ref on:keydown=on_keydown>
            <Show when=move || open.get()>
                <div class="side-nav-account-panel" role="menu" tabindex="-1">
                    <Suspense fallback=move || {
                        view! { <div class="menu-item muted">{t!(i18n, loading)}</div> }
                    }>
                        {move || {
                            let signed_in = user.get().flatten().is_some();
                            if signed_in {
                                view! {
                                    <A href="/profile" attr:class="menu-item">
                                        <Icon icon=i::BsPersonCircle width="1.1em" height="1.1em" />
                                        <span class="ml-2">{t!(i18n, profile)}</span>
                                    </A>
                                    <A href="/settings" attr:class="menu-item">
                                        <Icon icon=i::IoSettingsSharp width="1.1em" height="1.1em" />
                                        <span class="ml-2">{t!(i18n, settings)}</span>
                                    </A>
                                    <div class="menu-divider"></div>
                                    <LanguageAccordion />
                                    <QuickThemeToggle />
                                    <div class="menu-divider"></div>
                                    // No icon — matches the existing logout
                                    // link, which is also icon-less. Don't
                                    // invent an icondata identifier here
                                    // without checking it resolves.
                                    <a rel="external" href="/logout" class="menu-item">
                                        <span class="ml-2">{t!(i18n, logout)}</span>
                                    </a>
                                }
                                    .into_any()
                            } else {
                                view! {
                                    <a rel="external" href="/login" class="menu-item">
                                        <Icon icon=i::BsDiscord width="1.1em" height="1.1em" />
                                        <span class="ml-2">{t!(i18n, login_with_discord)}</span>
                                    </a>
                                    <A href="/settings" attr:class="menu-item">
                                        <Icon icon=i::IoSettingsSharp width="1.1em" height="1.1em" />
                                        <span class="ml-2">{t!(i18n, settings)}</span>
                                    </A>
                                    <div class="menu-divider"></div>
                                    <LanguageAccordion />
                                    <QuickThemeToggle />
                                }
                                    .into_any()
                            }
                        }}
                    </Suspense>
                </div>
            </Show>

            <button
                class="side-nav-account-trigger"
                aria-haspopup="menu"
                aria-expanded=move || if open.get() { "true" } else { "false" }
                aria-label=t_string!(i18n, account).to_string()
                on:click=move |_| set_open.update(|v| *v = !*v)
            >
                <Suspense fallback=move || {
                    view! { <Icon icon=i::BsPersonCircle width="1.25em" height="1.25em" /> }
                }>
                    {move || {
                        match user.get().flatten() {
                            Some(auth) => {
                                view! {
                                    <img class="avatar" src=auth.avatar alt=auth.username.clone() />
                                    <span class="side-nav-label ml-2">{auth.username}</span>
                                }
                                    .into_any()
                            }
                            None => {
                                view! {
                                    <Icon icon=i::BsPersonCircle width="1.25em" height="1.25em" />
                                    <span class="side-nav-label ml-2">{t!(i18n, sign_in)}</span>
                                }
                                    .into_any()
                            }
                        }
                    }}
                </Suspense>
                <Icon
                    icon=i::BiChevronUpSolid
                    width="1em"
                    height="1em"
                    attr:class="side-nav-account-caret"
                />
            </button>
        </div>
    }
    .into_any()
}
```

- [ ] **Step 2: Register the module**

In `components/mod.rs`:

```rust
pub mod account_menu;
```

- [ ] **Step 3: Add the account row CSS**

Append to `style/tailwind.css`:

```css
/* ----- Sidebar account row + drop-up ----- */
.side-nav-account {
    position: relative;
    border-top: 1px solid var(--color-outline);
}
.side-nav-account-trigger {
    display: flex;
    align-items: center;
    width: 100%;
    padding: 0.55rem 0.7rem;
    color: var(--color-text);
    background: transparent;
    border: none;
    font-size: 0.85rem;
    text-align: left;
}
.side-nav-account-trigger:hover {
    background-color: color-mix(in srgb, var(--brand-ring) 14%, transparent);
}
.side-nav-account-caret {
    margin-left: auto;
    flex: none;
    color: var(--color-text-muted);
}

/* Anchored to the bottom so the panel grows UPWARD and is never clipped
   by the viewport edge — the whole point of a drop-up. */
.side-nav-account-panel {
    position: absolute;
    bottom: calc(100% + 0.35rem);
    left: 0.35rem;
    right: 0.35rem;
    z-index: 70;
    display: flex;
    flex-direction: column;
    gap: 0.05rem;
    padding: 0.3rem;
    border: 1px solid var(--color-outline);
    border-radius: 0.6rem;
    background-color: var(--color-background-elevated);
    box-shadow: 0 -10px 30px color-mix(in srgb, black 45%, transparent);
    max-height: 80vh;
    overflow-y: auto;
}

/* Collapsed sidebar is only 56px wide, so the panel cannot stay inset to
   both edges — it breaks out to a fixed width and overhangs the content. */
@media (min-width: 1024px) {
    .app-shell-collapsed .side-nav-account-trigger {
        justify-content: center;
        padding-inline: 0;
    }
    .app-shell-collapsed .side-nav-account-caret {
        display: none;
    }
    .app-shell-collapsed .side-nav-account-panel {
        left: 0.35rem;
        right: auto;
        width: 15rem;
    }
}
```

- [ ] **Step 4: Make `QuickThemeToggle` fit the panel**

`QuickThemeToggle` renders `<button class="nav-link">` with a `hidden lg:inline` label (`theme_picker.rs:144-153`), which was styled for the old top bar. Inside the drop-up it will sit among `.menu-item` siblings and look wrong — and its label will vanish below `lg`, which is exactly the mobile drawer case.

Add an optional prop rather than forking the component. In `theme_picker.rs`, change the signature:

```rust
pub fn QuickThemeToggle(#[prop(optional)] menu_item: bool) -> impl IntoView {
```

and replace the `class` and `<span>` in its `view!` with:

```rust
            class=move || if menu_item { "menu-item" } else { "nav-link" }
```

```rust
            <span class=move || if menu_item { "ml-2" } else { "hidden lg:inline" }>{label}</span>
```

Then in `account_menu.rs`, use `<QuickThemeToggle menu_item=true />` in **both** the signed-in and signed-out branches.

The existing call in `top_bar.rs` needs no change — `#[prop(optional)]` defaults `menu_item` to `false`.

- [ ] **Step 5: Run CI checks**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

Expected: `REAL_EXIT=0`. If clippy reports `leptos_use::on_click_outside` not found, check the installed leptos-use version's export path with `grep -rn "on_click_outside" ~/.cargo/registry/src/*/leptos-use-*/src/lib.rs` and adjust the import; do **not** silence it.

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/ style/tailwind.css
git commit -m "feat(nav): add sidebar account drop-up"
```

---

### Task 6: Mount the account menu in the sidebar footer

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/side_nav.rs:130-144`

**Interfaces:**
- Consumes: `AccountMenu` (Task 5)

- [ ] **Step 1: Add the import**

In `side_nav.rs`:

```rust
use crate::components::account_menu::AccountMenu;
```

- [ ] **Step 2: Insert the account row above the existing utility strip**

Replace the existing footer block (currently `side_nav.rs:130-144`) with:

```rust
            <AccountMenu />

            <div class="side-nav-footer">
                <a href=crate::DISCORD_INVITE class="side-nav-icon-link" aria-label="Discord">
                    <Icon icon=i::BsDiscord />
                </a>
                <a href="https://github.com/akarras/ultros" class="side-nav-icon-link" aria-label="GitHub">
                    <Icon icon=i::IoLogoGithub />
                </a>
                <a
                    href=format!("https://github.com/akarras/ultros/commit/{git_hash}")
                    class="side-nav-version"
                    title=t_string!(i18n, version).to_string()
                >
                    {git_hash}
                </a>
            </div>
```

Only the `<AccountMenu />` line is new — the utility strip is unchanged and is reproduced here so the edit is unambiguous.

- [ ] **Step 3: Run CI checks**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

Expected: `REAL_EXIT=0`.

- [ ] **Step 4: Manual verification**

1. Signed out: the row reads "Sign in". Clicking opens a panel upward with Login / Settings / Language / Theme.
2. Expand Language — the list expands inline; picking a locale switches the UI and collapses the accordion.
3. Signed in: the row shows avatar + username; the panel adds Profile, Settings and Log out.
4. `Escape` closes the panel. Clicking elsewhere on the page closes it. Navigating closes it.
5. Collapse the sidebar (chevron, ≥1024px): the row becomes avatar-only and the panel breaks out to ~15rem overhanging the content.
6. The top bar's own language/theme/user controls still work — duplication is expected at this stage.

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/side_nav.rs
git commit -m "feat(nav): mount account drop-up in the sidebar footer"
```

---

# Stage 3 — Sidebar reorder (additive)

---

### Task 7: Promote Search and Item Explorer above Tools

Search and Explorer are both "find me an item" surfaces — Search jumps to a known item, Explorer browses when you don't know what you want. Pairing them above the divider leaves `Tools` an honest list of nine analyzers rather than nine analyzers plus a database.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/side_nav.rs:50-97`
- Modify: `style/tailwind.css`

**Interfaces:**
- Consumes: `use_search_overlay_state()` (Task 2); i18n key `search` (Task 1)

- [ ] **Step 1: Add the import and state handle**

In `side_nav.rs` add:

```rust
use crate::global_state::search_overlay::use_search_overlay_state;
```

and inside `SideNav`, after `let nav = use_side_nav_settings();`:

```rust
    let search_overlay = use_search_overlay_state();
```

- [ ] **Step 2: Insert the Search + Explorer pair at the top of the nav**

At the start of the `<nav class="side-nav-sections">` block, **before** the existing `<A href="/" exact=true …>` Home link, insert:

```rust
                <button
                    class="side-nav-item side-nav-item-hero"
                    on:click=move |_| search_overlay.toggle()
                >
                    <Icon icon=i::AiSearchOutlined />
                    <span class="side-nav-label">{t!(i18n, search)}</span>
                    <span class="side-nav-kbd">"⌘K"</span>
                </button>
                <A href="/items" attr:class="side-nav-item side-nav-item-hero">
                    <Icon icon=i::MdiJellyfish />
                    <span class="side-nav-label">{t!(i18n, item_explorer)}</span>
                </A>

                <div class="side-nav-rule"></div>
```

- [ ] **Step 3: Remove the old Item Explorer entry from Tools**

Delete this block from the `Tools` section (currently `side_nav.rs:90-93`):

```rust
                <A href="/items" attr:class="side-nav-item">
                    <Icon icon=i::MdiJellyfish />
                    <span class="side-nav-label">{t!(i18n, item_explorer)}</span>
                </A>
```

`Currency Exchange` stays in `Tools`. After this, `Tools` contains exactly nine entries: Flip Finder, Vendor Resale, Recipe Analyzer, FC Crafting, Leve Analyzer, Market Trends, Scrip Sources, Venture Analyzer, Currency Exchange.

- [ ] **Step 4: Add the supporting CSS**

Append to `style/tailwind.css`:

```css
.side-nav-item-hero {
    background-color: color-mix(in srgb, var(--brand-ring) 10%, transparent);
    border: 1px solid var(--color-outline);
    width: 100%;
}
.side-nav-kbd {
    margin-left: auto;
    font-size: 0.62rem;
    font-family: ui-monospace, monospace;
    color: var(--color-text-muted);
    border: 1px solid var(--color-outline);
    border-radius: 0.25rem;
    padding: 0.05rem 0.25rem;
}
.side-nav-rule {
    height: 1px;
    background-color: var(--color-outline);
    margin: 0.5rem 0.5rem 0.25rem;
}

@media (min-width: 1024px) {
    .app-shell-collapsed .side-nav-kbd {
        display: none;
    }
}
```

- [ ] **Step 5: Run CI checks**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

Expected: `REAL_EXIT=0`.

- [ ] **Step 6: Manual verification**

1. Sidebar order reads: Search, Item Explorer, rule, Home, TOOLS (9), SAVED, HELP.
2. Clicking the Search row opens the overlay.
3. Item Explorer navigates to `/items` and shows the active highlight there.
4. Item Explorer appears exactly once — not still in Tools.
5. Collapsed sidebar: the `⌘K` badge hides and both hero rows stay icon-centred.

- [ ] **Step 7: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/side_nav.rs style/tailwind.css
git commit -m "feat(nav): promote search and item explorer above tools"
```

---

# Stage 4 — The flip (atomic)

Everything in this stage lands in one commit. Adding the mobile bar before removing the top bar would leave phones with chrome at both edges; removing the top bar before adding the mobile bar would leave phones with no way to open the drawer. Neither half ships alone.

---

### Task 8: Mobile bottom bar component

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/mobile_bar.rs`
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs`

**Interfaces:**
- Consumes: `use_side_nav_settings()`, `use_search_overlay_state()` (Task 2); i18n keys `menu`, `search` (Task 1), `items` (existing)
- Produces: `#[component] pub fn MobileBar() -> impl IntoView`

- [ ] **Step 1: Create the component**

Create `ultros-frontend/ultros-app/src/components/mobile_bar.rs`:

```rust
//! Fixed bottom bar, below 1024px only.
//!
//! Three slots — Menu, Search, Items. Buttons only, never a text input:
//! focusing an input inside a `position: fixed; bottom: 0` element leaves
//! it behind the iOS virtual keyboard, because Safari shrinks the visual
//! viewport without shrinking the layout viewport. Search therefore opens
//! the overlay, which anchors its input to the top of the sheet.
//!
//! Account is deliberately absent — it lives in the sidebar footer
//! drop-up, reached through the Menu slot. This is an accepted trade: the
//! old top bar showed a persistent sign-in button on phones.

use crate::components::icon::Icon;
use crate::global_state::search_overlay::use_search_overlay_state;
use crate::global_state::side_nav::use_side_nav_settings;
use crate::i18n::{t, t_string, use_i18n};
use icondata as i;
use leptos::prelude::*;
use leptos_router::components::A;

#[component]
pub fn MobileBar() -> impl IntoView {
    let i18n = use_i18n();
    let nav = use_side_nav_settings();
    let search_overlay = use_search_overlay_state();

    view! {
        <nav class="mobile-bar" aria-label=t_string!(i18n, side_nav_aria_primary)>
            <button
                class="mobile-bar-slot"
                aria-label=t_string!(i18n, side_nav_toggle_navigation).to_string()
                aria-expanded=move || if nav.drawer_open.get() { "true" } else { "false" }
                on:click=move |_| nav.drawer_open.update(|v| *v = !*v)
            >
                <Icon icon=i::AiMenuOutlined width="1.4em" height="1.4em" />
                <span class="mobile-bar-label">{t!(i18n, menu)}</span>
            </button>

            <button
                class="mobile-bar-slot"
                aria-label=t_string!(i18n, search).to_string()
                on:click=move |_| search_overlay.toggle()
            >
                <Icon icon=i::AiSearchOutlined width="1.4em" height="1.4em" />
                <span class="mobile-bar-label">{t!(i18n, search)}</span>
            </button>

            <A href="/items" attr:class="mobile-bar-slot">
                <Icon icon=i::MdiJellyfish width="1.4em" height="1.4em" />
                <span class="mobile-bar-label">{t!(i18n, items)}</span>
            </A>
        </nav>
    }
    .into_any()
}
```

- [ ] **Step 2: Register the module**

In `components/mod.rs`:

```rust
pub mod mobile_bar;
```

Do not commit yet — this task's commit is combined with Task 9.

---

### Task 9: Remove the top bar, mount the mobile bar, delete dead code

**Files:**
- Delete: `ultros-frontend/ultros-app/src/components/top_bar.rs`
- Delete: `ultros-frontend/ultros-app/src/components/apps_menu.rs`
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs`
- Modify: `ultros-frontend/ultros-app/src/components/app_shell.rs`
- Modify: `ultros-frontend/ultros-app/src/components/language_picker.rs` (delete `LanguageNavMenu`)
- Modify: `style/tailwind.css`

**Interfaces:**
- Consumes: `MobileBar` (Task 8)

`apps_menu.rs` goes in its entirety: `AppsMenu` has no callers anywhere in the workspace (the only mention of the name outside its own definition is a comment in `side_nav.rs:21`), and `UserMenu` is superseded by `AccountMenu` from Task 5.

- [ ] **Step 1: Confirm the deletions are safe**

```bash
cd ultros-frontend/ultros-app
grep -rn "AppsMenu\|UserMenu\|LanguageNavMenu\|TopBar\|top_bar" src/ | grep -v "src/components/apps_menu.rs\|src/components/top_bar.rs"
```

Expected: only `src/components/mod.rs` (module declarations), `src/components/app_shell.rs` (the `TopBar` import and usage), `src/components/language_picker.rs` (the `LanguageNavMenu` definition), and the stale comment at `src/components/side_nav.rs:21`. If anything else appears, stop and rewire that caller first.

- [ ] **Step 2: Delete the files**

```bash
git rm ultros-frontend/ultros-app/src/components/top_bar.rs
git rm ultros-frontend/ultros-app/src/components/apps_menu.rs
```

- [ ] **Step 3: Update the module list**

In `components/mod.rs`, remove:

```rust
pub mod top_bar;
pub mod apps_menu;
```

- [ ] **Step 4: Delete `LanguageNavMenu`**

In `language_picker.rs`, delete the whole `#[component] pub fn LanguageNavMenu()` function (it starts at the `/// ...`-free `#[component]` immediately above `pub fn LanguageNavMenu`, currently line 127, and runs to the end of its `.into_any() }`).

Then remove imports that only it used. After deletion, check each of these and drop any the file no longer references:

```rust
use cfg_if::cfg_if;
use leptos::html;
#[cfg(feature = "hydrate")]
use leptos_use::use_element_hover;
```

`LANGUAGE_OPTIONS`, `reload_locale_data`, `LanguagePicker` and `LanguageAccordion` all stay.

- [ ] **Step 5: Fix the stale comment in `side_nav.rs`**

Line 21 currently reads:

```rust
    // Build world-aware URLs the same way `AppsMenu` does.
```

Replace with:

```rust
    // Build world-aware URLs from the current home world, falling back to
    // the world-less route when none is set.
```

- [ ] **Step 6: Update the shell**

In `app_shell.rs`, remove the `TopBar` import and add `MobileBar`:

```rust
use crate::components::mobile_bar::MobileBar;
```

Delete the `<TopBar />` line from the `view!`, and add `<MobileBar />` after `<main>`:

```rust
            <main class="app-shell-content" role="main">
                {children()}
            </main>

            <MobileBar />

            <div class="app-shell-ad-rail">
                <DesktopAdRail />
            </div>

            <SearchOverlay />
```

- [ ] **Step 7: Rewrite the shell grid**

In `style/tailwind.css`, replace the `.app-shell` rule (currently lines 1624-1633) with:

```css
.app-shell {
    display: grid;
    grid-template-columns: 240px minmax(0, 1fr);
    grid-template-areas: "side  main";
    min-height: 100dvh;
    width: 100%;
}
```

Delete the `.app-shell > .top-bar` rule (lines 1644-1646).

In the mobile block (`@media (max-width: 1023px)`), replace the grid override:

```css
    .app-shell {
        grid-template-columns: minmax(0, 1fr);
        grid-template-areas: "main";
    }
```

and add, inside that same media block:

```css
    /* Clear the fixed bottom bar so it never covers the last table row. */
    .app-shell > .app-shell-content {
        padding-bottom: calc(4.25rem + env(safe-area-inset-bottom));
    }
```

In both wide-viewport blocks (`@media (min-width: 1536px)` and `@media (min-width: 1660px)`), replace the two-row `grid-template-areas` with the single-row form:

```css
        grid-template-areas: "side  main  ads";
```

- [ ] **Step 8: Delete the topbar CSS and add the mobile bar CSS**

Delete the entire `/* ----- Topbar component styles ----- */` block from `style/tailwind.css` — the `@utility top-bar` rule and the `.top-bar-hamburger`, `.top-bar-search`, `.top-bar-actions` rules and their media query (currently lines 1864-1917).

Append:

```css
/* ----- Mobile bottom bar (below 1024px only) ----- */
.mobile-bar {
    display: none;
}

@media (max-width: 1023px) {
    .mobile-bar {
        position: fixed;
        left: 0;
        right: 0;
        bottom: 0;
        z-index: 45;
        display: flex;
        border-top: 1px solid var(--color-outline);
        background-color: color-mix(in srgb, var(--color-background) 92%, transparent);
        backdrop-filter: blur(20px);
        /* env() keeps the bar clear of the iPhone home indicator. */
        padding: 0.3rem 0.15rem calc(0.3rem + env(safe-area-inset-bottom));
    }
    .mobile-bar-slot {
        flex: 1;
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 0.15rem;
        padding: 0.35rem 0;
        border-radius: 0.5rem;
        color: var(--color-text-muted);
        background: transparent;
        border: none;
        text-decoration: none;
    }
    .mobile-bar-slot[aria-current="page"] {
        color: var(--color-text);
        background-color: color-mix(in srgb, var(--brand-ring) 16%, transparent);
    }
    .mobile-bar-label {
        font-size: 0.6rem;
        line-height: 1;
    }
}
```

- [ ] **Step 9: Run CI checks**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

Expected: `REAL_EXIT=0`. Compile errors here will point at any remaining reference to the deleted components — fix the caller, never re-add the file.

- [ ] **Step 10: Manual verification — this is the stage that can regress silently**

Check all four widths:

| Width | Expect |
|---|---|
| ≥1536px | Sidebar, content, ad rail. No top bar. No mobile bar. |
| 1024–1535px | Sidebar + content, no ad rail. No bars. |
| 1023px | Sidebar hidden as drawer; mobile bar visible at the bottom. |
| 375px | Mobile bar with three slots; labels not truncated. |

Then:
1. ☰ opens the drawer; the drawer's footer account row and its drop-up work.
2. 🔍 opens the overlay full-screen with the input at the top; the on-screen keyboard does not cover it.
3. 🪼 navigates to `/items` and shows the active highlight.
4. Scroll a long table to the bottom — the last row is fully visible above the bar.
5. Collapse and re-expand the desktop sidebar; the setting still survives a reload (cookie unaffected).
6. Switch to German — the sidebar, account panel and bar labels all still fit.
7. Signed out and signed in, confirm the account panel differs correctly.
8. Confirm no horizontal scrollbar appears at any width.

- [ ] **Step 11: Commit**

```bash
git add -A ultros-frontend/ultros-app/src/components/ style/tailwind.css
git commit -m "feat(nav)!: remove the top bar, add the mobile bottom bar

The sidebar now owns all global chrome on desktop. Below 1024px a
three-slot fixed bar carries Menu, Search and Items; account moves into
the drawer footer drop-up.

Deletes top_bar.rs, apps_menu.rs (AppsMenu was already unreferenced) and
LanguageNavMenu."
```

---

## Post-implementation

- [ ] Re-read the spec's "Risks and accepted trade-offs" section and confirm each still holds as built.
- [ ] Confirm `body` still has no `overflow-x` declaration in `style/tailwind.css`.
- [ ] Open a PR noting that CI does not run `cargo test`, so the manual verification matrix above is the real gate.
