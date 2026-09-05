# Recipe Analyzer Labs review — 2026-09-04

Reviewed baseline: `cc7003794ea4eace38b72df7a15b72061dcce5fc`.
This branch contains only Labs correctness changes and their regression coverage.

## Recipe Analyzer assessment

Reviewed the current implementation and relevant changes from PRs
[#1253](https://github.com/akarras/ultros/pull/1253),
[#1254](https://github.com/akarras/ultros/pull/1254),
[#1257](https://github.com/akarras/ultros/pull/1257),
[#1259](https://github.com/akarras/ultros/pull/1259),
[#1260](https://github.com/akarras/ultros/pull/1260),
[#1264](https://github.com/akarras/ultros/pull/1264),
[#1265](https://github.com/akarras/ultros/pull/1265), and
[#1266](https://github.com/akarras/ultros/pull/1266).

The formula, signal, column, and enrichment separation is useful. The explicit
buy/sell distinction, optional data fetching, virtual scrolling, stable URL
tokens, and deliberate SSR shapes are worth retaining. A wholesale rewrite is
not justified by this review.

Confidence in the existing tests was too high. Characterization fixtures prove
compatibility with previous arithmetic, not its business correctness. Some
tests inspect source text or helper booleans without exercising reactive
lifecycle transitions. Browser markup smoke checks do not prove data loads,
matches the displayed world, or settles after navigation.

### Confirmed remaining defects corrected in this change

1. **Revenue quality and market context could disagree.** #1266 selected a
   median using the ingredient HQ setting, while revenue continued to select
   the cheaper output quality. With only an HQ listing, an NQ median could
   falsely label a reasonable price as suspicious. The row now retains the
   winning revenue quality and uses that exact quality for its median, VWAP,
   other quality-specific statistics, sparkline key, and 30-day statistics.
   Missing same-quality history stays missing. Ingredient and revenue pricing
   policies are preserved.
2. **A slow response from an earlier visit to a world could overwrite data.**
   The shared enrichment hook compared world names only. An A → B → A sequence
   could therefore accept the first A response. Scope epochs now reject it.
3. **Changing worlds could clear enrichment without requesting it again.**
   Reset and fetch lived in separate effects; the fetch effect did not subscribe
   to the world. Reset and window selection now share an effect.
4. **Closing a fetch gate did not cancel a pending debounce.** An empty row
   mirror returned before invalidation, allowing a request after hiding a
   column or narrowing the viewport. Every window transition now invalidates
   pending debounce work before checking for keys.
5. **An unavailable vendor ingredient could hide its missing-cost warning.**
   Requiring HQ disables vendor fallback, but the warning still exempted any
   vendor-sold item. A zero-cost market line now counts as unpriced when no
   source actually priced it. This corrects the warning without changing the
   existing vendor or HQ pricing policy.

New tests use independent ingredient/output fixtures with both qualities,
missing history, ties, and sale-signal fallbacks, plus explicit request lifecycle
transitions. Already-started requests may still fill the current world's cache
after scrolling or hiding columns; this preserves useful cache reuse.

### Product semantics intentionally unchanged

- `require_hq` applies to ingredients. Revenue uses the cheaper NQ/HQ signal,
  as specified by the existing design. Choosing a particular output quality
  would be a separate product change.
- `Profit/day` is unit profit multiplied by transactions/day. That is an
  opportunity score, not a demonstrated earnings forecast; stack quantities,
  competition, and the player's market share are not modeled. Decide whether
  to rename/explain it or adopt a units-based model before promoting Labs.
- Trend and Drift remain deliberately unsortable because only visible rows
  have the required data.

## Validation and remaining acceptance work

- `cargo test -p ultros-app --lib routes::recipe_analyzer::test --offline`
  passed on the shared checkout with these recipe changes: 66 passed, 0 failed.
  The existing pricing oracle and all three new quality fixtures passed.
- An independent read-only review traced both enrichment consumers and found no
  blocking issue in scope reset, debounce cancellation, claim deduplication,
  stale-result rejection, or disposal handling. Tracker tests are not a browser
  lifecycle test.
- Rust formatting and whitespace checks passed for this isolated branch.
- This isolated branch passed `./check_ci.sh`, including the feature-gated
  `xiv-gen` lint. A first attempt hit a Git Perl/OpenSSL environment failure;
  the successful retry explicitly selected Strawberry Perl.
- The isolated `cargo test --locked -p ultros-app --lib` subsequently passed:
  792 passed, 0 failed, 0 ignored, with `CARGO_PROFILE_TEST_DEBUG=0` and two
  build jobs. Assertions and tests remained enabled.
- A populated combined audit build passed five bounded Puppeteer checks after
  its real `ultros:hydrated` event: narrow viewport request gates; opening both
  lazy feeds with the revenue quality; same-document picker A → B → A with a
  deliberately delayed stale response; the rendered matching-quality median;
  and world changes while hidden followed by successful reopening. No page
  errors occurred. Listings came from the disposable database; sale-history
  and sparkline responses were controlled fixtures to make ordering and
  quality differences deterministic.
- Those checks do not establish production market accuracy, live ClickHouse
  integration, fully priced crafting costs, or performance under load. Keep
  issue #1233 and the Labs opt-in open while those broader checks and the
  product semantics above remain unresolved.

## Integration with main on 2026-09-05

Merged main at `58c672d5`, including Phase F's wider revenue scopes and the
individual JSON changelog format. The conflict resolution keeps the winning
revenue quality for all home-world history and lazy-feed keys, while retaining
Phase F's suppression of price/median and price/VWAP ratios between different
geographical scopes. Added combined quality/scope/fallback fixtures. Updated
the default-scope oracle's five HQ fallback flags; all recorded prices and
fallback decisions remain unchanged.

- Full `./check_ci.sh` passed, including 840 app tests, 275 server tests,
  workspace tests, both Clippy invocations, and game-data pack sanity.
- `cargo leptos build --bin-features=""` passed for the WASM client and server.
  Empty bin features avoid the unsupported jemalloc build on Windows; existing
  local static OpenSSL artifacts were selected explicitly.
- The 66 local JavaScript regression tests passed.
- Seven controlled, hydrated browser cases passed on fresh table mounts:
  listing and sale-median pricing at world/datacenter/region scope, plus failed
  region-body fallback to the ingredient market. Assertions checked the price,
  exact home-world quality in 7d/30d history, settled sparklines, and suppression
  of cross-market ratios. No page errors. API responses were deterministic
  fixtures; this does not validate live ClickHouse data or fully priced costs.
- The standard route runner passed the selected home/changelog/Analyzer routes
  at desktop, mobile, and wide sizes. The broader driver's optional FC-crafting
  fixture timed out in the disposable database, and dashboard probes encountered
  unavailable analytics and third-party ad errors.

### Separate remaining row-refresh defect

A same-document revenue change from `listing-min` to `sale-median` can update
the header while leaving existing rows at their previous price. Reproduction:
use output 5056 with home listings NQ=100/HQ=500 and history NQ=110/HQ=550,
mount the listing view, then change `revenue=sale-median` without leaving the
route. The header reads median but the row still shows 100; remounting the table
shows the correct 110. `AnalyzerGrid` captures the row supplied to its keyed
`VirtualScroller`, and the scroller's `For` does not replace a retained key's
captured value. Both files and the recipe key implementation are unchanged from
main. The seven pricing checks above deliberately mount each configuration
afresh and do not establish correctness of in-place revenue-signal switching.
Keep this acceptance item open; it is outside the conflict resolution.
