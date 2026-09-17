# New list Online / On this device toggle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Signed-in users creating a list from the Labs lists directory get an Online / On this device toggle (default Online) with a description that reacts to the choice; Online lists are created through the existing make-online handoff.

**Architecture:** The "New list" modal lives in `DeviceDirectory` inside the `#[cfg(feature = "hydrate")] mod browser` of `ultros-frontend/ultros-app/src/routes/guest_lists.rs`. Both choices still create the device IndexedDB document via `GuestListHandle::create` (the device UUID is the list identity). The only difference is the editor URL the modal navigates to: Online appends `&make_online=1`, and the editor's existing `DeviceListAdoption` effect performs the upload. A small pure helper builds that URL and is unit-tested.

**Tech Stack:** Rust, Leptos 0.8 (`view!`, `RwSignal`, `Show`, `Either`), `leptos-i18n` (`t!`/`t_string!`), Puppeteer e2e scripts under `integration/`.

## Global Constraints

- Every user-facing string goes through `leptos-i18n`; every new key is added to all seven locale files `en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc` in `ultros-frontend/ultros-i18n/locales/`, with real translations.
- Keys are `snake_case` and prefixed `online_new_storage_`.
- Run `./check_ci.sh` (fmt-check + clippy) before every commit. Log to the scratchpad, not `/tmp`. Check `REAL_EXIT` explicitly.
- Windows build env: prepend `/c/Strawberry/perl/bin:/c/Strawberry/c/bin:` to PATH and export `OPENSSL_RUST_USE_NASM=0` and `CARGO_PROFILE_DEV_DEBUG=0` before any cargo command.
- Do not touch the legacy lists panel (`LegacyEditLists` in `routes/lists.rs`) or the "Make online" card action.
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.
- Run every command from the worktree root `C:/Users/chw11/code/ultros/.claude/worktrees/list-online-local-toggle-20910d` and verify `git rev-parse --show-toplevel` prints it before committing.

---

### Task 1: `device_list_href` helper with unit tests

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/guest_lists.rs` (add a free function after the `GuestListRoute` component, around line 38, and a `#[cfg(test)] mod tests` at the very end of the file, after the closing `}` of `mod browser` at line 886)

**Interfaces:**
- Produces: `fn device_list_href(id: &str, online: bool) -> String`, cfg-gated `#[cfg(any(feature = "hydrate", test))]`, at module scope (NOT inside `mod browser`). Returns `/list/device/{id}?labs=lists-sync` when `online` is false and `/list/device/{id}?labs=lists-sync&make_online=1` when true.

- [ ] **Step 1: Write the failing tests**

Append to the end of `ultros-frontend/ultros-app/src/routes/guest_lists.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_list_opens_the_plain_device_editor() {
        assert_eq!(
            device_list_href("abc-123", false),
            "/list/device/abc-123?labs=lists-sync"
        );
    }

    #[test]
    fn online_list_resumes_make_online_in_the_editor() {
        assert_eq!(
            device_list_href("abc-123", true),
            "/list/device/abc-123?labs=lists-sync&make_online=1"
        );
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:$PATH" OPENSSL_RUST_USE_NASM=0 CARGO_PROFILE_DEV_DEBUG=0
cargo test -p ultros-app device_list_href 2>&1 | tail -20
```

Expected: compile error `cannot find function `device_list_href` in this scope`.

- [ ] **Step 3: Write the helper**

Insert after the closing `}` of `pub fn GuestListRoute` (line 38) and before `#[cfg(feature = "hydrate")] mod browser {`:

```rust
/// Editor URL for a freshly created device list. `online` appends the
/// `make_online=1` flag that `DeviceListAdoption` resumes in the editor, so
/// the upload reuses the same consent and scope handling as "Make online".
#[cfg(any(feature = "hydrate", test))]
fn device_list_href(id: &str, online: bool) -> String {
    if online {
        format!("/list/device/{id}?labs=lists-sync&make_online=1")
    } else {
        format!("/list/device/{id}?labs=lists-sync")
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test -p ultros-app device_list_href 2>&1 | tail -20
```

Expected: `test result: ok. 2 passed`.

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/guest_lists.rs
git commit -m "feat(lists): device_list_href helper for the New list handoff

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Locale keys in all seven locale files

**Files:**
- Modify: `ultros-frontend/ultros-i18n/locales/{en,fr,de,ja,cn,ko,tc}.json` (insert five keys immediately after the `"online_finish_edit"` line, which is line 2498 in every file)

**Interfaces:**
- Produces: locale keys `online_new_storage_label`, `online_new_storage_online`, `online_new_storage_local`, `online_new_storage_online_desc`, `online_new_storage_local_desc`, consumed by Task 3 through `t!(i18n, …)`.

- [ ] **Step 1: Write the insertion script**

Use the Write tool (not a bash heredoc) to create `<scratchpad>/add_storage_keys.mjs` with this content:

```js
import fs from "node:fs";
const root = "ultros-frontend/ultros-i18n/locales";
const keys = {
  en: {
    online_new_storage_label: "Where to save",
    online_new_storage_online: "Online",
    online_new_storage_local: "On this device",
    online_new_storage_online_desc: "Saved to your account. Open it on any device you sign in to and invite others when you want. Only you have access until you do.",
    online_new_storage_local_desc: "Saved only in this browser. No account needed, but it won’t follow you to other devices. You can make it online later from the list.",
  },
  fr: {
    online_new_storage_label: "Où enregistrer",
    online_new_storage_online: "En ligne",
    online_new_storage_local: "Sur cet appareil",
    online_new_storage_online_desc: "Enregistrée sur votre compte. Ouvrez-la sur n’importe quel appareil où vous êtes connecté et invitez d’autres personnes quand vous le souhaitez. Vous seul y avez accès tant que vous n’invitez personne.",
    online_new_storage_local_desc: "Enregistrée uniquement dans ce navigateur. Aucun compte requis, mais elle ne vous suivra pas sur d’autres appareils. Vous pourrez la mettre en ligne plus tard depuis la liste.",
  },
  de: {
    online_new_storage_label: "Speicherort",
    online_new_storage_online: "Online",
    online_new_storage_local: "Auf diesem Gerät",
    online_new_storage_online_desc: "Wird in deinem Konto gespeichert. Öffne sie auf jedem Gerät, auf dem du angemeldet bist, und lade bei Bedarf andere ein. Bis dahin hast nur du Zugriff.",
    online_new_storage_local_desc: "Wird nur in diesem Browser gespeichert. Kein Konto nötig, aber sie ist auf anderen Geräten nicht verfügbar. Du kannst sie später aus der Liste heraus online stellen.",
  },
  ja: {
    online_new_storage_label: "保存先",
    online_new_storage_online: "オンライン",
    online_new_storage_local: "この端末のみ",
    online_new_storage_online_desc: "アカウントに保存されます。サインインしたどの端末からでも開け、必要に応じて他の人を招待できます。招待するまではあなただけがアクセスできます。",
    online_new_storage_local_desc: "このブラウザーにのみ保存されます。アカウントは不要ですが、他の端末では利用できません。あとからリスト内でオンラインにできます。",
  },
  cn: {
    online_new_storage_label: "保存位置",
    online_new_storage_online: "在线",
    online_new_storage_local: "仅此设备",
    online_new_storage_online_desc: "保存到你的账号。在任何已登录的设备上都能打开，需要时可邀请他人。邀请之前只有你能访问。",
    online_new_storage_local_desc: "仅保存在此浏览器中。无需账号，但在其他设备上无法使用。之后可以在清单中将其转为在线。",
  },
  ko: {
    online_new_storage_label: "저장 위치",
    online_new_storage_online: "온라인",
    online_new_storage_local: "이 기기에만",
    online_new_storage_online_desc: "계정에 저장됩니다. 로그인한 모든 기기에서 열 수 있고, 원할 때 다른 사람을 초대할 수 있습니다. 초대하기 전까지는 본인만 접근할 수 있습니다.",
    online_new_storage_local_desc: "이 브라우저에만 저장됩니다. 계정은 필요 없지만 다른 기기에서는 사용할 수 없습니다. 나중에 목록에서 온라인으로 전환할 수 있습니다.",
  },
  tc: {
    online_new_storage_label: "儲存位置",
    online_new_storage_online: "線上",
    online_new_storage_local: "僅此裝置",
    online_new_storage_online_desc: "儲存到你的帳號。在任何已登入的裝置上都能開啟，需要時可邀請其他人。邀請之前只有你能存取。",
    online_new_storage_local_desc: "僅儲存在此瀏覽器中。不需要帳號，但在其他裝置上無法使用。之後可以在清單中將其轉為線上。",
  },
};
for (const [locale, entries] of Object.entries(keys)) {
  const file = `${root}/${locale}.json`;
  const lines = fs.readFileSync(file, "utf8").split("\n");
  const at = lines.findIndex((l) => l.trimStart().startsWith('"online_finish_edit"'));
  if (at < 0) throw new Error(`${locale}: anchor not found`);
  if (lines.some((l) => l.includes('"online_new_storage_label"'))) throw new Error(`${locale}: already added`);
  const added = Object.entries(entries).map(([k, v]) => `    ${JSON.stringify(k)}: ${JSON.stringify(v)},`);
  lines.splice(at + 1, 0, ...added);
  fs.writeFileSync(file, lines.join("\n"));
  JSON.parse(fs.readFileSync(file, "utf8")); // still valid JSON
  console.log(`${locale}: ok`);
}
```

- [ ] **Step 2: Run it and verify every file parses and has all five keys**

```bash
node "<scratchpad>/add_storage_keys.mjs"
for f in en fr de ja cn ko tc; do echo -n "$f "; grep -c '"online_new_storage_' ultros-frontend/ultros-i18n/locales/$f.json; done
git diff --stat
```

Expected: seven `ok` lines, a count of `5` for every locale, and `git diff --stat` shows exactly seven files each with 5 insertions.

- [ ] **Step 3: Check the i18n crate builds without missing-key warnings for the new keys**

```bash
cargo check -p ultros-i18n 2>&1 | grep -i "online_new_storage" ; echo "grep exit=$? (1 means no warnings mentioning the new keys)"
```

Expected: no output, `grep exit=1`.

- [ ] **Step 4: Commit**

```bash
git add ultros-frontend/ultros-i18n/locales/
git commit -m "i18n(lists): storage choice strings for the New list modal

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: Toggle, reactive description and create wiring in the modal

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/guest_lists.rs`
  - `DeviceDirectory` signals block (around line 109, next to `let busy = RwSignal::new(false);`)
  - `create` closure (lines ~188-212)
  - "New list" button (line ~266)
  - create modal markup (lines ~297-302)

**Interfaces:**
- Consumes: `device_list_href(id: &str, online: bool) -> String` from Task 1; locale keys from Task 2; existing `user_id: Signal<Option<u64>>` defined at line ~120 of `DeviceDirectory`.
- Produces: DOM `data-testid` values `device-list-storage-online`, `device-list-storage-local` (buttons with `aria-pressed`) and `device-list-storage-desc` (the description paragraph), consumed by Task 4.

- [ ] **Step 1: Add the storage signal**

In `DeviceDirectory`, directly after `let busy = RwSignal::new(false);` add:

```rust
        // Online is the default for signed-in users; reset each time the
        // modal opens so a previous Local choice does not stick.
        let storage_online = RwSignal::new(true);
```

- [ ] **Step 2: Reset the choice when the modal opens**

Change the "New list" button's `on:click` from

```rust
on:click=move |_| {error.set(String::new());set_creating(true);}
```

to

```rust
on:click=move |_| {error.set(String::new());storage_online.set(true);set_creating(true);}
```

(Only the button with `data-testid="list-new"`.)

- [ ] **Step 3: Use the choice in the create closure**

In the `create` closure, replace

```rust
            let name = name.get_untracked();
            leptos::task::spawn_local(async move {
                match GuestListHandle::create(name.trim()).await {
                    Ok(handle) => {
                        let id = handle.id();
                        handle.close();
                        go.try_with_value(|go| {
                            go(
                                &format!("/list/device/{id}?labs=lists-sync"),
                                Default::default(),
                            )
                        });
                    }
```

with

```rust
            let name = name.get_untracked();
            // Signed-out users never see the toggle, so never hand off online.
            let online = storage_online.get_untracked() && user_id.get_untracked().is_some();
            leptos::task::spawn_local(async move {
                match GuestListHandle::create(name.trim()).await {
                    Ok(handle) => {
                        let id = handle.id();
                        handle.close();
                        go.try_with_value(|go| {
                            go(&device_list_href(&id, online), Default::default())
                        });
                    }
```

Note: `user_id` is declared after `busy` but before `create` in the existing code (line ~120), so it is in scope. If the compiler says otherwise, move the `let storage_online` line to just after the `let user_id = …` line instead.

- [ ] **Step 4: Add the toggle and description to the modal**

Replace the create modal block

```rust
                <Show when=creating><Modal set_visible=set_creating aria_label=Signal::derive(move || t_string!(i18n,online_new).to_string())>
                    <div class="space-y-3"><h2 class="text-xl font-bold">{t!(i18n,online_new)}</h2>
                    <input class="input w-full" data-testid="device-list-name" aria-label=move || t_string!(i18n,list_name).to_string() placeholder=move || t_string!(i18n,guest_workspace_placeholder).to_string() prop:value=move || name.get() on:input=move |ev| name.set(event_target_value(&ev)) maxlength="100" />
                    <Show when=move || !error.get().is_empty()><p role="alert" class="text-red-400">{move || error.get()}</p></Show>
                    <button class="btn-primary" data-testid="device-list-create" disabled=move || busy.get() || name.get().trim().is_empty() on:click=create>{t!(i18n,create_list)}</button></div>
                </Modal></Show>
```

with

```rust
                <Show when=creating><Modal set_visible=set_creating aria_label=Signal::derive(move || t_string!(i18n,online_new).to_string())>
                    <div class="space-y-3"><h2 class="text-xl font-bold">{t!(i18n,online_new)}</h2>
                    <input class="input w-full" data-testid="device-list-name" aria-label=move || t_string!(i18n,list_name).to_string() placeholder=move || t_string!(i18n,guest_workspace_placeholder).to_string() prop:value=move || name.get() on:input=move |ev| name.set(event_target_value(&ev)) maxlength="100" />
                    <Show when=move || user_id.get().is_some()>
                        <div class="space-y-2" data-testid="device-list-storage">
                            <p id="device-list-storage-label" class="label font-semibold">{t!(i18n,online_new_storage_label)}</p>
                            <div class="flex flex-wrap gap-2" role="group" aria-labelledby="device-list-storage-label">
                                <button type="button" class=move || if storage_online.get() { "btn-primary min-h-11" } else { "btn-secondary min-h-11" } aria-pressed=move || storage_online.get().to_string() data-testid="device-list-storage-online" on:click=move |_| storage_online.set(true)>{t!(i18n,online_new_storage_online)}</button>
                                <button type="button" class=move || if storage_online.get() { "btn-secondary min-h-11" } else { "btn-primary min-h-11" } aria-pressed=move || (!storage_online.get()).to_string() data-testid="device-list-storage-local" on:click=move |_| storage_online.set(false)>{t!(i18n,online_new_storage_local)}</button>
                            </div>
                            <p class="text-sm text-[color:var(--color-text-muted)]" data-testid="device-list-storage-desc">{move || if storage_online.get() { Either::Left(t!(i18n,online_new_storage_online_desc)) } else { Either::Right(t!(i18n,online_new_storage_local_desc)) }}</p>
                        </div>
                    </Show>
                    <Show when=move || !error.get().is_empty()><p role="alert" class="text-red-400">{move || error.get()}</p></Show>
                    <button class="btn-primary" data-testid="device-list-create" disabled=move || busy.get() || name.get().trim().is_empty() on:click=create>{t!(i18n,create_list)}</button></div>
                </Modal></Show>
```

`Either` is already imported at the top of the file (`use leptos::either::Either;`, line 5) and `mod browser` does `use super::*;`, so no new import is needed.

- [ ] **Step 5: Build both flavors**

```bash
export PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:$PATH" OPENSSL_RUST_USE_NASM=0 CARGO_PROFILE_DEV_DEBUG=0
cargo check -p ultros-app 2>&1 | tail -5
cargo check -p ultros-app --no-default-features --features hydrate --target wasm32-unknown-unknown 2>&1 | tail -5
cargo test -p ultros-app device_list_href 2>&1 | tail -5
```

Expected: both checks finish with no errors (warnings about unrelated code are fine), tests `2 passed`.

- [ ] **Step 6: Run the CI gate**

```bash
./check_ci.sh > "<scratchpad>/ci.log" 2>&1; echo "REAL_EXIT=$?"; tail -30 "<scratchpad>/ci.log"
```

Expected: `REAL_EXIT=0`. If fmt fails, run `cargo fmt --all` and re-run. If clippy is OOM-killed (exit 137), re-run `cargo clippy --all-targets -j 2 -- -D warnings`.

- [ ] **Step 7: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/guest_lists.rs
git commit -m "feat(lists): Online / On this device toggle in the New list modal

Signed-in users default to Online; the description reacts to the choice.
Online lists still create the device document first and hand off to the
editor with make_online=1 so the existing adoption path does the upload.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: Keep the signed-in e2e adoption script creating local lists

**Files:**
- Modify: `integration/list-adoption.cjs:73` (inside `createDevice`)

**Interfaces:**
- Consumes: `data-testid="device-list-storage-local"` from Task 3.

Background: `createDevice` in this script is called both before sign-in (the first call, where no toggle renders) and after sign-in (every later call, where the toggle renders and defaults to Online). Every caller expects the created list to be local so it can exercise "Make online" itself. `integration/device-build-prices.cjs` and `integration/list-cart-feedback.cjs` create their lists while signed out, so they need no change.

- [ ] **Step 1: Choose Local when the toggle is present**

Replace line 73

```js
    await page.click(tid('list-new'));await replace(page,tid('device-list-name'),name);await page.click(tid('device-list-create'));
```

with

```js
    await page.click(tid('list-new'));await replace(page,tid('device-list-name'),name);
    // Signed-in sessions default to Online; this script exercises the local→online transition itself.
    const local=await page.$(tid('device-list-storage-local'));if(local){await local.click();await page.waitForFunction(sel=>document.querySelector(sel)?.getAttribute('aria-pressed')==='true',{},tid('device-list-storage-local'));}
    await page.click(tid('device-list-create'));
```

- [ ] **Step 2: Syntax-check the script**

```bash
node --check integration/list-adoption.cjs && echo SYNTAX_OK
```

Expected: `SYNTAX_OK`.

- [ ] **Step 3: Run the adoption e2e if a test-auth server is available**

Per `AGENTS.md`, this script runs from `./scripts/run_e2e.sh` when built with `LEPTOS_FEATURES=test-auth`. If a compatible server is already up (see memory notes on `BASE_URL`, `E2E_BLOCK_EXTERNAL=1`, and scratch ClickHouse), run:

```bash
BASE_URL=<server> node integration/list-adoption.cjs 2>&1 | tail -20
```

Expected: the script's existing `PASS` lines, in particular the first assertion that the sign-in continuation includes `make_online=1`. If no test-auth server can be brought up in this session, record that in the PR description as "adoption e2e not run locally" and rely on hosted CI.

- [ ] **Step 4: Commit**

```bash
git add integration/list-adoption.cjs
git commit -m "test(e2e): adoption script picks On this device when the toggle renders

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: Manual browser verification and PR

**Files:** none new.

- [ ] **Step 1: Serve the branch's own build and open the modal signed in**

Build and serve per the repo's local recipes (`cargo leptos build` with `LEPTOS_FEATURES="test-auth"`, then `PORT=<free port> METRICS_PORT=<free port> ./target/debug/ultros.exe`). In the built-in browser: visit `/test/login?user_id=990000001700&username=StorageToggleQA&redirect=/list%3Flabs%3Dlists-sync`, click "New list", and confirm:
  - The toggle renders with "Online" pressed and the Online description shown.
  - Clicking "On this device" flips `aria-pressed` and swaps the description.
  - Creating with Online lands on `/list/device/<id>?labs=lists-sync&make_online=1` and the editor header shows "Connecting…" then "Online".
  - Creating with On this device lands on the plain device URL with the "Saved on this device" status.

- [ ] **Step 2: Confirm the signed-out modal is unchanged**

Clear cookies (or open a fresh context), visit `/list?labs=lists-sync`, click "New list": no toggle, no description, create still lands on the plain device URL.

- [ ] **Step 3: Open the PR**

```bash
git push -u origin claude/list-online-local-toggle-20910d
gh pr create --base main --title "feat(lists): Online / On this device toggle in the New list modal" --body "$(cat <<'EOF'
## Summary
- Signed-in users see an Online / On this device toggle in the New list modal, defaulting to Online, with a description that reacts to the choice.
- Online lists still create the device document first and hand off to the editor with `make_online=1`, so the existing adoption path (consent record, scope, retries) is reused unchanged.
- Signed-out users see the modal exactly as before.
- Five new locale keys (`online_new_storage_*`) translated in all seven locales.

## Testing
- `cargo test -p ultros-app device_list_href`
- `./check_ci.sh` green
- `integration/list-adoption.cjs` picks On this device when the toggle renders (signed-in calls); device-build-prices and list-cart-feedback create while signed out and are unaffected.
- Manual: signed-in Online create lands on the editor with `make_online=1` and connects; signed-in Local create and signed-out create land on the plain device URL.

Spec: docs/superpowers/specs/2026-09-17-list-create-online-local-toggle-design.md

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```
