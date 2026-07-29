# First-load boot messaging

**Date:** 2026-07-26
**Status:** Approved, ready for implementation plan

## Problem

A cold load of Ultros downloads two large payloads before the page becomes
interactive: the `.wasm` bundle and the game-data `.rkyv` archive. Both are
cached afterwards — the wasm by the HTTP cache, the archive in IndexedDB — so
every subsequent load is dramatically faster. The user sees none of that. The
boot indicator says `Loading…` and nothing else, so a slow first load is
indistinguishable from a broken site, and there is no signal that waiting once
buys fast loads forever.

Worth stating plainly: this is not only a *first ever* load. The IndexedDB key
is `{data_version}-{lang}`, so a game-data version bump puts returning users
back on the same cold path. The messaging must cover that case too.

## Current behaviour

`shell()` in `ultros-frontend/ultros-app/src/lib.rs` injects an inline
vanilla-JS boot indicator into `<head>`:

- `#boot-progress` — a 2px gradient bar pinned to the top of the viewport.
- `#boot-progress-status` — a 12px label at `top:8px;right:12px`, text
  `Loading…`.

It advances on two events dispatched by the wasm's `hydrate()` in
`ultros-frontend/ultros-client/src/lib.rs` through the `dispatch_boot_event`
helper:

| Event | Effect |
| --- | --- |
| `ultros:wasm-loaded` | bar gets `.mid` (animates 75% → 92%) |
| `ultros:hydrated` | bar gets `.done`, fades, both elements removed |

A 30s watchdog flips the bar to `.error` and the label to
`Loading is taking longer than expected — reload`.

The slow work lives in `try_populate_xiv_gen_data_internal`: it opens the
`game_data` store, looks up `{data_version}-{lang}`, and on a miss fetches
`/static/data/{version}/{lang}.rkyv` and stores it.

All four boot strings (`Loading…`, `Failed to load app`, `App crashed during
load`, `Loading is taking longer than expected`) are currently hardcoded
English, which violates the project rule in `CLAUDE.md`.

## Design

### 1. The wasm reports the cold path

The boot script must **not** probe IndexedDB itself. `indexedDB.open('ultros')`
with no version creates the database at version 1 with no object stores; rexie's
`Rexie::builder().version(1)` would then find version 1 already present, skip its
upgrade callback, and never create `game_data` — permanently breaking data
caching for that user.

Instead the wasm, which already has the store open and knows the answer,
dispatches a new boot event on the miss path:

```rust
dispatch_boot_event("ultros:game-data-download");
```

Placed immediately before the `init_data().await` call in
`try_populate_xiv_gen_data_internal`, plus the two fallback paths in
`try_populate_xiv_gen_data` that also go to network (rexie build failure, retry
exhaustion). This is strictly more accurate than a JS-side probe and covers the
post-version-bump cold load for free.

### 2. Gate: event plus delay

`ultros:game-data-download` arms a 1200ms timer in the boot script. If
`ultros:hydrated` arrives first the timer is cleared and the label never
changes, so a cold load on a fast connection stays silent. Otherwise the label
is replaced with the first-load text.

The watchdog extends from 30s to 60s once `ultros:game-data-download` fires. A
first load on slow mobile can legitimately exceed 30s, and claiming failure
while the download is still progressing is worse than staying quiet.

Both the existing `finish` and `fail` paths already guard on the `done` flag and
`clearTimeout(wd)`; the new timer needs clearing in both.

### 3. Presentation: the existing label, wrapping

No new elements. `#boot-progress-status` gains
`max-width:min(70vw,320px);text-align:right;line-height:1.35` so a full sentence
wraps rather than running under page content on a narrow viewport. The element is
`position:fixed` and out of flow, so wrapping causes no layout shift.

`#boot-progress-status` is not currently marked `aria-live`, and this change
deliberately does not add it. The element is created and destroyed by a script
that runs before hydration, and an `aria-live` region that appears, mutates
twice, and is then removed mid-page-load is a plausible source of confusing
announcements. Screen-reader users are not left without a signal: the SSR HTML is
already complete and readable at this point, since the whole reason hydration can
be slow is that the server-rendered page arrived first. Revisiting this is a
reasonable follow-up, but it is a question about the boot indicator as a whole
rather than about this message.

### 4. Localisation: locale threaded into `shell()`

The boot script runs before wasm, so `t!` and `use_i18n()` are unreachable.
`leptos_i18n` provides `td_string!(locale, key)`, which resolves against an
explicit locale with no reactive context, and its generated `Locale` implements
`FromStr`.

- `shell()` gains a `locale: Locale` parameter and resolves all five boot
  strings itself, keeping every translation inside `ultros-app` where the
  generated i18n module lives.
- `render_leptos` in `ultros/src/leptos.rs` reads the `i18n_pref_locale` cookie
  from the `Request` it already holds and parses it to `Locale`, falling back to
  the default when absent or unrecognised. This is the same cookie
  `get_i18n_lang()` reads client-side, so server and client agree on language.
- The fallback `shell()` caller in `ultros/src/web.rs` passes `Locale::default()`.

Keys, added to **all seven** locale files (`en`, `fr`, `de`, `ja`, `cn`, `ko`,
`tc`) with real translations:

| Key | English |
| --- | --- |
| `boot_loading` | Loading… |
| `boot_first_load` | First load: downloading game data. Future visits will be much faster. |
| `boot_failed` | Failed to load app |
| `boot_crashed` | App crashed during load |
| `boot_slow` | Loading is taking longer than expected |

The four pre-existing strings move onto the same mechanism rather than leaving
a half-translated boot chrome. `boot_loading` is a separate key from the
existing `loading` key, which is used inside the app's view tree — they happen
to share a value today but serve different surfaces and should be free to
diverge.

### 5. Escaping

`td_string!` returns a `Display` type, not a `String`, so each value needs
`.to_string()`. The resolved strings are then collected into a JSON object with
`serde_json::to_string` — which handles quotes and backslashes — and passed
through the `<`/`>`/`&`/U+2028/U+2029 escaping that keeps the payload inert to
the HTML parser. The boot script reads the object instead of having strings
interpolated into it as bare literals.

`escape_for_script_tag` currently exists as a private fn in `ultros/src/leptos.rs`
and is unsafe for bare strings by design (it assumes serde_json already escaped
quotes). Since `ultros` depends on `ultros-app`, the helper moves up into
`ultros-app` and is made public; `ultros/src/leptos.rs` uses the shared copy for
its bootstrap payload. One escaping path, no duplication.

`ultros-app` already has `serde_json` as a dependency and already uses
`serde_json::to_string` to embed strings into an inline script in
`error_reporting_script`, so this follows an established pattern in the same file.

## Known limitation

The `ultros:game-data-download` event can only fire after the wasm has
downloaded and started executing, which on a truly cold load is already the
slower of the two payloads. A user on a bad connection sees plain `Loading…`
through the worst of the wait, then gets the explanation. Covering the wasm
download would require inferring the cold path from elapsed time alone, which is
strictly less accurate and was explicitly rejected. Accepted as-is: the message
sometimes arrives later than the pain does.

## Testing

- **Cold load** — clear IndexedDB and the HTTP cache, throttle the network,
  confirm the label expands to the first-load text and the watchdog does not
  fire before 60s.
- **Warm load** — reload with data cached, confirm `ultros:game-data-download`
  never fires and the label stays `Loading…` for its brief lifetime.
- **Fast cold load** — clear IndexedDB on an unthrottled local connection,
  confirm the 1200ms gate suppresses the message.
- **Version bump** — change `data_version`, confirm a previously-warm browser
  takes the cold path and shows the message.
- **Locale** — set `i18n_pref_locale` to each of the seven locales and confirm
  the boot label renders translated on first paint, before wasm loads.
- **Escaping** — confirm a translation containing an apostrophe, a double quote,
  and a `<` renders without breaking the inline script.
- **Rexie integrity** — confirm the `game_data` store still exists and populates
  on a browser that has never visited before, guarding the hazard in §1.
- **Reduced motion** — the existing `prefers-reduced-motion` block shortens the
  bar animation; confirm the label swap is unaffected.

## Out of scope

- The `.Jules` / `.jules` case-collision in the repo (two tracked paths for one
  file on a case-insensitive filesystem) — unrelated, worth a separate fix.
- Any change to the bar's animation timing or colours.
- Service-worker precaching of the wasm or the data archive, which would attack
  the root cause rather than the messaging.
