# Lists Page Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix datacenter exclusion so it applies to the whole list view, reorganize the filters into one unified row, convert the auto-mark-purchases accordion into a toolbar button + modal, and cover it all with unit, insta-snapshot, and Puppeteer tests.

**Architecture:** All UI work is in `ultros-frontend/ultros-app` (Leptos SSR/hydrate app). Filtering is centralized in `ListView` (route component) with one pure `filter_excluded` function. The filter UI moves to a new `components/list/filter_row.rs` with context-free props so it can be SSR-snapshot-tested. `AutoMarkPurchases` keeps its subscription logic but renders as a toolbar button + `Modal` (Portal-based, no clipping concerns).

**Tech Stack:** Rust, Leptos 0.8 (pinned 0.8.20), leptos-i18n, insta (new dev-dep), Puppeteer harness in `integration/`.

## Global Constraints

- Run `./check_ci.sh` (fmt + clippy `-D warnings`) before every commit; use `bash -c './check_ci.sh > /tmp/ci.log 2>&1; echo REAL_EXIT=$?'` to read the exit code.
- Every new user-facing string needs a key in **all 7** locale files (`en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`) with real translations.
- `.list-toolbar` class must remain on the toolbar row div (Puppeteer hook).
- Leptos tests: wrap reactive code in `Owner::new()` + `owner.with(...)`, init executor via `any_spawner::Executor::init_futures_executor()` (ignore repeat-init error) — see `i18n_fallback.rs` tests.
- No `#[allow]` to silence clippy without a justifying comment.
- `cargo test -p ultros-app` runs with default features (ssr) on Windows; the `ultros` bin tests won't link on Windows — never run workspace-wide `cargo test`.

---

### Task 1: Unified exclusion filter (the bug fix)

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/list_view.rs` (fn `filter_excluded_worlds` at ~line 57, call site at ~line 1192, tests module at end)

**Interfaces:**
- Produces: `fn filter_excluded(items: &[(ListItem, Vec<ActiveListing>)], excluded_worlds: &HashSet<i32>, excluded_datacenters: &HashSet<String>, world_helper: Option<&WorldHelper>) -> Vec<(ListItem, Vec<ActiveListing>)>` in `list_view.rs`. Uses the existing `crate::components::listing_filters::filter_active_listings` per item (already handles world + DC + `None` world-data fallback).

- [ ] **Step 1: Write failing tests** in `list_view.rs` `#[cfg(test)] mod tests`. Reuse the `WorldHelper` fixture shape from `components/listing_filters.rs` tests (Aether/id 10/world 100 Adamantoise, Primal/id 11/world 110 Behemoth):

```rust
fn world_helper() -> ultros_api_types::world_helper::WorldHelper {
    // same WorldData literal as components/listing_filters.rs tests
}

#[test]
fn filter_excluded_removes_datacenter_listings_from_every_item() { /* DC-only set drops world-110 listings, keeps items */ }
#[test]
fn filter_excluded_applies_worlds_and_datacenters_together() { /* world 100 excluded + DC Primal excluded -> empty listings, items kept */ }
#[test]
fn filter_excluded_with_empty_sets_is_identity() { }
#[test]
fn filter_excluded_without_world_data_still_applies_world_exclusions() { /* helper: None */ }
```

- [ ] **Step 2:** `cargo test -p ultros-app filter_excluded` → FAIL (function not defined).
- [ ] **Step 3:** Implement `filter_excluded` (delegating per-item to `filter_active_listings`), delete `filter_excluded_worlds` and its two old tests (behavior covered by the new identity/world tests).
- [ ] **Step 4:** Wire into `ListView`: the render closure computes `let world_helper = use_context::<LocalWorldData>()...` once at component setup; call site becomes `filter_excluded(&item_snapshot, &excluded_worlds.get(), &excluded_datacenters.get(), helper)`. Reading `excluded_datacenters.get()` inside the closure makes the table rebuild on DC toggles (it already does for worlds).
- [ ] **Step 5:** `cargo test -p ultros-app` → PASS.
- [ ] **Step 6:** `./check_ci.sh`, then commit `fix(lists): apply datacenter exclusions to the whole list view`.

---

### Task 2: Unified filter row component

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/list/filter_row.rs`
- Modify: `ultros-frontend/ultros-app/src/components/list/mod.rs` (add `pub mod filter_row;`)
- Modify: `ultros-frontend/ultros-app/src/routes/list_view.rs` (delete `WorldExclusionControl`, `SortSpec`; replace the filters `<div class="panel...">` block at ~lines 844-955 with `<ListFilterRow .../>`)
- Modify: `ultros-frontend/ultros-app/Cargo.toml` (add `insta = "1"` to `[dev-dependencies]`)
- Modify: all 7 `ultros-frontend/ultros-app/locales/*.json` (new key `list_view_exclusions_label`)

**Interfaces:**
- Produces (in `filter_row.rs`, `pub(crate)`):
  - `struct SortSpec { key: SortKey, descending: bool }` + `enum SortKey` + `FromStr`/`Display` — moved verbatim from `list_view.rs` (with their tests).
  - `fn worlds_in_listings(items: &[(ListItem, Vec<ActiveListing>)], helper: Option<&WorldHelper>) -> Vec<(i32, String)>` — sorted by world id, names falling back to `format!("World {id}")`.
  - `#[component] fn ListFilterRow(worlds: Vec<(i32, String)>, datacenters: Vec<String>, excluded_worlds: Signal<HashSet<i32>>, set_excluded_worlds: Callback<HashSet<i32>>, excluded_datacenters: Signal<HashSet<String>>, set_excluded_datacenters: Callback<HashSet<String>>, sort_spec: Signal<Option<SortSpec>>, set_sort_spec: Callback<Option<SortSpec>>, hide_acquired: Signal<bool>, set_hide_acquired: Callback<bool>) -> impl IntoView`
- Consumes: nothing from Task 1; i18n via `use_i18n()` only (no router, no LocalWorldData — that's what makes it snapshot-testable).

Layout inside one `panel` row (`flex flex-wrap items-center gap-x-6 gap-y-3`):
1. **Exclusions group** — label `{t!(i18n, list_view_exclusions_label)}`; DC chips (current `btn-secondary` + red excluded styling, from the old inline block); then the world `<select id="list-world-exclusion">` + excluded-world chips + Clear button (markup from old `WorldExclusionControl`, minus its own panel border/background).
2. **Sort group** — existing label + `<select id="list-sort-select">` markup.
3. **View group** — existing hide-acquired toggle button.

- [ ] **Step 1: Write failing tests** in `filter_row.rs`:

```rust
#[test]
fn worlds_in_listings_dedupes_and_names() { /* two items sharing world 100 -> one ("Adamantoise") entry; unknown id -> "World 999" */ }

#[test]
fn filter_row_renders_all_groups() {
    let _ = any_spawner::Executor::init_futures_executor();
    let owner = Owner::new();
    owner.with(|| {
        provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
        let html = view! { <ListFilterRow
            worlds=vec![(100, "Adamantoise".into())]
            datacenters=vec!["Aether".into(), "Primal".into()]
            excluded_worlds=Signal::derive(HashSet::new)
            set_excluded_worlds=Callback::new(|_| {})
            excluded_datacenters=Signal::derive(|| HashSet::from(["Primal".to_string()]))
            set_excluded_datacenters=Callback::new(|_| {})
            sort_spec=Signal::derive(|| None)
            set_sort_spec=Callback::new(|_| {})
            hide_acquired=Signal::derive(|| false)
            set_hide_acquired=Callback::new(|_| {})
        /> }.to_html();
        insta::assert_snapshot!(html);
    });
}
```

Plus the moved `sort_spec_round_trips_through_query_param_encoding` test.

- [ ] **Step 2:** `cargo test -p ultros-app filter_row` → FAIL (module missing).
- [ ] **Step 3:** Implement `filter_row.rs`; add `insta` dev-dep; add `list_view_exclusions_label` ("Exclude" / fr "Exclure" / de "Ausschließen" / ja "除外" / cn "排除" / ko "제외" / tc "排除") to all 7 locales.
- [ ] **Step 4:** `cargo test -p ultros-app filter_row` → snapshot created on first run; review with `cargo insta review` or accept via `INSTA_UPDATE=always` then eyeball the `.snap` file; re-run → PASS.
- [ ] **Step 5:** Wire into `list_view.rs`: compute `worlds_in_listings` + datacenter names (existing `helper.get_datacenters(&wdr_filter)` logic) inside the existing `list_view.get()`-driven closure; replace the whole old filters panel; delete `WorldExclusionControl` and the old inline DC/sort/hide markup; keep `list_view_exclude_worlds`/`list_view_exclude_datacenters` keys as the subgroup aria-labels.
- [ ] **Step 6:** `cargo test -p ultros-app` → PASS. `./check_ci.sh` → commit `refactor(lists): unified filter row with snapshot coverage`.

---

### Task 3: Auto-mark purchases → toolbar button + modal

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/list/auto_mark_purchases.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/list_view.rs` (move `<AutoMarkPurchases list_view=list_view />` from page top into the toolbar's right-side button group, before Subscribe)

**Interfaces:**
- `AutoMarkPurchases(list_view)` signature unchanged. New internal `#[component] fn AutoMarkModalContent(watch_character_name: RwSignal<String>, is_watching: Signal<bool>, can_write: Signal<bool>, on_toggle: Callback<()>) -> impl IntoView` (pub(crate) for tests).
- `apply_purchase_to_list`, `mark_item_purchased`, the realtime Effect, and all four existing tests stay untouched.

Component renders:
- A `sticky-bar-button sticky-bar-button-shrink` button (icon `BiPurchaseTagSolid`, label `{t!(i18n, list_auto_mark_title)}` in a `sticky-bar-button-label` span, `data-testid="list-auto-mark-btn"`), with `class:bg-brand-900`/`class:border-brand-500` bound to `is_watching` — the running watcher stays visible when the modal is closed.
- `<Show when=modal_open><Modal set_visible=set_modal_open>` containing `AutoMarkModalContent`: h2 title + Experimental badge, `{t!(i18n, list_auto_mark_description)}` paragraph (key already exists, currently unused), the character-name input and start/stop button (existing markup).

- [ ] **Step 1: Write failing snapshot test** in `auto_mark_purchases.rs` (same Owner + i18n harness as Task 2's):

```rust
#[test]
fn auto_mark_modal_content_renders() { /* insta::assert_snapshot! of AutoMarkModalContent with is_watching=false, can_write=true */ }
```

- [ ] **Step 2:** `cargo test -p ultros-app auto_mark` → FAIL (component missing).
- [ ] **Step 3:** Restructure the component as above; delete the `<details>` accordion markup.
- [ ] **Step 4:** Move the component invocation in `list_view.rs` into the toolbar right group. Page order becomes: toolbar → filter row → table.
- [ ] **Step 5:** `cargo test -p ultros-app` → PASS (snapshot accepted after review). `./check_ci.sh` → commit `refactor(lists): auto-mark purchases as toolbar button + modal`.

---

### Task 4: Puppeteer coverage

**Files:**
- Modify: `integration/list-flow.cjs` (new steps after the recipe step, before mark-acquired)

**Interfaces:**
- Consumes: `data-testid="list-auto-mark-btn"` (Task 3), DC chip buttons inside the filter row (Task 2), `excluded-datacenters` query param (existing).

- [ ] **Step 1:** Add step "auto-mark modal opens": click `[data-testid="list-auto-mark-btn"]` via `page.evaluate` click, `waitForFunction` for `document.querySelector('[role="dialog"]')` whose innerText includes "Auto-mark Purchases", assert the Character Name placeholder input exists, press Escape, wait for dialog to close.
- [ ] **Step 2:** Add step "datacenter exclusion round-trips": click the first DC chip via `clickByText` scoped to the filter row, `waitForFunction` for `location.search.includes("excluded-datacenters=")`, assert the price column's body text changed to "No listing data" for the test world's rows (the test list is single-world, so excluding its DC empties every price cell), click the chip again, wait for the param to clear.
- [ ] **Step 3:** Run the suite: `./scripts/run_e2e.sh` (or against an existing server with `BASE_URL`). Expected: `[ok] list flow passed` with the new `+` lines.
- [ ] **Step 4:** Commit `test(e2e): cover auto-mark modal and datacenter exclusion in list flow`.

---

### Task 5: Final verification

- [ ] **Step 1:** `cargo test -p ultros-app` and `cargo test -p ultros-api-types` → all PASS.
- [ ] **Step 2:** `./check_ci.sh` clean (`REAL_EXIT=0`).
- [ ] **Step 3:** Visual check of the redesigned page via the e2e screenshot run or local serve; confirm toolbar → filter row → table order, modal opens centered, DC chip excludes visibly.
- [ ] **Step 4:** Update `CHANGELOG` route entry if the repo convention expects one (check `changelog` route for the 2026-08-29 pattern) — append entry for the lists redesign.
- [ ] **Step 5:** Final commit + push branch `claude/lists-page-redesign-tests-060e5b`, open PR to `main`.
