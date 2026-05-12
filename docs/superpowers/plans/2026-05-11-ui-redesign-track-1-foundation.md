# UI Redesign Track 1 — Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the new application shell (sidebar + slim topbar + fluid content) without changing any individual route's body. After this lands, every existing route renders inside the new layout, and Tracks 2–5 (home redesign, toolbar migration, item view, item explorer) can proceed in parallel.

**Architecture:** Three new Leptos components — `AppShell` (CSS-grid layout owner), `SideNav` (persistent left rail with collapse + mobile drawer), `TopBar` (slim row with breadcrumb + search + lang/theme/user) — plus a `Toolbar` filter primitive that Track 3 will consume. Sidebar collapse state lives in a `SideNavSettings` context mirroring the existing `ThemeSettings` pattern (RwSignal + cookie + localStorage). `lib.rs::AppInner` swaps `NavRow + app-route-shell` for `AppShell { Routes }`. CSS adds `@utility app-shell`/`side-nav`/`top-bar`/`toolbar` blocks and replaces `.app-route-shell`'s `max-w-7xl` cap with a sidebar-aware grid that retains the existing 1536px / 1660px ad-rail breakpoints.

**Tech Stack:** Rust + Leptos 0.7 (SSR + hydrate), Tailwind v4 (with `@utility` and `@theme` blocks in [style/tailwind.css](../../style/tailwind.css)), `cookie` crate via the existing [`Cookies`](../../ultros-frontend/ultros-app/src/global_state/cookies.rs) abstraction, `leptos_router` `<A>` / `<Routes>`, `leptos_meta`, `icondata`. Reference spec: [docs/superpowers/specs/2026-05-11-ui-redesign-v2-design.md](../specs/2026-05-11-ui-redesign-v2-design.md).

---

## Conventions

- **Compile-as-test.** Leptos UI components without business logic can't be unit-tested in this codebase. For those tasks, the validation step is `cargo clippy --all-targets -- -D warnings` (catches type, lifetime, and prop errors that `cargo check` misses) plus `cargo fmt --all -- --check`. We use TDD where logic is testable (route-to-section mapper, settings enum parsing); we use compile + Puppeteer for the rest.
- **One commit per task.** Each task ends with a `git commit`. Conventional-commit prefixes: `feat:`, `refactor:`, `style:`, `test:`.
- **Each task leaves the app buildable.** New components are added before their integration point; the integration step (Task 9 → `lib.rs`) is the last code change before the E2E smoke.
- **`./check_ci.sh` before every commit.** The user's [CLAUDE.md](../../CLAUDE.md) makes this non-negotiable. The plan does not repeat it as a step inside every task; it's an implicit precondition for the commit step. Run it explicitly when a task is large or you suspect breakage.
- **Submodule init.** If `./check_ci.sh` fails with a build-script panic about `cn/Item.csv`, run `git submodule update --init --recursive --depth=1`. See [CLAUDE.md](../../CLAUDE.md) for context.

---

## File map

**Created:**

| File | Responsibility |
|---|---|
| `ultros-frontend/ultros-app/src/global_state/side_nav.rs` | `SideNavSettings` context (collapse signal + drawer signal + cookie persistence) |
| `ultros-frontend/ultros-app/src/components/app_shell.rs` | Grid layout: sidebar + topbar + content + ad rail. Mobile drawer behavior. |
| `ultros-frontend/ultros-app/src/components/side_nav.rs` | Sidebar UI: brand, sections, items, collapse toggle |
| `ultros-frontend/ultros-app/src/components/top_bar.rs` | Topbar UI + `breadcrumb_for_path` helper (with unit tests) |
| `ultros-frontend/ultros-app/src/components/toolbar.rs` | `Toolbar`, `ToolbarField`, `ToolbarPills` primitives (no callers yet) |

**Modified:**

| File | Change |
|---|---|
| `ultros-frontend/ultros-app/src/global_state/mod.rs` | Add `pub mod side_nav;` |
| `ultros-frontend/ultros-app/src/components/mod.rs` | Add four new `pub mod` lines |
| `ultros-frontend/ultros-app/src/lib.rs` | `AppInner` adopts `AppShell`; remove `NavRow` (or leave dead and remove in Task 11) |
| `style/tailwind.css` | New `@utility` blocks; rewrite `.app-route-shell` block to support sidebar grid |

**Untouched (intentional):**

- All routes in `ultros-frontend/ultros-app/src/routes/` — they render inside the new shell with no edits.
- `components/apps_menu.rs` — kept compiled for back-compat; no longer rendered after Task 9. Track 6 cleanup deletes it.
- `components/filter_card.rs` — Track 3 migrates callers; Track 6 deletes the component.

---

## Task 1: SideNavSettings — collapse + drawer state, with persistence

**Files:**
- Create: `ultros-frontend/ultros-app/src/global_state/side_nav.rs`
- Modify: `ultros-frontend/ultros-app/src/global_state/mod.rs`

- [ ] **Step 1: Write the failing unit tests for the bool round-trip**

This module exposes a `SideNavSettings` context whose `collapsed: RwSignal<bool>` is persisted to a cookie named `side_nav_collapsed`. The cookie value is the literal `"true"` / `"false"`. Add a unit test module at the bottom of the file.

Create `ultros-frontend/ultros-app/src/global_state/side_nav.rs` with only this content first:

```rust
//! Sidebar collapse + mobile drawer state.
//!
//! Mirrors the `ThemeSettings` pattern in [`super::theme`]: an RwSignal
//! persisted to a cookie via the [`Cookies`](super::cookies::Cookies) helper.

#[cfg(test)]
mod tests {
    #[test]
    fn parse_collapsed_true() {
        assert_eq!(super::parse_collapsed("true"), Some(true));
    }

    #[test]
    fn parse_collapsed_false() {
        assert_eq!(super::parse_collapsed("false"), Some(false));
    }

    #[test]
    fn parse_collapsed_garbage() {
        assert_eq!(super::parse_collapsed("yes"), None);
        assert_eq!(super::parse_collapsed(""), None);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p ultros-app --lib global_state::side_nav -- --nocapture
```

Expected: compile error — `parse_collapsed` is undefined.

- [ ] **Step 3: Add the parser and the rest of the module**

Append (above the `#[cfg(test)] mod tests` block):

```rust
use leptos::prelude::*;

use crate::global_state::cookies::Cookies;

const COOKIE_NAME: &str = "side_nav_collapsed";

fn parse_collapsed(s: &str) -> Option<bool> {
    match s {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Sidebar state. `collapsed` is desktop icon-only; `drawer_open` is the mobile overlay.
#[derive(Clone, Copy)]
pub struct SideNavSettings {
    pub collapsed: RwSignal<bool>,
    pub drawer_open: RwSignal<bool>,
}

impl SideNavSettings {
    fn new() -> Self {
        let initial_collapsed = load_collapsed_from_cookie().unwrap_or(false);
        let collapsed = RwSignal::new(initial_collapsed);
        let drawer_open = RwSignal::new(false);

        let settings = SideNavSettings { collapsed, drawer_open };

        // Persist collapse changes to cookie.
        Effect::new(move |_| {
            let value = collapsed.get();
            if let Some(cookies) = use_context::<Cookies>() {
                let (_, set_cookie) =
                    cookies.use_cookie_typed::<_, String>(COOKIE_NAME);
                set_cookie(Some(if value { "true".into() } else { "false".into() }));
            }
        });

        settings
    }
}

fn load_collapsed_from_cookie() -> Option<bool> {
    let cookies = use_context::<Cookies>()?;
    let (sig, _setter) = cookies.use_cookie_typed::<_, String>(COOKIE_NAME);
    let value = sig.get_untracked()?;
    parse_collapsed(&value)
}

/// Provide `SideNavSettings` into context if not already present, and return it.
pub fn provide_side_nav_settings() -> SideNavSettings {
    if let Some(existing) = use_context::<SideNavSettings>() {
        return existing;
    }
    let settings = SideNavSettings::new();
    provide_context(settings);
    settings
}

/// Retrieve `SideNavSettings` from context. Panics if not provided.
pub fn use_side_nav_settings() -> SideNavSettings {
    use_context::<SideNavSettings>().expect("SideNavSettings not provided")
}
```

- [ ] **Step 4: Register the module**

Read `ultros-frontend/ultros-app/src/global_state/mod.rs` and add `pub mod side_nav;` alongside the existing `pub mod theme;` line. Match the existing pattern exactly — don't reorder unrelated lines.

- [ ] **Step 5: Run the tests to verify they pass**

```bash
cargo test -p ultros-app --lib global_state::side_nav
```

Expected: 3 passed.

- [ ] **Step 6: Run check_ci.sh and commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-app/src/global_state/side_nav.rs \
        ultros-frontend/ultros-app/src/global_state/mod.rs
git commit -m "feat(ui): add SideNavSettings context for sidebar collapse state"
```

---

## Task 2: Route-to-breadcrumb helper, with unit tests

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/top_bar.rs` (stub-only at this task; full component in Task 5)
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs`

The topbar shows the current section as a breadcrumb. Given a path like `/flip-finder/Gilgamesh`, render `"Flip Finder"` as the section and `"Gilgamesh"` as a trailing subtitle. Implement the mapping as a pure function so it's unit-testable, then wire it into the TopBar in Task 5.

- [ ] **Step 1: Write the failing unit tests**

Create `ultros-frontend/ultros-app/src/components/top_bar.rs` with only:

```rust
//! Topbar: breadcrumb + search + global controls. The component itself
//! is filled in by Task 5; this file currently exists for the pure
//! `breadcrumb_for_path` helper.

#[derive(Debug, PartialEq, Eq)]
pub struct Breadcrumb {
    pub section: &'static str,
    pub trailing: Option<String>,
}

pub fn breadcrumb_for_path(path: &str) -> Breadcrumb {
    let mut parts = path.trim_start_matches('/').split('/');
    let first = parts.next().unwrap_or("");
    let rest = parts.collect::<Vec<_>>();
    let trailing = if rest.is_empty() {
        None
    } else {
        Some(rest.join(" / "))
    };
    let section = match first {
        "" => "Home",
        "flip-finder" | "analyzer" => "Flip Finder",
        "vendor-resale" => "Vendor Resale",
        "recipe-analyzer" => "Recipe Analyzer",
        "fc-crafting-analyzer" => "FC Crafting",
        "leve-analyzer" => "Leve Analyzer",
        "trends" => "Market Trends",
        "scrip-sources" => "Scrip Sources",
        "venture-analyzer" => "Venture Analyzer",
        "items" => "Item Explorer",
        "item" => "Item",
        "currency-exchange" => "Currency Exchange",
        "list" => "Lists",
        "retainers" => "Retainers",
        "alerts" => "Alerts",
        "settings" => "Settings",
        "profile" => "Profile",
        "history" => "History",
        "welcome" => "Welcome",
        "about" => "About",
        "privacy" | "cookie-policy" => "Legal",
        _ => "Ultros",
    };
    Breadcrumb { section, trailing }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root() {
        let b = breadcrumb_for_path("/");
        assert_eq!(b, Breadcrumb { section: "Home", trailing: None });
    }

    #[test]
    fn flip_finder_with_world() {
        let b = breadcrumb_for_path("/flip-finder/Gilgamesh");
        assert_eq!(b.section, "Flip Finder");
        assert_eq!(b.trailing.as_deref(), Some("Gilgamesh"));
    }

    #[test]
    fn legacy_analyzer_alias_maps_to_flip_finder() {
        let b = breadcrumb_for_path("/analyzer/Goblin");
        assert_eq!(b.section, "Flip Finder");
    }

    #[test]
    fn item_with_world_and_id() {
        let b = breadcrumb_for_path("/item/Gilgamesh/12345");
        assert_eq!(b.section, "Item");
        assert_eq!(b.trailing.as_deref(), Some("Gilgamesh / 12345"));
    }

    #[test]
    fn unknown_route_falls_back() {
        let b = breadcrumb_for_path("/blarghl");
        assert_eq!(b.section, "Ultros");
        assert_eq!(b.trailing, None);
    }

    #[test]
    fn no_leading_slash() {
        let b = breadcrumb_for_path("settings");
        assert_eq!(b.section, "Settings");
    }
}
```

- [ ] **Step 2: Register the module**

In `ultros-frontend/ultros-app/src/components/mod.rs`, add `pub mod top_bar;` in alphabetical position (between `tooltip` and `top_deals`).

- [ ] **Step 3: Run the tests**

```bash
cargo test -p ultros-app --lib components::top_bar
```

Expected: 6 passed.

- [ ] **Step 4: Run check_ci.sh and commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-app/src/components/top_bar.rs \
        ultros-frontend/ultros-app/src/components/mod.rs
git commit -m "feat(ui): add breadcrumb_for_path helper for topbar section labels"
```

---

## Task 3: SideNav component (no collapse yet, no mobile drawer)

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/side_nav.rs`
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs`

Render the sidebar at its full 240px width on desktop. Sections: Brand → Home → TOOLS → SAVED → footer (version + Discord/GitHub icons). No collapse toggle yet, no drawer. The component is rendered standalone (without `AppShell` integration) until Task 6/9.

- [ ] **Step 1: Create the component**

```rust
use crate::components::icon::Icon;
use crate::global_state::home_world::use_home_world;
use crate::i18n::{t, use_i18n};
use git_const::git_short_hash;
use icondata as i;
use leptos::prelude::*;
use leptos_router::components::A;

/// Persistent left sidebar. Brand at top, sections in the middle,
/// utility links + version hash at the bottom.
///
/// Renders at 240px desktop width via the `side-nav` CSS utility
/// (see `style/tailwind.css`). Collapse + mobile drawer behavior
/// is added in later tasks.
#[component]
pub fn SideNav() -> impl IntoView {
    let i18n = use_i18n();
    let (homeworld, _set_homeworld) = use_home_world();

    // Build world-aware URLs the same way `AppsMenu` does.
    let with_world = move |path_with_world: &str, path_no_world: &str| {
        let path_with_world = path_with_world.to_string();
        let path_no_world = path_no_world.to_string();
        Signal::derive(move || match homeworld.get() {
            Some(w) => path_with_world.replace("{world}", &w.name),
            None => path_no_world.clone(),
        })
    };

    let git_hash = git_short_hash!();

    view! {
        <aside class="side-nav" aria-label="Primary">
            <div class="side-nav-brand">
                <A href="/" attr:class="side-nav-brand-link">
                    <Icon icon=i::MdiJellyfish width="1.6em" height="1.6em" />
                    <span class="side-nav-brand-text">"ULTROS"</span>
                </A>
            </div>

            <nav class="side-nav-sections">
                <A href="/" exact=true attr:class="side-nav-item">
                    <Icon icon=i::AiHomeFilled />
                    <span class="side-nav-label">{t!(i18n, home)}</span>
                </A>

                <div class="side-nav-section-header">"TOOLS"</div>

                <A href=with_world("/flip-finder/{world}", "/flip-finder") attr:class="side-nav-item">
                    <Icon icon=i::FaMoneyBillTrendUpSolid />
                    <span class="side-nav-label">{t!(i18n, flip_finder)}</span>
                </A>
                <A href=with_world("/vendor-resale/{world}", "/vendor-resale") attr:class="side-nav-item">
                    <Icon icon=i::FaShopSolid />
                    <span class="side-nav-label">{t!(i18n, vendor_resale)}</span>
                </A>
                <A href=with_world("/recipe-analyzer?world={world}", "/recipe-analyzer") attr:class="side-nav-item">
                    <Icon icon=i::FaHammerSolid />
                    <span class="side-nav-label">{t!(i18n, recipe_analyzer)}</span>
                </A>
                <A href=with_world("/fc-crafting-analyzer/{world}", "/fc-crafting-analyzer") attr:class="side-nav-item">
                    <Icon icon=i::MdiSubmarine />
                    <span class="side-nav-label">"FC Crafting"</span>
                </A>
                <A href=with_world("/leve-analyzer?world={world}", "/leve-analyzer") attr:class="side-nav-item">
                    <Icon icon=i::FaScrollSolid />
                    <span class="side-nav-label">{t!(i18n, leve_analyzer)}</span>
                </A>
                <A href=with_world("/trends/{world}", "/trends") attr:class="side-nav-item">
                    <Icon icon=i::FaChartLineSolid />
                    <span class="side-nav-label">{t!(i18n, market_trends)}</span>
                </A>
                <A href=with_world("/scrip-sources?world={world}", "/scrip-sources") attr:class="side-nav-item">
                    <Icon icon=i::FaCoinsSolid />
                    <span class="side-nav-label">"Scrip Sources"</span>
                </A>
                <A href=with_world("/venture-analyzer?world={world}", "/venture-analyzer") attr:class="side-nav-item">
                    <Icon icon=i::FaBriefcaseSolid />
                    <span class="side-nav-label">"Venture Analyzer"</span>
                </A>
                <A href="/items" attr:class="side-nav-item">
                    <Icon icon=i::MdiJellyfish />
                    <span class="side-nav-label">{t!(i18n, item_explorer)}</span>
                </A>
                <A href="/currency-exchange" attr:class="side-nav-item">
                    <Icon icon=i::BsArrowLeftRight />
                    <span class="side-nav-label">{t!(i18n, currency_exchange)}</span>
                </A>

                <div class="side-nav-section-header">"SAVED"</div>

                <A href="/list" attr:class="side-nav-item">
                    <Icon icon=i::AiOrderedListOutlined />
                    <span class="side-nav-label">{t!(i18n, lists)}</span>
                </A>
                <A href="/retainers/listings" attr:class="side-nav-item">
                    <Icon icon=i::BiGroupSolid />
                    <span class="side-nav-label">{t!(i18n, retainers)}</span>
                </A>
                <A href="/alerts" attr:class="side-nav-item">
                    <Icon icon=i::BsBell />
                    <span class="side-nav-label">"Alerts"</span>
                </A>
            </nav>

            <div class="side-nav-footer">
                <a href="https://discord.gg/pgdq9nGUP2" class="side-nav-icon-link" aria_label="Discord">
                    <Icon icon=i::BsDiscord />
                </a>
                <a href="https://github.com/akarras/ultros" class="side-nav-icon-link" aria_label="GitHub">
                    <Icon icon=i::IoLogoGithub />
                </a>
                <a
                    href=format!("https://github.com/akarras/ultros/commit/{git_hash}")
                    class="side-nav-version"
                    title="Version"
                >
                    {git_hash}
                </a>
            </div>
        </aside>
    }
    .into_any()
}
```

- [ ] **Step 2: Register the module**

In `ultros-frontend/ultros-app/src/components/mod.rs`, add `pub mod side_nav;` in alphabetical position (between `search_box` and `skeleton`).

- [ ] **Step 3: Verify it compiles**

```bash
cargo clippy -p ultros-app --all-targets -- -D warnings
```

Expected: 0 warnings, 0 errors. If `home_world::use_home_world` import path doesn't compile, grep for its current usage in `apps_menu.rs` and match that import exactly.

- [ ] **Step 4: Run check_ci.sh and commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-app/src/components/side_nav.rs \
        ultros-frontend/ultros-app/src/components/mod.rs
git commit -m "feat(ui): add SideNav component (static, no collapse yet)"
```

---

## Task 4: TopBar full component

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/top_bar.rs` (extend the stub from Task 2)

Build out the `TopBar` component: hamburger button (mobile only), breadcrumb (desktop only), `SearchBox`, `LanguagePicker`, `QuickThemeToggle`, `UserMenu`. The breadcrumb reads the current path via `use_location()` and feeds it through `breadcrumb_for_path` from Task 2.

- [ ] **Step 1: Add the component to the existing top_bar.rs**

Read the file (currently contains only the helper + tests) and append:

```rust
use crate::components::apps_menu::UserMenu;
use crate::components::icon::Icon;
use crate::components::language_picker::LanguagePicker;
use crate::components::search_box::SearchBox;
use crate::components::theme_picker::QuickThemeToggle;
use crate::global_state::side_nav::use_side_nav_settings;
use icondata as i;
use leptos::prelude::*;
use leptos_router::hooks::use_location;

/// Slim topbar: hamburger (mobile), breadcrumb (desktop), search, then
/// global controls (language, theme, user). 56px tall.
#[component]
pub fn TopBar() -> impl IntoView {
    let location = use_location();
    let breadcrumb = Signal::derive(move || {
        location.pathname.with(|p| breadcrumb_for_path(p))
    });
    let nav = use_side_nav_settings();

    view! {
        <header class="top-bar" role="banner">
            <button
                class="top-bar-hamburger lg:hidden"
                aria_label="Toggle navigation"
                on:click=move |_| nav.drawer_open.update(|v| *v = !*v)
            >
                <Icon icon=i::AiMenuOutlined width="1.4em" height="1.4em" />
            </button>

            <div class="top-bar-breadcrumb hidden lg:flex">
                <span class="top-bar-section">{move || breadcrumb.get().section}</span>
                {move || breadcrumb.get().trailing.map(|t| view! {
                    <span class="top-bar-divider">"/"</span>
                    <span class="top-bar-trailing">{t}</span>
                })}
            </div>

            <div class="top-bar-search">
                <SearchBox />
            </div>

            <div class="top-bar-actions">
                <div class="hidden md:block">
                    <LanguagePicker />
                </div>
                <div class="hidden md:block">
                    <QuickThemeToggle />
                </div>
                <UserMenu />
            </div>
        </header>
    }
    .into_any()
}
```

- [ ] **Step 2: Verify it compiles**

```bash
cargo clippy -p ultros-app --all-targets -- -D warnings
```

Expected: clean. If `use_location` import path is wrong, the existing usage in routes (`grep "use_location" ultros-frontend/ultros-app/src/`) is authoritative.

- [ ] **Step 3: Run check_ci.sh and commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-app/src/components/top_bar.rs
git commit -m "feat(ui): add TopBar component (breadcrumb + search + controls)"
```

---

## Task 5: AppShell — layout grid + mobile drawer

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/app_shell.rs`
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs`

The shell takes one prop: `children` for the route content. It provides `SideNavSettings` into context, renders `<SideNav>` (desktop persistent + mobile drawer) and `<TopBar>`, and slots the children into the content area. Mobile drawer state lives in `SideNavSettings::drawer_open`; backdrop click and escape key dismiss it. Route changes also dismiss it (handled via an Effect watching the path).

- [ ] **Step 1: Create the component**

```rust
use crate::components::ad::DesktopAdRail;
use crate::components::side_nav::SideNav;
use crate::components::top_bar::TopBar;
use crate::global_state::side_nav::provide_side_nav_settings;
use leptos::prelude::*;
use leptos_router::hooks::use_location;

/// Application shell: persistent sidebar + slim topbar + fluid content
/// + optional ad rail. Mobile collapses the sidebar into a hamburger-
/// toggled overlay drawer.
#[component]
pub fn AppShell(children: Children) -> impl IntoView {
    let nav = provide_side_nav_settings();
    let location = use_location();

    // Dismiss the mobile drawer on any navigation.
    Effect::new(move |_| {
        let _ = location.pathname.get();
        nav.drawer_open.set(false);
    });

    // Escape closes the drawer.
    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Escape" && nav.drawer_open.get_untracked() {
            nav.drawer_open.set(false);
        }
    };

    let drawer_open = nav.drawer_open;
    let collapsed = nav.collapsed;

    let shell_classes = move || {
        let mut classes = String::from("app-shell");
        if collapsed.get() {
            classes.push_str(" app-shell-collapsed");
        }
        if drawer_open.get() {
            classes.push_str(" app-shell-drawer-open");
        }
        classes
    };

    view! {
        <div class=shell_classes on:keydown=on_keydown>
            <SideNav />

            <div
                class="app-shell-backdrop"
                aria-hidden="true"
                on:click=move |_| drawer_open.set(false)
            />

            <TopBar />

            <main class="app-shell-content" role="main">
                {children()}
            </main>

            <aside class="app-shell-ad-rail" aria-label="Sponsored">
                <DesktopAdRail />
            </aside>
        </div>
    }
    .into_any()
}
```

- [ ] **Step 2: Register the module**

In `ultros-frontend/ultros-app/src/components/mod.rs`, add `pub mod app_shell;` in alphabetical position (between `apps_menu` and `cheapest_price`).

Wait — `app_shell` comes before `apps_menu` alphabetically (`p` < `s` after the prefix). Place `app_shell` between `ad` and `apps_menu`.

- [ ] **Step 3: Verify it compiles**

```bash
cargo clippy -p ultros-app --all-targets -- -D warnings
```

Expected: clean. If `DesktopAdRail` is not directly exposed from `components::ad`, grep for its definition (`grep -n "DesktopAdRail" ultros-frontend/ultros-app/src/components/ad.rs`) and import the actual exported name.

- [ ] **Step 4: Run check_ci.sh and commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-app/src/components/app_shell.rs \
        ultros-frontend/ultros-app/src/components/mod.rs
git commit -m "feat(ui): add AppShell layout with sidebar + topbar + drawer"
```

---

## Task 6: CSS — shell layout utilities

**Files:**
- Modify: `style/tailwind.css`

Replace the current `.app-route-shell` block (lines ~1419–1487 of [style/tailwind.css](../../style/tailwind.css)) with a sidebar-aware grid, and add new `@utility` blocks for sidebar, topbar, and toolbar primitives. The existing 1536px / 1660px ad-rail breakpoints are preserved.

- [ ] **Step 1: Locate the current `.app-route-shell` block**

```bash
grep -n "app-route-shell" style/tailwind.css
```

Expected: matches around lines 1419 (`.app-route-shell {`), 1447, 1452, 1459 (the responsive overrides), and 1478. Confirm the block ends at the closing brace before the next CSS rule.

- [ ] **Step 2: Replace the `.app-route-shell` block with the new shell utilities**

Open `style/tailwind.css` and replace everything from `.app-route-shell {` through (and including) the `@media (min-width: 1660px)` block that targets `.app-route-shell` (the last `}` before line ~1488) with:

```css
/* Legacy app-route-shell kept for back-compat with any route body that
   wraps itself in this class. It no longer contributes to the global
   layout — AppShell owns that. */
.app-route-shell {
    display: block;
    width: 100%;
    max-width: 80rem;
    margin-inline: auto;
    padding: 1rem;
}
.app-route-content {
    min-width: 0;
}

/* New application shell — grid that owns sidebar + topbar + content + ad rail. */
.app-shell {
    display: grid;
    grid-template-columns: 240px minmax(0, 1fr);
    grid-template-rows: 56px minmax(0, 1fr);
    grid-template-areas:
        "side  top"
        "side  main";
    min-height: 100vh;
    width: 100%;
}

.app-shell > .side-nav {
    grid-area: side;
}
.app-shell > .top-bar {
    grid-area: top;
}
.app-shell > .app-shell-content {
    grid-area: main;
    min-width: 0;
    padding: 1rem;
}
.app-shell > .app-shell-ad-rail {
    display: none;
}
.app-shell > .app-shell-backdrop {
    display: none;
}

/* Collapsed sidebar: 56px icon-only. */
.app-shell.app-shell-collapsed {
    grid-template-columns: 56px minmax(0, 1fr);
}

/* Mobile (< 1024px): sidebar becomes an overlay drawer. */
@media (max-width: 1023px) {
    .app-shell {
        grid-template-columns: minmax(0, 1fr);
        grid-template-areas:
            "top"
            "main";
    }
    .app-shell > .side-nav {
        position: fixed;
        top: 0;
        bottom: 0;
        left: 0;
        width: 280px;
        z-index: 60;
        transform: translateX(-100%);
        transition: transform 0.2s ease;
    }
    .app-shell.app-shell-drawer-open > .side-nav {
        transform: translateX(0);
    }
    .app-shell.app-shell-drawer-open > .app-shell-backdrop {
        display: block;
        position: fixed;
        inset: 0;
        z-index: 50;
        background-color: rgba(0, 0, 0, 0.5);
    }
}

/* Wide viewport: ad rail appears in the third column. */
@media (min-width: 1536px) {
    .app-shell {
        grid-template-columns: 240px minmax(0, 1fr) 240px;
        grid-template-areas:
            "side  top   top"
            "side  main  ads";
    }
    .app-shell.app-shell-collapsed {
        grid-template-columns: 56px minmax(0, 1fr) 240px;
    }
    .app-shell > .app-shell-ad-rail {
        display: block;
        grid-area: ads;
        min-width: 0;
        padding: 1rem;
    }
    .app-shell > .app-shell-ad-rail:has(.ad.hidden) {
        display: none;
    }
    .app-shell > .app-shell-content {
        padding: 1.5rem;
    }
}

@media (min-width: 1660px) {
    .app-shell {
        grid-template-columns: 240px minmax(0, 1fr) 300px;
        grid-template-areas:
            "side  top   top"
            "side  main  ads";
    }
    .app-shell.app-shell-collapsed {
        grid-template-columns: 56px minmax(0, 1fr) 300px;
    }
}

/* ----- Sidebar component styles ----- */
@utility side-nav {
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--color-outline);
    background-color: var(--color-background-panel);
    overflow-y: auto;
}
.side-nav-brand {
    display: flex;
    align-items: center;
    height: 56px;
    padding: 0 1rem;
    border-bottom: 1px solid var(--color-outline);
}
.side-nav-brand-link {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    color: var(--color-text);
    text-decoration: none;
}
.side-nav-brand-text {
    font-weight: 800;
    letter-spacing: 0.08em;
    font-size: 0.95rem;
}
.side-nav-sections {
    display: flex;
    flex-direction: column;
    gap: 0.125rem;
    padding: 0.5rem;
    flex: 1;
}
.side-nav-section-header {
    font-size: 0.7rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    color: var(--color-text-muted);
    padding: 0.75rem 0.75rem 0.25rem;
}
.side-nav-item {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0.5rem 0.75rem;
    border-radius: 0.5rem;
    color: var(--color-text);
    text-decoration: none;
    font-size: 0.9rem;
    transition: background-color 120ms ease;
}
.side-nav-item:hover {
    background-color: color-mix(in srgb, var(--brand-ring) 14%, transparent);
}
.side-nav-item[aria-current="page"] {
    background-color: color-mix(in srgb, var(--brand-ring) 24%, transparent);
}
.side-nav-item > svg {
    width: 1.1em;
    height: 1.1em;
    flex-shrink: 0;
}
.side-nav-label {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
}
.side-nav-footer {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.75rem;
    border-top: 1px solid var(--color-outline);
}
.side-nav-icon-link {
    color: var(--color-text-muted);
    padding: 0.25rem;
}
.side-nav-icon-link:hover {
    color: var(--color-text);
}
.side-nav-version {
    margin-left: auto;
    font-family: monospace;
    font-size: 0.7rem;
    color: var(--color-text-muted);
    text-decoration: none;
}

/* Collapsed sidebar: hide labels and section headers. */
.app-shell-collapsed .side-nav-label,
.app-shell-collapsed .side-nav-section-header,
.app-shell-collapsed .side-nav-brand-text,
.app-shell-collapsed .side-nav-version {
    display: none;
}
.app-shell-collapsed .side-nav-item {
    justify-content: center;
}
.app-shell-collapsed .side-nav-brand-link {
    justify-content: center;
}

/* ----- Topbar component styles ----- */
@utility top-bar {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    height: 56px;
    padding: 0 1rem;
    border-bottom: 1px solid var(--color-outline);
    background-color: color-mix(in srgb, var(--color-background) 70%, transparent);
    backdrop-filter: blur(20px);
}
.top-bar-hamburger {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    border-radius: 0.5rem;
    color: var(--color-text);
    background: transparent;
    border: 1px solid transparent;
}
.top-bar-hamburger:hover {
    background-color: color-mix(in srgb, var(--brand-ring) 14%, transparent);
}
.top-bar-breadcrumb {
    align-items: baseline;
    gap: 0.5rem;
    min-width: 0;
}
.top-bar-section {
    font-weight: 700;
    color: var(--color-text);
    white-space: nowrap;
}
.top-bar-divider {
    color: var(--color-text-muted);
}
.top-bar-trailing {
    color: var(--color-text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
}
.top-bar-search {
    flex: 1;
    max-width: 480px;
    margin-inline: auto;
    min-width: 0;
}
.top-bar-actions {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-left: auto;
}

/* ----- Toolbar primitive (consumed by Track 3) ----- */
@utility toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: 0.75rem;
    padding: 0.75rem 1rem;
    border: 1px solid var(--color-outline);
    border-radius: 0.5rem;
    background-color: var(--color-background-panel);
}
.toolbar-field {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    min-width: 8rem;
}
.toolbar-field-label {
    font-size: 0.7rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--color-text-muted);
}
.toolbar-pills {
    display: inline-flex;
    border: 1px solid var(--color-outline);
    border-radius: 0.5rem;
    overflow: hidden;
}
.toolbar-pills > button {
    padding: 0.4rem 0.75rem;
    background: transparent;
    color: var(--color-text);
    font-size: 0.85rem;
}
.toolbar-pills > button[aria-pressed="true"] {
    background-color: color-mix(in srgb, var(--brand-ring) 22%, transparent);
}
.toolbar-spacer {
    flex: 1;
}
```

- [ ] **Step 3: Verify the rest of the file is intact**

```bash
grep -c "app-route-shell" style/tailwind.css
```

Expected: at least 1 (the back-compat definition is still there).

```bash
grep -c "@utility app-shell\|@utility side-nav\|@utility top-bar\|@utility toolbar" style/tailwind.css
```

Expected: 4.

- [ ] **Step 4: Run check_ci.sh and commit**

CSS isn't covered by `cargo fmt` or clippy, but build needs to succeed (Tailwind compiles at `cargo leptos build` time). Just commit; the integration test in Task 10 will compile the full app.

```bash
./check_ci.sh
git add style/tailwind.css
git commit -m "style(ui): add app-shell/side-nav/top-bar/toolbar CSS utilities"
```

---

## Task 7: Toolbar primitive component

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/toolbar.rs`
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs`

A composable filter bar. `Toolbar` is a flex container; `ToolbarField` wraps a label + input; `ToolbarPills` wraps a pill-group of buttons. No analyzer migrates to it in this track — Track 3 does that. We just ship the primitive and let it compile.

- [ ] **Step 1: Create the component**

```rust
use leptos::prelude::*;

/// Horizontal filter bar primitive. Use as the top-of-route filter row
/// on analyzer pages; compose with [`ToolbarField`] and [`ToolbarPills`].
#[component]
pub fn Toolbar(children: Children) -> impl IntoView {
    view! {
        <div class="toolbar" role="toolbar">{children()}</div>
    }
    .into_any()
}

/// Labeled column inside a [`Toolbar`]. Renders the label above the slot
/// at small caps; slot is any input/select/control.
#[component]
pub fn ToolbarField(
    #[prop(into)] label: String,
    children: Children,
) -> impl IntoView {
    view! {
        <div class="toolbar-field">
            <span class="toolbar-field-label">{label}</span>
            {children()}
        </div>
    }
    .into_any()
}

/// Segmented pill group. Children should be `<button aria-pressed=...>`
/// elements that the caller controls.
#[component]
pub fn ToolbarPills(children: Children) -> impl IntoView {
    view! {
        <div class="toolbar-pills" role="group">{children()}</div>
    }
    .into_any()
}

/// Flex spacer to push subsequent toolbar children to the right edge.
#[component]
pub fn ToolbarSpacer() -> impl IntoView {
    view! { <div class="toolbar-spacer" /> }.into_any()
}
```

- [ ] **Step 2: Register the module**

In `ultros-frontend/ultros-app/src/components/mod.rs`, add `pub mod toolbar;` in alphabetical position (between `toggle` and `tooltip`).

- [ ] **Step 3: Verify it compiles**

```bash
cargo clippy -p ultros-app --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 4: Run check_ci.sh and commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-app/src/components/toolbar.rs \
        ultros-frontend/ultros-app/src/components/mod.rs
git commit -m "feat(ui): add Toolbar/ToolbarField/ToolbarPills primitives"
```

---

## Task 8: Sidebar collapse toggle UI

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/side_nav.rs`

Add a collapse button to the SideNav brand row. Desktop only — hidden on mobile (mobile uses the topbar hamburger to open/close the drawer, never to "collapse"). Toggling flips `SideNavSettings::collapsed`. The CSS class `app-shell-collapsed` on the parent already hides labels and section headers, so the component code just needs to render the button.

- [ ] **Step 1: Read the current side_nav.rs**

Locate the `.side-nav-brand` `<div>` block.

- [ ] **Step 2: Modify the imports and brand block**

Update the imports at the top of `side_nav.rs` to include:

```rust
use crate::global_state::side_nav::use_side_nav_settings;
```

Replace the existing `.side-nav-brand` block:

```rust
            <div class="side-nav-brand">
                <A href="/" attr:class="side-nav-brand-link">
                    <Icon icon=i::MdiJellyfish width="1.6em" height="1.6em" />
                    <span class="side-nav-brand-text">"ULTROS"</span>
                </A>
            </div>
```

with:

```rust
            <div class="side-nav-brand">
                <A href="/" attr:class="side-nav-brand-link">
                    <Icon icon=i::MdiJellyfish width="1.6em" height="1.6em" />
                    <span class="side-nav-brand-text">"ULTROS"</span>
                </A>
                <button
                    class="side-nav-collapse hidden lg:inline-flex"
                    aria_label="Toggle sidebar"
                    on:click=move |_| nav.collapsed.update(|v| *v = !*v)
                >
                    <Icon icon=i::BiChevronLeftSolid />
                </button>
            </div>
```

And immediately inside the component body (right after the `let i18n = use_i18n();` line), add:

```rust
    let nav = use_side_nav_settings();
```

- [ ] **Step 3: Add CSS for the new button**

Open `style/tailwind.css`. Find the `.side-nav-brand` block added in Task 6, and add immediately after the existing brand rules:

```css
.side-nav-collapse {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: 0.375rem;
    color: var(--color-text-muted);
    background: transparent;
    border: 1px solid transparent;
}
.side-nav-collapse:hover {
    background-color: color-mix(in srgb, var(--brand-ring) 14%, transparent);
    color: var(--color-text);
}
.app-shell-collapsed .side-nav-collapse > svg {
    transform: rotate(180deg);
}
```

- [ ] **Step 4: Verify it compiles**

```bash
cargo clippy -p ultros-app --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 5: Run check_ci.sh and commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-app/src/components/side_nav.rs style/tailwind.css
git commit -m "feat(ui): wire SideNav collapse button to SideNavSettings"
```

---

## Task 9: Wire AppShell into lib.rs

**Files:**
- Modify: `ultros-frontend/ultros-app/src/lib.rs`

Replace `NavRow + app-route-shell` with `AppShell { Routes }`. The existing `<Footer />` at the bottom of `AppInner` stays — the spec says routes can opt out later, but Track 1 keeps it visible for safety.

- [ ] **Step 1: Remove the NavRow render and the route-shell wrapper**

Open `ultros-frontend/ultros-app/src/lib.rs`. Locate the block starting at the comment `// Navigation` inside `AppInner` (around line 304) — it begins with `<Router>` and contains `<NavRow />` and `<main class="flex-1">`.

Replace this entire block:

```rust
            <Router>
                <NavRow />
                // <AnimatedRoutes outro="route-out" intro="route-in" outro_back="route-out-back" intro_back="route-in-back">
                // https://github.com/leptos-rs/leptos/issues/1754
                <main class="flex-1">
                    <div class="app-route-shell">
                        <div class="app-route-content">
                            <Routes fallback=NotFound>
                                // ...all the existing routes...
                            </Routes>
                        </div>
                        <DesktopAdRail />
                    </div>
                </main>
            </Router>
```

with:

```rust
            <Router>
                <AppShell>
                    <Routes fallback=NotFound>
                        // ...all the existing routes (unchanged, copy verbatim)...
                    </Routes>
                </AppShell>
            </Router>
```

**Important:** preserve every `<Route ...>` and `<ParentRoute ...>` child inside `<Routes>` exactly as it was. The only structural change is the wrappers — sidebar/topbar/content/ad rail are now `AppShell`'s job, not inline JSX.

- [ ] **Step 2: Update imports**

In `ultros-frontend/ultros-app/src/lib.rs`, in the `use crate::{ components::{ ... } }` block (around lines 24–28), remove `ad::DesktopAdRail` (now owned by AppShell) and add `app_shell::AppShell`. Also remove the `apps_menu::*` import since `AppShell` provides `UserMenu` via `TopBar`'s internal import — confirm by grepping for other usages of `AppsMenu`/`UserMenu` in `lib.rs`:

```bash
grep -n "AppsMenu\|UserMenu" ultros-frontend/ultros-app/src/lib.rs
```

If `UserMenu` is still referenced in `NavRow` (which we're about to delete), removing the import is safe. If it's referenced elsewhere, keep the import.

- [ ] **Step 3: Delete the `NavRow` function**

The `NavRow` component (around lines 180–248 of `lib.rs`) is no longer rendered. Delete the entire `#[component] pub fn NavRow() -> impl IntoView { ... }` block. If clippy complains about the `use_hotkeys` import being unused after this deletion, remove it from the imports as well.

- [ ] **Step 4: Verify it compiles**

```bash
cargo clippy -p ultros-app --all-targets -- -D warnings
```

Expected: clean. If there are stale imports (`leptos_hotkeys::use_hotkeys`, `apps_menu::*`, etc.), clippy will flag them as unused — remove each one until the warning list is empty.

- [ ] **Step 5: Run check_ci.sh and commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-app/src/lib.rs
git commit -m "refactor(ui): replace NavRow + app-route-shell with AppShell"
```

---

## Task 10: End-to-end smoke test

**Files:**
- None modified (purely verification).

Run the Puppeteer harness against the new shell. It visits every curated route at desktop and mobile breakpoints, screenshots each, asserts on title and body content, and fails on `console.error` or `pageerror`. The screenshots are the de-facto acceptance test for "every route still renders inside the new shell."

- [ ] **Step 1: Initialize submodules if not already**

```bash
git submodule update --init --recursive --depth=1
```

If this is blocked in your sandbox, see [CLAUDE.md](../../CLAUDE.md) for the fallback path.

- [ ] **Step 2: Run the E2E suite**

```bash
./scripts/run_e2e.sh
```

Expected: builds (`cargo leptos build`), spawns a server on a free port, polls `/` ready, runs the Puppeteer suite, prints PASS lines per route at desktop + mobile breakpoints, tears down the server.

If a route fails with `console.error` from a missing-import or hydration mismatch, fix the underlying bug — do NOT silence the assertion. If a route fails on a content assertion because text moved into the sidebar (e.g., a test asserts "Home" in the topbar), update the assertion in `integration/runner.cjs` to match the new shell.

- [ ] **Step 3: Inspect screenshots**

Check `integration/artifacts/` for the screenshots. Sanity-check:
- `/` (Home page) renders inside the shell, no double scrollbars, no overflow
- `/flip-finder` shows the existing analyzer UI inside the shell
- `/item/Gilgamesh/<some-id>` (or the canonical item-view smoke route) renders
- Mobile breakpoint shows the hamburger and the drawer-closed sidebar (transformed offscreen)
- Desktop ≥1536px shows the ad rail in the third column

- [ ] **Step 4: Commit any integration test updates**

If you edited `integration/runner.cjs` or its assertion list:

```bash
git add integration/
git commit -m "test(e2e): update assertions for new app shell"
```

If no changes were needed, skip this step.

---

## Task 11: Cleanup pass — remove dead `apps_menu` topbar callers and stale CSS

**Files:**
- Possibly modify: `ultros-frontend/ultros-app/src/components/apps_menu.rs`
- Possibly modify: `ultros-frontend/ultros-app/src/lib.rs`
- Possibly modify: `style/tailwind.css`

The `AppsMenu` component is no longer rendered (Sidebar absorbed it). `UserMenu` is still rendered by `TopBar`. We don't delete `apps_menu.rs` yet — that's Track 6's cleanup pass once *all* migrations are done — but we should confirm zero compile warnings about unused code.

- [ ] **Step 1: Run clippy and inspect warnings**

```bash
cargo clippy -p ultros-app --all-targets -- -D warnings
```

Expected: clean. If clippy flags `AppsMenu` as dead code, suppress with a single `#[allow(dead_code)]` on the function and leave a one-line comment: `// Kept for Track 6 cleanup; no longer rendered after Track 1.` Don't delete it — Track 6 audits all card-heavy callers in one pass.

- [ ] **Step 2: Verify `.app-route-shell` callers**

Some routes wrap their own contents in `.app-route-shell` or `.app-route-content`. Grep to confirm we're not leaving orphans that double-wrap:

```bash
grep -rn 'app-route-shell\|app-route-content' ultros-frontend/ultros-app/src
```

Expected: zero or only intentional usages (e.g., legal pages that want the constrained reading width). If any analyzer route still wraps itself, that's fine — the back-compat block in `style/tailwind.css` still renders it as a 80rem max-width centered block *inside* the AppShell content cell. No action needed; just confirm none look broken at the screenshots from Task 10.

- [ ] **Step 3: Run check_ci.sh and the E2E one more time**

```bash
./check_ci.sh
./scripts/run_e2e.sh
```

Both must pass.

- [ ] **Step 4: Final commit if there were any cleanup edits**

If you added `#[allow(dead_code)]` or any other touch-ups:

```bash
git add -A
git commit -m "chore(ui): mark Track 1 dead code for Track 6 cleanup pass"
```

Otherwise this task is verification-only — no commit.

---

## Done

After Task 11:

- `cargo clippy --all-targets -- -D warnings` is clean
- `./scripts/run_e2e.sh` passes at desktop + mobile
- The app renders every existing route inside the new shell
- Sidebar collapse persists across refresh (cookie round-trip)
- Mobile drawer opens via hamburger, closes via backdrop / Escape / route change
- Track 2–5 can now proceed in parallel without further shell work

Each track gets its own implementation plan written separately when you're ready to start it.
