# Item View Layout — Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the four reported problems on `/item/:world/:id` — truncated stat names, two uncapped stacked listings tables, no way to jump between sections, and market share ranked too high — without introducing the lens yet.

**Architecture:** Extract the pure logic (quality filtering, preview slicing, section identity, href building) into small tested modules, then rewire `ItemView`'s section order and chrome around them. The listings tables merge into one bounded, scrollable table with a quality filter. A slim `position: sticky` nav bar is placed *below* the existing world menu in the DOM, so it pins on its own as the world pills scroll away — no scroll listener, and no new SSR/hydration surface on a route with a long history of tachys hydration panics.

**Tech Stack:** Rust, Leptos 0.8 (SSR + hydrate), Tailwind CSS v4, `leptos-i18n` (7 locales), `cargo test`.

**Spec:** `docs/superpowers/specs/2026-07-26-item-view-layout-design.md`

---

## Working notes

**Repo root:** `/Users/aaronkarras/code/ffxiv-playground/.claude/worktrees/item-view-page-layout-8cfce2`

All paths below are relative to that root. The frontend crate is `ultros-app` at
`ultros-frontend/ultros-app/`.

**Before every commit:** run `./check_ci.sh` from the repo root (`cargo fmt --all -- --check`
plus `cargo clippy --all-targets -- -D warnings`). CI fails on either.

**Hydration rules for this route.** `item_view.rs` and `listings_table.rs` carry
extensive comments documenting real production crashes (GlitchTip #6831, #6864,
#6865). Two rules that this plan must not break:

1. Reactive reads inside `<Transition>` bodies on this page go through the
   `with_or` / `get_or_default` helpers at `item_view.rs:56` and `:64`, never bare
   `.with()` / `.get()`.
2. In `listings_table.rs`, the element following the `<For>` must be a dynamic
   `{ move || … }` block, because a `<For>` relies on its next sibling to emit the
   marker node that bounds the keyed list. See the comment at `listings_table.rs:51-73`.

---

## Task 0: Initialize git submodules

Nothing compiles without these — `xiv-gen-db`'s build script reads
`xiv-gen/ffxiv-datamining/`, and `ultros-xiv-icons/build.rs` reads
`universalis-assets/icon2x`. `git submodule status` currently shows all three
uninitialized (leading `-`).

**Files:** none (working tree setup)

- [ ] **Step 1: Init the two straightforward submodules**

```bash
git submodule update --init --recursive xiv-gen/ffxiv-datamining ultros/static/classjob-icons
```

- [ ] **Step 2: Init universalis-assets against the main clone**

Do **not** use `--depth=1` here. The shallow fetch does not contain the pinned
commit; git aborts with `fatal: Unable to find current revision in submodule
path ...`, leaves the directory empty, and leaves a broken per-worktree gitdir
that makes every later attempt fail until it is removed.

```bash
rm -rf /Users/aaronkarras/code/ffxiv-playground/.git/worktrees/item-view-page-layout-8cfce2/modules/ultros-frontend/universalis-assets
rm -f ultros-frontend/ultros-xiv-icons/universalis-assets/.git
git submodule update --init --reference /Users/aaronkarras/code/ffxiv-playground/.git/modules/ultros-frontend/universalis-assets ultros-frontend/ultros-xiv-icons/universalis-assets
```

Never delete `.git/modules/...` itself — that is the main checkout's shared clone.

- [ ] **Step 3: Verify the build works**

Run: `cargo test -p ultros-app --lib 2>&1 | tail -20`
Expected: compiles, and the existing `item_view` / `listing_filters` tests pass.

If `ultros-xiv-icons/build.rs` panics with `No such file or directory` on
`universalis-assets/icon2x`, Step 2 did not populate the directory — re-run it.

- [ ] **Step 4: No commit**

Submodule pointers are unchanged; nothing to commit.

---

## Task 1: Fix truncated item stat names

`ItemStats` renders a 4-column grid inside the header's
`minmax(320px,1.2fr)` track, leaving roughly 40px per name, so `truncate`
clips "Vitality" to "Vi…".

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/stats_display.rs:119`

- [ ] **Step 1: Widen the columns**

Replace the grid container class at line 119:

```rust
<div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-x-8 gap-y-2 w-full">
```

with:

```rust
<div class="grid grid-cols-1 sm:grid-cols-2 gap-x-5 gap-y-2 w-full">
```

Two columns at every breakpoint above `sm`, and a smaller column gap. The
`truncate` on the name stays as a safety net for unusually long localised stat
names; it should no longer engage for the common ones.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p ultros-app 2>&1 | tail -5`
Expected: `Finished` with no errors.

- [ ] **Step 3: Verify visually**

Start the app and open an item with four or more stats — a Diadochos accessory
(e.g. `/item/Aether/40644`) is a good case. Confirm "Vitality",
"Determination" and "Critical Hit" render in full.

- [ ] **Step 4: Commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-app/src/components/stats_display.rs
git commit -m "fix(item-view): stop truncating item stat names

Four columns inside the header's 1.2fr track left ~40px per name, so
'Vitality' rendered as 'Vi...'. Two columns with a tighter gap fits the
longest stat names at every breakpoint."
```

---

## Task 2: Listing quality filter

Pure, testable filtering so the merged table can show All / HQ / NQ.

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/listing_quality.rs`
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs:31`

- [ ] **Step 1: Write the failing tests**

Create `ultros-frontend/ultros-app/src/components/listing_quality.rs` containing
only the test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use ultros_api_types::ActiveListing;

    fn listing(id: i32, hq: bool) -> (ActiveListing, ()) {
        (
            ActiveListing {
                id,
                world_id: 100,
                item_id: 1,
                retainer_id: id,
                price_per_unit: id * 10,
                quantity: 1,
                hq,
                timestamp: NaiveDate::from_ymd_opt(2026, 1, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap(),
            },
            (),
        )
    }

    #[test]
    fn all_is_the_default() {
        assert_eq!(ListingQuality::default(), ListingQuality::All);
    }

    #[test]
    fn all_keeps_every_row() {
        let rows = vec![listing(1, true), listing(2, false)];

        let result = filter_by_quality(rows.clone(), ListingQuality::All);

        assert_eq!(result, rows);
    }

    #[test]
    fn hq_keeps_only_high_quality() {
        let rows = vec![listing(1, true), listing(2, false), listing(3, true)];

        let result = filter_by_quality(rows, ListingQuality::Hq);

        assert_eq!(
            result.iter().map(|(l, _)| l.id).collect::<Vec<_>>(),
            vec![1, 3]
        );
    }

    #[test]
    fn nq_keeps_only_normal_quality() {
        let rows = vec![listing(1, true), listing(2, false), listing(3, true)];

        let result = filter_by_quality(rows, ListingQuality::Nq);

        assert_eq!(
            result.iter().map(|(l, _)| l.id).collect::<Vec<_>>(),
            vec![2]
        );
    }

    #[test]
    fn filtering_preserves_input_order() {
        let rows = vec![listing(5, false), listing(1, false), listing(3, false)];

        let result = filter_by_quality(rows, ListingQuality::Nq);

        assert_eq!(
            result.iter().map(|(l, _)| l.id).collect::<Vec<_>>(),
            vec![5, 1, 3]
        );
    }

    #[test]
    fn empty_input_yields_empty_output() {
        let rows: Vec<(ActiveListing, ())> = Vec::new();

        assert!(filter_by_quality(rows, ListingQuality::Hq).is_empty());
    }
}
```

Register the module by adding this line to
`ultros-frontend/ultros-app/src/components/mod.rs`, immediately after the
existing `pub(crate) mod listing_filters;` on line 31:

```rust
pub(crate) mod listing_quality;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ultros-app --lib listing_quality 2>&1 | tail -20`
Expected: FAIL — `cannot find type ListingQuality in this scope` and
`cannot find function filter_by_quality in this scope`.

- [ ] **Step 3: Write the implementation**

Prepend to `listing_quality.rs`, above the `#[cfg(test)]` module:

```rust
use ultros_api_types::ActiveListing;

/// Which quality of listing the reader is currently looking at.
///
/// `All` is the default so the merged table opens showing exactly the rows the
/// two split HQ/NQ tables used to show between them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ListingQuality {
    #[default]
    All,
    Hq,
    Nq,
}

impl ListingQuality {
    /// True when a listing carrying this `hq` flag belongs in the current view.
    pub(crate) fn matches(self, hq: bool) -> bool {
        match self {
            ListingQuality::All => true,
            ListingQuality::Hq => hq,
            ListingQuality::Nq => !hq,
        }
    }
}

/// Keep only the rows matching `quality`. Input order is preserved, so a
/// caller that has already sorted by price stays sorted.
pub(crate) fn filter_by_quality<T>(
    listings: Vec<(ActiveListing, T)>,
    quality: ListingQuality,
) -> Vec<(ActiveListing, T)> {
    if quality == ListingQuality::All {
        return listings;
    }
    listings
        .into_iter()
        .filter(|(listing, _)| quality.matches(listing.hq))
        .collect()
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p ultros-app --lib listing_quality 2>&1 | tail -20`
Expected: PASS — 6 passed.

- [ ] **Step 5: Commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-app/src/components/listing_quality.rs ultros-frontend/ultros-app/src/components/mod.rs
git commit -m "feat(item-view): add listing quality filter

Pure All/HQ/NQ filtering, ahead of merging the two split listings
tables into one."
```

---

## Task 3: Bound the listings table height

Today `ListingsTable` shows 10 rows and "Show more" expands to every listing,
growing the page by however many screens that takes. Cap the table's height and
scroll inside it instead.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/listings_table.rs`

- [ ] **Step 1: Write the failing test**

Append this module to the end of `listings_table.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapsed_shows_at_most_the_preview_count() {
        assert_eq!(visible_listing_count(100, false), LISTING_PREVIEW_ROWS);
    }

    #[test]
    fn collapsed_never_exceeds_the_total() {
        assert_eq!(visible_listing_count(3, false), 3);
    }

    #[test]
    fn expanded_shows_everything() {
        assert_eq!(visible_listing_count(100, true), 100);
    }

    #[test]
    fn empty_is_empty_either_way() {
        assert_eq!(visible_listing_count(0, false), 0);
        assert_eq!(visible_listing_count(0, true), 0);
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p ultros-app --lib listings_table 2>&1 | tail -20`
Expected: FAIL — `cannot find function visible_listing_count in this scope`
and `cannot find value LISTING_PREVIEW_ROWS in this scope`.

- [ ] **Step 3: Add the pure function**

Insert directly below the `use` block at the top of `listings_table.rs`:

```rust
/// Rows rendered before the reader asks for more. Kept small because the
/// table now lives in a fixed-height scroller — the preview exists to bound
/// render cost on liquid items, not to bound visible height.
pub(crate) const LISTING_PREVIEW_ROWS: usize = 10;

/// How many rows to render for a given expansion state.
pub(crate) fn visible_listing_count(total: usize, show_more: bool) -> usize {
    if show_more {
        total
    } else {
        total.min(LISTING_PREVIEW_ROWS)
    }
}
```

- [ ] **Step 4: Use it in the slicing memo**

Replace the `listings` memo (currently `listings_table.rs:27-35`):

```rust
    let listings = Memo::new(move |_| {
        sorted_listings.with(|listings| {
            if show_more() {
                listings.clone()
            } else {
                listings.iter().take(10).cloned().collect()
            }
        })
    });
```

with:

```rust
    let listings = Memo::new(move |_| {
        sorted_listings.with(|listings| {
            let take = visible_listing_count(listings.len(), show_more());
            listings.iter().take(take).cloned().collect::<Vec<_>>()
        })
    });
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p ultros-app --lib listings_table 2>&1 | tail -20`
Expected: PASS — 4 passed.

- [ ] **Step 6: Bound the height and pin the header row**

Replace the opening wrapper and `<thead>` (currently `listings_table.rs:37-49`):

```rust
        <div class="overflow-x-auto">
            <table class="w-full min-w-[720px]">
            <thead>
                <tr>
                    <th scope="col">{t!(i18n, listings_col_price)}</th>
```

with:

```rust
        <div class="max-h-[26rem] overflow-y-auto overflow-x-auto rounded-lg border border-[color:var(--color-outline)]">
            <table class="w-full min-w-[720px]">
            <thead class="sticky top-0 z-10 bg-[color:var(--color-background)]">
                <tr>
                    <th scope="col">{t!(i18n, listings_col_price)}</th>
```

The header needs an opaque background — without it, rows scroll visibly
underneath the pinned row.

Leave everything from the `<For>` down untouched. In particular the "show more"
row stays inside its `{ move || … }` block: it is the dynamic sibling that
supplies the marker node bounding the `<For>`, and making it static reintroduces
the hydration panic documented at `listings_table.rs:51-73`.

- [ ] **Step 7: Verify it compiles and existing tests still pass**

Run: `cargo test -p ultros-app --lib 2>&1 | tail -20`
Expected: PASS, no regressions.

- [ ] **Step 8: Commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-app/src/components/listings_table.rs
git commit -m "feat(item-view): bound listings table height

Cap the table at 26rem with an internal scroller and a sticky header
row, so 'Show more' grows the box instead of the page."
```

---

## Task 4: Add the new i18n keys

Every user-facing string must exist in all seven locale files with a real
translation before the components that use them will compile (`leptos-i18n`
fails the build on a key missing from any locale). Doing this in one pass keeps
the later tasks focused.

**Files:**
- Modify: `ultros-frontend/ultros-app/locales/en.json`
- Modify: `ultros-frontend/ultros-app/locales/fr.json`
- Modify: `ultros-frontend/ultros-app/locales/de.json`
- Modify: `ultros-frontend/ultros-app/locales/ja.json`
- Modify: `ultros-frontend/ultros-app/locales/cn.json`
- Modify: `ultros-frontend/ultros-app/locales/ko.json`
- Modify: `ultros-frontend/ultros-app/locales/tc.json`

- [ ] **Step 1: Append the keys to each locale**

The files are flat JSON objects in insertion order with 4-space indent. Append
each block just before the closing `}` of the corresponding file, adding a
comma to the previously-last entry.

`en.json`:

```json
    "item_view_nav_aria": "Jump to section",
    "item_view_nav_overview": "Overview",
    "item_view_nav_listings": "Listings",
    "item_view_nav_history": "History",
    "item_view_nav_sources": "Sources",
    "item_view_nav_related": "Related",
    "item_view_listings_count": "{{count}} listings",
    "item_view_quality_filter_aria": "Filter listings by quality",
    "item_view_quality_all": "All"
```

`fr.json`:

```json
    "item_view_nav_aria": "Aller à une section",
    "item_view_nav_overview": "Aperçu",
    "item_view_nav_listings": "Annonces",
    "item_view_nav_history": "Historique",
    "item_view_nav_sources": "Obtention",
    "item_view_nav_related": "Objets liés",
    "item_view_listings_count": "{{count}} annonces",
    "item_view_quality_filter_aria": "Filtrer les annonces par qualité",
    "item_view_quality_all": "Toutes"
```

`de.json`:

```json
    "item_view_nav_aria": "Zu Abschnitt springen",
    "item_view_nav_overview": "Übersicht",
    "item_view_nav_listings": "Angebote",
    "item_view_nav_history": "Verlauf",
    "item_view_nav_sources": "Bezugsquellen",
    "item_view_nav_related": "Verwandte",
    "item_view_listings_count": "{{count}} Angebote",
    "item_view_quality_filter_aria": "Angebote nach Qualität filtern",
    "item_view_quality_all": "Alle"
```

`ja.json`:

```json
    "item_view_nav_aria": "セクションへ移動",
    "item_view_nav_overview": "概要",
    "item_view_nav_listings": "出品",
    "item_view_nav_history": "履歴",
    "item_view_nav_sources": "入手方法",
    "item_view_nav_related": "関連",
    "item_view_listings_count": "出品{{count}}件",
    "item_view_quality_filter_aria": "品質で出品を絞り込む",
    "item_view_quality_all": "すべて"
```

`cn.json`:

```json
    "item_view_nav_aria": "跳转到板块",
    "item_view_nav_overview": "概览",
    "item_view_nav_listings": "在售",
    "item_view_nav_history": "记录",
    "item_view_nav_sources": "获取方式",
    "item_view_nav_related": "相关",
    "item_view_listings_count": "{{count}} 条在售",
    "item_view_quality_filter_aria": "按品质筛选在售",
    "item_view_quality_all": "全部"
```

`ko.json`:

```json
    "item_view_nav_aria": "섹션으로 이동",
    "item_view_nav_overview": "개요",
    "item_view_nav_listings": "판매 목록",
    "item_view_nav_history": "내역",
    "item_view_nav_sources": "획득 방법",
    "item_view_nav_related": "관련",
    "item_view_listings_count": "{{count}}개 판매 중",
    "item_view_quality_filter_aria": "품질별로 판매 목록 필터",
    "item_view_quality_all": "전체"
```

`tc.json`:

```json
    "item_view_nav_aria": "跳至區塊",
    "item_view_nav_overview": "概覽",
    "item_view_nav_listings": "在售",
    "item_view_nav_history": "紀錄",
    "item_view_nav_sources": "獲取方式",
    "item_view_nav_related": "相關",
    "item_view_listings_count": "{{count}} 筆在售",
    "item_view_quality_filter_aria": "依品質篩選在售",
    "item_view_quality_all": "全部"
```

- [ ] **Step 2: Verify every locale parses and has the same key set**

Run:

```bash
cd ultros-frontend/ultros-app && python3 -c "
import json
ls=['en','fr','de','ja','cn','ko','tc']
ds={l:set(json.load(open(f'locales/{l}.json'))) for l in ls}
base=ds['en']
for l in ls:
    assert ds[l]==base, (l, sorted(base^ds[l]))
print('all', len(base), 'keys present in all 7 locales')
"
```

Expected: `all 1520 keys present in all 7 locales`

- [ ] **Step 3: Commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-app/locales
git commit -m "i18n(item-view): add section nav and listings filter strings"
```

---

## Task 5: Merge the HQ and NQ listings tables

Replace two stacked tables with one panel: a quality filter, a row count, the
datacenter-exclusion controls folded into the header, and a single bounded
table.

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/listings_panel.rs`
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/item_view.rs` (remove
  `HighQualityTable` at `:1436-1483` and `LowQualityTable` at `:1485-1529`;
  rewire `ListingsContent`'s view at `:1724-1728`)

- [ ] **Step 1: Create the panel component**

Create `ultros-frontend/ultros-app/src/components/listings_panel.rs`:

```rust
use crate::components::listing_quality::{ListingQuality, filter_by_quality};
use crate::components::listings_table::ListingsTable;
use crate::error::AppError;
use crate::i18n::{t, t_string};
use leptos::prelude::*;
use std::sync::Arc;
use ultros_api_types::{ActiveListing, CurrentlyShownItem, Retainer};

type ListingRows = Vec<(ActiveListing, Arc<Retainer>)>;

/// The active-listings section: one table for both qualities, with a filter,
/// rather than two stacked tables with two independent "Show more" buttons.
///
/// The datacenter exclusion controls used to occupy a section of their own;
/// they now live in a `<details>` disclosure in this panel's header.
#[component]
pub fn ListingsPanel(
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
    #[prop(into)] filtered_listings: Signal<ListingRows>,
    world: Memo<String>,
    excluded_datacenters: RwSignal<std::collections::HashSet<String>>,
) -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    let (quality, set_quality) = signal(ListingQuality::default());

    let quality_button = move |value: ListingQuality, label: String| {
        view! {
            <button
                type="button"
                aria-pressed=move || (quality.get() == value).to_string()
                class=move || {
                    [
                        "px-3 py-1 text-sm transition-colors",
                        if quality.get() == value {
                            "bg-[color:var(--brand-bg)] text-[color:var(--brand-fg)] font-bold"
                        } else {
                            "text-[color:var(--color-text-muted)] hover:text-brand-100"
                        },
                    ]
                        .join(" ")
                }
                on:click=move |_| set_quality.set(value)
            >
                {label}
            </button>
        }
    };

    view! {
        <Transition fallback=move || view! { <crate::components::skeleton::BoxSkeleton /> }>
            {move || {
                // Read `listing_resource` inside the Transition so this section
                // actually suspends on it during SSR. `filtered_listings` is a Memo
                // created outside any Suspense boundary, so reading it alone does NOT
                // subscribe this Transition to the resource — the server would then
                // render an empty table while the client hydrates a populated one,
                // tripping the tachys hydration `unreachable!()` panic (GlitchTip #6831).
                if !listing_resource.with(|r| matches!(r, Some(Ok(_)))) {
                    return ().into_any();
                }
                let rows = Memo::new(move |_| {
                    let all = crate::routes::item_view::get_or_default(&filtered_listings);
                    filter_by_quality(all, quality.get())
                });
                view! {
                    <div class="flex flex-col gap-3 rounded-lg border border-[color:var(--color-outline)] p-3 sm:p-4">
                        <div class="flex flex-wrap items-center gap-3">
                            <h2 class="text-xl font-bold text-brand-200">
                                {t!(i18n, active_listings)}
                            </h2>
                            <div
                                role="group"
                                aria-label=move || {
                                    t_string!(i18n, item_view_quality_filter_aria).to_string()
                                }
                                class="inline-flex overflow-hidden rounded-md border border-[color:var(--color-outline)]"
                            >
                                {quality_button(
                                    ListingQuality::All,
                                    t_string!(i18n, item_view_quality_all).to_string(),
                                )}
                                {quality_button(
                                    ListingQuality::Hq,
                                    t_string!(i18n, hq).to_string(),
                                )}
                                {quality_button(
                                    ListingQuality::Nq,
                                    t_string!(i18n, nq).to_string(),
                                )}
                            </div>
                            <span class="text-sm text-[color:var(--color-text-muted)]">
                                {move || t!(i18n, item_view_listings_count, count = rows.with(|r| r.len()))}
                            </span>
                        </div>
                        <details class="group">
                            <summary class="cursor-pointer text-sm text-brand-300 hover:text-brand-100">
                                {t!(i18n, item_view_exclude_datacenters)}
                            </summary>
                            <div class="mt-2">
                                <crate::routes::item_view::DatacenterExclusionControls
                                    world=world
                                    excluded_datacenters=excluded_datacenters
                                />
                            </div>
                        </details>
                        <ListingsTable listings=rows />
                    </div>
                }
                    .into_any()
            }}
        </Transition>
    }
    .into_any()
}
```

`DatacenterExclusionControls` is currently private to `item_view.rs`. Change its
declaration at `item_view.rs:340` from `fn DatacenterExclusionControls(` to
`pub fn DatacenterExclusionControls(` — the `#[component]` attribute above it
stays as-is.

- [ ] **Step 2: Register the module**

Add to `ultros-frontend/ultros-app/src/components/mod.rs`, alphabetically
adjacent to the existing `pub mod listings_table;` on line 32:

```rust
pub mod listings_panel;
```

- [ ] **Step 3: Make the shared helper reachable**

`ListingsPanel` calls `get_or_default`, which is currently a private free
function in `item_view.rs`. Change its signature at
`ultros-frontend/ultros-app/src/routes/item_view.rs:64` from:

```rust
fn get_or_default<S>(signal: &S) -> S::Value
```

to:

```rust
pub(crate) fn get_or_default<S>(signal: &S) -> S::Value
```

Leave the doc comment above it as-is.

- [ ] **Step 4: Delete the two old table components**

Remove `HighQualityTable` (`item_view.rs:1436-1483`) and `LowQualityTable`
(`item_view.rs:1485-1529`) entirely.

- [ ] **Step 5: Rewire the section**

In `ListingsContent`'s view, replace the `#listings` block currently at
`item_view.rs:1724-1728`:

```rust
            <div id="listings" class="grid grid-cols-1 gap-6 mt-6">
                <DatacenterExclusionControls world excluded_datacenters />
                <HighQualityTable listing_resource filtered_listings />
                <LowQualityTable listing_resource filtered_listings />
            </div>
```

with:

```rust
            <div id="listings" class="scroll-mt-16 mt-6">
                <ListingsPanel
                    listing_resource
                    filtered_listings
                    world
                    excluded_datacenters
                />
            </div>
```

Update the import block at `item_view.rs:10-14` to bring in the new component —
add `listings_panel::ListingsPanel,` alongside the existing `listings_table::*,`.

- [ ] **Step 6: Verify it compiles and tests pass**

Run: `cargo test -p ultros-app --lib 2>&1 | tail -20`
Expected: PASS. If the compiler reports `HighQualityTable`/`LowQualityTable`
still referenced, a call site was missed in Step 5.

- [ ] **Step 7: Verify in the app**

Open an item with both HQ and NQ listings on a datacenter scope. Confirm: one
table, All/HQ/NQ switches the rows, the count updates, the table scrolls
internally rather than growing the page, and the datacenter exclusion controls
still work from the header.

- [ ] **Step 8: Commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-app/src
git commit -m "feat(item-view): merge HQ and NQ listings into one table

Two tables with identical columns, stacked, each with its own 'Show
more' were the largest contributor to page height. One table with an
All/HQ/NQ filter halves the section and makes cross-quality comparison
possible without scrolling between tables. Datacenter exclusion moves
into the panel header instead of occupying its own section."
```

---

## Task 6: Section identity

The jump-nav needs a single source of truth for section order and anchor ids.
Two of these ids are already linked from callouts in `MarketStatsPanel`
(`#listings` at `item_view.rs:981`, `#history` at `:1020`) and must not change.

**Files:**
- Create: `ultros-frontend/ultros-app/src/routes/item_view_sections.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/mod.rs`

- [ ] **Step 1: Write the failing tests**

Create `ultros-frontend/ultros-app/src/routes/item_view_sections.rs` with only
the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_section_has_a_unique_id() {
        let ids: HashSet<&str> = Section::ALL.iter().map(|s| s.id()).collect();

        assert_eq!(ids.len(), Section::ALL.len());
    }

    #[test]
    fn no_id_is_empty() {
        assert!(Section::ALL.iter().all(|s| !s.id().is_empty()));
    }

    #[test]
    fn preexisting_anchors_are_preserved() {
        // MarketStatsPanel already links to these; renaming them would break
        // the savings callout and the stat-tile links.
        assert_eq!(Section::Listings.id(), "listings");
        assert_eq!(Section::History.id(), "history");
    }

    #[test]
    fn overview_is_first() {
        assert_eq!(Section::ALL.first(), Some(&Section::Overview));
    }

    #[test]
    fn href_is_a_fragment_link() {
        assert_eq!(Section::Listings.href(), "#listings");
    }
}
```

Register it by adding to `ultros-frontend/ultros-app/src/routes/mod.rs`,
immediately after the existing `pub mod item_explorer_scope;` on line 14:

```rust
pub mod item_view_sections;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ultros-app --lib item_view_sections 2>&1 | tail -20`
Expected: FAIL — `cannot find type Section in this scope`.

- [ ] **Step 3: Write the implementation**

Prepend to `item_view_sections.rs`:

```rust
//! Jump-nav destinations for the item view.
//!
//! The order here is the page's DOM order. Phase 2's lens work reorders the
//! rendered sections with CSS `order` while leaving this DOM order — and
//! therefore this list — untouched.

/// One navigable section of `/item/:world/:id`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Overview,
    Listings,
    History,
    Sources,
    Related,
}

impl Section {
    /// Every section, in DOM order.
    pub const ALL: [Section; 5] = [
        Section::Overview,
        Section::Listings,
        Section::History,
        Section::Sources,
        Section::Related,
    ];

    /// The `id` attribute the section renders with, and the fragment the nav
    /// links to.
    ///
    /// `listings` and `history` predate this module — `MarketStatsPanel`'s
    /// stat tiles and savings callout link to them directly — so they are
    /// fixed by compatibility, not choice.
    pub fn id(self) -> &'static str {
        match self {
            Section::Overview => "overview",
            Section::Listings => "listings",
            Section::History => "history",
            Section::Sources => "sources",
            Section::Related => "related",
        }
    }

    /// Fragment link to this section.
    pub fn href(self) -> String {
        format!("#{}", self.id())
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p ultros-app --lib item_view_sections 2>&1 | tail -20`
Expected: PASS — 5 passed.

- [ ] **Step 5: Commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-app/src/routes/item_view_sections.rs ultros-frontend/ultros-app/src/routes/mod.rs
git commit -m "feat(item-view): add section identity for jump nav"
```

---

## Task 7: Preserve the query string when switching worlds

`WorldButton` builds `/item/{world}/{id}` and drops the query string, so
switching worlds silently discards the reader's `?exclude-worlds=` filter. Phase
2 adds `?lens=` to the same query string, so fix it now.

**Files:**
- Create: `ultros-frontend/ultros-app/src/routes/item_view_scope.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/mod.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/item_view.rs` (`WorldButton`
  at `:104-181`)

- [ ] **Step 1: Write the failing tests**

Create `ultros-frontend/ultros-app/src/routes/item_view_scope.rs` with only the
test module:

```rust
#[cfg(test)]
mod tests {
    use super::item_href;

    #[test]
    fn empty_query_yields_a_clean_path() {
        assert_eq!(item_href("Aether", 40644, ""), "/item/Aether/40644");
    }

    #[test]
    fn query_is_appended() {
        assert_eq!(
            item_href("Aether", 40644, "exclude-worlds=100,200"),
            "/item/Aether/40644?exclude-worlds=100,200",
        );
    }

    #[test]
    fn world_names_are_escaped() {
        // Region names from the cn/ko data are non-ASCII, and "North-America"
        // must survive unchanged.
        assert_eq!(
            item_href("North-America", 1, ""),
            "/item/North-America/1",
        );
    }
}
```

Register it in `ultros-frontend/ultros-app/src/routes/mod.rs` next to the entry
added in Task 6:

```rust
pub mod item_view_scope;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ultros-app --lib item_view_scope 2>&1 | tail -20`
Expected: FAIL — `cannot find function item_href in this scope`.

- [ ] **Step 3: Write the implementation**

Prepend to `item_view_scope.rs`:

```rust
//! URL construction for the item view.

use leptos_router::location::Url;

/// Canonical item URL for a scope name, carrying the current query string
/// forward.
///
/// Switching worlds must not discard the reader's filters: `?exclude-worlds=`
/// today, and `?lens=` once Phase 2 lands.
pub fn item_href(world: &str, item_id: i32, query: &str) -> String {
    let path = format!("/item/{}/{item_id}", Url::escape(world));
    if query.is_empty() {
        path
    } else {
        format!("{path}?{query}")
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p ultros-app --lib item_view_scope 2>&1 | tail -20`
Expected: PASS — 3 passed.

- [ ] **Step 5: Use it in WorldButton**

In `item_view.rs`, add to the imports:

```rust
use crate::routes::item_view_scope::item_href;
```

`use_query_map` is already imported on line 25.

Inside `WorldButton`, immediately after the existing
`let world_name = world.get_name().to_string();` (line 111), add:

```rust
    let label = world_name.clone();
    let query = use_query_map();
    // Only the params this route actually owns are carried forward, so a
    // stale or hostile query key can't be reflected back into a link.
    let search = Signal::derive(move || {
        query.with(|query| match query.get("exclude-worlds") {
            Some(worlds) if !worlds.is_empty() => {
                format!("exclude-worlds={}", Url::escape(&worlds))
            }
            _ => String::new(),
        })
    });
```

Then replace the `href` attribute (line 166):

```rust
                href=format!("/item/{}/{item_id}", Url::escape(&world_name))
```

with:

```rust
                href=move || search.with(|search| item_href(&world_name, item_id, search))
```

Because that closure now captures `world_name`, change the text node on line
178 from `{world_name}` to `{label}`.

- [ ] **Step 6: Verify it compiles and tests pass**

Run: `cargo test -p ultros-app --lib 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 7: Verify in the app**

Load an item with `?exclude-worlds=` set (exclude a datacenter, then copy the
URL), click a different world in the world menu, and confirm the query string
survives the navigation.

- [ ] **Step 8: Commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-app/src
git commit -m "fix(item-view): keep query params when switching worlds

WorldButton rebuilt the path and dropped the query string, silently
discarding the reader's ?exclude-worlds= filter. Phase 2 puts ?lens= in
the same place."
```

---

## Task 8: Two-tier sticky chrome

Un-stick the world menu; add a slim bar below it carrying a compact world
picker and the jump-nav.

The slim bar is placed *after* the world menu in the DOM with
`position: sticky; top: 0`. Sticky positioning only engages once an element
reaches its threshold, so the bar sits in normal flow, the world pills scroll
away above it, and it pins itself — no scroll listener, no client-only state,
and nothing that can differ between the server render and hydration.

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/section_nav.rs`
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/item_view.rs` (`WorldMenu` at
  `:262`, `ItemView`'s view at `:1948-1955`)

- [ ] **Step 1: Create the nav component**

Create `ultros-frontend/ultros-app/src/components/section_nav.rs`:

```rust
use crate::i18n::{t_string, use_i18n};
use crate::routes::item_view_sections::Section;
use leptos::prelude::*;

/// Slim sticky bar for the item view: scope picker on the left, in-page jump
/// nav on the right.
///
/// Rendered below the full world menu in the DOM. `position: sticky` engages
/// only when the bar reaches the top of the viewport, so the world pills — ~30
/// crawlable links to sibling worlds — scroll away naturally and this takes
/// over without a scroll listener.
#[component]
pub fn SectionNav(children: Children) -> impl IntoView {
    let i18n = use_i18n();
    let label = move |section: Section| match section {
        Section::Overview => t_string!(i18n, item_view_nav_overview).to_string(),
        Section::Listings => t_string!(i18n, item_view_nav_listings).to_string(),
        Section::History => t_string!(i18n, item_view_nav_history).to_string(),
        Section::Sources => t_string!(i18n, item_view_nav_sources).to_string(),
        Section::Related => t_string!(i18n, item_view_nav_related).to_string(),
    };
    view! {
        <div class="sticky top-0 z-20 backdrop-blur bg-[color:color-mix(in_srgb,var(--color-background)_88%,transparent)] border-b border-[color:var(--color-outline)]">
            <div class="w-full px-3 sm:px-4 py-2 flex items-center gap-3 flex-wrap">
                {children()}
                <nav
                    aria-label=move || t_string!(i18n, item_view_nav_aria).to_string()
                    class="flex items-center gap-1 overflow-x-auto"
                >
                    {Section::ALL
                        .iter()
                        .map(|&section| {
                            view! {
                                <a
                                    href=section.href()
                                    class="whitespace-nowrap rounded-md px-2.5 py-1 text-sm text-brand-300 transition-colors hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] hover:text-brand-100"
                                >
                                    {label(section)}
                                </a>
                            }
                        })
                        .collect_view()}
                </nav>
            </div>
        </div>
    }
    .into_any()
}
```

Register it in `ultros-frontend/ultros-app/src/components/mod.rs`:

```rust
pub mod section_nav;
```

- [ ] **Step 2: Un-stick the world menu**

In `item_view.rs`, change `WorldMenu`'s wrapper (line 262) from:

```rust
        <div class="sticky top-0 z-10 backdrop-blur bg-[color:color-mix(in_srgb,var(--color-background)_85%,transparent)] border-y border-[color:var(--color-outline)]">
```

to:

```rust
        <div class="border-y border-[color:var(--color-outline)]">
```

The pills stay exactly as they are — same markup, same links, same order. They
simply scroll with the page now.

- [ ] **Step 3: Add the slim bar and the section anchors**

In `ItemView`'s view, replace the block at `item_view.rs:1948-1955`:

```rust
            <WorldMenu world_name=world item_id />

            <div class="main-content px-0 sm:px-4">
                <ListingsContent item_id world excluded_worlds />
                <div class="mt-6">
                    <RelatedItems item_id=Signal::from(item_id) />
                </div>
            </div>
```

with:

```rust
            <WorldMenu world_name=world item_id />

            <SectionNav>
                <span class="text-sm font-bold text-brand-200 whitespace-nowrap">
                    {move || Url::unescape(&world())}
                </span>
            </SectionNav>

            <div class="main-content px-0 sm:px-4">
                <ListingsContent item_id world excluded_worlds />
                <div id="related" class="scroll-mt-16 mt-6">
                    <RelatedItems item_id=Signal::from(item_id) />
                </div>
            </div>
```

Add `section_nav::SectionNav,` to the component import block at
`item_view.rs:10-14`.

The scope slot renders the current world name as text for now. Swapping it for
`<WorldPicker>` (`components/world_picker.rs:63`, already used by the item
explorer toolbar, alert drawers and lists) needs a navigating setter and is
deferred to Phase 2, where `?lens=` gives it a second reason to exist.

- [ ] **Step 4: Add the remaining anchors and scroll offsets**

In `ListingsContent`'s view (`item_view.rs:1707-1737`), wrap the overview
group and add `scroll-mt-16` to the existing anchors so the sticky bar does not
cover a heading after a jump.

Replace:

```rust
            <DecisionHeader listing_resource filtered_listings world />
            <div class="flex flex-col gap-4 sm:gap-6">
                <MarketStatsPanel
                    listing_resource
                    filtered_listings
                    item_id
                    realtime_status=realtime_status.into()
                    last_update_at=last_update_at.into()
                />
                <WorldMarketShare listing_resource filtered_listings world />
                <div id="history">
                    <ChartWrapper listing_resource filtered_listings item_id world />
                </div>
            </div>
```

with:

```rust
            <div id="overview" class="scroll-mt-16">
                <DecisionHeader listing_resource filtered_listings world />
                <MarketStatsPanel
                    listing_resource
                    filtered_listings
                    item_id
                    realtime_status=realtime_status.into()
                    last_update_at=last_update_at.into()
                />
            </div>
            <div id="history" class="scroll-mt-16 mt-4 sm:mt-6">
                <ChartWrapper listing_resource filtered_listings item_id world />
            </div>
```

`WorldMarketShare` is removed from here — Task 9 places it at the bottom.

- [ ] **Step 5: Verify it compiles and tests pass**

Run: `cargo test -p ultros-app --lib 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 6: Verify in the app**

Scroll the item page. Confirm: the world pills scroll away, the slim bar pins
itself at the top, each nav link jumps to the right section, and the target
heading is not hidden underneath the pinned bar.

- [ ] **Step 7: Commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-app/src
git commit -m "feat(item-view): two-tier sticky chrome with jump nav

The world pills stop being sticky and stay in flow, keeping ~30
crawlable sibling-world links in the page body. A slim bar below them
carries the in-page jump nav and pins itself via position:sticky as
they scroll past — no scroll listener, so nothing new can differ
between SSR and hydration on a route with a long history of tachys
hydration panics."
```

---

## Task 9: Move market share to the bottom

"Where is the supply" is a research question. It currently renders third,
above the chart.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/item_view.rs` (`ListingsContent`
  view)

- [ ] **Step 1: Add the sources anchor and place market share last**

In `ListingsContent`'s view, replace the trailing blocks:

```rust
            <div class="grid grid-cols-1 gap-6 mt-8">
                <SalesDetails listing_resource />
            </div>

            <div class="mt-6 mx-auto">
                <Ad class="h-[336px] w-[280px] rounded-xl overflow-hidden" />
            </div>
```

with:

```rust
            <div class="grid grid-cols-1 gap-6 mt-8">
                <SalesDetails listing_resource />
            </div>

            <div class="mt-6">
                <WorldMarketShare listing_resource filtered_listings world />
            </div>

            <div class="mt-6 mx-auto">
                <Ad class="h-[336px] w-[280px] rounded-xl overflow-hidden" />
            </div>
```

- [ ] **Step 2: Add the sources anchor**

The `#sources` nav link needs a target. In `components/related_items.rs`,
change the wrapper of the vendor/exchange/leve grid at line 1016 from:

```rust
            <div class="grid grid-cols-1 lg:grid-cols-2 xl:grid-cols-3 gap-6 empty:hidden">
```

to:

```rust
            <div id="sources" class="scroll-mt-16 grid grid-cols-1 lg:grid-cols-2 xl:grid-cols-3 gap-6 empty:hidden">
```

The inner `#vendor-sources`, `#exchange-sources` and `#leve-sources` ids stay —
`MarketStatsPanel`'s source callout links to them directly.

- [ ] **Step 3: Verify it compiles and tests pass**

Run: `cargo test -p ultros-app --lib 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 4: Verify in the app**

Open an item on a datacenter or region scope (market share hides on a single
world). Confirm it now renders below the sale history, and that the `#sources`
nav link lands on the vendor/exchange/leve row.

- [ ] **Step 5: Commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-app/src
git commit -m "refactor(item-view): move market share below sale history

Per-world supply distribution is a research question, not something
every visitor should scroll past on the way to the chart."
```

---

## Task 10: Full-page verification

**Files:** none

- [ ] **Step 1: Run the whole suite**

Run: `cargo test -p ultros-app --lib 2>&1 | tail -20`
Expected: PASS, no ignored failures.

- [ ] **Step 2: Run CI checks**

Run: `./check_ci.sh`
Expected: clean — no fmt diff, no clippy warnings.

- [ ] **Step 3: Confirm locale parity**

Run:

```bash
cd ultros-frontend/ultros-app && python3 -c "
import json
ls=['en','fr','de','ja','cn','ko','tc']
ds={l:set(json.load(open(f'locales/{l}.json'))) for l in ls}
base=ds['en']
for l in ls:
    assert ds[l]==base, (l, sorted(base^ds[l]))
print('locale parity OK:', len(base), 'keys')
"
```

Expected: `locale parity OK: 1520 keys`

- [ ] **Step 4: Walk the page against the four reported problems**

On an item with many listings, on a datacenter scope:

1. Stat names render in full — no "Vi…".
2. One listings table; All/HQ/NQ filters it; "Show more" scrolls inside the box
   and does not grow the page.
3. The slim bar pins after the world pills scroll away, and every nav link
   lands on its section without the heading hidden under the bar.
4. Market share renders near the bottom, below the sale history.

Also confirm no console errors on load or after a world switch — this route's
hydration failures surface as `RuntimeError: unreachable` in the browser
console.

- [ ] **Step 5: No commit**

Verification only.

---

## Deferred to Phase 2

- `?lens=` parsing and the CSS `order` map.
- Per-lens verdict hero (merging `DecisionHeader` and `MarketStatsPanel`).
- Swapping the `SectionNav` scope slot for a live `<WorldPicker>`.
- Scrollspy highlighting of the active nav link. Deliberately omitted here: it
  needs an `IntersectionObserver` behind a client-only guard, and Phase 1 should
  not add client-only reactive state to this route without a stronger reason.

## Deferred to Phase 3

- Bulk basket, undercut wall, days of stock, `launder_suspicion`, VWAP vs spot.
