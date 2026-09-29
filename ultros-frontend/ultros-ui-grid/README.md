# ultros-ui-grid

Rendered virtual grids, data tables, scrolling, sort headers, column filters
and grid saved views. Rows, columns and cell renderers come from consumers;
this crate has no frontend API or game-data dependency.

Enable `ssr` or `hydrate` to match the application. The `test-support` feature
exposes layout assertions used by app route tests and is enabled only through
the app's dev-dependencies. Browser fixtures remain in the app.

See the [frontend architecture](../README.md) for style discovery and validation.
