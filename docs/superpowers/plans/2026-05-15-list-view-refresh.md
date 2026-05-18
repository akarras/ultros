# List View Refresh Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refresh `/list/{id}` with consistent toolbar styling, owner-only rename/share/delete controls in a new settings drawer, live-update visual polish, permission gating, and always-visible per-row acquired indicators.

**Architecture:** Extract `ShareListModal` from `routes/lists.rs` so both the top-level lists page and the new list-detail settings drawer can reuse it. Add a `ListSettingsDrawer` component with sections gated by `ListPermission`. Update `list_item_row.rs` to always render `acquired/quantity`. Add a `recently_changed` signal and a `last_update_at` clock to give WS-driven refreshes visible feedback. Scope CSS overrides through a `list-toolbar` wrapper so we avoid touching the global legacy `.btn-secondary` rules. New i18n keys land in all 7 locale files. A new `integration/list-flow.cjs` Puppeteer harness exercises the full UI flow against `--features test-auth`.

**Tech Stack:** Rust + Leptos 0.7 (CSR with hydrate), Tailwind v4 utilities, `leptos-i18n`, Puppeteer (Node `.cjs`), websocket client at `src/ws/realtime.rs`.

**Spec:** [`docs/superpowers/specs/2026-05-15-list-view-refresh-design.md`](../specs/2026-05-15-list-view-refresh-design.md)

---

## File Map

**Create:**
- `ultros-frontend/ultros-app/src/components/list/share_list_modal.rs` — extracted `ShareListModal`, `ShareListSection`, `AccessList`, `AccessRow`, and helpers.
- `ultros-frontend/ultros-app/src/components/list/list_settings_drawer.rs` — new drawer with Details / Sharing / Danger zone sections.
- `integration/list-flow.cjs` — Puppeteer E2E covering add item, add recipe, mark acquired, share controls.

**Modify:**
- `ultros-frontend/ultros-app/src/components/list/mod.rs` — re-export new modules.
- `ultros-frontend/ultros-app/src/components/list/list_item_row.rs` — always-visible `acquired/quantity`, toggle mark-acquired, completed-row styling.
- `ultros-frontend/ultros-app/src/routes/list_view.rs` — header restructure, `ViewCaps` memo, live indicator, `recently_changed`, settings button.
- `ultros-frontend/ultros-app/src/routes/lists.rs` — replace inline `ShareListModal` with import.
- `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json` — new keys.
- `integration/package.json` — register `test:list-flow` script.
- `scripts/run_e2e.sh` — invoke list-flow when `test-auth` feature is built.

**Touch (read-only check):**
- `ultros-frontend/ultros-app/src/components/list_subscribe_drawer.rs` — pattern reference for the new drawer (no changes).
- `ultros-frontend/ultros-app/src/components/modal.rs` — pattern reference for the `Modal` wrapper.

---

## Validation Cadence

After every task with code changes:

```bash
./check_ci.sh
```

This runs `cargo fmt --all -- --check` and `cargo clippy --all-targets -- -D warnings`. CI will fail on either. Fix `cargo fmt --all` for formatting, fix the code for clippy.

The full E2E + manual smoke runs at the end (Task 18).

---

## Task 1: Extract ShareListModal into a shared component

Move the share UI out of `routes/lists.rs` so the new settings drawer can embed the same logic. No behavior change to the top-level page.

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/list/share_list_modal.rs`
- Modify: `ultros-frontend/ultros-app/src/components/list/mod.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/lists.rs` (remove inline definitions, add import)

- [ ] **Step 1: Create the new module file**

Read `routes/lists.rs` lines 1-429 and copy the following items verbatim into the new file `ultros-frontend/ultros-app/src/components/list/share_list_modal.rs`:

- helper functions: `permission_label`, `editable_permission`, `invite_url`, `copy_invite_url`, `invite_uses_label`
- components: `AccessRow`, `AccessList`, `ShareListModal`

Make every helper that's only called from this module **private** (no `pub`). Make `permission_label`, `editable_permission`, `invite_url`, `copy_invite_url`, `invite_uses_label`, `AccessRow`, `AccessList` **`pub(crate)`** so they can be reused by the drawer in Task 14. Mark `ShareListModal` as `pub(crate)`.

After the components, add a new `ShareListSection` component — same body as `ShareListModal` but **without** the outer `<Modal>` wrapper. The body is everything inside the `view!` block of `ShareListModal` between `<Modal ...>` and `</Modal>`. This lets us embed the share UI in a drawer.

The file must keep the imports from `routes/lists.rs:1-28` minus the ones not used by the moved items. Likely needed:

```rust
use crate::api::{
    create_list_invite, delete_list_invite, get_groups, get_list_invites, get_list_shares,
    share_list_with_group, share_list_with_user, unshare_list_from_group, unshare_list_from_user,
};
use crate::components::icon::Icon;
use crate::components::loading::*;
use crate::components::modal::Modal;
use crate::global_state::clipboard_text::GlobalLastCopiedText;
use crate::global_state::toasts::{Toasts, use_toast};
use crate::i18n::*;
use icondata as i;
use leptos::prelude::*;
use ultros_api_types::list::{
    CreateInvite, List, ListInvite, ListPermission, ListSharedGroup, ListSharedUser,
    ShareListGroup, ShareListUser,
};
```

The new `ShareListSection` signature:

```rust
#[component]
pub(crate) fn ShareListSection(list: List) -> impl IntoView {
    // ... same logic as ShareListModal body, no <Modal> wrapper
}
```

Refactor `ShareListModal` to delegate to `ShareListSection`:

```rust
#[component]
pub(crate) fn ShareListModal(list: List, set_visible: WriteSignal<bool>) -> impl IntoView {
    let i18n = use_i18n();
    let list_name = list.name.clone();
    view! {
        <Modal set_visible=set_visible max_width="max-w-5xl w-[96%] sm:w-[820px]".to_string()>
            <div class="space-y-6">
                <div class="pr-10">
                    <h2 class="text-3xl font-black text-[color:var(--color-text)]">
                        {t!(i18n, lists_share_heading_prefix)} {list_name}
                    </h2>
                </div>
                <ShareListSection list=list />
            </div>
        </Modal>
    }
}
```

`ShareListSection` does **not** render the `<h2>` heading (the embedding context — modal or drawer — provides its own heading).

- [ ] **Step 2: Re-export from `components/list/mod.rs`**

Replace the contents of `ultros-frontend/ultros-app/src/components/list/mod.rs`:

```rust
pub mod auto_mark_purchases;
pub mod buying_view;
pub mod list_item_row;
pub mod list_summary;
pub mod share_list_modal;
```

- [ ] **Step 3: Update `routes/lists.rs` to use the new module**

In `ultros-frontend/ultros-app/src/routes/lists.rs`:

- Delete the moved items: helper functions `permission_label`, `editable_permission`, `invite_url`, `copy_invite_url`, `invite_uses_label` (lines ~30-86), and components `AccessRow`, `AccessList`, `ShareListModal` (lines ~88-429).
- Keep `ListInviteAccept`, `PermissionPill`, `ListCard`, `EditLists`, `Lists`.
- Remove imports that are no longer used in `routes/lists.rs`: `Modal`, `GlobalLastCopiedText`, `Toasts`, `use_toast` (unless still used elsewhere in the file — verify with grep before deleting), and unused API imports `create_list_invite`, `delete_list_invite`, `get_groups`, `get_list_invites`, `get_list_shares`, `share_list_with_group`, `share_list_with_user`, `unshare_list_from_group`, `unshare_list_from_user`.
- Add `use crate::components::list::share_list_modal::ShareListModal;` near the top with other component imports.

Verify `ListCard` line ~693-695 still compiles:

```rust
<Show when=share_open>
    <ShareListModal list=list_for_share.clone() set_visible=set_share_open />
</Show>
```

- [ ] **Step 4: Build and verify**

Run:

```bash
./check_ci.sh
```

Expected: clean. If clippy complains about unused imports in `routes/lists.rs`, remove them.

- [ ] **Step 5: Manual smoke check the top-level lists page**

```bash
cargo leptos serve --bin-features test-auth
```

In another shell:

```bash
curl -fsS "http://127.0.0.1:8080/test/login?user_id=7777777777777&username=E2ETestUser&redirect=/list" > /dev/null
```

Open `/list` in a browser with the cookie set. Create a list, click the share button on the card → modal opens, has "Add people", "Share via link", "Who has access". Closes correctly. No console errors.

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/list/mod.rs \
        ultros-frontend/ultros-app/src/components/list/share_list_modal.rs \
        ultros-frontend/ultros-app/src/routes/lists.rs
git commit -m "$(cat <<'EOF'
refactor(lists): extract ShareListModal into shared component

Moves the share modal out of routes/lists.rs so both the top-level lists
page and the upcoming list-detail settings drawer can reuse it. Adds a
ShareListSection (body without Modal wrapper) for drawer embedding.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Always-visible acquired/quantity indicator on every row

Update `list_item_row.rs` so the Quantity column always shows `{acquired}/{quantity}` plus a progress bar, even when quantity is 1.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/list/list_item_row.rs:120-141`

- [ ] **Step 1: Replace the conditional quantity rendering**

In `list_item_row.rs`, locate the read-only quantity cell (currently around lines 120-141). Replace it with always-visible rendering:

```rust
<td class="px-3 py-3 align-middle">
    {move || {
        let item = item.get();
        let q = item.quantity.unwrap_or(1).max(1);
        let a = item.acquired.unwrap_or(0).max(0).min(q);
        let complete = a >= q;
        view! {
            <div class="flex flex-col gap-1 w-full">
                <span class=move || {
                    if complete {
                        "text-sm font-semibold text-green-300"
                    } else {
                        "text-sm"
                    }
                }>{format!("{a} / {q}")}</span>
                <progress
                    class="progress progress-primary h-2 w-full rounded"
                    value=a
                    max=q
                ></progress>
            </div>
        }
    }}
</td>
```

Note: `let item = item.get();` here is fine because the cell's outer block was already a closure tied to `item.get()`. Confirm the existing pattern matches.

- [ ] **Step 2: Build and verify**

```bash
./check_ci.sh
```

Expected: clean.

- [ ] **Step 3: Manual smoke**

With the dev server running and logged in, navigate to a list with items. Every row's Quantity column now shows `0 / 1` (empty progress bar) for unacquired single items. Mark one acquired via the check icon → updates to `1 / 1` (full bar, green text).

- [ ] **Step 4: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/list/list_item_row.rs
git commit -m "$(cat <<'EOF'
feat(list-view): show acquired/quantity progress on every row

Previously only items with quantity > 1 showed the acquired indicator
and progress bar. Now every row makes acquired state visible, including
single-quantity items.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Toggle behavior on mark-acquired + completed-row styling

The mark-acquired button currently always sets `acquired = quantity`. Change it to toggle: if fully acquired, reset to 0. Add a subtle green background and a check icon for completed rows.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/list/list_item_row.rs`

- [ ] **Step 1: Change the `<tr>` to react to completion**

Locate the `<tr class="group transition-colors hover:bg-[color:var(--color-background-panel)]">` line near the top of the view (around line 47). Replace with:

```rust
<tr
    class=move || {
        let item = item.get();
        let q = item.quantity.unwrap_or(1).max(1);
        let a = item.acquired.unwrap_or(0);
        let complete = a >= q;
        if complete {
            "group transition-colors bg-green-900/15 hover:bg-green-900/25"
        } else {
            "group transition-colors hover:bg-[color:var(--color-background-panel)]"
        }
    }
>
```

- [ ] **Step 2: Add a check icon in the HQ column when complete**

Locate the HQ cell (around lines 72-86). Replace with a version that shows either HQ badge OR a check icon when complete (HQ takes precedence if both are true — show HQ above the check):

```rust
<td class="px-3 py-3 align-middle">
    {move || {
        let item = item.get();
        let q = item.quantity.unwrap_or(1).max(1);
        let a = item.acquired.unwrap_or(0);
        let complete = a >= q;
        view! {
            <div class="flex flex-col items-start gap-1">
                {item.hq.and_then(|hq| {
                    hq.then_some(view! {
                        <span class="inline-flex rounded-md border border-[color:var(--brand-ring)]/40 px-2 py-0.5 text-xs font-bold text-[color:var(--brand-fg)]">
                            "HQ"
                        </span>
                    })
                })}
                {complete.then(|| view! {
                    <span
                        class="inline-flex items-center gap-1 rounded-md border border-green-400/40 px-1.5 py-0.5 text-xs text-green-200"
                        aria-label=t_string!(i18n, list_view_completed_row_aria).to_string()
                    >
                        <Icon icon=i::BiCheckCircle />
                    </span>
                })}
            </div>
        }
    }}
</td>
```

- [ ] **Step 3: Make the mark-acquired button toggle**

Locate the "Mark acquired" button (around lines 192-205). Replace with toggle behavior and i18n-aware tooltip:

```rust
<Tooltip tooltip_text=Signal::derive(move || {
    let q = item.with(|i| i.quantity.unwrap_or(1).max(1));
    let a = item.with(|i| i.acquired.unwrap_or(0));
    if a >= q {
        t_string!(i18n, list_view_mark_unacquired).to_string()
    } else {
        t_string!(i18n, list_item_row_mark_acquired).to_string()
    }
})>
    <button
        class="btn-secondary h-8 w-8 p-0"
        aria-label=move || {
            let q = item.with(|i| i.quantity.unwrap_or(1).max(1));
            let a = item.with(|i| i.acquired.unwrap_or(0));
            if a >= q {
                t_string!(i18n, list_view_mark_unacquired).to_string()
            } else {
                t_string!(i18n, list_item_row_mark_acquired).to_string()
            }
        }
        on:click=move |_| {
            item.update(|i| {
                let q = i.quantity.unwrap_or(1).max(1);
                let a = i.acquired.unwrap_or(0);
                if a >= q {
                    i.acquired = Some(0);
                } else {
                    i.acquired = i.quantity.or(Some(1));
                }
            });
            let _ = edit_item.dispatch(item.get());
        }
    >
        <Icon icon=i::BiCheckRegular />
    </button>
</Tooltip>
```

- [ ] **Step 4: Build and verify**

```bash
./check_ci.sh
```

Expected: clean. The `list_view_mark_unacquired` and `list_view_completed_row_aria` keys don't exist yet — they will fail to compile via leptos-i18n. **Stop here and proceed to Task 4** which adds those keys to `en.json` only (compiles), and Task 17 fills in the other locales. If you prefer compile-first, jump to Task 4, complete it, then return.

- [ ] **Step 5: Commit (after Task 4 lands the en.json keys)**

After Task 4 adds the keys for `en.json` (which is enough to compile because leptos-i18n only **warns** on missing locales, doesn't fail), come back and commit:

```bash
git add ultros-frontend/ultros-app/src/components/list/list_item_row.rs
git commit -m "$(cat <<'EOF'
feat(list-view): completed-row treatment and toggle mark-acquired

Adds a subtle green background and check icon to rows where acquired
matches quantity. The mark-acquired button now toggles: if already
fully acquired, resets to 0; otherwise sets acquired = quantity.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Add new i18n keys to en.json (enables compile of Tasks 3, 5-15)

Land the English keys first so subsequent tasks compile cleanly. Other locales follow in Task 17.

**Files:**
- Modify: `ultros-frontend/ultros-app/locales/en.json`

- [ ] **Step 1: Append the new keys**

Open `ultros-frontend/ultros-app/locales/en.json`. Find the existing `list_view_*` block (search for `"list_view_done"`). Add these keys near the existing list_view group. JSON syntax — match the file's indentation and comma placement.

Keys to add:

```json
"list_view_settings": "Settings",
"list_view_settings_tooltip": "List settings",
"list_view_list_options": "List options",
"list_view_settings_details_heading": "Details",
"list_view_settings_sharing_heading": "Sharing",
"list_view_settings_danger_zone": "Danger zone",
"list_view_acquired": "Acquired",
"list_view_acquired_quantity": "Acquired / Quantity",
"list_view_units_acquired_progress": "{acquired} / {quantity} units acquired ({pct}%)",
"list_view_units_acquired_aria": "Overall progress: {acquired} of {quantity} units acquired",
"list_view_no_items_yet": "No items yet",
"list_view_mark_unacquired": "Mark unacquired",
"list_view_live_status_live": "Live",
"list_view_live_status_connecting": "Connecting",
"list_view_live_status_reconnecting": "Reconnecting",
"list_view_live_status_offline": "Offline",
"list_view_updated_seconds_ago": "Updated {seconds}s ago",
"list_view_updated_just_now": "Just updated",
"list_view_completed_row_aria": "Item fully acquired",
"list_view_acquired_items_tooltip": "{count} of {total} items fully acquired",
"list_view_delete_list": "Delete list",
"list_view_delete_list_confirm": "Click again to confirm delete",
"list_view_settings_save": "Save",
"list_view_settings_cancel": "Cancel",
"list_view_settings_rename_label": "List name",
"list_view_settings_world_label": "World / Region"
```

These keys are referenced in Tasks 3, 5, 6, 9, 10, 11, 13.

- [ ] **Step 2: Validate JSON**

```bash
node -e "JSON.parse(require('fs').readFileSync('ultros-frontend/ultros-app/locales/en.json','utf8'))"
```

Expected: no output (parses cleanly). If you get a SyntaxError, fix the comma/braces.

- [ ] **Step 3: Build**

```bash
./check_ci.sh
```

Expected: clean (leptos-i18n only warns on missing locales, doesn't fail).

- [ ] **Step 4: Commit**

```bash
git add ultros-frontend/ultros-app/locales/en.json
git commit -m "$(cat <<'EOF'
i18n(en): add list view refresh keys

Adds keys for the new list settings drawer, live indicator, acquired
indicators, and danger-zone confirmations. Other locales follow.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 5: Return to Task 3 Step 5 and commit Task 3**

---

## Task 5: ViewCaps memo + apply permission gating

Replace the ad-hoc `can_write` boolean with a single `ViewCaps` struct memo. Apply hide-rather-than-disable to bulk-edit / delete / add-item / add-recipe / make-place.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/list_view.rs`

- [ ] **Step 1: Add the ViewCaps struct and memo near the top of the view function**

After the existing signal declarations (around line 169, before `view! {`), add:

```rust
#[derive(Clone, Copy, Default)]
struct ViewCaps {
    can_write: bool,
    can_admin: bool,
    can_leave: bool,
}

let view_caps = Memo::new(move |_| {
    match list_view.get() {
        Some(Ok((list_with_perm, _))) => {
            let p = list_with_perm.permission;
            ViewCaps {
                can_write: p >= ListPermission::Write,
                can_admin: p >= ListPermission::Owner,
                can_leave: p == ListPermission::Write || p == ListPermission::Read,
            }
        }
        _ => ViewCaps::default(),
    }
});
```

Note: place `ViewCaps` inside `fn ListView()` body so it's local to this file.

- [ ] **Step 2: Replace inline `can_write` reads**

In the same function, find `let can_write = list.permission >= ListPermission::Write;` (currently around line 442). Delete it.

Find every `disabled=move || !can_write` and replace with hide-style:

```rust
// before:
<button class="btn-secondary" disabled=move || !can_write ...>

// after:
<Show when=move || view_caps.with(|c| c.can_write)>
    <button class="btn-secondary" ...>
</Show>
```

Wrap the **entire** add/recipe/make-place button row (around lines 178-220) and the bulk-edit / delete buttons (around lines 502-560) in a `<Show when=move || view_caps.with(|c| c.can_write)>` block.

- [ ] **Step 3: Build and verify**

```bash
./check_ci.sh
```

Expected: clean.

- [ ] **Step 4: Manual smoke**

This task is hard to fully smoke without a second user. After Task 18, the E2E harness will exercise read-only viewer. For now, verify the page still works for an owner — all buttons visible, edits work.

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/list_view.rs
git commit -m "$(cat <<'EOF'
feat(list-view): ViewCaps permission gating

Replaces ad-hoc can_write with a ViewCaps memo derived from
ListPermission. Hides (rather than disables) write-only affordances
from read-only viewers.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Page-level acquired/quantity progress bar

Add a unit-level progress indicator to the header summary row.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/list_view.rs`

- [ ] **Step 1: Compute totals next to the existing stat counts**

Locate the block where `total_items`, `remaining_items`, `acquired_items` are computed (currently around lines 433-441). Add unit-level totals:

```rust
let total_items = item_snapshot.len();
let remaining_items = item_snapshot
    .iter()
    .filter(|(item, _)| item.quantity.unwrap_or(1) > item.acquired.unwrap_or(0))
    .count();
let acquired_items = total_items.saturating_sub(remaining_items);

let total_quantity: i32 = item_snapshot
    .iter()
    .map(|(i, _)| i.quantity.unwrap_or(1).max(1))
    .sum();
let total_acquired: i32 = item_snapshot
    .iter()
    .map(|(i, _)| {
        let q = i.quantity.unwrap_or(1).max(1);
        i.acquired.unwrap_or(0).clamp(0, q)
    })
    .sum();
let pct: i32 = if total_quantity > 0 {
    100 * total_acquired / total_quantity
} else {
    0
};
```

- [ ] **Step 2: Render the progress bar in the title block (non-buying-view branch)**

Locate the title block of the non-buying-view branch (currently around lines 476-498). Replace the `<div class="grid grid-cols-3 gap-2 text-center text-sm">` stat tiles area to keep the three tiles but add a progress summary row inside the same panel header. Use the existing structure plus the new summary.

Inside the title `<div>` that already holds the name and "List" label, add a summary line below the heading (after the existing `<div class="mt-2 inline-flex rounded-lg ...">{realtime_status}</div>`):

```rust
<div class="mt-3 flex items-center gap-3 text-sm">
    {move || {
        if total_quantity > 0 {
            Either::Left(view! {
                <div
                    class="flex min-w-0 flex-1 flex-col gap-1"
                    aria-label=t_string!(
                        i18n,
                        list_view_units_acquired_aria,
                        acquired = total_acquired,
                        quantity = total_quantity
                    ).to_string()
                >
                    <span class="text-[color:var(--color-text-muted)]">
                        {t!(i18n, list_view_units_acquired_progress,
                            acquired = total_acquired,
                            quantity = total_quantity,
                            pct = pct
                        )}
                    </span>
                    <progress
                        class="progress progress-primary h-2 w-full rounded"
                        value=total_acquired
                        max=total_quantity
                    ></progress>
                </div>
            })
        } else {
            Either::Right(view! {
                <span class="text-[color:var(--color-text-muted)]">
                    {t!(i18n, list_view_no_items_yet)}
                </span>
            })
        }
    }}
</div>
```

Make sure to wrap `total_quantity`, `total_acquired`, `pct` in `move` closures correctly. Since they're computed inside the `Either::Left(move || ...)` branch above this view, they're captured by value (they're `i32`). Confirm by reading the surrounding closure context — the `let item_snapshot = items.get_value();` block scopes these.

- [ ] **Step 3: Rename the "Done" tile to "Acquired"**

In the stat tiles `<div class="grid grid-cols-3 gap-2 text-center text-sm">` block, find the third tile (currently has `{t!(i18n, list_view_done)}` label). Replace with:

```rust
<Tooltip tooltip_text=Signal::derive(move || {
    t_string!(i18n, list_view_acquired_items_tooltip, count = acquired_items, total = total_items).to_string()
})>
    <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] px-3 py-2">
        <div class="text-lg font-bold">{acquired_items}</div>
        <div class="text-xs text-[color:var(--color-text-muted)]">{t!(i18n, list_view_acquired)}</div>
    </div>
</Tooltip>
```

- [ ] **Step 4: Build and verify**

```bash
./check_ci.sh
```

Expected: clean.

- [ ] **Step 5: Manual smoke**

Refresh the list view. Header now shows "X / Y units acquired (Z%)" with a progress bar above the action row. Stat tile reads "Acquired" not "Done". Mark an item acquired → progress bar and percentage update.

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/list_view.rs
git commit -m "$(cat <<'EOF'
feat(list-view): unit-level acquired progress bar in header

Adds a per-unit progress indicator showing total acquired vs total
quantity across all items, with percentage. Renames the 'Done' stat
tile to 'Acquired' with a tooltip clarifying row-level semantics.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Header restructure with consistent button styling

Restructure the action row into a `.list-toolbar` wrapper that applies scoped overrides to keep all buttons the same size and shape.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/list_view.rs`
- Modify: `style/tailwind.css`

- [ ] **Step 1: Add a `.list-toolbar` scoped utility to `style/tailwind.css`**

Open `style/tailwind.css`. Find the `/* Buttons */` section near line 1232. Add this block AFTER the existing `.btn-*` definitions (after the `@utility btn-danger` block, around line 1301):

```css
/* Scoped button consistency for the list-view toolbar.
   The legacy `.btn-secondary` plain CSS at line 865 makes secondary buttons
   render as oversized rounded pills. We don't touch the global rule (it
   affects every secondary button in the app and warrants its own audit);
   instead we override only inside this wrapper. */
.list-toolbar .btn-primary,
.list-toolbar .btn-secondary,
.list-toolbar .btn-ghost {
    padding: 0.5rem 0.75rem;
    margin: 0;
    border-radius: 0.5rem;
    font-size: 0.875rem;
    font-weight: 600;
    height: 2.25rem;
    min-height: 2.25rem;
    line-height: 1;
}
.list-toolbar .btn-primary > *,
.list-toolbar .btn-secondary > *,
.list-toolbar .btn-ghost > * {
    font-size: 0.875rem;
    font-weight: 600;
}
.list-toolbar .btn-primary > svg,
.list-toolbar .btn-secondary > svg,
.list-toolbar .btn-ghost > svg {
    width: 1rem;
    height: 1rem;
}
```

- [ ] **Step 2: Restructure the panel header in list_view.rs**

Open `ultros-frontend/ultros-app/src/routes/list_view.rs`. Locate the top action panel currently at lines 177-247 (`<div class="panel rounded-lg p-3"> ... </div>`). Replace with this two-row structure that has add-actions on top and view-actions on the right:

```rust
<div class="panel rounded-lg p-3">
    <div class="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between list-toolbar">
        <div class="flex flex-wrap items-center gap-2">
            <Show when=move || view_caps.with(|c| c.can_write)>
                <Tooltip tooltip_text=t_string!(i18n, list_view_tooltip_add_item).to_string()>
                    <button
                        class="btn-primary"
                        class:active=move || menu() == MenuState::Item
                        on:click=move |_| set_menu(
                            match menu() {
                                MenuState::Item => MenuState::None,
                                _ => MenuState::Item,
                            },
                        )
                    >
                        <Icon icon=i::BiPlusRegular />
                        <span>{t!(i18n, list_view_add_item)}</span>
                    </button>
                </Tooltip>
                <Tooltip tooltip_text=t_string!(i18n, list_view_tooltip_add_recipe).to_string()>
                    <button
                        class="btn-secondary"
                        class:active=move || recipe_modal_open()
                        on:click=move |_| set_recipe_modal_open(true)
                    >
                        <Icon icon=i::BiBookAddRegular />
                        <span>{t!(i18n, list_view_add_recipe)}</span>
                    </button>
                </Tooltip>
                <Tooltip tooltip_text=t_string!(i18n, list_view_tooltip_import_item).to_string()>
                    <button
                        class="btn-secondary"
                        class:active=move || menu() == MenuState::MakePlace
                        on:click=move |_| set_menu(
                            match menu() {
                                MenuState::MakePlace => MenuState::None,
                                _ => MenuState::MakePlace,
                            },
                        )
                    >
                        <Icon icon=i::BiImportRegular />
                        <span>{t!(i18n, list_view_make_place)}</span>
                    </button>
                </Tooltip>
            </Show>
        </div>

        <div class="flex flex-wrap gap-2 self-start lg:self-auto">
            <Tooltip tooltip_text=t_string!(i18n, list_view_subscribe_tooltip).to_string()>
                <button
                    class="btn-secondary"
                    aria-label=t_string!(i18n, list_view_subscribe_aria)
                    on:click=move |_| set_subscribe_open(true)
                >
                    <Icon icon=i::BsBell />
                    <span>{t!(i18n, list_view_subscribe_button)}</span>
                </button>
            </Tooltip>
            <Tooltip tooltip_text=t_string!(i18n, list_view_tooltip_purchasing_view).to_string()>
                <button
                    class="btn-secondary"
                    class:bg-brand-900=buying_view
                    class:border-brand-500=buying_view
                    class:active=buying_view
                    on:click=move |_| set_buying_view.update(|v| *v = !*v)
                >
                    <Icon icon=i::BiCartRegular />
                    <span>{t!(i18n, list_view_purchasing_view)}</span>
                </button>
            </Tooltip>
            <Tooltip tooltip_text=t_string!(i18n, list_view_settings_tooltip).to_string()>
                <button
                    class="btn-secondary"
                    aria-label=t_string!(i18n, list_view_settings).to_string()
                    data-testid="list-settings-btn"
                    on:click=move |_| set_settings_open(true)
                >
                    <Icon icon=i::BsGear />
                    <span>{t!(i18n, list_view_settings)}</span>
                </button>
            </Tooltip>
        </div>
    </div>
</div>
```

This wraps the row in `list-toolbar` so the CSS overrides apply, and it adds a new Settings button at the end of the right cluster.

- [ ] **Step 3: Declare the settings-open signal**

Near the other signal declarations (around line 166), add:

```rust
let (settings_open, set_settings_open) = signal(false);
```

The drawer body itself lands in Task 13; for now the button does nothing visible but it must compile.

For the next step to compile without the drawer existing, also add a placeholder `<Show when=settings_open>` block at the bottom of the view (just before the closing `</div>` of the outer `<div class="flex flex-col gap-4">`):

```rust
<Show when=settings_open>
    <div data-testid="list-settings-placeholder"></div>
</Show>
```

Note: Leptos 0.7's `<Show>` takes direct children, not a closure. Match the existing pattern at `routes/lists.rs:693`.

- [ ] **Step 4: Build and verify**

```bash
./check_ci.sh
```

Expected: clean.

- [ ] **Step 5: Manual smoke**

Reload the list page. Buttons now all share the same height, same padding, same font weight. Add Item is the only primary-colored one; the rest are secondary. Settings button visible with a gear icon. Clicking it does nothing yet (placeholder).

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/list_view.rs style/tailwind.css
git commit -m "$(cat <<'EOF'
feat(list-view): consistent toolbar styling and Settings button

Adds a .list-toolbar wrapper with scoped CSS overrides that bring the
secondary buttons in line with the primary Add Item button (matching
size, padding, font weight, icon size). Avoids touching the global
legacy .btn-secondary rule. Adds a Settings button placeholder for
the upcoming list-settings drawer.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Live status indicator with dot + tooltip

Replace the text-pill realtime status with a colored-dot indicator that has a tooltip showing the full state plus "Updated Xs ago".

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/list_view.rs`

- [ ] **Step 1: Add `last_update_at` tracking and a tick signal**

Near the other signal declarations (around line 67-68), update / add:

```rust
let (realtime_status, set_realtime_status) = signal("connecting".to_string());
let (last_update_at, set_last_update_at) = signal::<Option<chrono::DateTime<chrono::Utc>>>(None);
let (clock_tick, set_clock_tick) = signal(0_u32);
```

Note: `realtime_status` becomes a stable **status key** (lowercase, snake) — `connecting` / `live` / `reconnecting` / `offline`. The display label is derived in the view via i18n.

In the existing `Effect::new(move |_| { ... })` block that subscribes to the list (currently around lines 103-129), change every `set_realtime_status.set("Foo".to_string())` to use the new keys: `"live"`, `"connecting"`, `"reconnecting"`, `"offline"`. Add `set_last_update_at.set(Some(chrono::Utc::now()));` inside the `ListUpdate(_)` and `Stale { .. }` branches (before `list_view.refetch()`).

Add a 1-second clock tick (only on the client side) after the websocket effects (after the `on_cleanup` block around line 161). The simplest pattern uses `gloo_timers::callback::Interval`:

```rust
#[cfg(not(feature = "ssr"))]
{
    use gloo_timers::callback::Interval;
    let interval = Interval::new(1_000, move || {
        set_clock_tick.update(|n| *n = n.wrapping_add(1));
    });
    // Leak it on purpose — it lives as long as the page.
    interval.forget();
}
```

If `gloo_timers` is not already in `ultros-frontend/ultros-app/Cargo.toml`, check:

```bash
grep gloo-timers ultros-frontend/ultros-app/Cargo.toml
```

It should be there (used by `ws/realtime.rs`). If not, add `gloo-timers = { version = "0.3", features = ["futures"] }` under `[target.'cfg(target_arch = "wasm32")'.dependencies]` or the existing wasm deps block.

- [ ] **Step 2: Add a helper for the "Updated Xs ago" string**

Inside `fn ListView()`, near the existing helpers, add a closure-style helper as a `Signal`:

```rust
let updated_label = Signal::derive(move || {
    // Subscribing to clock_tick keeps this signal fresh every second.
    let _ = clock_tick.get();
    let Some(t) = last_update_at.get() else {
        return String::new();
    };
    let now = chrono::Utc::now();
    let secs = now.signed_duration_since(t).num_seconds().max(0);
    if secs < 2 {
        t_string!(i18n, list_view_updated_just_now).to_string()
    } else {
        t_string!(i18n, list_view_updated_seconds_ago, seconds = secs).to_string()
    }
});
```

- [ ] **Step 3: Replace the realtime-status pill rendering with a dot + tooltip**

There are two render sites for the status pill:

1. Buying-view branch (around lines 456-462): `<span class="rounded-lg border ...">{realtime_status}</span>`
2. Non-buying-view branch (around line 480-482): same shape.

Replace both with this component-style block. To avoid duplication, factor a small helper before the view starts:

After the `view_caps` memo, add:

```rust
let live_indicator = move || {
    let status_key = realtime_status.get();
    let (dot_class, label_key): (&'static str, &'static str) = match status_key.as_str() {
        "live" => ("bg-green-400", "list_view_live_status_live"),
        "reconnecting" => ("bg-amber-400 animate-pulse", "list_view_live_status_reconnecting"),
        "offline" => ("bg-gray-500", "list_view_live_status_offline"),
        _ => ("bg-amber-400 animate-pulse", "list_view_live_status_connecting"),
    };
    let status_label = match label_key {
        "list_view_live_status_live" => t_string!(i18n, list_view_live_status_live).to_string(),
        "list_view_live_status_reconnecting" => t_string!(i18n, list_view_live_status_reconnecting).to_string(),
        "list_view_live_status_offline" => t_string!(i18n, list_view_live_status_offline).to_string(),
        _ => t_string!(i18n, list_view_live_status_connecting).to_string(),
    };
    let updated = updated_label.get();
    let tooltip_text = if updated.is_empty() {
        status_label.clone()
    } else {
        format!("{status_label} · {updated}")
    };
    view! {
        <Tooltip tooltip_text=tooltip_text>
            <span
                class="inline-flex items-center gap-2 rounded-lg border border-[color:var(--color-outline)] px-2 py-1 text-xs text-[color:var(--color-text-muted)]"
                data-testid="list-live-indicator"
                data-status=status_key.clone()
            >
                <span class=format!("h-2 w-2 rounded-full {}", dot_class) aria-hidden="true"></span>
                <span>{status_label}</span>
            </span>
        </Tooltip>
    }
};
```

Then at the two existing render sites, replace `<span class="rounded-lg border ...">{realtime_status}</span>` with `{live_indicator()}`.

- [ ] **Step 4: Build and verify**

```bash
./check_ci.sh
```

Expected: clean.

- [ ] **Step 5: Manual smoke**

Reload the list page. A green dot with "Live" appears at the top of the header. Make an edit (add an item) → the dot/label briefly transitions if the WS roundtrips. Hover the indicator → tooltip shows "Live · Updated 0s ago" or "Live · Just updated".

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/list_view.rs
git commit -m "$(cat <<'EOF'
feat(list-view): live status dot indicator with last-update tooltip

Replaces the text status pill with a colored-dot indicator (green/amber/
gray) plus tooltip showing 'Last updated Xs ago'. A 1-second clock tick
keeps the tooltip fresh while open.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Recently-changed row highlight on WS updates

Diff incoming refetches against the previous snapshot and briefly highlight rows whose `acquired` or `quantity` changed.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/list_view.rs`
- Modify: `ultros-frontend/ultros-app/src/components/list/list_item_row.rs`

- [ ] **Step 1: Add a `recently_changed` signal and effect in `list_view.rs`**

Near the other signal declarations:

```rust
let recently_changed: RwSignal<std::collections::HashSet<i32>> =
    RwSignal::new(std::collections::HashSet::new());
let prev_snapshot: StoredValue<std::collections::HashMap<i32, (Option<i32>, Option<i32>)>> =
    StoredValue::new(std::collections::HashMap::new());
```

Then add an Effect that runs whenever `list_view` resolves:

```rust
Effect::new(move |_| {
    let Some(Ok((_list, items))) = list_view.get() else {
        return;
    };
    let new_snapshot: std::collections::HashMap<i32, (Option<i32>, Option<i32>)> =
        items.iter().map(|(i, _)| (i.id, (i.quantity, i.acquired))).collect();

    let mut newly_changed: std::collections::HashSet<i32> = std::collections::HashSet::new();
    let prev = prev_snapshot.get_value();
    for (id, current) in &new_snapshot {
        if let Some(prior) = prev.get(id) {
            if prior != current {
                newly_changed.insert(*id);
            }
        }
    }
    prev_snapshot.set_value(new_snapshot);

    if !newly_changed.is_empty() {
        recently_changed.update(|set| set.extend(newly_changed.iter().copied()));
        #[cfg(not(feature = "ssr"))]
        {
            use gloo_timers::callback::Timeout;
            let ids: Vec<i32> = newly_changed.into_iter().collect();
            Timeout::new(1500, move || {
                recently_changed.update(|set| {
                    for id in &ids {
                        set.remove(id);
                    }
                });
            })
            .forget();
        }
    }
});
```

- [ ] **Step 2: Pass `recently_changed` to `ListItemRow`**

Update the `<ListItemRow ... />` call site in `list_view.rs`:

```rust
<ListItemRow
    item=item
    listings=listings
    edit_list_mode=edit_list_mode.into()
    selected_items=selected_items
    delete_item=delete_item
    edit_item=edit_item
    recently_changed=recently_changed
/>
```

- [ ] **Step 3: Accept the new prop in `ListItemRow`**

In `ultros-frontend/ultros-app/src/components/list/list_item_row.rs`, add a prop:

```rust
#[component]
pub fn ListItemRow(
    item: ListItem,
    listings: Vec<ActiveListing>,
    edit_list_mode: Signal<bool>,
    #[prop(into)] selected_items: RwSignal<HashSet<i32>>,
    delete_item: Action<i32, Result<(), crate::error::AppError>>,
    edit_item: Action<ListItem, Result<(), crate::error::AppError>>,
    recently_changed: RwSignal<HashSet<i32>>,
) -> impl IntoView {
```

In the `<tr>` class-builder (updated in Task 3), incorporate the highlight:

```rust
<tr
    class=move || {
        let item_now = item.get();
        let q = item_now.quantity.unwrap_or(1).max(1);
        let a = item_now.acquired.unwrap_or(0);
        let complete = a >= q;
        let highlighted = recently_changed.with(|set| set.contains(&item_now.id));
        let highlight_class = if highlighted { " ring-2 ring-brand-400/60" } else { "" };
        if complete {
            format!("group transition-all duration-700 bg-green-900/15 hover:bg-green-900/25{highlight_class}")
        } else {
            format!("group transition-all duration-700 hover:bg-[color:var(--color-background-panel)]{highlight_class}")
        }
    }
>
```

Note the class function now returns `String` (use `format!`). Leptos accepts both `&'static str` and `String` for class values.

- [ ] **Step 4: Build and verify**

```bash
./check_ci.sh
```

Expected: clean.

- [ ] **Step 5: Manual smoke (single user)**

Reload the list page. Open dev tools → run:

```js
document.querySelector('button[aria-label*="Mark"]').click()
```

The row that gets toggled briefly shows a brand-color ring around it for ~1.5s, then fades. The ring requires the next websocket-driven refetch to land, which the local optimistic flow may not trigger — verify the highlight does fire on cross-tab edits in Task 18 e2e.

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/list_view.rs \
        ultros-frontend/ultros-app/src/components/list/list_item_row.rs
git commit -m "$(cat <<'EOF'
feat(list-view): briefly highlight rows changed by WS updates

When a remote update or refetch lands new quantity/acquired values for
existing items, those rows get a brand-color ring for ~1.5s so the user
can see what changed.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: Inline list-name rename for owners

Add an editable list name on the header — pencil icon next to the title for owners; opens an inline form, saves via the `edit_list` action.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/list_view.rs`

- [ ] **Step 1: Add the rename action and editing signals**

Near the other `Action::new` calls in `list_view.rs` (around line 50-61), add:

```rust
let edit_list_action = Action::new(move |list: &ultros_api_types::list::List| {
    crate::api::edit_list(list.clone())
});
```

Near the other signal declarations (around line 168):

```rust
let (rename_open, set_rename_open) = signal(false);
let (rename_value, set_rename_value) = signal(String::new());
```

Also add the `crate::api::edit_list` import or use full path (the API already exists, see `api.rs:303`).

- [ ] **Step 2: Make `list_view` Resource refetch on rename**

Update the Resource declaration (around lines 69-83) to include `edit_list_action.version().get()` in the dependency tuple. The cleanest place is the existing inner tuple — add an additional element.

- [ ] **Step 3: Replace the title block (non-buying-view branch)**

Locate the title block currently at `<h1 class="text-3xl font-bold ...">{list_name.clone()}</h1>` around line 479. Replace the surrounding `<div>` content so it conditionally renders an inline editor:

```rust
<div class="flex items-center gap-2">
    {move || {
        if rename_open() && view_caps.with(|c| c.can_admin) {
            Either::Left(view! {
                <div class="flex flex-wrap items-center gap-2">
                    <input
                        class="input text-xl font-bold"
                        prop:value=rename_value
                        on:input=move |ev| set_rename_value(event_target_value(&ev))
                        data-testid="list-rename-input"
                    />
                    <button
                        class="btn-primary"
                        data-testid="list-rename-save"
                        on:click={
                            let list_for_save = list.list.clone();
                            move |_| {
                                let mut new_list = list_for_save.clone();
                                new_list.name = rename_value().trim().to_string();
                                if !new_list.name.is_empty() {
                                    edit_list_action.dispatch(new_list);
                                    set_rename_open(false);
                                }
                            }
                        }
                    >
                        <Icon icon=i::BiSaveSolid />
                        <span>{t!(i18n, list_view_settings_save)}</span>
                    </button>
                    <button
                        class="btn-secondary"
                        on:click=move |_| set_rename_open(false)
                    >
                        {t!(i18n, list_view_settings_cancel)}
                    </button>
                </div>
            })
        } else {
            let list_name = list_name.clone();
            Either::Right(view! {
                <>
                    <h1 class="text-3xl font-bold text-[color:var(--brand-fg)]">{list_name.clone()}</h1>
                    <Show when=move || view_caps.with(|c| c.can_admin)>
                        <button
                            class="btn-ghost p-1"
                            aria-label=t_string!(i18n, edit_list).to_string()
                            data-testid="list-rename-btn"
                            on:click={
                                let name = list_name.clone();
                                move |_| {
                                    set_rename_value(name.clone());
                                    set_rename_open(true);
                                }
                            }
                        >
                            <Icon icon=i::BsPencilFill />
                        </button>
                    </Show>
                </>
            })
        }
    }}
</div>
```

- [ ] **Step 4: Build and verify**

```bash
./check_ci.sh
```

Expected: clean.

- [ ] **Step 5: Manual smoke**

As owner: pencil icon next to list name. Click → input + Save/Cancel. Type new name → Save → name updates. WS may refresh the entire view due to the `ListUpdated` activity event; that's fine.

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/list_view.rs
git commit -m "$(cat <<'EOF'
feat(list-view): inline list rename for owners

Owners can click the pencil icon next to the list name to rename it
inline. Non-owners see the static heading without the affordance.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: Create ListSettingsDrawer skeleton

Stand up the drawer component with header, body sections, and close behavior. Sharing and Danger zone wired up in Tasks 12-13.

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/list/list_settings_drawer.rs`
- Modify: `ultros-frontend/ultros-app/src/components/list/mod.rs`

- [ ] **Step 1: Create the drawer module**

Create `ultros-frontend/ultros-app/src/components/list/list_settings_drawer.rs` with this content:

```rust
use crate::api::{delete_list, leave_list};
use crate::components::icon::Icon;
use crate::components::list::share_list_modal::ShareListSection;
use crate::components::modal::Modal;
use crate::i18n::*;
use icondata as i;
use leptos::either::Either;
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;
use ultros_api_types::list::{List, ListPermission};

#[component]
pub fn ListSettingsDrawer(
    list: List,
    permission: ListPermission,
    self_user_id: Signal<Option<u64>>,
    set_visible: WriteSignal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let can_admin = permission >= ListPermission::Owner;
    let can_leave = permission == ListPermission::Read || permission == ListPermission::Write;
    let navigate = use_navigate();

    let list_id = list.id;
    let list_name = list.name.clone();

    let delete_action = Action::new(move |_: &()| async move { delete_list(list_id).await });
    let leave_action = Action::new(move |user_id: &u64| {
        let user_id = *user_id;
        async move { leave_list(list_id, user_id).await }
    });

    // After deletion or leave succeeds, navigate back to /list.
    Effect::new(move |_| {
        if matches!(delete_action.value().get(), Some(Ok(_)))
            || matches!(leave_action.value().get(), Some(Ok(_)))
        {
            navigate("/list", Default::default());
        }
    });

    let (delete_confirm, set_delete_confirm) = signal(false);

    let title_key = if can_admin {
        t_string!(i18n, list_view_settings)
    } else {
        t_string!(i18n, list_view_list_options)
    };
    let title_string = title_key.to_string();

    view! {
        <Modal set_visible=set_visible max_width="max-w-5xl w-[96%] sm:w-[820px]".to_string()>
            <div class="space-y-6" data-testid="list-settings-drawer">
                <div class="pr-10">
                    <h2 class="text-3xl font-black text-[color:var(--color-text)]">{title_string}</h2>
                    <p class="text-sm text-[color:var(--color-text-muted)]">{list_name.clone()}</p>
                </div>

                {move || {
                    if can_admin {
                        Some(view! {
                            <section class="space-y-3" data-testid="list-settings-sharing">
                                <h3 class="text-lg font-bold text-[color:var(--color-text)]">
                                    {t!(i18n, list_view_settings_sharing_heading)}
                                </h3>
                                <ShareListSection list=list.clone() />
                            </section>
                        })
                    } else {
                        None
                    }
                }}

                <div class="h-px bg-[color:var(--color-outline)]"></div>

                <section class="space-y-3" data-testid="list-settings-danger">
                    <h3 class="text-lg font-bold text-red-200">
                        {t!(i18n, list_view_settings_danger_zone)}
                    </h3>
                    {move || {
                        if can_admin {
                            Either::Left(view! {
                                <div class="flex flex-wrap items-center gap-2">
                                    <button
                                        class=move || if delete_confirm() { "btn-danger" } else { "btn-secondary" }
                                        data-testid="list-delete-btn"
                                        on:click=move |_| {
                                            if delete_confirm() {
                                                delete_action.dispatch(());
                                            } else {
                                                set_delete_confirm(true);
                                            }
                                        }
                                    >
                                        <Icon icon=i::BiTrashSolid />
                                        <span>
                                            {move || if delete_confirm() {
                                                t_string!(i18n, list_view_delete_list_confirm).to_string()
                                            } else {
                                                t_string!(i18n, list_view_delete_list).to_string()
                                            }}
                                        </span>
                                    </button>
                                    <Show when=delete_confirm>
                                        <button
                                            class="btn-ghost"
                                            on:click=move |_| set_delete_confirm(false)
                                        >
                                            {t!(i18n, list_view_settings_cancel)}
                                        </button>
                                    </Show>
                                </div>
                            })
                        } else if can_leave {
                            Either::Right(view! {
                                <button
                                    class="btn-danger"
                                    data-testid="list-leave-btn"
                                    on:click=move |_| {
                                        if let Some(uid) = self_user_id.get() {
                                            leave_action.dispatch(uid);
                                        }
                                    }
                                >
                                    <Icon icon=i::BiExitRegular />
                                    <span>{t!(i18n, leave_list)}</span>
                                </button>
                            })
                        } else {
                            Either::Right(view! {
                                <button class="btn-secondary" disabled=true>
                                    <span>{t!(i18n, list_view_settings_danger_zone)}</span>
                                </button>
                            })
                        }
                    }}
                </section>
            </div>
        </Modal>
    }
}
```

- [ ] **Step 2: Re-export from `components/list/mod.rs`**

Append to `ultros-frontend/ultros-app/src/components/list/mod.rs`:

```rust
pub mod list_settings_drawer;
```

The file should now read:

```rust
pub mod auto_mark_purchases;
pub mod buying_view;
pub mod list_item_row;
pub mod list_settings_drawer;
pub mod list_summary;
pub mod share_list_modal;
```

- [ ] **Step 3: Build and verify**

```bash
./check_ci.sh
```

Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/list/mod.rs \
        ultros-frontend/ultros-app/src/components/list/list_settings_drawer.rs
git commit -m "$(cat <<'EOF'
feat(list-view): ListSettingsDrawer with sharing and danger zone

New drawer component shown via the Modal wrapper. Owners get the
embedded share section plus delete-list affordance with confirm. Non-
owners (read/write) get leave-list. Read-only users with no leave
ability see a disabled placeholder.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 12: Add Details (rename + world) section to the drawer

The owner-facing Details section lives at the top of the drawer above Sharing. Rename uses the same edit_list action wired in Task 10; world change uses a `WorldPicker`.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/list/list_settings_drawer.rs`

- [ ] **Step 1: Inject the edit_list action**

Update the component signature:

```rust
#[component]
pub fn ListSettingsDrawer(
    list: List,
    permission: ListPermission,
    self_user_id: Signal<Option<u64>>,
    edit_list: Action<List, Result<(), crate::error::AppError>>,
    set_visible: WriteSignal<bool>,
) -> impl IntoView {
```

Add the missing import at the top:

```rust
use crate::components::world_picker::*;
```

- [ ] **Step 2: Render the Details section**

Insert this `<section>` block at the top of the drawer body, immediately after the title `<div class="pr-10">...</div>` block:

```rust
{move || {
    if can_admin {
        let list_for_form = list.clone();
        let (name, set_name) = signal(list_for_form.name.clone());
        let (world, set_world) = signal(Some(list_for_form.wdr_filter));
        Some(view! {
            <section class="space-y-3" data-testid="list-settings-details">
                <h3 class="text-lg font-bold text-[color:var(--color-text)]">
                    {t!(i18n, list_view_settings_details_heading)}
                </h3>
                <div class="grid gap-3 md:grid-cols-2">
                    <div class="flex flex-col gap-1">
                        <label class="label text-sm font-semibold">{t!(i18n, list_view_settings_rename_label)}</label>
                        <input
                            class="input w-full"
                            prop:value=name
                            on:input=move |ev| set_name(event_target_value(&ev))
                            data-testid="drawer-rename-input"
                        />
                    </div>
                    <div class="flex flex-col gap-1">
                        <label class="label text-sm font-semibold">{t!(i18n, list_view_settings_world_label)}</label>
                        <WorldPicker
                            current_world=world.into()
                            set_current_world=set_world.into()
                        />
                    </div>
                </div>
                <div class="flex justify-end">
                    <button
                        class="btn-primary"
                        data-testid="drawer-save-details"
                        on:click={
                            let list_for_save = list_for_form.clone();
                            move |_| {
                                let mut next = list_for_save.clone();
                                next.name = name().trim().to_string();
                                if let Some(w) = world() {
                                    next.wdr_filter = w;
                                }
                                if !next.name.is_empty() {
                                    edit_list.dispatch(next);
                                }
                            }
                        }
                    >
                        <Icon icon=i::BiSaveSolid />
                        <span>{t!(i18n, list_view_settings_save)}</span>
                    </button>
                </div>
            </section>
        })
    } else {
        None
    }
}}
```

- [ ] **Step 3: Build and verify**

```bash
./check_ci.sh
```

Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/list/list_settings_drawer.rs
git commit -m "$(cat <<'EOF'
feat(list-view): Details section in list settings drawer

Owners can change the list name and world inside the drawer. Uses the
same edit_list action dispatched from list_view.rs.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 13: Wire ListSettingsDrawer into list_view.rs

Replace the placeholder `<Show when=settings_open>` block from Task 7 with the real drawer.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/list_view.rs`

- [ ] **Step 1: Import the drawer**

In `list_view.rs` near the existing `use crate::components::list::...` imports (around line 17-24), add:

```rust
use crate::components::list::list_settings_drawer::ListSettingsDrawer;
```

- [ ] **Step 2: Provide self_user_id**

Resolve the logged-in user's id so the drawer can pass it to the leave action. Add near other Resources (around line 84):

```rust
let user_resource = Resource::new(|| {}, |_| async move { crate::api::get_login().await.ok() });
let self_user_id = Signal::derive(move || user_resource.get().flatten().map(|u| u.id));
```

If `get_login` is not directly accessible at that path, check `api.rs` for the right name (`pub(crate) async fn get_login` is in `routes/lists.rs:14` import — it's `crate::api::get_login`).

- [ ] **Step 3: Replace the placeholder Show block**

Find the placeholder `<Show when=settings_open> ... </Show>` block added in Task 7 Step 3 and replace it with:

```rust
<Show when=settings_open>
    {move || {
        let Some(Ok((list_with_perm, _))) = list_view.get() else {
            return view! { <div></div> }.into_any();
        };
        view! {
            <ListSettingsDrawer
                list=list_with_perm.list.clone()
                permission=list_with_perm.permission
                self_user_id=self_user_id
                edit_list=edit_list_action
                set_visible=set_settings_open
            />
        }
        .into_any()
    }}
</Show>
```

- [ ] **Step 4: Build and verify**

```bash
./check_ci.sh
```

Expected: clean.

- [ ] **Step 5: Manual smoke**

Reload the list page. Click the Settings button (gear icon). The drawer opens with Details + Sharing + Danger zone. Rename the list → save → name updates. Open the share section → create an invite → URL gets copied. Click delete twice → list deletes and navigation returns to `/list`.

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/list_view.rs
git commit -m "$(cat <<'EOF'
feat(list-view): wire ListSettingsDrawer into the list detail page

Settings button now opens the drawer with the list's current data,
permission, and self user id.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 13.5: Refresh share data on WS list updates

The drawer's share-data `Resource` is currently versioned only by the local share-action signals. Add a refresh signal so remote share/invite changes flush the access list while the drawer is open. (Spec §3 last bullet.)

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/list/share_list_modal.rs`
- Modify: `ultros-frontend/ultros-app/src/components/list/list_settings_drawer.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/list_view.rs`

- [ ] **Step 1: Extend `ShareListSection` with an optional refresh signal**

In `share_list_modal.rs`, update `ShareListSection`:

```rust
#[component]
pub(crate) fn ShareListSection(
    list: List,
    #[prop(into, optional)] refresh_signal: Option<Signal<u32>>,
) -> impl IntoView {
```

Inside the body, where the `share_data` Resource is defined, fold `refresh_signal` into the dependency tuple:

```rust
let share_data = Resource::new(
    move || {
        (
            share_user.version().get(),
            unshare_user.version().get(),
            share_group.version().get(),
            unshare_group.version().get(),
            create_invite.version().get(),
            delete_invite.version().get(),
            refresh_signal.map(|s| s.get()).unwrap_or(0),
        )
    },
    move |_| async move {
        // ... existing body
    },
);
```

- [ ] **Step 2: Plumb `last_update_at` through the drawer**

In `list_settings_drawer.rs`, accept it as a prop:

```rust
#[component]
pub fn ListSettingsDrawer(
    list: List,
    permission: ListPermission,
    self_user_id: Signal<Option<u64>>,
    edit_list: Action<List, Result<(), crate::error::AppError>>,
    refresh_signal: Signal<u32>,
    set_visible: WriteSignal<bool>,
) -> impl IntoView {
```

Pass it to `ShareListSection`:

```rust
<ShareListSection list=list.clone() refresh_signal=refresh_signal />
```

- [ ] **Step 3: Derive the refresh signal in `list_view.rs`**

In `list_view.rs`, derive a `u32` signal from `last_update_at`:

```rust
let drawer_refresh = Signal::derive(move || {
    last_update_at.get()
        .map(|t| t.timestamp_millis() as u32)
        .unwrap_or(0)
});
```

Pass it to the drawer:

```rust
<ListSettingsDrawer
    list=list_with_perm.list.clone()
    permission=list_with_perm.permission
    self_user_id=self_user_id
    edit_list=edit_list_action
    refresh_signal=drawer_refresh
    set_visible=set_settings_open
/>
```

- [ ] **Step 4: Build and verify**

```bash
./check_ci.sh
```

Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/list/share_list_modal.rs \
        ultros-frontend/ultros-app/src/components/list/list_settings_drawer.rs \
        ultros-frontend/ultros-app/src/routes/list_view.rs
git commit -m "$(cat <<'EOF'
feat(list-view): refresh drawer share data on WS list updates

The drawer's access list and invites now refresh when remote share
events (shared_user, unshared_user, invite_created, invite_used) land
via the websocket. Previously the user had to close and reopen the
drawer.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 14: Add column header rename and "Acquired / Quantity" string

Rename the Quantity column header to reflect the new dual-display.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/list_view.rs`

- [ ] **Step 1: Update the column header**

Locate the `<th>` cell for Quantity (around line 576): `<th scope="col" class="w-40 px-3 py-3 text-left">{t!(i18n, list_view_quantity)}</th>`. Replace with:

```rust
<th scope="col" class="w-40 px-3 py-3 text-left">{t!(i18n, list_view_acquired_quantity)}</th>
```

- [ ] **Step 2: Build and verify**

```bash
./check_ci.sh
```

Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/list_view.rs
git commit -m "$(cat <<'EOF'
feat(list-view): rename Quantity column to Acquired / Quantity

Reflects the new always-visible dual indicator added in Task 2.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 15: Translate all new keys into 6 remaining locales

Add the keys from Task 4 to `fr`, `de`, `ja`, `cn`, `ko`, `tc` with appropriate translations.

**Files:**
- Modify: `ultros-frontend/ultros-app/locales/fr.json`
- Modify: `ultros-frontend/ultros-app/locales/de.json`
- Modify: `ultros-frontend/ultros-app/locales/ja.json`
- Modify: `ultros-frontend/ultros-app/locales/cn.json`
- Modify: `ultros-frontend/ultros-app/locales/ko.json`
- Modify: `ultros-frontend/ultros-app/locales/tc.json`

- [ ] **Step 1: French translations**

Append these key-value pairs to `ultros-frontend/ultros-app/locales/fr.json` (same JSON block placement as `en.json`):

```json
"list_view_settings": "Paramètres",
"list_view_settings_tooltip": "Paramètres de la liste",
"list_view_list_options": "Options de la liste",
"list_view_settings_details_heading": "Détails",
"list_view_settings_sharing_heading": "Partage",
"list_view_settings_danger_zone": "Zone dangereuse",
"list_view_acquired": "Acquis",
"list_view_acquired_quantity": "Acquis / Quantité",
"list_view_units_acquired_progress": "{acquired} / {quantity} unités acquises ({pct} %)",
"list_view_units_acquired_aria": "Progression globale : {acquired} sur {quantity} unités acquises",
"list_view_no_items_yet": "Aucun objet pour le moment",
"list_view_mark_unacquired": "Marquer comme non acquis",
"list_view_live_status_live": "En direct",
"list_view_live_status_connecting": "Connexion",
"list_view_live_status_reconnecting": "Reconnexion",
"list_view_live_status_offline": "Hors ligne",
"list_view_updated_seconds_ago": "Mis à jour il y a {seconds} s",
"list_view_updated_just_now": "Mis à jour à l'instant",
"list_view_completed_row_aria": "Objet entièrement acquis",
"list_view_acquired_items_tooltip": "{count} sur {total} objets entièrement acquis",
"list_view_delete_list": "Supprimer la liste",
"list_view_delete_list_confirm": "Cliquez à nouveau pour confirmer la suppression",
"list_view_settings_save": "Enregistrer",
"list_view_settings_cancel": "Annuler",
"list_view_settings_rename_label": "Nom de la liste",
"list_view_settings_world_label": "Monde / Région"
```

- [ ] **Step 2: German translations**

Append to `ultros-frontend/ultros-app/locales/de.json`:

```json
"list_view_settings": "Einstellungen",
"list_view_settings_tooltip": "Listeneinstellungen",
"list_view_list_options": "Listenoptionen",
"list_view_settings_details_heading": "Details",
"list_view_settings_sharing_heading": "Teilen",
"list_view_settings_danger_zone": "Gefahrenzone",
"list_view_acquired": "Erworben",
"list_view_acquired_quantity": "Erworben / Menge",
"list_view_units_acquired_progress": "{acquired} / {quantity} Einheiten erworben ({pct} %)",
"list_view_units_acquired_aria": "Gesamtfortschritt: {acquired} von {quantity} Einheiten erworben",
"list_view_no_items_yet": "Noch keine Items",
"list_view_mark_unacquired": "Als nicht erworben markieren",
"list_view_live_status_live": "Live",
"list_view_live_status_connecting": "Verbinden",
"list_view_live_status_reconnecting": "Erneut verbinden",
"list_view_live_status_offline": "Offline",
"list_view_updated_seconds_ago": "Aktualisiert vor {seconds}s",
"list_view_updated_just_now": "Gerade aktualisiert",
"list_view_completed_row_aria": "Item vollständig erworben",
"list_view_acquired_items_tooltip": "{count} von {total} Items vollständig erworben",
"list_view_delete_list": "Liste löschen",
"list_view_delete_list_confirm": "Erneut klicken, um zu bestätigen",
"list_view_settings_save": "Speichern",
"list_view_settings_cancel": "Abbrechen",
"list_view_settings_rename_label": "Listenname",
"list_view_settings_world_label": "Welt / Region"
```

- [ ] **Step 3: Japanese translations**

Append to `ultros-frontend/ultros-app/locales/ja.json`:

```json
"list_view_settings": "設定",
"list_view_settings_tooltip": "リスト設定",
"list_view_list_options": "リストオプション",
"list_view_settings_details_heading": "詳細",
"list_view_settings_sharing_heading": "共有",
"list_view_settings_danger_zone": "危険操作",
"list_view_acquired": "取得済み",
"list_view_acquired_quantity": "取得済み / 数量",
"list_view_units_acquired_progress": "{acquired} / {quantity} 個取得済み ({pct}%)",
"list_view_units_acquired_aria": "全体進捗: {quantity} 個中 {acquired} 個取得済み",
"list_view_no_items_yet": "まだアイテムがありません",
"list_view_mark_unacquired": "未取得にする",
"list_view_live_status_live": "ライブ",
"list_view_live_status_connecting": "接続中",
"list_view_live_status_reconnecting": "再接続中",
"list_view_live_status_offline": "オフライン",
"list_view_updated_seconds_ago": "{seconds}秒前に更新",
"list_view_updated_just_now": "更新したばかり",
"list_view_completed_row_aria": "アイテムを取得済み",
"list_view_acquired_items_tooltip": "{total} 件中 {count} 件取得済み",
"list_view_delete_list": "リストを削除",
"list_view_delete_list_confirm": "もう一度クリックして削除を確定",
"list_view_settings_save": "保存",
"list_view_settings_cancel": "キャンセル",
"list_view_settings_rename_label": "リスト名",
"list_view_settings_world_label": "ワールド / リージョン"
```

- [ ] **Step 4: Simplified Chinese translations**

Append to `ultros-frontend/ultros-app/locales/cn.json`:

```json
"list_view_settings": "设置",
"list_view_settings_tooltip": "清单设置",
"list_view_list_options": "清单选项",
"list_view_settings_details_heading": "详情",
"list_view_settings_sharing_heading": "共享",
"list_view_settings_danger_zone": "危险操作",
"list_view_acquired": "已获得",
"list_view_acquired_quantity": "已获得 / 数量",
"list_view_units_acquired_progress": "已获得 {acquired} / {quantity} 件 ({pct}%)",
"list_view_units_acquired_aria": "总体进度: {quantity} 件中已获得 {acquired} 件",
"list_view_no_items_yet": "尚无物品",
"list_view_mark_unacquired": "标记为未获得",
"list_view_live_status_live": "实时",
"list_view_live_status_connecting": "连接中",
"list_view_live_status_reconnecting": "重新连接中",
"list_view_live_status_offline": "离线",
"list_view_updated_seconds_ago": "{seconds} 秒前更新",
"list_view_updated_just_now": "刚刚更新",
"list_view_completed_row_aria": "物品已完全获得",
"list_view_acquired_items_tooltip": "{total} 件中已完全获得 {count} 件",
"list_view_delete_list": "删除清单",
"list_view_delete_list_confirm": "再次点击以确认删除",
"list_view_settings_save": "保存",
"list_view_settings_cancel": "取消",
"list_view_settings_rename_label": "清单名称",
"list_view_settings_world_label": "服务器 / 地区"
```

- [ ] **Step 5: Korean translations**

Append to `ultros-frontend/ultros-app/locales/ko.json`:

```json
"list_view_settings": "설정",
"list_view_settings_tooltip": "목록 설정",
"list_view_list_options": "목록 옵션",
"list_view_settings_details_heading": "세부 정보",
"list_view_settings_sharing_heading": "공유",
"list_view_settings_danger_zone": "위험 영역",
"list_view_acquired": "획득함",
"list_view_acquired_quantity": "획득 / 수량",
"list_view_units_acquired_progress": "{acquired} / {quantity} 개 획득 ({pct}%)",
"list_view_units_acquired_aria": "전체 진행률: {quantity} 개 중 {acquired} 개 획득",
"list_view_no_items_yet": "아이템이 아직 없습니다",
"list_view_mark_unacquired": "획득 취소",
"list_view_live_status_live": "실시간",
"list_view_live_status_connecting": "연결 중",
"list_view_live_status_reconnecting": "다시 연결 중",
"list_view_live_status_offline": "오프라인",
"list_view_updated_seconds_ago": "{seconds}초 전 업데이트됨",
"list_view_updated_just_now": "방금 업데이트됨",
"list_view_completed_row_aria": "아이템 완전히 획득됨",
"list_view_acquired_items_tooltip": "{total}개 중 {count}개 완전히 획득됨",
"list_view_delete_list": "목록 삭제",
"list_view_delete_list_confirm": "삭제하려면 다시 클릭",
"list_view_settings_save": "저장",
"list_view_settings_cancel": "취소",
"list_view_settings_rename_label": "목록 이름",
"list_view_settings_world_label": "월드 / 지역"
```

- [ ] **Step 6: Traditional Chinese translations**

Append to `ultros-frontend/ultros-app/locales/tc.json`:

```json
"list_view_settings": "設定",
"list_view_settings_tooltip": "清單設定",
"list_view_list_options": "清單選項",
"list_view_settings_details_heading": "詳細資訊",
"list_view_settings_sharing_heading": "分享",
"list_view_settings_danger_zone": "危險區域",
"list_view_acquired": "已取得",
"list_view_acquired_quantity": "已取得 / 數量",
"list_view_units_acquired_progress": "已取得 {acquired} / {quantity} 件 ({pct}%)",
"list_view_units_acquired_aria": "總進度: {quantity} 件中已取得 {acquired} 件",
"list_view_no_items_yet": "尚無物品",
"list_view_mark_unacquired": "標記為未取得",
"list_view_live_status_live": "即時",
"list_view_live_status_connecting": "連線中",
"list_view_live_status_reconnecting": "重新連線中",
"list_view_live_status_offline": "離線",
"list_view_updated_seconds_ago": "{seconds} 秒前更新",
"list_view_updated_just_now": "剛剛更新",
"list_view_completed_row_aria": "物品已完全取得",
"list_view_acquired_items_tooltip": "{total} 件中已完全取得 {count} 件",
"list_view_delete_list": "刪除清單",
"list_view_delete_list_confirm": "再次點擊以確認刪除",
"list_view_settings_save": "儲存",
"list_view_settings_cancel": "取消",
"list_view_settings_rename_label": "清單名稱",
"list_view_settings_world_label": "伺服器 / 地區"
```

- [ ] **Step 7: Validate JSON of all locales**

```bash
for f in ultros-frontend/ultros-app/locales/*.json; do
  node -e "JSON.parse(require('fs').readFileSync('$f','utf8'))" || echo "FAILED: $f"
done
```

Expected: no failures.

- [ ] **Step 8: Build**

```bash
./check_ci.sh
```

Expected: clean.

- [ ] **Step 9: Commit**

```bash
git add ultros-frontend/ultros-app/locales/
git commit -m "$(cat <<'EOF'
i18n: translate list view refresh keys for fr/de/ja/cn/ko/tc

Adds translations for the 26 new keys added in the list-view refresh.
Flag for native speaker review: machine-quality translation, please
correct anything off.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 16: New E2E harness — `integration/list-flow.cjs`

Build the Puppeteer harness that exercises every list flow against the test-auth feature. This is the validation step the user explicitly asked for.

**Files:**
- Create: `integration/list-flow.cjs`
- Modify: `integration/package.json` — register `test:list-flow` script.
- Modify: `scripts/run_e2e.sh` — invoke list-flow when test-auth is built.

- [ ] **Step 1: Create the harness**

Create `integration/list-flow.cjs`:

```javascript
#!/usr/bin/env node
/* eslint-disable no-console */
/**
 * List flow E2E. Requires the server to be built with `--features test-auth`.
 *
 * Exercises the /list/{id} page:
 *   1. Owner creates a list via the UI ("Add Item" flow).
 *   2. Owner adds a recipe via the modal.
 *   3. Owner marks an item acquired via the row toggle.
 *   4. Owner opens the settings drawer, renames the list, creates an invite.
 *   5. A second user redeems the invite, sees the renamed list.
 *   6. Read-only verification: the second user (Read permission) does NOT see
 *      Add Item / Add Recipe / Settings.
 *   7. Owner deletes the list and is navigated back to /list.
 *
 * Env:
 *   BASE_URL    default http://127.0.0.1:8080
 *   HEADLESS    "false" to watch, anything else uses puppeteer's "new" mode
 *   TIMEOUT_MS  default 30000
 */

"use strict";

const USERS = {
  owner: { id: 990000000001, username: "ListFlowOwner" },
  reader: { id: 990000000002, username: "ListFlowReader" },
};

async function login(page, baseUrl, user) {
  const url = new URL("/test/login", baseUrl);
  url.searchParams.set("user_id", String(user.id));
  url.searchParams.set("username", user.username);
  url.searchParams.set("redirect", "/list");
  const resp = await page.goto(url.toString(), { waitUntil: "domcontentloaded" });
  if (!resp || resp.status() >= 400) {
    throw new Error(`test login failed for ${user.username}: ${resp ? resp.status() : -1}`);
  }
}

async function api(page, method, path, body) {
  return page.evaluate(
    async ({ method, path, body }) => {
      const r = await fetch(path, {
        method,
        credentials: "include",
        headers: body === undefined ? {} : { "Content-Type": "application/json" },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      const text = await r.text();
      let parsed = null;
      try {
        parsed = text ? JSON.parse(text) : null;
      } catch {
        parsed = text;
      }
      return { status: r.status, body: parsed };
    },
    { method, path, body },
  );
}

function fail(failures, msg) {
  console.error(`  ✗ ${msg}`);
  failures.push(msg);
}

function pass(msg) {
  console.log(`  ✓ ${msg}`);
}

async function waitFor(page, selector, timeout) {
  return page.waitForSelector(selector, { timeout, visible: true });
}

async function main() {
  const puppeteer = require("puppeteer");
  const BASE_URL = process.env.BASE_URL || "http://127.0.0.1:8080";
  const TIMEOUT_MS = Number(process.env.TIMEOUT_MS || 30000);
  const headless = process.env.HEADLESS === "false" ? false : "new";

  const browser = await puppeteer.launch({
    headless,
    args: ["--no-sandbox", "--disable-setuid-sandbox"],
  });

  const failures = [];
  let listId = null;

  try {
    const ownerPage = await browser.newPage();
    const readerPage = await browser.newPage();
    for (const p of [ownerPage, readerPage]) {
      p.setDefaultTimeout(TIMEOUT_MS);
      // Force desktop viewport so the list-toolbar buttons render in their
      // full labeled form (the layout collapses below ~lg breakpoint).
      await p.setViewport({ width: 1280, height: 900, deviceScaleFactor: 1 });
    }

    // ===== Owner setup =====
    console.log("[step] owner login");
    await login(ownerPage, BASE_URL, USERS.owner);
    await login(readerPage, BASE_URL, USERS.reader);

    // Create a list via the API (faster + reliable).
    const worldData = await api(ownerPage, "GET", "/api/v1/world_data");
    if (worldData.status !== 200) {
      fail(failures, `world_data expected 200, got ${worldData.status}`);
      throw new Error("cannot continue");
    }
    const worldId = worldData.body.regions[0].datacenters[0].worlds[0].id;
    const name = `ListFlow E2E ${Date.now()}`;
    const create = await api(ownerPage, "POST", "/api/v1/list/create", {
      name,
      wdr_filter: { World: worldId },
    });
    if (create.status !== 200) {
      fail(failures, `create list expected 200, got ${create.status}`);
      throw new Error("cannot continue");
    }
    const ownerLists = await api(ownerPage, "GET", "/api/v1/list");
    const owned = ownerLists.body.find((e) => e.list.name === name);
    if (!owned) {
      fail(failures, "created list not returned to owner");
      throw new Error("cannot continue");
    }
    listId = owned.list.id;
    pass(`created list ${listId} via API`);

    // ===== Step 1: Add an item via the UI =====
    console.log("[step] owner adds an item via the UI");
    const listUrl = new URL(`/list/${listId}`, BASE_URL).toString();
    await ownerPage.goto(listUrl, { waitUntil: "domcontentloaded" });
    await ownerPage.waitForFunction(() => !!document.querySelector("h1"), { timeout: TIMEOUT_MS });

    // Click the Add Item button (matches by text on the .list-toolbar primary).
    const addItemBtn = await ownerPage.$x(
      "//*[contains(@class,'list-toolbar')]//button[.//span[normalize-space()='Add Item']]",
    );
    if (!addItemBtn.length) {
      fail(failures, "Add Item button not found");
    } else {
      await addItemBtn[0].click();
      await waitFor(ownerPage, "input[placeholder*='Search']", 5000);
      await ownerPage.type("input[placeholder*='Search']", "Maple Log");
      // Wait for at least one result row to render.
      await ownerPage.waitForFunction(
        () => Array.from(document.querySelectorAll("button")).some((b) => b.textContent && b.textContent.includes("Add")),
        { timeout: 10000 },
      );
      // Click the first row's Add button.
      const innerAddBtn = await ownerPage.$x(
        "//button[.//span[normalize-space()='Add']]",
      );
      if (!innerAddBtn.length) fail(failures, "row-level Add button not found");
      else {
        await innerAddBtn[0].click();
        await new Promise((r) => setTimeout(r, 1500));
        // Verify the item is now in the table.
        const itemCount = await ownerPage.evaluate(
          () => document.querySelectorAll("table tbody tr").length,
        );
        if (itemCount < 1) fail(failures, `expected >=1 row after add, got ${itemCount}`);
        else pass(`added item via UI (${itemCount} row(s))`);
      }
    }

    // ===== Step 2: Add a recipe via the modal =====
    console.log("[step] owner adds a recipe");
    const addRecipeBtn = await ownerPage.$x(
      "//*[contains(@class,'list-toolbar')]//button[.//span[normalize-space()='Add Recipe']]",
    );
    if (!addRecipeBtn.length) {
      fail(failures, "Add Recipe button not found");
    } else {
      await addRecipeBtn[0].click();
      // Modal renders — type a recipe name; the modal's search input should exist.
      try {
        await ownerPage.waitForSelector("input[placeholder]", { timeout: 5000 });
        // Find the most recently-rendered search input (the modal one).
        const inputs = await ownerPage.$$("input[placeholder]");
        const target = inputs[inputs.length - 1];
        await target.focus();
        await target.type("Bronze Ingot");
        await ownerPage.waitForFunction(
          () => {
            const buttons = Array.from(document.querySelectorAll("button"));
            return buttons.some((b) =>
              ["Add ingredients", "Add Ingredients", "Add"].some((s) =>
                (b.textContent || "").includes(s),
              ),
            );
          },
          { timeout: 10000 },
        );
        const recipeAddBtns = await ownerPage.$x(
          "//button[contains(.,'ingredient') or contains(.,'Ingredient') or contains(.,'Add')]",
        );
        if (!recipeAddBtns.length) fail(failures, "recipe add button not found");
        else {
          await recipeAddBtns[0].click();
          await new Promise((r) => setTimeout(r, 2000));
          // Close modal by clicking outside / pressing Escape.
          await ownerPage.keyboard.press("Escape");
          const newCount = await ownerPage.evaluate(
            () => document.querySelectorAll("table tbody tr").length,
          );
          if (newCount <= 1) {
            // Tolerant: a single-ingredient recipe could collide with the existing
            // Maple Log row by item_id. Verify by API instead.
            const apiRes = await api(ownerPage, "GET", `/api/v1/list/${listId}/listings`);
            const itemsLen = apiRes.body && apiRes.body[1] ? apiRes.body[1].length : 0;
            if (itemsLen <= 1) fail(failures, `expected recipe to add rows, table rows=${newCount}, api items=${itemsLen}`);
            else pass(`added recipe via UI (api items=${itemsLen})`);
          } else {
            pass(`added recipe via UI (${newCount} row(s))`);
          }
        }
      } catch (e) {
        fail(failures, `recipe modal interaction failed: ${e.message || e}`);
      }
    }

    // ===== Step 3: Mark an item acquired via the row toggle =====
    console.log("[step] owner marks an item acquired");
    // The mark-acquired button is the last button in the row's action cluster
    // and has aria-label="Mark acquired" (en).
    const markBtn = await ownerPage.$('button[aria-label="Mark acquired"]');
    if (!markBtn) {
      fail(failures, "Mark acquired button not found");
    } else {
      await markBtn.click();
      await new Promise((r) => setTimeout(r, 1500));
      // After toggle, the same button should now read "Mark unacquired".
      const unmarkBtn = await ownerPage.$('button[aria-label="Mark unacquired"]');
      if (!unmarkBtn) fail(failures, "after toggle, expected Mark unacquired aria-label");
      else pass("marked item acquired (toggle works)");
      // Header progress should reflect at least 1 acquired unit.
      const progressText = await ownerPage.evaluate(() => document.body.innerText);
      if (!/units acquired/.test(progressText)) {
        fail(failures, "header progress string 'units acquired' not visible");
      } else {
        pass("header units-acquired summary visible");
      }
    }

    // ===== Step 4: Settings drawer — rename + invite =====
    console.log("[step] owner opens settings drawer");
    const settingsBtn = await ownerPage.$('[data-testid="list-settings-btn"]');
    if (!settingsBtn) {
      fail(failures, "Settings button not found");
    } else {
      await settingsBtn.click();
      await waitFor(ownerPage, '[data-testid="list-settings-drawer"]', 5000);
      pass("settings drawer opened");

      // Rename via drawer.
      const newName = `${name} (renamed)`;
      const renameInput = await ownerPage.$('[data-testid="drawer-rename-input"]');
      if (!renameInput) fail(failures, "drawer rename input not found");
      else {
        await renameInput.click({ clickCount: 3 });
        await renameInput.type(newName);
        const saveBtn = await ownerPage.$('[data-testid="drawer-save-details"]');
        if (!saveBtn) fail(failures, "drawer save button not found");
        else {
          await saveBtn.click();
          await new Promise((r) => setTimeout(r, 1500));
          const heading = await ownerPage.$eval("h1", (h) => h.textContent || "");
          if (!heading.includes("(renamed)")) {
            fail(failures, `expected heading to include '(renamed)', got '${heading}'`);
          } else {
            pass("renamed list via drawer");
          }
        }
      }

      // Create an invite via the drawer's share section.
      // The share section is identified by data-testid="list-settings-sharing".
      const inviteCreateBtn = await ownerPage.$x(
        "//section[@data-testid='list-settings-sharing']//button[contains(.,'Copy') or contains(.,'Invite') or contains(.,'Create')]",
      );
      if (!inviteCreateBtn.length) {
        fail(failures, "drawer invite-create button not found");
      } else {
        await inviteCreateBtn[inviteCreateBtn.length - 1].click();
        // The created invite gets auto-copied. Verify via API.
        await new Promise((r) => setTimeout(r, 1500));
        const invitesResp = await api(ownerPage, "GET", `/api/v1/list/${listId}/invites`);
        if (invitesResp.status !== 200 || !Array.isArray(invitesResp.body) || invitesResp.body.length === 0) {
          fail(failures, `expected at least 1 invite, got ${invitesResp.status} body=${JSON.stringify(invitesResp.body)}`);
        } else {
          pass(`created invite via drawer (${invitesResp.body.length} invite(s))`);

          // ===== Step 5: Reader redeems the invite =====
          const inviteId = invitesResp.body[invitesResp.body.length - 1].id;
          const redeem = await api(readerPage, "POST", `/api/v1/invite/${inviteId}/use`);
          if (redeem.status !== 200 || redeem.body !== listId) {
            fail(failures, `invite redeem expected 200 + listId ${listId}, got ${redeem.status} body=${redeem.body}`);
          } else {
            pass("reader redeemed invite");

            // Reader visits /list/{id} — should NOT see Add Item, Settings.
            await readerPage.goto(`${BASE_URL}/list/${listId}`, { waitUntil: "domcontentloaded" });
            await readerPage.waitForFunction(() => !!document.querySelector("h1"), { timeout: TIMEOUT_MS });
            const visibleControls = await readerPage.evaluate(() => {
              const text = document.body.innerText;
              return {
                hasAddItem: text.includes("Add Item"),
                hasSettings: !!document.querySelector('[data-testid="list-settings-btn"]'),
                hasNotify: text.includes("Notify"),
              };
            });
            // Reader is Read permission by default — should NOT see write affordances.
            if (visibleControls.hasAddItem) {
              fail(failures, "read-only viewer should NOT see Add Item");
            } else pass("read-only viewer hides Add Item");
            // Settings is visible to non-owners only if they can leave the list — they can.
            if (!visibleControls.hasSettings) {
              fail(failures, "read-only viewer should see Settings (for Leave option)");
            } else pass("read-only viewer sees Settings button");
            if (!visibleControls.hasNotify) {
              fail(failures, "read-only viewer should still see Notify");
            } else pass("read-only viewer sees Notify");
          }
        }
      }

      // Close drawer with Escape.
      await ownerPage.keyboard.press("Escape");
      await new Promise((r) => setTimeout(r, 500));
    }

    // ===== Step 6: Delete the list =====
    console.log("[step] owner deletes the list");
    const settingsBtn2 = await ownerPage.$('[data-testid="list-settings-btn"]');
    if (settingsBtn2) {
      await settingsBtn2.click();
      await waitFor(ownerPage, '[data-testid="list-settings-drawer"]', 5000);
      const deleteBtn = await ownerPage.$('[data-testid="list-delete-btn"]');
      if (!deleteBtn) fail(failures, "delete button not found");
      else {
        await deleteBtn.click(); // first click: confirm prompt
        await new Promise((r) => setTimeout(r, 500));
        const deleteBtn2 = await ownerPage.$('[data-testid="list-delete-btn"]');
        if (!deleteBtn2) fail(failures, "delete confirm button not found");
        else {
          await deleteBtn2.click();
          // Should navigate back to /list.
          await ownerPage.waitForFunction(
            () => window.location.pathname === "/list",
            { timeout: 5000 },
          ).catch(() => {});
          if (ownerPage.url().endsWith("/list")) pass("owner returned to /list after delete");
          else fail(failures, `expected url to end with /list, got ${ownerPage.url()}`);
          // API confirms deletion.
          const checkResp = await api(ownerPage, "GET", "/api/v1/list");
          const stillThere = (checkResp.body || []).find((e) => e.list.id === listId);
          if (stillThere) fail(failures, `list ${listId} still exists after delete`);
          else pass(`list ${listId} no longer in API list`);
        }
      }
    }

    for (const p of [ownerPage, readerPage]) await p.close();
  } finally {
    await browser.close();
  }

  if (failures.length) {
    console.error(`[fail] ${failures.length} list-flow assertion(s) failed`);
    process.exit(1);
  }
  console.log("[ok] list flow passed");
}

main().catch((err) => {
  console.error("[error]", err && err.stack ? err.stack : err);
  process.exit(1);
});
```

- [ ] **Step 2: Register the script in `integration/package.json`**

Modify `integration/package.json`. Add `"test:list-flow": "node ./list-flow.cjs"` to the `scripts` block:

```json
"scripts": {
    "pretest": "node -e \"const B=(process.env.BASE_URL||'http://127.0.0.1:8080');const C=(process.env.CONCURRENCY||'16');console.log('BASE_URL='+B);console.log('CONCURRENCY='+C)\"",
    "run": "node ./runner.cjs",
    "test": "npm run test:desktop && npm run test:mobile",
    "test:mobile": "cross-env CONCURRENCY=16 DEVICE=mobile npm run run",
    "test:desktop": "cross-env CONCURRENCY=16 DEVICE=desktop npm run run",
    "test:login": "node ./login.cjs",
    "test:push": "node ./push.cjs",
    "test:shared-list": "node ./shared-list.cjs",
    "test:list-flow": "node ./list-flow.cjs"
}
```

- [ ] **Step 3: Wire it into `scripts/run_e2e.sh`**

Open `scripts/run_e2e.sh`. Find the `*" test-auth "*)` case (around line 158-180) and append the list-flow invocation in the same pattern:

```bash
        log "running list-flow E2E (test-auth feature detected)"
        list_flow_exit=0
        ( cd integration && BASE_URL="$BASE_URL" npm run test:list-flow ) || list_flow_exit=$?
        if [ "$list_flow_exit" -ne 0 ] && [ "$test_exit" -eq 0 ]; then
            test_exit="$list_flow_exit"
        fi
```

Place this block immediately after the `test:shared-list` block, before `test:push`. Keeps the ordering: login → shared-list → list-flow → push.

- [ ] **Step 4: Commit the harness**

```bash
git add integration/list-flow.cjs integration/package.json scripts/run_e2e.sh
git commit -m "$(cat <<'EOF'
test(e2e): exercise full list flow via Puppeteer

New integration/list-flow.cjs covers:
  - Owner adds an item via the UI search-and-add panel
  - Owner adds a recipe via the recipe modal
  - Owner marks the item acquired and verifies the toggle flips
  - Owner opens the settings drawer, renames the list, creates an invite
  - Reader redeems the invite and sees the list with read-only chrome
    (no Add Item; Settings present for leave; Notify present)
  - Owner deletes the list via the drawer and lands back on /list

Hooked into scripts/run_e2e.sh after shared-list when --features
test-auth is enabled.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 17: Run the full E2E pipeline and fix anything it surfaces

This task is the user's explicit ask: "aggressive e2e validation with the test user feature enabled".

**Files:**
- (None new — this is a validation pass that may produce patch commits.)

- [ ] **Step 1: Ensure no orphan dev servers on the standard ports**

Check what's listening; kill any from previous runs in this worktree only:

```bash
# Linux/macOS:
ss -ltn 'sport = :8080' 2>/dev/null || netstat -an | grep 8080
```

On Windows PowerShell, the script picks an ephemeral port (see `scripts/run_e2e.sh:pick_free_port`), so this is mostly a precaution.

- [ ] **Step 2: Run the full pipeline**

From the repo root (with submodules initialized — see CLAUDE.md):

```bash
LEPTOS_FEATURES=test-auth bash scripts/run_e2e.sh
```

This builds the app with `test-auth`, spawns a server on a fresh port, runs:

1. The screenshot suite (`test:desktop` + `test:mobile`).
2. The login flow.
3. The shared-list flow.
4. **The list-flow harness** (new).
5. The push smoke.

Expected: all flows pass. The last log line should be `[done] all routes ok, screenshots + asserts complete` and `[ok] list flow passed`.

- [ ] **Step 3: Triage and fix any failure**

If `test:list-flow` fails:

- Read the `[fail] ...` lines printed at the end.
- For UI element not found: rerun with `HEADLESS=false` and watch the run. Most likely cause: selector mismatch (e.g., the existing `Add Recipe` button uses `Add Recipe` label in English locale — verify it's not been retranslated by the language picker; harness assumes English).
- For permission mismatch: verify Task 5's `view_caps` is applied to every affordance referenced in the harness.
- For drawer not opening: open dev tools console, look for compile errors from `leptos-i18n` about missing keys.

Apply the fix, run `./check_ci.sh`, then rerun the e2e:

```bash
LEPTOS_FEATURES=test-auth REUSE_SERVER=0 bash scripts/run_e2e.sh
```

If the screenshot suite fails on `/list` because of unrelated console errors:

- Check `integration/artifacts/list-desktop.png` for the rendered output.
- Tail `/tmp/ultros-e2e-server.log` for server-side errors.

- [ ] **Step 4: Manual cross-tab smoke for live updates (the one thing E2E can't easily cover)**

Open two browser windows. In window A, log in as `user_id=991`. In window B, log in as `user_id=992`. In window A, create a list and share Write to user 992. Both navigate to `/list/{id}`.

- In window A, mark an item acquired. Within ~2s, window B's row gets a brief brand-color ring, the progress bar increments, and the activity feed shows the new entry.
- Hover the live indicator in window B — tooltip reads `Live · Updated <X>s ago`.
- In window A, change the list name via the drawer. Window B's heading updates without reload.

If any of these don't fire, the issue is likely the `Effect` order or that `recently_changed` doesn't get the cross-tab diff. Diagnose with browser dev tools network → WS frames; expect `ListUpdate` payloads inbound.

- [ ] **Step 5: Final clean-CI pass**

```bash
./check_ci.sh
```

Expected: clean.

- [ ] **Step 6: Commit any harness or wiring tweaks**

If you had to adjust selectors, locales, or wiring, commit them:

```bash
git add <changed files>
git commit -m "$(cat <<'EOF'
test(e2e): adjust list-flow selectors/timings after first run

Iterations after the initial harness pass — selector fixes for the
recipe modal and timing tweaks for the WS-driven activity flush.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 7: Push the branch and report**

Do NOT open a PR or push without user confirmation. Report back with:

- Total tasks completed.
- E2E pipeline result.
- Manual cross-tab smoke result.
- Any flagged-for-native-speaker translations (Task 15).
- Any out-of-scope items punted (e.g., legacy `.btn-secondary` audit).

---

## Self-Review Checklist

After completing all tasks, verify:

1. **Header looks tidy** — every button in the top action row shares the same height/padding/font size.
2. **Add Item flow** — clicking opens the search panel, typing yields results, Add adds the item, the row shows `0 / 1` with empty progress.
3. **Add Recipe flow** — modal opens, search works, "Add ingredients" adds rows.
4. **Mark acquired** — clicking the row toggle flips the row green, adds a check icon, increments the page progress bar; clicking again resets to 0.
5. **Settings drawer (owner)** — Details / Sharing / Danger zone all render. Rename saves. Invite copies a URL. Delete (confirm pattern) deletes and redirects.
6. **Settings drawer (reader)** — only Danger zone with Leave list shows.
7. **Read-only viewer** — no Add buttons, no edit affordances on rows, no rename pencil.
8. **Live indicator** — green dot with "Live" label; tooltip shows "Updated Xs ago".
9. **Row highlight on remote change** — visible in cross-tab smoke.
10. **i18n** — switch the language picker to French; the page renders translated labels.
11. **`./check_ci.sh`** — clean.
12. **`LEPTOS_FEATURES=test-auth bash scripts/run_e2e.sh`** — all suites pass.
