# Item Explorer Revamp Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the page-local sidebar and mobile hamburger on `/items/*` with a top-of-content two-row Toolbar (group pills + subcategory chip strip), so the page stops competing with the global `AppShell` sidebar/TopBar.

**Architecture:** A new sibling module `routes/item_explorer_toolbar.rs` hosts pure data-shaping helpers (testable via the existing `xiv_gen_db` pattern) and a single `ItemExplorerToolbar` Leptos component. `ItemExplorer` in `routes/item_explorer.rs` shrinks to `<ItemExplorerToolbar/><Outlet/>` inside a thin wrapper div, and the now-dead helpers (`SideMenuButton`, `CategoryView`, `JobsList`, `CategorySection`) plus the entire `<aside>`/mobile-header/drawer block are deleted.

**Tech Stack:** Rust, Leptos 0.7-ish reactive view macros, `leptos_router` `<A>` / `query_signal` / `use_params_map`, the existing `toolbar::{Toolbar, ToolbarPills}` primitives, `xiv_gen` static data, `icondata`, Tailwind v4 (`@utility` utilities in `style/tailwind.css`).

**Spec:** [`docs/superpowers/specs/2026-05-12-item-explorer-revamp-design.md`](../specs/2026-05-12-item-explorer-revamp-design.md)

---

## Prerequisites

This worktree is currently behind `origin/main`. The implementation must run against the post-AppShell main, which contains `components/app_shell.rs`, `components/side_nav.rs`, `components/top_bar.rs`, and `components/toolbar.rs`. Before starting:

- [ ] **Verify the worktree is current.** Run:

  ```bash
  git fetch origin main
  git rev-parse HEAD
  git rev-parse origin/main
  ```

  If they differ, ask the user how they want to sync. Do not silently rebase or force-update — the worktree branch may carry intentional divergence. Suggest: `git merge origin/main` from this branch, or rebase onto `origin/main`. Wait for an answer before proceeding.

- [ ] **Confirm submodules are initialized.** The build script for `xiv-gen-db` reads from nested submodules under `xiv-gen/ffxiv-datamining/`. Run:

  ```bash
  git submodule status --recursive | head -20
  ```

  If `cn/`, `ko/`, `tc/` etc. show as missing, run `git submodule update --init --recursive --depth=1`. This may take a few minutes the first time.

---

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `ultros-frontend/ultros-app/src/routes/item_explorer_toolbar.rs` | **Create** | Pure helpers (`active_group_from_params`, `category_chips_for_group`, `job_chips_sorted`), the `ItemExplorerToolbar` view component, and unit tests for the helpers. |
| `ultros-frontend/ultros-app/src/routes/mod.rs` | **Modify** | Add `pub mod item_explorer_toolbar;` next to the existing `pub mod item_explorer;`. |
| `ultros-frontend/ultros-app/src/routes/item_explorer.rs` | **Modify** | Slim `ItemExplorer` down to `<ItemExplorerToolbar/><Outlet/>`. Delete `SideMenuButton`, `CategoryView`, `JobsList`, `CategorySection`. Keep everything else (`CategoryItems`, `JobItems`, `DefaultItems`, `ItemList`, `Items`, sort enums, `APersistQuery`, `job_category_lookup`, the `tests` mod). |
| `style/tailwind.css` | **Modify** | Append `.item-explorer-chip-row` and `.item-explorer-chip` rules near the existing `.toolbar*` block (≈line 1820). |
| `ultros-frontend/ultros-app/src/lib.rs` | **No change** | `ItemExplorer` is still exported via `use crate::routes::item_explorer::*` and the routes don't change. |

`active_category_group` and `is_open` (the `Memo` from inside the old `ItemExplorer`) are replaced by `active_group_from_params` (pure function) and a local `selected_group: RwSignal<u8>` initialized from the active group, both inside the toolbar module.

---

## Task 1: Pure helpers and unit tests

Build the data-shaping helpers first. They have stable inputs (`xiv_gen` static data + a `ParamsMap`) and stable outputs (small `Vec`s of references), which is exactly the shape the existing `test_job_filtering` already validates. Following the codebase's test style, we add tests in a `#[cfg(test)] mod tests` block in the new file.

**Files:**
- Create: `ultros-frontend/ultros-app/src/routes/item_explorer_toolbar.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/mod.rs`

- [ ] **Step 1: Register the new module**

Open `ultros-frontend/ultros-app/src/routes/mod.rs` and add `pub mod item_explorer_toolbar;` immediately after `pub mod item_explorer;`. The file's modules are alphabetised, so it sorts naturally between `item_explorer` and `item_view`:

```rust
pub mod item_explorer;
pub mod item_explorer_toolbar;
pub mod item_view;
```

- [ ] **Step 2: Create the file scaffold with failing tests**

Create `ultros-frontend/ultros-app/src/routes/item_explorer_toolbar.rs` with helper signatures (returning empty/wrong values) and the test module. We want `cargo test -p ultros-app item_explorer_toolbar` to fail before we implement.

```rust
//! Toolbar for `/items/*`: group pill selector over a subcategory chip
//! strip. Replaces the page-local sidebar that pre-dated the AppShell.

use xiv_gen::{ClassJob, ItemSearchCategoryId};

/// Resolve the active top-level category group (1=Weapons, 2=Armor,
/// 3=Items, 4=Housing, 5=Job Sets) from route params. Both args come
/// directly from `ParamsMap::get(...).as_deref()` at the call site, so
/// this helper has no router dependency and is trivial to unit-test.
pub(crate) fn active_group_from_route(
    jobset: Option<&str>,
    category: Option<&str>,
) -> Option<u8> {
    let _ = (jobset, category);
    None
}

/// Return the search categories that belong to a non-job group
/// (1..=4), sorted by `cat.order`. Each entry is
/// `(display_name, ItemSearchCategoryId)`. Group 5 returns empty —
/// jobs use `job_chips_sorted` instead.
pub(crate) fn category_chips_for_group(
    group: u8,
) -> Vec<(&'static str, ItemSearchCategoryId)> {
    let _ = group;
    Vec::new()
}

/// Return the visible class jobs sorted by `ui_priority`. Mirrors the
/// filter used by `routes::item_explorer::JobsList` and the existing
/// `test_job_filtering` test: only jobs with `job_index > 0` or
/// `doh_dol_job_index >= 0`, and with a non-empty abbreviation or name.
pub(crate) fn job_chips_sorted() -> Vec<&'static ClassJob> {
    Vec::new()
}

/// Segment label shown on a job chip: prefer the abbreviation, fall
/// back to the full name. Matches the path-segment logic that
/// `routes::item_explorer::JobsList` uses for the `href`.
pub(crate) fn job_chip_label(job: &ClassJob) -> &str {
    if job.abbreviation.is_empty() {
        job.name.as_str()
    } else {
        job.abbreviation.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_group_is_none_on_bare_items_route() {
        assert_eq!(active_group_from_route(None, None), None);
    }

    #[test]
    fn active_group_for_jobset_route_is_five() {
        assert_eq!(active_group_from_route(Some("PLD"), None), Some(5));
    }

    #[test]
    fn active_group_for_weapon_category_is_one() {
        // "Pugilist's Arm" is a weapon (category = 1) in the xiv data.
        // Percent-encoded as it would arrive from the router.
        assert_eq!(
            active_group_from_route(None, Some("Pugilist%27s%20Arm")),
            Some(1),
        );
    }

    #[test]
    fn active_group_for_unknown_category_is_none() {
        assert_eq!(
            active_group_from_route(None, Some("Not%20A%20Real%20Category")),
            None,
        );
    }

    #[test]
    fn jobset_wins_over_category_when_both_present() {
        // Defensive — if the router ever produces both, Job Sets takes
        // precedence (matches the original `active_category_group` order).
        assert_eq!(
            active_group_from_route(Some("PLD"), Some("Sword")),
            Some(5),
        );
    }

    #[test]
    fn weapon_chips_are_sorted_by_order_and_non_empty() {
        let chips = category_chips_for_group(1);
        assert!(!chips.is_empty(), "weapons group must have chips");

        // Re-fetch the source-of-truth order from xiv data to assert sort.
        let data = xiv_gen_db::data();
        let mut expected: Vec<_> = data
            .item_search_categorys
            .iter()
            .filter(|(_, c)| c.category == 1)
            .map(|(id, c)| (c.order, c.name.as_str(), *id))
            .collect();
        expected.sort_by_key(|(order, _, _)| *order);
        let expected_names: Vec<&str> =
            expected.iter().map(|(_, name, _)| *name).collect();
        let actual_names: Vec<&str> = chips.iter().map(|(name, _)| *name).collect();
        assert_eq!(actual_names, expected_names);
    }

    #[test]
    fn job_sets_group_returns_no_category_chips() {
        // Group 5 is rendered as job chips, not category chips.
        assert!(category_chips_for_group(5).is_empty());
    }

    #[test]
    fn job_chips_contain_samurai_and_carpenter_but_not_marauder() {
        let chips = job_chips_sorted();
        let names: Vec<&str> = chips.iter().map(|j| j.name.as_str()).collect();
        assert!(names.contains(&"samurai"), "samurai should be in job chips");
        assert!(names.contains(&"carpenter"), "carpenter should be in job chips");
        assert!(!names.contains(&"marauder"), "marauder must not be in job chips");
    }

    #[test]
    fn job_chip_label_prefers_abbreviation() {
        let data = xiv_gen_db::data();
        let pld = data
            .class_jobs
            .iter()
            .find(|(_, j)| j.name == "paladin")
            .map(|(_, j)| j)
            .expect("paladin job must exist");
        assert_eq!(job_chip_label(pld), pld.abbreviation.as_str());
    }
}
```

- [ ] **Step 3: Run the tests; confirm they fail**

```bash
cargo test -p ultros-app --lib item_explorer_toolbar -- --nocapture
```

Expected: all 9 tests fail with assertion errors (e.g. "weapons group must have chips" panics because we return empty; the `Some(5)` test fails because we return `None`). The compile must succeed — if it doesn't, fix the imports until it does before moving on.

- [ ] **Step 4: Implement `active_group_from_route`**

Replace the body in `item_explorer_toolbar.rs`. The logic mirrors the deleted `active_category_group` `Memo` (originally at [item_explorer.rs:752-769 on origin/main](../../ultros-frontend/ultros-app/src/routes/item_explorer.rs:752)):

```rust
pub(crate) fn active_group_from_route(
    jobset: Option<&str>,
    category: Option<&str>,
) -> Option<u8> {
    if jobset.is_some() {
        return Some(5);
    }
    let cat_raw = category?;
    let cat_name = percent_encoding::percent_decode_str(cat_raw)
        .decode_utf8()
        .ok()?;
    let data = xiv_gen_db::data();
    data.item_search_categorys
        .values()
        .find(|cat| cat.name == cat_name)
        .map(|cat| cat.category)
}
```

`percent_encoding` is already a workspace dep used by `item_explorer.rs` — no `Cargo.toml` change needed.

- [ ] **Step 5: Implement `category_chips_for_group` and `job_chips_sorted`**

```rust
pub(crate) fn category_chips_for_group(
    group: u8,
) -> Vec<(&'static str, ItemSearchCategoryId)> {
    if group == 5 || group == 0 {
        return Vec::new();
    }
    let data = xiv_gen_db::data();
    let mut rows: Vec<(u8, &'static str, ItemSearchCategoryId)> = data
        .item_search_categorys
        .iter()
        .filter(|(_, cat)| cat.category == group)
        .map(|(id, cat)| (cat.order, cat.name.as_str(), *id))
        .collect();
    rows.sort_by_key(|(order, _, _)| *order);
    rows.into_iter().map(|(_, name, id)| (name, id)).collect()
}

pub(crate) fn job_chips_sorted() -> Vec<&'static ClassJob> {
    let data = xiv_gen_db::data();
    let mut jobs: Vec<&'static ClassJob> = data
        .class_jobs
        .iter()
        .filter(|(_, job)| job.job_index > 0 || job.doh_dol_job_index >= 0)
        .filter(|(_, job)| !job.abbreviation.is_empty() || !job.name.is_empty())
        .map(|(_, job)| job)
        .collect();
    jobs.sort_by_key(|job| job.ui_priority);
    jobs
}
```

The `cat.order` field is `u8` in `xiv_gen` and the sort is non-negative; this matches the original `CategoryView` ordering.

- [ ] **Step 6: Run the tests; confirm they pass**

```bash
cargo test -p ultros-app --lib item_explorer_toolbar -- --nocapture
```

Expected: 9 tests pass. If `active_group_for_weapon_category_is_one` fails, check whether `Pugilist's Arm` exists in the current `xiv_gen` data — if not, swap the assertion to another known weapon category name found by greping `xiv-gen/ffxiv-datamining/csv/ItemSearchCategory.csv` for a row with `Category=1`. The point of the test is to assert that a real category resolves to group 1.

- [ ] **Step 7: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/item_explorer_toolbar.rs ultros-frontend/ultros-app/src/routes/mod.rs
git commit -m "item-explorer: extract pure helpers for chip toolbar

Pre-cursor to replacing the page-local sidebar. Helpers cover route
-> active-group resolution and the chip-row data for both category
groups (Weapons/Armor/Items/Housing) and Job Sets. Unit tests pin
the same filtering invariants the old test_job_filtering covered."
```

---

## Task 2: Render `ItemExplorerToolbar`

Add the Leptos view to the new module. This task is rendering-only — the data shaping was proven correct in Task 1. We use the existing `Toolbar` and `ToolbarPills` primitives for Row 1 and plain Tailwind for the Row 2 chip strip (no new primitive needed).

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/item_explorer_toolbar.rs`

- [ ] **Step 1: Add imports and the component skeleton**

At the top of `item_explorer_toolbar.rs`, replace the import block with:

```rust
use crate::components::icon::Icon;
use crate::components::item_icon::{ClassJobIcon, ItemSearchCategoryIcon};
use crate::components::toolbar::{Toolbar, ToolbarPills};
use crate::i18n::{t, t_string, use_i18n};
use icondata as i;
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;
use xiv_gen::{ClassJob, ItemSearchCategoryId};
```

Drop the stale `Item, ItemSearchCategory` imports from the scaffold — only `ClassJob` and `ItemSearchCategoryId` are still referenced by the helper signatures.

- [ ] **Step 2: Add the component**

Append below the helpers (above `#[cfg(test)] mod tests`):

```rust
#[component]
pub fn ItemExplorerToolbar() -> impl IntoView {
    let i18n = use_i18n();
    let params = use_params_map();

    let active_group = Memo::new(move |_| {
        let p = params();
        active_group_from_route(p.get("jobset").as_deref(), p.get("category").as_deref())
    });

    // Default selection: whatever the route says, else Weapons (1).
    let selected_group = RwSignal::new(active_group.get_untracked().unwrap_or(1));

    // When the route changes (e.g. browser back), follow it.
    Effect::new(move |_| {
        if let Some(group) = active_group.get() {
            selected_group.set(group);
        }
    });

    let pill = move |group: u8, label_view: AnyView| {
        view! {
            <button
                aria-pressed=move || (selected_group.get() == group).to_string()
                on:click=move |_| selected_group.set(group)
            >
                {label_view}
            </button>
        }
    };

    view! {
        <div class="flex flex-col gap-3 mb-4">
            <Toolbar>
                <ToolbarPills>
                    {pill(1, view! { {t!(i18n, item_explorer_weapons)} }.into_any())}
                    {pill(2, view! { {t!(i18n, item_explorer_armor)} }.into_any())}
                    {pill(3, view! { {t!(i18n, item_explorer_items)} }.into_any())}
                    {pill(4, view! { {t!(i18n, item_explorer_housing)} }.into_any())}
                    {pill(5, view! { {t!(i18n, item_explorer_job_sets)} }.into_any())}
                </ToolbarPills>
            </Toolbar>

            <div
                class="item-explorer-chip-row"
                role="navigation"
                aria-label=t_string!(i18n, item_explorer_categories).to_string()
            >
                {move || {
                    let group = selected_group.get();
                    if group == 5 {
                        job_chips_sorted()
                            .into_iter()
                            .map(|job| {
                                let label = job_chip_label(job).to_string();
                                let href = format!(
                                    "/items/jobset/{}",
                                    label.replace('/', "%2F")
                                );
                                let job_id = job.key_id;
                                view! {
                                    <A href=href attr:class="item-explorer-chip">
                                        <ClassJobIcon id=job_id />
                                        <span>{label}</span>
                                    </A>
                                }.into_any()
                            })
                            .collect::<Vec<_>>()
                    } else {
                        category_chips_for_group(group)
                            .into_iter()
                            .map(|(name, id)| {
                                let href = format!(
                                    "/items/category/{}",
                                    name.replace('/', "%2F")
                                );
                                view! {
                                    <A href=href attr:class="item-explorer-chip">
                                        <ItemSearchCategoryIcon id=id />
                                        <span>{name}</span>
                                    </A>
                                }.into_any()
                            })
                            .collect::<Vec<_>>()
                    }
                }}
            </div>
        </div>
    }
    .into_any()
}
```

Notes for the implementer:

- The `pill` closure captures `selected_group` and emits one `<button>` for the `ToolbarPills` group; this matches the analyzer pattern at [analyzer.rs:593-606 on origin/main](../../ultros-frontend/ultros-app/src/routes/analyzer.rs:593).
- `Icon` is imported but currently unused — keep it imported if you want to add a right-chevron affordance later; otherwise drop the line and rerun fmt/clippy. Clippy will flag it under `unused_imports`. Remove it now if you don't add a chevron icon.
- `t!(i18n, ...)` is the Leptos i18n reactive macro used throughout the project (`use crate::i18n::*;` is the pattern; see [item_explorer.rs:14 on origin/main](../../ultros-frontend/ultros-app/src/routes/item_explorer.rs:14)).
- `ItemSearchCategoryIcon` and `ClassJobIcon` are the existing icon components from `components/item_icon.rs` and accept the same `id` props they accept inside `CategoryView` / `JobsList`.

- [ ] **Step 3: Compile-check**

```bash
cargo check -p ultros-app
```

Expected: clean compile. Fix import errors, missing macros, or trait bounds before continuing. Common pitfall: `t!` macro requires `use_i18n()` to be called once at the top of the component — already done.

If you used `Icon` only in the snippet above and didn't actually emit it, delete `use crate::components::icon::Icon;` and `use icondata as i;` to silence the unused-import warning.

- [ ] **Step 4: Run the helper tests again**

```bash
cargo test -p ultros-app --lib item_explorer_toolbar -- --nocapture
```

Expected: still 9 passing. No new test logic; just confirming the rewrite didn't regress the helpers.

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/item_explorer_toolbar.rs
git commit -m "item-explorer: add ItemExplorerToolbar view component

Two-row toolbar — group pills (Weapons/Armor/Items/Housing/Job Sets)
over a horizontal chip strip of subcategories for the active group.
Reuses Toolbar/ToolbarPills primitives from the AppShell redesign.
Not yet wired into ItemExplorer; that swap is the next task."
```

---

## Task 3: Stylesheet for the chip row

The chip row needs a Tailwind-v4 utility plus a chip button style. Both are co-located with the existing `.toolbar*` utilities in `style/tailwind.css`.

**Files:**
- Modify: `style/tailwind.css`

- [ ] **Step 1: Locate the toolbar block**

Open `style/tailwind.css`. Find the line:

```css
.toolbar-spacer {
    flex: 1;
}
```

(Around line 1821-1823 on origin/main.) The new rules go immediately after this block.

- [ ] **Step 2: Add the chip row utilities**

Insert:

```css
/* ----- Item Explorer chip strip (consumed by ItemExplorerToolbar) ----- */
@utility item-explorer-chip-row {
    display: flex;
    flex-wrap: nowrap;
    gap: 0.5rem;
    overflow-x: auto;
    padding: 0.25rem 0.25rem 0.5rem 0.25rem;
    scrollbar-width: thin;
    -webkit-mask-image: linear-gradient(
        to right,
        black 0,
        black calc(100% - 2rem),
        transparent 100%
    );
    mask-image: linear-gradient(
        to right,
        black 0,
        black calc(100% - 2rem),
        transparent 100%
    );
}

@utility item-explorer-chip {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.35rem 0.7rem;
    border: 1px solid var(--color-outline);
    border-radius: 9999px;
    background: transparent;
    color: var(--color-text-muted);
    font-size: 0.85rem;
    white-space: nowrap;
    transition: color 150ms ease, background-color 150ms ease, border-color 150ms ease;
}
.item-explorer-chip:hover {
    color: var(--color-text);
    background-color: color-mix(in srgb, var(--brand-ring) 8%, transparent);
}
.item-explorer-chip[aria-current="page"] {
    color: var(--color-text);
    border-color: var(--brand-ring);
    background-color: color-mix(in srgb, var(--brand-ring) 18%, transparent);
}
```

The gradient mask hints at horizontal overflow without occupying layout space. The `aria-current="page"` selector lights up the active chip — `leptos_router`'s `<A>` sets this automatically when the link's `href` matches the current path.

- [ ] **Step 3: Verify CSS builds**

The frontend uses `cargo-leptos` (Tailwind runs as part of the build). The cheapest local check is:

```bash
cargo check -p ultros-app
```

This won't compile Tailwind, but it confirms no Rust files reference a CSS class that doesn't exist (they don't — CSS classes are strings on Rust's side). For the actual Tailwind build, the next task's smoke test exercises it.

- [ ] **Step 4: Commit**

```bash
git add style/tailwind.css
git commit -m "item-explorer: add chip-row + chip utilities for the new toolbar

@utility classes co-located with the existing toolbar primitive.
Active-chip highlight hangs off aria-current=page set by the router."
```

---

## Task 4: Swap `ItemExplorer` and delete the old sidebar

This is the load-bearing change: replace the body of `ItemExplorer` and delete every chunk of code that becomes unreachable. Done as one task because the deletes only compile after the new body lands.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/item_explorer.rs`

- [ ] **Step 1: Replace the `ItemExplorer` component**

In [routes/item_explorer.rs:744-870 on origin/main](../../ultros-frontend/ultros-app/src/routes/item_explorer.rs:744), delete the entire `#[component] pub fn ItemExplorer()` body and replace it with:

```rust
#[component]
pub fn ItemExplorer() -> impl IntoView {
    view! {
        <div class="flex flex-col min-h-[calc(100vh-64px)]">
            <div class="p-4 lg:p-8 max-w-[1600px] mx-auto w-full">
                <crate::routes::item_explorer_toolbar::ItemExplorerToolbar />
                <Outlet />
            </div>
        </div>
    }
    .into_any()
}
```

The 64px subtract in `min-h-[calc(...)]` matches the old wrapper's footprint; tweak if the visual height drifts (the AppShell topbar is 56px, so technically `calc(100vh-56px)` is more correct — change to that if you notice a stray scroll).

- [ ] **Step 2: Delete the now-dead helpers**

In the same file, delete these definitions:

- `SideMenuButton` (component at the top of the file, ~lines 30-49 on origin/main). It was only used inside `CategoryView` and `JobsList`.
- `CategoryView` component (~lines 51-83).
- `JobsList` component (~lines 195-222).
- `CategorySection` component (~lines 720-742).

The `tests` module at the bottom stays. `job_category_lookup` stays — it's still used by `JobItems`. `CategoryItems`, `JobItems`, `DefaultItems`, the sort enums, `APersistQuery`, `ItemList`, and `Items` all stay.

- [ ] **Step 3: Prune unused imports**

After the deletes, the following imports at the top of `item_explorer.rs` may now be unused. Run clippy to see; if so, remove them:

- `use crate::components::query_button::QueryButton;` — used by `ItemList`, keep it. Check by grep.
- `use leptos_router::hooks::query_signal;` — used by `JobItems` and `ItemList` still, keep it.
- `use leptos_router::components::A;` — used by `ItemList`. Keep it.
- The `Memo`, `Signal`, etc. uses inside `use leptos::prelude::*;` are wildcard — unaffected.

The actual unused imports to remove (verify via clippy in Step 5):
- `use icondata as i;` — `BiChevronDownRegular`, `BiMenuRegular`, `BiXRegular` were only used in the deleted sidebar. `BiChevronRightRegular` is still used in `ItemList`'s next-page button. Keep `icondata` import.
- `t_string!` may go unused depending on what `CategoryItems`/`JobItems` retain. Verify.

Don't guess — let clippy be the source of truth.

- [ ] **Step 4: Run formatter and clippy**

```bash
./check_ci.sh
```

This runs `cargo fmt --all -- --check` and `cargo clippy --all-targets -- -D warnings`. Expected: clean. Common failures and fixes:

- "unused import" → delete the named import.
- "function `foo` is never used" → that function (likely a previously-internal helper) lost its only caller. Either delete it or, if it's still referenced, check the imports.
- Formatting diffs → run `cargo fmt --all` to autofix.

If the submodule isn't initialized, clippy will panic during `xiv-gen-db` build. Run `git submodule update --init --recursive --depth=1` once and rerun.

- [ ] **Step 5: Run the existing test suite**

```bash
cargo test -p ultros-app --lib
```

Expected: all tests pass, including the relocated `item_explorer_toolbar::tests::*` (8 tests) and the unchanged `routes::item_explorer::tests::test_job_filtering`.

- [ ] **Step 6: Manual smoke (browser)**

The project provides `./scripts/run_e2e.sh` but a quicker check is just running the app locally. From the repo root:

```bash
cargo leptos watch
```

Wait for it to boot, then in a browser:

1. Visit `http://localhost:3000/items` — Weapons pill is pressed, weapon-category chips fill Row 2.
2. Click the "Armor" pill — chip row swaps to armor categories; URL does **not** change.
3. Click a chip — URL becomes `/items/category/<name>`, the chip shows `aria-current` highlight, the item grid renders.
4. Click "Job Sets" pill, then PLD chip — URL becomes `/items/jobset/PLD`, items render.
5. Resize the browser to ~375px wide — the toolbar collapses cleanly; only the global `TopBar` hamburger is visible; no horizontal page scroll; the chip row scrolls horizontally inside the toolbar.
6. Use browser Back — pills follow route, chip selection follows route.

If any step fails, fix and re-run `./check_ci.sh` before committing.

- [ ] **Step 7: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/item_explorer.rs
git commit -m "item-explorer: replace sidebar+hamburger with chip toolbar

ItemExplorer is now a thin wrapper around ItemExplorerToolbar and the
nested route Outlet. The lg:hidden mobile header, the <aside>, the
backdrop, the ?menu-open query toggle, and SideMenuButton/CategoryView
/JobsList/CategorySection are all deleted. The only hamburger visible
on /items/* is the global TopBar one; the desktop content area reclaims
~280px from the deleted aside.

Spec: docs/superpowers/specs/2026-05-12-item-explorer-revamp-design.md"
```

---

## Self-review checklist (run after Task 4, before declaring done)

- [ ] `cargo fmt --all -- --check` passes.
- [ ] `cargo clippy --all-targets -- -D warnings` passes.
- [ ] `cargo test -p ultros-app --lib` passes, including the 9 `item_explorer_toolbar::tests::*` cases and `routes::item_explorer::tests::test_job_filtering`.
- [ ] `git grep "menu-open" ultros-frontend/` returns no hits in source — the deleted query param is gone everywhere.
- [ ] `git grep "SideMenuButton\|CategoryView\|JobsList\|CategorySection" ultros-frontend/` returns no hits in source — all deleted symbols are gone.
- [ ] In a running app, `/items` shows exactly one hamburger button (the `TopBar` one) at all viewport widths.
- [ ] On desktop ≥1024px, no element with `class*="aside"` or width `280px` appears inside the item-explorer route.

If any check fails, fix inline and rerun.

---

## Risks and rollback

- **Single commit per task** means a `git revert` of Task 4's commit restores the old `ItemExplorer` body without losing the toolbar module or its tests — useful if a regression shows up after merge.
- **Job chip overflow.** If you discover that the Job Sets row visually breaks (e.g. icons stacking instead of scrolling), audit the `flex-wrap: nowrap` rule and the `ClassJobIcon` size — the icon might need a fixed `width` to honor `flex-nowrap`. This is a CSS tweak inside `.item-explorer-chip`, not a code change.
- **i18n key drift.** All six `item_explorer_*` group/label keys (`weapons`, `armor`, `items`, `housing`, `job_sets`, `categories`) are confirmed present in `locales/en.json` on `origin/main`. If a translator removes one before this lands, the `t!` macro will fail to compile — fall back to `t_string!` with a literal or restore the key.
