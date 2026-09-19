# List workspace shell: align the cloud list page with the device list page

Date: 2026-09-18
Scope: `/list/:id` under the `lists-sync` lab (`ListViewSync`) and `/list/device/:id` (`DeviceEditor`).
Out of scope: the legacy non-Labs `ListView`, the `/list` directory (already one page).

## Problem

Both editors render the same cart (`ListCart` fed by a `ListWorkspaceSource`), but the
page around the cart differs. The device page is a short header, a Build/Shop toggle,
one price row and the cart. The cloud page wraps the same cart in a six-button sticky
bar, a header card (eyebrow label, h1 with a pencil-rename flow, a realtime dot, a second
save-state line, a progress bar, three stat tiles), a separate filter-row panel, a
settings drawer and an open activity panel. Three things are reachable twice (rename,
price scope, sharing); sort is offered twice (filter row and the cart's own headers);
status is shown twice.

## Decision

Extract the device page's frame into a shared `ListWorkspaceShell` and render the cloud
page into it. Cloud-only features move into the shell's ⋮ menu or a collapsed section
below the cart. Nothing is added to the device page except two cart features both pages
gain (hide-acquired toggle, units-acquired line).

## The shell

New component `ultros-frontend/ultros-app/src/components/list_workspace_shell.rs`,
`ListWorkspaceShell`. It owns layout only; every behaviour is passed in.

Rendered top to bottom:

1. **Back link** `← All lists` (`guest_workspace_back`) to `/list?labs=lists-sync`.
2. **Title row.** Left: the device page's inline name input (`text-2xl font-bold`,
   transparent background, border on hover/focus, Enter blurs, Escape reverts, commit on
   `change`, `maxlength=100`, test id `list-name-input`). Right: a `primary` slot followed by one ⋮ button
   (`online_more`, `aria-haspopup="dialog"`) that opens the `menu` slot in a `Modal`
   titled `online_more`.
3. **Status line** under the name: one `<p role="status">` with the caller's text and an
   optional trailing `status_actions` slot (the cloud save-failed Retry/Export buttons).
4. **Notices slot** (errors, "finish your edit" hint, price error, recovery panel).
5. **Build / Shop** `ListWorkspaceModes` at full size (not `compact`).
6. **Price row** (`flex flex-wrap items-center gap-2`): `WorldPicker` bound to the
   caller's scope, an optional `refresh` button slot (device only), then
   `ListTravelPanel`. Hidden while the caller says the page is read-only for scope
   (cloud non-admin still sees the travel panel; only the picker is gated).
7. **Children:** the Shop mount and the cart, exactly as each page builds them today.

Props (all `Signal`/`Callback`, `Copy`):

| Prop | Device | Cloud |
|---|---|---|
| `name`, `on_rename: Option<Callback<String>>` | handle name, `h.rename` | doc meta name (fallback server name); `Some` only when `can_admin`, else read-only input |
| `status: Signal<String>` | `handle.status` | see "Status text" |
| `status_actions` | none | Retry + Download recovery copy when save state is `Failed` |
| `status_testid` | `device-list-status` | `account-list-save-state` |
| `primary` | `DeviceListAdoption` | Access button (`list-access-btn`), `can_admin` only |
| `menu_testid` | `device-list-storage-toggle` | `list-settings-btn` |
| `menu` | separate upload, backup warning, Export, Delete + confirm (unchanged) | see "Cloud ⋮ menu" |
| `shop`, `set_shop` | in-memory | `buy` query param |
| `scope`, `set_scope`, `can_set_scope` | handle scope / price zone; not in recovery | doc scope / server `wdr_filter`; `can_admin && handle.is_some()` |
| `refresh` | Prices / Refresh prices button (`device-prices-refresh`) | none |
| `price_row_testid` | `device-price-controls` | `list-price-controls` |
| `travel` | `use_list_travel()` | same |
| `notices` | error, finish-edit hint, price error | `ListRecovery` only (mutation errors already reach the cart through `source.feedback`) |

### Status text (cloud)

One string, first match wins:

1. Save state `Failed` → `account_list_save_failed`, with Retry (`account_list_save_retry`)
   and Download (`account_list_save_export`) in `status_actions`.
2. Otherwise `"{save} · {live}"` where `save` is `device_runtime_saving` while `Pending`
   and `device_runtime_saved` when `Saved`, and `live` is the existing
   `list_view_live_status_*` label for the handle's status (`live`, `reconnecting`,
   `offline`, `connecting`). No document open (anonymous reader): the live label alone.
   The separator is a literal `" · "`, no new key.

`RealtimeStatus` (dot component) is no longer used on this page.

### Cloud ⋮ menu

Contents of the modal, in this order, each a `btn-secondary` row or inline section:

- **Subscribe** (`list_view_subscribe_button`) → opens `ListSubscribeDrawer`, closes menu.
- **Import from MakePlace** (`list_view_make_place`), `can_write` → opens the existing
  MakePlace modal, closes menu.
- **Auto-mark purchases**, `can_write` → the existing `AutoMarkPurchases` button (it
  opens its own modal); styled `btn-secondary` instead of `sticky-bar-button`.
- **Download recovery copy** (`account_list_save_export`) → `doc.download_recovery()`,
  when a document is open.
- **Leave list** (`leave_list`), `can_leave` → existing `leave_list` API, navigate to
  `/list` on success. Test id `list-leave-btn`.
- **Delete list** (`delete`), `can_admin` → inline confirm block identical to the device
  page's (`guest_workspace_delete_confirm`, Keep / Delete permanently), then
  `delete_list`, navigate to `/list`. Test ids `list-delete-btn`, `list-confirm-delete`.

The ⋮ menu counts as `modal_open` for the undo keybindings, like the device page's.

## Cloud page: removed

- The sticky bar and the `.list-toolbar` hook.
- The header card: eyebrow, h1 + pencil, `list-rename-btn`/`list-rename-input` flow with
  Save/Cancel, `RealtimeStatus`, the separate save-state block, progress bar, the three
  stat tiles, and the legacy bulk bar that lived inside it.
- The `ListFilterRow` panel (`list-filter-row`) and its Sort select.
- The "Price scope" labelled panel (`list-price-scope`); the picker moves into the shell's
  price row.
- `ListSettingsDrawer` and everything that reached it. Rename is inline; scope is the
  price row; sharing is the Access button; delete/leave are in ⋮.
- The `edit_list_action` rename path (replaced by `Edit::Rename` from the inline input,
  keeping the current scope).

`?cart=legacy` keeps rendering `ListBuildWorkspace` under the shell; it loses the bulk
bar and the per-world `ListSummary` stays as today below the grid. The URL `sort` param
still drives the legacy grid's pre-sort.

## Cloud page: kept, relocated

- **Activity feed:** below the cart, inside `<details>` (closed by default) whose
  `<summary>` is `list_view_activity_heading`. Same `ActivityFeed` component, same data.
- **Shop mount, recovery panel, MakePlace modal, Subscribe drawer, Access modal,
  bulk-delete confirm:** unchanged, opened from the new places above.

## Cart changes (both pages)

1. **Hide acquired** moves into `ListCart`'s search row as a toggle button
   (`list_view_hide_acquired`, `aria-pressed`). `ListWorkspaceSource.hide_acquired`
   and `reset_filters` already exist; `ListCart` gains `set_hide_acquired: Callback<bool>`
   on the source. Device wires it to an in-memory `RwSignal<bool>` (today it is a
   constant `false`); cloud keeps the `hide-acquired` query param.
2. **Units line** in `ListEstimateSummary`: a new `progress: Signal<Option<(i32, i32)>>`
   prop (`acquired`, `needed`), rendered as `list_view_units_acquired_progress` under the
   status line when `needed > 0`. `ListCart` computes it from `source.rows` before any
   filter: `needed = Σ quantity.unwrap_or(1).max(1)`,
   `acquired = Σ acquired.unwrap_or(0).clamp(0, needed_i)`. `pct = 100 * acquired / needed`.
   Test id `list-estimate-progress`.
3. The cart's sort UI (column headers on `sm+`, `<select>` below) is now the only sort
   control on both pages.

## Device page changes

Only what the shell extraction implies: `DeviceEditor` renders `ListWorkspaceShell` with
its existing pieces as props/slots. Markup, test ids, strings and behaviour are otherwise
unchanged, and the two cart features above appear.

## i18n

Reused: `guest_workspace_back`, `online_more`, `online_access`, `list_view_subscribe_button`,
`list_view_make_place`, `list_auto_mark_title`, `account_list_save_export`,
`account_list_save_retry`, `account_list_save_failed`, `device_runtime_saved`,
`device_runtime_saving`, `list_view_live_status_*`, `leave_list`, `delete`,
`guest_workspace_delete_confirm`, `guest_workspace_keep`,
`guest_workspace_delete_permanent`, `list_view_hide_acquired`,
`list_view_units_acquired_progress`, `list_view_activity_heading`.
Expected new keys: none. If one turns out to be needed it goes into all seven locales.

## Tests

Unit: shell renders read-only input without `on_rename`; cloud status text priority;
units line arithmetic (clamp, zero needed hides the line); hide-acquired filters
`visible` in `ListCart`.

Integration scripts to update (they locate removed elements):

| Script | Old hook | New hook |
|---|---|---|
| `list-flow.cjs`, `screenshots.cjs`, `shared-list.cjs` | `.list-toolbar`, `list-settings-drawer`, `drawer-rename-input`, `list-filter-row` | shell title input (`list-name-input`), ⋮ modal, cart hide-acquired button |
| `list-view-rename.cjs` | `list-rename-btn`, `list-rename-input` | `list-name-input` (type, Enter) |
| `list-adoption.cjs`, `labs.cjs`, `list-modal-keyboard.cjs`, `list-sync.cjs` | `list-settings-btn` opening the drawer | same id, now the ⋮ modal; `list-delete-btn` inside it |
| `shared-list.cjs` | `list-leave-btn` in drawer | same id in ⋮ |

`account-list-save-state`, `list-access-btn`, `guest-build-mode`/`guest-shop-mode`,
`list-price-controls`, and every device-page id keep their meaning.

## Non-goals

Redesigning the cart, changing the Shop view, touching the legacy `ListView`, changing
sync or storage behaviour, removing the `cart=legacy` escape hatch.
