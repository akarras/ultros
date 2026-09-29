# Issue #1482 implementation

The calculation strip is the only persistent editor for price assumptions.
Arithmetic badges connect its inputs to matching result columns. Row filters
keep their own toolbar and menus; clearing them preserves pricing, tax, scopes,
and the selected market window.

| Tool | Formula shown | Shared market-column Use target |
|---|---|---|
| Recipes | Profit = revenue − tax − ingredient cost, with buy/sell scopes | Existing compatible revenue/cost shortcuts |
| Flip Finder | Profit = sale estimate − tax − purchase price | Sale estimate |
| Vendor Resale | Profit = sale estimate − tax − vendor price | Sale estimate |
| FC Crafting | Profit = completed-item value − ingredient cost | Ingredient pricing |
| Leves | Profit = gil reward + item rewards − turn-in cost | Turn-in pricing |
| Ventures | Return = item value × quantity returned | Returned-item pricing |
| Scrip Sources | Cost per scrip = ingredient cost ÷ scrip reward | Ingredient pricing |
| Currency Exchange | Total gil = unit value × quantity received | None; existing fixed valuation |

Trends and Item Explorer have no profit assumptions and keep their existing
controls. Fixed-window statistics do not offer shortcuts that would select a
different window implicitly. For multi-ingredient tools, Use changes the pricing
method; it does not equate one ingredient's price with the full craft cost.

## Validation

- `check_ci.sh`: passed (formatting, both Clippy gates, deterministic Rust tests,
  CRDT convergence, feature-gated CSV tests, and game-data pack checks).
- `node --test integration/*.test.cjs`: 95 passed.
- `cargo leptos build --bin-features=`: passed (WASM, stylesheet, and server).
- Ten grid routes at desktop, mobile, and wide sizes: all 30 passed, with
  console, page-error, content, and overflow assertions enabled.
- Final `run_e2e.sh` focused run: passed. All seven pricing analyzers exercised
  direct selection, result recalculation, matching header Use actions, selected
  window labels, Clear all, hidden filters, sorting, reload, and mobile wrapping.
  Use buttons also stay entirely inside their grid headers. Trends and the
  deterministic shared-registry and market-window suites passed.
- Analyzer world URL compatibility, FC world consistency, all seven grid/view
  restoration probes, and Currency Exchange's valuation/quantity/mobile probe:
  passed.

The original broad driver also encountered two environment-dependent failures:
Google's headless ad script raised `Wl`/`Xl` in the search probe, and the FC
material-dialog probe found no market-backed rows in the empty database. The
same search assertions passed with the app's Hide Ads cookie; the same FC dialog
assertions passed on desktop and touch using the repository's deterministic
market fixture. Temporary diagnostic copies were removed. The final focused
driver ran against a freshly started binary from this checkout after its assets
finished building.

The local Windows preview disables the optional jemalloc feature with
`cargo leptos build --bin-features=`. The default allocator build fails in the
existing jemalloc C compiler setup on this host. An isolated Postgres container,
`ultros-formula-1482-pg`, supplies the test database; market-value assertions use
the repository's deterministic browser fixtures.

New labels currently use the English fallback in other locales.

## Screenshots

These populated screenshots use deterministic test prices. “Open analyzer” is
the harness's navigation link, not a product control.

- [Flip Finder, desktop](after-flip-finder-desktop.png)
- [Recipes, mobile](after-recipes-mobile.png)
- [FC Crafting, desktop](after-fc-crafting-desktop.png)
- [Leves, mobile](after-leves-mobile.png)
