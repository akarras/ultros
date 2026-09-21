# Symbolicated Rust WASM panic stacks in GlitchTip

**Date:** 2026-09-21
**Status:** approved design, awaiting plan

## Problem

Every client-side Rust panic reaches GlitchTip as a `RustWasmPanic` event whose
stack trace is useless, for two independent reasons:

1. **The stack is the wrong stack.** `report_rust_panic`
   (`ultros-frontend/ultros-client/src/lib.rs`) defers the JS reporter call via
   `set_timeout(0)` so the reporter never re-enters the wasm-bindgen-futures
   executor mid-poll. Sentry then captures `new Error()` *inside that timer
   callback*, so every event's frames are the timer trampoline
   (`__wbg_call → closure → __ultrosReportRustPanic`), not the panic site. The
   trap's own `RuntimeError: unreachable` — thrown on the panicking call stack —
   is the one event that carries the real frames, and `error_filter.js` rule 6
   drops it as a duplicate.
2. **The frames are anonymous.** cargo-leptos's release wasm-opt pass emits no
   `name` section, so even a correct stack reads
   `ultros.wasm:wasm-function[51239]:0xca0ed2`. GlitchTip has no wasm
   symbolication, so nothing upstream can resolve it.

Verified on the latest event of GlitchTip #7389 (2026-09-21): five
`wasm-function[N]` frames, all `?`, all from the trampoline.

## Non-goals (deferred, not rejected)

- `-Zbuild-std-features=panic_immediate_abort` for bundle size. It deletes the
  panic hook, message and `Location`; a separate decision once this work lets
  stacks carry the site on their own. `panic = "abort"` alone is a measured
  no-op on `wasm32-unknown-unknown` (target spec is already abort-strategy).
- Per-frame `file:line` from DWARF (`wasm-split`). The DWARF for a 16 MB module
  is tens of MB, there is no lightweight JS DWARF reader, and wasm-bindgen /
  wasm-opt degrade DWARF through their transforms. With the hook kept, the panic
  `Location` already gives `file:line` for the site; function names give the
  path there.
- A server-side symbolication endpoint. Dead exactly when errors spike.

## Design

Three small pieces. No new server routes; the only app-side Rust change is the panic hook.

### 1. Build: a per-release symbol map next to the wasm

**cargo-leptos config** (`[[workspace.metadata.leptos]]` in the root
`Cargo.toml`):

```toml
wasm-opt-features = ["-Oz", "--enable-bulk-memory", "--enable-nontrapping-float-to-int", "-g"]
```

`-g` makes wasm-opt keep the `name` section in its output. It must be `-g`
rather than `--symbolmap=…` because cargo-leptos 0.3.1 stores
`wasm-opt-features` as a `HashSet<String>`: argument order is not preserved,
and `--symbolmap` is a *positional pass* — before `-Oz` it dumps
pre-optimization indices, which are silently wrong after DCE and function
reordering. `-g`, `-Oz` and the `--enable-*` flags are all position-independent,
so the set is safe. wasm-bindgen has already demangled the names (its default),
with `rustc_demangle`'s `{}` form, i.e. a trailing `::h<16 hex>` per symbol.

**New workspace bin `wasm-symbols`** (`wasm-symbols/`, native only,
deps: `wasmparser`, `brotli`, `flate2` — all already in the lockfile):

```
wasm-symbols <pkg-dir>/ultros.wasm
```

- Parses the module with `wasmparser::Parser`, reads the `name` custom section
  (function-names subsection).
- Writes `<pkg-dir>/ultros.symbols`: one `index:name\n` line per named
  function, index ascending, with `::h[0-9a-f]{16}` suffixes stripped so names
  are stable across builds and the file is smaller.
- Rewrites `ultros.wasm` **without** the `name` custom section by copying every
  other section's bytes verbatim (header + section ranges from the parser; no
  re-encoding). Function indices and code are therefore identical to what
  cargo-leptos produces today without `-g`.
- Writes `ultros.wasm.br` / `.gz` and `ultros.symbols.br` / `.gz` (brotli q11,
  lgwin 22; gzip level 9), replacing the now-stale siblings that
  `cargo leptos build --precompress` wrote from the named module.
- Exit non-zero if the module has no `name` section (the `-g` plumbing
  regressed) — the build must fail loudly rather than ship a wasm with no map.

**Dockerfile**: one `RUN` after the `cargo leptos build` line:

```
cargo build --release -p wasm-symbols && \
  ./target/release/wasm-symbols target/site/pkg/ultros.wasm
```

`target/site` is copied into the runtime image already, so the map ships at
`/pkg/<GIT_HASH>/ultros.symbols` via the existing `pkg_service` `ServeDir`
(`ultros/src/leptos.rs`) with `precompressed_br` and
`public, max-age=31536000, immutable`. The Sentry `release` is
`ultros@<GIT_HASH>`, so map and module are versioned together for free.

Local `cargo leptos build --release` produces a wasm *with* names (bigger on
disk, harmless; browsers then show real names in stacks without any map). Dev
builds (`watch`) never run wasm-opt, so nothing changes there.

### 2. Rust: capture the stack before deferring

`report_rust_panic`:

- Captures the stack **synchronously** inside the hook via an extern binding
  `fn stack(error: &js_sys::Error) -> String` (the pattern
  `console_error_panic_hook` uses; `js_sys::Error` has no `stack` getter).
- Keeps the `set_timeout(0)` deferral for the *reporter call* — the re-entrancy
  hazard it guards against (GlitchTip 909/881/915) is unchanged — and passes the
  captured string as a third argument: `__ultrosReportRustPanic(message,
  location, stack)`.

`hydrate()` sets `Error.stackTraceLimit = 50` via
`js_sys::Error::set_stack_trace_limit` before installing the hook. V8's default
is 10 frames, which the panic machinery alone (`begin_panic_handler`,
`rust_panic_with_hook`, the hook closure, `console_error_panic_hook::hook`, the
`Error()` import…) consumes before the site is reached.

### 3. JS: rewrite the frames in `beforeSend`

**Reporter** (`error_reporting_script()` in `ultros-frontend/ultros-app/src/lib.rs`):
when `stack` is provided, set `error.stack = "RustWasmPanic: " + message + "\n"
+ <captured stack minus its first line>`. Sentry's stack parser reads
`error.stack`, so the event now carries the panic-site frames. Fingerprint stays
`["rust-wasm-panic", location || message]`.

**New `ultros-frontend/ultros-app/src/wasm_symbolicate.js`**, injected verbatim
like `error_filter.js`, defining `window.__ultrosSymbolicateEvent(event) ->
Promise<event>`:

1. Collect frames across `event.exception.values[*].stacktrace.frames` whose
   `filename`/`abs_path` matches `^(.*\/pkg\/[^/]+\/ultros\.wasm):wasm-function\[(\d+)\]`
   and whose `function` is empty or `?`. None → resolve with the event unchanged.
2. Map URL = the matched module URL with `ultros.wasm` → `ultros.symbols`.
   Deriving it from the frame (not from the SDK's `release`) guarantees the map
   matches the module that produced the frame, even in a tab that outlived a
   deploy.
3. `fetch` it once per URL: memoized promise, **failures memoized too** (no
   retry storm from a crash loop), `AbortController` timeout 10 s. On any
   failure resolve with the event unchanged.
4. Parse `index:name` lines into a `Map`. Set `frame.function = name`;
   `frame.in_app = /^(ultros|xiv_gen)/.test(name)`; leave `filename` alone.
5. Trim the panic machinery from the top of the stack (Sentry stores frames
   oldest-first, so the top is the array's end): pop while the last frame is a
   `ultros.js` glue frame or its function starts with one of
   `std::panicking::`, `core::panicking::`, `rust_panic`,
   `console_error_panic_hook::`, `ultros_client::set_panic_hook`,
   `ultros_client::report_rust_panic`. Never pop the last remaining frame.
   `core::option::unwrap_failed` / `expect_failed` are kept — the frame under
   them is the site and they name the failure mode.
6. Resolve with the mutated event.

**Wiring** in the existing `config.beforeSend`, after the drop check:

```js
if (window.__ultrosShouldDropEvent && window.__ultrosShouldDropEvent(event)) return null;
if (window.__ultrosSymbolicateEvent) return window.__ultrosSymbolicateEvent(event);
```

Sentry Browser SDK ≥ 7 accepts a `PromiseLike<Event | null>` from
`beforeSend`; we are on 10.52. Ordering matters: dropped events (rule 6's
`RuntimeError: unreachable`, injected-translation floods) must never trigger a
map download.

**`error_filter.js` rule 6 stays as-is.** With the real stack on
`RustWasmPanic`, the `RuntimeError: unreachable` twin is a pure duplicate again.

## Data flow

```
panic!() ──hook──▶ Error().stack (sync)  ──set_timeout(0)──▶ __ultrosReportRustPanic(msg, loc, stack)
                                                                     │ error.stack = stack
                                                                     ▼
                                                     Sentry.captureException ─▶ beforeSend
                                                                                   │ drop? → null
                                                                                   ▼
                                                                 __ultrosSymbolicateEvent
                                                                   fetch /pkg/<hash>/ultros.symbols (memo)
                                                                   frame.function = names[N]; trim; in_app
                                                                                   ▼
                                                                              GlitchTip
```

## Error handling

| Failure | Behaviour |
|---|---|
| `ultros.symbols` 404 (stale tab after deploy, local build without the tool) | event sent unsymbolicated — today's behaviour |
| fetch timeout / network down | same; failure memoized for the page lifetime |
| frame already has a name (local `-g` build, engine-provided) | left alone |
| `stack` missing (old reporter signature, non-panic caller) | reporter behaves as today |
| wasm has no `name` section at build time | `wasm-symbols` exits non-zero, Docker build fails |

## Testing

- **`wasm-symbols` unit test**: build a tiny module with `wasm-encoder`
  (dev-dep) containing two functions and a `name` section with hashed names;
  run extract + strip; assert the map text, that the output parses with no
  `name` section, and that the code section bytes are identical.
- **`integration/wasm-symbolicate.test.cjs`** (Node, same harness style as
  `error-filter.test.cjs`, `fetch` stubbed): names filled from a fixture map;
  `in_app` flags; machinery frames trimmed and the last frame never popped;
  fetch called once across two events (memoization); 404 and timeout leave the
  event untouched; events without wasm frames untouched; frames with existing
  names untouched.
- **Build check during implementation**: the stripped `ultros.wasm` from the
  `-g` pipeline must be the same size (±0) as a plain `-Oz` build of the same
  commit — confirms `-g` did not change optimization. Record the
  `ultros.symbols` raw/br sizes in the PR.
- **Prod verification**: the first `RustWasmPanic` event after deploy shows
  `ultros_*` function names with `in_app` highlighting. Check #7389 / #7309 /
  #7391 specifically — they are the recurring ones.

## Files

| Path | Change |
|---|---|
| `Cargo.toml` | `wasm-opt-features` with `-g`; add `wasm-symbols` to members |
| `wasm-symbols/{Cargo.toml,src/lib.rs,src/main.rs,tests/roundtrip.rs}` | new bin |
| `Dockerfile` | build + run `wasm-symbols` after `cargo leptos build` |
| `ultros-frontend/ultros-client/src/lib.rs` | sync stack capture, third reporter arg, `stackTraceLimit` |
| `ultros-frontend/ultros-app/src/lib.rs` | reporter accepts `stack`; inject and wire `wasm_symbolicate.js` |
| `ultros-frontend/ultros-app/src/wasm_symbolicate.js` | new |
| `integration/wasm-symbolicate.test.cjs` | new; add a `test:wasm-symbolicate` script. CI already runs `node --test integration/*.test.cjs` (`rust.yml`), so it is picked up automatically |
| `AGENTS.md` / `docs` | note the symbol map artifact and how to read a symbolicated event |
