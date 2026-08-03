# Item Explorer Subcategory Accordion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the item explorer's subcategory popover *and* its inline scrolling chip strip with one inline accordion that opens when a group pill is clicked and collapses when a subcategory is chosen.

**Architecture:** `components/grouped_nav_popover.rs` is renamed to `grouped_nav_accordion.rs` and converted from an overlay panel to an in-flow accordion whose expansion is a `grid-template-rows: 0fr → 1fr` animation. The `open` state moves out to `ItemExplorerToolbar`, which owns it because a group-pill click must force it open. The toolbar's `group_uses_popover` / `>8 chips` / inline-strip branching collapses to a single code path that builds `Vec<(Option<String>, Vec<NavLink>)>` for every group.

**Tech Stack:** Rust, Leptos 0.8 (SSR + hydrate), `leptos_router`, Tailwind CSS v4 (`@utility` blocks in `style/tailwind.css`), `leptos-i18n`.

**Spec:** `docs/superpowers/specs/2026-07-31-item-explorer-category-accordion-design.md`

## Global Constraints

- **Run `./check_ci.sh` from the repo root before every commit.** It runs `cargo fmt --all -- --check` and `cargo clippy --all-targets -- -D warnings`. Read its exit code explicitly — do not pipe into `tail`/`grep` and read `$?`:
  ```bash
  ./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
  ```
- **`-D warnings` means dead code fails the build.** When a function's last caller is deleted in the same task, the function must be deleted in that task too.
- **CI does not run `cargo test`** for this repo (it is commented out in `rust.yml`). Every task that adds or changes a test must be verified with a **local** `cargo test` run. Green CI only proves it compiles.
- **No hardcoded user-facing strings** in `ultros-frontend/ultros-app/`. This plan introduces **zero new i18n keys** — every label reuses an existing key. Do not add a string literal to any `view!`.
- **`ultros-app` is edition 2024.** `cargo fmt --all` formats it; `cargo fmt` alone may not.
- This worktree needs no setup — game data ships as LFS packs, there are no submodules.
- Set a **short, out-of-repo** `CARGO_TARGET_DIR` before building in this worktree (Windows path-length limits bite otherwise), e.g. `export CARGO_TARGET_DIR=/c/ct/iea`. Use the same value for every command in this plan so the build cache is shared.

---

### Task 1: Extract the duplicated role-bucketing into `role_buckets`

Groups 1 and 5 in `ItemExplorerToolbar` each contain their own near-identical copy of the "seed a bucket per `RoleGroup::ORDERED`, push each link into its role's bucket, drop the empties, label the rest" loop (lines 194-219 and 225-253). Task 2 needs that logic once more, so pull it out first as a standalone refactor with no behaviour change. This lands green on its own and shrinks the diff Task 2 has to carry.

**Why this task and not the `accordion_open_for_route` helper:** that helper has no non-test caller until Task 2 wires the `open` signal, and `cargo clippy --all-targets -- -D warnings` compiles the lib target without `cfg(test)`, so it would fail `dead_code` on its own commit. It therefore lives in Task 2, alongside its caller.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/item_explorer_toolbar.rs`

**Interfaces:**
- Consumes: `RoleGroup::ORDERED` and the existing `PopoverLink` type (renamed to `NavLink` in Task 2).
- Produces: `fn role_buckets(items: impl Iterator<Item = (RoleGroup, PopoverLink)>, role_label: impl Fn(RoleGroup) -> String) -> Vec<(Option<String>, Vec<PopoverLink>)>` — Task 2 renames its element type to `NavLink` and adds a third caller.

- [ ] **Step 1: Write the failing test**

Add to the bottom of the existing `mod tests` block in `ultros-frontend/ultros-app/src/routes/item_explorer_toolbar.rs` (the block starting `#[cfg(test)] mod tests {` at line 301), just before its closing `}`:

```rust
    #[test]
    fn role_buckets_orders_sections_and_drops_empty_roles() {
        let link = |label: &str| PopoverLink {
            label: label.to_string(),
            href: String::new(),
            icon: PopoverIcon::Job(ClassJobId(0)),
        };
        let sections = role_buckets(
            [
                (RoleGroup::Caster, link("BLM")),
                (RoleGroup::Tank, link("PLD")),
                (RoleGroup::Tank, link("WAR")),
            ]
            .into_iter(),
            |role| format!("{role:?}"),
        );
        // Sections follow RoleGroup::ORDERED (Tank before Caster) no matter
        // what order the links arrive in, and the six roles that got no
        // links produce no section header at all.
        assert_eq!(sections.len(), 2, "empty roles must not render a section");
        assert_eq!(sections[0].0.as_deref(), Some("Tank"));
        assert_eq!(
            sections[0]
                .1
                .iter()
                .map(|l| l.label.as_str())
                .collect::<Vec<_>>(),
            vec!["PLD", "WAR"],
            "links keep their input order within a bucket",
        );
        assert_eq!(sections[1].0.as_deref(), Some("Caster"));
    }
```

The test module opens with `use super::*;`. Add a second import line directly under it:

```rust
    use xiv_gen::ClassJobId;
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test -p ultros-app --lib role_buckets_
```

Expected: FAIL to compile, `cannot find function 'role_buckets' in this scope`.

- [ ] **Step 3: Write the implementation**

Insert after `job_chip_label` (which ends at line 82), before `#[component] pub fn ItemExplorerToolbar`:

```rust
/// Bucket `(role, link)` pairs into ordered, labeled sections, dropping the
/// roles that got no links. Both role-grouped tabs — Weapons (via the
/// category's `class_job`) and Job Sets (via the job's abbreviation) — build
/// their sections through this.
fn role_buckets(
    items: impl Iterator<Item = (RoleGroup, PopoverLink)>,
    role_label: impl Fn(RoleGroup) -> String,
) -> Vec<(Option<String>, Vec<PopoverLink>)> {
    let mut buckets: Vec<(RoleGroup, Vec<PopoverLink>)> = RoleGroup::ORDERED
        .iter()
        .map(|role| (*role, Vec::new()))
        .collect();
    for (role, link) in items {
        if let Some((_, links)) = buckets.iter_mut().find(|(r, _)| *r == role) {
            links.push(link);
        }
    }
    buckets
        .into_iter()
        .filter(|(_, links)| !links.is_empty())
        .map(|(role, links)| (Some(role_label(role)), links))
        .collect()
}
```

- [ ] **Step 4: Route both existing branches through it**

In the `group == 5` branch, replace lines 194-219 (from `let mut buckets: Vec<(RoleGroup, Vec<PopoverLink>)> = RoleGroup::ORDERED` through the `.collect();` that ends the `let groups` binding) with:

```rust
                        let groups = role_buckets(
                            job_chips_sorted()
                                .into_iter()
                                .map(|job| {
                                    let label = job_chip_label(job).to_string();
                                    (
                                        role_for_job_abbr(&job.abbreviation),
                                        PopoverLink {
                                            href: href_with_world(
                                                format!(
                                                    "/items/jobset/{}",
                                                    label.replace('/', "%2F"),
                                                ),
                                                world.as_deref(),
                                            ),
                                            label,
                                            icon: PopoverIcon::Job(job.key_id),
                                        },
                                    )
                                }),
                            role_label,
                        );
```

In the `group == 1` branch, replace lines 225-253 (same span: the `let mut buckets` through the `let groups ... .collect();`) with:

```rust
                        let groups = role_buckets(
                            category_chips_for_group(1)
                                .into_iter()
                                .map(|(name, id)| {
                                    let role = data
                                        .item_search_categorys
                                        .get(&id)
                                        .map(|cat| role_for_weapon_category(cat, data))
                                        .unwrap_or(RoleGroup::Other);
                                    (
                                        role,
                                        PopoverLink {
                                            label: name.to_string(),
                                            href: href_with_world(
                                                format!("/items/category/{}", id.0),
                                                world.as_deref(),
                                            ),
                                            icon: PopoverIcon::Category(id),
                                        },
                                    )
                                }),
                            role_label,
                        );
```

Both branches keep their existing trailing `view! { <GroupedNavPopover button_label=button_label groups=groups /> }.into_any()` untouched.

- [ ] **Step 5: Run the tests to verify they pass**

```bash
cargo test -p ultros-app --lib
```

Expected: PASS, including the pre-existing `active_group_*`, `job_chips_*`, and `weapon_chips_*` tests. If `role_buckets_orders_sections_and_drops_empty_roles` fails on ordering, the bug is `role_buckets` iterating the input rather than `RoleGroup::ORDERED` — section order must come from `ORDERED`, never from insertion order.

- [ ] **Step 6: Run CI checks**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

Expected: `REAL_EXIT=0`. If clippy is OOM-killed (exit `137` / `Killed: 9` — not a lint failure), re-run with `cargo clippy --all-targets -j 2 -- -D warnings`.

- [ ] **Step 7: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/item_explorer_toolbar.rs
git commit -m "refactor(item-explorer): extract role_buckets from the two nav branches

Weapons and Job Sets each carried their own copy of the seed-per-role,
push, drop-empties, label loop. No behaviour change.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 2: Convert the popover to an inline accordion

This task is deliberately atomic: renaming the component breaks its only caller, so the component conversion, the toolbar rewrite, and the CSS must land in one commit or the tree does not compile.

**Files:**
- Rename: `ultros-frontend/ultros-app/src/components/grouped_nav_popover.rs` → `ultros-frontend/ultros-app/src/components/grouped_nav_accordion.rs`
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs:24`
- Modify: `ultros-frontend/ultros-app/src/routes/item_explorer_toolbar.rs`
- Modify: `style/tailwind.css` (add utilities after the `item-explorer-chip` block, which ends at line 2189)

**Interfaces:**
- Consumes: `role_buckets` (Task 1); the existing `active_group_from_route`, `category_chips_for_group`, `job_chips_sorted`, `job_chip_label` in the toolbar; `RoleGroup`, `role_for_job_abbr`, `role_for_weapon_category` from `item_explorer_roles.rs`; `href_with_world` from `item_explorer_scope.rs`.
- Produces:
  - `pub enum NavIcon { Job(ClassJobId), Category(ItemSearchCategoryId) }` (was `PopoverIcon`)
  - `pub struct NavLink { pub label: String, pub href: String, pub icon: NavIcon }` (was `PopoverLink`)
  - `#[component] pub fn GroupedNavAccordion(button_label: Signal<String>, groups: Signal<Vec<(Option<String>, Vec<NavLink>)>>, open: RwSignal<bool>)`
  - `pub(crate) fn accordion_open_for_route(jobset: Option<&str>, category: Option<&str>) -> bool`
  - `role_buckets` keeps the shape Task 1 gave it; only its element type is renamed `PopoverLink` → `NavLink`.

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` block in `ultros-frontend/ultros-app/src/routes/item_explorer_toolbar.rs`:

```rust
    #[test]
    fn accordion_starts_open_on_bare_items_route() {
        // Nothing is selected yet, so the picker is the whole point of the
        // page — show it expanded.
        assert!(accordion_open_for_route(None, None));
    }

    #[test]
    fn accordion_starts_closed_for_a_jobset_deep_link() {
        assert!(!accordion_open_for_route(Some("PLD"), None));
    }

    #[test]
    fn accordion_starts_closed_for_a_category_id_deep_link() {
        // The canonical route key is the numeric id.
        let data = xiv_gen_db::data();
        let weapon = data
            .item_search_categorys
            .values()
            .filter(|cat| cat.category == 1)
            .min_by_key(|cat| cat.key_id.0)
            .expect("weapons group must have categories");
        assert!(!accordion_open_for_route(
            None,
            Some(&weapon.key_id.0.to_string()),
        ));
    }

    #[test]
    fn accordion_starts_closed_for_a_legacy_name_param() {
        // Links minted before the switch to ids are percent-encoded names
        // and still resolve; they name a category, so they collapse too.
        assert!(!accordion_open_for_route(
            None,
            Some("Pugilist%27s%20Arms"),
        ));
    }

    #[test]
    fn accordion_starts_open_for_an_unresolvable_category() {
        // A dead link selects nothing, so falling back to the expanded
        // picker is the useful answer rather than a collapsed header
        // labelled with a category that does not exist.
        assert!(accordion_open_for_route(None, Some("999999")));
    }

    #[test]
    fn every_group_produces_at_least_one_subcategory_link() {
        // All five groups now render through one accordion instead of the
        // old popover/chip-strip fork, so an empty group would silently
        // produce an accordion that expands to nothing.
        for group in 1..=4u8 {
            assert!(
                !category_chips_for_group(group).is_empty(),
                "group {group} must have at least one category chip",
            );
        }
        assert!(
            !job_chips_sorted().is_empty(),
            "the job sets group must have at least one chip",
        );
    }
```

Then update the two type names inside Task 1's `role_buckets_orders_sections_and_drops_empty_roles` test, which the rename in Step 4 invalidates: `PopoverLink` becomes `NavLink`, and `PopoverIcon::Job` becomes `NavIcon::Job`. Its `use xiv_gen::ClassJobId;` import stays as-is.

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p ultros-app --lib accordion_ every_group_
```

Expected: FAIL to compile — `cannot find function 'accordion_open_for_route' in this scope`.

- [ ] **Step 3: Rename the component module**

```bash
git mv ultros-frontend/ultros-app/src/components/grouped_nav_popover.rs \
       ultros-frontend/ultros-app/src/components/grouped_nav_accordion.rs
```

Then in `ultros-frontend/ultros-app/src/components/mod.rs:24`, swap the declaration in place — the list is alphabetical and `grouped_nav_accordion` sorts to the same slot, between `gil` and `history_panel`:

```rust
pub mod gil;
pub mod grouped_nav_accordion;
pub mod history_panel;
```

- [ ] **Step 4: Rewrite the component**

Replace the **entire contents** of `ultros-frontend/ultros-app/src/components/grouped_nav_accordion.rs` with:

```rust
//! Inline accordion for the item explorer's subcategory navigation.
//! Sections have optional headers (role groups) and hold the chip links
//! the toolbar builds for the selected group.

use leptos::prelude::*;
use leptos_router::components::A;
use xiv_gen::{ClassJobId, ItemSearchCategoryId};

use crate::components::fonts::{ClassJobIcon, ItemSearchCategoryIcon};
use crate::components::icon::Icon;
use icondata as i;

/// Icon shown on a nav chip. Kept as plain data (not a view) so the link
/// lists stay `Clone` and can live inside signals.
#[derive(Clone, Copy, PartialEq)]
pub enum NavIcon {
    Job(ClassJobId),
    Category(ItemSearchCategoryId),
}

#[derive(Clone, PartialEq)]
pub struct NavLink {
    pub label: String,
    pub href: String,
    pub icon: NavIcon,
}

/// Accordion of navigation chips in labeled sections.
///
/// The panel's children are always rendered; only the wrapper's
/// `grid-template-rows` track changes between collapsed and expanded. That
/// keeps the SSR and hydration view trees identical in shape — the same
/// discipline the popover this replaced followed by toggling a class rather
/// than mounting and unmounting — and unlike a `hidden` toggle it animates.
///
/// `open` is owned by the caller. The item explorer toolbar forces the
/// accordion open when a group pill is clicked and collapses it on
/// navigation, so the state cannot live in here.
#[component]
pub fn GroupedNavAccordion(
    /// Header label — the active subcategory or a "browse" prompt.
    #[prop(into)]
    button_label: Signal<String>,
    /// Sections: optional header + chip links.
    #[prop(into)]
    groups: Signal<Vec<(Option<String>, Vec<NavLink>)>>,
    /// Expanded state, owned by the caller.
    open: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <div class="item-explorer-accordion">
            <button
                class="item-explorer-accordion-header"
                aria-expanded=move || open.get().to_string()
                aria-controls="item-explorer-subcategories"
                on:click=move |_| open.update(|o| *o = !*o)
            >
                <span>{move || button_label.get()}</span>
                <Icon
                    icon=i::BiChevronDownRegular
                    attr:class="item-explorer-accordion-chevron"
                />
            </button>
            <div
                id="item-explorer-subcategories"
                class="item-explorer-accordion-panel"
                data-open=move || open.get().to_string()
            >
                <div class="item-explorer-accordion-inner">
                    {move || {
                        groups
                            .get()
                            .into_iter()
                            .map(|(header, links)| {
                                view! {
                                    <div class="flex flex-col gap-2">
                                        {header
                                            .map(|header| {
                                                view! {
                                                    <div class="text-xs font-bold uppercase tracking-wider text-[color:var(--color-text-muted)]">
                                                        {header}
                                                    </div>
                                                }
                                            })}
                                        <div class="flex flex-wrap gap-2">
                                            {links
                                                .into_iter()
                                                .map(|link| {
                                                    view! {
                                                        <A href=link.href attr:class="item-explorer-chip">
                                                            {match link.icon {
                                                                NavIcon::Job(id) => {
                                                                    view! { <ClassJobIcon id=id /> }.into_any()
                                                                }
                                                                NavIcon::Category(id) => {
                                                                    view! { <ItemSearchCategoryIcon id=id /> }.into_any()
                                                                }
                                                            }}
                                                            <span>{link.label}</span>
                                                        </A>
                                                    }
                                                })
                                                .collect::<Vec<_>>()}
                                        </div>
                                    </div>
                                }
                            })
                            .collect::<Vec<_>>()
                    }}
                </div>
            </div>
        </div>
    }
    .into_any()
}
```

Note what is gone versus the popover: `on_click_outside`, the Escape `on:keydown`, the `NodeRef<Div>` container, the `use_location`/`pathname` effect, and the `absolute left-0 top-full z-[100]` positioning. Their imports (`leptos::html::Div`, `leptos_router::hooks::use_location`) are gone from the import block above — do not leave them behind or `-D warnings` fails on unused imports.

- [ ] **Step 5: Add the CSS**

In `style/tailwind.css`, append immediately after the `.item-explorer-chip[aria-current="page"]` rule (which ends at line 2189):

```css

/* ----- Item Explorer subcategory accordion (ItemExplorerToolbar) ----- */
@utility item-explorer-accordion {
    border: 1px solid var(--color-outline);
    border-radius: 0.5rem;
    background-color: var(--color-background-panel);
    overflow: hidden;
}

@utility item-explorer-accordion-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    width: 100%;
    padding: 0.6rem 0.9rem;
    border: 0;
    background: transparent;
    color: var(--color-text);
    font-size: 0.9rem;
    font-weight: 600;
    text-align: left;
    cursor: pointer;
    transition: background-color 150ms ease;
}
.item-explorer-accordion-header:hover {
    background-color: color-mix(in srgb, var(--brand-ring) 8%, transparent);
}
.item-explorer-accordion-chevron {
    margin-left: auto;
    flex-shrink: 0;
    transition: transform 200ms ease;
}
.item-explorer-accordion-header[aria-expanded="true"] .item-explorer-accordion-chevron {
    transform: rotate(180deg);
}

/* Expansion animates the grid track rather than mounting/unmounting the
   panel, so the children are always present and the SSR and hydration view
   trees keep identical shape. The `min-height: 0` on the inner element is
   what actually lets the `0fr` track collapse. */
@utility item-explorer-accordion-panel {
    display: grid;
    grid-template-rows: 0fr;
    transition: grid-template-rows 200ms ease;
}
.item-explorer-accordion-panel[data-open="true"] {
    grid-template-rows: 1fr;
}
.item-explorer-accordion-inner {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    min-height: 0;
    overflow: hidden;
    padding: 0 0.9rem 0.9rem;
}
```

No `prefers-reduced-motion` block is needed — `style/tailwind.css:134-141` already forces `transition-duration: 0.01ms` globally under that query.

- [ ] **Step 6: Rewrite the toolbar**

In `ultros-frontend/ultros-app/src/routes/item_explorer_toolbar.rs`. **Task 1 already edited this file, so every location below is given as an anchor rather than a line number — the original line numbers have shifted.**

**6a.** Replace the `grouped_nav_popover` import (near the top of the import block):

```rust
use crate::components::grouped_nav_popover::{GroupedNavPopover, PopoverIcon, PopoverLink};
```

with:

```rust
use crate::components::grouped_nav_accordion::{GroupedNavAccordion, NavIcon, NavLink};
```

**6b.** Delete the whole `pub(crate) fn group_uses_popover` item, including its `/// Whether the given group renders its subcategories in a popover` doc comment. Its only callers disappear in this task, and `-D warnings` fails on dead code.

**6c.** Rename the element type in `role_buckets` (added in Task 1, sitting just after `job_chip_label`). Three occurrences of `PopoverLink` become `NavLink`; nothing else about it changes. The result must read:

```rust
fn role_buckets(
    items: impl Iterator<Item = (RoleGroup, NavLink)>,
    role_label: impl Fn(RoleGroup) -> String,
) -> Vec<(Option<String>, Vec<NavLink>)> {
    let mut buckets: Vec<(RoleGroup, Vec<NavLink>)> = RoleGroup::ORDERED
        .iter()
        .map(|role| (*role, Vec::new()))
        .collect();
    for (role, link) in items {
        if let Some((_, links)) = buckets.iter_mut().find(|(r, _)| *r == role) {
            links.push(link);
        }
    }
    buckets
        .into_iter()
        .filter(|(_, links)| !links.is_empty())
        .map(|(role, links)| (Some(role_label(role)), links))
        .collect()
}
```

Then add the open-state route helper immediately after the closing brace of `active_group_from_route`, before the `/// Return the search categories that belong to a non-job group` doc comment:

```rust
/// Whether the subcategory accordion starts expanded for a route.
///
/// Open only when the URL names no subcategory — the bare `/items` landing,
/// where the picker is the entire content of the page. A deep link that
/// already names a category or job set starts collapsed so the item list
/// gets the viewport. An unresolvable category counts as "names nothing"
/// and opens, so a dead link lands on a usable picker.
///
/// Both args come straight from `ParamsMap::get(...).as_deref()` at the call
/// site, so this stays router-free and unit-testable like its neighbour.
pub(crate) fn accordion_open_for_route(jobset: Option<&str>, category: Option<&str>) -> bool {
    active_group_from_route(jobset, category).is_none()
}
```

**6d.** Add the `open` signal and its collapse effect. Insert immediately after the existing route-following `Effect` inside `ItemExplorerToolbar` — the one under the comment `// When the route changes (e.g. browser back), follow it.` whose body is `selected_group.set(active_group.get().unwrap_or(1));`. **Leave that effect exactly as it is**; it must not touch `open`, or browser-back would expand the accordion.

```rust
    // Expanded state, owned here rather than in the accordion: a group pill
    // click has to force it open.
    let open = RwSignal::new({
        let p = params.get_untracked();
        accordion_open_for_route(p.get("jobset").as_deref(), p.get("category").as_deref())
    });

    // Collapse onto a chosen subcategory; re-open on the bare /items route.
    //
    // `active_group` MUST stay a `Memo` — this effect leans on the memo's
    // value diffing. Navigating between two categories in the same group
    // (/items/category/24 -> /items/category/25) leaves it at the same
    // `Some(group)`, the memo stays quiet, and the accordion stays collapsed.
    // Tracking `active_group` also reads `params`, so no separate
    // `pathname.track()` is needed. On its first run it writes the value
    // `open` was already initialised to, so there is no hydration flicker.
    Effect::new(move |_| {
        open.set(active_group.get().is_none());
    });
```

**6e.** Replace the `let pill = move |group: u8, label_view: AnyView| { ... };` closure with one that also opens the accordion:

```rust
    let pill = move |group: u8, label_view: AnyView| {
        view! {
            <button
                aria-pressed=move || (selected_group.get() == group).to_string()
                on:click=move |_| {
                    selected_group.set(group);
                    open.set(true);
                }
            >
                {label_view}
            </button>
        }
    };
```

**6f.** Replace the whole subcategory `<div>` inside the `view!` — it starts with the comment `// The popover needs a normal wrapper — 'item-explorer-chip-row's` and its `class=move || { if group_uses_popover(...) ... }` closure, and ends with the `</div>` that closes it just before the outer wrapper's `</div>`. Everything in between (the `role_label` closure, the `button_label` computation, and all three group branches) is replaced by:

```rust
            <div
                role="navigation"
                aria-label=t_string!(i18n, item_explorer_categories).to_string()
            >
                {move || {
                    // Track the locale-swap revision so category/job names
                    // re-render after `reload_xiv_data`.
                    let data = tracked_data();
                    let group = selected_group.get();
                    let world = scope.query_world.get();
                    let role_label = |role: RoleGroup| -> String {
                        match role {
                            RoleGroup::Tank => t_string!(i18n, item_explorer_role_tank).to_string(),
                            RoleGroup::Healer => t_string!(i18n, item_explorer_role_healer).to_string(),
                            RoleGroup::Melee => t_string!(i18n, item_explorer_role_melee).to_string(),
                            RoleGroup::PhysRanged => {
                                t_string!(i18n, item_explorer_role_phys_ranged).to_string()
                            }
                            RoleGroup::Caster => t_string!(i18n, item_explorer_role_caster).to_string(),
                            RoleGroup::Hand => t_string!(i18n, item_explorer_role_hand).to_string(),
                            RoleGroup::Land => t_string!(i18n, item_explorer_role_land).to_string(),
                            RoleGroup::Other => t_string!(i18n, item_explorer_role_other).to_string(),
                        }
                    };
                    // Header label: the active subcategory when it belongs to
                    // the selected group, else a browse prompt.
                    let button_label = if active_group.get() == Some(group) {
                        let p = params.get();
                        match p.get("jobset") {
                            Some(jobset) => percent_encoding::percent_decode_str(&jobset)
                                .decode_utf8()
                                .ok()
                                .map(|s| s.to_string()),
                            // The category param is an id, so resolve it back
                            // to the localized name instead of labelling the
                            // header with a bare number.
                            None => p.get("category").and_then(|cat| {
                                resolve_category_param(data, &cat)
                                    .map(|category| category.name.clone())
                            }),
                        }
                    } else {
                        None
                    }
                    .unwrap_or_else(|| {
                        t_string!(i18n, item_explorer_browse_subcategories).to_string()
                    });

                    let groups: Vec<(Option<String>, Vec<NavLink>)> = if group == 5 {
                        // Job sets: jobs bucketed by role.
                        role_buckets(
                            job_chips_sorted()
                                .into_iter()
                                .map(|job| {
                                    let label = job_chip_label(job).to_string();
                                    (
                                        role_for_job_abbr(&job.abbreviation),
                                        NavLink {
                                            href: href_with_world(
                                                format!(
                                                    "/items/jobset/{}",
                                                    label.replace('/', "%2F"),
                                                ),
                                                world.as_deref(),
                                            ),
                                            label,
                                            icon: NavIcon::Job(job.key_id),
                                        },
                                    )
                                }),
                            role_label,
                        )
                    } else if group == 1 {
                        // Weapons: categories bucketed by the role of their
                        // associated job.
                        role_buckets(
                            category_chips_for_group(1)
                                .into_iter()
                                .map(|(name, id)| {
                                    let role = data
                                        .item_search_categorys
                                        .get(&id)
                                        .map(|cat| role_for_weapon_category(cat, data))
                                        .unwrap_or(RoleGroup::Other);
                                    (
                                        role,
                                        NavLink {
                                            label: name.to_string(),
                                            href: href_with_world(
                                                format!("/items/category/{}", id.0),
                                                world.as_deref(),
                                            ),
                                            icon: NavIcon::Category(id),
                                        },
                                    )
                                }),
                            role_label,
                        )
                    } else {
                        // Armor, Items, Housing: one headerless section.
                        let links: Vec<NavLink> = category_chips_for_group(group)
                            .into_iter()
                            .map(|(name, id)| NavLink {
                                label: name.to_string(),
                                href: href_with_world(
                                    format!("/items/category/{}", id.0),
                                    world.as_deref(),
                                ),
                                icon: NavIcon::Category(id),
                            })
                            .collect();
                        vec![(None, links)]
                    };

                    view! {
                        <GroupedNavAccordion
                            button_label=button_label
                            groups=groups
                            open=open
                        />
                    }
                }}
            </div>
```

Two imports in this file now have no users, and `-D warnings` fails on unused imports. Delete both lines:

```rust
use crate::components::fonts::ItemSearchCategoryIcon;
use leptos_router::components::A;
```

The deleted inline chip strip was the only place the toolbar rendered an `<A>` or an icon itself; the accordion component does both now. Every other import in the file is still live — `resolve_category_param`, `href_with_world`, `RoleGroup`/`role_for_job_abbr`/`role_for_weapon_category`, `percent_encoding`, `tracked_data`, and the `Toolbar*` primitives all still have callers.

- [ ] **Step 7: Run the tests to verify they pass**

```bash
cargo test -p ultros-app --lib
```

Expected: PASS — the 6 tests added here, Task 1's `role_buckets_orders_sections_and_drops_empty_roles` (now with renamed types), and every pre-existing test in the file.

- [ ] **Step 8: Run CI checks**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

Expected: `REAL_EXIT=0`. A clippy exit of `127` with no error text on Windows means MSYS perl is shadowing Strawberry Perl on `PATH`, not a lint failure — prepend `/c/Strawberry/perl/bin:/c/Strawberry/c/bin:` to `$PATH` and re-run.

- [ ] **Step 9: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/grouped_nav_accordion.rs \
        ultros-frontend/ultros-app/src/components/mod.rs \
        ultros-frontend/ultros-app/src/routes/item_explorer_toolbar.rs \
        style/tailwind.css
git commit -m "feat(item-explorer): swap the subcategory popover for an inline accordion

All five groups now render one accordion instead of forking between an
overlay popover (Weapons, Job Sets, >8 categories) and a horizontally
scrolling chip strip. It expands on a group pill click and collapses when
a subcategory is chosen.

Expansion animates a grid-template-rows track rather than mounting and
unmounting the panel, so the children are always present and the SSR and
hydration view trees keep identical shape.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 3: Retire the chip-strip CSS and verify in a browser

**Files:**
- Modify: `style/tailwind.css` (delete the `item-explorer-chip-row` utility, lines 2144-2166)

**Interfaces:**
- Consumes: everything from Task 2.
- Produces: nothing — this is cleanup plus the manual verification gate.

- [ ] **Step 1: Confirm the utility has no remaining consumers**

```bash
grep -rn "item-explorer-chip-row" --include="*.rs" --include="*.css" --include="*.html" .
```

Expected: exactly one hit, the `@utility item-explorer-chip-row {` definition in `style/tailwind.css`. If any `.rs` file still references it, Task 2's step 6f was applied incompletely — fix that before deleting.

- [ ] **Step 2: Delete the utility**

In `style/tailwind.css`, delete the whole block from `@utility item-explorer-chip-row {` through its closing `}` (lines 2144-2166), plus the now-inaccurate section comment on line 2143:

```css
/* ----- Item Explorer chip strip (consumed by ItemExplorerToolbar) ----- */
```

Leave `@utility item-explorer-chip` and its two companion rules alone — the accordion still uses them for every chip.

- [ ] **Step 3: Re-run the full suite and CI**

```bash
cargo test -p ultros-app --lib
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

Expected: tests PASS, `REAL_EXIT=0`.

- [ ] **Step 4: Verify in a browser**

Build and run the SSR app locally, then walk the behaviour table from the spec. Check each of these on `/items`:

1. Land on `/items` — accordion is **expanded**, header reads the browse prompt.
2. Click each group pill in turn (Weapons, Armor, Items, Housing, Job Sets) — accordion **opens every time**, and the panel content swaps to that group. Weapons and Job Sets show role section headers; Armor/Items/Housing show one unheadered wrap.
3. Click a subcategory chip — navigates, accordion **collapses**, header now reads the chosen subcategory's name.
4. Reload that URL — accordion starts **collapsed**.
5. Browser back to `/items` — accordion **re-opens**.
6. Click the header itself — toggles both directions, chevron rotates.
7. Check the browser console for a tachys hydration panic (`failed_to_cast_element` / `RuntimeError: unreachable`) on each of the pages above. There must be none — the panel's always-rendered children are what prevent it.
8. At mobile width, confirm the Job Sets panel flows and scrolls with the page rather than trapping a nested scrollbar.

- [ ] **Step 5: Commit**

```bash
git add style/tailwind.css
git commit -m "style(item-explorer): drop the now-unused chip-strip utility

item-explorer-chip-row's horizontal scroll and mask gradient existed to
hold the small groups' inline strip; the accordion replaced its only
consumer.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```
