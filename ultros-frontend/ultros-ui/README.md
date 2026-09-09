# Shared Ultros UI

Basic controls, links, icons, overlays, loading indicators and feedback state.
This crate has no market API, application route or game-data pack dependency.
Rendered grids and their controls live in `ultros-ui-grid`.

Applications supply router and locale contexts. Toast and clipboard providers
must use the definitions in this crate, also re-exported by frontend core and
the app. Enable `ssr` or `hydrate` to match the consuming application.

See the [frontend architecture](../README.md) for dependency direction, style
discovery and validation. A focused native check is:

```sh
cargo test --locked -p ultros-ui --features ssr
```
