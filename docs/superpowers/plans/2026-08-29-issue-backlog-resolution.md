# Plan: Issue backlog resolution (2026-08-29)

Resolve the open UI/feature issue backlog: #1151, #1127, #1080, #1129, #1130, #1131, #1132.
Each task is one PR, branched from latest `origin/main`, reviewed, then merged before the
next task starts (stacked PRs get zero CI in this repo — every PR must base on `main`).

## Global Constraints

- **i18n**: every user-facing string goes through `leptos-i18n` — `t!(i18n, key)` / `t_string!` for attributes. New keys must be added to ALL locale files in `ultros-frontend/ultros-app/locales/` (`en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`) with real translations, snake_case keys, feature-prefixed.
- **CI gate**: before every commit run `./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log` from the repo root and require REAL_EXIT=0 (`cargo fmt --all` to autofix formatting). Exit 137 = clippy OOM, re-run `cargo clippy --all-targets -j 2 -- -D warnings`.
- **Query-param signals**: URL-backed *filter* state must use `filter_query_signal` (see `ultros-frontend/ultros-app/src/global_state/`), never plain `query_signal` — plain query_signal scrolls to top and spams history on every keystroke. When converting a page, audit EVERY `query_signal` left on it, not just the ones you touch.
- **Range filters are inclusive** (`>=`/`<=`) and labeled `≥`/`≤`. Never exclusive comparisons behind an inclusive label.
- **FilterChip props trap**: `FilterChip`'s `min`/`max`/`step` are `optional, into` String props — you cannot pass an `Option`; an optional bound needs an `Either` branch that omits the prop entirely.
- **No hardcoded palette colors** that break light mode (e.g. `bg-gray-700/50`); use the CSS token classes (`.panel`, `--color-*` vars) and the skeleton components in `components/skeleton.rs`.
- **Leptos class toggles**: `class=("name", sig)` only ADDS the class when true; it does not remove a statically-listed copy.
- **Dependency ceilings**: leptos pinned 0.8.20, web-sys capped 0.3.103 (`web_sys` mouse/scroll getters return f64 on wasm).
- **Changelog**: each merged PR that changes something a player can see gets an entry appended at the TOP of the changelog page data (`ultros-frontend/ultros-app/src/routes/changelog.rs` or wherever the entries table lives — find it; current through 2026-08-23).
- **Commits**: end commit messages with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`. Conventional-commit style subjects (`feat(...)`, `fix(...)`, `refactor(...)`).
- **Do not delete or modify** `docs/superpowers/` content from other efforts.
- Legacy CSS: never add rules to `legacy.css` (dead file); shared CSS goes in `style/tailwind.css`.

## Task 1: #1151 — members can leave a group + groups.rs light-mode pulse fix

**Branch**: `claude/leave-group-ui` off `origin/main`. Closes #1151, part of #1132.

The backend `remove_group_member` (`ultros-db/src/lists.rs` ~801) already permits
self-removal (owner OR the member themselves). The UI gap: in
`ultros-frontend/ultros-app/src/routes/groups.rs` (~line 531) the remove-member button
only renders when `group.owner_id == member_id` of the *viewer* (`is_owner`). Verify how
the existing remove action at groups.rs:442 (`remove_group_member(*group_id, *user_id)`)
is wired (server fn / api call) and reuse it.

Requirements:
1. A non-owner member viewing a group they belong to gets a "Leave group" control (their
   own row or the group card — follow the existing layout). It calls the same removal
   path with their own user id, then refreshes the groups resource.
2. The owner does NOT get a leave button for themselves (owner exits via delete/transfer —
   out of scope). Keep the owner's existing per-member remove buttons unchanged.
3. Confirm dialog or two-step confirm consistent with existing destructive actions on the
   page (match whatever the delete-group flow does; if there is no confirm pattern, a
   simple confirm step via `Dismissable`/modal is fine).
4. New i18n keys (e.g. `group_leave_button`, `group_leave_confirm`) in all 7 locales.
5. Same PR, separate commit: fix the two light-mode-breaking skeleton fallbacks at
   groups.rs:519 and groups.rs:694 — `animate-pulse h-8 bg-gray-700/50 rounded` → use the
   shared skeleton components (`BoxSkeleton` or equivalent from `components/skeleton.rs`)
   or token-based classes. (This is the groups.rs item from #1132.)
6. Changelog entry (player-visible: "You can now leave a group you've joined").
7. Tests: if `remove_group_member` self-removal has no test in `ultros-db`, add one
   following the existing test patterns in `lists.rs`; UI-level tests not required.

## Task 2: #1127a — convert the four analyzer routes off `Toolbar`

**Branch**: `claude/toolbar-to-controlbar-analyzers` off `origin/main`. Part of #1127.

Convert `routes/venture_analyzer.rs`, `routes/leve_analyzer.rs`,
`routes/recipe_analyzer.rs`, `routes/fc_crafting_analyzer.rs` from the old
`components/toolbar.rs` idiom (`Toolbar`/`ToolbarField`/`ToolbarPills`/`ToolbarSpacer`)
to the shared sticky control bar (`components/control_bar.rs` + `components/filter_chip.rs`),
matching the flip finder (`routes/analyzer.rs`) and currency exchange
(`routes/currency_exchange.rs`) as reference implementations — study both before writing.

Requirements per route:
1. Replace the `Toolbar` filter rows with `ControlBar` + `FilterChip`s (numeric ranges as
   chips with `≥`/`≤`, toggles/selects per how analyzer.rs does them). Keep every existing
   filter's query-param KEY verbatim — deep links must not break. Keep behavior inclusive.
2. Uncommitted-filter UX: use the `pending_filter: RwSignal<Option<&'static str>>` +
   `start_editing` pattern (see currency_exchange.rs) rather than seeding defaults into
   the URL.
3. Audit every remaining `query_signal` on the page; migrate filter-like ones to
   `filter_query_signal`.
4. Reuse existing i18n keys where the label text is unchanged; add new keys to all 7
   locales only where genuinely new text appears.
5. Keep the pages' existing sort controls/skeletons untouched (out of scope here).
6. Do NOT delete `components/toolbar.rs` yet (Task 3 does).
7. Changelog entry (one line covering the four analyzers' new filter bar).

## Task 3: #1127b — convert the remaining Toolbar routes and delete toolbar.rs

**Branch**: `claude/toolbar-to-controlbar-rest` off `origin/main`. Closes #1127.

Convert `routes/scrip_sources.rs`, `routes/trends.rs`, `routes/vendor_resale.rs`,
`routes/job_set_detail.rs` the same way as Task 2 (same requirements 1–4), then delete
`components/toolbar.rs` and its `mod`/re-exports. `components/price_history_chart.rs`
also imports `toolbar::` — check what it uses (likely `ToolbarPills` or similar) and
either inline a local equivalent or move that piece to a neutral shared component; do not
keep toolbar.rs alive for one consumer.

Notes:
- `scrip_sources.rs` is described as closest to the target already.
- `vendor_resale.rs`: while there, migrate its filters to `filter_query_signal` (known
  outstanding page for the history-spam bug).
- Deleting a shared component surfaces `unused_imports` in lib.rs glob re-exports and
  `dead_code` on props — check both before calling clippy green.
- Comment on #1127 is not needed; PR body says "Closes #1127".
- Changelog entry.

## Task 4: #1080 — extract `components/data_table.rs`

**Branch**: `claude/data-table-extraction` off `origin/main`. Closes #1080.

Extract a reusable sortable data table and port the two existing hand-copies onto it.
Read issue #1080 body for full context. Current state: the sort machinery is ALREADY
shared (`components/sort_header.rs`: `SortHeader`, `SortableHeaderCell`,
`sort_and_truncate`, `cmp_none_last`) — this task is only the table itself.

Requirements:
1. New `components/data_table.rs`: columns described once as data (label/i18n, grid
   width, alignment, sortable-or-not, cell renderer closure), header row and body rows
   derive their grid template from that single description. Substrate decision: the
   explorer uses a `role="table"` div grid, currency exchange a real `<table>` — pick ONE
   (prefer the div grid: it's what the explorer's virtualized rows already use; confirm
   against how `VirtualScroller` composes) and document the choice in the component docs.
2. Port the item explorer's `ItemList` (`routes/item_explorer.rs:563`, grid strings at
   ~779/781/832/834) onto it, eliminating the four hand-matched grid-template strings.
   Preserve the explorer-specific context reads (`CheapestPrices`, `ExplorerPriceScope`)
   in the explorer's cell renderers — the shared component itself must not depend on
   explorer contexts and must not `.expect()` on missing context (see the `hydrated`
   gate comment near item_explorer.rs:684 — preserve hydration behavior exactly).
3. Port the currency exchange results table (`routes/currency_exchange.rs`) onto it,
   preserving its `?cols=` column toggling, sort keys, and the `.hscroll-fade` scrollport.
4. Zero visual change intended on both pages; zero URL-contract change (the currency
   exchange range-key test `range_filter_keys_are_a_stable_url_contract` must still pass).
5. This is the hydration-risk task: SSR/CSR render order must stay deterministic (no
   HashMap iteration order reaching the DOM). Test hydration locally if feasible;
   otherwise flag in the PR body.
6. No changelog entry (pure refactor).

## Task 5: #1129 — rebuild Retainers on the kit

**Branch**: `claude/retainers-kit-rebuild` off `origin/main`. Closes #1129.

`routes/retainers.rs`: two raw `<table>` blocks (~121, ~207) on legacy element selectors,
no page `<h1>`, `<Loading />` spinners at ~305/309/387/504, one lone `BoxSkeleton`.

1. Add `ToolHeader` (help link optional if the component requires one — make it optional
   rather than pointing at a nonexistent help page).
2. Restyle both tables onto the shared data table from Task 4 (or, if row shape doesn't
   fit, the kit's table conventions à la currency exchange — reviewer should push back if
   a hand-copy of grid strings reappears).
3. Replace `<Loading />` spinners with `TableSkeleton`/`BoxSkeleton` matching final layout.
4. `routes/edit_retainers.rs` (or wherever edit lives) only if the issue text includes it —
   check the issue; otherwise leave.
5. i18n for any new heading strings, all 7 locales. Changelog entry (visual refresh).

## Task 6: #1130 — rebuild Alerts on the kit, merge the two drawers

**Branch**: `claude/alerts-kit-rebuild` off `origin/main`. Closes #1130.

1. `routes/alerts.rs:34` raw `<h1>` → `ToolHeader`.
2. `components/alert_rules_panel.rs:121` raw `<table>` → kit table conventions (shared
   data table if it fits).
3. Replace `<Loading />` fallbacks with skeletons in `components/endpoints_panel.rs`,
   `history_panel.rs`.
4. Merge `components/alert_drawer.rs` and `components/alert_config_drawer.rs` — same
   `<Modal>`, same fields, same `alert_drawer_*` i18n keys duplicated. One component
   survives; update all call sites; delete the other; prune orphaned i18n keys from ALL
   locale files (build warns per-locale on missing, and orphans linger silently — grep).
5. Changelog entry.

## Task 7: #1131 — rebuild Lists / List View on the kit

**Branch**: `claude/lists-kit-rebuild` off `origin/main`. Closes #1131.

1. Raw `<h1 class="text-3xl font-bold">` at `routes/lists.rs:419`,
   `routes/list_view.rs:1100`, `list_view.rs:1176` → `ToolHeader` (help link optional).
2. Raw `<table class="w-full min-w-[760px] text-sm">` at `list_view.rs:1402` → shared
   data table / kit conventions.
3. `.list-toolbar` CSS family (style/tailwind.css ~1399-1421, verify) → converge on the
   shared control-bar/`.sticky-bar` styles; delete the private CSS family.
4. `<Loading />` fallbacks → skeletons.
5. Changelog entry.

## Task 8: #1132 — hygiene sweep (remainder)

**Branch**: `claude/ui-kit-hygiene` off `origin/main`. Closes #1132 (and #1133 once merged).

Re-read issue #1132 body first — earlier tasks already fixed some sites (groups.rs in
Task 1; possibly others). Fix what remains:
1. Inline `animate-pulse` text fallbacks in the four analyzers (venture ~573, leve ~651,
   recipe ~1130, fc_crafting ~699) → proper skeletons sized to the content they replace.
2. Hand-written chart placeholder `animate-pulse panel h-[26rem]` at `routes/item_view.rs`
   ~1591 → `BoxSkeleton` equivalent.
3. Everything else the issue body lists (surface-token cleanup, small pages) — enumerate
   the issue's bullets, verify each against current code, fix or note "already fixed by
   #NNNN" in the PR body.
4. Changelog entry only if visible; otherwise none.
5. After merge the umbrella #1133 should have all phases closed — close #1133 with a
   summary comment.
