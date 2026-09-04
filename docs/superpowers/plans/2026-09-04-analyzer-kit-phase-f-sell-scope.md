# Analyzer Kit Phase F: Sell-Side Scope and Scope vs Home — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Under the existing `analyzer-recipe` Labs toggle, the recipe analyzer's revenue side gains a scope of its own — `?sell-scope=world|datacenter|region`, default `world` — so Price, the four `rev-*` columns and Profit can be read across the sell world's datacenter or region instead of only that one world, plus a `scope-vs-home` column saying what the wider scope is worth per unit. With the toggle off, and with the toggle on at the default scope, every URL renders, fetches and computes exactly what it does today.

**Architecture:** The ledger already carries the slot: `ProfitFormula.sell_scope: Term<BuyScope>` has been `Fixed(BuyScope::World)` and unread since Phase A. Phase F seats it with `ProfitFormula::with_sell_scope(SellScope)`, a newtype whose `Default` is `World` (a bare `Scope` would default to the *buy* side's `Datacenter` and silently re-price every existing URL). `needed.rs` grows two roles, `BodyRole::{CheapestSellScope, SellScopeStats(u16)}`, gated on `sell_scope != World` and deduped against the buy side. `price_rows` splits the one "sell" input in two: the **sell place** (`revenue_listings`, `revenue_stats`) feeds `SignalView`'s `over` layer and `rev_alt`; the **sell world** (`sell_listings`, `sell_stats`) keeps feeding velocity, Avg price, Confidence, Last sold, Volume, VWAP, the median tell, `stat_hq`, the sparkline key, the 30-day body and Hop gain's home side, exactly as the spec requires. `ScopeVsHome` is a `Layer::Computed` column over one new row field, `scope_vs_home: Option<(i32, i32)>` — the revenue signal read at the scope and on the sell world's own map — rendered by one new `CellValue::SignedGil`. The UI is one more `<select>` inside the revenue chip of the strip the Market button already opens.

**Tech Stack:** Rust 2024, Leptos 0.8.20 / reactive_graph 0.2.14 / tachys 0.2.18 (SSR + hydrate), leptos_i18n 0.6 (seven locales), the analyzer kit (`ultros-frontend/ultros-app/src/analyzer_kit/`), `ultros-api-types`.

**Specs:** `docs/superpowers/specs/2026-09-01-analyzer-kit-design.md` is binding — §1 asks 2, 3, 5, 7 and 19 (L46–52, L64), §2 decision 4 and 5 (L96–100), §3 module table and core types (L107–224), §4 the sell-side scope paragraph (L243–251), §5 the `ScopeVsHome` catalog row and the Travel picker group (L296–301), §6 the `CheapestSellScope` / `SellScopeStats(7)` fetch rule and the capacity table's fourth cache key (L310–355), §8 Phase F (L436–439) and the variant ledger's "F: the sell-scope roles" (L378), §9 "F adds the key `sell-scope` and the token `scope-vs-home`" (L459–480), §10 decision points 1 and 12 (L484–485, L505), §11 "Phase F's sell scope ships under the same token" (L525–526). Line numbers in the tasks below are against branch `integration-1265-1266` at **`8395bc02`** — `origin/main` (`55fa34d8`, Phase E2 as #1264) plus PR #1265 (the viewport-blind fetch gate) and PR #1266 (the median tell) — and they shift as tasks land. **Search for the quoted code, never trust an offset.**

**Not in this plan.** No comment is posted on #1233, and Kosyne is not asked anything: Aaron has approved Phase F as specified, including shipping it without the third-party reviewer's answer that spec §8 Phase 0 wanted first. The spec's declined-fallback ("rev-* columns at a fixed region scope without a selector", L439) is therefore not built.

## Global Constraints

Every task's requirements implicitly include this section.

1. **Flag-off byte-identity.** With the `analyzer-recipe` Labs toggle off, every URL must render the same DOM, issue the same requests and compute the same numbers. Phase E2 declared four carve-outs; Phase F must add none. Every task that touches markup states how it verified this.
   *(Bookkeeping note for the reviewer: the E2 plan's own Global Constraints record **one** carve-out — the container-mode row-clip fix's `min-w-max` header band and `max-content` row spacer — plus one deliberate difference, the retired `?labs=analyzer-ledger` / `?labs=analyzer-signal-columns` tokens. Whichever count is authoritative, the operative rule for this phase is unchanged: **Phase F adds none.** The specific flag-off hazard Phase F introduces is a URL that carries `?sell-scope=…` or `?cols=scope-vs-home` while the lab is off; Task 7 pins that such a URL is inert down to the "no active filters" hint.)*
2. **A hidden optional child still emits a `<!>` marker in tachys** — dropping a column at build time (the grid's `lab_columns` prop) is the mechanism, not `?cols=` filtering alone. The new `scope-vs-home` column therefore carries `lab: Some(LAB_ANALYZER_RECIPE)` like every other Phase C–E2 column, and `BASE_COLUMN_ORDER` never learns its token.
3. **`#[prop(optional)]` on an `Option<T>` strips the Option** from the builder setter; use `optional_no_strip` when a caller must pass an `Option`. Phase F's new `RecipeAnalyzerTable` props are all required, so none of them uses `optional`.
4. **No `#[allow]`.** Dead code between tasks is expected; it must be gone by the final task. `-D warnings` over `pub(crate)` modules means a field, fn or variant whose only readers are tests fails CI, so the branch-level gate is `./check_ci.sh` in Task 8; each task's own gate is `cargo test -p ultros-app --lib -- <filter>`, which tolerates dead-code warnings.
5. **Every user-facing string via `leptos-i18n` in all seven locales** (`en fr de ja cn ko tc`) with real translations, never English stubs. A key missing from a non-default locale only *warns* and falls back to `en`, so the seven-locale check is a key-count step in the task that adds the key, not a green build.
6. **The viewport gate (#1265) is load-bearing.** Any new lazy fetch must be gated the same way, and any new read of `wide_viewport` must terminate in an `Effect` — a guard test bans call syntax and `.with` on it precisely because `Signal<bool>` is callable and a read inside a `view!` would tear hydration. **Phase F adds no lazy fetch**: both new bodies are `Layer::Bulk` and join the Suspense gate, and `ScopeVsHome` is `Layer::Computed`. `the_page_wires_both_gates_to_what_it_fetches`'s `assert_eq!(reads.matches("wide_viewport.get()").count(), 2)` must therefore still read **2** at the end of this branch; Task 8 re-asserts it deliberately rather than by accident.
7. **Gate commands**, foreground and unpiped, exit read from a variable never a pipe:
   ```bash
   cargo test -p ultros-app --lib
   cargo check -p ultros-app --no-default-features --features hydrate --target wasm32-unknown-unknown
   ./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
   ```
   The default feature is `ssr`, so `--no-default-features` is required for the wasm check; run it with **no `RUSTFLAGS` in the environment** (an env `RUSTFLAGS` replaces `[build] rustflags` and fakes web-sys i32/f64 errors). On Windows, Strawberry Perl must lead `PATH`: `export PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:$PATH"`.
8. **Numbers: none for any existing URL** (spec §8 Phase F, L438). Every URL that does not carry `?sell-scope=datacenter|region` must produce byte-identical numbers, and the recorded oracle `price_rows_matches_recorded_oracle_on_fixture` must not move. **That oracle is not sufficient proof** — it projects six fields (`key_id, profit, roi, cost, market_price, tax`) from a run whose revenue signal is `ListingMin`, so it never exercises the sell-stat lookup, `rev_alt[1..=3]`, `revenue_fell_back` or `sell_median`. Task 3 records a second characterization oracle that observes exactly those values.
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
| What is "a fourth Market select and strip term" (spec L436)? | **The fourth `<select>` reachable from the Market button, which under the lab is the stacked `FormulaStrip` inside that popover** — i.e. one `place_select` on the strip's revenue term, rendered by both the inline row and the popover. It is *not* a fourth `PricingSelect` in `MarketMenu`'s fallback branch: that branch is the flag-off popover, and adding a control there would be a flag-off DOM change (Global Constraint 1). |
| Does the revenue chip keep the place name once it grows a scope select? | **Yes — `place: Some(revenue_place)` *and* `place_select: Some(…)`,** so the chip reads `+ [7d median ▾] · Aether · [Datacenter ▾]`. The cost chip's precedent (`place: None`) would drop "Gilgamesh" from the default lab-on view, and `StripSelect.options` is a plain `Vec<(&'static str, String)>` captured at build time, so putting the resolved place name into the option labels instead would be non-reactive and could stick on "…" forever. The redundancy at `world` scope is accepted deliberately. |
| `SellScope` newtype or a bare `Scope`? | **Newtype.** `Scope::default()` is `Datacenter` (the buy side's default). A bare `sell.unwrap_or_default()`, or the default-stripping setter idiom this page uses everywhere (`parsed.filter(\|s\| *s != Scope::default())`), would move the sell side to the datacenter on every existing URL and strip the wrong token — the single number change this phase must not make. `SellScope::default() == SellScope(Scope::World)` makes both idioms correct by construction. |
| Change `recipe_from_query`'s signature or add a builder? | **Builder.** `ProfitFormula::recipe_from_query` has **33** call sites; a fourth parameter is 33 mechanical edits for no benefit. `with_sell_scope(self, SellScope) -> Self` is called at **3** production sites and 1 test-harness site, and a caller that never calls it keeps `Term::Fixed(BuyScope::World)` — literally today's value, so the flag-off `ProfitFormula` is `PartialEq`-identical to today's and `Memo<ProfitFormula>` cannot fire on it. |
| Which lookups follow the sell scope, and which stay on the sell world? | **Follow the scope:** `market_price` (the `SignalView` `over` layer), all four `rev_alt` entries, `revenue_fell_back`, the Price / Revenue header marks, the picker's "Revenue · ‹place›" heading and the live info sentence's `sell` slot. **Stay on the sell world** (spec L247–248): `daily_sales`, `avg_price`, `total_sales`, `last_sold_unix`, `units_sold`, `vwap`, `vwap_pct`, `confidence`, `stat_hq`, `sell_median` (the Price median tell), the sparkline key, the 30-day body, Hop gain's home run and Worlds to visit. The Daily sales / Confidence / Trend / Drift sub-labels therefore keep saying "7d · ‹sell **world**›" whatever the sell scope is; Task 5 pins that with a test, because the one variable both need is spelled `sell_place` today and getting it wrong is silent. |
| The median tell's basis under a scope | **The sell world's 7-day median, unchanged.** `sell_median` is read off the same `(item, stat_hq)` row every other 7-day figure on the row comes from, and the tell exists to catch a troll listing against what the item actually trades for on the player's own world. A scope-wide median would need the sell-scope statistics body, which is only fetched under a sale revenue signal, so the tell would appear and vanish with an unrelated selection. |
| `scope-vs-home` sign convention and its `None` cases | `delta = revenue signal at the sell scope − the same signal on the sell world's own map`. `None` (the dash) when: the column was not asked for, the sell scope IS the world, the scope has no value, or the **home** has no value (spec L249–250: "None without a home value"). Under `listing-min` the delta is **at most zero** — a region contains the world, so its cheapest listing can only be lower — which is a genuine finding, not a bug: a wider sell scope means more competition. Under a sale statistic it goes either way, and that is the direction that answers "is it worth DC hopping" on the sell side. The tooltip says all of this. |
| `ScopeVsHome`'s cell | A new `CellValue::SignedGil { delta: Option<i32>, pct: Option<f32> }`. `MutedGil` cannot be reused: it filters `amount.filter(\|a\| *a > 0)`, so it renders every negative delta — the common case here — as a dash. The new arm reuses `signed_gil`, `GilIcon` and `signed_delta_class(pct, DELTA_DEAD_BAND_PCT)`, and follows the one-shape rule (the icon hides and the value mutes by class; the arms never swap elements). |
| Where `scope-vs-home` sits in the table and the `?cols=` contract | **Appended after `vwap-30d`, immediately before Actions**, exactly as E2 appended its five — so every serialized old `?cols=` stays byte-identical. Its `PickerGroup` is `Travel`, and `grouped_picker_options` sorts by `(group, table index)`, so it still lists third in Travel behind `hop-gain` and `hop-worlds` despite being last in the table. |
| Which body does the dedupe actually save? | **The cheapest listings body always; the statistics body only when the buy side really fetched one.** `CheapestBuyScope` is unconditional, so a sell scope that resolves to the same place name as the buy scope reuses it outright. `BuyScopeStats(7)` is itself conditional, so `SellScopeStats(7)` is suppressed only when `BuyScopeStats(7)` is *in the computed set* — deduping against a body that was never fetched is how a cell ends up permanently "—". |
| Do the two new bodies join Suspense? | **Yes** (spec L313: "Formula bodies join the Suspense gate"). They price the ledger, so the table cannot render without them. The cost — up to 578 KB on the wire for a region — is opt-in and paid only by a URL that asked for a non-default sell scope. Neither is viewport-gated, so Global Constraint 6's `wide_viewport.get()` count stays at 2. |
| How is flag-off inertness made testable? | One pure helper, `fn sell_scope_for(preview: bool, param: Option<SellScope>) -> Option<SellScope>`, used at all three sites that read the param (the page's formula memo, the page's body key, the table's active-filter list). Its unit test proves the gate; a source-read test in the style of `the_page_wires_both_gates_to_what_it_fetches` proves the page consults it rather than the raw signal. |
| i18n budget | **4 new keys per locale** (1794 → 1798) plus one **edited** existing value (`labs_analyzer_recipe_desc` gains the sell scope). The spec's §9 estimate was "F 6"; two of the six are unnecessary because the sell-scope select's `datacenter` / `region` option labels reuse the existing `datacenter` and `region` keys, exactly as `buy_scope_options` does. |
| Does `?sell-scope` join `ADDABLE_FILTERS`? | **No.** It is a Market control, not a row filter — the same call `cost-basis`, `revenue` and `buy-scope` already make. It is counted in `active_filters` (spec L436: "counted in active filters") and cleared by Clear all, but it never renders a chip and never appears in the `+ Filter` menu, so `ADDABLE_FILTERS` stays at nine. |
| What closes when this merges | Spec §10 decision 12: **#1233 closes after F**, with the remaining ports (G–L) tracked on a new issue. Task 8's PR body says so; it does not close anything by itself. |

## File map

| File | Responsibility in this phase |
|---|---|
| `ultros-frontend/ultros-app/src/analyzer_kit/formula.rs` | `Scope` alias, `SellScope` newtype, `ProfitFormula::{with_sell_scope, sell_scope}` (Task 1). |
| `ultros-frontend/ultros-app/src/analyzer_kit/needed.rs` | `BodyRole::{CheapestSellScope, SellScopeStats}`, `RecipeNeeds::{sell_scope_is_buy_scope, rev_signals}`, `SignalWants::{visible_rev, sort_rev, scope_vs_home}`, `NeededSignals::{rev, scope_vs_home}`, the two new `needed_bodies` rules (Task 2). |
| `ultros-frontend/ultros-app/src/analyzer_kit/cells.rs` | `CellValue::SignedGil` + its render arm + its shape test (Task 4). |
| `ultros-frontend/ultros-app/src/analyzer_kit/columns.rs` | `ColumnKind::ScopeVsHome`; the doc line on `PickerContext.sell_place` (Task 4, Task 5). |
| `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs` | `FILTER_SELL_SCOPE` + its contract pin (Task 1); `rev_signal_at`, the `PriceInputs` sell-place / sell-world split, `scope_vs_home`, the discriminating fixture and the revenue oracle (Task 3); `COL_SCOPE_VS_HOME`, `SPEC_SCOPE_VS_HOME`, `label_scope_vs_home`, `cell_scope_vs_home`, `SortMode::ScopeVsHome`, the comparator, the 31st table row, the URL and sort contracts (Task 4); `revenue_place`, the marks / picker / info-sentence split (Task 5); `sell_scope_options`, the strip's `place_select`, `active_filters`, `clear_all` (Task 6); `SellScopeBodies`, `fetch_sell_scope`, the resource, the Suspense join, `sell_scope_for` and the flag-off pins (Task 7). |
| `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json` | 4 new keys and 1 edited value, per locale, added in the task that first reads them (Tasks 4 and 6). |
| `ultros-frontend/ultros-app/src/routes/changelog.rs` | The player-facing entry, dated `2026-09-04` (Task 8). |
| `integration/runner.cjs` | The `analyzer-recipe` route gains `scope-vs-home` in its `?cols=` list (Task 8). |
| `docs/superpowers/specs/2026-09-01-analyzer-kit-design.md` | The §9 "F 6 keys" figure corrected to 4 (Task 8, docs-only). |

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

## Test counts at the branch base

For the "Expected: N passed" lines below. Re-count with `grep -c '#\[test\]'` before trusting any of them if the base has moved:

`routes::recipe_analyzer` 47, and across the kit: `cells` 4, `columns` 5, `enrichment` 12, `formula` 10, `grid` 6, `hop` 4, `needed` 9, `signals` 5, `strip` 1.

---

### Task 1: The sell-scope term, defaulting to the world and not to the buy side's datacenter

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/formula.rs:88-120` (add the `Scope` alias beside `BuyScope`), `:186-200` (`ProfitFormula.sell_scope`'s doc comment), `:204-235` (add the two methods), and its `mod tests`
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:519-537` (the filter-key block) and `:4488-4496` (the contract test)

**Interfaces:**
- Consumes: `BuyScope` (`formula.rs:94`), `Term<T>` (`formula.rs:123`), `ProfitFormula` (`formula.rs:188`) — all unchanged in shape.
- Produces, for every later task:
  - `pub type Scope = BuyScope;` — the spec's name for the enum when it is not the buy side's.
  - `pub struct SellScope(pub Scope);` with `Default = SellScope(Scope::World)`, `FromStr` / `Display` delegating to `Scope` (tokens `world` / `datacenter` / `region`), `Copy + Clone + Debug + PartialEq + Eq + Hash`, and `pub fn scope(self) -> Scope`.
  - `pub fn ProfitFormula::with_sell_scope(self, sell: SellScope) -> Self` — sets `sell_scope: Term::Select(sell.scope())`, returns `self`.
  - `pub fn ProfitFormula::sell_scope(&self) -> Scope` — `self.sell_scope.value()`.
  - `const FILTER_SELL_SCOPE: &str = "sell-scope";` in `recipe_analyzer.rs`, read by Tasks 6 and 7.

- [ ] **Step 1: Write the failing test**

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

/// Where the *product* is sold — [`ProfitFormula::sell_scope`]'s URL value
/// under `?sell-scope=`.
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
    /// Where the product is sold. `Fixed(Scope::World)` — today's and every
    /// pre-Phase-F URL's value — until [`ProfitFormula::with_sell_scope`]
    /// seats it, which only the recipe analyzer does and only under the
    /// `analyzer-recipe` lab.
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
Expected: PASS, 13 passed (10 at the base + 3).

- [ ] **Step 5: Write the failing URL-key test**

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

- [ ] **Step 6: Run it to verify it fails**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test::filter_registry`
Expected: FAIL — `cannot find value FILTER_SELL_SCOPE in this scope`.

- [ ] **Step 7: Add the constant**

In `recipe_analyzer.rs`, after `const FILTER_BUY_SCOPE: &str = "buy-scope";` (`:525`):

```rust
/// Phase F: where the product is sold. Default `world`, stripped from the
/// URL at the default, read only under the `analyzer-recipe` lab.
const FILTER_SELL_SCOPE: &str = "sell-scope";
```

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test::filter_registry`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add ultros-frontend/ultros-app/src/analyzer_kit/formula.rs ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "feat(analyzer-kit): the sell-side scope term, defaulting to the world"
```

---

### Task 2: The two sell-scope bodies, and the dedupe against the buy side

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/needed.rs:50-60` (`BodyRole`), `:61-79` (`RecipeNeeds`), `:81-105` (`needed_bodies`), `:105-125` (`SignalWants`, `NeededSignals`), `:133-190` (`needed_signals`), and its `mod tests`

**Interfaces:**
- Consumes: `ProfitFormula::{sell_scope, revenue_signal, cost_signal, buy_scope}`, `Scope`, `SellScope` (Task 1); `SALE_STATS_WINDOW_DAYS` (`needed.rs:12`).
- Produces:
  - `BodyRole::CheapestSellScope` and `BodyRole::SellScopeStats(u16)` — declared **after** `CheapestSellWorld` and after `SellWorldStats` respectively in the enum, because `BodyRole` derives `Ord` and `needed_bodies` returns a `BTreeSet` whose iteration order the existing "today's three bodies" test asserts as a `Vec`.
  - `RecipeNeeds.sell_scope_is_buy_scope: bool` and `RecipeNeeds.rev_signals: BTreeSet<PriceSignal>`.
  - `SignalWants.{visible_rev: Vec<PriceSignal>, sort_rev: Option<PriceSignal>, scope_vs_home: bool}`.
  - `NeededSignals.{rev: BTreeSet<PriceSignal>, scope_vs_home: bool}` — `rev` is `{selected revenue} ∪ visible_rev ∪ sort_rev`, uncapped (revenue alternatives are array reads, not `compute_cost` runs).
  - Read by Task 3 (`scope_vs_home`), Task 4 (`signal_wants`) and Task 7 (the resource key).

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
        // confidence, last sold, volume, VWAP and the median tell all read it.
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

In `needed.rs`, replace the `BodyRole` enum (`:50-60`) with:

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

Add two fields to `RecipeNeeds` (`:61-79`), after `cost_signals`:

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
        // in the set: the buy-scope one is itself conditional, and reusing
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

Update the existing `needs(outliers, same)` helper in `mod tests` to fill the two new `RecipeNeeds` fields with `false` / `BTreeSet::new()`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p ultros-app --lib -- analyzer_kit::needed`
Expected: PASS, 14 passed (9 at the base + 5). `needed_bodies_default_is_todays_three_bodies` must still pass **unchanged** — if it fails, a new `BodyRole` variant was declared too early in the enum and moved the `BTreeSet` order.

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/analyzer_kit/needed.rs
git commit -m "feat(analyzer-kit): the sell-scope bodies, deduped against the buy side"
```

---

### Task 3: Revenue prices at the sell place, everything else at the sell world

This is the task that can silently change numbers, so it opens with two characterization steps and its fixture is built to make a wrong lookup impossible to miss.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:96-165` (`RecipeProfitData` gains one field), `:2061-2104` (`PriceInputs`), `:2105-2360` (`price_rows`), and `mod test`'s fixture harness at `:4920-5152` and its oracle at `:5361-5395`

**Interfaces:**
- Consumes: `SignalView` (`signals.rs:130`), `stat_only_cheapest` (`signals.rs:95`), `NeededSignals.scope_vs_home` (Task 2), `Scope` (Task 1).
- Produces:
  - `fn rev_signal_at(listings: Option<&CheapestListingsMap>, stats: Option<&StatsIndex>, item: i32, signal: PriceSignal) -> Option<i32>` — the bare number for one revenue signal at one place, no cross-fallback. Both `rev_alt` (the sell place) and Scope vs home's home side (the sell world) read it.
  - `PriceInputs.revenue_listings: Option<&CheapestListingsMap>` and `PriceInputs.revenue_stats: Option<&StatsIndex>` — the **sell place**. Existing `sell_listings` and `sell_stats` keep their names and now mean the **sell world** only.
  - `PriceInputs.revenue_stats_loaded: bool` — replaces the meaning of `sell_stats_loaded` for `ProfitFormula::effective`'s second argument. `sell_stats_loaded` stays, and keeps meaning "the sell **world's** body arrived", which is what Hop gain's home run reads.
  - `RecipeProfitData.scope_vs_home: Option<(i32, i32)>` — `(the signal at the sell place, the same signal on the sell world)`. Read by Task 4's cell and comparator.
  - `RunOpts.{sell_scope: Option<Scope>, scope_bodies: bool}` in the test harness, and `fn scope_fixture(...)`.

- [ ] **Step 1: Record the revenue characterization oracle against the UNCHANGED code**

This step's test is expected to **pass** immediately: it is a characterization test recorded before the refactor, which is the only way to prove afterwards that the sale-side revenue numbers did not move. `price_rows_matches_recorded_oracle_on_fixture` cannot do that job — it projects `key_id, profit, roi, cost, market_price, tax` from a run whose revenue signal is `ListingMin`, so it never touches `stat_only_cheapest`, `rev_alt[1..=3]`, `revenue_fell_back` or `sell_median`.

Add to `recipe_analyzer.rs`'s `mod test`, beside the existing oracle:

```rust
    /// The revenue-side characterization oracle: everything the sell-stat
    /// lookup produces, for a run whose revenue signal IS a statistic.
    /// Recorded on `8395bc02` before Phase F split the sell place from the
    /// sell world; regenerate ONLY if a phase moves these numbers on
    /// purpose (run with `--nocapture` and copy the printed tuples).
    #[test]
    fn revenue_projection_is_unchanged_at_the_default_sell_scope() {
        let rows = run(PriceSignal::ListingMin, PriceSignal::SaleMedian, false);
        let got: Vec<(i32, i32, [Option<i32>; 4], bool, Option<i32>, bool)> = rows
            .iter()
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
            .collect();
        println!("REVENUE_ORACLE = {got:?}");
        const ORACLE: &[(i32, i32, [Option<i32>; 4], bool, Option<i32>, bool)] = &[
            // PASTE the printed tuples here, verbatim, from the run in Step 2.
        ];
        assert_eq!(got.as_slice(), ORACLE);
    }
```

- [ ] **Step 2: Run it, paste the recording, run it again**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test::revenue_projection -- --nocapture`
Expected: FAIL with a left/right mismatch against the empty `ORACLE`, and a `REVENUE_ORACLE = [...]` line above it. Paste those tuples into `ORACLE`, re-run, and expect PASS. Commit this on its own so the recording is separable from the change it guards:

```bash
git add ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "test(recipe-analyzer): record what the revenue side computes today"
```

- [ ] **Step 3: Write the failing discriminating-fixture test**

The fixture must vary the discriminator in **both** directions, or it proves nothing: E2's median-tell defect shipped past a green suite because every fixture gave an item exactly one quality of statistics, so a lookup that read the wrong one returned the right answer. Here the discriminator is *which map* a lookup reads, so the sell-scope map differs from the sell-world map downward on some items, **upward** on others (deliberately unrealistic — it is what catches an implementation that quietly takes `min(scope, world)`), and is absent for a third class.

Add to `mod test`:

```rust
    /// The sell-scope fixture. Derived from the sell-world map so a test can
    /// state the expected number in terms of the home one:
    ///   * even output ids  -> the scope is HALF the home price (a wider
    ///     scope undercuts: the realistic direction),
    ///   * odd output ids   -> the scope is DOUBLE it (impossible in
    ///     production, and exactly why it is here: a lookup that read the
    ///     home map, or took `min(scope, home)`, would still pass on the
    ///     even half alone),
    ///   * every third recipe -> absent from the scope map entirely, so the
    ///     `SignalView` `over` layer falls through to the buy-scope `base`.
    /// Statistics move the same three ways on the median.
    fn scope_fixture(
        recipes: &[&'static Recipe],
        sell: &CheapestListingsMap,
        sell_stats: &StatsIndex,
    ) -> (CheapestListingsMap, StatsIndex) {
        let mut listings = Vec::new();
        let mut stats = StatsIndex::new();
        for (i, r) in recipes.iter().enumerate() {
            if i % 3 == 2 {
                continue; // absent from the scope entirely
            }
            let out = r.item_result;
            let scale = |p: i32| if out % 2 == 0 { p / 2 } else { p * 2 };
            if let Some(home) = sell.find_matching_listings(out).lq {
                listings.push(CheapestListingItem {
                    item_id: out,
                    hq: false,
                    cheapest_price: scale(home.price),
                    world_id: 9,
                });
            }
            if let Some(row) = sell_stats.get(&(out, false)) {
                stats.insert(
                    (out, false),
                    ItemSaleStats {
                        min_price: scale(row.min_price),
                        median_price: scale(row.median_price),
                        avg_price: scale(row.avg_price),
                        ..*row
                    },
                );
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
    /// row actually discriminates: a row whose scope price equals its home
    /// price would prove nothing, so the classes are counted and all three
    /// must be non-empty AFTER the drop rule has run.
    #[test]
    fn revenue_reads_the_sell_scope_and_every_class_of_row_says_so() {
        for signal in [PriceSignal::ListingMin, PriceSignal::SaleMedian] {
            let home = run_with(
                PriceSignal::ListingMin,
                signal,
                &RunOpts {
                    sell_scope: None,
                    ..RunOpts::default()
                },
            );
            let scoped = run_with(
                PriceSignal::ListingMin,
                signal,
                &RunOpts {
                    sell_scope: Some(Scope::Region),
                    scope_bodies: true,
                    ..RunOpts::default()
                },
            );
            let home_by_key: HashMap<i32, &RecipeProfitData> =
                home.iter().map(|r| (r.recipe.key_id.0, r)).collect();

            let (mut cheaper, mut dearer, mut fell_through) = (0, 0, 0);
            for r in &scoped {
                let Some(h) = home_by_key.get(&r.recipe.key_id.0) else {
                    continue;
                };
                match r.market_price.cmp(&h.market_price) {
                    Ordering::Less => {
                        cheaper += 1;
                        assert_ne!(r.market_price, h.market_price);
                    }
                    Ordering::Greater => {
                        dearer += 1;
                        assert_ne!(r.market_price, h.market_price);
                    }
                    Ordering::Equal => fell_through += 1,
                }
            }
            assert!(
                cheaper > 0 && dearer > 0,
                "{signal:?}: the fixture must move prices BOTH ways \
                 (cheaper {cheaper}, dearer {dearer}); a one-directional \
                 fixture cannot tell a scope lookup from a clamp"
            );
            assert!(
                fell_through > 0,
                "{signal:?}: no row fell through to the buy-scope layer"
            );
        }
    }

    /// The sell world's own figures do NOT follow the sell scope: velocity,
    /// avg price, confidence, last sold, volume, VWAP, the median tell, the
    /// statistics quality (the sparkline and 30-day key) and Hop gain's home
    /// run all stay where the spec puts them.
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
            assert_eq!(r.sell_median, h.sell_median, "the median tell is the WORLD's");
            assert_eq!(r.hop, h.hop, "Hop gain is buy-side and prices home at the world");
            assert_eq!(r.worlds, h.worlds);
        }
        assert!(compared > 20, "only {compared} rows compared");
    }

    /// Scope vs home: both places under one signal, both directions of sign,
    /// and every `None` case the spec names.
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
        assert!(quiet.iter().all(|r| r.scope_vs_home.is_none()));

        // Asked for, but the sell scope IS the world: nothing to compare.
        let flat = run_with(
            PriceSignal::ListingMin,
            PriceSignal::ListingMin,
            &RunOpts {
                needs: wanted.clone(),
                ..RunOpts::default()
            },
        );
        assert!(flat.iter().all(|r| r.scope_vs_home.is_none()));

        // Asked for at a wider scope: both directions appear.
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
        let deltas: Vec<i32> = scoped
            .iter()
            .filter_map(|r| r.scope_vs_home.map(|(place, home)| place - home))
            .collect();
        assert!(!deltas.is_empty());
        assert!(deltas.iter().any(|d| *d < 0), "no row where the scope undercuts");
        assert!(deltas.iter().any(|d| *d > 0), "no row where the scope is dearer");
        // Every recorded pair has a real home value (the spec's "None
        // without a home value").
        assert!(scoped.iter().all(|r| r.scope_vs_home.is_none_or(|(_, h)| h > 0)));
    }
```

- [ ] **Step 4: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test::revenue_reads recipe_analyzer::test::the_sell_worlds recipe_analyzer::test::scope_vs_home`
Expected: FAIL — `no field sell_scope on RunOpts`, `no field scope_vs_home on RecipeProfitData`.

- [ ] **Step 5: Split the sell place from the sell world in `PriceInputs`**

Replace the four sell-side fields of `PriceInputs` (`:2068-2082`) with:

```rust
    /// Sell-**world** listings (absent before a world resolves). Hop gain's
    /// home run prices against these, and only these.
    sell_listings: Option<&'a CheapestListingsMap>,
    /// Buy-scope sale stats, indexed. `None` when not fetched.
    buy_stats: Option<&'a StatsIndex>,
    /// Sell-**world** sale stats, indexed. Empty when not fetched. Velocity,
    /// avg price, confidence, last sold, volume, VWAP, the median tell and
    /// the statistics quality every lazy column keys on all read this, at
    /// every sell scope (spec §4).
    sell_stats: &'a StatsIndex,
    /// Sell-**place** listings: the sell world's map under the default sell
    /// scope, the scope's own map otherwise. The `SignalView` `over` layer
    /// revenue is priced from.
    revenue_listings: Option<&'a CheapestListingsMap>,
    /// Sell-**place** sale stats. `Some(sell_stats)` under the default sell
    /// scope; `None` when a wider scope's body was not fetched, which makes
    /// every `rev-sale-*` cell "—" rather than a sell-world number under a
    /// scope heading.
    revenue_stats: Option<&'a StatsIndex>,
```

and, beside the existing `sell_stats_loaded` (`:2098-2100`), add:

```rust
    /// Whether the sell-**place** statistics body arrived. This is the
    /// second argument the caller passed to `ProfitFormula::effective`, so
    /// a sale revenue signal with no body is already downgraded here; it is
    /// carried for the `rev_alt` guard, which must not invent numbers.
    revenue_stats_loaded: bool,
```

- [ ] **Step 6: Add the one lookup both places share, and use it**

Above `price_rows` (`:2104`):

```rust
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

and replace the `rev_alt` literal (`:2314-2322`) with:

```rust
        // The bare sell-PLACE number per revenue signal, no fallback.
        let item = recipe.item_result;
        let rev_alt = [
            rev_signal_at(inp.revenue_listings, inp.revenue_stats, item, PriceSignal::ListingMin),
            rev_signal_at(inp.revenue_listings, inp.revenue_stats, item, PriceSignal::SaleMin),
            rev_signal_at(inp.revenue_listings, inp.revenue_stats, item, PriceSignal::SaleMedian),
            rev_signal_at(inp.revenue_listings, inp.revenue_stats, item, PriceSignal::SaleAvg),
        ];
        let revenue_fell_back = rev_alt[inp.formula.revenue_signal().index()] != Some(market_price);

        // Scope vs home: the selected revenue signal at the sell place and
        // on the sell world's own map. `None` unless asked for, at the
        // default sell scope, or without a value on either side.
        let scope_vs_home = (inp.needs.scope_vs_home && !sell_scope_is_world)
            .then(|| {
                let signal = inp.formula.revenue_signal();
                let place = rev_alt[signal.index()]?;
                let home = rev_signal_at(inp.sell_listings, Some(inp.sell_stats), item, signal)?;
                Some((place, home))
            })
            .flatten();
```

Add `scope_vs_home,` to the `RecipeProfitData` literal, and to the struct (`:96-165`), after `worlds`:

```rust
    /// `(the selected revenue signal at the sell place, the same signal on
    /// the sell world's own map)`. `None` when Scope vs home was not asked
    /// for, when the sell scope IS the sell world, or when either place has
    /// no value. The column renders `place − home`.
    scope_vs_home: Option<(i32, i32)>,
```

Finally, delete the now-unused `let item = recipe.item_result;` at `:2313` if it is duplicated, and leave `sell_stats_loaded`'s use in `hop_signal` (`:2146-2150`) **exactly as it is** — that is the sell world's body, and Hop gain must keep reading it.

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

with `None` / `false` in `Default`, and in `run_with`:

```rust
        let (scope_listings, scope_stats) = scope_fixture(&recipes, &sell, &sell_index);
        let use_scope = o.sell_scope.is_some_and(|s| s != Scope::World) && o.scope_bodies;
        let formula = ProfitFormula::recipe_from_query(Some(cost), Some(revenue), o.scope);
        let formula = match o.sell_scope {
            Some(s) => formula.with_sell_scope(SellScope(s)),
            None => formula,
        };
```

then in the `PriceInputs` literal:

```rust
            revenue_listings: if use_scope {
                Some(&scope_listings)
            } else if o.sell_scope.is_some_and(|s| s != Scope::World) {
                None
            } else {
                o.sell_listings.then_some(&sell)
            },
            revenue_stats: if use_scope {
                Some(&scope_stats)
            } else if o.sell_scope.is_some_and(|s| s != Scope::World) {
                None
            } else {
                o.sell_stats.then_some(&sell_index)
            },
            revenue_stats_loaded: if o.sell_scope.is_some_and(|s| s != Scope::World) {
                use_scope
            } else {
                o.sell_stats
            },
            formula,
```

Add `use std::cmp::Ordering;` to `mod test`'s imports if it is not already there, and `Scope, SellScope` to the `analyzer_kit::formula` import at the top of the file.

- [ ] **Step 8: Run every pricing test**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test`
Expected: PASS. Specifically, all four of these must be green in the same run:
- `price_rows_matches_recorded_oracle_on_fixture` (unchanged numbers),
- `revenue_projection_is_unchanged_at_the_default_sell_scope` (unchanged revenue numbers — the recording from Step 2),
- `revenue_reads_the_sell_scope_and_every_class_of_row_says_so`,
- `the_sell_worlds_own_figures_ignore_the_sell_scope`.

If either oracle moved, **stop**: something that should have stayed on the sell world followed the scope.

- [ ] **Step 9: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "feat(recipe-analyzer): price revenue at the sell place, keep the rest on the sell world"
```

---

### Task 4: The `scope-vs-home` column — kind, cell, sort mode and URL token

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/columns.rs:29-66` (`ColumnKind`)
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/cells.rs:50-99` (`CellValue`), `:388-423` (the render arms), and its `mod tests`
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:561-590` (`COL_*`), `:787-812` (labels), `:930-965` (specs), `:1099-1113` (cells), `:1240` + the row before Actions (the table), `:1623-1642` (`signal_wants`), `:1888-1932` (`SortMode`, `lab_only`), `:1965-1996` (the sort key), `:1997-2060` (`compare_recipes`), and the URL / sort / picker tests
- Modify: `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json` (2 keys each)

**Interfaces:**
- Consumes: `RecipeProfitData.scope_vs_home` (Task 3), `NeededSignals.scope_vs_home` and `SignalWants.{visible_rev, sort_rev, scope_vs_home}` (Task 2), `delta_pct` (`recipe_analyzer.rs:1051`), `signed_gil` (`cells.rs:136`), `signed_delta_class` / `DELTA_DEAD_BAND_PCT` (`analysis.rs:380-392`), `cmp_none_last` (`sort_header.rs:168`), `LAB_ANALYZER_RECIPE`.
- Produces:
  - `ColumnKind::ScopeVsHome`.
  - `CellValue::SignedGil { delta: Option<i32>, pct: Option<f32> }` and its render arm.
  - `const COL_SCOPE_VS_HOME: &str = "scope-vs-home";`, `static SPEC_SCOPE_VS_HOME: ColumnSpec`, `fn label_scope_vs_home`, `fn cell_scope_vs_home`, `fn scope_vs_home_delta(&RecipeProfitData) -> Option<i32>`, `SortMode::ScopeVsHome`, and the 31st entry of `RECIPE_COLUMNS`.
  - i18n keys `analyzer_col_scope_vs_home` and `analyzer_scope_vs_home_help`, read here and by Task 5's header extras.

- [ ] **Step 1: Write the failing cell-shape test**

Append to `cells.rs`'s `mod tests`:

```rust
    /// A signed delta keeps one shape across "there is a number" and "there
    /// is not": the gil icon hides by class and the value mutes by class,
    /// and the sub-line element is always present. A negative delta is the
    /// COMMON case for Scope vs home under the cheapest listing, so this
    /// asserts the number survives — `MutedGil`'s `amount > 0` filter would
    /// have swallowed it.
    #[test]
    fn signed_gil_cells_keep_one_shape_and_render_negatives() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let down = render(CellValue::SignedGil {
                delta: Some(-1_250),
                pct: Some(-8.0),
            });
            let up = render(CellValue::SignedGil {
                delta: Some(430),
                pct: Some(3.0),
            });
            let none = render(CellValue::SignedGil {
                delta: None,
                pct: None,
            });
            assert!(down.contains("-1,250"), "{down}");
            assert!(down.contains("text-red-300"), "{down}");
            assert!(down.contains("-8%"), "{down}");
            assert!(up.contains("+430"), "{up}");
            assert!(up.contains("text-emerald-300"), "{up}");
            assert!(none.contains("—"), "{none}");
            for html in [&down, &up, &none] {
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
    /// state (a wider sell scope can only undercut under the cheapest
    /// listing). `None` renders the dash.
    SignedGil {
        delta: Option<i32>,
        pct: Option<f32>,
    },
```

and, before the `CellValue::Custom` arm (`:424`):

```rust
        CellValue::SignedGil { delta, pct } => {
            let has = delta.is_some();
            let text = delta.map(signed_gil).unwrap_or_else(|| "—".to_string());
            let sub = pct.map(|p| format!("{p:+.0}%")).unwrap_or_default();
            let value_class = if has {
                signed_delta_class(pct, DELTA_DEAD_BAND_PCT)
            } else {
                "text-[color:var(--color-text-muted)]"
            };
            // One shape (the `GilOrDash` rule): the icon hides and the value
            // mutes by class; the arms never swap elements.
            view! {
                <div role="cell" class=class>
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
Expected: PASS, 5 passed.

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

7. Add a new test:

```rust
    /// Scope vs home renders the delta, its percent against the home value,
    /// and nothing at all when there is no pair. The sort key is the same
    /// delta, and it sorts none-last in both directions like every other
    /// optional-value column on this page.
    #[test]
    fn scope_vs_home_cell_and_sort_read_the_same_delta() {
        let ctx = test_ctx();
        let cheaper = scope_row(1, Some((900, 1_000)));
        let dearer = scope_row(2, Some((1_100, 1_000)));
        let missing = scope_row(3, None);
        assert_eq!(
            cell_scope_vs_home(&cheaper, &ctx),
            CellValue::SignedGil {
                delta: Some(-100),
                pct: Some(-10.0)
            }
        );
        assert_eq!(
            cell_scope_vs_home(&dearer, &ctx),
            CellValue::SignedGil {
                delta: Some(100),
                pct: Some(10.0)
            }
        );
        assert_eq!(
            cell_scope_vs_home(&missing, &ctx),
            CellValue::SignedGil {
                delta: None,
                pct: None
            }
        );
        assert_eq!(scope_vs_home_delta(&cheaper), Some(-100));
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
```

with the helper, beside the existing `hop_row` (`:5519`):

```rust
    fn scope_row(key: i32, pair: Option<(i32, i32)>) -> RecipeProfitData {
        let mut r = (*hop_row(key, None, None)).clone();
        r.scope_vs_home = pair;
        r
    }
```

- [ ] **Step 6: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test`
Expected: FAIL — `no variant named ScopeVsHome`, `cannot find function cell_scope_vs_home`, and the three count assertions.

- [ ] **Step 7: Add the kind, the token, the label, the cell and the sort mode**

`columns.rs`, in `ColumnKind` after `HopWorlds` (`:63`):

```rust
    /// The revenue signal at the sell scope minus the same signal on the
    /// sell world's own map: the sell-side counterpart of Hop gain.
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
    r.scope_vs_home.map(|(place, home)| place - home)
}

fn cell_scope_vs_home(r: &RecipeRow, _: &CellCtx) -> CellValue {
    CellValue::SignedGil {
        delta: scope_vs_home_delta(r),
        // Against the HOME value: "the wider scope is 10% below your world".
        pct: r.scope_vs_home.and_then(|(place, home)| delta_pct(Some(place), home)),
    }
}
```

`SortMode`, after `Vwap30` (`:1888`):

```rust
    /// The sell-scope revenue signal minus the sell world's own.
    ScopeVsHome,
```

and add `| SortMode::ScopeVsHome` to `lab_only`'s `matches!` list.

`compare_recipes`, after the `Vwap30` arm:

```rust
        SortMode::ScopeVsHome => cmp_none_last(
            scope_vs_home_delta(a),
            scope_vs_home_delta(b),
            dir,
            i32::cmp,
        ),
```

The table: change `static RECIPE_COLUMNS: [ToolColumnMeta<RecipeRow, SortMode>; 30]` to `; 31`, and insert **immediately before** the `SPEC_ACTIONS` entry:

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

`signal_wants` (`:1623`) gains the three new wants:

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

`analyzer_scope_vs_home_help` (the header tooltip, Task 5 renders it):

- en: `The revenue signal read across the sell scope, minus the same signal on your sell world. Negative means the wider scope prices lower — more sellers to undercut — so under the cheapest listing it is never above zero; a sale statistic can go either way. Blank when the sell scope is the sell world.`
- fr: `Le signal de revenu lu sur la portée de vente, moins le même signal sur votre monde de vente. Négatif signifie que la portée plus large affiche un prix plus bas — plus de vendeurs à sous-coter — donc, avec l'annonce la moins chère, jamais au-dessus de zéro ; une statistique de ventes peut aller dans les deux sens. Vide lorsque la portée de vente est le monde de vente.`
- de: `Das Erlössignal über den Verkaufsbereich, minus dasselbe Signal auf deiner Verkaufswelt. Negativ heißt, der weitere Bereich ist günstiger — mehr Verkäufer, die man unterbieten muss — beim günstigsten Angebot also nie über null; eine Verkaufsstatistik kann in beide Richtungen gehen. Leer, wenn der Verkaufsbereich die Verkaufswelt ist.`
- ja: `販売範囲で読んだ収益シグナルから、販売ワールドでの同じシグナルを引いた値です。マイナスは範囲が広いほど価格が低い（競合する出品が多い）ことを意味し、最安出品ではプラスになりません。売上統計ではどちらにも振れます。販売範囲が販売ワールドと同じ場合は空欄です。`
- cn: `在销售范围内读取的收益信号，减去销售服务器上的同一信号。负值表示范围越大价格越低（要压价的卖家更多），因此按最低寄售价永远不会为正；按成交统计则可高可低。销售范围就是销售服务器时留空。`
- ko: `판매 범위에서 읽은 수익 신호에서 판매 서버의 같은 신호를 뺀 값입니다. 음수는 범위가 넓을수록 가격이 낮다는 뜻이며(가격을 낮춰야 할 판매자가 더 많음), 최저 판매 등록가 기준으로는 0을 넘지 않습니다. 판매 통계 기준으로는 양방향 모두 가능합니다. 판매 범위가 판매 서버와 같으면 비어 있습니다.`
- tc: `在銷售範圍讀取的收益訊號，減去銷售伺服器上的同一訊號。負值表示範圍越大價格越低（要壓價的賣家更多），因此以最低寄售價計算永遠不會為正；以成交統計計算則可能為正或負。銷售範圍即銷售伺服器時留空。`

Verify every file grew by exactly two:

```bash
for l in en fr de ja cn ko tc; do
  printf '%s ' "$l"
  python -c "import json,sys; print(len(json.load(open(sys.argv[1],encoding='utf-8'))))" \
    ultros-frontend/ultros-app/locales/$l.json
done
```
Expected: `1796` for all seven.

- [ ] **Step 9: Run the tests to verify they pass**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test analyzer_kit`
Expected: PASS. The three contract counts now read 23 / 25 / 14, and `every_recipe_sort_mode_is_catalogued_exactly_once` covers the new mode without edits (it iterates `ALL_SORT_MODES`).

- [ ] **Step 10: Commit**

```bash
git add ultros-frontend/ultros-app/src/analyzer_kit ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs ultros-frontend/ultros-app/locales
git commit -m "feat(recipe-analyzer): Scope vs home as a sortable column"
```

---

### Task 5: The sell place and the sell world are two different labels

Everything that names "where revenue comes from" must follow the scope; everything that names "where the 7-day figures come from" must not. Both are spelled `sell_place` today, which is exactly why this is its own task with its own test.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/columns.rs:222-231` (doc only, `PickerContext.sell_place`)
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/formula.rs:262-278` (doc only, `FormulaMarks.sell_place`)
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:2490-2500` (a new table prop), `:2665-2755` (`marks`, `header_extras`), `:2995-3013` (`column_options`), `:3855-3875` (the page's memos), `:4220-4235` (the live info sentence), `:4355-4375` (the table call)

**Interfaces:**
- Consumes: `sell_scope_for` — **not yet written**; Task 5 introduces the *place* memo only and reads the sell-scope query param directly through a `Memo` the page already needs. To keep the tasks independently reviewable, Task 5 computes `revenue_place` from a `Signal<Option<SellScope>>` parameter it is handed, and Task 7 replaces that parameter's source with `sell_scope_for`. Concretely, Task 5 adds:
  ```rust
  let (sell_scope, _) = filter_query_signal::<SellScope>(FILTER_SELL_SCOPE);
  ```
  in `RecipeAnalyzer`, reads it as `preview.get().then_some(sell_scope()).flatten()`, and Task 7 lifts that expression into the helper.
- Produces:
  - `let revenue_place: Memo<String>` on the page — the world / datacenter / region name revenue is priced at.
  - `RecipeAnalyzerTable`'s new required prop `#[prop(into)] revenue_place: Signal<String>`.
  - The rule, enforced by a test: `sell_place` reaches `market_extra` and nothing else; `revenue_place` reaches `marks`, the `RevSignal` header arm, `PickerContext.sell_place` and `recipe_analyzer_calc_formula_live`'s `sell` slot.

- [ ] **Step 1: Write the failing test**

Add to `recipe_analyzer.rs`'s `mod test`:

```rust
    /// The market columns' second line names the sell WORLD at every sell
    /// scope: Daily sales, Confidence, Trend and Drift all read the sell
    /// world's 7-day body whatever the revenue side is doing (spec §4), so
    /// a scope name there would be a lie. The two places are one variable
    /// apart in `header_extras`, which is why this is pinned.
    #[test]
    fn the_seven_day_sub_labels_name_the_sell_world_not_the_sell_scope() {
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
                let extra = market_extra(i18n, kind, "Gilgamesh").expect("a market extra");
                let line2 = extra.line2.expect("a second line");
                assert!(
                    line2.sub_label.contains("Gilgamesh"),
                    "{kind:?}: {}",
                    line2.sub_label
                );
                assert!(
                    !line2.sub_label.contains("Aether"),
                    "{kind:?} must not name the sell scope: {}",
                    line2.sub_label
                );
            }
        });
    }

    /// `market_extra` takes the sell world; the marks, the alternative
    /// revenue headers, the picker heading and the live sentence take the
    /// sell place. Reading the production half back out of the source is the
    /// only way to see which variable reached which call — the same
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
    }
```

with, beside the existing source-reading tests, one shared helper (extract it from `the_page_wires_both_gates_to_what_it_fetches`, which then calls it too):

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
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test::the_seven_day recipe_analyzer::test::the_two_places`
Expected: the first FAILS only if `market_extra` was wired wrong (it passes today — keep it, it is the regression net for the next two steps); the second FAILS on every `revenue_place` assertion.

- [ ] **Step 3: Add the page's second place**

In `RecipeAnalyzer`, beside the three pricing signals (`:3736-3739`):

```rust
    // Phase F's fourth pricing param. Read only under the lab (Task 7 lifts
    // this expression into `sell_scope_for`); the setter strips the default.
    let (sell_scope, set_sell_scope) = filter_query_signal::<SellScope>(FILTER_SELL_SCOPE);
```

and, immediately after the `sell_place` memo (`:3863-3868`):

```rust
    // Where REVENUE is priced. `sell_place` above stays the sell world, and
    // the difference is load-bearing: the market columns' "7d · ‹place›"
    // sub-labels, the sparkline feed, the 30-day body, the median tell and
    // Hop gain's home run all read the sell world's own data at every sell
    // scope (spec §4), so naming the scope there would be a lie.
    let revenue_place = Memo::new(move |_| {
        match preview
            .get()
            .then_some(sell_scope())
            .flatten()
            .map(SellScope::scope)
            .unwrap_or(Scope::World)
        {
            Scope::World => sell_place.get(),
            Scope::Datacenter => datacenter().unwrap_or_else(|| region.get()),
            Scope::Region => region(),
        }
    });
```

Pass it down: add `revenue_place=revenue_place` to the `<RecipeAnalyzerTable>` call beside `sell_place=sell_place`, add the prop

```rust
    /// The sell PLACE's name: the sell world under the default sell scope,
    /// its datacenter or region otherwise. Everything that names where
    /// revenue came from reads this; everything that names where the 7-day
    /// figures came from reads `sell_place`.
    #[prop(into)]
    revenue_place: Signal<String>,
```

next to `sell_place`, and switch the `sell` slot of `recipe_analyzer_calc_formula_live` (`:4228`) from `sell_place.get()` to `revenue_place.get()`.

- [ ] **Step 4: Switch the three label sites inside the table**

- `marks` (`:2670`): `let m = f.marks(revenue_place.get(), buy_place.get());`
- `header_extras`' `RevSignal` arm (`:2704`): `format!("{} · {}", short_signal(i18n, s), revenue_now)`, with `let revenue_now = revenue_place.get();` added beside the existing `let sell_now = sell_place.get();` and its comment extended to say why there are two.
- `column_options`' `PickerContext` (`:3003`): `sell_place: revenue_place.get(),`

Leave the `kind => match market_extra(i18n, kind, &sell_now)` arm exactly as it is.

Add one doc line to `PickerContext.sell_place` (`columns.rs:225`) and to `FormulaMarks.sell_place` (`formula.rs:266`):

```rust
    /// Where revenue is priced — the sell world, or the wider sell scope
    /// when a page has one. Never the place a market column's 7-day
    /// figures came from.
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test`
Expected: PASS. `formula_marks_labels_name_signal_and_place` and `market_headers_carry_their_tooltip_and_the_window` must be green **unchanged** — they are what proves the swap did not cross the two names.

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/analyzer_kit ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "feat(recipe-analyzer): name the sell place on the revenue side, the sell world on the 7d columns"
```

---

### Task 6: The fourth select, the active-filter count and Clear all

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:299-305` (beside `buy_scope_options`), `:2586-2595` (the table's signals), `:2905-2960` (`active_filters`), `:3059-3075` (`clear_all`), `:3875-3925` (`strip_terms`)
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/strip.rs` — **no change**; `StripTerm.place_select` already exists and `FormulaStrip` already renders it
- Modify: `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json` (2 keys each)

**Interfaces:**
- Consumes: `StripTerm.{place, place_select}` and `StripSelect` (`strip.rs:18-40`), `SellScope` (Task 1), `FILTER_SELL_SCOPE` (Task 1), `revenue_place` (Task 5).
- Produces:
  - `fn sell_scope_options(i18n) -> Vec<(&'static str, String)>` — `[("world", …), ("datacenter", …), ("region", …)]`, reusing the existing `datacenter` and `region` keys exactly as `buy_scope_options` does.
  - The revenue `StripTerm` gains `place_select: Some(StripSelect { … })` writing `?sell-scope=` through the default-stripping setter.
  - `active_filters` gains `FILTER_SELL_SCOPE`, gated on `preview`; `clear_all` calls `set_sell_scope(None)`.
  - i18n keys `sell_scope_this_world`, `formula_change_sell_scope_aria`.

- [ ] **Step 1: Write the failing tests**

```rust
    /// The strip's revenue chip carries two selects under Phase F — the
    /// signal and the place — and the cost chip still carries two. Four
    /// selects total: the "fourth Market select" the spec asks for is the
    /// fourth `<select>` reachable from the Market button, which under the
    /// lab is this strip.
    #[test]
    fn the_lab_strip_carries_four_selects_and_names_the_sell_place() {
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
            assert_eq!(html.matches("<select").count(), 2, "{html}");
            assert!(html.contains("Aether"), "the resolved place stays visible: {html}");
            assert!(html.contains("value=\"region\""), "{html}");
        });
    }

    /// The three sell-scope tokens are the buy-scope tokens, and every one
    /// of them has a label in every locale — a select whose option renders
    /// blank is how a bookmarked value becomes unreachable.
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
        });
    }

    /// The sell scope is counted like the three pricing params it sits
    /// beside, and Clear all resets it — but only under the lab, or a
    /// bookmarked `?sell-scope=` would silently change the flag-off page's
    /// "no active filters" hint.
    #[test]
    fn the_sell_scope_is_counted_and_cleared_like_the_other_market_params() {
        let production = production_source();
        assert!(
            production.contains(&format!(
                "{}(preview, {}()).is_some()",
                "sell_scope_for", "sell_scope"
            )),
            "active_filters must gate the sell scope on the lab"
        );
        assert!(
            production.contains(&format!("{}(FILTER_SELL_SCOPE)", "active.push")),
            "…and push the same key the URL uses"
        );
        assert!(
            production.contains(&format!("{}(None);", "set_sell_scope")),
            "Clear all must reset it"
        );
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test::the_lab_strip recipe_analyzer::test::every_sell_scope recipe_analyzer::test::the_sell_scope_is_counted`
Expected: FAIL — `cannot find function sell_scope_options`, unknown key `formula_change_sell_scope_aria`.

- [ ] **Step 3: Add the options and the two keys**

After `buy_scope_options` (`:299-305`):

```rust
/// Where the product is sold. The same three tokens the buy side uses, with
/// their own "this world" label: the buy side's reads "This world only" in
/// a buying sentence, and a translator is entitled to inflect the two
/// differently. Datacenter and Region reuse the shared nouns.
fn sell_scope_options(i18n: I18nContext<Locale, I18nKeys>) -> Vec<(&'static str, String)> {
    vec![
        ("world", t_string!(i18n, sell_scope_this_world).to_string()),
        ("datacenter", t_string!(i18n, datacenter).to_string()),
        ("region", t_string!(i18n, region).to_string()),
    ]
}
```

Locale values:

| key | en | fr | de | ja | cn | ko | tc |
|---|---|---|---|---|---|---|---|
| `sell_scope_this_world` | `This world only` | `Ce monde uniquement` | `Nur diese Welt` | `このワールドのみ` | `仅此服务器` | `이 서버만` | `僅此伺服器` |
| `formula_change_sell_scope_aria` | `Change where the product is sold` | `Changer où le produit est vendu` | `Ändern, wo das Produkt verkauft wird` | `商品を販売する場所を変更` | `更改产品的销售范围` | `제품을 판매할 범위 변경` | `變更產品的銷售範圍` |

Verify: every locale is now **1798** keys.

- [ ] **Step 4: Hang the select off the revenue term**

In `strip_terms` (`:3878-3901`), on the `TermRole::Revenue` term: keep `place: Some(sell_place.into())` but change it to `place: Some(revenue_place.into())`, and add

```rust
                place_select: Some(StripSelect {
                    value: Signal::derive(move || {
                        preview
                            .get()
                            .then_some(sell_scope())
                            .flatten()
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

in place of its `place_select: None`.

- [ ] **Step 5: Count it and clear it**

In the table, add `let (sell_scope, set_sell_scope) = filter_query_signal::<SellScope>(FILTER_SELL_SCOPE);` beside the other three (`:2591`); in `active_filters`, after the `FILTER_BUY_SCOPE` block (`:2925-2927`):

```rust
        // Lab-gated, unlike the three above: those are pre-lab params, and a
        // bookmarked `?sell-scope=` must not change the flag-off page's "no
        // active filters" hint. `sell_scope_for` is Task 7's helper.
        if sell_scope_for(preview, sell_scope()).is_some() {
            active.push(FILTER_SELL_SCOPE);
        }
```

and in `clear_all` (`:3059`), after `set_buy_scope(None);`:

```rust
        set_sell_scope(None);
```

(`clear_all` is deliberately **not** lab-gated: clearing a param that is absent is a no-op, and a user who turns the lab off after setting a scope should still be able to clear it.)

Because Task 6 now references `sell_scope_for`, add it in this task rather than Task 7 — it is three lines and Task 7's tests then only have to prove the *page* consults it:

```rust
/// The sell scope the page acts on: `None` — i.e. `Term::Fixed(World)`,
/// today's ledger exactly — whenever the `analyzer-recipe` lab is off, so a
/// bookmarked `?sell-scope=region` is inert on the flag-off page down to
/// the "no active filters" hint. The one gate, read by the page's formula
/// memo, the page's body key and the table's active-filter list.
fn sell_scope_for(preview: bool, param: Option<SellScope>) -> Option<SellScope> {
    preview.then_some(param).flatten()
}
```

and add its unit test:

```rust
    #[test]
    fn the_sell_scope_is_inert_with_the_toggle_off() {
        for param in [None, Some(SellScope(Scope::Region)), Some(SellScope::default())] {
            assert_eq!(sell_scope_for(false, param), None, "{param:?}");
        }
        assert_eq!(sell_scope_for(true, None), None);
        assert_eq!(
            sell_scope_for(true, Some(SellScope(Scope::Datacenter))),
            Some(SellScope(Scope::Datacenter))
        );
    }
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test analyzer_kit::strip`
Expected: PASS. `fixed_terms_render_static_chips_and_select_terms_render_selects` (strip.rs) must be green unchanged — the strip component itself did not move.

- [ ] **Step 7: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs ultros-frontend/ultros-app/locales
git commit -m "feat(recipe-analyzer): the sell-scope select, counted in filters and reset by Clear all"
```

---

### Task 7: The page fetches the sell-scope bodies, and proves the flag-off page does not

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:2427-2458` (beside `SellHistory` / `raw_sales_key`), `:2459-2530` (two table props), `:2640-2660` (the index resolution), `:2760-2800` (`PriceInputs`), `:3855-3875` and `:3940-3990` (the page's memos and the resource), `:4290-4400` (the Suspense join)

**Interfaces:**
- Consumes: `needed_bodies`, `BodyRole::{CheapestSellScope, SellScopeStats}`, `RecipeNeeds.{sell_scope_is_buy_scope, rev_signals}` (Task 2); `NeededSignals.rev` (Task 2); `sell_scope_for` (Task 6); `revenue_place` (Task 5); `PriceInputs.{revenue_listings, revenue_stats, revenue_stats_loaded}` (Task 3); `get_cheapest_listings`, `get_sale_stats`, `SALE_STATS_WINDOW_DAYS`.
- Produces:
  - `struct SellScopeBodies { listings: Option<CheapestListings>, stats: Option<BulkSaleStats>, stats_failed: bool }` — `Clone + Debug + PartialEq + serde::{Serialize, Deserialize}` (an `ArcResource` value round-trips through `JsonSerdeCodec`).
  - `async fn fetch_sell_scope(name: String, want_listings: bool, want_stats: bool) -> SellScopeBodies`.
  - `RecipeAnalyzerTable`'s two new props: `sell_scope_bodies: Option<SellScopeBodies>` and `sell_scope_is_buy_scope: bool`.
  - The page's `sell_scope_source: Memo<Option<(String, bool, bool)>>` resource key.

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
        let dc = world.with_sell_scope(SellScope(Scope::Datacenter));
        assert_eq!(
            sell_scope_key(&dc, &needs, "Aether"),
            Some(("Aether".to_string(), true, false))
        );

        // Datacenter, sale revenue: both halves.
        let dc_stats =
            ProfitFormula::recipe_from_query(None, Some(PriceSignal::SaleMedian), None)
                .with_sell_scope(SellScope(Scope::Datacenter));
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
        let both = ProfitFormula::recipe_from_query(
            Some(PriceSignal::SaleMin),
            Some(PriceSignal::SaleMedian),
            Some(BuyScope::Datacenter),
        )
        .with_sell_scope(SellScope(Scope::Datacenter));
        assert_eq!(sell_scope_key(&both, &deduped, "Aether"), None);
    }

    /// The page consults the gate rather than a constant, and consults the
    /// lab gate rather than the raw param. `-D warnings` proves only that
    /// *something* calls each one.
    #[test]
    fn the_page_wires_the_sell_scope_to_what_it_fetches() {
        let production = production_source();
        assert!(
            production.contains(&format!("{}(&formula, &needs, &{})", "sell_scope_key", "place")),
            "the resource key must come from `sell_scope_key`"
        );
        // The formula the page prices with reads the lab gate, not the param.
        assert!(
            production.contains(&format!(
                "match {}(preview.get(), {}())",
                "sell_scope_for", "sell_scope"
            )),
            "the page's formula memo must go through `sell_scope_for`"
        );
        assert!(
            !production.contains(&format!("{}({}().unwrap_or_default())", "with_sell_scope", "sell_scope")),
            "a raw param read would seat the scope with the lab off"
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
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test::the_sell_scope_bodies recipe_analyzer::test::the_page_wires_the_sell_scope`
Expected: FAIL — `cannot find function sell_scope_key`.

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
    /// A statistics body was asked for and did not arrive: the revenue
    /// signal degrades to the listing and the page says so, exactly as a
    /// failed buy-scope or sell-world body does.
    stats_failed: bool,
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
        stats_failed: want_stats && stats.is_none(),
        listings,
        stats,
    }
}
```

- [ ] **Step 4: Wire the page**

In `RecipeAnalyzer`, change `formula_page` (`:3754-3756`) to:

```rust
    let formula_page = Memo::new(move |_| {
        let f = ProfitFormula::recipe_from_query(cost_basis(), revenue_metric(), buy_scope());
        // The lab gate, never the raw param: with the toggle off this leaves
        // `Term::Fixed(Scope::World)`, which is what every pre-Phase-F URL
        // has always produced.
        match sell_scope_for(preview.get(), sell_scope()) {
            Some(s) => f.with_sell_scope(s),
            None => f,
        }
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

Add `sell_scope_bodies` to the Suspense join's tuple and to the `match`, and pass through:

```rust
                                        sell_scope_bodies=bodies
                                        sell_scope_is_buy_scope=sell_scope_is_buy_scope.get()
```

- [ ] **Step 5: Resolve the two revenue inputs inside the table**

Add the two props (beside `buy_stats_aliased`, `:2517`):

```rust
    /// Phase F's payload: the sell scope's cheapest map and, under a sale
    /// revenue signal, its statistics. `None` at the default sell scope.
    sell_scope_bodies: Option<SellScopeBodies>,
    /// The sell scope resolved to the buy scope's place, so the buy-side
    /// bodies stand in for it.
    sell_scope_is_buy_scope: bool,
```

and, after the existing index construction (`:2645-2652`):

```rust
    // Where revenue is priced. Three cases, and the middle one is why the
    // dedupe is a page-level memo rather than a string compare here.
    let scope_prices = sell_scope_bodies
        .as_ref()
        .and_then(|b| b.listings.clone())
        .map(|l| Arc::new(CheapestListingsMap::from(l)));
    let scope_stats_index: Option<Arc<StatsIndex>> = sell_scope_bodies
        .as_ref()
        .and_then(|b| b.stats.as_ref())
        .map(|s| Arc::new(stats_index(s)));
    let scope_stats_failed = sell_scope_bodies.as_ref().is_some_and(|b| b.stats_failed);
    let sell_scope_value = sell_scope_for(preview, sell_scope_untracked)
        .map(SellScope::scope)
        .unwrap_or(Scope::World);
    let revenue_prices: Option<Arc<CheapestListingsMap>> = match sell_scope_value {
        Scope::World => sell_world_prices.clone(),
        _ if sell_scope_is_buy_scope => Some(prices.clone()),
        _ => scope_prices,
    };
    // `revenue_stats_loaded` is what `effective()` downgrades on, so it must
    // say "the body revenue reads arrived", never "the sell world's did".
    let (revenue_stats_index, revenue_stats_loaded) = match sell_scope_value {
        Scope::World => (Some(sell_stats_index.clone()), sell_stats_loaded),
        _ => match (scope_stats_index, sell_scope_is_buy_scope) {
            (Some(i), _) => (Some(i), true),
            (None, true) => (buy_stats_index.clone(), buy_stats_loaded),
            (None, false) => (None, false),
        },
    };
```

`sell_scope_untracked` is `sell_scope.get_untracked()` read once at the top of the component: the table already remounts on a lab flip and on every resource change, and the sell scope is one of those resources' keys, so it cannot change under a mounted table.

Change the table's `formula` memo (`:2650`) to `.effective(buy_stats_loaded, revenue_stats_loaded)`, feed the `PriceInputs` literal `revenue_listings: revenue_prices.as_deref()`, `revenue_stats: revenue_stats_index.as_deref()`, `revenue_stats_loaded`, and add `|| scope_stats_failed` to the amber "sale statistics unavailable" banner condition (`:3373`).

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p ultros-app --lib`
Expected: PASS across the crate.

- [ ] **Step 7: Check the client build**

Run (no `RUSTFLAGS` in the environment):
```bash
cargo check -p ultros-app --no-default-features --features hydrate --target wasm32-unknown-unknown
```
Expected: exit 0. This is what proves `fetch_sell_scope`'s two awaits and the new resource compile for the client.

- [ ] **Step 8: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "feat(recipe-analyzer): fetch the sell-scope bodies, gated on the lab and the scope"
```

---

### Task 8: The changelog, the whole contract in one place, and every gate green

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/changelog.rs:33` (a new newest-first entry)
- Modify: `integration/runner.cjs` (the `analyzer-recipe` route's `?cols=` list)
- Modify: `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json` (`labs_analyzer_recipe_desc` edited)
- Modify: `docs/superpowers/specs/2026-09-01-analyzer-kit-design.md:479` (the "F 6" key estimate)
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs` (one contract test)

**Interfaces:**
- Consumes: everything above.
- Produces: no new API. This task exists because the URL contract, the changelog and the dead-code sweep are deliverables, not afterthoughts.

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
    }
```

- [ ] **Step 2: Run it to verify it passes or names the drift**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test::phase_f_adds_exactly`
Expected: PASS if Tasks 1–7 landed as written. Any failure here is a real contract drift — fix the production side, never the assertion.

- [ ] **Step 3: Say what shipped, to players**

At the **top** of `CHANGELOG` in `routes/changelog.rs` (newest first — `entries_are_sorted_newest_first` guards it):

```rust
    ChangelogEntry {
        date: "2026-09-04",
        title: "Recipe Analyzer: price what you make across your datacenter or region, not just one world",
        blurb: "With \"Recipe Analyzer: the market model\" on under Settings › Labs, the price the analyzer expects you to sell at now has a scope of its own, next to the one the ingredients already had. Leave it on your sell world, or widen it to your datacenter or the whole region — Price, Profit and the alternative revenue columns follow it, and a new Scope vs home column in the Columns picker says what the wider scope is worth per unit. It is usually negative for the cheapest listing, because a bigger market has more sellers to undercut; the sale-history signals are where a wider scope can pay. Sales per day, Confidence, Trend and the rest keep describing your own world, so the numbers you judge speed by never move.",
        link: Some("/recipe-analyzer"),
    },
```

- [ ] **Step 4: Extend the Labs description in all seven locales**

`labs_analyzer_recipe_desc` gains a final clause (edit, not a new key):

- en: `…, and a sell-side scope so revenue can be read across your datacenter or region.`
- fr: `…, et une portée côté vente pour lire le revenu sur votre centre serveur ou votre région.`
- de: `… sowie ein Verkaufsbereich, mit dem Erlöse über dein Rechenzentrum oder deine Region gelesen werden.`
- ja: `…、および収益をデータセンターや地域全体で読むための販売範囲。`
- cn: `…，以及销售端范围，可按大区或区域读取收益。`
- ko: `…, 그리고 데이터 센터나 지역 전체에서 수익을 읽을 수 있는 판매 범위.`
- tc: `…，以及銷售端範圍，可依大區或區域讀取收益。`

Verify all seven are still **1798** keys (this is an edit, not an addition).

- [ ] **Step 5: Cover the column in the e2e route**

In `integration/runner.cjs`, add `scope-vs-home` to the `?cols=` list on the `analyzer-recipe` route so the screenshot harness renders the new column at least once.

- [ ] **Step 6: Correct the spec's key estimate**

`docs/superpowers/specs/2026-09-01-analyzer-kit-design.md:479` — "F 6" becomes "F 4"; append `(the sell-scope select's datacenter and region labels reuse the shared `datacenter` / `region` keys, as `buy_scope_options` does)`. Docs-only.

- [ ] **Step 7: Sweep the dead code, then run every gate**

There must be no `#[allow]` anywhere on this branch. Between tasks, `NeededSignals.rev`, `SignalWants.{visible_rev, sort_rev}` and `SellScopeBodies.stats_failed` had no production reader; by now they all do (Task 7's key, Task 4's `signal_wants`, Task 7's banner). Confirm:

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
Expected: all green, `REAL_EXIT=0`. If clippy is OOM-killed (exit `137`), re-run as `cargo clippy --all-targets -j 2 -- -D warnings` — that is not a lint failure.

- [ ] **Step 8: Commit and open the PR**

```bash
git add ultros-frontend/ultros-app/src/routes/changelog.rs ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs \
        ultros-frontend/ultros-app/locales integration/runner.cjs docs/superpowers/specs/2026-09-01-analyzer-kit-design.md
git commit -m "docs(analyzer-kit): the phase F changelog entry, the URL contract, and the spec's key count"
```

Rebase onto `origin/main` once #1265 and #1266 have merged, then open the PR against `main` (a PR whose base is not `main` gets no CI). The PR body must record:

- **Numbers:** none for any existing URL. Two oracles pin it — `price_rows_matches_recorded_oracle_on_fixture` (profit, ROI, cost, price, tax) and `revenue_projection_is_unchanged_at_the_default_sell_scope` (`rev_alt`, `revenue_fell_back`, `sell_median`, `stat_hq`), the second recorded specifically because the first cannot see the sell-stat lookup.
- **A second URL selection key** (`sell-scope`), which the v1 spec's Decision 1 ruled out. Spec §9 names Phases F and J as the two that spend it; this is F's.
- **Capacity:** a non-default sell scope adds at most one cache key per view — the spec's fourth (§6, "plus sell scope Region with buy DC (F)"). The DC and region 7-day keys already exist as buy-scope keys, so the byte budget is unchanged for anyone who does not opt in.
- **Flag-off:** unchanged, no new carve-out; `?sell-scope=region` and `?cols=scope-vs-home` are both inert with the toggle off, pinned by `the_sell_scope_is_inert_with_the_toggle_off`, `the_page_wires_the_sell_scope_to_what_it_fetches` and `phase_f_adds_exactly_one_key_and_one_column_token`.
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
| pinned in the URL-contract test | 1 (the key), 4 (the token), 8 (both, in one place) |
| a fourth Market select and strip term | 6 |
| revenue over the sell place | 3 |
| `rev-*` over the sell place | 3 (`rev_alt` via `rev_signal_at`) |
| `scope-vs-home` | 3 (the row field), 4 (kind, cell, sort, token) |
| `SignalView { over: scope, base: buy scope, stats: scope }` | 3 |
| "under World it is today's composition byte for byte, pinned by a parity test that includes items with no sell-world listing" | 3 — `revenue_projection_is_unchanged_at_the_default_sell_scope`, over a fixture whose `sell` map holds outputs only, so ingredients have no sell-world listing by construction, plus `scope_fixture`'s "absent from the scope" third class |
| velocity, avg price, confidence, last sold, volume, VWAP, drift, trend stay on the sell world | 3 (`the_sell_worlds_own_figures_ignore_the_sell_scope`), 5 (`the_seven_day_sub_labels_name_the_sell_world_not_the_sell_scope`) |
| Hop gain stays buy-side | 3 (asserted equal across scopes) |
| "None without a home value, at most zero under listings" | 3 (the `None` cases), 4 (the tooltip states the sign rule) |
| `CheapestSellScope` / `SellScopeStats(7)` iff the sell scope is not World, deduped against the buy scope | 2 |
| the fourth cache key | 8 (recorded in the PR body) |
| Numbers: none for any existing URL | 3 (two oracles), 8 |
| Changelog | 8 |
| ships under `analyzer-recipe` | 4 (`lab:`), 6 (`sell_scope_for`), 7 |
| i18n in seven locales | 4 (2 keys), 6 (2 keys), 8 (1 edit) |

Two spec sentences are deliberately **not** implemented, each with a stated reason: the Phase 0 comment and Kosyne's question (the user has approved shipping without them), and the declined-fallback "rev-* columns at a fixed region scope without a selector" (only reachable if Phase F were declined).

**2. Placeholder scan.** No "TBD", no "add error handling", no "similar to Task N", no test described without its code. The one intentional blank is `revenue_projection_is_unchanged_at_the_default_sell_scope`'s `ORACLE`, which cannot be written in advance because it is a recording of the current build — Task 3 Steps 1–2 spell out the record-and-paste loop, the exact command, and what to do with the output, which is the same mechanism `price_rows_matches_recorded_oracle_on_fixture` already documents in-tree.

**3. Type consistency.** Checked every name that crosses a task boundary:
`SellScope` / `Scope` / `with_sell_scope` / `sell_scope()` (Task 1 → 2, 3, 5, 6, 7); `BodyRole::{CheapestSellScope, SellScopeStats}` (2 → 7); `RecipeNeeds.{sell_scope_is_buy_scope, rev_signals}` (2 → 7); `NeededSignals.{rev, scope_vs_home}` and `SignalWants.{visible_rev, sort_rev, scope_vs_home}` (2 → 3, 4, 7); `rev_signal_at` (3, used twice in 3); `PriceInputs.{revenue_listings, revenue_stats, revenue_stats_loaded}` (3 → 7); `RecipeProfitData.scope_vs_home: Option<(i32, i32)>` (3 → 4); `scope_vs_home_delta` (4, used by the cell and the comparator); `CellValue::SignedGil { delta, pct }` (4); `COL_SCOPE_VS_HOME` / `SortMode::ScopeVsHome` (4 → 8); `revenue_place` (5 → 6, 7); `sell_scope_for` (6 → 7, and forward-referenced by name in 5's Interfaces block with the reason); `sell_scope_key` / `SellScopeBodies` / `fetch_sell_scope` (7); `production_source()` (5, reused by 6 and 7).

One ordering hazard found and fixed during this pass: Task 6's `active_filters` change references `sell_scope_for`, which the first draft introduced in Task 7. `sell_scope_for` and its unit test moved into Task 6, and Task 7's tests now only prove that the *page* consults it — which keeps each task independently reviewable.
