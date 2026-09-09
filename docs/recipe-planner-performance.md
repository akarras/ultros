# Recipe planner checkbox investigation

Investigation of [issue 1334's feedback](https://github.com/akarras/ultros/issues/1334#issuecomment-5590693408), September 8, 2026.

## Reproduction and findings

The supplied reproduction is `/recipe/37835?world=Gilgamesh&lang=en&require-hq=false&shards-exclude=false&route=40%2C54%2C57%2C65%2C73%2C79%2C99&quantity=9999&unavailable=44062%3A73`.
It defaults to datacenter buying scope. Market data changes over time.

On production version `00909f6`, this loaded 1,131 purchase rows. Checking the
first Adamantoise Fire Cluster stack (2,000 at 29 gil) moved that checked listing
to the second row. The row count and selected spend (5,231,791,452 gil) stayed
the same. The selected route's badge changed from seven remaining world hops
to six: Adamantoise became a visited world. This reproduced the row shift,
not the reported two-to-four-world route change.

There are three separate mechanisms:

1. **Ordering:** `purchase_with_locked` sorts combined purchases by world and
   listing ID. Other solver branches return price order or market input order.
   Ticking changes which branch provides the displayed offer sequence even
   when the set of purchased listings stays identical.
2. **DOM replacement:** the itinerary was rendered inside a closure depending
   on the whole selected plan, with nested `collect_view()` calls. A change to
   travel metadata rebuilt all world panels and purchase inputs. At this size,
   replacing more than a thousand inputs is unnecessary work and loses focus.
3. **Synchronous search:** `locks → context → comparison → cards → selected`
   runs route comparison on the browser thread. Its per-item purchase cache
   exists only within one comparison, so a tick starts a fresh search. Work is
   bounded per purchase and per beam, but those bounds multiply across items,
   worlds and route candidates. A pinned route outside the frontier also runs
   `shop`, and the finished-item comparison can recalculate afterwards.

These code paths explain concrete risks; no production CPU trace was captured
to apportion time between search, allocation, layout and rendering. Standalone
WASM stress cases reproduce expensive search without any checkbox DOM, but
are synthetic and should not be presented as timings for this recipe.

## Changes

`ultros-calc`'s `planner::itinerary` gives every solver branch the same display ordering:
world grouping, then item, unit price, stack quantity and listing ID. The UI
memo compares only these rows, so a travel-only change does not invalidate
the itinerary. World panels and purchase rows use keyed `For` lists. A row's
key includes its item, listing ID, price and quantity, retaining unchanged
inputs while allowing a refreshed offer's displayed values to update.

This deliberately preserves existing shopping semantics: ticked listings keep
their price/quantity snapshots; other purchases may re-plan; ticked worlds
are free to revisit. An absent `route` still follows Best value, while an
explicit route is evaluated as a pinned candidate when necessary. Pinning the
entire purchase plan on the first tick would be a separate behavior change.
The fix does not make route search asynchronous or remove that remaining cost.

## Validation and profiling

Validation after rebasing onto the `ultros-calc` extraction, using
`nightly-2026-01-06`:

- `check_ci.sh` passes end to end in an isolated Ubuntu WSL copy of the patch,
  including both Clippy stages, workspace tests, standalone Universalis and
  CSV-conversion tests, and the game-data pack sanity check. This includes all
  902 SSR application tests and all 86 calculation-core tests.
- `cargo test --locked -p ultros-calc` also passes on Windows, including the
  new ordering regression.
- All 81 JavaScript unit tests, formatting and JavaScript syntax checks pass.
- `cargo leptos build --frontend-only` passes on Windows against the rebased
  patch, including JS/WASM generation.
- The new dense browser regression is implemented but **not verified against
  the patched app**. The earlier full Windows server build failed at MSVC
  linking; the Linux validation above runs tests, not a browser-facing server.
  Run the browser regression on a working server build before merging. The
  live reproduction above tests the old production implementation only.

The original Windows `check_ci.sh` attempt also encountered a vendored OpenSSL
build failure because Git Bash's Perl lacked `Locale::Maketext::Simple`.
Running the complete gate in Linux resolved that validation blocker without
adding compiler or toolchain workarounds to the repository.

The engine regression exercises exact, greedy and insufficient-supply branches
with prices deliberately ordered differently from listing IDs. The dense
fixture in `integration/recipe-planner.cjs` checks tick and untick preserve row
DOM identity, ordering, focus, spend and the explicit route parameter. It avoids
machine-dependent timing assertions; retaining the existing nodes directly
guards against the expensive rebuild.

The standalone harness includes the actual engine source and requires no
database, game-data packs or third-party dependencies:

```sh
mkdir -p target/recipe-profile
rustc --edition=2024 -C opt-level=z --target wasm32-unknown-unknown \
  --crate-type cdylib scripts/profile_recipe_planner.rs \
  -o target/recipe-profile/planner.wasm
node scripts/profile_recipe_planner.cjs target/recipe-profile/planner.wasm
```

For native comparison, compile the same Rust source as a binary. The exports
separate setup from search; setup prepares one selected purchase's locked
snapshot. Both runners warm each case and report five-sample medians for fresh
searches with and without that lock. Run benchmarks sequentially on an idle
machine, separately from builds. Node's WASM runtime measures engine cost,
not browser rendering, hydration, network cost or end-to-end click latency.
The app's release profile uses size optimization, so prefer `opt-level=z`
when comparing to production rather than an unoptimized development build.

For a production trace, use Chrome's Performance panel after the plan has
loaded: record a brief idle interval, tick several purchases individually,
untick one, then stop and save the trace. Keep the full recipe URL, browser
version, CPU-throttling setting, visible row count, and whether prices were
refreshed. Record load separately from interaction. Even without Rust symbols,
the trace can distinguish long script/WASM tasks from layout, paint and
third-party work. Symbols help attribute hot engine functions, but are not
required for that first split. See the [Performance reference](https://developer.chrome.com/docs/devtools/performance/reference)
and [trace export instructions](https://developer.chrome.com/docs/devtools/performance/save-trace).

For future search optimization, measure route comparison separately from
selected-route and finished-item calculations. Consider preserving purchase
results for unaffected items across ticks, with explicit invalidation for
market prices, quantities, vendor prices, HQ choices and unavailable reports.
If search remains a long task, a worker would keep it off the browser thread;
it needs stale-result cancellation and snapshot consistency, not just a delay
around the existing synchronous calculation.
