# Analyzer grid and interaction audit

Audit date: 2026-09-20 (America/Denver). Production: https://ultros.app. Production's displayed version and the local checkout both identify commit `9b93be11f03fd3a14afdc04552eeb8467612c57f`. Three independent code-review subagents covered the grid core, crafting tools, and market tools. The primary agent operated production and checked their proposed reproductions.

**Recommendation: consolidate the analyzer experience around the existing grid, a shared toolbar, and explicit column/market contracts. Keep the domain calculations and routes separate.** The foundations are already shared. The debt is largely in the remaining adapters, duplicated metadata, incompatible metric meanings, and page-owned interaction policies.

This document records the original audit, before implementation. No application code, deployment, or account data was changed during that audit. Browser interactions changed local query/view state. Production market values are time-sensitive examples, not correctness benchmarks. Subsequent changes are described in [implementation.md](implementation.md).

## Evidence and walkthrough

Screenshots 01–20 were captured from production during the audit, saved, reopened, and visually inspected. `evidence.html` presents those screenshots and captions in capture order. Screenshots 21 onward document the later local implementation; the Currency captures use deterministic market fixtures. The production browser began without a home world, with price zone North-America. Most analyzer comparisons used explicit Gilgamesh routes and `v=1` to avoid restoring a previous view or applying landing presets; those screenshots should not be read as claims about first-visit preset defaults.

| Step | Flow inspected | Health and verified observations | Screenshots |
|---|---|---|---|
| 1 | Recipe: enter without home world, choose Gilgamesh, change sale scope, open columns, sort via menu then header | Richest market controls; no-home state still calculates with a blank sell selector; duplicate-looking metrics and competing sort state | 01–04 |
| 2 | Ventures: load Gilgamesh, add selected-window Sales/day | Works, but native and added sales rates mean different things | 05–06 |
| 3 | Leves: inspect world, formula, columns and result rows | Functional; “Select World for Prices” sits beside regional statistics, with native world history mixed in | 07 |
| 4 | FC Crafting: inspect formula, scope, result rows | Functional; “every project” wording overstates the eligible set according to source | 08 |
| 5 | Flip Finder: inspect formula, Filter menu and Views presets | Useful presets and actions; duplicate World/DC filter labels and separate Save view interaction | 09 |
| 6 | Vendor Sell: sort World through heading, then menu ascending | Confirmed comparator and sort-indicator defect | 10–11 |
| 7 | Vendor Resale: inspect formula, toolbar and rows | Clean shared controls; fixed item-width policy worth retaining | 12 |
| 8 | Scrip Sources: inspect formula, market subject and pricing note | Strong explicit ingredient/region disclosure worth retaining | 13 |
| 9 | Currency Exchange: choose Poetics | Blocked by missing global home world; page sends user to Settings instead of offering local market selection | 14 |
| 10 | Trends: choose Gilgamesh, inspect window and rows | Functional; distinct 30-day default and no 1-day selector, which reflect a different data product | 15 |
| 11 | Item Explorer: choose Gladiator's Arms, inspect Columns and mobile sort | Functional; useful visible sort control and scope support; no named Views surface | 16–17 |
| 12 | Recipe mobile: inspect formula wrapping and Columns popup | Controls wrap and popup works; initial grid viewport shows only Item | 18–19 |
| 13 | Recipe: enable Filter Outliers on the Ruthenium example | Confirmed gap: the troll listing remains; historical-sample cleaning does not reject implausible listing valuations | 20 |

Production observations below are marked **Observed**. Code findings not exercised in production are marked **Source**. Recommendations are proposals, not descriptions of current behavior.

## Findings, ordered by impact

### 0. Recommended views must exclude implausible opportunities — first-phase requirement

The user explicitly requested intelligent, filtered default views across the tools, with editable filters and custom saved views. Treat this as a product requirement for the consolidation, not optional polish.

**Observed:** On the explicit unfiltered Recipe view, Ruthenium Vambraces of Fending appears with a **999,999,999-gil price** and **949,998,990-gil profit**, while the UI itself labels the price “troll listing.” Enabling **Filter Outliers** leaves that row in place (20). This does not prove it passes the current first-visit `min-sales=1` default; it proves that the named outlier control is not a listing-plausibility filter. A velocity threshold alone also cannot exclude an implausible listing for an otherwise actively traded item.

**Source:** Recipe detects `price > 50 × median` when median is positive, but currently converts that finding into `CellNote::Troll` (`recipe_analyzer.rs:1208`; `ultros-calc/src/analysis.rs:386`). Its outlier switch cleans historical price samples using IQR and changes native sales-statistics inputs; it does not remove the listing-derived opportunity (`recipe_analyzer.rs:2690`; `ultros-calc/src/analysis.rs:88`).

Current landing policy is not uniform:

| Tool | Fresh landing behavior, before user/restored view overrides |
|---|---|
| Recipe | Seeds `min-sales=1`; richer profit/ROI presets are optional |
| FC Crafting | Seeds `min-sales=1`, plus hardcoded positive-profit eligibility |
| Ventures / Leves | No seeded landing filter; historical outlier cleaning defaults off, though presets turn it on |
| Flip Finder | Realistic flips: buy ≥5,000, last sold within one day, ROI ≥30%, sort Profit/day; existing plausibility and sales-quality guards |
| Vendor Resale | Next-sale ceiling one day; ROI descending; tax on; own >50× history-median guard |
| Vendor Sell | Positive after-tax NPC profit, sorted by profit |
| Trends | 30-day cleaned-activity ranking, maximum 500 candidates; sales-quality screening is not current-listing plausibility screening |
| Currency Exchange | Total return ranking, sale within 60 days; estimates bounded by latest sale and current listing, without shared suspicion classification |
| Item Explorer | Category and item-level order, no seeded opportunity-quality filter |
| Scrip Sources | Cost/scrip order, excludes unpriced/zero-cost candidates and ranks partial pricing later; no seeded view filter |

Sources: `recipe_analyzer.rs:479,4474`; `fc_crafting_analyzer.rs:160,798`; `venture_analyzer.rs:124,311`; `leve_analyzer.rs:138,289`; `ultros-ui-query/src/components/saved_views.rs:76`; `vendor_resale.rs:194,867`; `vendor_sell.rs:70`; `trends.rs:375,468`; `currency_exchange.rs:193,207,360`; `item_explorer.rs:633`; `scrip_sources.rs:340,570`.

**Required target:** every tool offers a named **Recommended** view with visible, editable filters; **Reset to recommended**, **Save view**, and **Make my default** use one view system. Keep an explicit unrestricted view. Clear filters must stay cleared rather than silently reseeding, and applying an explicit saved/shared view must not splice in extra defaults.

Share a trust assessment with separate reasons: suspicious listing, suspicious sales, stale quote, thin history, incomplete pricing, and unavailable evidence. Enable **Exclude suspicious listing valuations** by default on opportunity tools, using matching subject/quality/scope/window evidence. Start from the existing documented guard and calibrate further thresholds against fixtures and real markets. Do not invent a universal threshold or treat expensive items as suspicious solely because of price.

Tune additional default filters to the actual task: plausible after-tax return and sale evidence for resale; realistic return and crafting constraints for recipes; useful returned value for ventures; fixed rewards and acquisition cost for leves; complete ingredient pricing for scrips; executable NPC margin and listing freshness for Vendor Sell. Sparse high-value FC projects should not automatically inherit common-item demand thresholds. Explorer is a catalog: offer the requested useful default quality filters while retaining a clear Browse all route and distinguishing an unreliable quote from an invalid item.

Unknown/pending evidence must not be classified as safe or zero. Keep coverage visible; recommended rankings should separate unverified opportunities from verified ones, with an editable policy for including them. The grid's existing unknown-row retention must remain compatible with lazy enrichment, so filters cannot prevent the data needed to classify a row from loading.

Default-view precedence must be explicit. Proposed behavior: shared URL or directly chosen view wins; otherwise the user's selected default wins; otherwise Recommended. Offer “resume last view” as an explicit preference rather than letting an exploratory unfiltered session silently replace a chosen default. Preserve existing users' saved state during migration.

Add a Ruthenium regression fixture with an extreme listing, normal matching historical median, and enough genuine sales to pass velocity. Recommended must exclude it; disabling the suspicion filter must reveal it; unavailable history must produce an unverified state. Also validate that every built-in preset has a supported domain: Scrip Sources currently offers Orange Gatherers even though non-crafted collectables are skipped (`scrip_sources.rs:367,613,671`).

### 1. Same label does not guarantee the same market or statistic — high priority

**Observed:** In Ventures, with Gilgamesh selected, adding the selected-window `Sales/day (7d)` beside native `Daily Sales` showed Battle Galley at **0.3/day versus 4.86**, and Speckled Peacock Bass at **0.4/day versus 25.00**. Both appeared in the same row without market/quality subtitles on those headers. Screenshot 06 is the strongest demonstration of why the tools can feel inconsistent even when every calculation follows its own implementation.

**Source:** There are at least three contracts:

| Statistic family | Scope | Quality | Time denominator |
|---|---|---|---|
| Recipe native Daily Sales | Selected sell world | NQ and HQ combined | Fixed seven days |
| Venture/Leve/FC native Daily Sales | Selected world | Samples grouped by item, combining qualities | Elapsed time since oldest returned sample, at least one hour |
| Shared Sales/day column | MarketSubject scope; region on Venture/Leve/FC | Subject's exact quality | Selected or explicitly fixed window |

Sources: `routes/recipe_analyzer.rs:279`; `ultros-calc/src/analysis.rs:60`; `routes/venture_analyzer.rs:354,608,612,683`; `routes/leve_analyzer.rs:360,770,774,838`; `routes/fc_crafting_analyzer.rs:848`; `analyzer_kit/market.rs:3,690`. Paths beginning `routes/` or `analyzer_kit/` are under `ultros-frontend/ultros-app/src/`; other crate paths are under `ultros-frontend/` unless specified.

**Recommendation:** each metric must declare subject, geographic scope, quality policy, time window, unit (transactions versus units), estimator and missing-data status. Display essential provenance under the heading and in the picker. Keep a recent-sample estimate if it is useful, but name it distinctly. Do not alias an old statistic to a new one merely because the visible labels look similar.

### 2. Header and context-menu sorting are competing implementations — high priority

**Observed reproduction:** Open Vendor Sell on Gilgamesh. Click the World heading: the first displayed world is **Brynhildr**, with `sort=world`. Open that same column's menu and choose **Sort ascending**: the first world becomes **Adamantoise**, with `sort=grid:world&dir=asc`. The header arrow disappears after the menu sort. Screenshots 10–11 show the change.

**Source:** the native World comparator sorts numeric `world_id`, while the metric comparator sorts the displayed world name (`routes/vendor_sell.rs:163,241,447`). Native `SortHeader` considers every `grid:` token inactive (`ultros-ui-grid/src/components/sort_header.rs:334`). The menu writes metric tokens (`virtual_grid/filter.rs:171`), while `QueryGrid` resolves metric ARIA state (`query_grid.rs:124`).

**Observed:** Recipe Cost shows the same dual state path: menu descending writes `grid:cost&dir=desc`; clicking the header switches to native `sort=cost` and its default ascending order. This happens to reverse direction in that sequence, but the header is re-entering a separate contract, not resolving the active metric sort.

**Recommendation:** one resolved sort descriptor and comparator per column, used by heading, menu, toolbar, URL restoration, ARIA state and visual arrow. Preserve native tokens as aliases. Borrow Trends' existing metric headers and legacy sort-alias approach (`routes/trends.rs:467,655`). Add deterministic row-ID tie breaks; Venture and Leve native sorts currently lack them (`venture_analyzer.rs:386`; `leve_analyzer.rs:517`).

### 3. Market controls do not have a consistent meaning — high priority

**Observed:** Recipe directly exposes world/DC/region for both cost and sale-price signals. Changing sale scope to Datacenter resolves Aether, updates the formula's market label, and explains that a wider cheapest-listing comparison can lower the expected sale price. This is a good starting point for a common toolbar.

**Observed and Source:** Venture, Leve, FC and Scrip Sources display world selectors alongside a regional statistics label. Their regional listing/pricing behavior differs from the selected-world history used by some native columns. Vendor Sell also selects a world to derive a region-wide purchase market. Currency Exchange has no local selector and cannot show prices without global home-world setup. Explorer supports world/DC/region locally; Trends is world-only.

Sources: `recipe_analyzer.rs:4593,4638,4670`; `vendor_sell.rs:521,550`; `currency_exchange.rs:431,649`; `item_explorer_scope.rs:37,80`; `item_explorer_toolbar.rs:230`; `trends.rs:357`.

**Recommendation:** introduce a common market context with explicit roles: reference/home world, buy market, sale-price comparison market, and statistics market. Resolve and show actual names such as “Aether” beside scope choices. Expose only meaningful roles: Vendor Sell has NPC proceeds, not a sell-world market. World/DC/region support must be backed by each provider; a dropdown alone cannot make a world-only scan regional.

Also unify no-market entry behavior. Recipe's blank sell picker with calculated results (01), Trends' “select a valid world,” and Currency's Settings detour (14) currently offer three very different experiences.

### 4. Column pickers expose the history of implementation — medium/high priority

**Observed:** Recipe contains `Sale minimum (7d)` in Revenue, Cost, selected-window history and fixed-seven-day history groups. These are not all duplicates mathematically, but the repeated label is insufficient when choosing among them. The selected-window and fixed-seven-day variants look identical until the window changes. Flip's Filter menu contains two World buttons, two Datacenter buttons, repeated Confidence entries and several sales-rate families (09).

**Source:** routes still maintain separate option lists, visibility parsers, callbacks, column definitions, header matches and measurement matches. Recipe's 33-entry registry is a richer abstraction, but `AnalyzerGrid` is effectively Recipe-only and still infers types from column kinds/IDs and strips legacy table classes (`analyzer_kit/grid.rs:352,553,569`). Currency, Trends and Flip manually assemble picker state (`currency_exchange.rs:534`; `trends.rs:481`; `analyzer.rs:1339`).

**Recommendation:** build the picker and filter catalogue from one shared resolved column descriptor. Vendor Resale/Sell already use `ControlBar`'s registry fallback (`ultros-ui-query/src/components/control_bar.rs:415`). Extend that path to retain help, availability and disabled reasons from Explorer and Recipe, rather than replacing their useful metadata. Use “Follows window” and “Fixed comparison” groups, and qualify subject/scope in repeated metric labels. A searchable picker and explicit “Buy worlds” versus “Listing world” names would reduce ambiguity without removing power features.

### 5. Item sizing creates materially different mobile experiences — medium priority

**Observed:** At 393×850, Recipe initially shows only the Item column; its formula and setup controls consume much of the screen above the grid (18). Explorer exposes a toolbar sort selector even when the desired column is offscreen (17). Recipe's columns popup wraps and scrolls successfully (19).

**Source:** Flip, Vendor Resale and Vendor Sell opt Item out of auto-fit (`analyzer.rs:1485`; `vendor_resale.rs:740`; `vendor_sell.rs:430`). Recipe begins at 330px and permits fitting against result names (`analyzer_kit/grid.rs:375`); most other hosts also permit auto-fit. This is an explicit route-policy difference. Fixed width is not a frozen/pinned column: the current layout has order and widths, not a pinned-column facility.

**Recommendation:** share a bounded item-width default, preserve manual resize and explicit fit, and allow documented exceptions for richer rows. Borrow Explorer's visible sort control for small screens. Keep currency multi-shop rows and recipe metadata as domain-specific content; identical row heights are unnecessary.

### 6. Saving and restoring views is not a single contract — medium priority

**Observed:** Flip offers separate Views and Save view buttons. Recipe and most other tools use a generic Views menu. Explorer has no named Views surface. Currency's grid was not exercised because the session lacked a home world.

**Source:** auto-last-view restoration covers eight routes but excludes Trends, Currency Exchange and Explorer (`ultros-ui-query/src/last_view.rs:8`). Explorer explicitly disables named grid views (`item_explorer.rs:1273`). Flip has its own saved-view implementation with optional sell-world pinning (`ultros-ui-query/src/components/saved_views.rs:42`), whereas generic saved views remove `world` from saved query state (`ultros-ui-grid/src/components/virtual_grid/saved_views.rs:65`).

**Migration warning:** Flip's `?world=` means purchase-world filter, not the selected sell world. Its current dedicated serializer correctly preserves it. Naively moving Flip to the generic serializer would lose that filter. This is a consolidation hazard, not a currently reproduced data-loss bug.

**Recommendation:** one view system with declared parameter roles, named presets, automatic restore policy, and “include market” capability. Preserve old local storage keys, URLs, layouts and query aliases through a versioned migration.

### 7. Missing data, eligibility and calculation controls still diverge — high-value follow-up

These are **source findings**, not production failure-injection results:

- Venture/Leve/FC native sales statistics can turn unavailable history into numeric zero (`venture_analyzer.rs:354`; `leve_analyzer.rs:360`; `fc_crafting_analyzer.rs:641`). Recipe uses `GridValue::Unavailable` when its statistics request fails (`recipe_analyzer.rs:4259`). A zero must not silently stand for failure.
- FC drops zero-cost and nonprofitable projects before grid filtering (`fc_crafting_analyzer.rs:462`). The observed “No filters — showing every project” therefore does not describe the full project universe. Make eligibility explicit or use a removable visible “profitable only” default filter.
- FC omits the page-owned `provide_grid_saved_views` used by Recipe/Venture/Leve. The shared helper documents why storage state must outlive a Suspense table during a world switch (`virtual_grid/saved_views.rs:39`; FC `:616,793`). This is a credible lifetime-regression risk; it was not reproduced here.
- FC shared statistic “Use” shortcuts target ingredient cost methodology while the row's market subject is the completed product (`fc_crafting_analyzer.rs:592,630`). The shared code explicitly treats shortcuts as methodology switches, so this is not proof of incorrect arithmetic. Use an explicit subject-role/input-role mapping to make the target understandable.
- Currency's column setter uses default navigation options, while Flip/Trends explicitly preserve scroll (`currency_exchange.rs:538`; `ultros-ui-query/src/query_defaults.rs:80`; `analyzer.rs:1329`; `trends.rs:379`). A currency scroll-jump reproduction remains outstanding.

## What is already unified and should be retained

All 11 analyzer/grid route families use `MarketGrid → QueryGrid → VirtualGrid`; Recipe adds `AnalyzerGrid`. Shared infrastructure includes two-axis virtualization, one horizontal scrollport, stable row identity, filter aliases, hidden-column filters, sort/filter data-demand tracking, resize and fit, reorder, keyboard navigation, ARIA indexing, and distinct pending/missing/unavailable values. Shared sale-history columns cover nine following-window metrics and 36 fixed-window metrics. A new grid engine is not justified by these findings.

The core preserves manual widths and limits expensive text measurements when fitting. Virtualization limits mounted DOM, but fitting still scans result values and query sorting still processes rows. Recipe can perform a native sort before a metric sort (`recipe_analyzer.rs:3797`), an avoidable cost once sorting is unified. These are source observations; this audit did not profile frame times or memory on production.

| Borrow from | Capability to standardize |
|---|---|
| Recipe | Direct scope choices, resolved market labels, formula roles, price provenance/fallback explanations |
| Trends | Metric-based sort state with legacy aliases; keep cleaned statistics explicitly distinguished |
| Vendor tools | Registry-derived picker and bounded item-width policy |
| Item Explorer | Visible toolbar sorting; local scope selection; disabled-column reasons |
| Flip Finder | Useful named presets, optional market-pinned views, incomplete-data disclosure, copy/list actions |
| Scrip Sources | Explicit statement of market subject and pricing geography |
| Shared grid | Virtualization, layout/URL compatibility, keyboard/touch behaviors, pending-data safeguards |

## Proposed target and rollout

Use one analyzer shell with four small contracts. These names describe responsibilities, not a mandate for particular Rust types:

1. **Tool definition:** route identity, title/help, domain controls, result noun, presets and view policy.
2. **Market context:** meaningful geographic roles, selected quality, window, supported scopes and resolved labels.
3. **Column descriptor:** stable ID, typed value, metric provenance, renderer/formatter/measurement, comparator/default direction, filter editor, visibility, width policy, group and availability reason.
4. **Row/data adapter:** stable row key, domain calculation, market subjects, dependencies/coverage, navigation and actions.

The shell owns the consistent sequence: context and formula controls, result/coverage summary, Views/Columns/Filter actions, active chips, then grid. Calculation choices remain separate from row filters so Clear filters does not change the formula. Domain extensions occupy declared slots.

Migrate incrementally:

1. **Correctness and useful defaults:** add the Ruthenium plausibility regression and per-tool Recommended policy; reproduce the same-column sorting defect in tests; document metric meanings; fix World comparison and unified active-sort state. Lock down old URL behavior.
2. **Column ownership:** extend registered descriptors with missing presentation and sort metadata. Migrate a smaller route such as Ventures, then vendor tools. Remove duplicated metadata only once parity is verified.
3. **Market/formula shell:** extend the shared calculation-term model with scope and provenance, then bring Recipe's useful controls into it. Convert one dual-market tool and one fixed-market tool to avoid overfitting the API.
4. **View policy:** unify storage and URL serialization with explicit context/configuration roles. Preserve Flip's buy-world filters and optional market pinning. Move state above remountable Suspense boundaries.
5. **Remaining routes and retirement:** convert Recipe, Leve/FC/Scrip, Currency, Trends and Explorer. Retire misleading legacy columns only with explicit compatibility treatment; retain meaningful alternate estimators under distinct names.

Do not combine every calculation into a universal analyzer switch statement. Craft recipes, leve rewards, venture quantities, currency exchanges, vendor arbitrage and trend scans have real differences. Retainer undercut lists use bounded native tables rather than VirtualGrid (`routes/retainers.rs:154,254`); forcing those into a large virtual market grid is not supported by this audit.

## Verification required for the consolidation

Existing tests are a useful base, not evidence that this audit ran them. `integration/analyzer-grids.cjs` covers eight analyzers; `virtual-grid.cjs` covers geometry, bounded DOM, fit, resize/reorder, keyboard and touch; `shared-analyzer-data.cjs` covers shared metric semantics and data availability. Currency and Explorer have dedicated probes. The common matrix currently does not establish native/menu comparator parity across all routes. Browser E2E is local-only under the repository instructions.

For the audit artifacts, the required `check_ci.sh` was attempted. Windows' Bash launcher failed with access denied. Git Bash then lacked `dirname`/`grep` in its inherited PATH, and the Cargo stage encountered Windows `STATUS_DLL_INIT_FAILED` build-script/linker failures. The gate did not pass; no code was committed. These failures do not invalidate the production interaction evidence, but no CI pass is claimed.

Add cross-tool cases for: same-column header/menu/toolbar sort parity; stable ties; window-following versus fixed metrics; world/DC/region and quality provenance; missing versus zero; hidden filtered/sorted column loading; clear/reset boundaries; column insertion/resize after reload; old query aliases; named and automatic view restoration; market switching during storage events; direct SSR load versus client navigation; mobile picker/focus behavior.

Run `check_ci.sh`, `cargo leptos build`, relevant JavaScript regressions, and `scripts/run_e2e.sh` for the implemented consolidation. Preserve hydration and data-demand behavior; a native Rust check alone does not validate them.

## Accessibility and evidence limits

The production DOM exposes grid/row/header roles, named Columns/Filter controls, and `aria-sort` for metric sorting. The missing visual sort arrow despite active sort is a confirmed feedback problem. World comboboxes have visible adjacent text but appeared unnamed in the inspected accessibility snapshots; verify programmatic labeling with an accessibility test. Repeated checkbox/button names make scope/window selection harder to distinguish. Mobile icon controls retained accessible names, but touch-target size, contrast, focus order and screen-reader announcements were not comprehensively tested.

Mobile checks used viewport resizing at 393×850, not a physical touch device. Menus and layout were exercised, but this is not a WCAG conformance audit. The Currency grid and authenticated retainer flows were not exercised. No account login, named-view creation, injected failures, production network interception, performance benchmark, or exhaustive reset/history matrix was performed. Suspected lifetime and scroll defects remain explicitly unverified. Screenshots show particular live values and connection badges; they do not establish persistent service outages.
