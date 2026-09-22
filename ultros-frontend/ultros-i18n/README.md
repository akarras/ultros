# Ultros translations

This crate owns the seven locale JSON files, their generated `i18n` module,
and the fallback context accessor. Application view edits can reuse the compiled
translations instead of including the generated Rust in `ultros-app`.

Dependencies must disable default features and forward `ssr` or `hydrate` from
their entry point. The crate has no default rendering mode. All consumers share
the same generated `Locale` and context types; do not generate another copy.

`ultros-app` re-exports `i18n` and retains its internal `i18n_fallback` alias so
existing page imports continue to work during subsequent crate extractions.
Edit translations in this crate's `locales/` directory. The build script tracks
locale changes; the translation helper scripts use this location too.

## `leptos_i18n`'s `dynamic_load` is off on purpose

All seven locales are compiled into every target, the wasm client included.
That is deliberate, not an oversight: measured on `9b93be11`, every translated
string in all seven languages is 168,924 B brotli — 5.1% of the client bundle,
2.2% of a first load — while turning `dynamic_load` on would make `t_string!`
async at 1,801 synchronous call sites and inline the active locale's whole
table into every SSR response. A scheme that avoids both — `en` compiled in,
the other six runtime-loaded — was built as a size probe and measured at
141,576 B brotli, 4.2%. Numbers, method and the conditions that would
make it worth revisiting are in
[docs/i18n-locale-bundle-size.md](../../docs/i18n-locale-bundle-size.md).

Validation:

```sh
cargo test --locked -p ultros-i18n --features ssr
./check_ci.sh
cargo leptos build
```

To inspect the build boundary, collect Cargo timings through cargo-leptos:

```sh
cargo leptos build --frontend-only --lib-cargo-args=--timings
cargo leptos build --server-only --bin-cargo-args=--timings
```

After warming each target, repeat following an application-only source edit.
`ultros-i18n` should remain fresh, while `ultros-app` recompiles. Translation
changes still rebuild this crate and its consumers. Compare timings on the same
machine, with identical profiles and features and no competing builds; this
boundary alone does not establish a percentage improvement in build time.
