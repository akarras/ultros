# Item Page Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Put marketboard data first on the item page: remove the Discord chip, collapse item stats into an accordion, compact the Cheapest Found card into a strip, and move the listings + sale-history tables above the chart (side by side at xl+).

**Architecture:** All changes live in `ultros-frontend/ultros-app/src/routes/item_view.rs` (plus locale files). Components are reordered and restyled; resources, realtime subscriptions, and Suspense structure untouched. Separate branch off `main` (NOT stacked on the tool-header branch — stacked PRs get zero CI).

**Tech Stack:** Rust / Leptos 0.8, Tailwind, leptos-i18n (all 7 locales).

## Global Constraints

- New i18n keys go into **every** locale file (`en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`) with real translations.
- Accordion default state must be static (collapsed) — SSR-deterministic, no hydration mismatch.
- Side-by-side breakpoint is `xl:` (NOT `lg:` — sidebar eats that width); grid columns `minmax(0,1fr)`.
- Anchor ids `#overview` / `#listings` / `#history` / `#related` keep existing names.
- Spec: `docs/superpowers/specs/2026-08-29-item-page-refactor-design.md`.
- `./check_ci.sh` before every commit (`REAL_EXIT` check).

---

### Task 1: Remove `DiscordCommandChip`

**Files:**
- Modify: `item_view.rs` — delete the call (~line 2200-2206) and the component (~line 1954-1992).
- Modify: locale files — remove keys used only by the chip (grep each key first; `discord` keys used by `/bot`, about page, side_nav stay).

- [ ] **Step 1:** Delete the `<div class="mt-1.5"><DiscordCommandChip .../></div>` block and the `DiscordCommandChip` component fn. `cargo check -p ultros-app` — fix any now-unused imports (e.g. clipboard helpers if the chip was the last user).
- [ ] **Step 2:** Grep the i18n keys the chip used (look at its body first); delete from all 7 locales only those with no other callers.
- [ ] **Step 3:** `./check_ci.sh`; commit `refactor(item): drop the Discord command chip from the item header`.

### Task 2: Item details accordion

**Files:**
- Modify: `item_view.rs:2233-2241` (the item-level + `ItemStats` grid).
- Modify: all 7 locale files — add `item_view_item_details` ("Item details" / "Détails de l'objet" / "Gegenstandsdetails" / "アイテム詳細" / "物品详情" / "아이템 상세" / "物品詳情").

**Interfaces:**
- Produces: collapsed-by-default disclosure containing the existing item-level badge + `<ItemStats>`.

- [ ] **Step 1:** Wrap the existing grid in a native `<details>` element (SSR-safe, no JS):

```rust
<details class="pt-3 border-t border-[color:var(--color-outline)] group">
    <summary class="cursor-pointer text-sm font-semibold text-brand-300 hover:text-[color:var(--brand-fg)] list-none flex items-center gap-2">
        <Icon icon=i::BsChevronRight width="0.8em" height="0.8em" attr:class="transition-transform group-open:rotate-90" />
        {t!(i18n, item_view_item_details)}
    </summary>
    // existing item-level + ItemStats grid, unchanged, inside
</details>
```

Check `group-open:` works with the repo's Tailwind version; if not, use plain CSS `details[open] .chevron { transform: rotate(90deg); }` in the component or an existing pattern (`ResultBreakdownDisclosure` in `tool_help.rs` uses bare `<details>` — mirror it).

- [ ] **Step 2:** Add the i18n key to all 7 locales. `cargo check -p ultros-app` (leptos-i18n fails the build on a missing locale).
- [ ] **Step 3:** `./check_ci.sh`; commit `refactor(item): tuck item stats behind an Item details accordion`.

### Task 3: Compact Cheapest Found strip

**Files:**
- Modify: `item_view.rs:782-1217` (`MarketStatsPanel`).

- [ ] **Step 1:** Read the whole component first. Restyle — do not change any data reads/Memos. Target: one `panel` row (`flex flex-wrap items-center gap-x-5 gap-y-2 px-4 py-3`) containing: "Cheapest found" label + Live badge, HQ chip, NQ chip, real-price chip, listings-count + velocity chip. Each chip = `<span>` with muted small label + value, no sub-cards.
- [ ] **Step 2:** Crafting recipe sub-card → inline chip: icon + `{t!(i18n, crafting_recipe)}` + "~62,368" linking where the old card linked. Reuse existing i18n keys; add none if avoidable.
- [ ] **Step 3:** `./check_ci.sh`; commit `refactor(item): compact the Cheapest Found panel into a stat strip`.

### Task 4: Reorder sections + side-by-side tables

**Files:**
- Modify: `item_view.rs:1910-1951` (`ListingsContent` view) and the `SectionNav` link order (find where Overview/Listings/History/Sources/Related links are defined — `section_nav.rs` or the item page).

- [ ] **Step 1:** Reorder `ListingsContent`:

```rust
<div id="overview" class="scroll-mt-16">
    <FlipRouteCard .../>
    <DecisionHeader .../>
    <MarketStatsPanel .../>
</div>
<div class="grid grid-cols-1 xl:grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-6 mt-6">
    <div id="listings" class="scroll-mt-16 min-w-0">
        <ListingsPanel .../>
    </div>
    <div id="history" class="scroll-mt-16 min-w-0">
        <SalesDetails .../>
    </div>
</div>
<div class="mt-6"><ChartWrapper .../></div>
<div class="mt-6"><WorldMarketShare .../></div>
// Ad unchanged
```

Note: `#history` moves onto the sales-table container; the chart follows full-width right below, satisfying "history = table first, chart after".

- [ ] **Step 2:** Update `SectionNav` order to Overview → Listings → History → Related (match visual order). Verify each table keeps its own `overflow-x-auto` inside the half-width column; add `min-w-0` wrappers as above.
- [ ] **Step 3:** Watch for popover clipping: if `ListingsPanel` hosts dropdown menus (datacenter exclusion), confirm they still escape — the known fix pattern is in `reference_overflow_x_auto_popover_clip`.
- [ ] **Step 4:** `./check_ci.sh`; commit `refactor(item): listings and sale history above the chart, side by side at xl`.

### Task 5: Visual verification + PR

- [ ] **Step 1:** Serve locally; verify at 375px / 768px / 1280px / wide: accordion collapsed on load and toggles; strip wraps sanely on mobile; tables side by side only at xl+; anchors + SectionNav scroll correctly; realtime updates still land (watch a busy item).
- [ ] **Step 2:** Changelog entry at TOP: "Item pages: market tables now come first; item stats moved into a collapsible section."
- [ ] **Step 3:** Push branch (off `main`), open PR with before/after screenshots.
