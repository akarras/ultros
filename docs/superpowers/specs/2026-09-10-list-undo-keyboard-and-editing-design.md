# Lists: keyboard undo on device lists, and predictable undo while editing

Issues: #1429 (device keyboard undo/redo) and #1430 (predictable undo while
editing cart cells). Parent #1427, epic #1372. Delivered as two stacked PRs;
this document is the contract both implement.

## Findings from the source review

- `list_doc::undo::install` is the only keyboard listener. It is called once
  per opened `ListDocHandle` from `routes/list_view_sync.rs`; the device
  editor in `routes/guest_lists.rs` wires Undo/Redo buttons to
  `GuestListHandle` but never installs the listener. Ctrl+Z on a device list
  therefore does nothing. This is the reported bug.
- The listener ignores every keydown whose target is an `INPUT`, `TEXTAREA`,
  `SELECT` or contenteditable element, and the numeric cells additionally
  call `stop_propagation()` on every keydown. After a commit through Tab
  (focus lands in the next cell) Ctrl+Z is swallowed even though the edit is
  already in the document.
- Every `Edit` variant is already applied inside `ListUndo::group`, so each
  applied edit is one step. `ListUndo::MERGE_INTERVAL_MS = 1000` then merges
  consecutive steps closer than a second, so two quick cell commits revert
  together.
- Loro's undo manager only ever undoes local operations; the crate tests pin
  that remote imports never enter the stack and that undoing a write that a
  peer overwrote never resurrects the undone value.
- `rebase_onto_snapshot` deliberately restarts undo history (the operations
  the stack pointed at no longer exist).

## PR 1 — #1429: one shortcut adapter for account and device lists

`undo::install` takes an `UndoBindings { undo: Callback<()>, redo:
Callback<()>, modal_open: Signal<bool> }` instead of a `ListDocHandle`. The
account page passes closures over its opened handle (unchanged behaviour);
the device editor installs the same listener over `GuestListHandle`, with
its delete-confirmation panel as the modal guard.

Lifecycle: the device editor component is re-created whenever a different
list loads, and `use_event_listener` unregisters on owner disposal, so there
is exactly one window listener per open editor and none after navigating
away. `GuestListHandle::undo` is a no-op on a closed handle.

Bindings stay as they are: Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y on Windows and
Linux, Cmd+Z / Cmd+Shift+Z on Apple platforms (no Cmd+Y).

Verification: a new browser regression `integration/list-undo-keyboard.cjs`
covers a device list on direct load and after client navigation: add,
edit, delete, undo, redo, persistence after reload, and listener cleanup
(one Ctrl+Z reverts exactly one step after switching lists). Account
keyboard undo stays covered by `integration/list-sync.cjs`.

## PR 2 — #1430: the editing contract

### Draft versus committed action

A **draft** is text in an editor that differs from the value the document
holds. A **committed action** is any applied `Edit`, a rename, or a purchase.

Ctrl+Z / Ctrl+Y decide what to do from the keydown target:

| Target | Behaviour |
| --- | --- |
| Outside any editable element | document undo/redo |
| `SELECT`, checkbox, radio, button-like inputs | document undo/redo |
| A list editor carrying `data-committed` whose live value equals it | document undo/redo |
| A list editor carrying `data-committed` whose live value differs (a draft) | native text undo; the document is untouched |
| Any other input, textarea or contenteditable | native text undo |
| Any target while a modal or confirmation panel is open | nothing |

List editors are the cells (needed, owned, target price, quality), the
catalog search, the quantity-to-add input, the row filter and the list name.
Each renders `data-committed` from the same memo that feeds its value, so
the attribute is always the document's current value. An editor with a
draft keeps ordinary text-edit undo; once the draft is gone (Enter, Tab,
Escape, or native undo back to the committed value) the next Ctrl+Z is a
document undo. Numeric cells stop propagation only for the keys they handle
(Enter, Escape) so the shortcut can reach the window listener.

### Commits and grouping

Enter commits and blurs. Tab and any other blur commit. Escape discards the
draft and keeps focus. Quality and checkbox changes commit immediately.
Every committed action is exactly one undo step: `ListUndo` no longer merges
consecutive steps by time (`MERGE_INTERVAL_MS` becomes 0). Bulk operations
(recipe add, remove selected, bulk quality) stay one step through explicit
grouping, as today.

### Availability

Both handles expose `can_undo()` / `can_redo()` that track the document
revision, and `ListWorkspaceSource` carries them as signals. Toolbar
buttons are disabled with a tooltip when unavailable. A keyboard undo/redo
with nothing to do writes "Nothing to undo" / "Nothing to redo" into the
workspace feedback line, so a shortcut that appears to do nothing is
explained.

### Shared edits and reconnect

Undo reverts only this page's own operations (Loro guarantee, pinned by
crate tests). After a reconnect that merged remote updates the stack is
intact; after a snapshot rebase the stack restarts, and the buttons read
disabled until the next local action. The account regression presses
Ctrl+Z immediately after reconnect rather than after an arbitrary settle
delay.

### Verification

`integration/list-undo-keyboard.cjs` grows editor scenarios on the device
list: focused empty search, focused search draft, numeric cell clean and
draft, quality select, checkbox, delete-confirmation panel, focus outside
the grid, rapid consecutive edits undone one at a time, disabled buttons
when the stack is empty. `integration/list-sync.cjs` covers shared remote
edits and the reconnect timing on account lists.

Out of scope: deletion toasts and focus restoration after removal (#1436
territory), and any change to what a purchase undo restores.
