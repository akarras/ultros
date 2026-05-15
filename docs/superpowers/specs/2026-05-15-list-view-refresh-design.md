# List View Refresh — Design

**Date:** 2026-05-15
**Scope:** `ultros-frontend/ultros-app/src/routes/list_view.rs` and its supporting components.

## Problem

The `/list/{id}` page has several gaps:

1. **Visual button mismatch.** `Add Item` (`btn-primary`) renders correctly, but `Add Recipe`, `Make Place`, `Notify me`, and `Purchasing View` (all `btn-secondary`) render as oversized rounded pills with `text-xl font-extrabold` children. The cause is a legacy plain `.btn-secondary` CSS block at `style/tailwind.css:865-882` that overrides the slim `@utility btn-secondary` at line 1279.
2. **No list-state controls.** The top-level `/list` page (`routes/lists.rs`) exposes rename, world, share, delete, and leave-list affordances inside `ListCard`. The list-detail page exposes none of them — a user landing directly on `/list/{id}` cannot rename, share, or delete the list.
3. **Live-update affordance is weak.** The websocket subscription works (refetches on `ListUpdate`/`Stale`, drives the activity feed), but the only visual cue is a small text pill that reads `Connecting` → `Live` → `Reconnecting`. There's no signal when a remote change just arrived, and the share/settings flows we add must also respond to remote updates.
4. **Permissions** are partially enforced (bulk-edit/delete use `can_write`) but the page lacks the owner-vs-write-vs-read affordance gating that `ListCard` has.
5. **Acquired state is not obvious.** Per-row, `acquired / quantity` and the progress bar only render when `quantity > 1` (`list_item_row.rs:121-140`), so single-quantity items show no acquired state at all. The page-level "Done" stat counts rows where `remaining == 0`, ignoring partial progress — misleading when a 100-quantity item is 26% done.

## Goals

- Consistent, compact button styling across the list-view header.
- Owners can rename, change world, share (users/groups/invites), and delete the list directly from `/list/{id}`. Non-owners can leave.
- Clear visual indication of live state and incoming remote changes.
- Every row makes acquired progress visible at a glance, regardless of quantity.
- Page-level summary reflects unit-level progress, not just row-level completion.
- Permission gates: hide owner-only affordances from non-owners; hide write-only affordances from read-only viewers.

## Non-goals

- Reworking the buying view (`BuyingView` component) — separate concern.
- Removing the legacy `.btn-secondary` plain CSS block — touches every secondary button in the app and warrants its own audit.
- Per-item granular WS updates (would require a new server message shape). Full refetch on `ListUpdate` is fine at current scale.
- New translation pipeline — every user-facing string still flows through `leptos-i18n` with keys in all 7 locale files per `CLAUDE.md`.

## Design

### 1. Header restructure

Replace the current two-row mixed-style header with a structured panel:

```
┌─────────────────────────────────────────────────────────────────────────┐
│  📋 test                                          [● Live]   [⚙ Settings]│
│     Aether · 4 items · 26 / 100 units acquired (26%)                     │
│     ████████░░░░░░░░░░░░░░░░░░░░                                         │
│                                                                          │
│  [+ Add Item]  [+ Recipe]  [↓ Make Place]    [🛒 Purchasing] [🔔 Notify] │
└─────────────────────────────────────────────────────────────────────────┘
```

**Components:**

- **Title row:** list name (large), inline-editable for owners via pencil affordance. World badge to the right of the name. A right-aligned cluster holds the Live indicator and the Settings button.
- **Summary row:** "{world} · {N} items · {acquired} / {quantity} units acquired ({pct}%)" plus a thin progress bar. When `quantity == 0` (empty list), the units string is omitted.
- **Action row:** primary `+ Add Item` plus secondary `+ Recipe`, `↓ Make Place` on the left; `🛒 Purchasing` toggle and `🔔 Notify` on the right.

**Styling fix without touching the legacy CSS:** introduce a scoped wrapper class `list-toolbar` on the action row that applies `[&_.btn-secondary]:rounded-lg [&_.btn-secondary]:px-3 [&_.btn-secondary]:py-2 [&_.btn-secondary]:text-sm [&_.btn-secondary]:font-medium [&_.btn-secondary>*]:text-sm [&_.btn-secondary>*]:font-medium` (or equivalent component-level overrides). This keeps the legacy global rule intact for the rest of the app while restoring consistency on this page. Same wrapper goes on the right cluster.

**Live indicator:** replaces the text pill with a `<span>` containing a colored dot:

- `Live` → green dot (`bg-green-400`)
- `Connecting` → amber pulse
- `Reconnecting` → amber pulse
- `Offline` → muted gray dot

Hovering the dot shows a tooltip with the full state text plus "Updated Xs ago" (driven by a `last_update_at` signal — see §3).

### 2. List settings drawer

Add `components/list/list_settings_drawer.rs`. The drawer is wider than a modal (similar to existing alert drawer) and contains four sections in this order:

1. **Details** (owner only) — name input, `WorldPicker`. Save dispatches `edit_list` action.
2. **Sharing** (owner only) — embeds the existing share UI extracted from `routes/lists.rs::ShareListModal`. Includes the "Add people", "Share via link" (invites), and "Who has access" sections, exactly as on the top-level page.
3. **Activity** (everyone) — short link to the existing activity feed; the feed itself stays at the bottom of the page (not duplicated in the drawer).
4. **Danger zone** — owner sees `Delete list`; non-owner sees `Leave list`. Confirmation lives inline (red secondary button → red primary button on second click). On success, navigate back to `/list`.

**Extracting `ShareListModal`:**

- Move `ShareListModal`, `AccessList`, `AccessRow`, and the helper functions `permission_label`, `editable_permission`, `invite_url`, `copy_invite_url`, `invite_uses_label` from `routes/lists.rs` into a new module `components/list/share_list_modal.rs`.
- Export `ShareListModal` and a new `ShareListSection` (the body of the modal without the `Modal` wrapper) for embedding inside the drawer.
- `routes/lists.rs` continues to use `ShareListModal` from the new location — no behavior change for the top-level page.

**Drawer trigger:** the `⚙ Settings` button in the header opens the drawer. The button is hidden entirely for read-only users *unless* they need the danger-zone "Leave list" affordance — in which case the drawer label becomes "List options" and only the Danger zone section renders.

### 3. Live updates

The websocket plumbing stays. Add:

- A `last_update_at: RwSignal<Option<DateTime<Utc>>>` updated every time `list_view.refetch()` is called from a WS handler or completes after one.
- The Live tooltip reads "Updated Xs ago" derived from `last_update_at`, using a 1-second ticker (or computed on hover; ticker is simpler).
- A `recently_changed: RwSignal<HashSet<i32>>` tracking item IDs whose `acquired` or `quantity` changed in the most recent refetch. After a refetch, diff against the previous snapshot, populate the set, and clear it after a 1.5s timeout (`gloo_timers::callback::Timeout`).
- `ListItemRow` reads `recently_changed.with(|set| set.contains(&self.id))` and adds a temporary `bg-brand-900/20` class on `<tr>` when true. The fade-out uses a CSS transition (`transition-colors duration-1000`).
- The settings drawer's share-data `Resource` (currently versioned by action signals only) takes a new dependency on `last_update_at` so remote `shared_user` / `invite_used` events refresh the access list while the drawer is open.

### 4. Permissions

Compute `permission` once from `list.permission` at the top of the view, then gate affordances:

| Affordance                 | Owner | Write | Read |
|----------------------------|:-----:|:-----:|:----:|
| Add Item, Recipe, MakePlace | ✓     | ✓     | hidden |
| Bulk edit / delete items    | ✓     | ✓     | hidden |
| Mark acquired (row)         | ✓     | ✓     | hidden |
| Settings → Details          | ✓     | hidden | hidden |
| Settings → Sharing          | ✓     | hidden | hidden |
| Settings → Delete list      | ✓     | hidden | hidden |
| Settings → Leave list       | hidden | ✓     | ✓    |
| Notify me, Purchasing View  | ✓     | ✓     | ✓    |
| Live indicator              | ✓     | ✓     | ✓    |

Implementation: helper `view_caps(permission) -> ViewCaps { can_write, can_admin, can_leave, ... }` returned as a `Memo`. Each affordance reads from the memo. Read-only viewers see a clean view + Notify + Purchasing + (if shared with them) Leave option.

Existing `disabled=move || !can_write` on bulk edit becomes a hide rather than disable.

### 5. Acquired indicator

**Page header progress:**

Compute totals from the items list:

```rust
let total_quantity: i32 = items.iter().map(|(i, _)| i.quantity.unwrap_or(1)).sum();
let total_acquired: i32 = items.iter()
    .map(|(i, _)| i.acquired.unwrap_or(0).min(i.quantity.unwrap_or(1)))
    .sum();
let pct = if total_quantity > 0 { 100 * total_acquired / total_quantity } else { 0 };
```

Render: "{total_acquired} / {total_quantity} units acquired ({pct}%)" plus a thin progress bar (`<progress class="h-2 w-full">`). When `total_quantity == 0`, render "No items yet".

**Stat tiles:**

Keep the existing three tiles. Rename "Done" → "Acquired" (or `list_view_acquired` key). Semantics unchanged: number of rows with `remaining == 0`. Add a tooltip on the Acquired tile: "{n} of {total} items fully acquired".

**Per-row indicator (always visible):**

Update `list_item_row.rs:120-141` so the Quantity cell always renders `{a} / {q}` plus a small progress bar, even when `q == 1` (e.g., `0 / 1` empty bar → `1 / 1` full bar). The progress element gets a stable height (`h-2`) to avoid layout shift. Column header changes from "Quantity" to "Acquired / Quantity" (key `list_view_acquired_quantity`).

**Completed-row treatment:**

When `remaining == 0`, the `<tr>` gets `bg-green-900/15` and the HQ column shows a small `Icon icon=i::BiCheckCircle` in green. The mark-acquired button toggles back (currently it always sets `acquired = quantity` — change to toggle: if `acquired >= quantity`, set to 0; else set to quantity). Tooltip becomes context-aware: "Mark acquired" / "Mark unacquired".

### 6. i18n keys

New keys to add to all 7 locale files (`en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`):

- `list_view_settings` — "Settings"
- `list_view_list_options` — "List options" (drawer title for non-owners)
- `list_view_settings_details_heading` — "Details"
- `list_view_settings_danger_zone` — "Danger zone"
- `list_view_acquired` — "Acquired" (replaces the existing `list_view_done` usage in this view)
- `list_view_acquired_quantity` — "Acquired / Quantity"
- `list_view_units_acquired_progress` — "{acquired} / {quantity} units acquired ({pct}%)"
- `list_view_no_items_yet` — "No items yet"
- `list_view_mark_unacquired` — "Mark unacquired"
- `list_view_live_status_live` / `_connecting` / `_reconnecting` / `_offline`
- `list_view_updated_seconds_ago` — "Updated {seconds}s ago"
- `list_view_completed_row_aria` — "Item fully acquired"
- `list_view_acquired_items_tooltip` — "{count} of {total} items fully acquired"

The existing `list_view_done` key stays (used elsewhere or for back-compat); the list view uses `list_view_acquired` going forward.

## File changes

**New files:**

- `ultros-frontend/ultros-app/src/components/list/share_list_modal.rs` — extracted from `routes/lists.rs`. Exports `ShareListModal` and `ShareListSection`.
- `ultros-frontend/ultros-app/src/components/list/list_settings_drawer.rs` — new drawer with Details / Sharing / Danger zone sections.

**Modified files:**

- `ultros-frontend/ultros-app/src/routes/list_view.rs` — header restructure, live indicator, recently-changed tracking, settings button, permission helper. (Buying-view branch left alone.)
- `ultros-frontend/ultros-app/src/components/list/list_item_row.rs` — always-visible acquired/quantity + progress bar; completed-row styling; toggle behavior on mark-acquired.
- `ultros-frontend/ultros-app/src/components/list/mod.rs` — re-export the new modules.
- `ultros-frontend/ultros-app/src/routes/lists.rs` — replace inline `ShareListModal` with import from the new module.
- `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json` — add the new keys above.

## Validation

- `./check_ci.sh` (fmt + clippy) must pass.
- Manual smoke (per `CLAUDE.md` UI testing rule): start `cargo run`, sign in, exercise the list page with an owned list and a shared (write) list:
  1. Open `/list/{id}`. Header renders clean, all buttons same height.
  2. Click `+ Add Item`, add an item — row appears, progress shows `0 / 1` with empty bar.
  3. Click mark-acquired — row goes green, progress shows `1 / 1`, page summary increments.
  4. Click again — row returns to default, summary decrements.
  5. Open Settings drawer — rename works, share form works (create invite, copy link), delete shows confirm.
  6. As a non-owner (write permission): Settings shows only Leave option.
  7. As a read-only viewer: no Settings, no Add buttons, no edit affordance on rows.
  8. Open two tabs as different users sharing the list; change in tab A appears in tab B within ~1s with a brief row highlight; Live tooltip shows "Updated <small> s ago".
- `cargo test -p ultros-app` to ensure no test regression. (Existing tests are minimal — most coverage is via the e2e harness in `integration/`.)

## Risks & rollback

- **Risk:** extracting `ShareListModal` could regress the top-level `/list` page if the import is wrong. Mitigation: smoke test that page too after the move.
- **Risk:** the row-highlight signal could flicker on rapid WS updates. Mitigation: 1.5s timeout; subsequent updates reset the timer.
- **Rollback:** the work is contained to the listed files. Revert via a single commit; no schema or API changes.

## Out of scope (follow-ups)

- Audit and consolidate the legacy `.btn-secondary` plain CSS at `style/tailwind.css:865-882` against the `@utility btn-secondary` at line 1279. Likely candidates for global fix: nav menus, lists nav, settings panels. Needs a sweep across `ultros-frontend/ultros-app/src/components`.
- Granular per-item WS messages (server-side) so we can update individual rows without a full list refetch.
- An "acquired" filter/sort on the table — once we surface per-row state more prominently, filtering by acquired/remaining becomes more obviously useful.
