# What the seven embedded UI languages actually cost

Investigation of "stop embedding every UI language", September 22, 2026, on
`9b93be11`. Raw numbers: [docs/evidence/2026-09-22-i18n-locale-bundle-size.json](evidence/2026-09-22-i18n-locale-bundle-size.json).

`ultros-i18n` compiles all seven locales into every build, including the wasm
client. `leptos_i18n` 0.6.2 ships a `dynamic_load` feature that would fetch
them instead, and it is not enabled. The obvious read is that the client is
carrying six languages nobody on a given page will look at, and that the
locale sources — 1.28 MB of JSON across `en`, `fr`, `de`, `ja`, `cn`, `ko`,
`tc` — are a rough measure of the waste.

They are not. Most of that 1.28 MB is JSON syntax and key names, neither of
which reaches the binary. The text does, so it was measured directly.

## The prize: 169 kB brotli, 5% of the bundle

Deleting locales to measure them does not work — `Locale` is matched
exhaustively across the app, so a six-locale tree does not compile, and
dropping keys changes which code leptos_i18n generates. Instead,
[scripts/i18n_placeholder_locales.py](../scripts/i18n_placeholder_locales.py)
rewrites every locale value to a short unique token while keeping the key set,
the nesting and every `{{variable}}` marker exactly where it was. The generated
module keeps the same accessors, the same per-locale match arms and the same
interpolation builders; only the text shrinks. Both trees were then built with
[scripts/measure_wasm_size.sh](../scripts/measure_wasm_size.sh), which runs the
same wasm-bindgen / `wasm-opt -Oz` / `brotli -q 11` pipeline cargo-leptos does.

| build | `wasm-opt -Oz` | brotli |
| --- | ---: | ---: |
| baseline | 16,450,786 | 3,337,135 |
| placeholder locale text | 16,026,832 | 3,168,211 |
| **difference** | **423,954** | **168,924** |

So every translated string in all seven languages — 585,728 bytes of UTF-8 as
stored in the generated `const STRINGS` tables — costs **168,924 B on the
wire, 5.06% of the bundle**.

Two things shrink that further in practice:

- The default locale has to stay reachable synchronously whatever scheme is
  used, and `en` is 71,652 of those 585,728 bytes. The six others are 87.8% of
  the text, putting the ceiling near **148 kB brotli** — and a build of that
  end state, below, lands at **141,576 B, 4.24%**.
- The client already downloads a 4.47 MB game-data pack before it hydrates
  (`/static/data/{version}/{lang}`, served as the raw `.rkyv`). Against a
  ~7.8 MB first load, all seven languages of UI text are **2.16%**.

The placeholder tree carries a few hundred *extra* string slots — leptos_i18n
de-duplicates identical segments within a locale, and real translations repeat
much more than unique placeholders do (`en` stores 2,270 segments, the
placeholder build 2,705) — so if anything the table understates the text cost
slightly. It does not understate it by a factor that changes the conclusion.

## Blocker 1: `dynamic_load` makes `t_string!` async

Under `dynamic_load` the generated accessors return `LitWrapperFut`, whose
`build_string` and `build_display` are `async fn`
(`leptos_i18n-0.6.2/src/macro_helpers/mod.rs:130`). This is not a
client-only change: the SSR variant `LitWrapperFut<LitWrapper<T>>` is async
too, it simply resolves immediately. `t_string!` therefore stops returning
`&'static str` everywhere.

The app has **1,746 `t_string!` and 55 `td_string!` call sites across 126
files**, and 1,346 of them are immediately `.to_string()`d. They are almost all
view attributes — `aria-label`, `title`, `alt`, `placeholder` — which take a
value or a signal and have nothing to await into:

```rust
// ultros-frontend/ultros-ui-shell/src/components/language_picker.rs
<div role="radiogroup" aria-label=move || t_string!(i18n, language).to_string()>
```

A wrapper macro could hand back a `Signal<String>` fed by an `AsyncDerived`,
but that changes the type at all 1,801 sites (breaking `.to_string()`,
`format!`, and every use in ordinary non-reactive Rust), and every attribute
would render empty for a tick first. `t_string!` exists precisely because
`CLAUDE.md` requires attribute values to go through i18n; making it async
undoes that.

## Blocker 2: it moves the whole table into every SSR response

`build.rs` configures no namespaces, so the generated `I18nTranslationUnitsId`
is `()` — **one translation unit per locale**
(`leptos_i18n_codegen-0.6.2/src/load_locales/mod.rs:1225`). Under
`dynamic_load` + `ssr`, the first `t!` on a page calls `register()`, and
`RegisterCtx::to_array` then inlines that locale's *entire* table into the
response as `window.__LEPTOS_I18N_TRANSLATIONS`.

Measured from the generated tables, per page view:

| locale | JSON | brotli |
| --- | ---: | ---: |
| en | 78,507 | 23,061 |
| ja | 109,625 | 26,408 |
| de | 95,088 | 27,691 |

That is the shape of the trade: save 169 kB once, on an artifact the browser
caches, and pay 23–28 kB on every HTML response, forever. It breaks even at
roughly **seven page views** and is a loss after that. Namespaces are the
upstream answer — they split the unit so a page embeds only what it uses — but
that means partitioning 2,569 keys across seven files and rescoping every
call site in the app, and it does nothing about blocker 1.

## What a version without those blockers would need

Keeping the accessors synchronous means not using `dynamic_load` at all:
compile `en` in as today, and give the other six a runtime-populated table.
`ultros-i18n/build.rs` would post-process the 14.7 MB module leptos_i18n
generates — move the six non-default `const STRINGS` arrays into an
`ssr`-only file so the server can still serve them, point
`__get_<locale>_translations__` at a `OnceLock`, and turn the 19,460
`const I18N_TRANSLATIONS` and 21,326 `const S` bindings into `let` so they stop
being const-folded. The client would fetch the active locale's table next to
the game-data pack it already awaits before `hydrate_body`, and the language
picker would fetch before `set_locale`.

That end state was built and measured. A throwaway `build.rs` did exactly the
rewrite above — the six non-default tables replaced by `&[""; N]` behind an
`#[inline(never)]` `OnceLock` accessor, all 19,460 `const I18N_TRANSLATIONS`
and 21,326 `const S` bindings turned into `let` — leaving `en` compiled in.
It renders non-English blank, so it is a size probe and nothing more; the
script is archived at
[docs/evidence/2026-09-22-i18n-runtime-tables-probe.build.rs.txt](evidence/2026-09-22-i18n-runtime-tables-probe.build.rs.txt):

| build | `wasm-opt -Oz` | brotli |
| --- | ---: | ---: |
| baseline | 16,450,786 | 3,337,135 |
| six locales runtime-loaded | 16,193,062 | 3,195,559 |
| **saving** | **257,724** | **141,576** |

So the concern that de-constifying 40,786 bindings would eat the win was
wrong: the overhead is about 6.7 kB brotli against a 148 kB ceiling. The
design simply has a low ceiling. **141,576 B — 4.24% of the bundle, 1.81% of a
first load** — is what it is worth, and it buys that with:

- a build script coupled to the exact text `leptos_i18n_codegen` 0.6.2 emits,
  which no upstream contract covers;
- a second thing that must load before hydration, in a codebase whose
  hydration-panic history is documented at length in
  `ultros-frontend/ultros-client/src/lib.rs` and `ultros-app/src/lib.rs`;
- a new failure mode when that fetch fails — an English flash, or a skipped
  hydration like the one the game-data path already falls back to.

Not taken. The locale text is not where the weight is.

## When to revisit

- `leptos_i18n` grows a dynamic-load mode that keeps `t_string!` synchronous,
  or the app's `t_string!` usage shrinks enough that async is tractable.
- The app adopts i18n namespaces for unrelated reasons, which removes
  blocker 2 and makes upstream `dynamic_load` a per-page win rather than a
  per-page cost.
- The bundle gets small enough that 142 kB is a meaningful share of it. At
  3.34 MB it is not; the game-data pack and leptos/tachys view-tree code are
  both far larger targets.

Re-run the measurement with:

```bash
./scripts/measure_wasm_size.sh baseline && python3 scripts/i18n_placeholder_locales.py && ./scripts/measure_wasm_size.sh placeholder && git checkout -- ultros-frontend/ultros-i18n/locales
```
