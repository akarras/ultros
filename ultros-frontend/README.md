# Frontend crate boundaries

`ultros-app` composes routes and provides contexts. Reusable views live in
functional crates, so changing a market card does not require compiling the
search controls, crafting controls, or alert editor again. Cargo still rebuilds
dependent crates and links the final application. This split does not promise a
particular build-time reduction.

| Crate | Responsibility |
| --- | --- |
| `ultros-ui` | Basic controls, icons, links, overlays, loading states, toast and clipboard state |
| `ultros-ui-grid` | Virtual grid, data table, scrolling, column controls and grid saved views |
| `ultros-ui-query` | Router-safe query signals, filter defaults, control bar and page saved views |
| `ultros-frontend-core` | API facade, shared application contexts, game-data access and realtime subscriptions |
| `ultros-ui-game` | Items, worlds, job cards, NPC locations and game terminology |
| `ultros-ui-charts` | Price history, chart controls, sparklines and market-history presentation |
| `ultros-ui-market` | Listings, sales, freshness, quality and market overview cards |
| `ultros-ui-alerts` | Alert rules, notification endpoints, history and subscription editors |
| `ultros-ui-lists` | Shopping-list rows, sharing, purchase controls and import UI |
| `ultros-ui-crafting` | Craft settings, recipe/set additions, inventory inputs and related items |
| `ultros-ui-shell` | Search, account/world/language/theme menus, mobile bar and update banner |
| `ultros-app` | Route composition, analyzer pages, app shell, social metadata and build version |

The existing calculation, pricing, crafting, grid-model, i18n, chart-rendering
and transport crates remain below these views. No extracted crate depends on
`ultros-app`. Keep page-specific orchestration in the app; put reusable views
in their functional crate and pass data or callbacks across a boundary when a
dependency would point back toward the app.

## Dependency direction

The base UI, grid and query crates do not depend on the frontend API facade or
game-data packs. The grid and query crates independently depend on the base UI
and pure grid model. The frontend core depends on the base UI's shared context
types and router-safe hooks.

Game views depend on the core. Charts add query controls. Market views compose
game views and charts; alerts compose game views and grids. List views compose
market and alert controls. Crafting and shell views use game views, with the
shell also using grids. Changes to one of these upper branches do not invalidate
its siblings, though their common consumer, `ultros-app`, must rebuild.

App-facing modules re-export the extracted definitions to preserve imports
while other work is in flight. These are aliases, not duplicate component or
context definitions. In particular, `RouterAvailable`, cookies, locale, user,
toast and update state must have one type shared by providers and consumers.
The app initializes contexts once; component crates consume them.

The app supplies its build commit to the core's update observer. Only the app
embeds `GIT_HASH`. The browser observer still works outside a reactive owner,
and its thread-local signal handle remains excluded from SSR.

## Features, styles and validation

The extracted UI library defaults are empty. Every consumer forwards `ssr` or `hydrate` through
its UI dependencies, including the corresponding Leptos, router, storage and
translation features. The browser build must not enable `ssr`. Native workspace
tests exclude `ultros-client` to avoid unifying its hydration feature with SSR.

`style/tailwind.css` explicitly scans every extracted crate. The SSR attribute
guard in `check_ci.sh` and the app's router-hook/source invariants also scan
the extracted source trees. Existing component tests and snapshots live beside
their implementations; dev-only browser fixtures stay in the app.

```sh
# Required native gate, including tests in the new workspace members.
./check_ci.sh
# Builds and bundles both client and server, including Tailwind.
cargo leptos build
# Example focused check while editing a functional crate.
cargo test --locked -p ultros-ui-market --features ssr
```

Run browser probes against a fresh build from the same worktree. Relevant
probes include `virtual-grid.cjs`, `shared-analyzer-data.cjs`, `app-update.cjs`,
`login.cjs`, the recipe/list/hover-card probes, and the route suite in
`integration/runner.cjs`.

Cargo-chef cooks placeholder workspace crates. Splitting source into crates
helps ordinary Cargo incremental reuse, but does not by itself preserve real
workspace artifacts across invalidated Docker source layers. That reuse also
depends on the build/cache arrangement. Keep comparisons on the same toolchain,
profile, target, features and cache; do not compare a cold build with a warm one.
