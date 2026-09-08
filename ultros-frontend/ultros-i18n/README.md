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
