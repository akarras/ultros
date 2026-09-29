# New list modal: Online / On this device toggle

Date: 2026-09-17. Scope: the Labs lists directory (`DeviceLists` in
`ultros-frontend/ultros-app/src/routes/guest_lists.rs`). The legacy lists panel
is unchanged.

## Problem

The "New list" modal only asks for a name and always creates a device-only
list. A signed-in user then has to find the separate "Make online" card action.
Most signed-in users want an online list, so the default is wrong and the
storage difference is never explained at the moment of choice.

## Behaviour

1. **Toggle.** Below the name field the modal shows a two-button segmented
   control: "Online" and "On this device". It uses the same `aria-pressed`
   button pattern as the Shop mode buttons (`btn-primary` for the selected
   option, `btn-secondary` otherwise). Buttons carry
   `data-testid="device-list-storage-online"` and
   `data-testid="device-list-storage-local"`.
2. **Visibility.** The toggle and description render only when the login
   resource resolves to a signed-in user. Signed-out users see the modal exactly
   as today (local only).
3. **Default.** Online. The choice resets to Online every time the modal opens.
4. **Reactive description.** One muted paragraph under the toggle, switched by
   the choice:
   - Online: saved to your account, available on any device you sign in to,
     you can invite others, only you have access until you do.
   - On this device: saved only in this browser, needs no account, does not
     follow you to other devices, can be made online later from the list.
5. **Create.** Both choices create the device document first via
   `GuestListHandle::create`, because the device UUID is the list's identity
   (see `docs/lists-online-transition.md`). The navigation target then differs:
   - Local: `/list/device/{id}?labs=lists-sync` (unchanged).
   - Online: `/list/device/{id}?labs=lists-sync&make_online=1`. The editor's
     existing `DeviceListAdoption` resume effect performs the upload with the
     user's price-zone scope. If no scope is available the effect does not
     fire and the editor header offers "Make online" with its scope picker,
     which is the existing fallback.
6. **Errors.** Unchanged: creation errors show in the modal; online transition
   errors show in the editor as they do today.

## Implementation units

- `fn device_list_href(id: &str, online: bool) -> String` in `guest_lists.rs`
  (cfg `any(feature = "hydrate", test)`), unit-tested for both branches.
- A `storage_online: RwSignal<bool>` in `DeviceDirectory`, set to `true` when
  the modal opens; the create closure reads it and uses `device_list_href`.
- Modal markup: toggle + description inside a `Show` gated on
  `user_id.get().is_some()`.

## i18n

New keys in all seven locale files (`en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`),
translated, not stubbed:

- `online_new_storage_label` — "Where to save"
- `online_new_storage_online` — "Online"
- `online_new_storage_local` — "On this device"
- `online_new_storage_online_desc` — the Online description above
- `online_new_storage_local_desc` — the local description above

## Tests

- Unit: `device_list_href` both branches.
- E2E: `integration/list-adoption.cjs` (calls after its first sign-in),
  `integration/list-shop-focus.cjs` and `integration/list-priced-acceptance.cjs`
  create a list while signed in and assume it is local; each waits for and
  clicks `device-list-storage-local` before create. The other scripts that
  create device lists do so before any sign-in, so the modal is unchanged for
  them. `integration/list-modal-keyboard.cjs` keeps working (it only focuses
  the name field).
- `./check_ci.sh` must pass before commit.

## Out of scope

Scope picker in the modal, changes to the legacy lists panel, changing the
"Make online" card action.
