# Shared Ultros UI

`ultros-ui` contains the rendered virtual grid, its query controls and saved
views, filter chips, icons, popover dismissal, and safe app links. It builds
against Leptos, `ultros-grid-core`, and `ultros-i18n` without application routes,
market APIs, or game-data packs. Callers supply rows, columns, and cell renderers.

The app retains its existing component import paths through re-exports. Router
and popover contexts are the same types for both the app and the grid; there are
no duplicate marker types. Browser fixture routes stay in the app, including
their existing release-build exclusion. Styles remain app-owned, with Tailwind
explicitly scanning this crate as well.

The default feature set is empty. Consumers must forward `ssr` or `hydrate`
to match their build, just as `ultros-app` does. The SSR feature forwards the
router, storage hooks, and translation configuration as well as Leptos itself.
The app's SSR-attribute CI guard also scans this crate.

Component bodies and existing tests move unchanged. The grid's `row_range`
re-export is public so existing app consumers can still use it, and its
hydration-only timer imports now use the same feature gate as their callers.
The generic grid can still be instantiated in the app; this boundary reduces
what must be recompiled with application code but does not establish a
percentage build-time improvement.

Validation:

```sh
cargo test --locked -p ultros-ui --features ssr
./check_ci.sh
cargo leptos build
```

The focused browser probes are `integration/virtual-grid.cjs` and
`integration/shared-analyzer-data.cjs`. Run them against a fresh build from
the same worktree. The shared analyzer probe can also exercise the real
flip-finder and recipe-analyzer routes with deterministic market API fixtures.
