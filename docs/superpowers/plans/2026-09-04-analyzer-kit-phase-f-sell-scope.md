# Analyzer Kit Phase F: Sell-Side Scope and Scope vs Home — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Under the existing `analyzer-recipe` Labs toggle, the recipe analyzer's revenue side gains a scope of its own — `?sell-scope=world|datacenter|region`, default `world` — so Price, the four `rev-*` columns and Profit can be *read* across the sell world's datacenter or region instead of only that one world, plus a `scope-vs-home` column saying how far the wider market sits from the player's own. With the toggle off, and with the toggle on at the default scope, every URL renders, fetches and computes exactly what it does today.

**This is a reference read, not a destination.** FFXIV retainers are home-world bound: a player may travel to *buy*, but may not list on another world's board. The repo already encodes that asymmetry — `analyzer_hop_gain_help` says "buy side only", `analyzer_hop_worlds_help` says "Buy side only", `formula_change_scope_aria` says "Change where ingredients are bought". Phase F must not become the first thing in the analyzer to claim a travel-to-sell action. Compounding it, the feature is built from *cheapest* primitives (`SignalView.over`, `stat_only_cheapest`) and the spec concedes the useful variant is absent — "a best-sell-world signal needs per-world maxima the cheapest maps do not hold and is left out" (spec L251). So there is no setting under which this names a place a player would earn more. Every string it ships says so: it answers "what is the going rate across my datacenter", never "sell it over there".

**Architecture:** The ledger already carries the slot: `ProfitFormula.sell_scope: Term<BuyScope>` has been `Fixed(BuyScope::World)` and unread since Phase A. Phase F seats it with `ProfitFormula::with_sell_scope(SellScope)`, a newtype whose `Default` is `World` (a bare `Scope` would default to the *buy* side's `Datacenter` and silently re-price every existing URL). `needed.rs` grows two roles, `BodyRole::{CheapestSellScope, SellScopeStats(u16)}`, gated on `sell_scope != World` and deduped against the buy side. `price_rows` splits the one "sell" input in two: the **sell place** (`revenue_listings`, `revenue_stats`) feeds `SignalView`'s `over` layer and `rev_alt`; the **sell world** (`sell_listings`, `sell_stats`) keeps feeding velocity, Avg price, Confidence, Last sold, Volume, VWAP, `stat_hq`, the sparkline key, the 30-day body and Hop gain's home side, exactly as the spec requires. `ScopeVsHome` is a `Layer::Computed` column over one new row field, `scope_vs_home: ScopeVsHome`, rendered by one new `CellValue::SignedGil`. The UI is one more `<select>` inside the revenue chip of the strip the Market button already opens.

**Tech Stack:** Rust 2024, Leptos 0.8.20 / reactive_graph 0.2.14 / tachys 0.2.18 (SSR + hydrate), leptos_i18n 0.6 (seven locales), the analyzer kit (`ultros-frontend/ultros-app/src/analyzer_kit/`), `ultros-api-types`.

**Specs:** `docs/superpowers/specs/2026-09-01-analyzer-kit-design.md` is binding — §1 asks 2, 3, 5, 7 and 19 (L46–52, L64), §2 decision 4 and 5 (L96–100), §3 module table and core types (L107–224), §4 the sell-side scope paragraph (L243–251), §5 the `ScopeVsHome` catalog row and the Travel picker group (L296–301), §6 the `CheapestSellScope` / `SellScopeStats(7)` fetch rule and the capacity table's fourth cache key (L310–355), §8 Phase F (L436–439) and the variant ledger's "F: the sell-scope roles" (**L389**), §9 "F adds the key `sell-scope` and the token `scope-vs-home`" (L459–480), §10 decision points 1 and 12 (L484–485, L505), §11 "Phase F's sell scope ships under the same token" (L525–526). Line numbers in the tasks below are against branch `integration-1265-1266` at **`8395bc02`** — `origin/main` (`55fa34d8`, Phase E2 as #1264) plus PR #1265 (the viewport-blind fetch gate) and PR #1266 (the median tell) — and they shift as tasks land. **Search for the quoted code, never trust an offset.**

**Not in this plan.** No comment is posted on #1233, and Kosyne is not asked anything: Aaron has approved Phase F as specified, including shipping it without the third-party reviewer's answer that spec §8 Phase 0 wanted first. The spec's declined-fallback ("rev-* columns at a fixed region scope without a selector", L439) is therefore not built.

**Task count: nine.** The eight-task draft folded the table's revenue resolution into the fetch task, and that is precisely where the phase's headline defect hid: the page seated the sell scope on *its* formula while the table's own `formula` memo (`:2648-2658`) never called `with_sell_scope`, so `sell_scope()` was `Fixed(World)` on every production render and the new column would have shipped as a permanently dashed column behind a green suite — Phase E2's median-tell escape repeated. Task 8 now exists for the table's resolution alone, with the structural pin that the harness and production share one seating function and `with_sell_scope` has exactly one call site.

## Global Constraints

Every task's requirements implicitly include this section.

1. **Flag-off byte-identity.** With the `analyzer-recipe` Labs toggle off, every URL must render the same DOM, issue the same requests and compute the same numbers. Phase E2 declared four carve-outs; Phase F must add none. **Every task states, in a "Flag-off" line under its Interfaces, how it verified this** — Tasks 3–9 all touch markup or a rendered value, and a claim with no stated check is not a check.
   *(Bookkeeping note for the reviewer: the E2 plan's own Global Constraints record **one** carve-out — the container-mode row-clip fix's `min-w-max` header band and `max-content` row spacer — plus one deliberate difference, the retired `?labs=analyzer-ledger` / `?labs=analyzer-signal-columns` tokens. Whichever count is authoritative, the operative rule for this phase is unchanged: **Phase F adds none.** The specific flag-off hazard Phase F introduces is a URL that carries `?sell-scope=…` or `?cols=scope-vs-home` while the lab is off; Task 6 and Task 9 pin that such a URL is inert down to the "no active filters" hint.)*
2. **A hidden optional child still emits a `<!>` marker in tachys** — dropping a column at build time (the grid's `lab_columns` prop) is the mechanism, not `?cols=` filtering alone. The new `scope-vs-home` column therefore carries `lab: Some(LAB_ANALYZER_RECIPE)` like every other Phase C–E2 column, and `BASE_COLUMN_ORDER` never learns its token.
3. **`#[prop(optional)]` on an `Option<T>` strips the Option** from the builder setter; use `optional_no_strip` when a caller must pass an `Option`. Phase F adds three `RecipeAnalyzerTable` props and one of them (`sell_scope: Option<SellScope>`) genuinely carries an `Option`, so **none of the three uses `optional`** — they are all required props and the caller passes `None` explicitly.
4. **No `#[allow]`.** Dead code between tasks is expected; it must be gone by the final task. `-D warnings` over `pub(crate)` modules means a field, fn or variant whose only readers are tests fails CI, so the branch-level gate is `./check_ci.sh` in Task 9; each task's own gate is `cargo test -p ultros-app --lib -- <filter>`, which tolerates dead-code warnings. **It does not tolerate compile errors**, so a task that adds a field to a struct with exhaustive literals fixes every one of them in the same task — see Task 2, which lists all six.
5. **Every user-facing string via `leptos-i18n` in all seven locales** (`en fr de ja cn ko tc`) with real translations, never English stubs. A key missing from a non-default locale only *warns* and falls back to `en`, so the seven-locale check is a key-count step in the task that adds the key, not a green build.
6. **The viewport gate (#1265) is load-bearing.** Any new lazy fetch must be gated the same way, and any new read of `wide_viewport` must terminate in an `Effect` — a guard test bans call syntax and `.with` on it precisely because `Signal<bool>` is callable and a read inside a `view!` would tear hydration. **Phase F adds no lazy fetch**: both new bodies are `Layer::Bulk` and join the Suspense gate, and `ScopeVsHome` is `Layer::Computed`. `the_page_wires_both_gates_to_what_it_fetches`'s `assert_eq!(reads.matches("wide_viewport.get()").count(), 2)` must therefore still read **2** at the end of this branch; Task 9 re-asserts it deliberately rather than by accident.
7. **Gate commands**, foreground and unpiped, exit read from a variable never a pipe:
   ```bash
   cargo test -p ultros-app --lib
   cargo check -p ultros-app --no-default-features --features hydrate --target wasm32-unknown-unknown
   ./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
   ```
   The default feature is `ssr`, so `--no-default-features` is required for the wasm check; run it with **no `RUSTFLAGS` in the environment** (an env `RUSTFLAGS` replaces `[build] rustflags` and fakes web-sys i32/f64 errors). On Windows, Strawberry Perl must lead `PATH`: `export PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:$PATH"`.
8. **Numbers: none for any existing URL** (spec §8 Phase F, L438). Every URL that does not carry `?sell-scope=datacenter|region` must produce byte-identical numbers, and the recorded oracle `price_rows_matches_recorded_oracle_on_fixture` must not move. **That oracle is not sufficient proof** — it projects six fields (`key_id, profit, roi, cost, market_price, tax`) from a run whose revenue signal is `ListingMin`, so it never exercises the sell-stat lookup, `rev_alt[1..=3]`, `revenue_fell_back` or `sell_median`. Task 3 records a second characterization oracle that observes exactly those values, **in two shapes**: the default fixture (every output has a sell-world listing) and `RunOpts { sell_listings: false, .. }` (no output has one), which is how the spec's "includes items with no sell-world listing" parity requirement (L246–247) is actually met.
9. **Run `cargo` in the foreground** inside subagents. No bare `git stash`. Branch `claude/issue-1233-phase-f-sell-scope`, cut from `integration-1265-1266` at `8395bc02`; the PR targets `main` and must be rebased onto `origin/main` once #1265 and #1266 have merged, or CI never runs (rust.yml only fires for base `main`).
10. **Plain-key `t_string!(i18n, key)` is `&'static str`**; only an interpolated key returns a builder needing `.to_string()`. Never `&t_string!(..)` in a `&str` position (`needless_borrow`).
11. **No `HashMap` iteration order may reach the DOM.** `HeaderExtras.by_kind`, `PickerContext` and both new statistics indexes are looked up by key only.
12. **SSR-render tests** (`to_html()`) that touch `<Gil>`, `<GilIcon>` or `t_string!` stand up the executor and an i18n context first, and any test creating an `RwSignal` runs inside an `Owner`:
    ```rust
    let _ = any_spawner::Executor::init_futures_executor();
    let owner = Owner::new();
    owner.with(|| {
        provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
        // ... render / signals ...
    });
    ```

## Decisions taken in this plan

| Question | Decision |
|---|---|
| What is this feature, in the player's language? | **A reference read.** Retainers are home-world bound, so the sell scope changes *which market the expected sale price is read from*, never where the sale happens. Every string ships that way: the aria-label is "Change which market the sale price is read from", the `world` option is "Your sell world" (not the buy side's borrowed "This world only"), the tooltip says outright that the number is never above zero under the cheapest-listing signal, and the changelog blurb carries "You still sell on your own world". |
| What is "a fourth Market select and strip term" (spec L436)? | **The fourth `<select>` reachable from the Market button, which under the lab is the stacked `FormulaStrip` inside that popover** — i.e. one `place_select` on the strip's revenue term, rendered by both the inline row and the popover. It is *not* a fourth `PricingSelect` in `MarketMenu`'s fallback branch: that branch is the flag-off popover (the `fallback` of `<Show when=move \|\| preview>`, `:430-436`), and adding a control there would be a flag-off DOM change (Global Constraint 1). |
| Does the revenue chip keep the place name once it grows a scope select? | **Yes — `place: Some(revenue_place)` *and* `place_select: Some(…)`,** so the chip reads `+ [7d median ▾] · Aether · [Datacenter ▾]`. The cost chip's precedent (`place: None`) would drop "Gilgamesh" from the default lab-on view, and `StripSelect.options` is a plain `Vec<(&'static str, String)>` captured at build time, so putting the resolved place name into the option labels instead would be non-reactive and could stick on "…" forever. The redundancy at `world` scope is accepted deliberately. |
| `SellScope` newtype or a bare `Scope`? | **Newtype.** `Scope::default()` is `Datacenter` (the buy side's default). A bare `sell.unwrap_or_default()`, or the default-stripping setter idiom this page uses everywhere (`parsed.filter(\|s\| *s != Scope::default())`), would move the sell side to the datacenter on every existing URL and strip the wrong token — the single number change this phase must not make. `SellScope::default() == SellScope(Scope::World)` makes both idioms correct by construction. |
| Change `recipe_from_query`'s signature or add a builder? | **Builder.** `ProfitFormula::recipe_from_query` has **33** call sites; a fourth parameter is 33 mechanical edits for no benefit. `with_sell_scope(self, SellScope) -> Self` is called from exactly **one** place in the whole crate — `seat_sell_scope` — and a caller that never reaches it keeps `Term::Fixed(BuyScope::World)`, literally today's value, so the flag-off `ProfitFormula` is `PartialEq`-identical to today's and `Memo<ProfitFormula>` cannot fire on it. |
| How does the *table's* formula get the scope? | **Through `seat_sell_scope`, the same function the page and the test harness use.** This is the phase's headline hazard: the page's `formula_page` memo and the table's own `formula` memo (`:2648-2658`) are two different constructions of the same ledger, and only the second one prices rows. `seat_sell_scope(f, preview, param)` is the only caller of `with_sell_scope`, `run_with` builds its formula through it too, and Task 8 pins the call counts so an edit that unwires the table fails the suite instead of shipping a dashed column. |
| Which lookups follow the sell scope, and which stay on the sell world? | **Follow the scope:** `market_price` (the `SignalView` `over` layer), all four `rev_alt` entries, `revenue_fell_back`, the Price / Revenue header marks, the picker's "Revenue · ‹place›" heading and the live info sentence's `sell` slot. **Stay on the sell world** (spec L247–248): `daily_sales`, `avg_price`, `total_sales`, `last_sold_unix`, `units_sold`, `vwap`, `vwap_pct`, `confidence`, `stat_hq`, the sparkline key, the 30-day body, Hop gain's home run and Worlds to visit. The Daily sales / Confidence / Trend / Drift sub-labels therefore keep saying "7d · ‹sell **world**›" whatever the sell scope is; Task 5 pins that, because the one variable both need is spelled `sell_place` today and getting it wrong is silent. |
| The Price median tell under a wider scope | **Suppressed, not re-based.** `price_note` compares the row's price against `sell_median`, the sell world's 7-day median. Move the price to a region and the two operands stop describing the same market: the tell would read negative and **red on nearly every row**, caused by the user's own setting rather than by a suspicious listing. PR #1266 was merged specifically to make that tell trustworthy, and a page-wide false alarm is how a colour stops being read. A scope-wide median is not the answer either — it needs the sell-scope statistics body, which is only fetched under a sale revenue signal, so the tell would appear and vanish with an unrelated selection. So `price_rows` leaves `sell_median: None` when the sell scope is not the world, `price_note` falls to `ListingFallback` / `None`, and the sub-line keeps its shape. Pinned by `the_price_median_tell_is_suppressed_at_a_wider_sell_scope`. |
| `scope-vs-home`'s sign convention and its three states | `delta = revenue signal at the sell scope − the same signal on the sell world's own map`. Under `listing-min` the delta is **at most zero** — a region contains the world, so its cheapest listing can only be lower — which is a genuine finding, not a bug: a wider market has more sellers undercutting each other. Under a sale statistic it goes either way. The row records one of three states (`ScopeVsHome::{Off, Unavailable, Pair}`) rather than an `Option`, because the dash otherwise means four things at once and the header tooltip can only name one: `Off` is "not asked for, or the sell scope IS the sell world" (the whole column is dashes, and the tooltip's last sentence says so), while `Unavailable` is "asked for at a wider scope, but one of the two markets has no figure for the signal" — the dominant case under a sale signal, where `item_stats_window` covers roughly 7% of traded items — and it carries `analyzer_drift_unavailable` as the dash's `title`, exactly as `CellValue::LazyPct`'s empty state does one file over. |
| `scope-vs-home`'s percentage, and how it avoids E2's defect | **Clamped, and muted wherever the sign is the whole message.** Phase E2 shipped a coloured percentage whose green arm meant "do not trust this", and #1266 corrected it with two guards four commits ago; Phase F inherits both rather than re-earning them. The percentage is against the *home* value, clamped to `VS_MEDIAN_DISPLAY_CEILING_PCT` (which exists because prod rendered `+399900%`), and dropped to `None` — so `signed_delta_class` renders muted, no colour — in two cases: when the revenue signal is a listing, where the delta is structurally ≤ 0 and a permanently red stripe in the codebase's warning colour would teach players to ignore the colour; and when `is_troll_listing(place, home)` fires, i.e. the only way the cell could render **green** is a home figure so thin that the scope statistic is 50× it. That is E2's defect mirrored, and it is gated by the same helper `price_note` uses. |
| Where `scope-vs-home` sits in the table and the `?cols=` contract | **Appended after `vwap-30d`, immediately before Actions**, exactly as E2 appended its five — so every serialized old `?cols=` stays byte-identical. Its `PickerGroup` is `Travel` (`columns.rs:77`), and `grouped_picker_options` sorts by `(group, table index)`, so it still lists third in Travel behind `hop-gain` and `hop-worlds` despite being last in the table. |
| Which body does the dedupe actually save? | **The cheapest listings body always; the statistics body only when the buy side really fetched one.** `CheapestBuyScope` is unconditional, so a sell scope that resolves to the same place name as the buy scope reuses it outright. `BuyScopeStats(7)` is itself conditional, so `SellScopeStats(7)` is suppressed only when `BuyScopeStats(7)` is *in the computed set* — deduping against a body that was never fetched is how a cell ends up permanently "—". The page therefore hands `sell_scope_key` a `RecipeNeeds` carrying `buy_scope_is_sell_world` from the page's real gate rather than `Default`: `needed_bodies` computes `BuyScopeStats` from that field, and a defaulted `false` can disagree with `:3962`. |
| Do the two new bodies join Suspense? | **Yes** (spec L313: "Formula bodies join the Suspense gate"). They price the ledger, so the table cannot render without them. The cost — up to 578 KB on the wire for a region — is opt-in and paid only by a URL that asked for a non-default sell scope. Neither is viewport-gated, so Global Constraint 6's `wide_viewport.get()` count stays at 2. |
| What happens when a sell-scope body fails? | **It is said, never silently re-priced.** `SellScopeBodies` tracks `listings_failed` *and* `stats_failed`; either one raises a second amber line naming the place (`recipe_analyzer_sell_scope_unavailable`), because the strip, the picker heading and the live sentence all still say "Aether" while the numbers have fallen through `SignalView`'s base layer to the buy scope. A silent fallback under a label that still names the scope is the worst of the three options. |
| How is flag-off inertness made testable? | **Two pure helpers, introduced together in Task 1.** `sell_scope_for(preview, param) -> Option<SellScope>` is the lab gate; `seat_sell_scope(f, preview, param) -> ProfitFormula` is the only thing in the crate that calls `with_sell_scope`. Every site that acts on the sell scope goes through one of them — the page's `formula_page`, the page's `revenue_place`, the page's body key, the page's table prop, and the table's active-filter list — and Task 8's source-read test pins the call counts, so "the one gate" is a checked claim rather than a hopeful one. |
| i18n budget | **6 new keys per locale** (1794 → 1800) plus one **edited** existing value (`labs_analyzer_recipe_desc` gains a sell-scope sentence). Four for the column and the selector, plus `recipe_analyzer_sell_scope_unavailable` (the failed-body banner) and `recipe_analyzer_calc_formula_live_scoped` (the live sentence's `on {{sell}}` reads as a world and would assert the thing retainers cannot do). The sell-scope select's `datacenter` / `region` option labels reuse the existing `datacenter` and `region` keys, exactly as `buy_scope_options` does. **The spec's §9 estimate of "F 6" is therefore right as written and needs no edit.** |
| Does `?sell-scope` join `ADDABLE_FILTERS`? | **No.** It is a Market control, not a row filter — the same call `cost-basis`, `revenue` and `buy-scope` already make. It is counted in `active_filters` (spec L436: "counted in active filters") and cleared by Clear all, but it never renders a chip and never appears in the `+ Filter` menu, so `ADDABLE_FILTERS` stays at nine. |
| What closes when this merges | Spec §10 decision 12: **#1233 closes after F**, with the remaining ports (G–L) tracked on a new issue. Task 9's PR body says so; it does not close anything by itself. |

## File map

| File | Responsibility in this phase |
|---|---|
| `ultros-frontend/ultros-app/src/analyzer_kit/formula.rs` | `Scope` alias, `SellScope` newtype, `ProfitFormula::{with_sell_scope, sell_scope}` (Task 1); one doc line on `FormulaMarks.sell_place` (Task 5). |
| `ultros-frontend/ultros-app/src/analyzer_kit/needed.rs` | `BodyRole::{CheapestSellScope, SellScopeStats}`, `RecipeNeeds::{sell_scope_is_buy_scope, rev_signals}`, `SignalWants::{visible_rev, sort_rev, scope_vs_home}`, `NeededSignals::{rev, scope_vs_home}`, the two new `needed_bodies` rules, and every exhaustive literal they break (Task 2). |
| `ultros-frontend/ultros-app/src/analyzer_kit/cells.rs` | `CellValue::SignedGil` + its render arm + its shape test (Task 4). |
| `ultros-frontend/ultros-app/src/analyzer_kit/columns.rs` | `ColumnKind::ScopeVsHome` (Task 4); the doc line on `PickerContext.sell_place` (Task 5). |
| `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs` | `FILTER_SELL_SCOPE`, `sell_scope_for`, `seat_sell_scope` + their pins (Task 1); `rev_signal_at`, the `PriceInputs` sell-place / sell-world split, `ScopeVsHome`, the suppressed median tell, the discriminating fixture and the revenue oracles (Task 3); `COL_SCOPE_VS_HOME`, `SPEC_SCOPE_VS_HOME`, `label_scope_vs_home`, `cell_scope_vs_home`, `SortMode::ScopeVsHome`, the comparator, the 31st table row, the URL and sort contracts (Task 4); `revenue_place`, the marks / picker / info-sentence split, the `ScopeVsHome` header-extras arm (Task 5); `sell_scope_options`, the strip's `place_select`, `active_filters`, `clear_all` (Task 6); `SellScopeBodies`, `sell_scope_key`, `fetch_sell_scope`, the resource, the Suspense join, the failure banner (Task 7); `RevenueSource`, the table's revenue resolution, the B1 pin (Task 8). |
| `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json` | 6 new keys and 1 edited value, per locale, added in the task that first reads them (Tasks 4, 5, 6, 7 and 9). |
| `ultros-frontend/ultros-app/src/routes/changelog.rs` | The player-facing entry, dated `2026-09-04` (Task 9). |
| `integration/runner.cjs` | The `analyzer-recipe` route gains `&sell-scope=datacenter` and `scope-vs-home` — **in both places the route string appears** (`:94` route-map key, `:144` sweep list) — and the adjacent comment's column counts move (Task 9). |

## The URL contract, before and after

Both halves are pinned by tests that assert exact lists, so a drifted token cannot ship quietly.

| Contract | Before (`8395bc02`) | After Phase F | Pinned by |
|---|---|---|---|
| `OPTIONAL_COLUMN_ORDER` | 22 tokens | **23** (`scope-vs-home` appended) | `recipe_optional_column_order_is_a_stable_url_contract` |
| `BASE_COLUMN_ORDER` (lab off) | 7 tokens | **7** (unchanged) | same test |
| `DEFAULT_COLS` | `["confidence"]` | **`["confidence"]`** (unchanged) | same test |
| `RECIPE_COLUMNS` | `[_; 30]` | **`[_; 31]`** | the array type itself |
| `ALL_SORT_MODES` | 24 | **25** (`ScopeVsHome`) | `sort_mode_round_trips_through_the_url`, `every_recipe_sort_mode_is_catalogued_exactly_once` |
| `SortMode::lab_only` count | 13 | **14** | `lab_only_sort_modes_are_exactly_the_fourteen` (renamed) |
| `grouped_picker_options(...).len()` | 22 | **23** | `picker_columns_are_a_subset_of_optional_column_order` |
| Travel picker group | `hop-gain, hop-worlds` | **`hop-gain, hop-worlds, scope-vs-home`** | `the_grouped_picker_lists_market_and_location` |
| `ADDABLE_FILTERS` | 9 | **9** (unchanged) | `filter_registry_keys_are_a_stable_url_contract` |
| Non-addable filter keys | `cost-basis, revenue, buy-scope, listing-world, listing-dc` | **+ `sell-scope`** | same test |
| `migrate_legacy_params` | 2 rules | **2 rules** (untouched) | `legacy_scope_param_becomes_buy_scope`, `modern_urls_are_left_alone` |

## Test counts

Re-count with `grep -c '#\[test\]'` before trusting any of them if the base has moved. **Verified on `8395bc02`:**

| Module | Base | After Phase F |
|---|---|---|
| `routes::recipe_analyzer` | **65** | **87** |
| `analyzer_kit::cells` | **7** | **8** |
| `analyzer_kit::columns` | **6** | 6 |
| `analyzer_kit::enrichment` | **15** | 15 |
| `analyzer_kit::formula` | **10** | **13** |
| `analyzer_kit::grid` | **10** | 10 |
| `analyzer_kit::hop` | **4** | 4 |
| `analyzer_kit::needed` | **10** | **15** |
| `analyzer_kit::signals` | **6** | 6 |
| `analyzer_kit::strip` | **1** | 1 |

Per-task `recipe_analyzer::test` running totals: T1 **66**, T2 66, T3 **71**, T4 **73**, T5 **78**, T6 **81**, T7 **84**, T8 **86**, T9 **87**. Per-task deltas, so a task that lands the wrong number is caught where it happens rather than at the end: **+1 / +0 / +5 / +2 / +5 / +3 / +3 / +2 / +1**. (Task 5 adds **five** tests, not four — `market_extras_put_the_place_they_are_given_on_the_second_line`, `the_two_places_reach_the_labels_they_belong_to`, `the_two_places_agree_until_the_scope_moves`, `the_scope_vs_home_header_has_its_own_extras_arm` and `the_live_formula_sentence_scopes_the_sell_slot`; the first passes on arrival and is kept as the regression net for the two steps after it, but it is still a `#[test]` and still counts.)

Per-locale i18n key totals: base **1794**; after T4 **1796**, T5 **1797**, T6 **1799**, T7 **1800**, T9 **1800** (T9 edits a value and adds none).

---

### Task 1: The sell-scope term, and the two gates every later task goes through

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/formula.rs:88-120` (add the `Scope` alias beside `BuyScope`), `:186-200` (`ProfitFormula.sell_scope`'s doc comment), `:204-235` (add the two methods), and its `mod tests`
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:519-537` (the filter-key block), the two helpers beside it, and `:4488-4496` (the contract test)

**Interfaces:**
- Consumes: `BuyScope` (`formula.rs:94`), `Term<T>` (`formula.rs:123`), `ProfitFormula` (`formula.rs:188`) — all unchanged in shape.
- Produces, for every later task:
  - `pub type Scope = BuyScope;` — the spec's name for the enum when it is not the buy side's.
  - `pub struct SellScope(pub Scope);` with `Default = SellScope(Scope::World)`, `FromStr` / `Display` delegating to `Scope` (tokens `world` / `datacenter` / `region`), `Copy + Clone + Debug + PartialEq + Eq + Hash`, and `pub fn scope(self) -> Scope`.
  - `pub fn ProfitFormula::with_sell_scope(self, sell: SellScope) -> Self` — sets `sell_scope: Term::Select(sell.scope())`, returns `self`.
  - `pub fn ProfitFormula::sell_scope(&self) -> Scope` — `self.sell_scope.value()`.
  - `const FILTER_SELL_SCOPE: &str = "sell-scope";` in `recipe_analyzer.rs`.
  - `fn sell_scope_for(preview: bool, param: Option<SellScope>) -> Option<SellScope>` — **the lab gate**, read by Tasks 5, 6, 7 and 8.
  - `fn seat_sell_scope(f: ProfitFormula, preview: bool, param: Option<SellScope>) -> ProfitFormula` — **the only caller of `with_sell_scope` in the crate**, read by Task 3's harness, Task 7's page memo and Task 8's table memo. Both helpers land here, three tasks before their first production reader, so the pricing harness in Task 3 can seat the scope the same way production does instead of inventing its own. Between now and Task 3 they are dead code, which `cargo test` tolerates (Global Constraint 4) and `check_ci.sh` never sees until Task 9.
- **Flag-off:** nothing rendered changes. `formula.rs` gains a type and two methods no caller reaches yet; `recipe_analyzer.rs` gains a `const` and two pure `fn`s. `seat_sell_scope(f, false, _)` returns `f` by value and `f.sell_scope` stays `Term::Fixed(Scope::World)` — asserted directly in Step 5's test, which is the byte-identity proof at this layer: `ProfitFormula` is `PartialEq`, so an identical value cannot make a `Memo<ProfitFormula>` fire.

- [ ] **Step 1: Write the failing formula tests**

Append to `formula.rs`'s `mod tests`:

```rust
    /// The sell side's default is the sell WORLD. `Scope::default()` is
    /// `Datacenter` — the buy side's default — so a bare
    /// `unwrap_or_default()` here, or a `filter(|s| *s != Scope::default())`
    /// default-stripping setter, would move every existing URL's revenue to
    /// the datacenter. The newtype is what makes both idioms correct.
    #[test]
    fn sell_scope_defaults_to_the_world_not_the_buy_sides_datacenter() {
        assert_eq!(SellScope::default().scope(), Scope::World);
        assert_ne!(SellScope::default().scope(), Scope::default());
        assert_eq!(Scope::default(), Scope::Datacenter);
    }

    #[test]
    fn sell_scope_tokens_are_the_buy_scope_tokens() {
        for s in ["world", "datacenter", "region"] {
            assert_eq!(s.parse::<SellScope>().unwrap().to_string(), s);
        }
        assert_eq!(SellScope::default().to_string(), "world");
        assert!("home".parse::<SellScope>().is_err());
    }

    /// A formula that never seats the sell scope is byte-identical to
    /// today's: `Fixed(World)`, not `Select(World)`. That is what keeps the
    /// flag-off `Memo<ProfitFormula>` from firing on a value nothing reads.
    #[test]
    fn with_sell_scope_is_the_only_way_to_move_the_sell_side() {
        let untouched = ProfitFormula::recipe_from_query(None, None, None);
        assert_eq!(untouched.sell_scope, Term::Fixed(Scope::World));
        assert_eq!(untouched.sell_scope(), Scope::World);

        let seated = untouched.with_sell_scope(SellScope::default());
        assert_eq!(seated.sell_scope, Term::Select(Scope::World));
        assert_eq!(seated.sell_scope(), Scope::World);

        let region = untouched.with_sell_scope(SellScope(Scope::Region));
        assert_eq!(region.sell_scope(), Scope::Region);
        // Nothing else in the ledger moved.
        assert_eq!(region.cost_signal(), untouched.cost_signal());
        assert_eq!(region.revenue_signal(), untouched.revenue_signal());
        assert_eq!(region.buy_scope(), untouched.buy_scope());
        assert_eq!(region.tax, untouched.tax);
        assert_eq!(region.roi, untouched.roi);
        assert_eq!(region.drop, untouched.drop);
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ultros-app --lib -- analyzer_kit::formula`
Expected: FAIL — `cannot find type SellScope in this scope`.

- [ ] **Step 3: Add the alias, the newtype and the two methods**

In `formula.rs`, immediately after the `BuyScope` `Display` impl (`:110-120`):

```rust
/// The same three places, named for what they are when they are not the
/// buy side's: the sell world, its datacenter, or the whole region. The
/// spec calls the shared enum `Scope`; `BuyScope` keeps its name at the
/// hundreds of sites that already spell it.
pub type Scope = BuyScope;

/// Where the *product's price is read* — [`ProfitFormula::sell_scope`]'s
/// URL value under `?sell-scope=`.
///
/// Named for the sale, not for a destination: FFXIV retainers list only on
/// their own world, so a wider sell scope is a reference read ("what does
/// this go for across my datacenter"), never somewhere to go and sell.
///
/// A newtype over [`Scope`] rather than a bare `Scope`, because
/// `Scope::default()` is `Datacenter`: that is the **buy** side's default,
/// and the sell side's is the world. A bare `param.unwrap_or_default()`, or
/// the default-stripping setter idiom this repo writes everywhere
/// (`parsed.filter(|s| *s != Scope::default())`), would silently re-price
/// every existing recipe-analyzer URL across the datacenter and strip the
/// wrong token out of the URL. Both idioms are correct on this type.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct SellScope(pub Scope);

impl Default for SellScope {
    fn default() -> Self {
        SellScope(Scope::World)
    }
}

impl SellScope {
    pub fn scope(self) -> Scope {
        self.0
    }
}

impl FromStr for SellScope {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<Scope>().map(SellScope)
    }
}

impl Display for SellScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}
```

Replace the `sell_scope` field's doc comment at `:190-196` with:

```rust
    /// Where the product's price is read. `Fixed(Scope::World)` — today's
    /// and every pre-Phase-F URL's value — until
    /// [`ProfitFormula::with_sell_scope`] seats it, which only the recipe
    /// analyzer does and only under the `analyzer-recipe` lab.
    pub sell_scope: Term<Scope>,
```

Add to the `impl ProfitFormula` block, after `buy_scope` (`:235-237`):

```rust
    /// Seat the sell side's scope. Phase F's one entry point: a caller that
    /// never calls this keeps `Term::Fixed(Scope::World)`, which is
    /// `PartialEq`-identical to what `recipe_from_query` has always
    /// produced, so the flag-off page's `Memo<ProfitFormula>` cannot fire
    /// on it. Takes a [`SellScope`], never an `Option<Scope>`: see the
    /// newtype's doc for why the default matters.
    ///
    /// Exactly one caller in the crate — `recipe_analyzer::seat_sell_scope`.
    /// The page and the table build their formulas in two different places
    /// and only the table's prices rows, so the seating goes through one
    /// function that both of them (and the pricing test harness) call.
    pub fn with_sell_scope(mut self, sell: SellScope) -> Self {
        self.sell_scope = Term::Select(sell.scope());
        self
    }

    /// Where revenue is priced: the sell world (the default), its
    /// datacenter, or the region.
    pub fn sell_scope(&self) -> Scope {
        self.sell_scope.value()
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p ultros-app --lib -- analyzer_kit::formula`
Expected: PASS, **13 passed** (10 at the base + 3).

- [ ] **Step 5: Write the failing URL-key and gate tests**

In `recipe_analyzer.rs`'s `filter_registry_keys_are_a_stable_url_contract`, after the three existing pricing-key assertions (`:4491-4495`):

```rust
        // Phase F. Not addable from `+ Filter` (it is a Market control, like
        // the three above), but it IS a bookmark contract and IS counted in
        // the active-filter list, so its key is pinned here with them.
        assert_eq!(FILTER_SELL_SCOPE, "sell-scope");
        assert!(
            !ADDABLE_FILTERS.contains(&FILTER_SELL_SCOPE),
            "sell-scope is a Market control, not a row filter"
        );
```

and add, in `mod test`:

```rust
    /// Both Phase F gates, together, because they are two halves of one
    /// rule: with the lab off the param is dropped, and a formula that
    /// never reaches `with_sell_scope` is `Term::Fixed(World)` — the exact
    /// value `recipe_from_query` has produced since Phase A, so the
    /// flag-off ledger is `PartialEq`-identical to today's.
    #[test]
    fn the_sell_scope_gate_and_its_seating_are_inert_with_the_toggle_off() {
        let base = ProfitFormula::recipe_from_query(None, None, None);
        for param in [
            None,
            Some(SellScope(Scope::Region)),
            Some(SellScope(Scope::Datacenter)),
            Some(SellScope::default()),
        ] {
            assert_eq!(sell_scope_for(false, param), None, "{param:?}");
            let off = seat_sell_scope(base.clone(), false, param);
            assert_eq!(off.sell_scope, Term::Fixed(Scope::World), "{param:?}");
            assert_eq!(off, base, "the flag-off ledger must be the same value");
        }
        // Lab on: the param passes through, and `None` still seats nothing.
        assert_eq!(sell_scope_for(true, None), None);
        assert_eq!(seat_sell_scope(base.clone(), true, None), base);
        assert_eq!(
            sell_scope_for(true, Some(SellScope(Scope::Datacenter))),
            Some(SellScope(Scope::Datacenter))
        );
        assert_eq!(
            seat_sell_scope(base, true, Some(SellScope(Scope::Region))).sell_scope(),
            Scope::Region
        );
    }
```

- [ ] **Step 6: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test::filter_registry recipe_analyzer::test::the_sell_scope_gate`
Expected: FAIL — `cannot find value FILTER_SELL_SCOPE in this scope`, `cannot find function sell_scope_for`.

- [ ] **Step 7: Add the constant and the two gates**

In `recipe_analyzer.rs`, after `const FILTER_BUY_SCOPE: &str = "buy-scope";` (`:525`):

```rust
/// Phase F: which market the sale price is read from. Default `world`,
/// stripped from the URL at the default, read only under the
/// `analyzer-recipe` lab.
const FILTER_SELL_SCOPE: &str = "sell-scope";
```

and, beside it:

```rust
/// The sell scope the page acts on: `None` — i.e. `Term::Fixed(World)`,
/// today's ledger exactly — whenever the `analyzer-recipe` lab is off, so a
/// bookmarked `?sell-scope=region` is inert on the flag-off page down to
/// the "no active filters" hint.
fn sell_scope_for(preview: bool, param: Option<SellScope>) -> Option<SellScope> {
    preview.then_some(param).flatten()
}

/// Seat the sell scope on a formula, through the lab gate.
///
/// **The only caller of [`ProfitFormula::with_sell_scope`] in the crate**,
/// and deliberately so. The page builds a `formula_page` for its fetch
/// keys and the table builds its own `formula` for the pricing pass; only
/// the second one reaches `price_rows`, so a scope seated on the first
/// alone yields a column of dashes that every unit test passes — which is
/// how Phase E2's median tell shipped broken. One function, three callers
/// (the page memo, the table memo, the pricing harness), and a source-read
/// test in Task 8 that counts them.
fn seat_sell_scope(
    f: ProfitFormula,
    preview: bool,
    param: Option<SellScope>,
) -> ProfitFormula {
    match sell_scope_for(preview, param) {
        Some(s) => f.with_sell_scope(s),
        None => f,
    }
}
```

Add `Scope, SellScope` to the `analyzer_kit::formula` import at the top of `recipe_analyzer.rs`, and `Term` to `mod test`'s imports.

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test`
Expected: PASS, **66 passed** (65 at the base + 1).

- [ ] **Step 9: Commit**

```bash
git add ultros-frontend/ultros-app/src/analyzer_kit/formula.rs ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "feat(analyzer-kit): the sell-side scope term, defaulting to the world, behind one gate"
```

---

### Task 2: The two sell-scope bodies, the dedupe against the buy side, and the six literals they break

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/needed.rs:60-69` (`BodyRole`), `:70-88` (`RecipeNeeds`), `:89-113` (`needed_bodies`), `:105-125` (`SignalWants`, `NeededSignals`), `:136-186` (`needed_signals`), and its `mod tests`
- Modify (compile-fix only, same task): the **six** exhaustive literals the two structs have. Adding a field to a struct whose literal has no `..Default::default()` is a *compile error*, and Global Constraint 4 only tolerates warnings between tasks:
  - `needed.rs:189` — the `needs(outliers, same)` test helper (`RecipeNeeds`)
  - `needed.rs:263` — `SignalWants` in the `needed_signals` collection test
  - `needed.rs:339` — `SignalWants` in the sub-craft-cap test
  - `recipe_analyzer.rs:3962` — the production `buy_sale_stats_scope` memo's `RecipeNeeds`, the **only** production `RecipeNeeds` literal without `..RecipeNeeds::default()`
  - `recipe_analyzer.rs:1636` — the production `signal_wants` `SignalWants` (Task 4 rewrites it properly; here it just gets the three new fields at their defaults so the tree compiles)
  - `recipe_analyzer.rs:5142` — `everything_wanted`'s `SignalWants` in `mod test`
  Every other `RecipeNeeds` / `SignalWants` literal on the branch (`recipe_analyzer.rs:4072`, `:6476`, `:6713`, `:6758`, `:6855`; `needed.rs:249`, `:290`, `:295`, `:319`, `:362`, `:381`, `:387`) already ends in `..Default::default()` and needs no edit. Fills: `sell_scope_is_buy_scope: false`, `rev_signals: BTreeSet::new()`, `visible_rev: Vec::new()`, `sort_rev: None`, `scope_vs_home: false`.

**Interfaces:**
- Consumes: `ProfitFormula::{sell_scope, revenue_signal, cost_signal, buy_scope}`, `Scope`, `SellScope` (Task 1); `SALE_STATS_WINDOW_DAYS` (`needed.rs:12`).
- Produces:
  - `BodyRole::CheapestSellScope` and `BodyRole::SellScopeStats(u16)` — declared **after** `CheapestSellWorld` and after `SellWorldStats` respectively in the enum, because `BodyRole` derives `Ord` and `needed_bodies` returns a `BTreeSet` whose iteration order the existing "today's three bodies" test asserts as a `Vec`.
  - `RecipeNeeds.sell_scope_is_buy_scope: bool` and `RecipeNeeds.rev_signals: BTreeSet<PriceSignal>`.
  - `SignalWants.{visible_rev: Vec<PriceSignal>, sort_rev: Option<PriceSignal>, scope_vs_home: bool}`.
  - `NeededSignals.{rev: BTreeSet<PriceSignal>, scope_vs_home: bool}` — `rev` is `{selected revenue} ∪ visible_rev ∪ sort_rev`, uncapped (revenue alternatives are array reads, not `compute_cost` runs).
  - Read by Task 3 (`scope_vs_home`), Task 4 (`signal_wants`) and Task 7 (the resource key).
- **Flag-off:** no markup and no rendered value; `needed.rs` is a pure module. The rendered-behaviour proof is `the_world_sell_scope_adds_no_body`, which asserts the whole `BTreeSet` is `==` the base set (not merely a superset) at `Scope::World` — i.e. every flag-off page and every pre-Phase-F URL issues exactly the requests it does today — plus the existing `needed_bodies_default_is_todays_three_bodies`, which must pass **unchanged**.

- [ ] **Step 1: Write the failing tests**

Append to `needed.rs`'s `mod tests`:

```rust
    fn sell(scope: Scope) -> ProfitFormula {
        ProfitFormula::recipe_from_query(None, None, None).with_sell_scope(SellScope(scope))
    }

    /// The default sell scope adds nothing at all — the flag-off page and
    /// every pre-Phase-F URL fetch exactly the three bodies they always did.
    #[test]
    fn the_world_sell_scope_adds_no_body() {
        let base = needed_bodies(
            &ProfitFormula::recipe_from_query(None, None, None),
            &needs(false, false),
        );
        assert_eq!(needed_bodies(&sell(Scope::World), &needs(false, false)), base);
        // Even with a sale revenue signal: that reads the sell-WORLD body,
        // which is already in the set.
        let f = ProfitFormula::recipe_from_query(None, Some(PriceSignal::SaleMedian), None)
            .with_sell_scope(SellScope(Scope::World));
        assert_eq!(needed_bodies(&f, &needs(false, false)), base);
    }

    /// A wider sell scope needs its own cheapest map, and — only under a
    /// sale revenue signal — its own statistics body.
    #[test]
    fn a_wider_sell_scope_adds_its_cheapest_map_and_only_then_its_stats() {
        let got = needed_bodies(&sell(Scope::Datacenter), &needs(false, false));
        assert!(got.contains(&BodyRole::CheapestSellScope));
        assert!(
            !got.contains(&BodyRole::SellScopeStats(SALE_STATS_WINDOW_DAYS)),
            "listing-min revenue reads no statistics: {got:?}"
        );

        let f = ProfitFormula::recipe_from_query(None, Some(PriceSignal::SaleAvg), None)
            .with_sell_scope(SellScope(Scope::Region));
        let got = needed_bodies(&f, &needs(false, false));
        assert!(got.contains(&BodyRole::CheapestSellScope));
        assert!(got.contains(&BodyRole::SellScopeStats(SALE_STATS_WINDOW_DAYS)));
        // The sell WORLD's 7-day body is still needed: velocity, avg price,
        // confidence, last sold, volume and VWAP all read it.
        assert!(got.contains(&BodyRole::SellWorldStats(SALE_STATS_WINDOW_DAYS)));
    }

    /// A visible or sorted `rev-sale-*` column needs the scope's statistics
    /// even when the *selected* revenue signal is the listing — the same
    /// rule `cost_signals` already gives the buy side.
    #[test]
    fn a_visible_sale_revenue_column_needs_the_sell_scope_stats() {
        let mut n = needs(false, false);
        n.rev_signals = set(&[PriceSignal::ListingMin, PriceSignal::SaleMedian]);
        assert!(
            needed_bodies(&sell(Scope::Region), &n)
                .contains(&BodyRole::SellScopeStats(SALE_STATS_WINDOW_DAYS))
        );
        n.rev_signals = set(&[PriceSignal::ListingMin]);
        assert!(
            !needed_bodies(&sell(Scope::Region), &n)
                .contains(&BodyRole::SellScopeStats(SALE_STATS_WINDOW_DAYS))
        );
        // And never under the default sell scope, whatever the columns say.
        n.rev_signals = set(&[PriceSignal::SaleMedian]);
        assert!(
            !needed_bodies(&sell(Scope::World), &n)
                .contains(&BodyRole::SellScopeStats(SALE_STATS_WINDOW_DAYS))
        );
    }

    /// Deduping: the cheapest map always, the statistics body only when the
    /// buy side actually fetched one. Deduping against a body that was never
    /// requested is how a cell ends up permanently "—".
    #[test]
    fn the_sell_scope_dedupes_against_the_buy_scope_it_matches() {
        // Buy = region (its cheapest body is unconditional), sell = region,
        // revenue = a sale statistic, cost = the listing (so no buy stats).
        let f = ProfitFormula::recipe_from_query(
            None,
            Some(PriceSignal::SaleMedian),
            Some(Scope::Region),
        )
        .with_sell_scope(SellScope(Scope::Region));
        let mut n = needs(false, false);
        n.sell_scope_is_buy_scope = true;
        let got = needed_bodies(&f, &n);
        assert!(
            !got.contains(&BodyRole::CheapestSellScope),
            "the buy scope's cheapest map holds these rows: {got:?}"
        );
        assert!(
            got.contains(&BodyRole::SellScopeStats(SALE_STATS_WINDOW_DAYS)),
            "the buy side fetched no statistics, so there is nothing to reuse: {got:?}"
        );

        // Now give the buy side a sale cost signal, so its statistics body
        // IS in the set: the sell side reuses it.
        let f = ProfitFormula::recipe_from_query(
            Some(PriceSignal::SaleMin),
            Some(PriceSignal::SaleMedian),
            Some(Scope::Region),
        )
        .with_sell_scope(SellScope(Scope::Region));
        let got = needed_bodies(&f, &n);
        assert!(got.contains(&BodyRole::BuyScopeStats(SALE_STATS_WINDOW_DAYS)));
        assert!(!got.contains(&BodyRole::SellScopeStats(SALE_STATS_WINDOW_DAYS)));
        assert!(!got.contains(&BodyRole::CheapestSellScope));

        // The buy side's OWN alias rule still applies underneath: with
        // `buy_scope_is_sell_world`, `BuyScopeStats` is never in the set, so
        // there is nothing to dedupe against and the sell scope must fetch.
        // The page passes its real gate into `sell_scope_key` for exactly
        // this reason; a defaulted `false` here would answer differently.
        let f = ProfitFormula::recipe_from_query(
            Some(PriceSignal::SaleMin),
            Some(PriceSignal::SaleMedian),
            None, // buy scope = World
        )
        .with_sell_scope(SellScope(Scope::Region));
        let mut aliased = needs(false, true);
        aliased.sell_scope_is_buy_scope = true;
        let got = needed_bodies(&f, &aliased);
        assert!(!got.contains(&BodyRole::BuyScopeStats(SALE_STATS_WINDOW_DAYS)));
        assert!(got.contains(&BodyRole::SellScopeStats(SALE_STATS_WINDOW_DAYS)));
    }

    #[test]
    fn needed_signals_collects_the_revenue_columns_and_the_scope_column() {
        let f = ProfitFormula::recipe_from_query(None, Some(PriceSignal::SaleMedian), None);
        let wants = SignalWants {
            visible_rev: vec![PriceSignal::ListingMin],
            sort_rev: Some(PriceSignal::SaleAvg),
            scope_vs_home: true,
            ..SignalWants::default()
        };
        let got = needed_signals(&f, &wants, false);
        assert_eq!(
            got.rev,
            set(&[
                PriceSignal::SaleMedian,
                PriceSignal::ListingMin,
                PriceSignal::SaleAvg
            ])
        );
        assert!(got.scope_vs_home);
        // The default: exactly the selected revenue signal, nothing else,
        // and no sub-craft cap applies to the revenue side (these are array
        // reads, not `compute_cost` runs).
        let plain = needed_signals(&f, &SignalWants::default(), true);
        assert_eq!(plain.rev, set(&[PriceSignal::SaleMedian]));
        assert!(!plain.scope_vs_home);
    }
```

Add the imports the tests need to the `mod tests` `use` line: `use crate::analyzer_kit::formula::{BuyScope, PriceSignal, ProfitFormula, Scope, SellScope};`

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- analyzer_kit::needed`
Expected: FAIL — `no variant named CheapestSellScope`, `no field sell_scope_is_buy_scope`.

- [ ] **Step 3: Grow the roles and the inputs**

In `needed.rs`, replace the `BodyRole` enum (`:60-69`) with:

```rust
/// A whole-scope body the page fetches. Symbolic: the page resolves each
/// role to a world / datacenter / region name.
///
/// The derived `Ord` is the order `needed_bodies`' `BTreeSet` iterates in,
/// which a test asserts as a `Vec` — so a new variant goes beside the one
/// it is a sibling of, never at the front.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BodyRole {
    CheapestBuyScope,
    CheapestSellWorld,
    /// The cheapest listings across the sell *scope*, when that is wider
    /// than the sell world (Phase F). The `over` layer revenue is read
    /// from; the sell world's own map stays for Hop gain's home side.
    CheapestSellScope,
    SellWorldStats(u16),
    BuyScopeStats(u16),
    /// The sell scope's sale statistics, read only by a sale revenue
    /// signal or a visible `rev-sale-*` column at a wider sell scope.
    SellScopeStats(u16),
    RecentSalesSellWorld,
}
```

Add two fields to `RecipeNeeds` (`:70-88`), after `cost_signals`:

```rust
    /// The sell scope resolved to the same place name as the buy scope, so
    /// the buy side's bodies already hold these rows.
    pub sell_scope_is_buy_scope: bool,
    /// Every revenue signal the view will read ([`NeededSignals::rev`]): the
    /// selected one plus any visible or sorted `rev-*` column. A sale signal
    /// in here is what makes the sell-scope statistics body necessary.
    pub rev_signals: BTreeSet<PriceSignal>,
```

Append to `needed_bodies`, immediately before `set` is returned:

```rust
    // Phase F. The sell scope only ever *adds*: at `Scope::World` — every
    // pre-Phase-F URL and every flag-off page — this block is skipped
    // entirely and the set is byte-identical to what it always was.
    if formula.sell_scope() != Scope::World {
        // `CheapestBuyScope` is unconditional, so a matching buy scope
        // always covers the cheapest half.
        if !needs.sell_scope_is_buy_scope {
            set.insert(BodyRole::CheapestSellScope);
        }
        let wants_sell_stats = formula.revenue_signal().sale_stat().is_some()
            || needs.rev_signals.iter().any(|s| s.sale_stat().is_some());
        // The statistics half dedupes only against a body that is actually
        // in the set: the buy-scope one is itself conditional (and is
        // suppressed outright when it aliases the sell world), and reusing
        // a body nobody fetched leaves the revenue cells permanently "—".
        let buy_covers = needs.sell_scope_is_buy_scope
            && set.contains(&BodyRole::BuyScopeStats(SALE_STATS_WINDOW_DAYS));
        if wants_sell_stats && !buy_covers {
            set.insert(BodyRole::SellScopeStats(SALE_STATS_WINDOW_DAYS));
        }
    }
```

- [ ] **Step 4: Grow the signal wants**

Add to `SignalWants` (`:107-113`):

```rust
    /// Visible `rev-*` columns, in table order.
    pub visible_rev: Vec<PriceSignal>,
    /// The sort target, when it is a `rev-*` column.
    pub sort_rev: Option<PriceSignal>,
    /// Scope vs home is visible or the sort target.
    pub scope_vs_home: bool,
```

Add to `NeededSignals` (`:116-124`):

```rust
    /// Revenue signals the view will read. No cap: an alternative revenue
    /// column is an index into a body that is already loaded, not a
    /// `compute_cost` run.
    pub rev: BTreeSet<PriceSignal>,
    pub scope_vs_home: bool,
```

In `needed_signals`, before the `NeededSignals { .. }` literal:

```rust
    let mut rev = BTreeSet::from([formula.revenue_signal()]);
    rev.extend(wants.sort_rev);
    rev.extend(wants.visible_rev.iter().copied());
```

and extend the literal with `rev, scope_vs_home: wants.scope_vs_home,`.

- [ ] **Step 5: Fix the six exhaustive literals**

`needed.rs:189` (`needs`) gains `sell_scope_is_buy_scope: false, rev_signals: BTreeSet::new(),`.
`needed.rs:263` and `needed.rs:339` gain `visible_rev: Vec::new(), sort_rev: None, scope_vs_home: false,` — or, equivalently and preferably, swap their trailing fields for `..SignalWants::default()` so the next field addition does not break them again.
`recipe_analyzer.rs:3962` (`buy_sale_stats_scope`) gains:

```rust
            sell_scope_is_buy_scope: false,
            rev_signals: BTreeSet::new(),
```

with a comment saying why the constants are honest here — this key answers the **buy-scope** body alone, and the sell scope's key is `sell_scope_key` (Task 7), which builds its own `RecipeNeeds`.

`recipe_analyzer.rs:1636` (`signal_wants`) gains `visible_rev: Vec::new(), sort_rev: None, scope_vs_home: false,` as a placeholder — Task 4 replaces all three with real derivations, and until then `NeededSignals.rev` is just `{selected revenue}`, which is what the page computes today.
`recipe_analyzer.rs:5142` (`everything_wanted`) gains `visible_rev: PriceSignal::ALL.to_vec(), sort_rev: None, scope_vs_home: true,` — this helper's whole job is "ask for everything", so it asks for the new things too.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p ultros-app --lib -- analyzer_kit::needed`
Expected: PASS, **15 passed** (10 at the base + 5). `needed_bodies_default_is_todays_three_bodies` must still pass **unchanged** — if it fails, a new `BodyRole` variant was declared too early in the enum and moved the `BTreeSet` order.

Then: `cargo test -p ultros-app --lib`
Expected: the whole crate still compiles and passes (`recipe_analyzer::test` **66**). This second run is the point of Step 5: a struct-field addition that misses one exhaustive literal is a compile error, not a warning.

- [ ] **Step 7: Commit**

```bash
git add ultros-frontend/ultros-app/src/analyzer_kit/needed.rs ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "feat(analyzer-kit): the sell-scope bodies, deduped against the buy side"
```

---

### Task 3: Revenue prices at the sell place, everything else at the sell world

This is the task that can silently change numbers, so it opens with two characterization recordings and its fixture is built to make a wrong lookup impossible to miss.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:96-165` (`RecipeProfitData` gains one field), `:2061-2104` (`PriceInputs`), `:2105-2360` (`price_rows`), and `mod test`'s fixture harness at `:4920-5152` and its oracle at `:5361-5395`
- Modify (compile-fix, same task): `recipe_analyzer.rs:5402` — the `row()` test helper's `RecipeProfitData` literal is exhaustive, so the new field must be filled there or the module does not compile. Fill: `scope_vs_home: ScopeVsHome::Off,`. It is the **only** `RecipeProfitData` literal besides `price_rows`' own (`:2332`).
- Modify (compile-fix, same task): `recipe_analyzer.rs:2786` — **the table's `priced` memo builds the other exhaustive `PriceInputs` literal.** There are exactly two on the branch: this one and `run_with`'s (`:5098`). Adding `revenue_listings` / `revenue_stats` to the struct is a `missing fields` compile error at both, so Step 5 fills both with today's values (Task 8 is what replaces them with the resolved ones).

**Interfaces:**
- Consumes: `SignalView` (`signals.rs:130`), `stat_only_cheapest` (`signals.rs:92`), `NeededSignals.scope_vs_home` (Task 2), `Scope` / `SellScope` / `seat_sell_scope` (Task 1).
- Produces:
  - `fn rev_signal_at(listings: Option<&CheapestListingsMap>, stats: Option<&StatsIndex>, item: i32, signal: PriceSignal) -> Option<i32>` — the bare number for one revenue signal at one place, no cross-fallback. Both `rev_alt` (the sell place) and Scope vs home's home side (the sell world) read it.
  - `PriceInputs.revenue_listings: Option<&CheapestListingsMap>` and `PriceInputs.revenue_stats: Option<&StatsIndex>` — the **sell place**. Existing `sell_listings` and `sell_stats` keep their names and now mean the **sell world** only. There is **no** `revenue_stats_loaded` field: `ProfitFormula::effective`'s second argument already downgrades a sale revenue signal with no body, and `revenue_stats: Option<&StatsIndex>` already encodes "no body" for `rev_alt`. A third spelling of the same fact would have no reader and fail `-D warnings`.
  - `enum ScopeVsHome { Off, Unavailable, Pair { place: i32, home: i32, two_sided: bool } }` and `RecipeProfitData.scope_vs_home: ScopeVsHome`. Read by Task 4's cell and comparator.
  - `RunOpts.{sell_scope: Option<Scope>, scope_bodies: bool}` in the test harness, and `fn scope_fixture(...)`.
- **Flag-off:** no markup. Two rendered values could move — `market_price` (via the Price cell) and the Price note's median tell — and both are pinned by recordings taken *before* the change: `price_rows_matches_recorded_oracle_on_fixture` (unchanged, must not move) plus the new `revenue_projection_is_unchanged_at_the_default_sell_scope`, whose two recordings cover the sale-stat lookup, `rev_alt[1..=3]`, `revenue_fell_back`, `sell_median` and `stat_hq` in both fixture shapes. With `sell_scope == World` every new branch in `price_rows` takes its `World` arm, `revenue_listings == sell_listings` and `revenue_stats == Some(sell_stats)`, so `SignalView` is constructed from the same three values as today.

- [ ] **Step 1: Record the revenue characterization oracle against the UNCHANGED code**

This step's test is expected to **pass** immediately once its constants are filled: it is a characterization test recorded before the refactor, which is the only way to prove afterwards that the sale-side revenue numbers did not move. `price_rows_matches_recorded_oracle_on_fixture` cannot do that job — it projects `key_id, profit, roi, cost, market_price, tax` from a run whose revenue signal is `ListingMin`, so it never touches `stat_only_cheapest`, `rev_alt[1..=3]`, `revenue_fell_back` or `sell_median`.

The second recording is what actually satisfies the spec's parity clause (L246–247, "a parity test that includes items with no sell-world listing"): `fixture()` gives **every** output a sell-world listing, so the default run cannot exercise the `over`-layer miss at all. `RunOpts { sell_listings: false, .. }` gives **no** output one, and every row then resolves revenue through `SignalView`'s `base` layer. Dropping the listing for every third output instead would change `fixture()` itself and move the existing recorded oracle, which Global Constraint 8 forbids.

Add to `recipe_analyzer.rs`'s `mod test`, beside the existing oracle:

```rust
    /// One row of the revenue projection: everything the sell-stat lookup
    /// produces that `price_rows_matches_recorded_oracle_on_fixture` cannot
    /// see.
    type RevProjection = (i32, i32, [Option<i32>; 4], bool, Option<i32>, bool);

    fn revenue_projection(rows: &[RecipeProfitData]) -> Vec<RevProjection> {
        rows.iter()
            .take(12)
            .map(|r| {
                (
                    r.recipe.key_id.0,
                    r.market_price,
                    r.rev_alt,
                    r.revenue_fell_back,
                    r.sell_median,
                    r.stat_hq,
                )
            })
            .collect()
    }

    /// The revenue-side characterization oracle, in the two fixture shapes
    /// that matter: every output has a sell-world listing (`WITH`), and no
    /// output has one (`WITHOUT`) — the spec's "includes items with no
    /// sell-world listing" parity case, which the default fixture cannot
    /// produce because it lists every output.
    ///
    /// Recorded on `8395bc02` before Phase F split the sell place from the
    /// sell world; regenerate ONLY if a phase moves these numbers on
    /// purpose (run with `--nocapture` and copy the printed tuples).
    #[test]
    fn revenue_projection_is_unchanged_at_the_default_sell_scope() {
        let with = revenue_projection(&run(
            PriceSignal::ListingMin,
            PriceSignal::SaleMedian,
            false,
        ));
        let f = ProfitFormula::recipe_from_query(
            Some(PriceSignal::ListingMin),
            Some(PriceSignal::SaleMedian),
            None,
        );
        let without = revenue_projection(&run_with(
            PriceSignal::ListingMin,
            PriceSignal::SaleMedian,
            &RunOpts {
                needs: needed_signals(&f, &SignalWants::default(), false),
                sell_listings: false,
                ..RunOpts::default()
            },
        ));
        println!("REVENUE_ORACLE_WITH = {with:?}");
        println!("REVENUE_ORACLE_WITHOUT = {without:?}");
        const WITH: &[RevProjection] = &[
            // PASTE the printed `REVENUE_ORACLE_WITH` tuples here, verbatim.
        ];
        const WITHOUT: &[RevProjection] = &[
            // PASTE the printed `REVENUE_ORACLE_WITHOUT` tuples here.
        ];
        assert_eq!(with.as_slice(), WITH);
        assert_eq!(without.as_slice(), WITHOUT, "no sell-world listing");
        assert!(
            without.iter().any(|(_, _, alt, ..)| alt[0].is_none()),
            "the WITHOUT shape must contain rows whose sell-world listing is \
             absent, or it is not the parity case the spec asks for"
        );
    }
```

- [ ] **Step 2: Run it, paste the recordings, run it again**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test::revenue_projection -- --nocapture`
Expected: FAIL with a left/right mismatch against the empty constants, and two `REVENUE_ORACLE_* = [...]` lines above it. Paste those tuples in, re-run, and expect PASS. Commit this on its own so the recording is separable from the change it guards:

```bash
git add ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "test(recipe-analyzer): record what the revenue side computes today"
```

- [ ] **Step 3: Write the failing discriminating-fixture tests**

The fixture must vary the discriminator in **both** directions, or it proves nothing: E2's median-tell defect shipped past a green suite because every fixture gave an item exactly one quality of statistics, so a lookup that read the wrong one returned the right answer.

Here the discriminator is *which map* a lookup reads, and the trap is quality. `fixture()` writes the sell world an **NQ row only** (`nq * 12 / 10`), while the buy scope has both (`nq`, `nq + 50`); `SignalView::quality(item, true)` therefore falls through to the buy scope's HQ listing, and `lowest_gil()` takes `min(lq, hq)`. A scope map written NQ-only at `nq * 2` would leave `market_price` pinned at `nq + 50` for every id with `nq > 250` — about 77% of them — so "dearer" would rest on a narrow slice and those rows would land in the fall-through bucket for the wrong reason. So `scope_fixture` derives **both qualities** from the home view and scales each, which makes the direction hold for every surviving row.

Add to `mod test`:

```rust
    /// The sell-scope fixture: the HOME price view, scaled.
    ///
    /// Derived through a `SignalView` with the same layering the pass uses,
    /// so every quality the home run can resolve is present here too and
    /// scaled the same way. NQ-only would leave HQ falling through to the
    /// buy scope and pin `min(lq, hq)` at the unscaled number for most ids.
    ///   * even output ids  -> HALF the home price (a wider market
    ///     undercuts: the realistic direction),
    ///   * odd output ids   -> DOUBLE it (impossible in production, and
    ///     exactly why it is here: a lookup that read the home map, or took
    ///     `min(scope, home)`, would still pass on the even half alone),
    ///   * every third recipe -> absent from the scope map entirely, so the
    ///     `SignalView` `over` layer falls through to the buy-scope `base`.
    /// Statistics move the same three ways.
    fn scope_fixture(
        recipes: &[&'static Recipe],
        buy: &CheapestListingsMap,
        sell: &CheapestListingsMap,
        sell_stats: &StatsIndex,
    ) -> (CheapestListingsMap, StatsIndex) {
        let home = SignalView {
            over: Some(sell),
            base: buy,
            stats: None,
        };
        let mut listings = Vec::new();
        let mut stats = StatsIndex::new();
        for (i, r) in recipes.iter().enumerate() {
            if i % 3 == 2 {
                continue; // absent from the scope entirely
            }
            let out = r.item_result;
            let scale = |p: i32| if out % 2 == 0 { p / 2 } else { p * 2 };
            let pair = home.find_matching_listings(out);
            for (hq, found) in [(false, pair.lq), (true, pair.hq)] {
                if let Some(l) = found {
                    listings.push(CheapestListingItem {
                        item_id: out,
                        hq,
                        cheapest_price: scale(l.price),
                        world_id: 9,
                    });
                }
            }
            for hq in [false, true] {
                if let Some(row) = sell_stats.get(&(out, hq)) {
                    stats.insert(
                        (out, hq),
                        ItemSaleStats {
                            min_price: scale(row.min_price),
                            median_price: scale(row.median_price),
                            avg_price: scale(row.avg_price),
                            ..*row
                        },
                    );
                }
            }
        }
        (
            CheapestListingsMap::from(CheapestListings {
                cheapest_listings: listings,
            }),
            stats,
        )
    }

    /// Revenue follows the sell scope, and the fixture proves each surviving
    /// row actually discriminates. The classes are read off
    /// `rev_alt[ListingMin]` rather than off `market_price`: that entry is
    /// the bare scope-map lookup with no HQ clamp and no base fallback, so
    /// `None` means "absent from the scope map" and nothing else, while a
    /// price comparison cannot tell a fall-through from an undercut (the
    /// buy-scope NQ price is below the home price too).
    #[test]
    fn revenue_reads_the_sell_scope_and_every_class_of_row_says_so() {
        let li = PriceSignal::ListingMin.index();
        for signal in [PriceSignal::ListingMin, PriceSignal::SaleMedian] {
            let f = ProfitFormula::recipe_from_query(
                Some(PriceSignal::ListingMin),
                Some(signal),
                None,
            );
            let needs = needed_signals(&f, &SignalWants::default(), false);
            let home = run_with(
                PriceSignal::ListingMin,
                signal,
                &RunOpts {
                    needs: needs.clone(),
                    ..RunOpts::default()
                },
            );
            let scoped = run_with(
                PriceSignal::ListingMin,
                signal,
                &RunOpts {
                    needs,
                    sell_scope: Some(Scope::Region),
                    scope_bodies: true,
                    ..RunOpts::default()
                },
            );
            let home_by_key: HashMap<i32, &RecipeProfitData> =
                home.iter().map(|r| (r.recipe.key_id.0, r)).collect();

            let (mut cheaper, mut dearer, mut fell_through) = (0, 0, 0);
            let (mut price_down, mut price_up) = (0, 0);
            for r in &scoped {
                let Some(h) = home_by_key.get(&r.recipe.key_id.0) else {
                    continue;
                };
                match (r.rev_alt[li], h.rev_alt[li]) {
                    (None, Some(_)) => {
                        fell_through += 1;
                        assert!(
                            r.market_price > 0,
                            "the base layer must keep a scope-missing row priceable"
                        );
                    }
                    (Some(s), Some(hh)) if s < hh => cheaper += 1,
                    (Some(s), Some(hh)) if s > hh => dearer += 1,
                    pair => panic!("{signal:?}: undiscriminating row {pair:?}"),
                }
                match r.market_price.cmp(&h.market_price) {
                    Ordering::Less => price_down += 1,
                    Ordering::Greater => price_up += 1,
                    Ordering::Equal => {}
                }
            }
            assert!(
                cheaper > 0 && dearer > 0,
                "{signal:?}: the fixture must move the scope lookup BOTH ways \
                 (cheaper {cheaper}, dearer {dearer}); a one-directional \
                 fixture cannot tell a scope lookup from a clamp"
            );
            assert!(
                fell_through > 0,
                "{signal:?}: no row was absent from the scope map"
            );
            assert!(
                price_down > 0 && price_up > 0,
                "{signal:?}: the headline price must move both ways too \
                 (down {price_down}, up {price_up})"
            );
        }
    }

    /// The sell world's own figures do NOT follow the sell scope: velocity,
    /// avg price, confidence, last sold, volume, VWAP, the statistics
    /// quality (the sparkline and 30-day key) and Hop gain's home run all
    /// stay where the spec puts them.
    #[test]
    fn the_sell_worlds_own_figures_ignore_the_sell_scope() {
        let needs = everything_wanted(PriceSignal::ListingMin);
        let home = run_with(
            PriceSignal::ListingMin,
            PriceSignal::SaleMedian,
            &RunOpts {
                needs: needs.clone(),
                scope: Some(BuyScope::Region),
                ..RunOpts::default()
            },
        );
        let scoped = run_with(
            PriceSignal::ListingMin,
            PriceSignal::SaleMedian,
            &RunOpts {
                needs,
                scope: Some(BuyScope::Region),
                sell_scope: Some(Scope::Region),
                scope_bodies: true,
                ..RunOpts::default()
            },
        );
        let by_key: HashMap<i32, &RecipeProfitData> =
            home.iter().map(|r| (r.recipe.key_id.0, r)).collect();
        let mut compared = 0;
        for r in &scoped {
            let Some(h) = by_key.get(&r.recipe.key_id.0) else {
                continue;
            };
            compared += 1;
            assert_eq!(r.daily_sales, h.daily_sales, "{}", r.recipe.key_id.0);
            assert_eq!(r.avg_price, h.avg_price);
            assert_eq!(r.units_sold, h.units_sold);
            assert_eq!(r.vwap, h.vwap);
            assert_eq!(r.last_sold_unix, h.last_sold_unix);
            assert_eq!(r.confidence, h.confidence);
            assert_eq!(r.stat_hq, h.stat_hq);
            assert_eq!(r.hop, h.hop, "Hop gain is buy-side and prices home at the world");
            assert_eq!(r.worlds, h.worlds);
        }
        assert!(compared > 20, "only {compared} rows compared");
    }

    /// The Price median tell is SUPPRESSED at a wider sell scope, not
    /// re-based. `price_note` compares the row's price against
    /// `sell_median`; move the price to a region and the two operands stop
    /// describing the same market, so the tell would read negative and red
    /// on nearly every row — caused by the user's own setting rather than
    /// by a suspicious listing. #1266 was merged to make that tell
    /// trustworthy; a page-wide false alarm is how a colour stops being
    /// read. The sub-line keeps its shape: `price_note` falls to
    /// `ListingFallback` or `None`.
    #[test]
    fn the_price_median_tell_is_suppressed_at_a_wider_sell_scope() {
        let f = ProfitFormula::recipe_from_query(
            Some(PriceSignal::ListingMin),
            Some(PriceSignal::SaleMedian),
            None,
        );
        let needs = needed_signals(&f, &SignalWants::default(), false);
        let home = run_with(
            PriceSignal::ListingMin,
            PriceSignal::SaleMedian,
            &RunOpts {
                needs: needs.clone(),
                ..RunOpts::default()
            },
        );
        assert!(
            home.iter().any(|r| r.sell_median.is_some()),
            "the fixture must carry medians at the default scope, or this \
             test cannot tell suppression from an empty fixture"
        );
        let scoped = run_with(
            PriceSignal::ListingMin,
            PriceSignal::SaleMedian,
            &RunOpts {
                needs,
                sell_scope: Some(Scope::Region),
                scope_bodies: true,
                ..RunOpts::default()
            },
        );
        assert!(
            scoped.iter().all(|r| r.sell_median.is_none()),
            "a wider sell scope must leave the median tell's operand empty"
        );
        // …and the note therefore never carries a percentage.
        for r in &scoped {
            assert!(
                !matches!(
                    price_note(r.market_price, r.sell_median, r.revenue_fell_back),
                    CellNote::VsMedian { .. } | CellNote::Troll { .. }
                ),
                "row {} still renders a median tell",
                r.recipe.key_id.0
            );
        }
    }

    /// Scope vs home: both places under one signal, both directions of
    /// sign, and every non-`Pair` state the design names.
    #[test]
    fn scope_vs_home_records_both_places_and_only_when_asked() {
        let wanted = NeededSignals {
            scope_vs_home: true,
            ..NeededSignals::default()
        };
        // Not asked for: never computed, whatever the scope.
        let quiet = run_with(
            PriceSignal::ListingMin,
            PriceSignal::ListingMin,
            &RunOpts {
                sell_scope: Some(Scope::Region),
                scope_bodies: true,
                ..RunOpts::default()
            },
        );
        assert!(quiet.iter().all(|r| r.scope_vs_home == ScopeVsHome::Off));

        // Asked for, but the sell scope IS the world: nothing to compare,
        // and the whole column is `Off` (the header tooltip says why).
        let flat = run_with(
            PriceSignal::ListingMin,
            PriceSignal::ListingMin,
            &RunOpts {
                needs: wanted.clone(),
                ..RunOpts::default()
            },
        );
        assert!(flat.iter().all(|r| r.scope_vs_home == ScopeVsHome::Off));

        // Asked for at a wider scope: both directions appear, and a row the
        // scope map does not hold is `Unavailable`, never `Off`.
        let scoped = run_with(
            PriceSignal::ListingMin,
            PriceSignal::ListingMin,
            &RunOpts {
                needs: wanted,
                sell_scope: Some(Scope::Region),
                scope_bodies: true,
                ..RunOpts::default()
            },
        );
        assert!(scoped.iter().all(|r| r.scope_vs_home != ScopeVsHome::Off));
        let deltas: Vec<i32> = scoped
            .iter()
            .filter_map(|r| match r.scope_vs_home {
                ScopeVsHome::Pair { place, home, .. } => Some(place - home),
                _ => None,
            })
            .collect();
        assert!(!deltas.is_empty());
        assert!(deltas.iter().any(|d| *d < 0), "no row where the scope undercuts");
        assert!(deltas.iter().any(|d| *d > 0), "no row where the scope is dearer");
        assert!(
            scoped
                .iter()
                .any(|r| r.scope_vs_home == ScopeVsHome::Unavailable),
            "the fixture's third class must reach the Unavailable state"
        );
        // Every recorded pair has a real value on BOTH sides, and a listing
        // signal is one-sided so the percentage will be dropped in Task 4.
        assert!(scoped.iter().all(|r| match r.scope_vs_home {
            ScopeVsHome::Pair { place, home, two_sided } =>
                place > 0 && home > 0 && !two_sided,
            _ => true,
        }));
    }
```

- [ ] **Step 4: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test::revenue_reads recipe_analyzer::test::the_sell_worlds recipe_analyzer::test::the_price_median recipe_analyzer::test::scope_vs_home`
Expected: FAIL — `no field sell_scope on RunOpts`, `cannot find type ScopeVsHome`.

- [ ] **Step 5: Split the sell place from the sell world in `PriceInputs`**

Replace the sell-side fields of `PriceInputs` (`:2068-2082`) with:

```rust
    /// Sell-**world** listings (absent before a world resolves). Hop gain's
    /// home run and Scope vs home's home side price against these, and only
    /// these.
    sell_listings: Option<&'a CheapestListingsMap>,
    /// Buy-scope sale stats, indexed. `None` when not fetched.
    buy_stats: Option<&'a StatsIndex>,
    /// Sell-**world** sale stats, indexed. Empty when not fetched. Velocity,
    /// avg price, confidence, last sold, volume, VWAP and the statistics
    /// quality every lazy column keys on all read this, at every sell scope
    /// (spec §4).
    sell_stats: &'a StatsIndex,
    /// Sell-**place** listings: the sell world's map under the default sell
    /// scope, the scope's own map otherwise. The `SignalView` `over` layer
    /// revenue is priced from.
    revenue_listings: Option<&'a CheapestListingsMap>,
    /// Sell-**place** sale stats. `Some(sell_stats)` under the default sell
    /// scope; `None` when a wider scope's body was not fetched, which makes
    /// every `rev-sale-*` cell "—" rather than a sell-world number under a
    /// scope heading. This is also what `ProfitFormula::effective`'s second
    /// argument was computed from at the call site, so a sale revenue
    /// signal with no body has already been downgraded before it gets here.
    revenue_stats: Option<&'a StatsIndex>,
```

`sell_stats_loaded` (`:2098-2100`) keeps its name and its meaning — "the sell **world's** body arrived" — because that is what `hop_signal` (`:2141-2150`) reads. Leave that use **exactly as it is**.

Then fill the two new fields at **both** exhaustive literals, or the module does not compile. The table's `priced` memo (`:2786-2800`), immediately after `sell_stats: &sell_stats_index,`:

```rust
                // Today's values, spelled out: at the default sell scope
                // the sell place IS the sell world. Task 8 replaces both
                // with the resolved sources; until then this is
                // byte-identical to the single "sell" input it replaces.
                revenue_listings: sell_world_prices.as_deref(),
                revenue_stats: Some(&sell_stats_index),
```

Both types line up with the fields directly above them: `sell_world_prices` is `Option<Arc<CheapestListingsMap>>`, so `.as_deref()` is already the `Option<&CheapestListingsMap>` `sell_listings` takes, and `sell_stats_index` is an `Arc<StatsIndex>` whose `&` deref-coerces to `&StatsIndex` inside `Some(..)` exactly as it does at `sell_stats: &sell_stats_index` one line up. `run_with`'s literal (`:5098`) is filled in Step 7.

- [ ] **Step 6: Add the row state, the shared lookup, and use them**

Above `price_rows` (`:2104`):

```rust
/// Scope vs home's three states. Not an `Option`, because a bare `None`
/// would make the dash mean four things at once and the header tooltip can
/// only name one of them.
#[derive(Copy, Clone, Debug, PartialEq, Default)]
enum ScopeVsHome {
    /// The column was not asked for, or the sell scope IS the sell world.
    /// The whole column is dashes and the header tooltip's last sentence is
    /// what explains it, so the cell adds no title of its own.
    #[default]
    Off,
    /// Asked for at a wider scope, but one of the two markets has no figure
    /// for the selected revenue signal — the dominant case under a sale
    /// signal, where the 7-day window covers a small minority of items. The
    /// cell titles its dash, the way `CellValue::LazyPct`'s empty state
    /// does.
    Unavailable,
    /// Both markets answered. `two_sided` is "the revenue signal is a sale
    /// statistic", i.e. the delta can genuinely go either way and a
    /// percentage against `home` answers a real question; under a listing
    /// signal a wider market can only undercut, the sign is the whole
    /// message, and Task 4 drops the percentage rather than painting a
    /// permanent red stripe. Page-wide rather than per-row, and carried on
    /// the row anyway because `CellCtx` is shared with the flip finder and
    /// has twenty exhaustive literals.
    Pair {
        place: i32,
        home: i32,
        two_sided: bool,
    },
}

/// The bare number one revenue signal reads at one place: the cheapest
/// listing with **no** statistics overlay and no cross-place fallback, or
/// the statistic with no listing fallback. `None` means "this place has no
/// such number", never 0.
///
/// One function for both places on purpose. `rev_alt` reads it at the sell
/// place; Scope vs home's home side reads it at the sell world with the
/// same signal, and a fixture that swaps the maps under it can therefore
/// tell the two apart.
fn rev_signal_at(
    listings: Option<&CheapestListingsMap>,
    stats: Option<&StatsIndex>,
    item: i32,
    signal: PriceSignal,
) -> Option<i32> {
    match signal.sale_stat() {
        None => listings
            .and_then(|l| l.find_matching_listings(item).lowest_gil())
            .filter(|p| *p > 0),
        Some(stat) => stats.and_then(|s| stat_only_cheapest(s, item, stat)),
    }
}
```

In `price_rows`, change `revenue_view` (`:2127-2136`) to read the sell place:

```rust
    let sell_scope_is_world = inp.formula.sell_scope() == Scope::World;
    let revenue_view = SignalView {
        over: inp.revenue_listings,
        base: inp.buy_listings,
        stats: inp
            .formula
            .revenue_signal()
            .sale_stat()
            .and_then(|stat| inp.revenue_stats.map(|idx| (idx, stat))),
    };
```

Replace the `rev_alt` literal (`:2314-2322`) — reusing the `let item = recipe.item_result;` that is already there (`:2312`) — with:

```rust
        // The bare sell-PLACE number per revenue signal, no fallback.
        let rev_alt = [
            rev_signal_at(inp.revenue_listings, inp.revenue_stats, item, PriceSignal::ListingMin),
            rev_signal_at(inp.revenue_listings, inp.revenue_stats, item, PriceSignal::SaleMin),
            rev_signal_at(inp.revenue_listings, inp.revenue_stats, item, PriceSignal::SaleMedian),
            rev_signal_at(inp.revenue_listings, inp.revenue_stats, item, PriceSignal::SaleAvg),
        ];
        let revenue_fell_back = rev_alt[inp.formula.revenue_signal().index()] != Some(market_price);

        // Scope vs home: the selected revenue signal at the sell place and
        // on the sell world's own map.
        let scope_vs_home = if !inp.needs.scope_vs_home || sell_scope_is_world {
            ScopeVsHome::Off
        } else {
            let signal = inp.formula.revenue_signal();
            let place = rev_alt[signal.index()];
            let home = rev_signal_at(inp.sell_listings, Some(inp.sell_stats), item, signal);
            match (place, home) {
                (Some(place), Some(home)) => ScopeVsHome::Pair {
                    place,
                    home,
                    two_sided: signal.sale_stat().is_some(),
                },
                _ => ScopeVsHome::Unavailable,
            }
        };
```

Change the `sell_median` binding (`:2330`) to:

```rust
        // The Price median tell's operand, and only that. Left empty at a
        // wider sell scope: `market_price` then comes from a whole
        // datacenter or region while this median is one world's, so the
        // tell would compare two different markets and read red on nearly
        // every row — the user's own setting wearing the colour #1266 set
        // aside for a suspicious listing. `price_note` degrades to
        // `ListingFallback` / `None` and the sub-line keeps its shape.
        let sell_median = sell_scope_is_world
            .then(|| sell_stat.map(|s| s.median_price).filter(|p| *p > 0))
            .flatten();
```

Add `scope_vs_home,` to the `RecipeProfitData` literal (`:2332`), and to the struct (`:96-165`), after `worlds`:

```rust
    /// Scope vs home's state for this row: `Off` unless the column was
    /// asked for at a wider sell scope, then the two places' figures under
    /// the selected revenue signal, or `Unavailable` when either place has
    /// none. The column renders `place − home`.
    scope_vs_home: ScopeVsHome,
```

and `scope_vs_home: ScopeVsHome::Off,` to the `row()` helper's literal (`:5402`).

- [ ] **Step 7: Teach the test harness the second pair of maps**

In `RunOpts`, add:

```rust
        /// The sell scope. `None` = `Scope::World`, i.e. today's behaviour
        /// and `Term::Fixed`.
        sell_scope: Option<Scope>,
        /// Hand the pass the scope maps from `scope_fixture`. Off with a
        /// non-`World` scope models "the body was asked for and failed",
        /// where revenue falls through to the buy-scope layer.
        scope_bodies: bool,
```

with `None` / `false` in `Default`, and in `run_with`, replacing the `formula:` line of the `PriceInputs` literal and adding two locals above it:

```rust
        let (scope_listings, scope_stats) = scope_fixture(&recipes, &buy, &sell, &sell_index);
        let wider = o.sell_scope.is_some_and(|s| s != Scope::World);
        let use_scope = wider && o.scope_bodies;
        // Seated through the SAME function production uses. Two
        // constructions of one ledger is exactly how Phase E2's median tell
        // shipped past a green suite; `seat_sell_scope(f, true, None)`
        // returns `f`, so every existing run is byte-identical.
        let formula = seat_sell_scope(
            ProfitFormula::recipe_from_query(Some(cost), Some(revenue), o.scope),
            true,
            o.sell_scope.map(SellScope),
        );
```

then, in the `PriceInputs` literal:

```rust
            revenue_listings: if use_scope {
                Some(&scope_listings)
            } else if wider {
                None
            } else {
                o.sell_listings.then_some(&sell)
            },
            revenue_stats: if use_scope {
                Some(&scope_stats)
            } else if wider {
                None
            } else {
                o.sell_stats.then_some(&sell_index)
            },
            formula,
```

Add `use std::cmp::Ordering;` to `mod test`'s imports if it is not already there, and make sure `SignalView`, `CheapestListingItem`, `CheapestListings` and `ItemSaleStats` are in scope for `scope_fixture`.

The two `if use_scope { … } else if wider { … } else { … }` chains above are **temporary**: Task 8 introduces `revenue_source`, the function the shipped table resolves through, and replaces both chains with a `match` on it so the harness picks its maps by production's rule rather than a parallel one. They are written out here because `revenue_source` does not exist for another five tasks, and the three arms are value-for-value identical, so nothing recorded in this task moves when they are swapped.

- [ ] **Step 8: Run every pricing test**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test`
Expected: PASS, **71 passed** (66 after Task 1 + 5). Specifically, all six of these must be green in the same run:
- `price_rows_matches_recorded_oracle_on_fixture` (unchanged numbers),
- `revenue_projection_is_unchanged_at_the_default_sell_scope` (both recordings from Step 2),
- `revenue_reads_the_sell_scope_and_every_class_of_row_says_so`,
- `the_sell_worlds_own_figures_ignore_the_sell_scope`,
- `the_price_median_tell_is_suppressed_at_a_wider_sell_scope`,
- `scope_vs_home_records_both_places_and_only_when_asked`.

If either oracle moved, **stop**: something that should have stayed on the sell world followed the scope.

- [ ] **Step 9: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "feat(recipe-analyzer): price revenue at the sell place, keep the rest on the sell world"
```

---

### Task 4: The `scope-vs-home` column — kind, cell, sort mode, URL token, and a percentage that cannot repeat E2's green

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/columns.rs:29-66` (`ColumnKind`, after `HopWorlds` at **`:65`**)
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/cells.rs:50-99` (`CellValue`), `:388-423` (the render arms), and its `mod tests`
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:561-590` (`COL_*`), `:787-812` (labels), `:930-965` (specs), `:1099-1113` (cells), `:1240` + the row before Actions (the table), `:1623-1642` (`signal_wants`), `:1891-1932` (`SortMode`, whose `Vwap30` sits at **`:1915`**, and `lab_only` at `:1921-1932`), `:1997-2060` (`compare_recipes`), and the URL / sort / picker tests
- Modify: `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json` (2 keys each)

**Interfaces:**
- Consumes: `RecipeProfitData.scope_vs_home` and `ScopeVsHome` (Task 3), `NeededSignals.scope_vs_home` and `SignalWants::{visible_rev, sort_rev, scope_vs_home}` (Task 2), `delta_pct` (`recipe_analyzer.rs:1051`), `signed_gil` (`cells.rs:136`), `signed_delta_class` / `DELTA_DEAD_BAND_PCT` / `VS_MEDIAN_DISPLAY_CEILING_PCT` / `is_troll_listing` (`analysis.rs:386`, `:380`, `:315`, `:458`), `cmp_none_last` (`sort_header.rs:168`), `LAB_ANALYZER_RECIPE`.
- Produces:
  - `ColumnKind::ScopeVsHome`.
  - `CellValue::SignedGil { delta: Option<i32>, pct: Option<f32>, unavailable: bool }` and its render arm.
  - `const COL_SCOPE_VS_HOME: &str = "scope-vs-home";`, `static SPEC_SCOPE_VS_HOME: ColumnSpec`, `fn label_scope_vs_home`, `fn cell_scope_vs_home`, `fn scope_vs_home_delta(&RecipeProfitData) -> Option<i32>`, `SortMode::ScopeVsHome`, and the 31st entry of `RECIPE_COLUMNS`.
  - i18n keys `analyzer_col_scope_vs_home` and `analyzer_scope_vs_home_help`, read here and (the tooltip) by Task 5's header-extras arm.
- **Flag-off:** the column carries `lab: Some(LAB_ANALYZER_RECIPE)`, so the grid drops it at build time and no `<!>` marker appears (Global Constraint 2); `BASE_COLUMN_ORDER` never learns the token and `SortMode::ScopeVsHome.lab_only()` is true, so neither `?cols=scope-vs-home` nor `?sort=scope-vs-home` survives parsing with the lab off. All three are asserted in `phase_f_adds_exactly_one_key_and_one_column_token` (Task 9). `CellValue` gains a variant no flag-off column constructs, and `cells.rs`'s existing `render_cell_keeps_one_shape_per_variant` must pass unchanged.

- [ ] **Step 1: Write the failing cell-shape test**

Append to `cells.rs`'s `mod tests`:

```rust
    /// A signed delta keeps one shape across "there is a number", "there is
    /// not" and "there could have been": the gil icon hides by class, the
    /// value mutes by class, and the sub-line element is always present. A
    /// negative delta is the COMMON case for Scope vs home under the
    /// cheapest listing, so this asserts the number survives —
    /// `MutedGil`'s `amount > 0` filter would have swallowed it — and that
    /// a `None` percentage renders muted rather than coloured, which is how
    /// the one-sided listing case avoids a permanent red stripe.
    #[test]
    fn signed_gil_cells_keep_one_shape_and_render_negatives() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let down = render(CellValue::SignedGil {
                delta: Some(-1_250),
                pct: Some(-8.0),
                unavailable: false,
            });
            let up = render(CellValue::SignedGil {
                delta: Some(430),
                pct: Some(3.0),
                unavailable: false,
            });
            let one_sided = render(CellValue::SignedGil {
                delta: Some(-1_250),
                pct: None,
                unavailable: false,
            });
            let off = render(CellValue::SignedGil {
                delta: None,
                pct: None,
                unavailable: false,
            });
            let missing = render(CellValue::SignedGil {
                delta: None,
                pct: None,
                unavailable: true,
            });
            assert!(down.contains("-1,250"), "{down}");
            assert!(down.contains("text-red-300"), "{down}");
            assert!(down.contains("-8%"), "{down}");
            assert!(up.contains("+430"), "{up}");
            assert!(up.contains("text-emerald-300"), "{up}");
            assert!(
                one_sided.contains("-1,250")
                    && !one_sided.contains("text-red-300")
                    && !one_sided.contains("text-emerald-300"),
                "a dropped percentage must render the delta with no colour: {one_sided}"
            );
            assert!(off.contains("—"), "{off}");
            assert!(!off.contains("title="), "the Off dash carries no title: {off}");
            assert!(missing.contains("title="), "the Unavailable dash is titled: {missing}");
            for html in [&down, &up, &one_sided, &off, &missing] {
                assert_eq!(html.matches("<div").count(), down.matches("<div").count());
                assert_eq!(html.matches("<span").count(), down.matches("<span").count());
            }
        });
    }
```

(`render` is the existing helper in that module.)

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ultros-app --lib -- analyzer_kit::cells`
Expected: FAIL — `no variant named SignedGil`.

- [ ] **Step 3: Add the variant and its arm**

In `cells.rs`, after `CellValue::Hop`'s declaration (`:96`):

```rust
    /// A signed gil delta against a baseline, with an always-present
    /// percent sub-line: Scope vs home. **Not** `MutedGil` — that one
    /// filters `amount > 0`, and a negative delta is this column's normal
    /// state (a wider market can only undercut under the cheapest
    /// listing). `pct: None` renders the value uncoloured, which is what
    /// the one-sided listing case wants: the sign is the whole message and
    /// a permanent red stripe teaches readers to ignore the colour.
    /// `unavailable` titles the dash with the reason, the way
    /// [`CellValue::LazyPct`]'s empty state does.
    SignedGil {
        delta: Option<i32>,
        pct: Option<f32>,
        unavailable: bool,
    },
```

and, before the `CellValue::Custom` arm (`:424`):

```rust
        CellValue::SignedGil {
            delta,
            pct,
            unavailable,
        } => {
            let has = delta.is_some();
            let text = delta.map(signed_gil).unwrap_or_else(|| "—".to_string());
            let sub = pct.map(|p| format!("{p:+.0}%")).unwrap_or_default();
            let value_class = if has {
                signed_delta_class(pct, DELTA_DEAD_BAND_PCT)
            } else {
                "text-[color:var(--color-text-muted)]"
            };
            // Only the "could have had a figure and did not" dash is
            // titled. The "sell scope is your sell world" dash is the whole
            // column at once and the header tooltip is what explains it; a
            // per-cell "Not enough sales" there would be a second wrong
            // answer.
            let title = unavailable
                .then(|| t_string!(i18n, analyzer_drift_unavailable).to_string());
            // One shape (the `GilOrDash` rule): the icon hides and the value
            // mutes by class; the arms never swap elements.
            view! {
                <div role="cell" class=class title=title>
                    <div class="flex flex-row items-center justify-end">
                        <span class=if has { "inline-flex" } else { "hidden" }><GilIcon /></span>
                        <div class=value_class>{text}</div>
                    </div>
                    <div class=SUB_LINE>{sub}</div>
                </div>
            }
            .into_any()
        }
```

- [ ] **Step 4: Run the cell tests**

Run: `cargo test -p ultros-app --lib -- analyzer_kit::cells`
Expected: PASS, **8 passed** (7 at the base + 1).

- [ ] **Step 5: Write the failing column-contract tests**

In `recipe_analyzer.rs`'s `mod test`:

1. Append `"scope-vs-home"` to the expected `OPTIONAL_COLUMN_ORDER` list in `recipe_optional_column_order_is_a_stable_url_contract`, under a comment:

```rust
                // Phase F, appended for the same reason E2's five were.
                "scope-vs-home",
```

2. Add `SortMode::ScopeVsHome` to the end of `ALL_SORT_MODES` and change its length to `[SortMode; 25]`.

3. In `sort_mode_round_trips_through_the_url`, add:

```rust
        assert_eq!(SortMode::ScopeVsHome.to_string(), "scope-vs-home");
        assert_eq!("scope-vs-home".parse::<SortMode>(), Ok(SortMode::ScopeVsHome));
```

4. Rename `lab_only_sort_modes_are_exactly_the_thirteen` to `..._the_fourteen`, change `13` to `14`, and add `assert!(SortMode::ScopeVsHome.lab_only());`.

5. In `picker_columns_are_a_subset_of_optional_column_order`, change `assert_eq!(ids.len(), 22)` to `23`.

6. In `the_grouped_picker_lists_market_and_location`, add:

```rust
            assert_eq!(
                ids_in("Travel"),
                ["hop-gain", "hop-worlds", "scope-vs-home"],
                "the picker groups by (group, table index), so the appended \
                 column still lists third in Travel"
            );
```

7. Add two new tests, and the row helper they share:

```rust
    /// `scope_row` returns a `RecipeRow`, i.e. `Arc<RecipeProfitData>`, the
    /// way `hop_row` and `price_row` do: every cell fn takes `&RecipeRow`,
    /// and `compare_recipes` takes `&RecipeProfitData`, which `&Arc<T>`
    /// deref-coerces into.
    fn scope_row(key: i32, state: ScopeVsHome) -> RecipeRow {
        let mut r = Arc::try_unwrap(row(key, 0, 0, 1.0, 1)).ok().unwrap();
        r.scope_vs_home = state;
        Arc::new(r)
    }

    fn pair(place: i32, home: i32, two_sided: bool) -> ScopeVsHome {
        ScopeVsHome::Pair {
            place,
            home,
            two_sided,
        }
    }

    /// Scope vs home renders the delta, its percent against the home value,
    /// and nothing at all when there is no pair. The sort key is the same
    /// delta, and it sorts none-last in both directions like every other
    /// optional-value column on this page.
    #[test]
    fn scope_vs_home_cell_and_sort_read_the_same_delta() {
        let ctx = test_ctx();
        let cheaper = scope_row(1, pair(900, 1_000, true));
        let dearer = scope_row(2, pair(1_100, 1_000, true));
        let off = scope_row(3, ScopeVsHome::Off);
        let missing = scope_row(4, ScopeVsHome::Unavailable);
        assert_eq!(
            cell_scope_vs_home(&cheaper, &ctx),
            CellValue::SignedGil {
                delta: Some(-100),
                pct: Some(-10.0),
                unavailable: false,
            }
        );
        assert_eq!(
            cell_scope_vs_home(&dearer, &ctx),
            CellValue::SignedGil {
                delta: Some(100),
                pct: Some(10.0),
                unavailable: false,
            }
        );
        assert_eq!(
            cell_scope_vs_home(&off, &ctx),
            CellValue::SignedGil {
                delta: None,
                pct: None,
                unavailable: false,
            }
        );
        assert_eq!(
            cell_scope_vs_home(&missing, &ctx),
            CellValue::SignedGil {
                delta: None,
                pct: None,
                unavailable: true,
            },
            "a dash that could have been a figure says so"
        );
        assert_eq!(scope_vs_home_delta(&cheaper), Some(-100));
        assert_eq!(scope_vs_home_delta(&off), None);
        assert_eq!(scope_vs_home_delta(&missing), None);

        for dir in [SortDir::Asc, SortDir::Desc] {
            assert_eq!(
                compare_recipes(SortMode::ScopeVsHome, dir, &cheaper, &missing, None),
                Ordering::Less,
                "a row with no pair sorts last whichever way the header points"
            );
            assert_eq!(
                compare_recipes(SortMode::ScopeVsHome, dir, &missing, &dearer, None),
                Ordering::Greater
            );
        }
        assert_eq!(
            compare_recipes(SortMode::ScopeVsHome, SortDir::Desc, &dearer, &cheaper, None),
            Ordering::Less,
            "descending puts the biggest gain first"
        );
        assert_eq!(SortMode::ScopeVsHome.default_dir(), SortDir::Desc);
    }

    /// Phase E2 shipped a coloured percentage whose GREEN arm meant "do not
    /// trust this figure", and #1266 corrected it with a display ceiling
    /// and a troll guard. Scope vs home inherits both rather than
    /// re-earning them:
    ///
    /// * under a listing signal the delta is structurally <= 0, so the
    ///   percentage is dropped and the cell renders uncoloured — a
    ///   permanently red stripe in the codebase's warning colour teaches
    ///   players to ignore the colour;
    /// * a scope figure 50x the home one is not a finding, it is a thin or
    ///   laundered home median, and `is_troll_listing` is the same helper
    ///   `price_note` gates on;
    /// * anything below that is clamped to the same ceiling that exists
    ///   because prod rendered "+399900%".
    #[test]
    fn scope_vs_home_never_paints_a_thin_home_median_green() {
        let ctx = test_ctx();
        let pct_of = |r: &RecipeRow| match cell_scope_vs_home(r, &ctx) {
            CellValue::SignedGil { pct, .. } => pct,
            other => panic!("{other:?}"),
        };
        // One-sided: the listing signal. The gil delta survives, the
        // percentage does not.
        let listing = scope_row(1, pair(900, 1_000, false));
        assert_eq!(scope_vs_home_delta(&listing), Some(-100));
        assert_eq!(pct_of(&listing), None);
        // Troll-shaped: the only way this column renders green.
        let thin = scope_row(2, pair(100_000, 100, true));
        assert!(is_troll_listing(100_000, 100));
        assert_eq!(
            pct_of(&thin),
            None,
            "a home figure the analyzer would not price against must not be \
             the baseline for an emerald percentage"
        );
        assert_eq!(scope_vs_home_delta(&thin), Some(99_900));
        // Below the troll multiple, the ceiling still applies.
        let big = scope_row(3, pair(2_000, 100, true));
        assert!(!is_troll_listing(2_000, 100));
        assert_eq!(pct_of(&big), Some(VS_MEDIAN_DISPLAY_CEILING_PCT));
        // And an ordinary figure is untouched.
        assert_eq!(pct_of(&scope_row(4, pair(1_100, 1_000, true))), Some(10.0));
    }
```

- [ ] **Step 6: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test`
Expected: FAIL — `no variant named ScopeVsHome` on `SortMode`, `cannot find function cell_scope_vs_home`, and the three count assertions.

- [ ] **Step 7: Add the kind, the token, the label, the cell and the sort mode**

`columns.rs`, in `ColumnKind` after `HopWorlds` (`:65`):

```rust
    /// The revenue signal at the sell scope minus the same signal on the
    /// sell world's own map: the sell-side counterpart of Hop gain, and a
    /// reference read rather than a place to go.
    ScopeVsHome,
```

`recipe_analyzer.rs`:

```rust
// after COL_VWAP_30D (:586)
/// Phase F.
const COL_SCOPE_VS_HOME: &str = "scope-vs-home";

// beside the other labels (after label_vwap_30d, :810)
fn label_scope_vs_home(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, analyzer_col_scope_vs_home).to_string()
}

// beside the other specs (after SPEC_VWAP_30D, :964)
static SPEC_SCOPE_VS_HOME: ColumnSpec = ColumnSpec {
    kind: ColumnKind::ScopeVsHome,
    label: label_scope_vs_home,
    // Travel, beside Hop gain: it answers the same question from the other
    // side of the ledger.
    group: PickerGroup::Travel,
};

// beside cell_hop_gain (:1105)
/// The delta the cell renders and the comparator sorts by: one function, so
/// a header click can never order rows by a number the cell does not show.
fn scope_vs_home_delta(r: &RecipeProfitData) -> Option<i32> {
    match r.scope_vs_home {
        ScopeVsHome::Pair { place, home, .. } => Some(place - home),
        _ => None,
    }
}

/// The percentage under the delta, against the HOME value: "the wider
/// market is 10% below your world".
///
/// `None` — which `signed_delta_class` renders as no colour at all — in the
/// two cases where a coloured percentage would say the opposite of what it
/// means. Under a listing signal the delta cannot be positive (a region
/// contains the world), so the figure would be a permanent red stripe and
/// the sign already carries the whole message. And a `place` that clears
/// `is_troll_listing` against `home` is not a wide-market finding: it is a
/// home figure so thin that the analyzer refuses to price against it
/// elsewhere, and painting that emerald is exactly the defect #1266
/// removed from the Price tell. Otherwise the same display ceiling
/// applies, for the same reason ROI is clamped.
fn scope_vs_home_pct(state: ScopeVsHome) -> Option<f32> {
    match state {
        ScopeVsHome::Pair {
            place,
            home,
            two_sided: true,
        } if !is_troll_listing(place, home) => {
            delta_pct(Some(place), home).map(|p| p.min(VS_MEDIAN_DISPLAY_CEILING_PCT))
        }
        _ => None,
    }
}

fn cell_scope_vs_home(r: &RecipeRow, _: &CellCtx) -> CellValue {
    CellValue::SignedGil {
        delta: scope_vs_home_delta(r),
        pct: scope_vs_home_pct(r.scope_vs_home),
        unavailable: r.scope_vs_home == ScopeVsHome::Unavailable,
    }
}
```

`SortMode`, after `Vwap30` (`:1915`):

```rust
    /// The sell-scope revenue signal minus the sell world's own.
    ScopeVsHome,
```

and add `| SortMode::ScopeVsHome` to `lab_only`'s `matches!` list (`:1921-1932`).

`compare_recipes`, after the `Vwap30` arm (`:2050`):

```rust
        SortMode::ScopeVsHome => cmp_none_last(
            scope_vs_home_delta(a),
            scope_vs_home_delta(b),
            dir,
            i32::cmp,
        ),
```

The table: change `static RECIPE_COLUMNS: [ToolColumnMeta<RecipeRow, SortMode>; 30]` (`:1240`) to `; 31`, and insert **immediately before** the `SPEC_ACTIONS` entry:

```rust
    ToolColumnMeta {
        spec: &SPEC_SCOPE_VS_HOME,
        id: COL_SCOPE_VS_HOME,
        sort_id: COL_SCOPE_VS_HOME,
        sort: sortability_for(Layer::Computed, Some(SortMode::ScopeVsHome)),
        header_class: HEAD_28_MD,
        cell_class: CELL_28_MD,
        default_on: false,
        cell: cell_scope_vs_home,
        lab: Some(LAB_ANALYZER_RECIPE),
        ..RECIPE_BASE
    },
```

(`RECIPE_BASE.default_dir` is already `SortDir::Desc`, which is what the test asserts; `SortMode::default_dir` reads it out of this table via `default_dir_for`, so no `SortMode` impl changes.)

`signal_wants` (`:1623`) replaces the three placeholders Task 2 left with real derivations:

```rust
    let visible_rev = RECIPE_COLUMNS
        .iter()
        .filter(|c| !c.id.is_empty() && visible.contains(c.id))
        .filter_map(|c| match c.spec.kind {
            ColumnKind::RevSignal(s) => Some(s),
            _ => None,
        })
        .collect();
    let sort_rev = match sort {
        Some(SortMode::RevSignal(s)) => Some(s),
        _ => None,
    };
```

and, in the `SignalWants { .. }` literal:

```rust
        visible_rev,
        sort_rev,
        scope_vs_home: visible.contains(COL_SCOPE_VS_HOME)
            || sort == Some(SortMode::ScopeVsHome),
```

- [ ] **Step 8: Add the two i18n keys to all seven locales**

`analyzer_col_scope_vs_home` (the `w-28` header, so short):

| locale | value |
|---|---|
| en | `Scope vs home` |
| fr | `Portée vs monde` |
| de | `Bereich vs. Welt` |
| ja | `範囲 vs 自ワールド` |
| cn | `范围对比本服` |
| ko | `범위 vs 홈 월드` |
| tc | `範圍對比本伺服器` |

`analyzer_scope_vs_home_help` (the header tooltip, Task 5 renders it). It has three jobs: say what the number is, say that it is a **reference read** and not a place to sell (retainers are home-world bound), and name **both** reasons the cell is blank — the whole-column one and the per-row one — because "Blank when the sell scope is the sell world" alone reads as "my setting did not take".

- en: `The revenue signal read across the sell scope, minus the same signal on your sell world. A comparison, not a place to sell: retainers only list on your own world. Negative means the wider market prices lower — more sellers undercutting each other — so under the cheapest listing it is never above zero; a sale statistic can go either way. Blank when the sell scope is your sell world, and blank for a row where one of the two markets has no figure for the signal, which is most items on a seven-day window.`
- fr: `Le signal de revenu lu sur la portée de vente, moins le même signal sur votre monde de vente. Une comparaison, pas un endroit où vendre : vos servants ne mettent en vente que sur votre propre monde. Négatif signifie que le marché élargi affiche un prix plus bas — davantage de vendeurs qui se sous-cotent — donc, avec l'annonce la moins chère, jamais au-dessus de zéro ; une statistique de ventes peut aller dans les deux sens. Vide lorsque la portée de vente est votre monde de vente, et vide pour une ligne dont l'un des deux marchés n'a aucun chiffre pour ce signal, ce qui est le cas de la plupart des objets sur sept jours.`
- de: `Das Erlössignal über den Verkaufsbereich, minus dasselbe Signal auf deiner Verkaufswelt. Ein Vergleichswert, kein Verkaufsort: Gehilfen bieten nur auf deiner eigenen Welt an. Negativ heißt, der weitere Markt ist günstiger — mehr Verkäufer, die sich gegenseitig unterbieten — beim günstigsten Angebot also nie über null; eine Verkaufsstatistik kann in beide Richtungen gehen. Leer, wenn der Verkaufsbereich deine Verkaufswelt ist, und leer für eine Zeile, in der einer der beiden Märkte keinen Wert für das Signal hat — das sind in sieben Tagen die meisten Gegenstände.`
- ja: `販売範囲で読んだ収益シグナルから、販売ワールドでの同じシグナルを引いた値です。売りに行く場所ではなく比較のための数値です（リテイナーは自分のワールドにしか出品できません）。マイナスは広い市場のほうが価格が低い（互いに値下げする出品者が多い）ことを意味し、最安出品ではプラスになりません。売上統計ではどちらにも振れます。販売範囲が販売ワールドと同じ場合は空欄で、どちらかの市場にその指標の値がない行も空欄です（7日間では大半の品が該当します）。`
- cn: `在销售范围内读取的收益信号，减去销售服务器上的同一信号。这只是参考对比，而不是可以去卖的地方——雇员只能在你自己的服务器上寄售。负值表示更大的市场价格更低（互相压价的卖家更多），因此按最低寄售价永远不会为正；按成交统计则可高可低。销售范围就是销售服务器时留空；某一行的两个市场中有一方没有该信号的数值时也留空——在七天窗口内多数物品都是如此。`
- ko: `판매 범위에서 읽은 수익 신호에서 판매 서버의 같은 신호를 뺀 값입니다. 팔러 갈 곳이 아니라 참고용 비교 수치입니다 — 리테이너는 자신의 서버에만 등록할 수 있습니다. 음수는 넓은 시장의 가격이 더 낮다는 뜻이며(서로 가격을 낮추는 판매자가 더 많음), 최저 판매 등록가 기준으로는 0을 넘지 않습니다. 판매 통계 기준으로는 양방향 모두 가능합니다. 판매 범위가 판매 서버와 같으면 비어 있고, 두 시장 중 한쪽에 해당 신호의 값이 없는 행도 비어 있습니다 — 7일 기준으로는 대부분의 아이템이 그렇습니다.`
- tc: `在銷售範圍讀取的收益訊號，減去銷售伺服器上的同一訊號。這只是參考比較，而不是可以前往販售的地方——雇員只能在你自己的伺服器上寄售。負值表示更大的市場價格更低（互相壓價的賣家更多），因此以最低寄售價計算永遠不會為正；以成交統計計算則可能為正或負。銷售範圍即銷售伺服器時留空；某一列的兩個市場中有一方沒有該訊號的數值時也留空——在七天區間內多數物品都是如此。`

Verify every file grew by exactly two:

```bash
for l in en fr de ja cn ko tc; do
  printf '%s ' "$l"
  python -c "import json,sys; print(len(json.load(open(sys.argv[1],encoding='utf-8'))))" \
    ultros-frontend/ultros-app/locales/$l.json
done
```
Expected: **1796** for all seven.

- [ ] **Step 9: Run the tests to verify they pass**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test analyzer_kit`
Expected: PASS. `recipe_analyzer::test` **73**, `analyzer_kit::cells` **8**. The three contract counts now read 23 / 25 / 14, and `every_recipe_sort_mode_is_catalogued_exactly_once` covers the new mode without edits (it iterates `ALL_SORT_MODES`).

- [ ] **Step 10: Commit**

```bash
git add ultros-frontend/ultros-app/src/analyzer_kit ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs ultros-frontend/ultros-app/locales
git commit -m "feat(recipe-analyzer): Scope vs home as a sortable column"
```

---

### Task 5: The sell place and the sell world are two different labels

Everything that names "where revenue comes from" must follow the scope; everything that names "where the 7-day figures come from" must not. Both are spelled `sell_place` today, which is exactly why this is its own task with its own tests. It also carries two things the column cannot ship without: the header tooltip's `header_extras` arm (without it the key ships dead), and a live-sentence variant that does not tell the player they are selling on a datacenter.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/columns.rs:223-231` (doc only, `PickerContext.sell_place`)
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/formula.rs:262-278` (doc only, `FormulaMarks.sell_place`)
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:2490-2500` (a new table prop), `:2665-2755` (`marks`, `header_extras`), `:2995-3013` (`column_options`), `:3736-3739` and `:3855-3875` (the page's signals and memos), `:4207-4235` (the live info sentence), `:4355-4375` (the table call)
- Modify: `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json` (1 key each)

**Interfaces:**
- Consumes: `sell_scope_for` (Task 1), `FILTER_SELL_SCOPE` (Task 1), `ColumnKind::ScopeVsHome` and `analyzer_scope_vs_home_help` (Task 4).
- Produces:
  - `let (sell_scope, set_sell_scope) = filter_query_signal::<SellScope>(FILTER_SELL_SCOPE);` on the **page** (the setter is Task 6's; the getter is read here through `sell_scope_for`).
  - `let revenue_place: Memo<String>` on the page — the world / datacenter / region name revenue is priced at.
  - `RecipeAnalyzerTable`'s new required prop `#[prop(into)] revenue_place: Signal<String>`.
  - The `ColumnKind::ScopeVsHome` arm of `header_extras`.
  - i18n key `recipe_analyzer_calc_formula_live_scoped`.
  - The rule, enforced by a test: `sell_place` reaches `market_extra` and nothing else; `revenue_place` reaches `marks`, the `RevSignal` header arm, `PickerContext.sell_place` and the live sentence's `sell` slot.
- **Flag-off:** three rendered values move and all three are already inside `if !preview { return … }` / `preview.then(..)` guards — `marks` (`:2669`), `header_extras` (`:2688-2691`) and `column_options` (`:2996`, whose `else` branch is the flag-off `picker_options`). The fourth, the live sentence, is inside `if preview.get()` (`:4210`). With the lab off `sell_scope_for` returns `None`, so `revenue_place == sell_place` **at every scope**, and the lab-on default (`sell-scope` absent) resolves the same way — pinned by `the_two_places_agree_until_the_scope_moves`. The new `header_extras` arm is reachable only for a column whose `lab` gate already dropped it flag-off.

- [ ] **Step 1: Write the failing tests**

Add to `recipe_analyzer.rs`'s `mod test`:

```rust
    /// This module's production half. `include_str!` pulls in the test
    /// module's own source, so a literal needle would satisfy itself;
    /// splitting on the test attribute keeps every search below to the code
    /// that actually ships. Split on two anchors rather than one needle
    /// holding a real newline: a CRLF checkout would make that needle miss.
    fn production_source() -> &'static str {
        const SRC: &str = include_str!("recipe_analyzer.rs");
        let (production, rest) = SRC
            .split_once(&format!("#[cfg({})]", "test"))
            .expect("the production half ends at the test module attribute");
        assert!(
            rest.trim_start().starts_with(&format!("mod {} {{", "test")),
            "the attribute ending the production half must be the test module's"
        );
        production
    }

    /// `production_source()` with all whitespace removed, so a needle
    /// cannot be broken by rustfmt's line wrapping (or by a CRLF
    /// checkout). Assert against this whenever the thing being pinned is a
    /// multi-argument call: rustfmt breaks any call it cannot fit in 100
    /// columns onto one line per argument, and a needle written as one
    /// line then pins text the formatter will never emit — a test that can
    /// only fail.
    fn production_squeezed() -> String {
        production_source()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect()
    }

    /// `market_extra` puts the place it is GIVEN on line 2 — it has no
    /// other source for one, so this pins the composition (`7d · ‹place›`)
    /// and nothing more. Which place actually reaches the call is a
    /// different question and a different test
    /// (`the_two_places_reach_the_labels_they_belong_to`), because the two
    /// variables are one character apart in `header_extras`.
    #[test]
    fn market_extras_put_the_place_they_are_given_on_the_second_line() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let i18n = use_i18n();
            for kind in [
                ColumnKind::SalesPerDay7,
                ColumnKind::Confidence,
                ColumnKind::Trend,
                ColumnKind::DriftSpark,
            ] {
                let one = market_extra(i18n, kind, "Gilgamesh").expect("a market extra");
                let two = market_extra(i18n, kind, "Aether").expect("a market extra");
                let (l1, l2) = (
                    one.line2.expect("a second line").sub_label,
                    two.line2.expect("a second line").sub_label,
                );
                assert!(l1.ends_with("Gilgamesh"), "{kind:?}: {l1}");
                assert!(l2.ends_with("Aether"), "{kind:?}: {l2}");
                assert_ne!(l1, l2, "{kind:?}: the place is interpolated, not baked in");
            }
        });
    }

    /// `market_extra` takes the sell WORLD; the marks, the alternative
    /// revenue headers, the picker heading and the live sentence take the
    /// sell PLACE. Reading the production half back out of the source is
    /// the only way to see which variable reached which call — the same
    /// technique `the_page_wires_both_gates_to_what_it_fetches` uses.
    #[test]
    fn the_two_places_reach_the_labels_they_belong_to() {
        let production = production_source();
        assert!(
            production.contains(&format!("{}(i18n, kind, &{})", "market_extra", "sell_now")),
            "market_extra takes the sell WORLD's name"
        );
        assert!(
            production.contains(&format!("let {} = {}.get();", "sell_now", "sell_place")),
            "and `sell_now` is the sell world"
        );
        assert!(
            production.contains(&format!("f.{}({}.get(), buy_place.get())", "marks", "revenue_place")),
            "the header marks name the sell PLACE"
        );
        assert!(
            production.contains(&format!("{}: {}.get(),", "sell_place", "revenue_place")),
            "the picker's Revenue heading names the sell PLACE"
        );
        assert!(
            production.contains(&format!("{} = {}.get(),", "sell", "revenue_place")),
            "the live formula sentence names the sell PLACE"
        );
        // …and the place memo itself goes through the pure resolver, whose
        // own body holds the lab gate — or a flag-off page with
        // `?sell-scope=region` would rename every revenue label it shows.
        //
        // Aimed at `revenue_place_for`, NOT at a bare
        // `sell_scope_for(preview.get(), sell_scope())`: that string is
        // also written by the live-sentence branch added in Step 4 of this
        // same task, and by Task 6's strip select and table prop, so it
        // would pass without `revenue_place` consulting anything. The
        // gate's own behaviour is what
        // `the_two_places_agree_until_the_scope_moves` proves; this pins
        // that the memo actually calls the function that has it.
        assert!(
            production_squeezed().contains(&format!(
                "{}(preview.get(),{}(),",
                "revenue_place_for", "sell_scope"
            )),
            "`revenue_place` must resolve through `revenue_place_for`, which \
             is where the lab gate lives"
        );
    }

    /// The two names are the same string until a lab-on URL asks for a
    /// wider scope. This is the flag-off byte-identity proof for every
    /// label this task moved: with the toggle off, or at the default scope,
    /// `revenue_place` and `sell_place` are indistinguishable, so the marks,
    /// the picker heading, the alternative revenue sub-labels and the live
    /// sentence render exactly what they render today.
    #[test]
    fn the_two_places_agree_until_the_scope_moves() {
        for preview in [false, true] {
            for param in [None, Some(SellScope::default())] {
                assert_eq!(
                    revenue_place_for(preview, param, "Gilgamesh", Some("Aether"), "North-America"),
                    "Gilgamesh",
                    "preview={preview} param={param:?}"
                );
            }
        }
        // Lab off, wider param: still the sell world.
        assert_eq!(
            revenue_place_for(false, Some(SellScope(Scope::Region)), "Gilgamesh", Some("Aether"), "North-America"),
            "Gilgamesh"
        );
        // Lab on, wider param: the wider name, and the region when no
        // datacenter has resolved yet.
        assert_eq!(
            revenue_place_for(true, Some(SellScope(Scope::Datacenter)), "Gilgamesh", Some("Aether"), "North-America"),
            "Aether"
        );
        assert_eq!(
            revenue_place_for(true, Some(SellScope(Scope::Datacenter)), "Gilgamesh", None, "North-America"),
            "North-America"
        );
        assert_eq!(
            revenue_place_for(true, Some(SellScope(Scope::Region)), "Gilgamesh", Some("Aether"), "North-America"),
            "North-America"
        );
    }

    /// `header_extras` ends in a catch-all that delegates to
    /// `market_extra`, which returns `None` for a non-market kind and makes
    /// the whole column `continue`. A column with no arm of its own
    /// therefore ships a header with no tooltip and the key it was written
    /// for ships dead in seven locales. Two arms already exist for exactly
    /// this reason (`HopGain`, `HopWorlds`); Scope vs home needs the third,
    /// because the sign convention only exists in that string.
    #[test]
    fn the_scope_vs_home_header_has_its_own_extras_arm() {
        let production = production_source();
        assert!(
            production.contains("ColumnKind::ScopeVsHome => HeaderExtra {"),
            "no `header_extras` arm: the catch-all's `market_extra` returns \
             None for this kind and the tooltip never renders"
        );
        assert_eq!(
            production.matches("analyzer_scope_vs_home_help").count(),
            1,
            "the tooltip key is read exactly once, by that arm"
        );
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            assert!(
                market_extra(use_i18n(), ColumnKind::ScopeVsHome, "Aether").is_none(),
                "if this ever returns Some, delete the arm instead of keeping both"
            );
        });
    }

    /// `recipe_analyzer_calc_formula_live` reads "‹revenue› **on** {{sell}}"
    /// against "‹cost› **across** {{buy}}" deliberately: `on` is a world,
    /// `across` is a scope. Feeding a datacenter into the `on` slot would
    /// read "Sale median on Aether" two rows under "Sell on: Gilgamesh" and
    /// assert the one thing retainers cannot do. A scoped variant is
    /// selected when the sell scope is wider, and the default sentence is
    /// untouched — which is also what keeps the flag-off and default-scope
    /// rendering byte-identical.
    #[test]
    fn the_live_formula_sentence_scopes_the_sell_slot() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let i18n = use_i18n();
            let plain = t_string!(
                i18n,
                recipe_analyzer_calc_formula_live,
                revenue = "Sale median".to_string(),
                sell = "Gilgamesh".to_string(),
                tax = "5% tax".to_string(),
                cost = "Cheapest listing".to_string(),
                buy = "Aether".to_string()
            )
            .to_string();
            let scoped = t_string!(
                i18n,
                recipe_analyzer_calc_formula_live_scoped,
                revenue = "Sale median".to_string(),
                sell = "Aether".to_string(),
                tax = "5% tax".to_string(),
                cost = "Cheapest listing".to_string(),
                buy = "Aether".to_string()
            )
            .to_string();
            assert!(plain.contains("on Gilgamesh"), "{plain}");
            assert!(!scoped.contains("on Aether −"), "{scoped}");
            assert!(scoped.contains("across Aether"), "{scoped}");
        });
        let production = production_source();
        assert!(
            production.contains("recipe_analyzer_calc_formula_live_scoped"),
            "the scoped variant must actually be selected somewhere"
        );
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test::market_extras recipe_analyzer::test::the_two_places recipe_analyzer::test::the_scope_vs_home_header recipe_analyzer::test::the_live_formula`
Expected: `market_extras_put_the_place_they_are_given_on_the_second_line` PASSES today (keep it — it is the regression net for the next two steps); the other four FAIL on `cannot find function revenue_place_for`, the missing `header_extras` arm, and the unknown key.

- [ ] **Step 3: Add the page's second place**

In `RecipeAnalyzer`, beside the three pricing signals (`:3736-3739`):

```rust
    // Phase F's fourth pricing param. Read only through `sell_scope_for`,
    // never raw; the setter strips the default (Task 6).
    let (sell_scope, set_sell_scope) = filter_query_signal::<SellScope>(FILTER_SELL_SCOPE);
```

**Expected inter-task warning:** `set_sell_scope` has no reader until Task 6 hangs the strip select off it, so this task ends with `unused variable: set_sell_scope` on that line. That is Global Constraint 4's tolerated dead code, not a mistake — do **not** silence it with `_set_sell_scope` (Task 6 would have to rename it back) and do **not** `#[allow]` it. `cargo test` reports it and passes; `check_ci.sh` does not run until Task 9, by which point it is gone.

Add the pure resolver beside `sell_scope_for` (so the memo has nothing in it a test cannot reach):

```rust
/// The name revenue is priced at: the sell world under the default sell
/// scope, its datacenter or the region otherwise. `sell_place` stays the
/// sell **world**, and the difference is load-bearing — the market columns'
/// "7d · ‹place›" sub-labels, the sparkline feed, the 30-day body and Hop
/// gain's home run all read the sell world's own data at every sell scope
/// (spec §4), so naming the scope there would be a lie.
fn revenue_place_for(
    preview: bool,
    param: Option<SellScope>,
    sell_world: &str,
    datacenter: Option<&str>,
    region: &str,
) -> String {
    match sell_scope_for(preview, param)
        .map(SellScope::scope)
        .unwrap_or(Scope::World)
    {
        Scope::World => sell_world.to_string(),
        // No datacenter resolved yet: the region is the honest wider name,
        // and it is what the fetch key would use too.
        Scope::Datacenter => datacenter.unwrap_or(region).to_string(),
        Scope::Region => region.to_string(),
    }
}
```

and, immediately after the `sell_place` memo (`:3863-3868`):

```rust
    let revenue_place = Memo::new(move |_| {
        revenue_place_for(
            preview.get(),
            sell_scope(),
            &sell_place.get(),
            datacenter().as_deref(),
            &region.get(),
        )
    });
```

Pass it down: add `revenue_place=revenue_place` to the `<RecipeAnalyzerTable>` call beside `sell_place=sell_place`, and add the prop next to `sell_place`:

```rust
    /// The sell PLACE's name: the sell world under the default sell scope,
    /// its datacenter or region otherwise. Everything that names where
    /// revenue came from reads this; everything that names where the 7-day
    /// figures came from reads `sell_place`.
    #[prop(into)]
    revenue_place: Signal<String>,
```

- [ ] **Step 4: Switch the label sites and add the tooltip arm**

- `marks` (`:2670`): `let m = f.marks(revenue_place.get(), buy_place.get());`
- `header_extras`' `RevSignal` arm (`:2704`): `format!("{} · {}", short_signal(i18n, s), revenue_now)`, with `let revenue_now = revenue_place.get();` added beside the existing `let sell_now = sell_place.get();` and that comment extended to say why there are now two.
- `header_extras`, after the `ColumnKind::HopWorlds` arm (`:2742-2746`):

```rust
                ColumnKind::ScopeVsHome => HeaderExtra {
                    title: t_string!(i18n, analyzer_scope_vs_home_help).to_string(),
                    line2: None,
                    header_class: None,
                },
```

- `column_options`' `PickerContext` (`:3003`): `sell_place: revenue_place.get(),`
- the live sentence (`:4224-4232`): pick the variant on the scope, keeping the default sentence byte-identical.

```rust
                                let scoped = sell_scope_for(preview.get(), sell_scope())
                                    .is_some_and(|s| s.scope() != Scope::World);
                                // Two keys, not one edited key: "on {{sell}}"
                                // is right for a world and wrong for a
                                // datacenter, and rewording the shared string
                                // would move the default page's sentence.
                                if scoped {
                                    t_string!(
                                        i18n,
                                        recipe_analyzer_calc_formula_live_scoped,
                                        revenue = label_of(f.revenue_signal()),
                                        sell = revenue_place.get(),
                                        tax = t_string!(i18n, formula_term_tax).to_string(),
                                        cost = label_of(f.cost_signal()),
                                        buy = buy_place.get()
                                    )
                                    .to_string()
                                } else {
                                    t_string!(
                                        i18n,
                                        recipe_analyzer_calc_formula_live,
                                        revenue = label_of(f.revenue_signal()),
                                        sell = revenue_place.get(),
                                        tax = t_string!(i18n, formula_term_tax).to_string(),
                                        cost = label_of(f.cost_signal()),
                                        buy = buy_place.get()
                                    )
                                    .to_string()
                                }
```

Leave the `kind => match market_extra(i18n, kind, &sell_now)` arm exactly as it is.

Add one doc line to `PickerContext.sell_place` (`columns.rs:223`) and to `FormulaMarks.sell_place` (`formula.rs:266`):

```rust
    /// Where revenue is priced — the sell world, or the wider sell scope
    /// when a page has one. Never the place a market column's 7-day
    /// figures came from.
```

- [ ] **Step 5: Add the scoped sentence to all seven locales**

`recipe_analyzer_calc_formula_live_scoped` — the existing sentence with the `sell` slot re-worded from a world preposition to a scope one, matching how each locale already handles `{{buy}}`:

| locale | existing `…_live` | new `…_live_scoped` |
|---|---|---|
| en | `profit / unit = {{revenue}} on {{sell}} − {{tax}} − {{cost}} across {{buy}}` | `profit / unit = {{revenue}} across {{sell}} − {{tax}} − {{cost}} across {{buy}}` |
| fr | `profit / unité = {{revenue}} sur {{sell}} − {{tax}} − {{cost}} sur {{buy}}` | `profit / unité = {{revenue}} sur l'ensemble de {{sell}} − {{tax}} − {{cost}} sur {{buy}}` |
| de | `Gewinn / Stück = {{revenue}} auf {{sell}} − {{tax}} − {{cost}} in {{buy}}` | `Gewinn / Stück = {{revenue}} in {{sell}} − {{tax}} − {{cost}} in {{buy}}` |
| ja | `利益 / 個 = {{sell}}の{{revenue}} − {{tax}} − {{buy}}の{{cost}}` | `利益 / 個 = {{sell}}全体の{{revenue}} − {{tax}} − {{buy}}の{{cost}}` |
| cn | `每件利润 = {{sell}} 的{{revenue}} − {{tax}} − {{buy}} 的{{cost}}` | `每件利润 = {{sell}} 范围内的{{revenue}} − {{tax}} − {{buy}} 的{{cost}}` |
| ko | `개당 이익 = {{sell}}의 {{revenue}} − {{tax}} − {{buy}}의 {{cost}}` | `개당 이익 = {{sell}} 전체의 {{revenue}} − {{tax}} − {{buy}}의 {{cost}}` |
| tc | `每件利潤 = {{sell}} 的{{revenue}} − {{tax}} − {{buy}} 的{{cost}}` | `每件利潤 = {{sell}} 範圍內的{{revenue}} − {{tax}} − {{buy}} 的{{cost}}` |

Verify: every locale is now **1797** keys.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test`
Expected: PASS, **78 passed** (73 after Task 4 + 5 — Step 1 adds five `#[test]`s, one of which passed on arrival). `formula_marks_labels_name_signal_and_place` and `market_headers_carry_their_tooltip_and_the_window` must be green **unchanged** — they are what proves the swap did not cross the two names.

- [ ] **Step 7: Commit**

```bash
git add ultros-frontend/ultros-app/src/analyzer_kit ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs ultros-frontend/ultros-app/locales
git commit -m "feat(recipe-analyzer): name the sell place on the revenue side, the sell world on the 7d columns"
```

---

### Task 6: The fourth select, the active-filter count and Clear all

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:299-305` (beside `buy_scope_options`), `:2586-2595` (the table's signals), `:2905-2955` (`active_filters`), `:3059-3075` (`clear_all`), `:3875-3925` (`strip_terms`)
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/strip.rs` — **no change**; `StripTerm.place_select` already exists (`strip.rs:25`) and `FormulaStrip` already renders it
- Modify: `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json` (2 keys each)

**Interfaces:**
- Consumes: `StripTerm.{place, place_select}` and `StripSelect` (`strip.rs:11-40`), `SellScope` and `sell_scope_for` (Task 1), `FILTER_SELL_SCOPE` (Task 1), `revenue_place` (Task 5).
- Produces:
  - `fn sell_scope_options(i18n) -> Vec<(&'static str, String)>` — `[("world", …), ("datacenter", …), ("region", …)]`, reusing the existing `datacenter` and `region` keys exactly as `buy_scope_options` does.
  - The revenue `StripTerm` gains `place_select: Some(StripSelect { … })` writing `?sell-scope=` through the default-stripping setter.
  - `RecipeAnalyzerTable`'s new required prop `sell_scope: Option<SellScope>` — the page resolves it through `sell_scope_for` **inside the Suspense closure**, which is what makes the table rebuild when it changes (see Task 8's I2 note). The table's `active_filters` reads that one value; only the *setter* comes from a `filter_query_signal` here, so the table has a single source of truth for the scope.
  - `active_filters` gains `FILTER_SELL_SCOPE`; `clear_all` calls `set_sell_scope(None)`.
  - i18n keys `sell_scope_this_world`, `formula_change_sell_scope_aria`.
- **Flag-off:** the strip is only ever rendered under `preview` — the inline row sits inside `<Show when=move || preview.get()>` (`:4310`) and `MarketMenu`'s popover puts it behind `<Show when=move || preview>` whose `fallback` is the three flag-off `PricingSelect`s (`:430-436`), which this task does not touch. So the fourth `<select>` cannot reach a flag-off DOM. `active_filters` pushes the key only when the prop is `Some`, and the page hands it `sell_scope_for(preview.get(), …)`, which is `None` flag-off — so a bookmarked `?sell-scope=region` does not change the "no active filters" hint, asserted by `the_sell_scope_is_counted_and_cleared_like_the_other_market_params`. `clear_all` is deliberately **not** gated: clearing an absent param is a no-op, and a user who turns the lab off after setting a scope must still be able to clear it.

- [ ] **Step 1: Write the failing tests**

```rust
    /// One strip term can carry BOTH selects — the signal and the place —
    /// and still show the resolved place name between them. That is the
    /// mechanism behind the spec's "fourth Market select": the cost chip
    /// already has two, and Phase F gives the revenue chip its second.
    ///
    /// This renders a hand-built term, so it pins the COMPONENT, not the
    /// page's `strip_terms` (a closure over the page's signals, which no
    /// unit test can call). The production half is pinned by the
    /// source-read assertions below it.
    #[test]
    fn a_strip_term_carries_both_a_signal_select_and_a_place_select() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let i18n = use_i18n();
            let terms = vec![
                StripTerm::fixed(TermRole::Result, Signal::derive(|| "Profit / unit".into())),
                StripTerm {
                    role: TermRole::Revenue,
                    label: Signal::derive(String::new),
                    place: Some(Signal::derive(|| "Aether".to_string())),
                    select: Some(StripSelect {
                        value: Signal::derive(|| "listing-min".to_string()),
                        options: cost_basis_options(i18n),
                        on_change: Callback::new(|_: String| {}),
                        aria: "signal".into(),
                    }),
                    place_select: Some(StripSelect {
                        value: Signal::derive(|| "datacenter".to_string()),
                        options: sell_scope_options(i18n),
                        on_change: Callback::new(|_: String| {}),
                        aria: t_string!(i18n, formula_change_sell_scope_aria).to_string(),
                    }),
                    degraded: Signal::derive(|| false),
                },
            ];
            let html = view! { <FormulaStrip terms=terms layout=StripLayout::Stacked /> }.to_html();
            assert_eq!(
                html.matches("<select").count(),
                2,
                "one revenue term, two selects: {html}"
            );
            assert!(html.contains("Aether"), "the resolved place stays visible: {html}");
            assert!(html.contains("value=\"region\""), "{html}");
        });

        // The production strip: the revenue term really does grow the
        // second select, and the page really does end up with four.
        let production = production_source();
        assert_eq!(
            production.matches("place_select: Some(StripSelect {").count(),
            2,
            "the cost chip's and the revenue chip's — four selects on the strip"
        );
        assert!(
            production.contains(&format!("options: {}(i18n),", "sell_scope_options")),
            "…and the revenue one offers the sell-scope tokens"
        );
    }

    /// The three sell-scope tokens are the buy-scope tokens, and every one
    /// of them has a label in every locale — a select whose option renders
    /// blank is how a bookmarked value becomes unreachable. The `world`
    /// label is its own key, not the buy side's: "This world only" belongs
    /// to a buying sentence, and this one is where a price is READ.
    #[test]
    fn every_sell_scope_token_has_a_picker_label() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let i18n = use_i18n();
            let options = sell_scope_options(i18n);
            let tokens: Vec<&str> = options.iter().map(|(t, _)| *t).collect();
            assert_eq!(tokens, ["world", "datacenter", "region"]);
            for (token, label) in &options {
                assert!(!label.is_empty(), "{token} has no label");
                assert_eq!(token.parse::<SellScope>().unwrap().to_string(), *token);
            }
            assert_ne!(
                options[0].1,
                t_string!(i18n, buy_scope_home_world),
                "the sell side's `world` label is its own string"
            );
        });
    }

    /// The sell scope is counted like the three pricing params it sits
    /// beside, and Clear all resets it — but the count is driven by the
    /// prop the page already gated, so a bookmarked `?sell-scope=` cannot
    /// change the flag-off page's "no active filters" hint.
    #[test]
    fn the_sell_scope_is_counted_and_cleared_like_the_other_market_params() {
        let production = production_source();
        assert!(
            production.contains(&format!("if {}.is_some() {{", "sell_scope")),
            "active_filters counts the lab-gated prop, not a raw query read"
        );
        assert!(
            production.contains(&format!("{}(FILTER_SELL_SCOPE)", "active.push")),
            "…and pushes the same key the URL uses"
        );
        assert!(
            production.contains(&format!("{}(None);", "set_sell_scope")),
            "Clear all must reset it"
        );
        assert!(
            !production.contains(&format!("{}.get_untracked()", "sell_scope")),
            "the table never reads the scope untracked: the page resolves it \
             inside the Suspense closure and hands it down"
        );
        // The positive half of that rule, and the thing the plan's own
        // self-review called out as unpinned: the scope has to be READ
        // inside the Suspense closure, because that read is what makes a
        // scope change rebuild the table and re-run the pricing memo. The
        // negative assertion above only bans the wrong way of doing it.
        // Squeezed (rustfmt does not touch `view!` bodies, but a needle
        // that survives reformatting either way costs nothing), and
        // anchored on the `sell_scope=` prop prefix: the same call appears
        // three more times in this module — the `revenue_place` memo, the
        // strip select's `value`, and the live sentence's `scoped` — and
        // only this one is the hand-off that forces the rebuild.
        assert!(
            production_squeezed()
                .contains("sell_scope=sell_scope_for(preview.get(),sell_scope())"),
            "the page must resolve the scope INSIDE the Suspense closure and \
             pass it as a prop; nothing else rebuilds the table when it moves"
        );
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test::a_strip_term recipe_analyzer::test::every_sell_scope recipe_analyzer::test::the_sell_scope_is_counted`
Expected: FAIL — `cannot find function sell_scope_options`, unknown key `formula_change_sell_scope_aria`.

- [ ] **Step 3: Add the options and the two keys**

After `buy_scope_options` (`:299-305`):

```rust
/// Which market the sale price is READ from. The same three tokens the buy
/// side uses, with their own "this world" label: the buy side's reads "This
/// world only" in a buying sentence, and this one sits in a chip about
/// where a price comes from. Datacenter and Region reuse the shared nouns.
fn sell_scope_options(i18n: I18nContext<Locale, I18nKeys>) -> Vec<(&'static str, String)> {
    vec![
        ("world", t_string!(i18n, sell_scope_this_world).to_string()),
        ("datacenter", t_string!(i18n, datacenter).to_string()),
        ("region", t_string!(i18n, region).to_string()),
    ]
}
```

Locale values. Both strings are deliberately about *reading a price*, never about travelling to sell — retainers list only on the player's own world, and this control cannot change that:

| key | en | fr | de | ja | cn | ko | tc |
|---|---|---|---|---|---|---|---|
| `sell_scope_this_world` | `Your sell world` | `Votre monde de vente` | `Deine Verkaufswelt` | `販売ワールド` | `你的销售服务器` | `내 판매 서버` | `你的銷售伺服器` |
| `formula_change_sell_scope_aria` | `Change which market the sale price is read from` | `Changer le marché sur lequel le prix de vente est lu` | `Ändern, auf welchem Markt der Verkaufspreis abgelesen wird` | `販売価格を読み取る市場を変更` | `更改读取售价的市场范围` | `판매 가격을 읽어올 시장 변경` | `變更讀取售價的市場範圍` |

Verify: every locale is now **1799** keys.

- [ ] **Step 4: Hang the select off the revenue term**

In `strip_terms` (`:3878-3901`), on the `TermRole::Revenue` term: change `place: Some(sell_place.into())` to `place: Some(revenue_place.into())`, and replace its `place_select: None` with

```rust
                place_select: Some(StripSelect {
                    value: Signal::derive(move || {
                        sell_scope_for(preview.get(), sell_scope())
                            .unwrap_or_default()
                            .to_string()
                    }),
                    options: sell_scope_options(i18n),
                    on_change: Callback::new(move |v: String| {
                        let parsed = v.parse::<SellScope>().ok();
                        // `SellScope::default()` is the WORLD, not
                        // `Scope::default()`'s datacenter: stripping the
                        // wrong one here would rewrite every URL.
                        set_sell_scope(parsed.filter(|s| *s != SellScope::default()));
                    }),
                    aria: t_string!(i18n, formula_change_sell_scope_aria).to_string(),
                }),
```

- [ ] **Step 5: Count it and clear it**

Add the table prop, beside `revenue_place`:

```rust
    /// The sell scope the page resolved through `sell_scope_for` — `None`
    /// with the lab off and at the default scope. A plain value, not a
    /// signal: the page reads it inside the Suspense closure, so a scope
    /// change rebuilds the table, which is what makes the pricing path
    /// re-resolve (Task 8).
    sell_scope: Option<SellScope>,
```

In the table, add only the **setter** beside the other three (`:2591`) — the value comes from the prop, so there is one source of truth:

```rust
    // Only the setter: `Clear all` writes it, and everything that READS the
    // scope inside this component reads the `sell_scope` prop, which the
    // page already put through the lab gate.
    let (_, set_sell_scope) = filter_query_signal::<SellScope>(FILTER_SELL_SCOPE);
```

In `active_filters`, after the `FILTER_BUY_SCOPE` block (`:2925-2927`):

```rust
        // Lab-gated at the source, unlike the three above: those are
        // pre-lab params, and a bookmarked `?sell-scope=` must not change
        // the flag-off page's "no active filters" hint.
        if sell_scope.is_some() {
            active.push(FILTER_SELL_SCOPE);
        }
```

and in `clear_all` (`:3059`), after `set_buy_scope(None);`:

```rust
        set_sell_scope(None);
```

Finally, pass the prop from the Suspense closure (`:4355-4375`), beside `preview=preview.get()`:

```rust
                                        sell_scope=sell_scope_for(preview.get(), sell_scope())
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test analyzer_kit::strip`
Expected: PASS, `recipe_analyzer::test` **81** (78 after Task 5 + 3). `fixed_terms_render_static_chips_and_select_terms_render_selects` (strip.rs, **1 passed**) must be green unchanged — the strip component itself did not move.

- [ ] **Step 7: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs ultros-frontend/ultros-app/locales
git commit -m "feat(recipe-analyzer): the sell-scope select, counted in filters and reset by Clear all"
```

---

### Task 7: The page fetches the sell-scope bodies, and says so when they fail

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:2427-2458` (beside `SellHistory` / `raw_sales_key`), `:3374-3380` (the amber banner), `:3754-3756` (`formula_page`), `:3940-3990` and `:4130-4140` (the page's memos and the resource), `:4355-4375` (the Suspense join)
- Modify: `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json` (1 key each)

**Interfaces:**
- Consumes: `needed_bodies`, `BodyRole::{CheapestSellScope, SellScopeStats}`, `RecipeNeeds.{sell_scope_is_buy_scope, rev_signals, buy_scope_is_sell_world}` (Task 2); `NeededSignals.rev` (Task 2); `seat_sell_scope` / `sell_scope_for` (Task 1); `revenue_place` (Task 5); `get_cheapest_listings`, `get_sale_stats`, `SALE_STATS_WINDOW_DAYS`.
- Produces:
  - `struct SellScopeBodies { listings: Option<CheapestListings>, stats: Option<BulkSaleStats>, listings_failed: bool, stats_failed: bool }` — `Clone + Debug + PartialEq + serde::{Serialize, Deserialize}` (an `ArcResource` value round-trips through `JsonSerdeCodec`).
  - `async fn fetch_sell_scope(name: String, want_listings: bool, want_stats: bool) -> SellScopeBodies`.
  - `fn sell_scope_key(formula, needs, place) -> Option<(String, bool, bool)>`.
  - `RecipeAnalyzerTable`'s new required props `sell_scope_bodies: Option<SellScopeBodies>` and `sell_scope_is_buy_scope: bool` (consumed by Task 8).
  - i18n key `recipe_analyzer_sell_scope_unavailable`.
- **Flag-off:** `formula_page` goes through `seat_sell_scope`, which returns the formula unchanged with the lab off, so `sell_scope_key` sees `Scope::World`, `needed_bodies` skips its Phase F block entirely and the resource key is `None` — **no request is issued**, pinned by `the_sell_scope_bodies_are_only_requested_when_a_wider_scope_is`. The one new markup element is a second amber line whose condition is `scope_bodies_failed`, and `sell_scope_bodies` is `None` flag-off so that is `false`; the existing `(buy_stats_error || sell_stats_error)` line's condition is untouched, which keeps the flag-off DOM byte-identical (a `.then(..)` that yields `None` emits the same `<!>` marker it emits today).

- [ ] **Step 1: Write the failing tests**

```rust
    /// The sell-scope resource key goes through `needed_bodies`, so the
    /// fetch gate lives in exactly one place — the rule `buy_stats_scope_key`
    /// and `stats_30_key` already follow.
    #[test]
    fn the_sell_scope_bodies_are_only_requested_when_a_wider_scope_is() {
        let world = ProfitFormula::recipe_from_query(None, None, None);
        let needs = RecipeNeeds::default();
        assert_eq!(sell_scope_key(&world, &needs, "Aether"), None);

        // Datacenter, listing revenue: the cheapest map only.
        let dc = seat_sell_scope(world.clone(), true, Some(SellScope(Scope::Datacenter)));
        assert_eq!(
            sell_scope_key(&dc, &needs, "Aether"),
            Some(("Aether".to_string(), true, false))
        );

        // Datacenter, sale revenue: both halves.
        let dc_stats = seat_sell_scope(
            ProfitFormula::recipe_from_query(None, Some(PriceSignal::SaleMedian), None),
            true,
            Some(SellScope(Scope::Datacenter)),
        );
        assert_eq!(
            sell_scope_key(&dc_stats, &needs, "Aether"),
            Some(("Aether".to_string(), true, true))
        );

        // The scope matched the buy scope, whose cheapest body is
        // unconditional: only the statistics half is left to fetch.
        let deduped = RecipeNeeds {
            sell_scope_is_buy_scope: true,
            ..RecipeNeeds::default()
        };
        assert_eq!(
            sell_scope_key(&dc_stats, &deduped, "Aether"),
            Some(("Aether".to_string(), false, true))
        );
        // …and with a sale COST signal the buy side already fetched those
        // statistics, so there is nothing left at all.
        let both = seat_sell_scope(
            ProfitFormula::recipe_from_query(
                Some(PriceSignal::SaleMin),
                Some(PriceSignal::SaleMedian),
                Some(BuyScope::Datacenter),
            ),
            true,
            Some(SellScope(Scope::Datacenter)),
        );
        assert_eq!(sell_scope_key(&both, &deduped, "Aether"), None);

        // But if the buy scope ALIASES the sell world, `BuyScopeStats` is
        // never in the set and there is nothing to reuse — which is why the
        // page fills `buy_scope_is_sell_world` from its real gate rather
        // than letting `Default` answer `false`.
        let aliased = RecipeNeeds {
            sell_scope_is_buy_scope: true,
            buy_scope_is_sell_world: true,
            ..RecipeNeeds::default()
        };
        let world_buy = seat_sell_scope(
            ProfitFormula::recipe_from_query(
                Some(PriceSignal::SaleMin),
                Some(PriceSignal::SaleMedian),
                None,
            ),
            true,
            Some(SellScope(Scope::Datacenter)),
        );
        assert_eq!(
            sell_scope_key(&world_buy, &aliased, "Aether"),
            Some(("Aether".to_string(), false, true))
        );
    }

    /// The page consults the gate rather than a constant, fills the needs
    /// from its real page state, and does not smuggle in a third viewport
    /// read. `-D warnings` proves only that *something* calls each one.
    #[test]
    fn the_page_wires_the_sell_scope_to_what_it_fetches() {
        let production = production_source();
        assert!(
            production.contains(&format!("{}(&formula, &needs, &{})", "sell_scope_key", "place")),
            "the resource key must come from `sell_scope_key`"
        );
        assert!(
            production.contains(&format!(
                "{}: {}.get(),",
                "buy_scope_is_sell_world", "buy_scope_is_sell_world"
            )),
            "…over the page's real alias gate, not RecipeNeeds::default()"
        );
        // The two places must be ONE place. `revenue_place`'s datacenter
        // arm falls back to the region when no datacenter has resolved
        // yet; `sell_scope_key` sends a name to the API and
        // `sell_scope_is_buy_scope` compares a name against the buy
        // scope's. If either of those reads anything but `revenue_place`,
        // the page fetches one market, dedupes against a second and labels
        // a third — with no test able to see it, because each half is
        // internally consistent. Squeezed, because both are multi-argument
        // lines rustfmt is free to wrap.
        let squeezed = production_squeezed();
        assert!(
            squeezed.contains(&format!("let{}={}.get();", "place", "revenue_place")),
            "the name `sell_scope_key` sends is `revenue_place`, fallback arm \
             included — not `sell_place`, and not a second resolution"
        );
        assert!(
            squeezed.contains(&format!(
                "{}.get()=={}.get()",
                "revenue_place", "buy_scope_name"
            )),
            "…and the dedupe gate compares that same name against the buy \
             scope's, or `needed_bodies` skips a body nobody fetched"
        );
        // Global Constraint 6: Phase F adds no lazy fetch, so the viewport
        // signal is still read by exactly the two E2 gates.
        let reads = production.replace("use_wide_viewport", "");
        assert_eq!(
            reads.matches("wide_viewport.get()").count(),
            2,
            "Phase F must not add a third viewport-gated fetch"
        );
    }

    /// A sell-scope body that was asked for and did not arrive must be
    /// said, not silently re-priced: revenue falls through `SignalView`'s
    /// base layer to the buy scope while the strip, the picker heading and
    /// the live sentence all still name the scope. Both halves count —
    /// `listings_failed` as much as `stats_failed`, because the listing
    /// half is the one a listing-min URL depends on.
    #[test]
    fn a_failed_sell_scope_body_says_so_instead_of_silently_repricing() {
        let none = SellScopeBodies {
            listings: None,
            stats: None,
            listings_failed: false,
            stats_failed: false,
        };
        assert!(!scope_bodies_failed(&None));
        assert!(!scope_bodies_failed(&Some(none.clone())));
        assert!(scope_bodies_failed(&Some(SellScopeBodies {
            stats_failed: true,
            ..none.clone()
        })));
        assert!(
            scope_bodies_failed(&Some(SellScopeBodies {
                listings_failed: true,
                ..none
            })),
            "a failed cheapest map is the half a listing-min URL prices from"
        );
        let production = production_source();
        assert!(
            production.contains("recipe_analyzer_sell_scope_unavailable"),
            "the banner key must be rendered somewhere"
        );
        assert!(
            production.contains(&format!("{}(&sell_scope_bodies)", "scope_bodies_failed")),
            "…off the same helper this test pins"
        );
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test::the_sell_scope_bodies recipe_analyzer::test::the_page_wires_the_sell_scope recipe_analyzer::test::a_failed_sell_scope`
Expected: FAIL — `cannot find function sell_scope_key`, `cannot find struct SellScopeBodies`.

- [ ] **Step 3: Add the key, the payload and the fetch**

Beside `raw_sales_key` (`:2438`):

```rust
/// The sell-scope bodies' resource key: `(place name, want listings, want
/// statistics)`, or `None` when nothing is needed. Both halves go through
/// [`needed_bodies`] so the gate lives in one place, and they are separate
/// booleans because the dedupe against the buy scope can cover one and not
/// the other.
fn sell_scope_key(
    formula: &ProfitFormula,
    needs: &RecipeNeeds,
    place: &str,
) -> Option<(String, bool, bool)> {
    let bodies = needed_bodies(formula, needs);
    let want_listings = bodies.contains(&BodyRole::CheapestSellScope);
    let want_stats = bodies.contains(&BodyRole::SellScopeStats(SALE_STATS_WINDOW_DAYS));
    (want_listings || want_stats).then(|| (place.to_string(), want_listings, want_stats))
}

/// One sell-scope payload. Two bodies behind one resource so the Suspense
/// join stays a six-tuple and the "which half did we get" logic lives in
/// one place, the way [`SellHistory`] already folds the rollup and its
/// failover.
// `ArcResource` values round-trip through `JsonSerdeCodec`, so serde is
// required (both field types already derive it).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct SellScopeBodies {
    listings: Option<CheapestListings>,
    stats: Option<BulkSaleStats>,
    /// The cheapest map was asked for and did not arrive: revenue falls
    /// through `SignalView`'s base layer to the buy scope, which is a
    /// different market from the one every label still names.
    listings_failed: bool,
    /// A statistics body was asked for and did not arrive: the revenue
    /// signal degrades to the listing, exactly as a failed buy-scope or
    /// sell-world body does.
    stats_failed: bool,
}

/// Either half of the sell-scope payload was asked for and missed. `false`
/// when there is no payload at all, which is every flag-off page and every
/// URL at the default sell scope.
fn scope_bodies_failed(bodies: &Option<SellScopeBodies>) -> bool {
    bodies
        .as_ref()
        .is_some_and(|b| b.listings_failed || b.stats_failed)
}

async fn fetch_sell_scope(name: String, want_listings: bool, want_stats: bool) -> SellScopeBodies {
    let listings = match want_listings {
        true => get_cheapest_listings(&name).await.ok(),
        false => None,
    };
    let stats = match want_stats {
        true => get_sale_stats(&name, SALE_STATS_WINDOW_DAYS).await.ok(),
        false => None,
    };
    SellScopeBodies {
        listings_failed: want_listings && listings.is_none(),
        stats_failed: want_stats && stats.is_none(),
        listings,
        stats,
    }
}
```

- [ ] **Step 4: Wire the page**

Change `formula_page` (`:3754-3756`) to:

```rust
    let formula_page = Memo::new(move |_| {
        // The lab gate, never the raw param: with the toggle off this
        // leaves `Term::Fixed(Scope::World)`, which is what every
        // pre-Phase-F URL has always produced.
        seat_sell_scope(
            ProfitFormula::recipe_from_query(cost_basis(), revenue_metric(), buy_scope()),
            preview.get(),
            sell_scope(),
        )
    });
```

and add, after the 30-day `Effect` (`:4134`):

```rust
    // The sell scope resolved to the same place the buy side already
    // fetches: its cheapest body holds these rows, and (when a sale cost
    // signal fetched it) its statistics body does too.
    let sell_scope_is_buy_scope =
        Memo::new(move |_| revenue_place.get() == buy_scope_name.get());

    // Phase F's bodies. A formula body, so it joins the Suspense gate: the
    // table cannot price a row without the map revenue comes from. `None` —
    // no fetch — at the default sell scope, which is every flag-off page and
    // every URL that has not asked for a wider one.
    let sell_scope_source = Memo::new(move |_| {
        let formula = formula_page.get();
        let signals = needs_page.get();
        let needs = RecipeNeeds {
            sell_scope_is_buy_scope: sell_scope_is_buy_scope.get(),
            // The page's REAL alias gate. `needed_bodies` computes
            // `BuyScopeStats` from this, and the sell side's dedupe only
            // fires when that body is actually in the set — a defaulted
            // `false` here would claim a body nobody fetched and leave
            // every `rev-sale-*` cell permanently "—".
            buy_scope_is_sell_world: buy_scope_is_sell_world.get(),
            cost_signals: signals.cost,
            rev_signals: signals.rev,
            ..RecipeNeeds::default()
        };
        let place = revenue_place.get();
        sell_scope_key(&formula, &needs, &place)
    });
    let sell_scope_bodies = ArcResource::new(
        sell_scope_source,
        move |key: Option<(String, bool, bool)>| async move {
            match key {
                Some((name, listings, stats)) => {
                    Some(fetch_sell_scope(name, listings, stats).await)
                }
                None => None,
            }
        },
    );
```

Add `sell_scope_bodies` to the Suspense join's tuple and to the `match`, and pass through beside the props Tasks 5 and 6 added:

```rust
                                        sell_scope_bodies=bodies
                                        sell_scope_is_buy_scope=sell_scope_is_buy_scope.get()
```

Add the two props to `RecipeAnalyzerTable` (beside `buy_stats_aliased`, `:2517`):

```rust
    /// Phase F's payload: the sell scope's cheapest map and, under a sale
    /// revenue signal, its statistics. `None` at the default sell scope.
    sell_scope_bodies: Option<SellScopeBodies>,
    /// The sell scope resolved to the buy scope's place, so the buy-side
    /// bodies stand in for it.
    sell_scope_is_buy_scope: bool,
```

- [ ] **Step 5: Say it when a body misses**

Beside the existing amber line (`:3374-3380`), add a second one — not a change to the first, whose condition and text must stay exactly as they are:

```rust
            {scope_bodies_failed(&sell_scope_bodies)
                .then(|| view! {
                    <div class="text-amber-400 text-sm">
                        {t!(i18n, recipe_analyzer_sell_scope_unavailable, place = revenue_place.get())}
                    </div>
                })}
```

The place name is in the message on purpose: without it the line cannot be told from the sale-history one, and the whole point is that the label the player is looking at is not where these numbers came from.

Locale values for `recipe_analyzer_sell_scope_unavailable`:

- en: `Couldn't reach the {{place}} market — the expected sale price falls back to where ingredients are priced.`
- fr: `Impossible d'atteindre le marché de {{place}} — le prix de vente attendu revient à la portée d'achat des ingrédients.`
- de: `Der Markt {{place}} war nicht erreichbar — der erwartete Verkaufspreis fällt auf den Einkaufsbereich der Zutaten zurück.`
- ja: `{{place}} の市場に接続できませんでした。想定売却価格は素材の購入範囲の価格に戻ります。`
- cn: `无法读取 {{place}} 的市场数据——预期售价回退到采购材料的范围价格。`
- ko: `{{place}} 시장에 접근하지 못했습니다 — 예상 판매가는 재료를 구매하는 범위의 가격으로 되돌아갑니다.`
- tc: `無法讀取 {{place}} 的市場資料——預期售價回退到採購材料的範圍價格。`

Verify: every locale is now **1800** keys.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p ultros-app --lib`
Expected: PASS across the crate; `recipe_analyzer::test` **84** (81 after Task 6 + 3).

- [ ] **Step 7: Check the client build**

Run (no `RUSTFLAGS` in the environment):
```bash
cargo check -p ultros-app --no-default-features --features hydrate --target wasm32-unknown-unknown
```
Expected: exit 0. This is what proves `fetch_sell_scope`'s two awaits and the new resource compile for the client.

- [ ] **Step 8: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs ultros-frontend/ultros-app/locales
git commit -m "feat(recipe-analyzer): fetch the sell-scope bodies, and name the place when one fails"
```

---

### Task 8: The table resolves the revenue side — and the pin that the tested formula is the shipped one

This task exists on its own because it is where the phase's headline defect lived. The page and the table build the ledger in two different places, and only the table's reaches `price_rows`. A draft that seated the sell scope on `formula_page` alone left the table's `formula` memo (`:2648-2658`) at `Term::Fixed(World)` on **every production render**, so `scope_vs_home` was `Off` for every row, Price never moved, and the whole suite stayed green because `run_with` seated the scope on its own formula. That is Phase E2's median-tell escape verbatim. The fix is structural, not a new assertion: one seating function (Task 1), used by the page, the table **and** the harness, plus a source-read test that counts its callers.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:2640-2660` (the index resolution and the table's `formula` memo), `:2760-2800` (the `PriceInputs` literal), `:2762-2766` (the `priced` memo's captured clones), and `mod test`'s `run_with` at `:5098` (its revenue inputs move onto the production resolver)

**Interfaces:**
- Consumes: `SellScopeBodies`, `sell_scope_is_buy_scope` (Task 7); the `sell_scope: Option<SellScope>` prop (Task 6); `seat_sell_scope` (Task 1); `PriceInputs.{revenue_listings, revenue_stats}` (Task 3).
- Produces:
  - `enum RevenueSource { SellWorld, BuyScope, Scope, Missing }` and two pure resolvers, `revenue_listings_source` / `revenue_stats_source`. The three-way choice is the silent-re-price hazard, so it is a function with a test rather than a `match` buried in a component — and `run_with` calls the same function, so the harness's inputs are the production inputs the way its formula is already the production formula.
  - The table's `formula` memo, rewritten to go through `seat_sell_scope`.
- **Flag-off:** `sell_scope` is `None` (the page gated it), so both resolvers return `RevenueSource::SellWorld` and the table feeds `PriceInputs` exactly `sell_world_prices` and `Some(sell_stats_index)` — the same two values `revenue_listings` / `revenue_stats` defaulted to in Task 3. `seat_sell_scope(f, preview, None)` returns `f`, so the memo's value is `PartialEq`-identical to today's and cannot fire. Asserted end-to-end by `the_tables_own_formula_is_what_fills_the_scope_column`'s flag-off arm and by the two oracles from Task 3, which are re-run here.

- [ ] **Step 1: Write the failing tests**

```rust
    /// **The Phase F pin.** The page's ledger and the table's ledger are two
    /// different constructions and only the table's prices rows, so a scope
    /// seated on the page alone yields a column of dashes behind a green
    /// suite — which is exactly how Phase E2's median tell shipped. Three
    /// assertions, in order of how hard they are to fool:
    ///
    /// 1. `with_sell_scope` has ONE caller in the production half. A second
    ///    one means somebody re-inlined the seating and the two paths can
    ///    drift again.
    /// 2. `seat_sell_scope` has exactly three: its own definition, the
    ///    page's `formula_page`, and the TABLE's `formula`. Unwire the
    ///    table and this drops to two.
    /// 3. The two seatings are TOLD APART, so the count cannot be satisfied
    ///    by a wrapper. `fn table_formula(..) { seat_sell_scope(..) }`
    ///    keeps the count at three while the table stops calling it, so the
    ///    counts only bite when something also pins the two call *shapes*.
    /// 4. A pricing pass whose formula came out of that function — the same
    ///    call `run_with` makes — actually fills the column. This is the
    ///    behavioural half: the counts could all hold while the seating
    ///    did nothing.
    ///
    /// Assertion 3's needles are matched against `production_squeezed()`,
    /// not `production_source()`. Both seatings are calls rustfmt is
    /// obliged to break one-argument-per-line: the first argument alone,
    /// `ProfitFormula::recipe_from_query(cost_basis(), revenue_metric(),
    /// buy_scope()),`, is 76 characters at indent 12, so the call cannot
    /// fit in 100 columns and a single-line needle would pin text the
    /// formatter will never emit.
    #[test]
    fn the_tables_own_formula_is_what_fills_the_scope_column() {
        let production = production_source();
        assert_eq!(
            production.matches(&format!("{}(", "with_sell_scope")).count(),
            1,
            "`with_sell_scope` is called in exactly one place: `seat_sell_scope`"
        );
        assert_eq!(
            production.matches(&format!("{}(", "seat_sell_scope")).count(),
            3,
            "its definition, the page's `formula_page`, and the TABLE's \
             `formula` memo — if this reads 2, the table is unwired and the \
             column ships as dashes"
        );
        // The two call SHAPES, which is what makes the count above bite.
        // They are distinguishable on purpose: the page seats from signals
        // (`preview.get()`, `sell_scope()`), the table from its two props
        // (`preview`, `sell_scope`), so neither needle can stand in for the
        // other and a wrapper that keeps the count at three fails here.
        let squeezed = production_squeezed();
        assert!(
            squeezed.contains(
                "recipe_from_query(cost_basis(),revenue_metric(),buy_scope()),preview,sell_scope,)"
            ),
            "the TABLE's formula memo must seat the scope from its own props"
        );
        assert!(
            squeezed.contains(",preview.get(),sell_scope(),)"),
            "…and the page's `formula_page` from its own signals"
        );

        // The behavioural half. `run_with` builds its formula with the same
        // function, so this exercises the production seating rather than a
        // hand-written `with_sell_scope`.
        let wanted = NeededSignals {
            scope_vs_home: true,
            ..NeededSignals::default()
        };
        let rows = run_with(
            PriceSignal::ListingMin,
            PriceSignal::ListingMin,
            &RunOpts {
                needs: wanted.clone(),
                sell_scope: Some(Scope::Region),
                scope_bodies: true,
                ..RunOpts::default()
            },
        );
        assert!(
            rows.iter()
                .any(|r| matches!(r.scope_vs_home, ScopeVsHome::Pair { .. })),
            "a pass seated through `seat_sell_scope` must fill the column"
        );
        // …and the flag-off arm of the same function leaves it empty.
        let off = seat_sell_scope(
            ProfitFormula::recipe_from_query(None, None, None),
            false,
            Some(SellScope(Scope::Region)),
        );
        assert_eq!(off.sell_scope(), Scope::World);
    }

    /// Which body the table prices revenue from. The middle case — the
    /// scope resolved to the buy scope's place, so the buy-side body stands
    /// in — is a silent re-price if it is wrong, and it is unreachable from
    /// a unit test while it lives inside the component, so it does not.
    #[test]
    fn the_table_resolves_the_revenue_side_from_the_pages_scope() {
        // NO `use RevenueSource::*;` here. Its `Scope` and `BuyScope`
        // variants land in the TYPE namespace and shadow the `Scope` alias
        // and the `BuyScope` enum this module imports from
        // `analyzer_kit::formula`, and `Scope::World` then fails to resolve
        // with `E0433: Scope is a variant, not a module`. Spell the
        // variants out.
        use RevenueSource::{BuyScope as FromBuyScope, Missing, Scope as FromScope, SellWorld};
        // Default scope: the sell world's own bodies, whatever else is true.
        for is_buy in [false, true] {
            for have in [false, true] {
                assert_eq!(revenue_listings_source(Scope::World, is_buy, have), SellWorld);
                assert_eq!(revenue_stats_source(Scope::World, is_buy, have), SellWorld);
            }
        }
        // Wider, body present: the scope's own, even if it also happens to
        // be the buy scope's place.
        assert_eq!(revenue_listings_source(Scope::Region, false, true), FromScope);
        assert_eq!(revenue_listings_source(Scope::Region, true, true), FromScope);
        // Wider, no body, but the place IS the buy scope: reuse it. That is
        // the dedupe `needed_bodies` counted on when it skipped the fetch.
        assert_eq!(
            revenue_listings_source(Scope::Datacenter, true, false),
            FromBuyScope
        );
        assert_eq!(
            revenue_stats_source(Scope::Datacenter, true, false),
            FromBuyScope
        );
        // Wider, no body, not the buy scope: nothing. `SignalView` falls to
        // its base layer for listings and `rev-sale-*` cells go "—" — and
        // Task 7's banner is what tells the player.
        assert_eq!(revenue_listings_source(Scope::Region, false, false), Missing);
        assert_eq!(revenue_stats_source(Scope::Region, false, false), Missing);

        // Squeezed, per `production_squeezed()`'s doc: both are three-argument
        // calls rustfmt breaks onto one line per argument, so `…_source(`
        // and `sell_scope_value` never share a source line.
        let squeezed = production_squeezed();
        assert!(
            squeezed.contains(&format!("{}(sell_scope_value,", "revenue_listings_source"))
                && squeezed.contains(&format!("{}(sell_scope_value,", "revenue_stats_source")),
            "the table must resolve through both helpers, not an inline match"
        );
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test::the_tables_own_formula recipe_analyzer::test::the_table_resolves`
Expected: FAIL — `cannot find function revenue_listings_source`, and the `seat_sell_scope` count reads 2 (the table is not wired yet), which is the assertion doing its job.

- [ ] **Step 3: Add the resolvers**

Beside `sell_scope_key` (Task 7):

```rust
/// Where the table reads one half of the revenue side from.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum RevenueSource {
    /// The sell world's own body: every pre-Phase-F page, and every page at
    /// the default sell scope.
    SellWorld,
    /// The buy-scope body stands in — the sell scope resolved to the same
    /// place name, which is why `needed_bodies` skipped the fetch.
    BuyScope,
    /// The sell scope's own body.
    Scope,
    /// A wider scope whose body did not arrive. Listings fall through
    /// `SignalView`'s base layer to the buy scope and `rev-sale-*` cells
    /// render "—"; the amber banner names the place.
    Missing,
}

fn revenue_source(scope: Scope, is_buy_scope: bool, have_body: bool) -> RevenueSource {
    match scope {
        Scope::World => RevenueSource::SellWorld,
        _ if have_body => RevenueSource::Scope,
        _ if is_buy_scope => RevenueSource::BuyScope,
        _ => RevenueSource::Missing,
    }
}

/// The cheapest map revenue's `over` layer reads.
fn revenue_listings_source(scope: Scope, is_buy_scope: bool, have_body: bool) -> RevenueSource {
    revenue_source(scope, is_buy_scope, have_body)
}

/// The statistics index a sale revenue signal reads. Same rule, named
/// separately because the dedupe can cover one half and not the other:
/// `CheapestBuyScope` is unconditional while `BuyScopeStats(7)` is not.
fn revenue_stats_source(scope: Scope, is_buy_scope: bool, have_body: bool) -> RevenueSource {
    revenue_source(scope, is_buy_scope, have_body)
}
```

Then **make the harness resolve through it too**, which is the same structural move as Task 1's `seat_sell_scope`: without it, "the tested path is the production path" is true of the *formula* and false of the *inputs*, and the component's arm → value mapping has no test at all. A mis-wire there ships `Pair { place: x, home: x }` — a whole column of `+0` — or silently re-prices under a scope label, and every existing test stays green because the harness picked its own maps by a rule the component does not use.

In `run_with` (`:5098`), replace Task 3 Step 7's two hand-written chains with the production function. Add one local immediately after `let use_scope = wider && o.scope_bodies;` (`wider` keeps its reader, so nothing above changes):

```rust
        // The SAME resolver the table runs, so the harness cannot pick a
        // map by a rule production does not use. `is_buy_scope` is `false`
        // here — the fixture's buy maps are a different place — and that
        // arm is covered directly by
        // `the_table_resolves_the_revenue_side_from_the_pages_scope`.
        let revenue_at = revenue_source(o.sell_scope.unwrap_or(Scope::World), false, use_scope);
```

and, in the `PriceInputs` literal:

```rust
            revenue_listings: match revenue_at {
                RevenueSource::SellWorld => o.sell_listings.then_some(&sell),
                RevenueSource::BuyScope => Some(&buy),
                RevenueSource::Scope => Some(&scope_listings),
                RevenueSource::Missing => None,
            },
            revenue_stats: match revenue_at {
                RevenueSource::SellWorld => o.sell_stats.then_some(&sell_index),
                RevenueSource::BuyScope => Some(&index),
                RevenueSource::Scope => Some(&scope_stats),
                RevenueSource::Missing => None,
            },
```

The three reachable arms are value-for-value what the chain they replace produced, so **no Task 3 test changes and neither oracle moves**: `World` → the sell world's maps gated on `o.sell_listings` / `o.sell_stats`, wider-with-bodies → the scope maps, wider-without → `None`.

- [ ] **Step 4: Seat the scope on the table's formula and resolve the two inputs**

Replace the table's `formula` memo (`:2648-2658`) with:

```rust
    let formula = Memo::new(move |_| {
        // Through the SAME function the page and the pricing harness use.
        // The page's `formula_page` answers fetch keys; THIS one prices
        // every row, and a scope seated only on the first is a column of
        // dashes that no unit test can see (Phase E2's median tell).
        let mut f = seat_sell_scope(
            ProfitFormula::recipe_from_query(cost_basis(), revenue_metric(), buy_scope()),
            preview,
            sell_scope,
        )
        .effective(buy_stats_loaded, revenue_stats_loaded);
        // The phase's one number change, and it only happens under the
        // lab: a 363,884% ROI off a single fake listing reads as noise, so
        // the clamped policy caps it at the display ceiling.
        if preview {
            f.roi = RoiMath::ClampedF64;
        }
        f
    });
```

and insert, **before** that memo and after the existing index construction (`:2639-2647`):

```rust
    // Where revenue is priced. Resolved once, from the scope the PAGE
    // gated and handed down — never from a `get_untracked()` read of the
    // query signal. The page passes this prop from inside the Suspense
    // closure, so a scope change rebuilds the table and re-runs this;
    // `the_sell_scope_is_counted_and_cleared_like_the_other_market_params`
    // pins both halves of that (the prop read inside the closure, and no
    // untracked read anywhere), because it is otherwise an accidental
    // invariant.
    let sell_scope_value = sell_scope.map(SellScope::scope).unwrap_or(Scope::World);
    let scope_prices = sell_scope_bodies
        .as_ref()
        .and_then(|b| b.listings.clone())
        .map(|l| Arc::new(CheapestListingsMap::from(l)));
    let scope_stats_index: Option<Arc<StatsIndex>> = sell_scope_bodies
        .as_ref()
        .and_then(|b| b.stats.as_ref())
        .map(|s| Arc::new(stats_index(s)));
    let revenue_prices: Option<Arc<CheapestListingsMap>> = match revenue_listings_source(
        sell_scope_value,
        sell_scope_is_buy_scope,
        scope_prices.is_some(),
    ) {
        RevenueSource::SellWorld => sell_world_prices.clone(),
        RevenueSource::BuyScope => Some(prices.clone()),
        RevenueSource::Scope => scope_prices,
        RevenueSource::Missing => None,
    };
    // `revenue_stats_loaded` is what `effective()` downgrades on, so it
    // must say "the body REVENUE reads arrived", never "the sell world's
    // did". `sell_stats_loaded` keeps its own meaning for `hop_signal`.
    let (revenue_stats_index, revenue_stats_loaded): (Option<Arc<StatsIndex>>, bool) =
        match revenue_stats_source(
            sell_scope_value,
            sell_scope_is_buy_scope,
            scope_stats_index.is_some(),
        ) {
            RevenueSource::SellWorld => (Some(sell_stats_index.clone()), sell_stats_loaded),
            RevenueSource::BuyScope => (buy_stats_index.clone(), buy_stats_loaded),
            RevenueSource::Scope => (scope_stats_index, true),
            RevenueSource::Missing => (None, false),
        };
```

Then:

- **Move the two `Option<Arc<…>>` values into the `priced` memo** alongside `prices` / `sell_world_prices` (`:2762-2766`): the memo's closure is `move`, so `revenue_prices` and `revenue_stats_index` need their own `let … = ….clone();` in that block or the memo will not compile.
- Feed the `PriceInputs` literal `revenue_listings: revenue_prices.as_deref(),` and `revenue_stats: revenue_stats_index.as_deref(),`.
- Fix the published pair (`:2559`): `stats_loaded` drives the strip's amber dot and the live sentence's `effective(loaded.0, loaded.1)`, whose **second** argument is the revenue side. Publishing `sell_stats_loaded` there would let a failed scope body leave the dot dark while the headers say the signal fell back:

```rust
    // The table is the only place that knows how each stats body actually
    // resolved; publish the loaded pair once so the page's strip and info
    // panel derive the fallback from the same two booleans the rows did.
    // The second half is the REVENUE side's body, not the sell world's —
    // `effective()`'s second argument — or a failed sell-scope fetch would
    // desync the strip from the headers.
    Effect::new(move |_| stats_loaded.set((buy_stats_loaded, revenue_stats_loaded)));
```

- [ ] **Step 5: Run the whole suite and the client build**

Run: `cargo test -p ultros-app --lib`
Expected: PASS, `recipe_analyzer::test` **86** (84 after Task 7 + 2). Both Task 3 oracles must still be green — this is the step that could move a default-scope number, because it is where `revenue_listings` / `revenue_stats` stop being defaults **and** where the harness stops choosing its maps by hand. Every Task 3 pricing test must also still be green unchanged; if one moved, the `revenue_source` mapping in Step 3 does not match the chain it replaced.

Run: `cargo check -p ultros-app --no-default-features --features hydrate --target wasm32-unknown-unknown`
Expected: exit 0.

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "feat(recipe-analyzer): seat the sell scope on the table's own formula, resolved from the page"
```

---

### Task 9: The changelog, the whole contract in one place, and every gate green

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/changelog.rs:33` (a new newest-first entry)
- Modify: `integration/runner.cjs` — the `analyzer-recipe` route string at **`:94`** (the route-map key) **and** at **`:144`** (the sweep list); they must stay identical, and the comment above `:94` names column counts that move
- Modify: `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json` (`labs_analyzer_recipe_desc` edited)
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs` (one contract test)

**Interfaces:**
- Consumes: everything above.
- Produces: no new API. This task exists because the URL contract, the changelog and the dead-code sweep are deliverables, not afterthoughts.
- **Flag-off:** the changelog is a different route; the e2e route already carries `labs=analyzer-recipe`, so the flag-off sweep entry (`/recipe-analyzer?world=Gilgamesh`, `:143`) is untouched. `labs_analyzer_recipe_desc` is only rendered inside the Labs settings card. The contract test re-asserts the flag-off half in one place: `BASE_COLUMN_ORDER` is still 7 and does not contain the token, `DEFAULT_COLS` is unchanged, `ADDABLE_FILTERS` is still 9, and the new column carries `lab: Some(LAB_ANALYZER_RECIPE)`.

- [ ] **Step 1: Write the failing whole-contract test**

```rust
    /// Phase F's complete URL surface, in one assertion, so a reviewer can
    /// read what a bookmark is promised without reconstructing it from six
    /// tests. Each half is also pinned where it lives; this is the index.
    #[test]
    fn phase_f_adds_exactly_one_key_and_one_column_token() {
        // One selection key, and it is NOT a row filter.
        assert_eq!(FILTER_SELL_SCOPE, "sell-scope");
        assert_eq!(ADDABLE_FILTERS.len(), 9);
        assert!(!ADDABLE_FILTERS.contains(&FILTER_SELL_SCOPE));
        // Its three values are the buy scope's three, and `world` is the
        // default the setter strips.
        assert_eq!(SellScope::default().to_string(), "world");

        // One column token, appended, lab-gated.
        assert_eq!(OPTIONAL_COLUMN_ORDER.len(), 23);
        assert_eq!(*OPTIONAL_COLUMN_ORDER.last().unwrap(), COL_SCOPE_VS_HOME);
        assert_eq!(BASE_COLUMN_ORDER.len(), 7);
        assert!(!BASE_COLUMN_ORDER.contains(&COL_SCOPE_VS_HOME));
        assert_eq!(DEFAULT_COLS.as_slice(), &["confidence"]);
        assert_eq!(RECIPE_COLUMNS.len(), 31);
        let col = RECIPE_COLUMNS
            .iter()
            .find(|c| c.id == COL_SCOPE_VS_HOME)
            .expect("catalogued");
        assert_eq!(col.lab, Some(LAB_ANALYZER_RECIPE));
        assert!(!col.default_on);

        // One sort token, lab-only.
        assert_eq!(ALL_SORT_MODES.len(), 25);
        assert_eq!(SortMode::ScopeVsHome.to_string(), COL_SCOPE_VS_HOME);
        assert!(SortMode::ScopeVsHome.lab_only());

        // And nothing was migrated, renamed or removed.
        assert_eq!(
            migrate_legacy_params(&[("sell-scope".into(), "region".into())]),
            None,
            "a Phase F URL is already modern"
        );

        // Global Constraint 6, re-asserted deliberately rather than by
        // accident: Phase F added no viewport-gated fetch.
        let reads = production_source().replace("use_wide_viewport", "");
        assert_eq!(reads.matches("wide_viewport.get()").count(), 2);
    }
```

- [ ] **Step 2: Run it to verify it passes or names the drift**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test::phase_f_adds_exactly`
Expected: PASS if Tasks 1–8 landed as written. Any failure here is a real contract drift — fix the production side, never the assertion.

- [ ] **Step 3: Say what shipped, to players**

At the **top** of `CHANGELOG` in `routes/changelog.rs` (newest first — `entries_are_sorted_newest_first` guards it). The title and the blurb both refuse the travel-to-sell framing: retainers list only on the player's own world, and this control changes which market a *price is read from*.

```rust
    ChangelogEntry {
        date: "2026-09-04",
        title: "Recipe Analyzer: check your sale price against the whole datacenter or region",
        blurb: "With \"Recipe Analyzer: the market model\" on under Settings › Labs, the price the analyzer expects you to sell at can now be read across a whole datacenter or region, next to the scope your ingredients already had. You still sell on your own world — this only changes which market the expected price is read from, so you can see what you are actually competing with. Price, Profit and the alternative revenue columns follow it, and a new Scope vs home column in the Columns picker says how far the wider market sits from your own. Under the cheapest-listing signal it is never above zero, because a bigger market has more sellers undercutting each other; the sale-history signals are where the two can differ in either direction. Sales per day, Confidence, Trend and the rest keep describing your own world, so the numbers you judge speed by never move.",
        link: Some("/recipe-analyzer"),
    },
```

- [ ] **Step 4: Extend the Labs description in all seven locales**

`labs_analyzer_recipe_desc` gains a final sentence (an edit, not a new key). Each locale's existing value ends with the market-columns list; append:

- en: ` Plus a sell-side scope that reads the expected sale price across your datacenter or region — you still sell on your own world.`
- fr: ` S'y ajoute une portée côté vente qui lit le prix de vente attendu sur votre centre serveur ou votre région — vous vendez toujours sur votre propre monde.`
- de: ` Dazu ein Verkaufsbereich, der den erwarteten Verkaufspreis über dein Rechenzentrum oder deine Region abliest — verkauft wird weiterhin auf deiner eigenen Welt.`
- ja: ` さらに、想定売却価格をデータセンターや地域全体で読み取る販売範囲を追加します（販売は自分のワールドのままです）。`
- cn: ` 另外新增销售端范围，可按大区或区域读取预期售价——你仍然只在自己的服务器上出售。`
- ko: ` 여기에 예상 판매가를 데이터 센터나 지역 전체에서 읽는 판매 범위가 더해집니다 — 판매는 여전히 자신의 서버에서 합니다.`
- tc: ` 另外新增銷售端範圍，可依大區或區域讀取預期售價——你仍然只在自己的伺服器上出售。`

Verify all seven are still **1800** keys (this is an edit, not an addition).

- [ ] **Step 5: Cover the column in the e2e route**

`integration/runner.cjs` carries the `analyzer-recipe` route string **twice** — the route-map key at `:94` and the sweep list at `:144` — and they must match exactly or the sweep renders a route with no assertions. Change both to:

```
/recipe-analyzer?world=Gilgamesh&labs=analyzer-recipe&sell-scope=datacenter&cols=confidence,cost-sale-median,rev-sale-median,hop-gain,hop-worlds,profit-per-day,trend,drift,volume-30d,vwap-30d,scope-vs-home
```

`&sell-scope=datacenter` is the point: without it every `scope-vs-home` cell is `ScopeVsHome::Off` and the harness screenshots a column of dashes, which pins nothing the flag-off run does not already pin.

Update the comment above `:94` in the same edit — it carries **three** counts and two of them move:

| `:87-90` today | after |
|---|---|
| "`cols=` names **ten of the twenty-two** optional columns" | "**eleven of the twenty-three**" — `OPTIONAL_COLUMN_ORDER` grew to 23 (the URL-contract table above) and the route now names one more of them |
| "the desktop pass renders **eighteen** columns at once" | "**nineteen**" |
| "only the **six** that are not `hidden md:`" | **unchanged** — `scope-vs-home` uses `HEAD_28_MD` / `CELL_28_MD`, both `hidden md:`, so the mobile pass still renders six |

Leave the rest of the comment (the Trend / Drift "no enrichment locally" paragraph) exactly as it is.

- [ ] **Step 6: Sweep the dead code, then run every gate**

There must be no `#[allow]` anywhere on this branch. Between tasks, `NeededSignals.rev`, `SignalWants.{visible_rev, sort_rev}`, `SellScopeBodies.{listings_failed, stats_failed}` and both `RevenueSource` resolvers had no production reader; by now they all do (Task 7's key, Task 4's `signal_wants`, Task 7's banner, Task 8's resolution). Confirm:

```bash
grep -rn "#\[allow" ultros-frontend/ultros-app/src/analyzer_kit ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
```
Expected: no new hits versus the branch base.

Then, foreground and unpiped:

```bash
cargo test -p ultros-app --lib
cargo check -p ultros-app --no-default-features --features hydrate --target wasm32-unknown-unknown
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```
Expected: all green, `REAL_EXIT=0`, `recipe_analyzer::test` **87**. If clippy is OOM-killed (exit `137`), re-run as `cargo clippy --all-targets -j 2 -- -D warnings` — that is not a lint failure.

- [ ] **Step 7: Commit and open the PR**

```bash
git add ultros-frontend/ultros-app/src/routes/changelog.rs ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs \
        ultros-frontend/ultros-app/locales integration/runner.cjs
git commit -m "docs(analyzer-kit): the phase F changelog entry and the URL contract in one place"
```

Rebase onto `origin/main` once #1265 and #1266 have merged, then open the PR against `main` (a PR whose base is not `main` gets no CI). The PR body must record:

- **This is a reference read.** Retainers are home-world bound; the sell scope changes which market the expected sale price is read from, never where the sale happens. The aria-label, the `world` option label, the column tooltip, the changelog title and its blurb all say so in seven locales, and `Scope vs home` is documented as "never above zero under the cheapest listing".
- **Numbers:** none for any existing URL. Two oracles pin it — `price_rows_matches_recorded_oracle_on_fixture` (profit, ROI, cost, price, tax) and `revenue_projection_is_unchanged_at_the_default_sell_scope` (`rev_alt`, `revenue_fell_back`, `sell_median`, `stat_hq`, recorded in **two** fixture shapes so the "no sell-world listing" parity case the spec asks for is actually covered), the second recorded specifically because the first cannot see the sell-stat lookup.
- **The Price median tell is suppressed, not re-based, at a wider sell scope.** #1266 made that tell trustworthy by comparing like qualities; comparing a region-wide price against one world's median would have made it fire red on nearly every row from the user's own setting. `sell_median` is left empty and the note degrades to its pre-#1266 states.
- **A second URL selection key** (`sell-scope`), which the v1 spec's Decision 1 ruled out. Spec §9 names Phases F and J as the two that spend it; this is F's.
- **Capacity:** a non-default sell scope adds at most one cache key per view — the spec's fourth (§6, "plus sell scope Region with buy DC (F)"). The DC and region 7-day keys already exist as buy-scope keys, so the byte budget is unchanged for anyone who does not opt in.
- **A `hidden md:` `rev-sale-*` column can now pull a region-wide 7-day statistics body on a phone for zero pixels.** This is deliberate and symmetric with the existing `cost_signals` path, which does the same thing for the buy scope — the viewport gate (#1265) covers *lazy* bodies, and both Phase F bodies are `Layer::Bulk` formula bodies that the ledger cannot price without. Said here explicitly rather than left implicit; if it needs gating, it needs gating on both sides at once.
- **Flag-off:** unchanged, no new carve-out; `?sell-scope=region` and `?cols=scope-vs-home` are both inert with the toggle off, pinned by `the_sell_scope_gate_and_its_seating_are_inert_with_the_toggle_off`, `the_two_places_agree_until_the_scope_moves`, `the_sell_scope_is_counted_and_cleared_like_the_other_market_params` and `phase_f_adds_exactly_one_key_and_one_column_token`.
- **The E2 escape cannot recur here.** `with_sell_scope` has one caller, `seat_sell_scope` has three (definition, page memo, table memo), the test harness builds its formula through the same function, and `the_tables_own_formula_is_what_fills_the_scope_column` fails if any of that changes.
- **Spec §10 decision 12:** #1233 can close after this merges, with the remaining ports (G–L) tracked on a new issue. This PR does not close it.

---

## Self-review

**1. Spec coverage.** Walked §1 asks 2 / 3 / 5 / 7 / 19, §2 decisions 4–5, §4's sell-side paragraph, §5's `ScopeVsHome` row and Travel group, §6's fetch rule and capacity line, §8's Phase F sentence, §9's URL and i18n line, §10's decision 1 and 12, §11's "same token".

| Spec requirement | Task |
|---|---|
| `sell-scope` on the recipe analyzer, default world | 1 (the term and its default), 6 (the selector), 7 (the page) |
| stripped from the URL at the default | 6 (`filter(\|s\| *s != SellScope::default())`) |
| counted in active filters | 6 |
| reset by Clear all | 6 |
| pinned in the URL-contract test | 1 (the key), 4 (the token), 9 (both, in one place) |
| a fourth Market select and strip term | 6 |
| revenue over the sell place | 3 (the pass), 8 (the table's resolution) |
| `rev-*` over the sell place | 3 (`rev_alt` via `rev_signal_at`) |
| `scope-vs-home` | 3 (the row state), 4 (kind, cell, sort, token) |
| `SignalView { over: scope, base: buy scope, stats: scope }` | 3 |
| "under World it is today's composition byte for byte, pinned by a parity test that includes items with no sell-world listing" | 3 — `revenue_projection_is_unchanged_at_the_default_sell_scope`, recorded in **two** shapes, the second under `RunOpts { sell_listings: false, .. }` so every output resolves through the `base` layer |
| velocity, avg price, confidence, last sold, volume, VWAP, drift, trend stay on the sell world | 3 (`the_sell_worlds_own_figures_ignore_the_sell_scope`), 5 (`market_extras_put_the_place_they_are_given_on_the_second_line` + `the_two_places_reach_the_labels_they_belong_to`) |
| Hop gain stays buy-side | 3 (asserted equal across scopes) |
| "None without a home value, at most zero under listings" | 3 (`ScopeVsHome::Unavailable`), 4 (the tooltip states the sign rule; the percentage is dropped under a listing signal) |
| `CheapestSellScope` / `SellScopeStats(7)` iff the sell scope is not World, deduped against the buy scope | 2, 7 |
| the fourth cache key | 9 (recorded in the PR body) |
| Numbers: none for any existing URL | 3 (two oracles), 8 (re-run), 9 |
| Changelog | 9 |
| ships under `analyzer-recipe` | 1 (`sell_scope_for` / `seat_sell_scope`), 4 (`lab:`), 6, 7, 8 |
| i18n in seven locales | 4 (2 keys), 5 (1 key), 6 (2 keys), 7 (1 key), 9 (1 edit) — **6 new, 1800 per locale**, which is exactly the spec's §9 "F 6" estimate, so the spec needs no correction |

Two spec sentences are deliberately **not** implemented, each with a stated reason: the Phase 0 comment and Kosyne's question (the user has approved shipping without them), and the declined-fallback "rev-* columns at a fixed region scope without a selector" (only reachable if Phase F were declined).

**2. Placeholder scan.** No "TBD", no "add error handling", no "similar to Task N", no test described without its code. The one intentional blank is `revenue_projection_is_unchanged_at_the_default_sell_scope`'s two `ORACLE` constants, which cannot be written in advance because they are a recording of the current build — Task 3 Steps 1–2 spell out the record-and-paste loop, the exact command, and what to do with the output, which is the same mechanism `price_rows_matches_recorded_oracle_on_fixture` already documents in-tree.

**3. Compile-breaking edits, listed where they happen.** Adding a field to a struct with exhaustive literals is a compile error, and Global Constraint 4 tolerates only warnings between tasks. Task 2 fixes six literals (`needed.rs:189`, `:263`, `:339`; `recipe_analyzer.rs:1636`, `:3962`, `:5142`). Task 3 adds a field to **two** structs and so fixes three literals: `RecipeProfitData`'s one literal outside `price_rows` (`recipe_analyzer.rs:5402`, the `row()` helper) and **both** of `PriceInputs`' — the table's `priced` memo (`:2786`, Step 5) and the harness's `run_with` (`:5098`, Step 7). Missing the first of those pair is a `missing fields` compile error at Step 8, not a warning; the plan's earlier draft patched only the harness. Every other literal of those four structs on the branch already ends in `..Default::default()`.

**4. Type consistency.** Checked every name that crosses a task boundary:
`SellScope` / `Scope` / `with_sell_scope` / `sell_scope()` (Task 1 → 2, 3, 5, 6, 7, 8); `sell_scope_for` and `seat_sell_scope` (1 → 3, 5, 6, 7, 8); `BodyRole::{CheapestSellScope, SellScopeStats}` (2 → 7); `RecipeNeeds.{sell_scope_is_buy_scope, rev_signals}` (2 → 7); `NeededSignals.{rev, scope_vs_home}` and `SignalWants.{visible_rev, sort_rev, scope_vs_home}` (2 → 3, 4, 7); `rev_signal_at` (3, used twice in 3); `PriceInputs.{revenue_listings, revenue_stats}` (3 → 8); `ScopeVsHome` the row enum (3 → 4); `scope_vs_home_delta` / `scope_vs_home_pct` (4, used by the cell and the comparator); `CellValue::SignedGil { delta, pct, unavailable }` (4); `COL_SCOPE_VS_HOME` / `SortMode::ScopeVsHome` (4 → 9); `revenue_place` / `revenue_place_for` (5 → 6, 7); `production_source()` and `production_squeezed()` (5, both reused by 6, 7, 8 and 9); `sell_scope_key` / `SellScopeBodies` / `scope_bodies_failed` / `fetch_sell_scope` (7 → 8); `RevenueSource`, `revenue_source` and its two named wrappers (8, and `run_with` moves onto `revenue_source` in the same task).

One name collision worth stating, because it is a hard compile error rather than a warning: `RevenueSource`'s `Scope` and `BuyScope` variants share their names with the `Scope` alias and the `BuyScope` enum this module imports from `analyzer_kit::formula`, and **enum variants live in the type namespace**, so a `use RevenueSource::*;` in a test makes `Scope::World` fail with `E0433: Scope is a variant, not a module`. Task 8's resolver test imports the two colliding variants under `FromScope` / `FromBuyScope` instead. Renaming the variants was the alternative and was rejected: `RevenueSource::Scope` is the right name at the four match arms that ship.

Return types checked against the code they must slot into: `scope_row` returns `RecipeRow` = `Arc<RecipeProfitData>` (`recipe_analyzer.rs:631`), matching `hop_row` (`:5519`) and `price_row`, because every cell fn takes `&RecipeRow` (`cell_hop_gain`, `:1099`) while `compare_recipes` takes `&RecipeProfitData` (`:2000-2001`) — which `&Arc<T>` deref-coerces into, so one helper serves both. `SortMode::default_dir` reads `RECIPE_BASE.default_dir` through `default_dir_for` (`:1960`), so the new column needs no `default_dir` field and `SortMode` needs no impl change.

**5. Ordering hazards found and fixed during this pass.**
- `sell_scope_for` and `seat_sell_scope` were introduced in Task 6 and Task 7 of the draft, but Task 3's harness must seat the scope the same way production does or the two paths diverge from the first pricing test onward. Both moved to Task 1, three tasks before their first production reader; they are dead code until Task 3, which `cargo test` tolerates and `check_ci.sh` does not see until Task 9.
- The table's revenue resolution was folded into the fetch task, where its divergence from the page's formula was invisible. It is now Task 8, with the call-count pin.
- Task 5 forward-referenced `sell_scope_for` "for Task 7 to replace"; with the helper in Task 1 there is no forward reference left, and `revenue_place` is computed by a pure `revenue_place_for` a unit test can call.

**6. What a second review should attack.** Two review passes have run; (a) and (b) below were their findings and are now closed in-plan, leaving one.

- ~~(a) Nothing pins the *rebuild* — only the resolution — so a `sell_scope` prop read outside the Suspense closure would leave the table stale on a scope change.~~ **Closed:** Task 6's `the_sell_scope_is_counted_and_cleared_like_the_other_market_params` now carries the positive needle `sell_scope=sell_scope_for(preview.get(),sell_scope())` beside the negative `get_untracked` ban. The `sell_scope=` prop prefix is what disambiguates it from the three other identical calls (the `revenue_place` memo, the strip select's `value`, the live sentence's `scoped`).
- ~~(b) `revenue_place`'s "no datacenter resolved yet → region" arm is asserted by construction to match what `sell_scope_key` sends, but no test compares them.~~ **Closed:** Task 7's `the_page_wires_the_sell_scope_to_what_it_fetches` now pins `let place = revenue_place.get();` and `revenue_place.get() == buy_scope_name.get()` (squeezed), so the name fetched, the name deduped against and the name displayed are the same expression.
- (c) Still open, and deliberately: whether the two amber lines stacking is acceptable visually when a page manages to fail both the sale-history body and the sell-scope body at once. Nothing in a unit test can answer it; it is a look-at-it item for the PR's screenshots.

**7. Needle hygiene, learned the hard way in this plan.** Every source-read needle that targets a **multi-argument call** goes through `production_squeezed()` (Task 5), never `production_source()`. rustfmt breaks any call it cannot fit in 100 columns onto one line per argument, so a needle written as one line pins text the formatter will never emit — a test that can only fail. The first review pass shipped three such needles (`preview, sell_scope`, `revenue_listings_source(sell_scope_value`, `revenue_stats_source(sell_scope_value`); all three are now squeezed, as are the ones added since. A needle targeting a single identifier, a key name or a `view!` attribute may stay unsqueezed. And no needle may embed a real `\n` — `production_source()`'s own doc says why: a CRLF checkout makes it miss.

