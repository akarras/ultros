# Analyzer Kit Phase D: Signal Columns, "use" Pills, Hop Gain Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Behind the Labs token `analyzer-signal-columns`, every price signal becomes a sortable recipe-analyzer column with a "use" pill that makes it the formula's input, plus Hop gain / unit and Worlds to visit; with the flag off the page renders, fetches and computes exactly as Phase C left it.

**Architecture:** The pricing core gains the two fields Phase A deferred (`PriceSummary::chosen`, `IngredientLine.world_id`) plus an unpriced-line count and the subcraft rescue; a new kit module `hop.rs` turns two `CostBreakdown`s into a signed gain and a world list; `needed.rs` gains `needed_signals` (the per-recipe cost-signal set, sub-craft cap included) and the buy-scope body gate reads it. The column table grows by ten `lab`-gated rows, `SortMode` by four variants, the picker by groups, and `SortableHeaderCell` by a `trailing` slot the grid fills with the pill. `price_rows` runs `compute_cost` once per needed signal (+ once against the sell world for hop) and fills `cost_alt` / `rev_alt` / `hop` / `worlds` / `unpriced` on the row.

**Tech Stack:** Rust 2024, Leptos 0.8.20 (SSR + hydrate), leptos_i18n 0.6 (seven locales), the analyzer kit (`ultros-frontend/ultros-app/src/analyzer_kit/`), `ultros-api-types`.

**Specs:** `docs/superpowers/specs/2026-09-01-analyzer-kit-design.md` (§3 modules, §5 catalog, §6 fetch gate, §8 Phase D, §9 URL, §11 Labs) and the v1 spec `docs/superpowers/specs/2026-09-01-recipe-analyzer-profit-formula-columns-design.md` (its "Phase 2" is this phase: model L57-128, UI L130-222, data flow L224-269, URL L271-286).

## Global Constraints

- Every user-facing string goes through `leptos-i18n`; every new key exists in **all seven** locale files (`en, fr, de, ja, cn, ko, tc`) with a real translation (CLAUDE.md).
- `./check_ci.sh` (fmt-check + `cargo clippy --all-targets -- -D warnings`) must exit 0 before the PR; **no `#[allow(dead_code)]`**. Read its exit code from a file, never through a pipe: `./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"`. On Windows, Strawberry Perl must lead `PATH` (`export PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:$PATH"`).
- Under `pub(crate)` modules and `-D warnings`, any field, fn, variant or `pub use` whose only readers are tests fails CI. Kit items are dead **between** tasks by design; the branch-level gate is `check_ci.sh` in Task 11. Each task's own gate is `cargo test -p ultros-app --lib` (and `cargo test -p ultros-api-types` where named), which tolerates dead-code warnings.
- **Flag off = byte-identical.** With the `analyzer-signal-columns` lab off, every URL without `subcrafts=true` renders the same DOM, issues the same requests and computes the same numbers as HEAD. The one number change of the phase, the sub-craft rescue, applies with sub-crafts on regardless of the flag (kit §8 "Numbers: the subcraft rescue on a small set of rows, delta recorded"; v1 decision 5) and is changelog'd.
- No HashMap iteration order may reach the DOM (hydration): every rendered list is a `Vec` in table or first-appearance order; maps are looked up by key only.
- Every new cell keeps one element shape between its value and no-value states (the `GilOrDash` rule): class toggles, never arm switches.
- The three existing URL params (`revenue`, `cost-basis`, `buy-scope`) *are* the formula; the pills write exactly one of them through `filter_query_signal`. No new URL key. `?cols=` gains the ten tokens below appended after the existing seven; `DEFAULT_COLS` stays `["confidence"]`; `migrate_legacy_params` is untouched.
- `?cols=` / `?sort=` tokens (exact): `rev-listing-min, rev-sale-min, rev-sale-median, rev-sale-avg, cost-listing-min, cost-sale-min, cost-sale-median, cost-sale-avg, hop-gain, hop-worlds`.
- `SortMode` gains four variants — `RevSignal(PriceSignal)`, `CostSignal(PriceSignal)`, `HopGain`, `HopWorlds` — for **21** distinct sort modes (the eleven today plus 4 + 4 + 2). Hop and alt-signal columns sort with `cmp_none_last`; `HopWorlds` and every `cost-*` default ascending; `rev-*` and `hop-gain` default descending.
- New columns keep the page's `hidden md:block` convention (kit decision 7).
- Run `cargo` in the **foreground** inside subagents (a backgrounded build that outlives its session leaves uncommitted work behind).
- Do **not** post anything to Kosyne on #1233 (Aaron's decision, 2026-09-02). The spec's "Kosyne validates hop semantics" becomes "Aaron validates hop semantics on the PR".

## Decisions taken in this plan (the specs left them open)

| Question | Decision |
|---|---|
| Default sort direction for `rev-*` / `cost-*` | `cost-*` ascending (cheapest first, like Cost / unit), `rev-*` descending (like Price). |
| Keep the `sub_unit > 0` guard in the rescue? | Yes: `sub_unit > 0 && (unit_cost == 0 \|\| sub_unit < unit_cost)`. An all-unpriced sub-recipe would otherwise "rescue" a line to 0 and mislabel it `Subcraft`. |
| Picker group names | v1's four: Revenue · ‹sell world›, Cost · ‹buy scope›, Travel, Other (the kit assigns the Market / Location split to Phase E2). |
| Worlds to visit under Buy from = This world only | Renders "—" (`None`), matching Hop gain's `Unavailable`; sorts last. |
| World name / datacenter resolution | The page's existing `world_names: HashMap<i32, (String, String)>`; `hop.rs` takes a `dc_of: &dyn Fn(i32) -> Option<&str>` so it never sees the page's map shape. |
| Delta-title wording, cap hint, capped-cell title | Keys `analyzer_alt_cost_delta_title`, `analyzer_alt_revenue_delta_title`, `analyzer_picker_subcraft_cap_hint`, `analyzer_alt_cost_capped_title` (Task 1). |
| `needed_signals` shape | A new fn beside `needed_bodies`; `RecipeNeeds` gains `cost_signals: BTreeSet<PriceSignal>` so the body gate sees visible / sorted sale-cost columns. |
| `FormulaSide` (kit) vs `TermRole` (tree) | `TermRole::{Revenue, Cost}` names the pill's side. |
| `HopInfo` (v1) | Folded into two row fields, `hop: Option<HopGain>` and `worlds: Option<WorldsToVisit>`, because Hop gain and Worlds are needed independently. |
| The sub-craft cap counts | Everything beyond the selected signal counts toward the two extra runs, in this order: `ListingMin` for Worlds, the sort target, then visible `cost-*` columns in table order. What does not fit is `capped` and renders "—". |
| Cost-* under a missing buy-scope body | `cost_alt` for sale signals is `None` ("—"): the page never shows a listing number under a sale-signal heading. |
| Hop under a sale cost signal with the sell-world body missing | Both sides degrade to the listing pass (v1 L205). |
| Sub-craft rescue gating | Not gated by the lab (see Global Constraints); the PR records the prod row delta with sub-crafts on. |
| "(= …)" mark vs pill state under a degraded formula | The equals-slot sub-label and the picker suffix follow the *effective* formula (what the numbers use); the pill's pressed/disabled state follows the *selected* param (what pressing it writes). Under a degraded formula the effective column reads "(= …)" with a live pill and the selected column keeps its pressed, disabled pill. |
| Picker greying under the sub-craft cap | A ticked column is never disabled (it must stay untickable); a capped option is hinted, and disabled only while unchecked. |
| Flag-off header identity | Ten hidden optional columns would each write a `<!>` marker into the header rowgroup (an `Option` child), so the grid takes a `lab_columns: bool` and drops lab-gated columns from the header at build time when it is false; the table remounts on a lab flip because the Suspense join reads the lab. |
| Info-panel semantics sentence (v1 L121-128) | Implemented: `ToolCalculation.details` becomes a `Signal<String>` (as Phase C did for `formula`) and the recipe page appends the per-signal rules sentence under the lab. |
| Tax comment | The peer review of #1257 found `net_after_tax` floors the *net* (`gross * 95 / 100`), so the tax rounds up; the `TaxMath::IntegerFloor` doc comment said the opposite. Task 3 fixes the comment; the math is untouched. |

## File map

| File | Responsibility in this phase |
|---|---|
| `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json` | 28 new keys (Task 1). |
| `ultros-frontend/ultros-app/src/global_state/labs.rs` | `LAB_ANALYZER_SIGNAL_COLUMNS` + `LABS` entry (Task 1). |
| `ultros-frontend/ultros-app/src/routes/settings.rs` | Labs title/desc arms (Task 1). |
| `ultros-api-types/src/cheapest_listings.rs` | `PriceSummary::chosen` (Task 2). |
| `ultros-frontend/ultros-app/src/components/crafting_cost.rs` | `IngredientLine.world_id`, `CostBreakdown.unpriced_market_lines`, the rescue (Task 2). |
| `ultros-frontend/ultros-app/src/analyzer_kit/formula.rs` | `PriceSignal::ALL`, `PriceSignal::index`, the `TaxMath::IntegerFloor` comment (Task 3). |
| `ultros-frontend/ultros-app/src/analyzer_kit/signals.rs` | `stat_only`, `stat_only_cheapest` (Task 3). |
| `ultros-frontend/ultros-app/src/analyzer_kit/needed.rs` | `SignalWants`, `NeededSignals`, `needed_signals`, `RecipeNeeds.cost_signals` (Task 3). |
| `ultros-frontend/ultros-app/src/analyzer_kit/hop.rs` (new) | `HopGain`, `WorldsToVisit`, `hop_gain`, `worlds_to_visit` (Task 4). |
| `ultros-frontend/ultros-app/src/analyzer_kit/columns.rs` | `ColumnKind` variants, `PickerGroup`, `ColumnSpec.group`, `ToolColumnMeta.lab`, `CellCtx` fields, `grouped_picker_options` (Task 5). |
| `ultros-frontend/ultros-app/src/analyzer_kit/cells.rs` | `CellValue::{MutedGil, GilWithNote, Hop}`, `CellNote`, `gil_per_day_label` (Task 5). |
| `ultros-frontend/ultros-app/src/components/control_bar.rs` | `ColumnOption {group, disabled, hint}`, `PickerHeading`, `ColumnsPickerList` (Task 6). |
| `ultros-frontend/ultros-app/src/routes/analyzer.rs`, `routes/currency_exchange.rs` | `ColumnOption::new` at the literal sites (Task 6). |
| `ultros-frontend/ultros-app/src/components/sort_header.rs` | `trailing` prop (Task 7). |
| `ultros-frontend/ultros-app/src/analyzer_kit/grid.rs` | `HeaderExtras`, `HeaderExtra`, `HeaderLine2`, `HeaderPill`, pill rendering, `extras` / `on_pill` props (Task 7). |
| `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs` | Row fields + `price_rows` (Task 8); table rows, `SortMode`, sorting, URL tests (Task 9); page wiring, headers, picker, cells (Task 10). |
| `ultros-frontend/ultros-app/src/components/tool_help.rs` | `ToolCalculation.details: Signal<String>` (Task 10). |
| `ultros-frontend/ultros-app/src/routes/changelog.rs`, `integration/runner.cjs` | Changelog entry, e2e route (Task 11). |

## Test commands used below

```bash
cargo test -p ultros-app --lib -- <filter>
cargo test -p ultros-api-types -- <filter>
```

Both are run from the worktree root. SSR-render tests (`to_html()`) that touch `<Gil>`, `TermBadge` or `t_string!` must stand up the executor and an i18n context first:

```rust
let _ = any_spawner::Executor::init_futures_executor();
let owner = Owner::new();
owner.with(|| {
    provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
    // ... render ...
});
```

---

### Task 1: The lab token, its Settings entry and every new i18n key

**Files:**
- Modify: `ultros-frontend/ultros-app/src/global_state/labs.rs:16-33` (token + `LABS` entry) and its tests
- Modify: `ultros-frontend/ultros-app/src/routes/settings.rs:389-405` (`lab_title` / `lab_desc` arms)
- Modify: `ultros-frontend/ultros-app/locales/en.json`, `fr.json`, `de.json`, `ja.json`, `cn.json`, `ko.json`, `tc.json` (28 keys each, inserted after `signal_short_sale_avg`)

**Interfaces:**
- Produces: `pub const LAB_ANALYZER_SIGNAL_COLUMNS: &str = "analyzer-signal-columns";` in `global_state/labs.rs`; the 28 keys below, read by Tasks 5–10.

- [ ] **Step 1: Write the failing test**

In `labs.rs`'s `mod tests`, extend `labs_cookie_round_trips_known_tokens_only` and add one test:

```rust
    #[test]
    fn labs_cookie_round_trips_known_tokens_only() {
        let labs: Labs = "analyzer-ledger,bogus,,analyzer-ledger".parse().unwrap();
        assert_eq!(labs.enabled.len(), 1);
        assert!(labs.has(LAB_ANALYZER_LEDGER));
        assert_eq!(labs.to_string(), "analyzer-ledger");
        let empty: Labs = "".parse().unwrap();
        assert!(!empty.has(LAB_ANALYZER_LEDGER));
        assert_eq!(empty.to_string(), "");
        let both: Labs = "analyzer-signal-columns,analyzer-ledger".parse().unwrap();
        assert!(both.has(LAB_ANALYZER_SIGNAL_COLUMNS));
        assert_eq!(both.to_string(), "analyzer-ledger,analyzer-signal-columns");
    }

    #[test]
    fn every_lab_token_is_listed_once() {
        let mut tokens: Vec<&str> = LABS.iter().map(|l| l.token).collect();
        tokens.sort_unstable();
        tokens.dedup();
        assert_eq!(tokens.len(), LABS.len());
        assert!(tokens.contains(&LAB_ANALYZER_SIGNAL_COLUMNS));
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ultros-app --lib -- global_state::labs`
Expected: compile error, `LAB_ANALYZER_SIGNAL_COLUMNS` not found.

- [ ] **Step 3: Add the token and the `LABS` entry**

Replace `labs.rs:16-33` with:

```rust
/// The recipe analyzer's formula strip, marked headers and live info
/// panel (kit Phase C).
pub const LAB_ANALYZER_LEDGER: &str = "analyzer-ledger";

/// The recipe analyzer's price signals as columns, their "use" pills,
/// Hop gain / unit and Worlds to visit (kit Phase D).
pub const LAB_ANALYZER_SIGNAL_COLUMNS: &str = "analyzer-signal-columns";

pub struct LabInfo {
    pub token: &'static str,
}

/// Every live experiment. Adding one here is what makes it appear in
/// Settings; deleting it is part of shipping the feature. Each entry's
/// comment names when it is deleted (a struct field for that would have
/// no non-test reader, which `-D warnings` rejects).
pub const LABS: &[LabInfo] = &[
    // Deleted in the phase after the strip is validated (kit §11).
    LabInfo {
        token: LAB_ANALYZER_LEDGER,
    },
    // Deleted in the phase after the signal columns are validated (kit §11).
    LabInfo {
        token: LAB_ANALYZER_SIGNAL_COLUMNS,
    },
];
```

- [ ] **Step 4: Add the Settings arms**

In `settings.rs`, `lab_title` and `lab_desc` each gain an arm before the `_` arm:

```rust
        crate::global_state::labs::LAB_ANALYZER_SIGNAL_COLUMNS => {
            t_string!(i18n, labs_analyzer_signal_columns_title).to_string()
        }
```

and

```rust
        crate::global_state::labs::LAB_ANALYZER_SIGNAL_COLUMNS => {
            t_string!(i18n, labs_analyzer_signal_columns_desc).to_string()
        }
```

- [ ] **Step 5: Add the 28 keys to all seven locales**

Write this script to the scratchpad as `add_phase_d_keys.py` and run it from the worktree root with `python add_phase_d_keys.py`. It inserts each key after the line holding `"signal_short_sale_avg"` in every locale, keeping the file's indentation and key order, and refuses to run twice.

```python
import io, json, os, re, sys

KEYS = {
 "en": {
  "labs_analyzer_signal_columns_title": "Recipe Analyzer: price signals as columns",
  "labs_analyzer_signal_columns_desc": "Every price signal becomes a sortable column with a “use” pill, plus Hop gain / unit and Worlds to visit.",
  "analyzer_col_hop_gain": "Hop gain / unit",
  "analyzer_col_hop_worlds": "Worlds to visit",
  "analyzer_picker_group_place": "{{name}} · {{place}}",
  "analyzer_picker_group_travel": "Travel",
  "analyzer_picker_group_other": "Other",
  "analyzer_picker_cost_group_title": "Shows sale history for {{place}} (loads once)",
  "analyzer_picker_subcraft_cap_hint": "Sub-crafts on: at most two extra cost columns are priced",
  "analyzer_equals_cost_slot": "(= Cost / unit)",
  "analyzer_equals_price_slot": "(= Price)",
  "analyzer_use_pill": "use",
  "analyzer_use_as_cost_aria": "Use {{signal}} as the cost in Profit",
  "analyzer_use_as_revenue_aria": "Use {{signal}} as the revenue in Profit",
  "analyzer_alt_cost_delta_title": "vs the formula's cost input",
  "analyzer_alt_revenue_delta_title": "vs the formula's revenue input",
  "analyzer_alt_cost_capped_title": "Not priced: with sub-crafts on, only two extra cost columns are priced",
  "analyzer_hop_needed": "needed",
  "analyzer_hop_gain_title": "≈ {{gil}} gil/day at {{rate}} sales/day",
  "analyzer_hop_gain_help": "Home cost minus buy-scope cost, per unit, buy side only: positive means the trip saves gil, negative means stay home. “needed” marks an ingredient with no home listing and no vendor.",
  "analyzer_hop_worlds_help": "Worlds other than the sell world holding the cheapest listing of an ingredient. Buy side only; sub-craft materials are not counted.",
  "analyzer_hop_worlds_row": "• {{world}} · ingredients: {{n}}",
  "analyzer_hop_worlds_dcs": "Datacenters: {{n}}",
  "analyzer_hop_worlds_note": "buy side only · sub-craft materials not counted",
  "analyzer_price_listing_fallback": "listing",
  "analyzer_cost_unpriced": "{{n}} unpriced",
  "analyzer_cost_unpriced_title": "Unpriced ingredients: {{n}} — no listing in the buy scope and no vendor; they cost 0 here",
  "recipe_analyzer_calc_signal_semantics": "Alternative columns follow the same rule per signal: ingredients take the cheapest matching listing (HQ-preferred under Require HQ) or the chosen sale statistic, revenue the cheaper of NQ and HQ on the sell world. A missing or zero statistic falls through to the listing per ingredient, never to 0; alternative revenue shows — when the sell world has no sale history.",
 },
 "fr": {
  "labs_analyzer_signal_columns_title": "Analyseur de recettes : les signaux de prix en colonnes",
  "labs_analyzer_signal_columns_desc": "Chaque signal de prix devient une colonne triable avec une pastille « utiliser », plus le gain par saut / unité et les mondes à visiter.",
  "analyzer_col_hop_gain": "Gain par saut / unité",
  "analyzer_col_hop_worlds": "Mondes à visiter",
  "analyzer_picker_group_place": "{{name}} · {{place}}",
  "analyzer_picker_group_travel": "Déplacement",
  "analyzer_picker_group_other": "Autres",
  "analyzer_picker_cost_group_title": "Affiche l'historique des ventes de {{place}} (chargé une seule fois)",
  "analyzer_picker_subcraft_cap_hint": "Sous-crafts activés : au plus deux colonnes de coût supplémentaires sont calculées",
  "analyzer_equals_cost_slot": "(= Coût / unité)",
  "analyzer_equals_price_slot": "(= Prix)",
  "analyzer_use_pill": "utiliser",
  "analyzer_use_as_cost_aria": "Utiliser {{signal}} comme coût dans le profit",
  "analyzer_use_as_revenue_aria": "Utiliser {{signal}} comme revenu dans le profit",
  "analyzer_alt_cost_delta_title": "par rapport au coût utilisé par la formule",
  "analyzer_alt_revenue_delta_title": "par rapport au revenu utilisé par la formule",
  "analyzer_alt_cost_capped_title": "Non calculé : avec les sous-crafts activés, seules deux colonnes de coût supplémentaires sont calculées",
  "analyzer_hop_needed": "requis",
  "analyzer_hop_gain_title": "≈ {{gil}} gils/jour à {{rate}} ventes/jour",
  "analyzer_hop_gain_help": "Coût sur le monde de vente moins coût dans la zone d'achat, par unité, côté achat uniquement : positif, le déplacement fait économiser des gils ; négatif, restez chez vous. « requis » signale un ingrédient sans annonce sur votre monde ni vendeur PNJ.",
  "analyzer_hop_worlds_help": "Mondes autres que le monde de vente où se trouve l'annonce la moins chère d'un ingrédient. Côté achat uniquement ; les matériaux des sous-crafts ne sont pas comptés.",
  "analyzer_hop_worlds_row": "• {{world}} · ingrédients : {{n}}",
  "analyzer_hop_worlds_dcs": "Centres de données : {{n}}",
  "analyzer_hop_worlds_note": "côté achat uniquement · matériaux des sous-crafts non comptés",
  "analyzer_price_listing_fallback": "annonce",
  "analyzer_cost_unpriced": "{{n}} sans prix",
  "analyzer_cost_unpriced_title": "Ingrédients sans prix : {{n}} — ni annonce dans la zone d'achat ni vendeur PNJ ; ils comptent pour 0 ici",
  "recipe_analyzer_calc_signal_semantics": "Les colonnes alternatives suivent la même règle par signal : les ingrédients prennent l'annonce correspondante la moins chère (HQ en priorité avec « HQ requis ») ou la statistique de vente choisie, le revenu le moins cher entre NQ et HQ sur le monde de vente. Une statistique absente ou nulle retombe sur l'annonce, ingrédient par ingrédient, jamais sur 0 ; un revenu alternatif affiche — quand le monde de vente n'a pas d'historique de ventes.",
 },
 "de": {
  "labs_analyzer_signal_columns_title": "Rezept-Analyse: Preissignale als Spalten",
  "labs_analyzer_signal_columns_desc": "Jedes Preissignal wird zu einer sortierbaren Spalte mit einer „verwenden“-Schaltfläche, dazu Sprunggewinn / Einheit und zu besuchende Welten.",
  "analyzer_col_hop_gain": "Sprunggewinn / Einheit",
  "analyzer_col_hop_worlds": "Zu besuchende Welten",
  "analyzer_picker_group_place": "{{name}} · {{place}}",
  "analyzer_picker_group_travel": "Reise",
  "analyzer_picker_group_other": "Sonstige",
  "analyzer_picker_cost_group_title": "Zeigt die Verkaufshistorie für {{place}} (wird einmal geladen)",
  "analyzer_picker_subcraft_cap_hint": "Unterrezepte aktiv: höchstens zwei zusätzliche Kostenspalten werden berechnet",
  "analyzer_equals_cost_slot": "(= Kosten / Einheit)",
  "analyzer_equals_price_slot": "(= Preis)",
  "analyzer_use_pill": "verwenden",
  "analyzer_use_as_cost_aria": "{{signal}} als Kosten im Gewinn verwenden",
  "analyzer_use_as_revenue_aria": "{{signal}} als Erlös im Gewinn verwenden",
  "analyzer_alt_cost_delta_title": "gegenüber dem Kostenwert der Formel",
  "analyzer_alt_revenue_delta_title": "gegenüber dem Erlöswert der Formel",
  "analyzer_alt_cost_capped_title": "Nicht berechnet: mit aktiven Unterrezepten werden nur zwei zusätzliche Kostenspalten berechnet",
  "analyzer_hop_needed": "nötig",
  "analyzer_hop_gain_title": "≈ {{gil}} Gil/Tag bei {{rate}} Verkäufen/Tag",
  "analyzer_hop_gain_help": "Kosten auf der Verkaufswelt minus Kosten im Kaufbereich, pro Einheit, nur Kaufseite: positiv heißt, die Reise spart Gil; negativ heißt, zu Hause bleiben. „nötig“ markiert eine Zutat ohne Angebot auf der Heimatwelt und ohne Händler.",
  "analyzer_hop_worlds_help": "Welten außer der Verkaufswelt, auf denen das günstigste Angebot einer Zutat liegt. Nur Kaufseite; Materialien aus Unterrezepten werden nicht gezählt.",
  "analyzer_hop_worlds_row": "• {{world}} · Zutaten: {{n}}",
  "analyzer_hop_worlds_dcs": "Datenzentren: {{n}}",
  "analyzer_hop_worlds_note": "nur Kaufseite · Materialien aus Unterrezepten nicht gezählt",
  "analyzer_price_listing_fallback": "Angebot",
  "analyzer_cost_unpriced": "{{n}} ohne Preis",
  "analyzer_cost_unpriced_title": "Zutaten ohne Preis: {{n}} — weder Angebot im Kaufbereich noch Händler; sie zählen hier mit 0",
  "recipe_analyzer_calc_signal_semantics": "Alternative Spalten folgen derselben Regel je Signal: Zutaten nehmen das günstigste passende Angebot (HQ bevorzugt bei „HQ erforderlich“) oder die gewählte Verkaufsstatistik, der Erlös das günstigere von NQ und HQ auf der Verkaufswelt. Eine fehlende oder leere Statistik fällt je Zutat auf das Angebot zurück, nie auf 0; ein alternativer Erlös zeigt —, wenn die Verkaufswelt keine Verkaufshistorie hat.",
 },
 "ja": {
  "labs_analyzer_signal_columns_title": "レシピアナライザー：価格シグナルを列として表示",
  "labs_analyzer_signal_columns_desc": "すべての価格シグナルが「使う」ボタン付きの並べ替え可能な列になり、さらに移動利益 / 個と訪問ワールドが加わります。",
  "analyzer_col_hop_gain": "移動利益 / 個",
  "analyzer_col_hop_worlds": "訪問ワールド",
  "analyzer_picker_group_place": "{{name}} · {{place}}",
  "analyzer_picker_group_travel": "移動",
  "analyzer_picker_group_other": "その他",
  "analyzer_picker_cost_group_title": "{{place}} の販売履歴を表示します（読み込みは1回のみ）",
  "analyzer_picker_subcraft_cap_hint": "サブクラフト有効時：追加のコスト列は最大2つまで計算されます",
  "analyzer_equals_cost_slot": "（= コスト / 個）",
  "analyzer_equals_price_slot": "（= 価格）",
  "analyzer_use_pill": "使う",
  "analyzer_use_as_cost_aria": "{{signal}} を利益計算のコストとして使う",
  "analyzer_use_as_revenue_aria": "{{signal}} を利益計算の収入として使う",
  "analyzer_alt_cost_delta_title": "計算式のコスト入力との差",
  "analyzer_alt_revenue_delta_title": "計算式の収入入力との差",
  "analyzer_alt_cost_capped_title": "未計算：サブクラフト有効時は追加のコスト列を2つまでしか計算しません",
  "analyzer_hop_needed": "要移動",
  "analyzer_hop_gain_title": "1日あたり {{rate}} 件の売上で ≈ {{gil}} ギル/日",
  "analyzer_hop_gain_help": "販売ワールドでのコストから購入範囲でのコストを引いた単価（購入側のみ）：正なら移動で節約でき、負ならホームに留まるべきです。「要移動」はホームに出品がなくNPC販売もない素材を示します。",
  "analyzer_hop_worlds_help": "素材の最安出品がある、販売ワールド以外のワールド。購入側のみで、サブクラフトの素材は含みません。",
  "analyzer_hop_worlds_row": "• {{world}} · 素材 {{n}} 種",
  "analyzer_hop_worlds_dcs": "データセンター {{n}} 件",
  "analyzer_hop_worlds_note": "購入側のみ · サブクラフトの素材は含まず",
  "analyzer_price_listing_fallback": "出品",
  "analyzer_cost_unpriced": "価格なし {{n}}",
  "analyzer_cost_unpriced_title": "{{n}} 種の素材は購入範囲に出品がなくNPC販売もないため、ここでは0として計算しています",
  "recipe_analyzer_calc_signal_semantics": "代替列も各シグナルで同じルールに従います：素材は最安の該当出品（「HQ必須」時はHQ優先）または選択した販売統計、収入は販売ワールドでのNQ/HQの安い方を使います。統計が無いかゼロの場合は素材ごとに出品価格へ戻り、0にはなりません。販売ワールドに販売履歴が無ければ代替収入は — と表示します。",
 },
 "cn": {
  "labs_analyzer_signal_columns_title": "配方分析器：将价格信号显示为列",
  "labs_analyzer_signal_columns_desc": "每个价格信号都成为可排序的列并带有“使用”按钮，另外增加跳服收益 / 单位和需前往的服务器。",
  "analyzer_col_hop_gain": "跳服收益 / 单位",
  "analyzer_col_hop_worlds": "需前往的服务器",
  "analyzer_picker_group_place": "{{name}} · {{place}}",
  "analyzer_picker_group_travel": "跨服",
  "analyzer_picker_group_other": "其他",
  "analyzer_picker_cost_group_title": "显示 {{place}} 的销售历史（仅加载一次）",
  "analyzer_picker_subcraft_cap_hint": "启用子制作时：最多计算两个额外的成本列",
  "analyzer_equals_cost_slot": "（= 单位成本）",
  "analyzer_equals_price_slot": "（= 价格）",
  "analyzer_use_pill": "使用",
  "analyzer_use_as_cost_aria": "将 {{signal}} 用作利润中的成本",
  "analyzer_use_as_revenue_aria": "将 {{signal}} 用作利润中的收入",
  "analyzer_alt_cost_delta_title": "相对于公式所用的成本",
  "analyzer_alt_revenue_delta_title": "相对于公式所用的收入",
  "analyzer_alt_cost_capped_title": "未计算：启用子制作时只计算两个额外的成本列",
  "analyzer_hop_needed": "需跨服",
  "analyzer_hop_gain_title": "按每天 {{rate}} 笔销售计算 ≈ {{gil}} 金币/天",
  "analyzer_hop_gain_help": "售出服务器成本减去购买范围成本（每单位，仅计购买侧）：正数表示跨服可省钱，负数表示留在本服更好。“需跨服”表示某材料在本服没有挂单也没有 NPC 出售。",
  "analyzer_hop_worlds_help": "拥有某材料最低价挂单的、售出服务器以外的服务器。仅计购买侧，不计入子制作的材料。",
  "analyzer_hop_worlds_row": "• {{world}} · {{n}} 种材料",
  "analyzer_hop_worlds_dcs": "{{n}} 个数据中心",
  "analyzer_hop_worlds_note": "仅计购买侧 · 不计入子制作材料",
  "analyzer_price_listing_fallback": "挂单",
  "analyzer_cost_unpriced": "{{n}} 项无价格",
  "analyzer_cost_unpriced_title": "{{n}} 种材料在购买范围内没有挂单也没有 NPC 出售，此处按 0 计算",
  "recipe_analyzer_calc_signal_semantics": "备选列遵循相同的每信号规则：材料取最便宜的匹配挂单（勾选“需要 HQ”时优先 HQ）或所选的销售统计，收入取售出服务器上 NQ 与 HQ 中较低者。统计缺失或为零时按材料回退到挂单价，绝不为 0；售出服务器没有销售历史时，备选收入显示 —。",
 },
 "ko": {
  "labs_analyzer_signal_columns_title": "제작 레시피 분석기: 가격 신호를 열로 표시",
  "labs_analyzer_signal_columns_desc": "모든 가격 신호가 “사용” 버튼이 있는 정렬 가능한 열이 되고, 이동 이득 / 개와 방문할 서버가 추가됩니다.",
  "analyzer_col_hop_gain": "이동 이득 / 개",
  "analyzer_col_hop_worlds": "방문할 서버",
  "analyzer_picker_group_place": "{{name}} · {{place}}",
  "analyzer_picker_group_travel": "이동",
  "analyzer_picker_group_other": "기타",
  "analyzer_picker_cost_group_title": "{{place}}의 판매 기록을 표시합니다 (한 번만 불러옴)",
  "analyzer_picker_subcraft_cap_hint": "하위 제작 켜짐: 추가 비용 열은 최대 두 개까지만 계산됩니다",
  "analyzer_equals_cost_slot": "(= 단가)",
  "analyzer_equals_price_slot": "(= 가격)",
  "analyzer_use_pill": "사용",
  "analyzer_use_as_cost_aria": "{{signal}}을(를) 이익 계산의 비용으로 사용",
  "analyzer_use_as_revenue_aria": "{{signal}}을(를) 이익 계산의 매출로 사용",
  "analyzer_alt_cost_delta_title": "공식의 비용 입력 대비",
  "analyzer_alt_revenue_delta_title": "공식의 매출 입력 대비",
  "analyzer_alt_cost_capped_title": "계산 안 됨: 하위 제작이 켜져 있으면 추가 비용 열은 두 개만 계산됩니다",
  "analyzer_hop_needed": "필요",
  "analyzer_hop_gain_title": "하루 {{rate}}건 판매 기준 ≈ {{gil}} 길/일",
  "analyzer_hop_gain_help": "판매 서버 비용에서 구매 범위 비용을 뺀 개당 값(구매 측만): 양수면 이동이 이득, 음수면 홈에 머무는 편이 낫습니다. “필요”는 홈 서버에 매물도 NPC 판매도 없는 재료를 뜻합니다.",
  "analyzer_hop_worlds_help": "재료의 최저가 매물이 있는, 판매 서버 이외의 서버. 구매 측만 계산하며 하위 제작 재료는 세지 않습니다.",
  "analyzer_hop_worlds_row": "• {{world}} · 재료 {{n}}종",
  "analyzer_hop_worlds_dcs": "데이터센터 {{n}}개",
  "analyzer_hop_worlds_note": "구매 측만 · 하위 제작 재료 제외",
  "analyzer_price_listing_fallback": "매물",
  "analyzer_cost_unpriced": "가격 없음 {{n}}",
  "analyzer_cost_unpriced_title": "{{n}}종의 재료는 구매 범위에 매물도 NPC 판매도 없어 여기서는 0으로 계산됩니다",
  "recipe_analyzer_calc_signal_semantics": "대체 열도 신호별로 같은 규칙을 따릅니다. 재료는 가장 싼 매물(“HQ 필요” 시 HQ 우선) 또는 선택한 판매 통계를, 매출은 판매 서버의 NQ/HQ 중 싼 쪽을 씁니다. 통계가 없거나 0이면 재료별로 매물 가격으로 돌아가며 0이 되지 않습니다. 판매 서버에 판매 기록이 없으면 대체 매출은 —로 표시됩니다.",
 },
 "tc": {
  "labs_analyzer_signal_columns_title": "配方分析器：將價格訊號顯示為欄位",
  "labs_analyzer_signal_columns_desc": "每個價格訊號都成為可排序的欄位並附有「使用」按鈕，另外新增跳服收益 / 單位與需前往的伺服器。",
  "analyzer_col_hop_gain": "跳服收益 / 單位",
  "analyzer_col_hop_worlds": "需前往的伺服器",
  "analyzer_picker_group_place": "{{name}} · {{place}}",
  "analyzer_picker_group_travel": "跨服",
  "analyzer_picker_group_other": "其他",
  "analyzer_picker_cost_group_title": "顯示 {{place}} 的銷售歷史（僅載入一次）",
  "analyzer_picker_subcraft_cap_hint": "啟用子製作時：最多計算兩個額外的成本欄位",
  "analyzer_equals_cost_slot": "（= 單位成本）",
  "analyzer_equals_price_slot": "（= 價格）",
  "analyzer_use_pill": "使用",
  "analyzer_use_as_cost_aria": "將 {{signal}} 用作利潤中的成本",
  "analyzer_use_as_revenue_aria": "將 {{signal}} 用作利潤中的收入",
  "analyzer_alt_cost_delta_title": "相對於公式所用的成本",
  "analyzer_alt_revenue_delta_title": "相對於公式所用的收入",
  "analyzer_alt_cost_capped_title": "未計算：啟用子製作時只計算兩個額外的成本欄位",
  "analyzer_hop_needed": "需跨服",
  "analyzer_hop_gain_title": "按每天 {{rate}} 筆銷售計算 ≈ {{gil}} 金幣/天",
  "analyzer_hop_gain_help": "售出伺服器成本減去購買範圍成本（每單位，僅計購買側）：正數表示跨服可省錢，負數表示留在本服較好。「需跨服」表示某材料在本服沒有掛單也沒有 NPC 販售。",
  "analyzer_hop_worlds_help": "擁有某材料最低價掛單的、售出伺服器以外的伺服器。僅計購買側，不計入子製作的材料。",
  "analyzer_hop_worlds_row": "• {{world}} · {{n}} 種材料",
  "analyzer_hop_worlds_dcs": "{{n}} 個資料中心",
  "analyzer_hop_worlds_note": "僅計購買側 · 不計入子製作材料",
  "analyzer_price_listing_fallback": "掛單",
  "analyzer_cost_unpriced": "{{n}} 項無價格",
  "analyzer_cost_unpriced_title": "{{n}} 種材料在購買範圍內沒有掛單也沒有 NPC 販售，此處按 0 計算",
  "recipe_analyzer_calc_signal_semantics": "替代欄位遵循相同的每訊號規則：材料取最便宜的相符掛單（勾選「需要 HQ」時優先 HQ）或所選的銷售統計，收入取售出伺服器上 NQ 與 HQ 中較低者。統計缺失或為零時按材料回退到掛單價，絕不為 0；售出伺服器沒有銷售歷史時，替代收入顯示 —。",
 },
}

ROOT = "ultros-frontend/ultros-app/locales"
for locale, keys in KEYS.items():
    assert len(keys) == 28, locale
    path = os.path.join(ROOT, f"{locale}.json")
    with io.open(path, encoding="utf-8") as f:
        lines = f.read().split("\n")
    if any('"labs_analyzer_signal_columns_title"' in l for l in lines):
        sys.exit(f"{path}: keys already present")
    idx = next(i for i, l in enumerate(lines) if '"signal_short_sale_avg"' in l)
    indent = re.match(r"\s*", lines[idx]).group(0)
    new = [indent + json.dumps(k, ensure_ascii=False) + ": " + json.dumps(v, ensure_ascii=False) + "," for k, v in keys.items()]
    lines[idx + 1:idx + 1] = new
    with io.open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines))
    with io.open(path, encoding="utf-8") as f:
        json.load(f)  # still valid JSON
    print(locale, "ok")
```

- [ ] **Step 6: Verify every locale parses and has every key once**

Run:

```bash
for l in en fr de ja cn ko tc; do python -c "import json,sys; d=json.load(open('ultros-frontend/ultros-app/locales/$l.json', encoding='utf-8')); print('$l', len(d), d['analyzer_hop_needed'])"; done
grep -c '"analyzer_hop_needed"' ultros-frontend/ultros-app/locales/*.json
```

Expected: every locale prints `1778` and its translation of "needed"; every `grep -c` prints `1`.

- [ ] **Step 7: Run the tests**

Run: `cargo test -p ultros-app --lib -- global_state::labs`
Expected: PASS (3 tests). Note: a key missing from a non-default locale only warns (`cargo::warning=Missing key … in locale …`) and falls back to en, so a green build is not the seven-locale check — Step 6's `grep -c` / 1778 count is. A locale that misspells a `{{var}}` does break the build, at every `t_string!` call site for that key, because the builder takes the union of variable names across locales.

- [ ] **Step 8: Commit**

```bash
git add ultros-frontend/ultros-app/src/global_state/labs.rs ultros-frontend/ultros-app/src/routes/settings.rs ultros-frontend/ultros-app/locales/
git commit -m "feat(labs): analyzer-signal-columns experiment and its i18n keys (seven locales)"
```

---

### Task 2: `PriceSummary::chosen`, the ingredient world, the unpriced count and the sub-craft rescue

**Files:**
- Modify: `ultros-api-types/src/cheapest_listings.rs:92-114` (`PriceSummary`) and its `mod tests`
- Modify: `ultros-frontend/ultros-app/src/components/crafting_cost.rs:82-110` (structs), `:139-196` (`compute_ingredient_cost`), `:236-323` (`compute_cost_inner`) and its `mod tests`

**Interfaces:**
- Produces: `PriceSummary::chosen(&self, prefer_hq: bool) -> Option<CheapestListingData>`; `IngredientLine.world_id: i32` (0 for vendor, sub-craft and unpriced lines); `CostBreakdown.unpriced_market_lines: u16`. Read by Task 4 (`hop.rs`) and Task 8 (`price_rows`).

- [ ] **Step 1: Write the failing `chosen` test (api-types)**

Append to `ultros-api-types/src/cheapest_listings.rs`'s `mod tests`:

```rust
    /// `chosen` replays `lowest_gil` / `price_preferring_hq` but keeps the
    /// entry: LQ wins an equal-price tie under lowest, HQ under prefer.
    #[test]
    fn chosen_matches_lowest_gil_and_prefer_hq_with_tie_rule() {
        let prices: [Option<i32>; 5] = [None, Some(1), Some(2), Some(3), Some(4)];
        for lq in prices {
            for hq in prices {
                let s = PriceSummary {
                    lq: lq.map(|p| data(p, 11)),
                    hq: hq.map(|p| data(p, 22)),
                };
                assert_eq!(s.chosen(false).map(|c| c.price), s.lowest_gil(), "{lq:?} {hq:?}");
                assert_eq!(
                    s.chosen(true).map(|c| c.price),
                    s.price_preferring_hq(),
                    "{lq:?} {hq:?}"
                );
                if lq.is_some() && lq == hq {
                    assert_eq!(s.chosen(false).unwrap().world_id, 11, "lowest: LQ wins a tie");
                    assert_eq!(s.chosen(true).unwrap().world_id, 22, "prefer: HQ wins a tie");
                }
            }
        }
        let s = PriceSummary { lq: None, hq: None };
        assert!(s.chosen(false).is_none());
        assert!(s.chosen(true).is_none());
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ultros-api-types -- chosen`
Expected: compile error, no method `chosen`.

- [ ] **Step 3: Implement `chosen`**

Add to `impl PriceSummary` after `price_preferring_hq`:

```rust
    /// The listing `lowest_gil` (prefer_hq = false) or `price_preferring_hq`
    /// (prefer_hq = true) would price from, entry and all, so a caller can
    /// keep its world. LQ wins an equal-price tie under lowest; HQ under
    /// prefer.
    pub fn chosen(&self, prefer_hq: bool) -> Option<CheapestListingData> {
        match (self.lq, self.hq) {
            (None, None) => None,
            (None, Some(hq)) => Some(hq),
            (Some(lq), None) => Some(lq),
            (Some(lq), Some(hq)) => Some(if prefer_hq || hq.price < lq.price { hq } else { lq }),
        }
    }
```

- [ ] **Step 4: Run it**

Run: `cargo test -p ultros-api-types -- chosen`
Expected: PASS.

- [ ] **Step 5: Write the failing crafting-cost tests**

Append to `crafting_cost.rs`'s `mod tests` (after `compute_cost_prefers_subcraft_when_cheaper`; the helpers `one_listing`, `make_recipe`, `make_recipe_yielding`, `fixture_categories` and the `Box::leak` idiom already exist there):

```rust
    #[test]
    fn ingredient_line_records_the_chosen_listing_world() {
        let oh = EmptyOnHand;
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 0,
            shards: ShardsMode::ExcludeShards,
            on_hand: &oh,
            vendor_prices: None,
        };
        // NQ on world 7, HQ (dearer) on world 9: lowest picks NQ's world.
        let both = CheapestListingsMap::from(CheapestListings {
            cheapest_listings: vec![
                CheapestListingItem { item_id: 1000, hq: false, world_id: 7, cheapest_price: 100 },
                CheapestListingItem { item_id: 1000, hq: true, world_id: 9, cheapest_price: 150 },
            ],
        });
        assert_eq!(compute_ingredient_cost(ItemId(1000), 1, &both, &opts).world_id, 7);
        let hq_opts = CraftingCostOptions { require_hq: true, ..opts_copy(&opts, &oh) };
        assert_eq!(compute_ingredient_cost(ItemId(1000), 1, &both, &hq_opts).world_id, 9);
        // No listing at all: world 0 and price 0.
        let none = CheapestListingsMap::from(CheapestListings { cheapest_listings: vec![] });
        let line = compute_ingredient_cost(ItemId(1000), 1, &none, &opts);
        assert_eq!((line.unit_price, line.world_id), (0, 0));
        // A cheaper vendor wins: the world is 0 because nothing is bought on a market.
        let mut vendors = HashMap::new();
        vendors.insert(1000, 40);
        let vendor_opts = CraftingCostOptions { vendor_prices: Some(&vendors), ..opts_copy(&opts, &oh) };
        let line = compute_ingredient_cost(ItemId(1000), 1, &both, &vendor_opts);
        assert_eq!((line.source, line.world_id), (PriceSource::Vendor, 0));
    }

    /// `CraftingCostOptions` holds a `&dyn OnHand`, so it is neither `Copy`
    /// nor `Clone`; rebuild it field by field.
    fn opts_copy<'a>(o: &CraftingCostOptions<'a>, oh: &'a dyn OnHand) -> CraftingCostOptions<'a> {
        CraftingCostOptions {
            require_hq: o.require_hq,
            max_subcraft_depth: o.max_subcraft_depth,
            shards: o.shards,
            on_hand: oh,
            vendor_prices: o.vendor_prices,
        }
    }

    #[test]
    fn zero_priced_line_can_be_rescued_by_subcraft() {
        // Outer needs 1x 2000, which has NO listing; 2000 is craftable from 1x 1000 @ 100.
        let prices = one_listing(1000, false, 100, 1);
        let cats = fixture_categories();
        let outer = make_recipe(&[(2000, 1)]);
        let inner: &'static Recipe = Box::leak(Box::new(make_recipe_yielding(&[(1000, 1)], 2000, 1)));
        let mut recipes_by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        recipes_by_output.insert(ItemId(2000), vec![inner]);
        let oh = EmptyOnHand;
        let is_shard = |id: ItemId| cats.get(&id.0) == Some(&59);
        let with_subs = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 2,
            shards: ShardsMode::ExcludeShards,
            on_hand: &oh,
            vendor_prices: None,
        };
        let cb = compute_cost(&outer, &prices, &recipes_by_output, &with_subs, &is_shard);
        assert_eq!(cb.cost, 100, "the unlisted intermediate is costed as a craft, not as free");
        assert_eq!(cb.ingredient_lines[0].source, PriceSource::Subcraft);
        assert_eq!(cb.ingredient_lines[0].world_id, 0);
        assert_eq!(cb.unpriced_market_lines, 0);
        // Sub-crafts off: still free, still counted as unpriced.
        let no_subs = CraftingCostOptions { max_subcraft_depth: 0, ..opts_copy(&with_subs, &oh) };
        let cb = compute_cost(&outer, &prices, &recipes_by_output, &no_subs, &is_shard);
        assert_eq!((cb.cost, cb.unpriced_market_lines), (0, 1));
        // An all-unpriced sub-recipe cannot rescue anything (the `sub_unit > 0` guard).
        let inner_unpriced: &'static Recipe =
            Box::leak(Box::new(make_recipe_yielding(&[(3000, 1)], 2000, 1)));
        let mut by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        by_output.insert(ItemId(2000), vec![inner_unpriced]);
        let cb = compute_cost(&outer, &prices, &by_output, &with_subs, &is_shard);
        assert_eq!(cb.ingredient_lines[0].source, PriceSource::Market);
        assert_eq!((cb.cost, cb.unpriced_market_lines), (0, 1));
    }

    #[test]
    fn unpriced_lines_counted_after_shard_flag_and_subcraft_pass() {
        // Outer: 1x 1000 (@100), 1x 1001 (shard, unlisted), 1x 2000 (unlisted, craftable
        // from 1x 1000 @100 + 1x 3000 unlisted).
        let prices = one_listing(1000, false, 100, 1);
        let cats = fixture_categories();
        let outer = make_recipe(&[(1000, 1), (1001, 1), (2000, 1)]);
        let inner: &'static Recipe =
            Box::leak(Box::new(make_recipe_yielding(&[(1000, 1), (3000, 1)], 2000, 1)));
        let mut by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        by_output.insert(ItemId(2000), vec![inner]);
        let oh = EmptyOnHand;
        let is_shard = |id: ItemId| cats.get(&id.0) == Some(&59);
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 2,
            shards: ShardsMode::ExcludeShards,
            on_hand: &oh,
            vendor_prices: None,
        };
        let cb = compute_cost(&outer, &prices, &by_output, &opts, &is_shard);
        // The shard is excluded; 2000 was rescued (sub cost 100 > 0); the sub-run's
        // own unpriced 3000 propagates from the winning sub-run.
        assert_eq!(cb.cost, 200);
        assert_eq!(cb.unpriced_market_lines, 1);
    }

    #[test]
    fn unpriced_ignores_excluded_shards_and_vendor_sold() {
        let prices = one_listing(1000, false, 100, 1);
        let cats = fixture_categories();
        let outer = make_recipe(&[(1000, 1), (1001, 1), (2000, 1)]);
        let by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        let oh = EmptyOnHand;
        let is_shard = |id: ItemId| cats.get(&id.0) == Some(&59);
        let mut vendors = HashMap::new();
        vendors.insert(2000, 25);
        let excl = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 0,
            shards: ShardsMode::ExcludeShards,
            on_hand: &oh,
            vendor_prices: Some(&vendors),
        };
        // Shard excluded, 2000 vendor-sold: nothing is unpriced.
        let cb = compute_cost(&outer, &prices, &by_output, &excl, &is_shard);
        assert_eq!(cb.unpriced_market_lines, 0);
        // Shards on the books: the unlisted shard counts.
        let incl = CraftingCostOptions { shards: ShardsMode::IncludeMarket, ..opts_copy(&excl, &oh) };
        let cb = compute_cost(&outer, &prices, &by_output, &incl, &is_shard);
        assert_eq!(cb.unpriced_market_lines, 1);
        // require_hq skips the vendor floor, but a vendor-sold item is still not "unpriced".
        let hq = CraftingCostOptions { require_hq: true, ..opts_copy(&excl, &oh) };
        let cb = compute_cost(&outer, &prices, &by_output, &hq, &is_shard);
        assert_eq!(cb.ingredient_lines[2].source, PriceSource::Market);
        assert_eq!(cb.ingredient_lines[2].unit_price, 0);
        assert_eq!(cb.unpriced_market_lines, 0);
    }
```

- [ ] **Step 6: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- crafting_cost::tests`
Expected: compile errors (`world_id`, `unpriced_market_lines` missing).

- [ ] **Step 7: Add the fields**

`IngredientLine` (`crafting_cost.rs:83-91`) gains, after `source`:

```rust
    /// World the chosen market listing sits on; 0 for vendor, sub-craft
    /// and unpriced lines.
    pub world_id: i32,
```

`CostBreakdown` (`:101-110`) gains, after `sub_crafts`:

```rust
    /// Lines bought on a market that no listing priced (`unit_price == 0`),
    /// after the shard flag and the sub-craft pass: shards under
    /// `ExcludeShards` and vendor-sold items are not counted, and the
    /// winning sub-run's count propagates up.
    pub unpriced_market_lines: u16,
```

- [ ] **Step 8: Price through `chosen` and keep the world**

In `compute_ingredient_cost` replace the `market_price` block and the returned literal:

```rust
    // The listing lowest_gil / price_preferring_hq would price from, kept
    // whole so the line can say which world it was priced on.
    let summary = prices.find_matching_listings(item_id.0);
    let chosen = summary.chosen(opts.require_hq);
    let market_price = chosen.map(|c| c.price).unwrap_or(0);
```

(the vendor block is unchanged) and

```rust
    let world_id = match source {
        PriceSource::Market if unit_price > 0 => chosen.map(|c| c.world_id).unwrap_or(0),
        _ => 0,
    };

    IngredientLine {
        item_id,
        needed_total: amount_needed,
        used_from_on_hand,
        used_from_market,
        unit_price,
        is_shard,
        source,
        world_id,
    }
```

- [ ] **Step 9: The rescue and the count in `compute_cost_inner`**

Add `let mut unpriced: u16 = 0;` beside the other accumulators. Replace the sub-craft block so the winner's count is tracked and the rescue admits a 0-priced line:

```rust
        let mut unit_cost = line.unit_price;
        let mut best_sub_crafts: Vec<SubcraftInfo> = Vec::new();
        let mut best_unpriced: u16 = 0;
        if depth < opts.max_subcraft_depth
            && line.used_from_market > 0
            && let Some(sub_recipes) = recipes_by_output.get(&item_id)
        {
            for sub in sub_recipes {
                let sub_breakdown =
                    compute_cost_inner(sub, prices, recipes_by_output, opts, is_shard, depth + 1);
                let yield_per_craft = sub.amount_result.max(1);
                let sub_unit = sub_breakdown.cost / yield_per_craft;
                // A line no listing priced (unit_cost == 0) is rescued by any
                // priced sub-recipe: an unlisted intermediate is not free when
                // it is craftable. A 0-cost sub-run never wins (it would only
                // relabel "unpriced" as "sub-craft").
                if sub_unit > 0 && (unit_cost == 0 || sub_unit < unit_cost) {
                    unit_cost = sub_unit;
                    let mut winner = sub_breakdown.sub_crafts;
                    winner.push(SubcraftInfo {
                        item_id,
                        amount: line.used_from_market,
                        unit_cost: sub_unit,
                    });
                    best_sub_crafts = winner;
                    best_unpriced = sub_breakdown.unpriced_market_lines;
                }
            }
            if !best_sub_crafts.is_empty() {
                line.unit_price = unit_cost;
                line.source = PriceSource::Subcraft;
                line.world_id = 0;
            }
            sub_crafts.extend(best_sub_crafts.into_iter());
        }
        unpriced = unpriced.saturating_add(best_unpriced);
```

After the shard/cost accounting and before `ingredient_lines.push(line);` add:

```rust
        // Counted after the shard flag and the sub-craft pass: a rescued line
        // is `Subcraft` by now, an excluded shard is off the books, and an
        // item the vendor sells is never "unpriced" even when require_hq
        // skipped its vendor floor.
        let vendor_sold = opts
            .vendor_prices
            .and_then(|m| m.get(&item_id.0))
            .is_some_and(|p| *p > 0);
        let off_the_books = line.is_shard && matches!(opts.shards, ShardsMode::ExcludeShards);
        if line.source == PriceSource::Market
            && line.used_from_market > 0
            && line.unit_price == 0
            && !off_the_books
            && !vendor_sold
        {
            unpriced = unpriced.saturating_add(1);
        }
```

and the returned literal gains `unpriced_market_lines: unpriced,`.

- [ ] **Step 10: Run the whole crafting-cost module**

Run: `cargo test -p ultros-app --lib -- crafting_cost`
Expected: PASS, every pre-existing test included (the rescue only changes the `unit_cost == 0` case, which no existing test exercises). Fix any other `IngredientLine { .. }` / `CostBreakdown { .. }` literal the compiler reports (grep: `grep -rn "IngredientLine {\|CostBreakdown {" ultros-frontend/ultros-app/src`; at HEAD both live only in `crafting_cost.rs`).

- [ ] **Step 11: Commit**

```bash
git add ultros-api-types/src/cheapest_listings.rs ultros-frontend/ultros-app/src/components/crafting_cost.rs
git commit -m "feat(pricing): PriceSummary::chosen, ingredient world ids, unpriced-line count and the sub-craft rescue"
```

---

### Task 3: `PriceSignal` indexing, bare-stat reads and `needed_signals`

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/formula.rs:36-46` (`impl PriceSignal`) and its tests
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/signals.rs:40-57` (after `stat_price`) and its tests
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/needed.rs` (whole file)

**Interfaces:**
- Consumes: `PriceSignal`, `SaleStat`, `StatsIndex`, `stat_price`, `ProfitFormula::{cost_signal, buy_scope}` (all at HEAD).
- Produces:
  - `PriceSignal::ALL: [PriceSignal; 4]` (token order) and `PriceSignal::index(self) -> usize`.
  - `signals::stat_only(index: &StatsIndex, item_id: i32, hq: bool, stat: SaleStat) -> Option<i32>` and `signals::stat_only_cheapest(index: &StatsIndex, item_id: i32, stat: SaleStat) -> Option<i32>`.
  - `needed::SignalWants { visible_cost: Vec<PriceSignal>, sort_cost: Option<PriceSignal>, hop: bool, worlds: bool }`, `needed::NeededSignals { cost: BTreeSet<PriceSignal>, capped: BTreeSet<PriceSignal>, hop: bool, worlds: bool }`, `needed::needed_signals(formula: &ProfitFormula, wants: &SignalWants, use_subcrafts: bool) -> NeededSignals`; `RecipeNeeds` gains `cost_signals: BTreeSet<PriceSignal>` and loses `Copy`.

- [ ] **Step 1: Write the failing tests**

formula.rs tests:

```rust
    #[test]
    fn price_signal_index_matches_all_order() {
        for (i, s) in PriceSignal::ALL.iter().enumerate() {
            assert_eq!(s.index(), i);
        }
        assert_eq!(PriceSignal::ALL[0], PriceSignal::ListingMin);
        assert_eq!(PriceSignal::ALL[3], PriceSignal::SaleAvg);
    }
```

signals.rs tests (the module already imports `ultros_api_types::cheapest_listings::*`; add `use ultros_api_types::sale_stats::ItemSaleStats;` if not present):

```rust
    #[test]
    fn stat_only_has_no_fallback() {
        let mut index = StatsIndex::new();
        index.insert((7, false), ItemSaleStats { item_id: 7, hq: false, min_price: 90, median_price: 100, avg_price: 110, num_sold: 3, ..Default::default() });
        index.insert((7, true), ItemSaleStats { item_id: 7, hq: true, min_price: 0, median_price: 80, avg_price: 0, num_sold: 1, ..Default::default() });
        assert_eq!(stat_only(&index, 7, false, SaleStat::Median), Some(100));
        assert_eq!(stat_only(&index, 7, true, SaleStat::Min), None, "a zero stat is no stat");
        assert_eq!(stat_only(&index, 8, false, SaleStat::Median), None, "no row, no number");
        assert_eq!(stat_only_cheapest(&index, 7, SaleStat::Median), Some(80));
        assert_eq!(stat_only_cheapest(&index, 7, SaleStat::Avg), Some(110), "the zero HQ avg is skipped");
        assert_eq!(stat_only_cheapest(&index, 8, SaleStat::Min), None);
    }
```

needed.rs tests (replace the `needs` helper and add):

```rust
    fn needs(outliers: bool, same: bool) -> RecipeNeeds {
        RecipeNeeds {
            outliers,
            buy_scope_is_sell_world: same,
            cost_signals: BTreeSet::new(),
        }
    }

    fn set(signals: &[PriceSignal]) -> BTreeSet<PriceSignal> {
        signals.iter().copied().collect()
    }

    #[test]
    fn needed_signals_is_selection_union_visible_union_sort_target() {
        let f = ProfitFormula::recipe_from_query(Some(PriceSignal::SaleMedian), None, None);
        let wants = SignalWants {
            visible_cost: vec![PriceSignal::ListingMin, PriceSignal::SaleMedian],
            sort_cost: Some(PriceSignal::SaleAvg),
            hop: false,
            worlds: true,
        };
        let got = needed_signals(&f, &wants, false);
        assert_eq!(
            got.cost,
            set(&[PriceSignal::ListingMin, PriceSignal::SaleMedian, PriceSignal::SaleAvg])
        );
        assert!(got.capped.is_empty());
        assert!(!got.hop);
        assert!(got.worlds);
        // The default: exactly the selected signal, nothing else.
        let plain = needed_signals(&f, &SignalWants::default(), false);
        assert_eq!(plain.cost, set(&[PriceSignal::SaleMedian]));
        assert!(!plain.hop && !plain.worlds);
    }

    #[test]
    fn needed_signals_sets_hop_when_a_hop_column_is_the_sort_target() {
        let f = ProfitFormula::recipe_from_query(None, None, None);
        let wants = SignalWants { hop: true, ..SignalWants::default() };
        assert!(needed_signals(&f, &wants, false).hop);
        let wants = SignalWants { worlds: true, ..SignalWants::default() };
        let got = needed_signals(&f, &wants, false);
        assert!(got.worlds);
        assert!(got.cost.contains(&PriceSignal::ListingMin), "Worlds needs the listing-min run");
    }

    /// The cap lives here, not in the picker, so a bookmarked `?cols=` with
    /// four cost columns and sub-crafts on prices the selected signal plus
    /// two extras and marks the rest capped; identically on SSR and CSR.
    #[test]
    fn subcraft_cap_applies_to_url_bookmarks() {
        let f = ProfitFormula::recipe_from_query(None, None, None); // listing-min
        let all = vec![
            PriceSignal::ListingMin,
            PriceSignal::SaleMin,
            PriceSignal::SaleMedian,
            PriceSignal::SaleAvg,
        ];
        let wants = SignalWants { visible_cost: all.clone(), ..SignalWants::default() };
        let got = needed_signals(&f, &wants, true);
        assert_eq!(got.cost, set(&[PriceSignal::ListingMin, PriceSignal::SaleMin, PriceSignal::SaleMedian]));
        assert_eq!(got.capped, set(&[PriceSignal::SaleAvg]));
        // Without sub-crafts nothing is capped.
        let got = needed_signals(&f, &wants, false);
        assert_eq!(got.cost.len(), 4);
        assert!(got.capped.is_empty());
        // The sort target and Worlds' listing-min take slots before visible columns.
        let f = ProfitFormula::recipe_from_query(Some(PriceSignal::SaleMedian), None, None);
        let wants = SignalWants {
            visible_cost: all,
            sort_cost: Some(PriceSignal::SaleAvg),
            hop: false,
            worlds: true,
        };
        let got = needed_signals(&f, &wants, true);
        assert_eq!(got.cost, set(&[PriceSignal::SaleMedian, PriceSignal::ListingMin, PriceSignal::SaleAvg]));
        assert_eq!(got.capped, set(&[PriceSignal::SaleMin]));
    }

    #[test]
    fn visible_sale_cost_column_needs_the_buy_scope_body() {
        let f = ProfitFormula::recipe_from_query(None, None, None); // listing-min selected
        let mut n = needs(false, false);
        n.cost_signals = set(&[PriceSignal::ListingMin, PriceSignal::SaleMin]);
        assert!(needed_bodies(&f, &n).contains(&BodyRole::BuyScopeStats(SALE_STATS_WINDOW_DAYS)));
        n.cost_signals = set(&[PriceSignal::ListingMin]);
        assert!(!needed_bodies(&f, &n).contains(&BodyRole::BuyScopeStats(SALE_STATS_WINDOW_DAYS)));
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- analyzer_kit`
Expected: compile errors (`ALL`, `index`, `stat_only`, `SignalWants`, `cost_signals` missing).

- [ ] **Step 3: `PriceSignal::ALL` and `index`**

In `formula.rs`, inside the existing `impl PriceSignal` (the one with `sale_stat`):

```rust
    /// Every signal in token order; also the index order of the per-signal
    /// arrays a priced row carries (`cost_alt`, `rev_alt`).
    pub const ALL: [PriceSignal; 4] = [
        PriceSignal::ListingMin,
        PriceSignal::SaleMin,
        PriceSignal::SaleMedian,
        PriceSignal::SaleAvg,
    ];

    /// Position in [`PriceSignal::ALL`].
    pub fn index(self) -> usize {
        match self {
            PriceSignal::ListingMin => 0,
            PriceSignal::SaleMin => 1,
            PriceSignal::SaleMedian => 2,
            PriceSignal::SaleAvg => 3,
        }
    }
```

Also in `formula.rs`, replace the doc comment on `TaxMath::IntegerFloor` (the enum at `formula.rs:143-146`) with the truth the #1257 review established — the math itself does not change:

```rust
    /// `net = gross * 95 / 100` in integer math: the *net* is floored, so
    /// the tax itself rounds up (5% of 3,911 shows as 196, not 195). The
    /// flip finder and vendor pages truncate an f32 instead; the two agree
    /// below 2,207,541 gil.
    IntegerFloor,
```

- [ ] **Step 4: `stat_only` / `stat_only_cheapest`**

In `signals.rs`, after `stat_price`:

```rust
/// The bare statistic for `(item, hq)`: no listing fallback, `None` when
/// the row is missing or zero. Alternative revenue columns read this so a
/// world with no sale history shows "—" rather than a listing.
pub fn stat_only(index: &StatsIndex, item_id: i32, hq: bool, stat: SaleStat) -> Option<i32> {
    index
        .get(&(item_id, hq))
        .map(|row| stat_price(row, stat))
        .filter(|p| *p > 0)
}

/// The cheaper of the NQ and HQ bare statistics — today's revenue rule
/// (the cheaper quality sells first).
pub fn stat_only_cheapest(index: &StatsIndex, item_id: i32, stat: SaleStat) -> Option<i32> {
    match (stat_only(index, item_id, false, stat), stat_only(index, item_id, true, stat)) {
        (None, None) => None,
        (Some(a), None) | (None, Some(a)) => Some(a),
        (Some(a), Some(b)) => Some(a.min(b)),
    }
}
```

- [ ] **Step 5: `needed.rs`**

Replace the module doc, `RecipeNeeds` and `needed_bodies` with:

```rust
//! Which bulk bodies a view needs, and which cost signals the pricing pass
//! must run per recipe. The page consults the body set for the buy-scope
//! stats gate and hands the signal set to `price_rows`.

use std::collections::BTreeSet;

use super::formula::{BuyScope, PriceSignal, ProfitFormula};

/// The one sale-history window every recipe-analyzer body uses. The
/// server serves 1 | 7 | 30 | 90; the labels in seven locales say "(7d)".
pub const SALE_STATS_WINDOW_DAYS: u16 = 7;

/// A whole-scope body the page fetches. Symbolic: the page resolves each
/// role to a world / datacenter / region name.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BodyRole {
    CheapestBuyScope,
    CheapestSellWorld,
    SellWorldStats(u16),
    BuyScopeStats(u16),
    RecentSalesSellWorld,
}

/// Page state that changes which bodies are needed but is not part of
/// the formula.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct RecipeNeeds {
    /// The opt-in outlier filter reads raw recent sales.
    pub outliers: bool,
    /// Buy from = This world only, and it resolved to the sell world: the
    /// sell-world stats body doubles as the buy-scope body.
    pub buy_scope_is_sell_world: bool,
    /// Every cost signal the pass will run ([`NeededSignals::cost`]); a
    /// visible or sorted sale-cost column needs the buy-scope body even
    /// when the selected signal is the listing.
    pub cost_signals: BTreeSet<PriceSignal>,
}

/// The bodies the recipe analyzer needs for `formula`. The default URL
/// yields exactly the three bodies the page fetches today.
pub fn needed_bodies(formula: &ProfitFormula, needs: &RecipeNeeds) -> BTreeSet<BodyRole> {
    let mut set = BTreeSet::from([
        BodyRole::CheapestBuyScope,
        BodyRole::CheapestSellWorld,
        BodyRole::SellWorldStats(SALE_STATS_WINDOW_DAYS),
    ]);
    // The buy-scope body aliases the sell-world body only when the scope
    // IS a world and that world resolved to the sell world.
    let aliased = formula.buy_scope() == BuyScope::World && needs.buy_scope_is_sell_world;
    let wants_sale_stats = formula.cost_signal().sale_stat().is_some()
        || needs.cost_signals.iter().any(|s| s.sale_stat().is_some());
    if wants_sale_stats && !aliased {
        set.insert(BodyRole::BuyScopeStats(SALE_STATS_WINDOW_DAYS));
    }
    if needs.outliers {
        set.insert(BodyRole::RecentSalesSellWorld);
    }
    set
}

/// What the visible columns and the sort target ask of the pricing pass,
/// before the sub-craft cap. `visible_cost` is in table order.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SignalWants {
    pub visible_cost: Vec<PriceSignal>,
    pub sort_cost: Option<PriceSignal>,
    pub hop: bool,
    pub worlds: bool,
}

/// The cost signals `price_rows` runs per recipe, plus the two hop flags.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct NeededSignals {
    pub cost: BTreeSet<PriceSignal>,
    /// Requested but not run: the sub-craft cap. Their cells render "—".
    pub capped: BTreeSet<PriceSignal>,
    pub hop: bool,
    pub worlds: bool,
}

/// {effective cost} ∪ {ListingMin when Worlds is wanted} ∪ {the sort
/// target} ∪ {visible cost-* columns}. With sub-crafts on, at most two
/// signals beyond the selected one are kept, claimed in that order; the
/// rest are `capped`. Enforced here, not in the picker, so it holds for any
/// bookmarked URL and identically on SSR and CSR.
pub fn needed_signals(
    formula: &ProfitFormula,
    wants: &SignalWants,
    use_subcrafts: bool,
) -> NeededSignals {
    let selected = formula.cost_signal();
    let cap = if use_subcrafts { 2 } else { usize::MAX };
    let mut cost = BTreeSet::from([selected]);
    let mut capped = BTreeSet::new();
    let mut extras = 0usize;
    let mut claim = |s: PriceSignal, cost: &mut BTreeSet<PriceSignal>, capped: &mut BTreeSet<PriceSignal>| {
        if cost.contains(&s) {
            return;
        }
        if extras < cap {
            cost.insert(s);
            extras += 1;
        } else {
            capped.insert(s);
        }
    };
    if wants.worlds {
        claim(PriceSignal::ListingMin, &mut cost, &mut capped);
    }
    if let Some(s) = wants.sort_cost {
        claim(s, &mut cost, &mut capped);
    }
    for s in &wants.visible_cost {
        claim(*s, &mut cost, &mut capped);
    }
    NeededSignals {
        cost,
        capped,
        hop: wants.hop,
        worlds: wants.worlds,
    }
}
```

- [ ] **Step 6: Fix the one non-test `RecipeNeeds` literal**

`routes/recipe_analyzer.rs:2518-2521` builds `RecipeNeeds { outliers: false, buy_scope_is_sell_world: false }`; add `cost_signals: BTreeSet::new(),` (import `std::collections::BTreeSet`). Task 10 replaces this memo; for now it must only compile.

- [ ] **Step 7: Run the kit tests**

Run: `cargo test -p ultros-app --lib -- analyzer_kit`
Expected: PASS, including the four pre-existing `needed.rs` tests.

- [ ] **Step 8: Commit**

```bash
git add ultros-frontend/ultros-app/src/analyzer_kit/formula.rs ultros-frontend/ultros-app/src/analyzer_kit/signals.rs ultros-frontend/ultros-app/src/analyzer_kit/needed.rs ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "feat(analyzer-kit): PriceSignal::ALL, bare-stat reads and needed_signals with the sub-craft cap"
```

---

### Task 4: `hop.rs` — Hop gain / unit and Worlds to visit

**Files:**
- Create: `ultros-frontend/ultros-app/src/analyzer_kit/hop.rs`
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/mod.rs` (add `pub mod hop;`)

**Interfaces:**
- Consumes: `CostBreakdown { cost, ingredient_lines, unpriced_market_lines, .. }`, `IngredientLine { source, used_from_market, world_id, .. }`, `PriceSource` (Task 2); `formula::per_unit_cost`.
- Produces:
  - `pub enum HopGain { Gain(i32), Needed, Unavailable }` (Copy, Clone, Debug, PartialEq, Eq).
  - `pub struct WorldsToVisit { pub worlds: Vec<(i32, u16)>, pub dcs: u8 }` (Clone, Debug, PartialEq, Eq, Default): `(world id, ingredient lines priced there)` in first-appearance order.
  - `pub fn hop_gain(home: &CostBreakdown, scope: &CostBreakdown, amount_result: i32, scope_is_home: bool) -> HopGain`.
  - `pub fn worlds_to_visit<'a>(scope_listing_run: &CostBreakdown, home_world: i32, dc_of: &dyn Fn(i32) -> Option<&'a str>) -> WorldsToVisit`.

- [ ] **Step 1: Write the module with its failing tests**

Create `hop.rs`:

```rust
//! Hop gain / unit and Worlds to visit: is the trip to another world worth
//! it? Buy side only — revenue stays the sell world (2026-08-30 decision).

use std::collections::BTreeSet;

use crate::components::crafting_cost::{CostBreakdown, PriceSource};

use super::formula::per_unit_cost;

/// Home cost minus buy-scope cost per unit: signed, never clamped.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HopGain {
    Gain(i32),
    /// The home run has an ingredient with no home listing and no vendor:
    /// the trip is not optional.
    Needed,
    /// The scope run has unpriced lines, or the buy scope IS the home world.
    Unavailable,
}

/// Distinct non-home worlds holding the cheapest listing of a top-level
/// ingredient, in first-appearance order, and the datacenters they span.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct WorldsToVisit {
    /// `(world id, ingredient lines priced there)`.
    pub worlds: Vec<(i32, u16)>,
    pub dcs: u8,
}

/// `home` is `compute_cost` over the sell-world listings alone (not layered
/// over the buy scope: an ingredient with no home listing would otherwise
/// be priced at the scope price and zero the gain for exactly the
/// ingredients that force the trip); `scope` is the page's normal
/// buy-scope run under the same cost signal.
pub fn hop_gain(
    home: &CostBreakdown,
    scope: &CostBreakdown,
    amount_result: i32,
    scope_is_home: bool,
) -> HopGain {
    if scope_is_home || scope.unpriced_market_lines > 0 {
        return HopGain::Unavailable;
    }
    if home.unpriced_market_lines > 0 {
        return HopGain::Needed;
    }
    HopGain::Gain(per_unit_cost(home.cost, amount_result) - per_unit_cost(scope.cost, amount_result))
}

/// Over the *listing-min* scope run's top-level market lines only: vendor
/// lines and sub-craft lines carry world 0 and are skipped, so sub-craft
/// materials are never counted.
pub fn worlds_to_visit<'a>(
    scope_listing_run: &CostBreakdown,
    home_world: i32,
    dc_of: &dyn Fn(i32) -> Option<&'a str>,
) -> WorldsToVisit {
    let mut worlds: Vec<(i32, u16)> = Vec::new();
    for line in &scope_listing_run.ingredient_lines {
        if line.source != PriceSource::Market
            || line.used_from_market == 0
            || line.world_id == 0
            || line.world_id == home_world
        {
            continue;
        }
        match worlds.iter_mut().find(|(w, _)| *w == line.world_id) {
            Some((_, n)) => *n = n.saturating_add(1),
            None => worlds.push((line.world_id, 1)),
        }
    }
    let dcs: BTreeSet<&str> = worlds.iter().filter_map(|(w, _)| dc_of(*w)).collect();
    WorldsToVisit {
        worlds,
        dcs: dcs.len() as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::crafting_cost::IngredientLine;
    use xiv_gen::ItemId;

    fn breakdown(cost: i32, unpriced: u16, lines: Vec<IngredientLine>) -> CostBreakdown {
        CostBreakdown {
            cost,
            shard_cost: 0,
            on_hand_savings: 0,
            ingredient_lines: lines,
            sub_crafts: vec![],
            unpriced_market_lines: unpriced,
        }
    }

    fn line(item: i32, source: PriceSource, world_id: i32) -> IngredientLine {
        IngredientLine {
            item_id: ItemId(item),
            needed_total: 1,
            used_from_on_hand: 0,
            used_from_market: 1,
            unit_price: if world_id == 0 { 0 } else { 100 },
            is_shard: false,
            source,
            world_id,
        }
    }

    #[test]
    fn hop_gain_is_home_cost_minus_scope_cost_signed() {
        let home = breakdown(13_450, 0, vec![]);
        let scope = breakdown(11_300, 0, vec![]);
        assert_eq!(hop_gain(&home, &scope, 1, false), HopGain::Gain(2_150));
        // Negative means stay home; nothing is clamped.
        assert_eq!(hop_gain(&scope, &home, 1, false), HopGain::Gain(-2_150));
        // Per unit of output.
        assert_eq!(hop_gain(&home, &scope, 2, false), HopGain::Gain(6_725 - 5_650));
    }

    #[test]
    fn hop_is_needed_when_home_has_unpriced_lines() {
        let home = breakdown(100, 1, vec![]);
        let scope = breakdown(300, 0, vec![]);
        assert_eq!(hop_gain(&home, &scope, 1, false), HopGain::Needed);
    }

    #[test]
    fn hop_is_unavailable_when_scope_has_unpriced_lines_or_world_scope() {
        let home = breakdown(100, 0, vec![]);
        let scope = breakdown(300, 2, vec![]);
        assert_eq!(hop_gain(&home, &scope, 1, false), HopGain::Unavailable);
        let scope = breakdown(300, 0, vec![]);
        assert_eq!(hop_gain(&home, &scope, 1, true), HopGain::Unavailable);
        // Unavailable outranks Needed.
        let home = breakdown(100, 1, vec![]);
        assert_eq!(hop_gain(&home, &scope, 1, true), HopGain::Unavailable);
    }

    #[test]
    fn hop_worlds_counts_distinct_non_home_listing_worlds_and_dcs() {
        let run = breakdown(
            0,
            0,
            vec![
                line(1, PriceSource::Market, 5),
                line(2, PriceSource::Market, 7),
                line(3, PriceSource::Market, 5),
                line(4, PriceSource::Vendor, 0),
                line(5, PriceSource::Market, 3), // the home world
                line(6, PriceSource::Subcraft, 0),
                line(7, PriceSource::Market, 0), // unpriced
            ],
        );
        let same_dc = |w: i32| match w {
            5 | 7 | 3 => Some("Aether"),
            _ => None,
        };
        let got = worlds_to_visit(&run, 3, &same_dc);
        assert_eq!(got.worlds, vec![(5, 2), (7, 1)], "first-appearance order, counts per world");
        assert_eq!(got.dcs, 1);
        let two_dcs = |w: i32| match w {
            5 => Some("Aether"),
            7 => Some("Primal"),
            _ => None,
        };
        assert_eq!(worlds_to_visit(&run, 3, &two_dcs).dcs, 2);
        // A line whose world is on hand entirely is not a trip.
        let mut on_hand = line(8, PriceSource::Market, 9);
        on_hand.used_from_market = 0;
        let run = breakdown(0, 0, vec![on_hand]);
        assert_eq!(worlds_to_visit(&run, 3, &two_dcs), WorldsToVisit::default());
    }
}
```

Add `pub mod hop;` to `analyzer_kit/mod.rs` (alphabetical: between `grid` and `needed`) and extend the module doc's list with "the hop maths (`hop`)".

- [ ] **Step 2: Run the tests**

Run: `cargo test -p ultros-app --lib -- analyzer_kit::hop`
Expected: PASS (4 tests). `hop.rs` has no non-test reader until Task 8; dead-code warnings are expected here.

- [ ] **Step 3: Commit**

```bash
git add ultros-frontend/ultros-app/src/analyzer_kit/hop.rs ultros-frontend/ultros-app/src/analyzer_kit/mod.rs
git commit -m "feat(analyzer-kit): hop.rs — signed hop gain per unit and worlds to visit"
```

---

### Task 5: Column kinds, picker groups, the lab gate on a column, and the three new cells

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/columns.rs` (`ColumnKind`, `ColumnSpec`, `CellCtx`, `ToolColumnMeta`, `picker_options`, new `PickerGroup`, `PickerContext`, `grouped_picker_options`) and its tests
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/cells.rs` (`CellValue`, `CellNote`, `render_cell`, `gil_per_day_label`) and its tests
- Modify: `ultros-frontend/ultros-app/src/components/control_bar.rs:43-55` (`ColumnOption` fields + `PickerHeading`; rendering is Task 6)
- Modify: every `ColumnSpec` / `ToolColumnMeta` / `CellCtx` / `ColumnOption` literal the compiler reports: `routes/recipe_analyzer.rs` (15 `SPEC_*`, `RECIPE_BASE`, `cell_ctx`), `analyzer_kit/grid.rs` tests, `analyzer_kit/cells.rs` tests, `analyzer_kit/columns.rs` tests, `routes/analyzer.rs:1382-1385`, `routes/currency_exchange.rs:702-717`

**Interfaces:**
- Consumes: `PriceSignal` (formula), `HopGain` (Task 4), `TermRole`, `GilIcon` / `Gil` / `GilOrDash` (`components/gil.rs`, all `pub`).
- Produces:
  - `ColumnKind::{RevSignal(PriceSignal), CostSignal(PriceSignal), HopGain, HopWorlds}`.
  - `pub enum PickerGroup { Revenue, Cost, Travel, Other }` (Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash — declaration order is picker order); `ColumnSpec.group: PickerGroup`.
  - `ToolColumnMeta.lab: Option<&'static str>` — the Labs token that gates the column; `None` = always available.
  - `CellCtx { now_unix: i64, signal_columns: bool, capped_cost: [bool; 4] }` (indexed by `PriceSignal::index`).
  - `CellValue::MutedGil { amount: Option<i32>, pct: Option<f32>, side: TermRole, capped: bool }`, `CellValue::GilWithNote { amount: i32, note: CellNote }`, `CellValue::Hop { gain: HopGain, daily_sales: f32 }`; `pub enum CellNote { None, ListingFallback }`.
  - `cells::gil_per_day_label(gil: f32) -> String` ("13.5k", "632", "1.5M").
  - `control_bar::ColumnOption { id, label, group: Option<PickerHeading>, disabled: bool, hint: Option<String> }`, `ColumnOption::new(id, label)` (group `None`, not disabled, no hint), `pub struct PickerHeading { pub label: String, pub title: Option<String> }`.
  - `columns::PickerContext { sell_place: String, buy_place: String, revenue: PriceSignal, cost: PriceSignal, capped: BTreeSet<PriceSignal> }` and `columns::grouped_picker_options(cols, i18n, ctx: &PickerContext) -> Vec<ColumnOption>`; `picker_options` (flat) now skips `lab`-gated columns.

- [ ] **Step 1: Write the failing tests**

cells.rs tests:

```rust
    #[test]
    fn new_cells_keep_one_shape_per_variant() {
        use crate::analyzer_kit::hop::HopGain;
        use crate::components::term_badge::TermRole;
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let i18n = crate::i18n::use_i18n();
            let ctx = CellCtx {
                now_unix: 1_700_000_000,
                signal_columns: true,
                capped_cost: [false; 4],
            };
            let render = |v: CellValue| render_cell("w-40", v, i18n, &ctx).unwrap().to_html();
            let a = render(CellValue::MutedGil { amount: Some(138), pct: Some(38.0), side: TermRole::Cost, capped: false });
            let b = render(CellValue::MutedGil { amount: None, pct: None, side: TermRole::Cost, capped: false });
            let c = render(CellValue::MutedGil { amount: None, pct: None, side: TermRole::Cost, capped: true });
            assert_eq!(count(&a, "<div"), count(&b, "<div"), "{a}\n{b}");
            assert_eq!(count(&b, "<div"), count(&c, "<div"));
            assert!(a.contains("+38%"), "{a}");
            assert!(a.contains("title=\"vs the formula's cost input\""), "{a}");
            assert!(b.contains("—"), "{b}");
            assert!(c.contains("Not priced"), "{c}");
            let r = render(CellValue::MutedGil { amount: Some(1), pct: Some(-4.0), side: TermRole::Revenue, capped: false });
            assert!(r.contains("vs the formula's revenue input") && r.contains("-4%"), "{r}");

            let plain = render(CellValue::GilWithNote { amount: 120, note: CellNote::None });
            let tell = render(CellValue::GilWithNote { amount: 120, note: CellNote::ListingFallback });
            assert_eq!(count(&plain, "<div"), count(&tell, "<div"));
            assert!(tell.contains(">listing<"), "{tell}");
            assert!(!plain.contains("listing"), "{plain}");

            let gain = render(CellValue::Hop { gain: HopGain::Gain(2_150), daily_sales: 6.3 });
            let loss = render(CellValue::Hop { gain: HopGain::Gain(-300), daily_sales: 1.0 });
            let needed = render(CellValue::Hop { gain: HopGain::Needed, daily_sales: 6.3 });
            let none = render(CellValue::Hop { gain: HopGain::Unavailable, daily_sales: 6.3 });
            for h in [&loss, &needed, &none] {
                assert_eq!(count(&gain, "<div"), count(h, "<div"), "{gain}\n{h}");
                assert_eq!(count(&gain, "<span"), count(h, "<span"));
            }
            assert!(gain.contains("+2,150") && gain.contains("title=\"≈ 13.5k gil/day at 6.3 sales/day\""), "{gain}");
            assert!(loss.contains("-300") && !loss.contains("+"), "{loss}");
            assert!(needed.contains(">needed<") && !needed.contains("title="), "{needed}");
            assert!(none.contains("—"), "{none}");
        });
    }

    #[test]
    fn gil_per_day_label_abbreviates() {
        assert_eq!(gil_per_day_label(13_545.0), "13.5k");
        assert_eq!(gil_per_day_label(632.0), "632");
        assert_eq!(gil_per_day_label(-2_150.0), "-2.2k");
        assert_eq!(gil_per_day_label(1_500_000.0), "1.5M");
        assert_eq!(gil_per_day_label(0.0), "0");
    }
```

Update the existing `render_cell_keeps_one_shape_per_variant` test's `CellCtx { now_unix: 1_700_000_000 }` to the three-field literal (`signal_columns: false, capped_cost: [false; 4]`).

columns.rs tests (the module has a `Col` enum and a `BASE`-style table; add a second table for the picker):

```rust
    use crate::analyzer_kit::formula::PriceSignal;
    use leptos::prelude::{Owner, provide_context};
    use std::collections::BTreeSet;

    fn lbl_conf(_: I18nContext<Locale, I18nKeys>) -> String { "Confidence".into() }
    fn lbl_rev(_: I18nContext<Locale, I18nKeys>) -> String { "Sale median (7d)".into() }
    fn lbl_cost(_: I18nContext<Locale, I18nKeys>) -> String { "Cheapest listing".into() }
    fn lbl_cost2(_: I18nContext<Locale, I18nKeys>) -> String { "Sale average (7d)".into() }
    fn lbl_hop(_: I18nContext<Locale, I18nKeys>) -> String { "Hop gain / unit".into() }
    static P_CONF: ColumnSpec = ColumnSpec { kind: ColumnKind::Confidence, label: lbl_conf, group: PickerGroup::Other };
    static P_REV: ColumnSpec = ColumnSpec { kind: ColumnKind::RevSignal(PriceSignal::SaleMedian), label: lbl_rev, group: PickerGroup::Revenue };
    static P_COST: ColumnSpec = ColumnSpec { kind: ColumnKind::CostSignal(PriceSignal::ListingMin), label: lbl_cost, group: PickerGroup::Cost };
    static P_COST2: ColumnSpec = ColumnSpec { kind: ColumnKind::CostSignal(PriceSignal::SaleAvg), label: lbl_cost2, group: PickerGroup::Cost };
    static P_HOP: ColumnSpec = ColumnSpec { kind: ColumnKind::HopGain, label: lbl_hop, group: PickerGroup::Travel };
    fn any_cell(_: &(), _: &CellCtx) -> CellValue { CellValue::Custom }
    const PBASE: ToolColumnMeta<(), Col> = ToolColumnMeta {
        spec: &P_CONF, id: "", sort_id: "", sort: Sortability::No, default_dir: SortDir::Desc,
        header_class: "", cell_class: "", default_on: false, cell: any_cell, side: None,
        formula_header_class: "", formula_cell_class: "", lab: None,
    };
    static PICKER: [ToolColumnMeta<(), Col>; 5] = [
        ToolColumnMeta { spec: &P_CONF, id: "confidence", ..PBASE },
        ToolColumnMeta { spec: &P_REV, id: "rev-sale-median", lab: Some("analyzer-signal-columns"), ..PBASE },
        ToolColumnMeta { spec: &P_COST, id: "cost-listing-min", lab: Some("analyzer-signal-columns"), ..PBASE },
        ToolColumnMeta { spec: &P_COST2, id: "cost-sale-avg", lab: Some("analyzer-signal-columns"), ..PBASE },
        ToolColumnMeta { spec: &P_HOP, id: "hop-gain", lab: Some("analyzer-signal-columns"), ..PBASE },
    ];

    /// Groups come out in `PickerGroup` order, entries in table order within
    /// a group; the selected signals carry their "(= …)" suffix; capped cost
    /// columns are disabled with the hint; the Cost heading carries the
    /// loads-once title.
    #[test]
    fn grouped_picker_keeps_option_order() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let i18n = crate::i18n::use_i18n();
            let ctx = PickerContext {
                sell_place: "Gilgamesh".into(),
                buy_place: "Aether".into(),
                revenue: PriceSignal::SaleMedian,
                cost: PriceSignal::ListingMin,
                capped: BTreeSet::from([PriceSignal::SaleAvg]),
            };
            let got = grouped_picker_options(&PICKER, i18n, &ctx);
            let ids: Vec<&str> = got.iter().map(|o| o.id).collect();
            assert_eq!(ids, ["rev-sale-median", "cost-listing-min", "cost-sale-avg", "hop-gain", "confidence"]);
            assert_eq!(got[0].label, "Sale median (7d) (= Price)");
            assert_eq!(got[0].group.as_ref().unwrap().label, "Revenue · Gilgamesh");
            assert_eq!(got[1].label, "Cheapest listing (= Cost / unit)");
            let cost_heading = got[1].group.as_ref().unwrap();
            assert_eq!(cost_heading.label, "Cost · Aether");
            assert_eq!(cost_heading.title.as_deref(), Some("Shows sale history for Aether (loads once)"));
            assert!(got[2].disabled && got[2].hint.is_some(), "{:?}", got[2]);
            assert!(!got[1].disabled && got[1].hint.is_none());
            assert_eq!(got[3].group.as_ref().unwrap().label, "Travel");
            assert_eq!(got[4].group.as_ref().unwrap().label, "Other");
            // The flat picker never lists a lab-gated column.
            let flat = picker_options(&PICKER, i18n);
            assert_eq!(flat.iter().map(|o| o.id).collect::<Vec<_>>(), ["confidence"]);
            assert_eq!(flat[0], ColumnOption::new("confidence", "Confidence".into()));
        });
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- analyzer_kit::columns analyzer_kit::cells`
Expected: compile errors.

- [ ] **Step 3: `columns.rs` types**

```rust
use std::collections::BTreeSet;

use crate::components::control_bar::{ColumnOption, PickerHeading};
use super::formula::PriceSignal;
```

`ColumnKind` gains, after `ListingDc`:

```rust
    /// An alternative revenue signal on the sell world, as a column.
    RevSignal(PriceSignal),
    /// An alternative cost signal over the buy scope, as a column.
    CostSignal(PriceSignal),
    HopGain,
    HopWorlds,
```

Before `ColumnSpec`:

```rust
/// Where a column sits in the grouped Columns picker. Declaration order is
/// picker order.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PickerGroup {
    /// "Revenue · ‹sell world›".
    Revenue,
    /// "Cost · ‹buy scope›".
    Cost,
    Travel,
    Other,
}
```

`ColumnSpec` gains `pub group: PickerGroup,`. `CellCtx` becomes:

```rust
/// Per-render context a cell extractor may read.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CellCtx {
    pub now_unix: i64,
    /// The `analyzer-signal-columns` lab: the Price slot renders its
    /// listing-fallback tell only under it.
    pub signal_columns: bool,
    /// Cost signals the sub-craft cap left unpriced, by
    /// `PriceSignal::index`; their cells render "—" with the cap title.
    pub capped_cost: [bool; 4],
}
```

`ToolColumnMeta` gains, after `formula_cell_class`:

```rust
    /// The Labs token that gates this column. A gated column is absent
    /// from the flat picker and from the `?cols=` contract the page uses
    /// while the lab is off, so an old URL renders exactly as before.
    pub lab: Option<&'static str>,
```

`picker_options` filters `!c.id.is_empty() && c.lab.is_none()`. Add after it:

```rust
/// What the grouped picker needs beyond the table: the places named in the
/// two signal-group headings, the effective formula (for the "(= Price)" /
/// "(= Cost / unit)" suffix) and the cost signals the sub-craft cap left
/// unpriced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerContext {
    pub sell_place: String,
    pub buy_place: String,
    pub revenue: PriceSignal,
    pub cost: PriceSignal,
    pub capped: BTreeSet<PriceSignal>,
}

fn heading(
    group: PickerGroup,
    i18n: I18nContext<Locale, I18nKeys>,
    ctx: &PickerContext,
) -> PickerHeading {
    match group {
        PickerGroup::Revenue => PickerHeading {
            label: t_string!(i18n, analyzer_picker_group_place, name = t_string!(i18n, revenue).to_string(), place = ctx.sell_place.clone()).to_string(),
            title: None,
        },
        PickerGroup::Cost => PickerHeading {
            label: t_string!(i18n, analyzer_picker_group_place, name = t_string!(i18n, cost).to_string(), place = ctx.buy_place.clone()).to_string(),
            title: Some(t_string!(i18n, analyzer_picker_cost_group_title, place = ctx.buy_place.clone()).to_string()),
        },
        PickerGroup::Travel => PickerHeading {
            label: t_string!(i18n, analyzer_picker_group_travel).to_string(),
            title: None,
        },
        PickerGroup::Other => PickerHeading {
            label: t_string!(i18n, analyzer_picker_group_other).to_string(),
            title: None,
        },
    }
}

/// The picker with group headings: every optional column (lab-gated ones
/// included), sorted by group then table position, the selected signals
/// suffixed, the capped cost columns hinted (and, in the list, disabled
/// only while unchecked — a ticked column must stay untickable).
pub fn grouped_picker_options<T, M>(
    cols: &'static [ToolColumnMeta<T, M>],
    i18n: I18nContext<Locale, I18nKeys>,
    ctx: &PickerContext,
) -> Vec<ColumnOption> {
    let mut entries: Vec<(PickerGroup, usize, ColumnOption)> = cols
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.id.is_empty())
        .map(|(i, c)| {
            let mut label = (c.spec.label)(i18n);
            let mut disabled = false;
            let mut hint = None;
            match c.spec.kind {
                // Plain-key `t_string!` yields a `&'static str`: pass it
                // straight through (`&t_string!(..)` is `needless_borrow`).
                ColumnKind::RevSignal(s) if s == ctx.revenue => {
                    label.push(' ');
                    label.push_str(t_string!(i18n, analyzer_equals_price_slot));
                }
                ColumnKind::CostSignal(s) => {
                    if s == ctx.cost {
                        label.push(' ');
                        label.push_str(t_string!(i18n, analyzer_equals_cost_slot));
                    }
                    if ctx.capped.contains(&s) {
                        disabled = true;
                        hint = Some(t_string!(i18n, analyzer_picker_subcraft_cap_hint).to_string());
                    }
                }
                _ => {}
            }
            let option = ColumnOption {
                id: c.id,
                label,
                group: Some(heading(c.spec.group, i18n, ctx)),
                disabled,
                hint,
            };
            (c.spec.group, i, option)
        })
        .collect();
    entries.sort_by_key(|(g, i, _)| (*g, *i));
    entries.into_iter().map(|(_, _, o)| o).collect()
}
```

- [ ] **Step 4: `ColumnOption` fields (control_bar.rs)**

Replace `control_bar.rs:43-55`:

```rust
/// A group heading in the columns picker. Options carrying the same
/// heading (by label) are rendered under one heading.
#[derive(Clone, Debug, PartialEq)]
pub struct PickerHeading {
    pub label: String,
    /// Hover text on the heading ("Shows sale history for Aether (loads once)").
    pub title: Option<String>,
}

/// One column the picker can turn on or off.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnOption {
    /// Stable token, as persisted in `?cols=`.
    pub id: &'static str,
    pub label: String,
    /// `None` = the flat, ungrouped picker every page renders today.
    pub group: Option<PickerHeading>,
    /// Greyed out and not toggleable; `hint` says why.
    pub disabled: bool,
    pub hint: Option<String>,
}

impl ColumnOption {
    pub fn new(id: &'static str, label: String) -> Self {
        Self {
            id,
            label,
            group: None,
            disabled: false,
            hint: None,
        }
    }
}
```

- [ ] **Step 5: `cells.rs` variants and arms**

Imports: add `use thousands::Separable;`, `use crate::components::term_badge::TermRole;`, `use super::hop::HopGain;`, and extend the existing `use crate::components::gil::{Gil, GilOrDash};` to `{Gil, GilIcon, GilOrDash}`.

`CellValue` gains, before `Custom`:

```rust
    /// An alternative-signal amount: muted, with an always-present 10px
    /// sub-line holding the delta against the same-side formula input.
    /// `capped` = the sub-craft cap left it unpriced (a different title).
    MutedGil {
        amount: Option<i32>,
        pct: Option<f32>,
        side: TermRole,
        capped: bool,
    },
    /// A gil amount with an always-present note sub-line (the Price slot's
    /// "listing" fallback tell).
    GilWithNote {
        amount: i32,
        note: CellNote,
    },
    /// Hop gain / unit: signed gil, the word "needed", or the dash, in one
    /// shape; `daily_sales` feeds the gil/day title.
    Hop {
        gain: HopGain,
        daily_sales: f32,
    },
```

and, after the enum:

```rust
/// The sub-line under a [`CellValue::GilWithNote`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CellNote {
    None,
    /// The price fell back to a listing (the selected signal had no row on
    /// the sell world, or the sell world had no listing at all).
    ListingFallback,
}

/// "13.5k", "632", "1.5M": the gil/day figure in a hop title.
pub fn gil_per_day_label(gil: f32) -> String {
    let abs = gil.abs();
    if abs >= 1_000_000.0 {
        format!("{:.1}M", gil / 1_000_000.0)
    } else if abs >= 1_000.0 {
        format!("{:.1}k", gil / 1_000.0)
    } else {
        format!("{gil:.0}")
    }
}

fn signed_gil(g: i32) -> String {
    if g > 0 {
        format!("+{}", g.separate_with_commas())
    } else {
        g.separate_with_commas()
    }
}

const SUB_LINE: &str = "text-[10px] leading-3 text-[color:var(--color-text-muted)]";
```

`render_cell` gains three arms before `CellValue::Custom`:

```rust
        CellValue::MutedGil { amount, pct, side, capped } => {
            let amount = amount.filter(|a| *a > 0);
            let sub = pct
                .filter(|_| amount.is_some())
                .map(|p| format!("{p:+.0}%"))
                .unwrap_or_default();
            let title = if capped {
                t_string!(i18n, analyzer_alt_cost_capped_title).to_string()
            } else if side == TermRole::Revenue {
                t_string!(i18n, analyzer_alt_revenue_delta_title).to_string()
            } else {
                t_string!(i18n, analyzer_alt_cost_delta_title).to_string()
            };
            view! {
                <div role="cell" class=class title=title>
                    <div class="text-[color:var(--color-text-muted)]">
                        <GilOrDash amount=amount />
                    </div>
                    <div class=SUB_LINE>{sub}</div>
                </div>
            }
            .into_any()
        }
        CellValue::GilWithNote { amount, note } => {
            let note = match note {
                CellNote::None => String::new(),
                CellNote::ListingFallback => {
                    t_string!(i18n, analyzer_price_listing_fallback).to_string()
                }
            };
            view! {
                <div role="cell" class=class>
                    <Gil amount=amount />
                    <div class=SUB_LINE>{note}</div>
                </div>
            }
            .into_any()
        }
        CellValue::Hop { gain, daily_sales } => {
            let (text, has_amount, title) = match gain {
                HopGain::Gain(g) => (
                    signed_gil(g),
                    true,
                    Some(
                        t_string!(
                            i18n,
                            analyzer_hop_gain_title,
                            gil = gil_per_day_label(g as f32 * daily_sales),
                            rate = format!("{daily_sales:.1}")
                        )
                        .to_string(),
                    ),
                ),
                HopGain::Needed => (t_string!(i18n, analyzer_hop_needed).to_string(), false, None),
                HopGain::Unavailable => ("—".to_string(), false, None),
            };
            // One shape (the `GilOrDash` rule): the icon hides and the value
            // mutes by class; the arms never swap elements.
            view! {
                <div role="cell" class=class title=title>
                    <div class="flex flex-row items-center">
                        <span class=if has_amount { "inline-flex" } else { "hidden" }><GilIcon /></span>
                        <div class=if has_amount { "" } else { "text-[color:var(--color-text-muted)]" }>{text}</div>
                    </div>
                </div>
            }
            .into_any()
        }
```

- [ ] **Step 6: Fix the literals the compiler reports**

- `routes/recipe_analyzer.rs`: every `static SPEC_*: ColumnSpec` (15) gains `group: PickerGroup::Other,`; `RECIPE_BASE` gains `lab: None,`; `cell_ctx` becomes `CellCtx { now_unix: chrono::Utc::now().timestamp(), signal_columns: false, capped_cost: [false; 4] }` (Task 10 makes it live); the `custom` closure's `other => unreachable!(..)` arm already covers the new kinds. Import `PickerGroup` from `crate::analyzer_kit::columns`.
- `analyzer_kit/grid.rs` tests: the three `ColumnSpec` statics gain `group: PickerGroup::Other,`; `BASE` gains `lab: None,`; the three `CellCtx` literals (grid.rs:356, 398, 440) gain the two fields.
- `analyzer_kit/columns.rs` tests: `SPEC_ITEM` / `SPEC_PROFIT` / `SPEC_COST` (columns.rs:184-195) gain `group: PickerGroup::Other,`; the test `BASE` (:206-219) gains `lab: None,`; the `CellCtx { now_unix: 0 }` in `cell_extractors_are_plain_fn_pointers` (:295) becomes `CellCtx { now_unix: 0, signal_columns: false, capped_cost: [false; 4] }`.
- `analyzer_kit/cells.rs` tests: done in Step 1.
- The two `ColumnOption` struct-literal sites (they must compile for this task's gate): `routes/analyzer.rs:1382-1385` becomes `.map(|col| ColumnOption::new(col, col_label(col)))`; in `routes/currency_exchange.rs:702-717` each `ColumnOption { id: X, label: Y }` becomes `ColumnOption::new(X, Y)`.

- [ ] **Step 7: Run the kit tests**

Run: `cargo test -p ultros-app --lib -- analyzer_kit`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add ultros-frontend/ultros-app/src/analyzer_kit/columns.rs ultros-frontend/ultros-app/src/analyzer_kit/cells.rs ultros-frontend/ultros-app/src/analyzer_kit/grid.rs ultros-frontend/ultros-app/src/components/control_bar.rs ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs ultros-frontend/ultros-app/src/routes/analyzer.rs ultros-frontend/ultros-app/src/routes/currency_exchange.rs
git commit -m "feat(analyzer-kit): signal and hop column kinds, picker groups, lab-gated columns, muted/note/hop cells"
```

---

### Task 6: The grouped picker in `ControlBar`

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/control_bar.rs:324-373` (picker popover) and its tests

**Interfaces:**
- Consumes: `ColumnOption { id, label, group, disabled, hint }`, `PickerHeading` (Task 5).
- Produces: `#[component] pub fn ColumnsPickerList(columns: Signal<Vec<ColumnOption>>, visible_columns: Signal<HashSet<&'static str>>, #[prop(optional_no_strip)] on_toggle_column: Option<Callback<&'static str>>) -> impl IntoView` — the option list the popover renders; a heading is emitted whenever an option's `group` label differs from the previous option's; a capped option is disabled only while it is unchecked.

- [ ] **Step 1: Write the failing tests**

Append to `control_bar.rs`'s test module (create `#[cfg(test)] mod tests` if the file has none; it needs `use super::*; use leptos_i18n::context::init_i18n_context;`):

```rust
    fn render_list(cols: Vec<ColumnOption>) -> String {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            view! {
                <ColumnsPickerList
                    columns=Signal::derive(move || cols.clone())
                    visible_columns=Signal::derive(HashSet::new)
                    on_toggle_column=None
                />
            }
            .to_html()
        })
    }

    /// Ungrouped options render the flat list every page renders today:
    /// no headings, no disabled inputs, no titles.
    #[test]
    fn picker_list_without_groups_is_the_flat_list() {
        let html = render_list(vec![
            ColumnOption::new("tax", "Tax".into()),
            ColumnOption::new("vwap", "VWAP (7d)".into()),
        ]);
        assert_eq!(html.matches("<label").count(), 2, "{html}");
        assert_eq!(
            html.matches("<label class=\"inline-flex items-center gap-2 cursor-pointer text-[color:var(--color-text)]\"><input type=\"checkbox\" class=\"accent-brand-300\"").count(),
            2,
            "{html}"
        );
        assert!(html.contains("<span>Tax</span>"), "{html}");
        assert!(!html.contains("basis-full"), "{html}");
        assert!(!html.contains("disabled"), "{html}");
        assert!(!html.contains("title="), "{html}");
    }

    #[test]
    fn picker_list_renders_group_headings_once_and_disables_capped_options() {
        let rev = PickerHeading { label: "Revenue · Gilgamesh".into(), title: None };
        let cost = PickerHeading { label: "Cost · Aether".into(), title: Some("loads once".into()) };
        let html = render_list(vec![
            ColumnOption { group: Some(rev.clone()), ..ColumnOption::new("rev-sale-min", "Sale minimum (7d)".into()) },
            ColumnOption { group: Some(rev), ..ColumnOption::new("rev-sale-avg", "Sale average (7d)".into()) },
            ColumnOption { group: Some(cost.clone()), ..ColumnOption::new("cost-sale-min", "Sale minimum (7d)".into()) },
            ColumnOption { group: Some(cost), disabled: true, hint: Some("capped".into()), ..ColumnOption::new("cost-sale-avg", "Sale average (7d)".into()) },
        ]);
        assert_eq!(html.matches("Revenue · Gilgamesh").count(), 1, "{html}");
        assert_eq!(html.matches("Cost · Aether").count(), 1, "{html}");
        assert!(html.contains("title=\"loads once\""), "{html}");
        assert_eq!(html.matches("basis-full").count(), 2, "{html}");
        assert_eq!(html.matches("disabled").count(), 1, "{html}");
        assert!(html.contains("title=\"capped\""), "{html}");
        // Headings precede their options.
        let rev_at = html.find("Revenue · Gilgamesh").unwrap();
        let first_opt = html.find("Sale minimum (7d)").unwrap();
        assert!(rev_at < first_opt, "{html}");
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- control_bar`
Expected: compile error, `ColumnsPickerList` not found.

- [ ] **Step 3: Extract and extend the list**

Add the component to `control_bar.rs` (above `ControlBar`):

```rust
/// The picker's option list. An option's `group` heading is rendered once,
/// where it first differs from the previous option's, so a page that passes
/// ungrouped options gets the flat list it always had. Options are a `Vec`
/// in the page's order — nothing here iterates a map.
#[component]
pub fn ColumnsPickerList(
    #[prop(into)] columns: Signal<Vec<ColumnOption>>,
    #[prop(into)] visible_columns: Signal<HashSet<&'static str>>,
    // `optional_no_strip`: `optional` on an `Option<T>` field strips the
    // Option from the builder setter (leptos_macro `component.rs:1033`),
    // which would reject both the bar's pass-through and the test's `None`.
    #[prop(optional_no_strip)]
    on_toggle_column: Option<Callback<&'static str>>,
) -> impl IntoView {
    move || {
        let mut out: Vec<AnyView> = Vec::new();
        let mut last_heading: Option<String> = None;
        for col in columns.get() {
            if let Some(heading) = &col.group
                && last_heading.as_deref() != Some(heading.label.as_str())
            {
                last_heading = Some(heading.label.clone());
                let label = heading.label.clone();
                out.push(match heading.title.clone() {
                    Some(title) => view! {
                        <span class="basis-full text-xs uppercase tracking-wide text-[color:var(--color-text-muted)] mt-1" title=title>{label}</span>
                    }
                    .into_any(),
                    None => view! {
                        <span class="basis-full text-xs uppercase tracking-wide text-[color:var(--color-text-muted)] mt-1">{label}</span>
                    }
                    .into_any(),
                });
            }
            let id = col.id;
            let toggle = move |_| {
                if let Some(toggle) = on_toggle_column {
                    toggle.run(id);
                }
            };
            // A ticked column is never locked: the cap greys an unchecked
            // capped entry, and only hints a checked one.
            let disabled = col.disabled && !visible_columns.get().contains(id);
            out.push(if disabled || col.hint.is_some() {
                let hint = col.hint.clone().unwrap_or_default();
                view! {
                    <label class="inline-flex items-center gap-2 cursor-not-allowed opacity-60 text-[color:var(--color-text)]" title=hint>
                        <input
                            type="checkbox"
                            class="accent-brand-300"
                            disabled=disabled
                            prop:checked=move || visible_columns.get().contains(id)
                            on:change=toggle
                        />
                        <span>{col.label.clone()}</span>
                    </label>
                }
                .into_any()
            } else {
                view! {
                    <label class="inline-flex items-center gap-2 cursor-pointer text-[color:var(--color-text)]">
                        <input
                            type="checkbox"
                            class="accent-brand-300"
                            prop:checked=move || visible_columns.get().contains(id)
                            on:change=toggle
                        />
                        <span>{col.label.clone()}</span>
                    </label>
                }
                .into_any()
            });
        }
        out
    }
}
```

In the popover (`control_bar.rs:332-355`) replace the `{move || { columns.get().into_iter().map(..).collect_view() }}` block with:

```rust
                                <ColumnsPickerList
                                    columns=columns
                                    visible_columns=visible_columns
                                    on_toggle_column=on_toggle_column
                                />
```

(`columns` and `visible_columns` are the bar's existing `Signal`s; `on_toggle_column` is its `Option<Callback<&'static str>>` prop, passed through as the `Option` it is — that is what `optional_no_strip` on the list's prop is for.)

- [ ] **Step 4: Check the struct-literal sites are already converted**

Task 5 converted `routes/analyzer.rs:1382-1385` and `routes/currency_exchange.rs:702-717` to `ColumnOption::new`; `grep -rn "ColumnOption {" ultros-frontend/ultros-app/src --include=*.rs` must now hit only `control_bar.rs` (the struct and the tests) and `analyzer_kit/columns.rs`.

- [ ] **Step 5: Run the tests and the crate build**

Run: `cargo test -p ultros-app --lib -- control_bar`
Expected: PASS (2 new tests).
Run: `cargo check -p ultros-app --features ssr`
Expected: OK (the two routes compile with `::new`).

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/control_bar.rs
git commit -m "feat(control-bar): grouped columns picker with headings, disabled options and hints"
```

---

### Task 7: The `trailing` header slot and the grid's per-column header extras (the "use" pill)

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/sort_header.rs:190-273` (`SortableHeaderCell`) and its tests
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/grid.rs` (`header_cell`, `AnalyzerGrid` props, new types) and its tests

**Interfaces:**
- Consumes: `ColumnKind` (Task 5), `Icon` (`components/icon.rs`), `icondata::AiCalculatorOutlined`.
- Produces:
  - `SortableHeaderCell` prop `#[prop(optional)] trailing: Option<ViewFn>` — rendered inside the sub-label line after the text; ignored without `sub_label`. Unset = byte-identical markup.
  - In `grid.rs`: `pub struct HeaderPill { pub aria: String, pub pressed: bool }`, `pub struct HeaderLine2 { pub sub_label: String, pub pill: HeaderPill }`, `pub struct HeaderExtra { pub title: String, pub line2: Option<HeaderLine2> }`, `pub struct HeaderExtras { pub by_kind: HashMap<ColumnKind, HeaderExtra> }` (all Clone, Debug, PartialEq, Eq; `HeaderExtras: Default`).
  - `AnalyzerGrid` props `#[prop(optional, into)] extras: Option<Signal<HeaderExtras>>`, `#[prop(optional)] on_pill: Option<Callback<ColumnKind>>` and `#[prop(optional)] lab_columns: bool` — when false (the default) lab-gated columns are left out of the header at build time, so the flag-off header carries no extra `<!>` markers.
  - The pill: `<button type="button" aria-pressed="true|false" aria-label=… disabled?>` with the calculator icon and the `analyzer_use_pill` word; clicking runs `on_pill(kind)`.

- [ ] **Step 1: Write the failing tests**

sort_header.rs tests:

```rust
    /// The pill slot lives inside the sub-label line, after the text, as a
    /// separate focus stop; unset it adds no markup to a two-line header.
    #[test]
    fn trailing_renders_after_the_sub_label_and_adds_nothing_when_unset() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let with = view! {
                <SortableHeaderCell
                    mode=Col::Cost
                    label="Sale median (7d)"
                    class="w-40"
                    sort_mode=Signal::derive(|| None::<Col>)
                    sort_dir=Signal::derive(|| None::<SortDir>)
                    sub_label=Signal::derive(|| "7d median · Aether".to_string())
                    trailing=ViewFn::from(|| view! { <button type="button">"use"</button> }.into_any())
                />
            }
            .to_html();
            let sub_at = with.find("7d median · Aether").unwrap();
            let btn_at = with.find("<button").unwrap();
            assert!(sub_at < btn_at, "{with}");
            assert!(with.contains("flex items-center gap-1"), "{with}");
            let without = view! {
                <SortableHeaderCell
                    mode=Col::Cost
                    label="Sale median (7d)"
                    class="w-40"
                    sort_mode=Signal::derive(|| None::<Col>)
                    sort_dir=Signal::derive(|| None::<SortDir>)
                    sub_label=Signal::derive(|| "7d median · Aether".to_string())
                />
            }
            .to_html();
            assert!(!without.contains("<button"), "{without}");
            assert!(!without.contains("flex items-center gap-1"), "{without}");
            assert!(
                without.contains("text-[color:var(--color-text-muted)] truncate max-w-full\">7d median · Aether</div>"),
                "{without}"
            );
        });
    }
```

grid.rs tests (the module already has `Row`, `Col`, `COLS`; add a signal column and drive `header_cell` directly):

```rust
    fn label_d(_: I18nContext<Locale, I18nKeys>) -> String {
        "Sale median (7d)".into()
    }
    static D: ColumnSpec = ColumnSpec {
        kind: ColumnKind::CostSignal(crate::analyzer_kit::formula::PriceSignal::SaleMedian),
        label: label_d,
        group: crate::analyzer_kit::columns::PickerGroup::Cost,
    };
    static SIGNAL_COL: ToolColumnMeta<Row, Col> = ToolColumnMeta {
        spec: &D,
        id: "cost-sale-median",
        sort_id: "cost-sale-median",
        sort: sortability_for(Layer::Computed, Some(Col::Profit)),
        header_class: "w-40 px-3 py-2 leading-tight",
        cell_class: "w-40",
        lab: Some("analyzer-signal-columns"),
        ..BASE
    };

    #[test]
    fn header_extras_render_title_sub_label_and_pill() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let i18n = crate::i18n::use_i18n();
            let kind = SIGNAL_COL.spec.kind;
            let extras = |pressed: bool| {
                let mut by_kind = HashMap::new();
                by_kind.insert(kind, HeaderExtra {
                    title: "The middle price".into(),
                    line2: Some(HeaderLine2 {
                        sub_label: "7d median · Aether".into(),
                        pill: HeaderPill { aria: "Use Sale median (7d) as the cost in Profit".into(), pressed },
                    }),
                });
                Signal::derive(move || HeaderExtras { by_kind: by_kind.clone() })
            };
            let clicked = RwSignal::new(None::<ColumnKind>);
            let on_pill = Callback::new(move |k| clicked.set(Some(k)));
            let none = Signal::derive(|| None::<Col>);
            let none_dir = Signal::derive(|| None::<SortDir>);
            let off = header_cell(&SIGNAL_COL, none, none_dir, i18n, None, Some(extras(false)), Some(on_pill)).to_html();
            assert!(off.contains("title=\"The middle price\""), "{off}");
            assert!(off.contains("7d median · Aether"), "{off}");
            assert!(off.contains("aria-pressed=\"false\""), "{off}");
            assert!(off.contains("aria-label=\"Use Sale median (7d) as the cost in Profit\""), "{off}");
            assert!(off.contains(">use<"), "{off}");
            assert!(!off.contains("disabled"), "{off}");
            let on = header_cell(&SIGNAL_COL, none, none_dir, i18n, None, Some(extras(true)), Some(on_pill)).to_html();
            assert!(on.contains("aria-pressed=\"true\"") && on.contains("disabled"), "{on}");
            // No extras: the plain sortable header, exactly as before.
            let plain = header_cell(&SIGNAL_COL, none, none_dir, i18n, None, None, None).to_html();
            assert!(!plain.contains("<button") && !plain.contains("title="), "{plain}");
            // The flag-off page passes `Some(empty map)`: identical by construction.
            let empty = header_cell(&SIGNAL_COL, none, none_dir, i18n, None, Some(Signal::derive(HeaderExtras::default)), Some(on_pill)).to_html();
            assert_eq!(empty, plain, "an empty extras map is the flag-off page path");
        });
    }

    /// A hidden optional column still writes a `<!>` marker into the header
    /// (an `Option` child), so the flag-off header would grow by one marker
    /// per lab column; `lab_columns=false` drops them at build time.
    #[test]
    fn lab_columns_are_absent_from_the_header_unless_enabled() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let render = |cols: &'static [ToolColumnMeta<Row, Col>], lab: bool, visible: &'static [&'static str]| {
                view! {
                    <AnalyzerGrid
                        columns=cols
                        rows=Signal::derive(|| vec![(0usize, Row(1))])
                        visible_cols=Signal::derive(move || visible.iter().copied().collect::<HashSet<_>>())
                        sort_mode=Signal::derive(|| None::<Col>)
                        sort_dir=Signal::derive(|| None::<SortDir>)
                        ctx=Signal::derive(|| CellCtx { now_unix: 0, signal_columns: false, capped_cost: [false; 4] })
                        custom=Arc::new(|_: &Row, _: ColumnKind, class: &'static str| view! { <div role="cell" class=class>"x"</div> }.into_any())
                        layout=GridLayout { viewport_height: 100.0, row_height: 10.0, header_height: 10.0, overscan: 1 }
                        header_class="h"
                        row_class=|_| "r"
                        lab_columns=lab
                    />
                }
                .to_html()
            };
            let base = render(&COLS, false, &[]);
            let with_lab_col_off = render(&COLS_PLUS, false, &[]);
            assert_eq!(with_lab_col_off, base, "a hidden lab column must add nothing to the flag-off header");
            let with_lab_col_on = render(&COLS_PLUS, true, &["cost-sale-median"]);
            assert!(with_lab_col_on.contains("Sale median (7d)"), "{with_lab_col_on}");
        });
    }
```

`COLS_PLUS` is a second `static [ToolColumnMeta<Row, Col>; 4]` holding the three `COLS` entries plus the `SIGNAL_COL` entry, each written out again as a literal (a `static` cannot be read inside another `static`'s initializer, so the entries are duplicated verbatim, `..BASE` spreads included).

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- sort_header analyzer_kit::grid`
Expected: compile errors (`trailing`, `HeaderExtra`, `header_cell` arity).

- [ ] **Step 3: The `trailing` prop**

`SortableHeaderCell` gains, after `emphasized`:

```rust
    /// Line-2 content after the sub-label (the "use" pill). Rendered only
    /// alongside `sub_label`; DOM order is the `SortHeader` `<a>` first,
    /// then the sub-label line `<div>` holding the text `<span>` and this
    /// `<button>` — two focus stops, never nested.
    #[prop(optional)]
    trailing: Option<ViewFn>,
```

and the sub-label branch becomes:

```rust
                    {sub_label.map(|s| match trailing {
                        None => view! {
                            <div class="text-[10px] leading-3 font-normal normal-case text-[color:var(--color-text-muted)] truncate max-w-full">{move || s.get()}</div>
                        }
                        .into_any(),
                        Some(trailing) => view! {
                            <div class="text-[10px] leading-3 font-normal normal-case text-[color:var(--color-text-muted)] flex items-center gap-1 max-w-full">
                                <span class="truncate">{move || s.get()}</span>
                                {trailing.run()}
                            </div>
                        }
                        .into_any(),
                    })}
```

(The `None` arm is the current markup, character for character.)

- [ ] **Step 4: The grid's extras**

Add to `grid.rs` (after `MarkLabels`):

```rust
/// The "use" pill on an alternative-signal header: pressed when that
/// signal is the selected input (the button is then disabled).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderPill {
    pub aria: String,
    pub pressed: bool,
}

/// Line 2 of an alternative-signal header: `‹short signal› · ‹place›` (or
/// "(= Cost / unit)") plus the pill.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderLine2 {
    pub sub_label: String,
    pub pill: HeaderPill,
}

/// What a page hangs off an unmarked sortable header: a hover title and,
/// for the signal columns, line 2. Columns with no entry render exactly as
/// they did before this existed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderExtra {
    pub title: String,
    pub line2: Option<HeaderLine2>,
}

/// Header extras by column kind. Looked up by key only, never iterated.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct HeaderExtras {
    pub by_kind: HashMap<ColumnKind, HeaderExtra>,
}

const PILL_OFF: &str = "inline-flex items-center gap-0.5 shrink-0 rounded-full border border-[color:var(--color-outline)] px-1.5 text-[10px] leading-3 font-medium text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)] hover:border-[color:var(--brand-ring)]";
const PILL_ON: &str = "inline-flex items-center gap-0.5 shrink-0 rounded-full border border-[color:var(--brand-ring)] bg-[color:color-mix(in_srgb,var(--brand-ring)_20%,transparent)] px-1.5 text-[10px] leading-3 font-medium text-[color:var(--brand-fg)]";

/// `<button type=button aria-pressed>`: pressing it writes one URL param
/// on the page (`on_pill`), which moves the badge, tint and sub-label to
/// the slot header; the pressed column stays on screen as a muted
/// duplicate with its pill filled and disabled.
fn pill_view(
    kind: ColumnKind,
    pill: HeaderPill,
    on_pill: Option<Callback<ColumnKind>>,
    i18n: I18nContext<Locale, I18nKeys>,
) -> AnyView {
    let pressed = pill.pressed;
    view! {
        <button
            type="button"
            class=if pressed { PILL_ON } else { PILL_OFF }
            aria-pressed=if pressed { "true" } else { "false" }
            aria-label=pill.aria
            disabled=pressed
            on:click=move |ev| {
                ev.prevent_default();
                ev.stop_propagation();
                if let Some(cb) = on_pill {
                    cb.run(kind);
                }
            }
        >
            <Icon icon=i::AiCalculatorOutlined width="0.9em" height="0.9em" />
            <span>{t_string!(i18n, analyzer_use_pill).to_string()}</span>
        </button>
    }
    .into_any()
}
```

Imports: `use crate::components::icon::Icon; use icondata as i;`.

`header_cell` gains two parameters, `extras: Option<Signal<HeaderExtras>>, on_pill: Option<Callback<ColumnKind>>`, and its `(Sortability::By(mode), None)` arm becomes:

```rust
        (Sortability::By(mode), None) => {
            let kind = col.spec.kind;
            let extra = extras.and_then(|e| e.with(|e| e.by_kind.get(&kind).cloned()));
            match extra {
                None => view! {
                    <SortableHeaderCell mode=mode label=label_fn(i18n) class=col.header_class sort_mode sort_dir />
                }
                .into_any(),
                Some(HeaderExtra { title, line2: None }) => view! {
                    <SortableHeaderCell mode=mode label=label_fn(i18n) title=title class=col.header_class sort_mode sort_dir />
                }
                .into_any(),
                Some(HeaderExtra { title, line2: Some(HeaderLine2 { sub_label, pill }) }) => view! {
                    <SortableHeaderCell
                        mode=mode
                        label=label_fn(i18n)
                        title=title
                        class=format!("{} truncate", col.header_class)
                        sort_mode
                        sort_dir
                        sub_label=Signal::derive(move || sub_label.clone())
                        trailing=ViewFn::from(move || pill_view(kind, pill.clone(), on_pill, i18n))
                    />
                }
                .into_any(),
            }
        }
```

`AnalyzerGrid` gains, after `marks`:

```rust
    /// Per-kind header titles and line-2 (sub-label + "use" pill) for the
    /// unmarked sortable columns. `None` leaves every header as it was.
    #[prop(optional, into)]
    extras: Option<Signal<HeaderExtras>>,
    /// Runs when a header pill is pressed, with the column's kind.
    #[prop(optional)]
    on_pill: Option<Callback<ColumnKind>>,
    /// Whether lab-gated columns (`lab.is_some()`) are part of this mount.
    /// Off, they are dropped from the header at build time: a hidden
    /// optional column still writes a `<!>` marker (an `Option` child), so
    /// a `?cols=` contract alone cannot keep the flag-off header
    /// byte-identical. The page remounts the grid on a lab flip.
    #[prop(optional)]
    lab_columns: bool,
```

Both `header_cell(..)` calls in the header pass `, extras, on_pill`, and the header's `columns.iter().map(|col| {` becomes `columns.iter().filter(|col| col.lab.is_none() || lab_columns).map(|col| {` (the row side already filters by `visible_cols` and needs no change).

- [ ] **Step 5: Run the tests**

Run: `cargo test -p ultros-app --lib -- sort_header analyzer_kit::grid`
Expected: PASS, including the pre-existing `header_cell_renders_badge_sub_label_and_emphasis` marker-count assertion and the grid's marked-header tests.

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/sort_header.rs ultros-frontend/ultros-app/src/analyzer_kit/grid.rs
git commit -m "feat(ui): SortableHeaderCell trailing slot and grid header extras with the use pill"
```

---

### Task 8: The priced row — alternative signals, the fallback tell, unpriced count, hop and worlds

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:76-101` (`RecipeProfitData`), `:996-1189` (`PriceInputs`, `price_rows`), `:1-74` (imports), the `mod test` helpers `run` (`:3299`) and `row` (`:3395`), plus new tests

**Interfaces:**
- Consumes: `NeededSignals` (Task 3), `stat_only_cheapest`, `SaleStat`, `PriceSignal::{ALL, index}` (Task 3), `hop_gain`, `worlds_to_visit`, `HopGain`, `WorldsToVisit` (Task 4), `CostBreakdown.unpriced_market_lines` (Task 2).
- Produces on `RecipeProfitData`: `cost_alt: [Option<i32>; 4]`, `rev_alt: [Option<i32>; 4]` (both by `PriceSignal::index`), `revenue_fell_back: bool`, `unpriced: u16`, `hop: Option<HopGain>`, `worlds: Option<WorldsToVisit>`. On `PriceInputs`: `needs: &'a NeededSignals`, `sell_stats_loaded: bool`, `home_world_id: i32`, `dc_of: &'a dyn Fn(i32) -> Option<&'a str>`. Read by Tasks 9 and 10.

- [ ] **Step 1: Imports**

Add / extend at the top of `recipe_analyzer.rs`:

```rust
use crate::analyzer_kit::formula::{FormulaMarks, PriceSignal, ProfitFormula, RoiMath, SaleStat, per_unit_cost, profit_line};
use crate::analyzer_kit::hop::{HopGain, WorldsToVisit, hop_gain, worlds_to_visit};
use crate::analyzer_kit::needed::{BodyRole, NeededSignals, RecipeNeeds, SALE_STATS_WINDOW_DAYS, needed_bodies};
use crate::analyzer_kit::signals::{PriceLookup, SignalView, StatsIndex, stat_only_cheapest, stats_index};
use crate::components::crafting_cost::{CostBreakdown, CraftingCostOptions, EmptyOnHand, OnHand, ShardsMode, compute_cost, vendor_price_map};
```

- [ ] **Step 2: Write the failing tests**

In `mod test`, replace `run` with a parameterised runner and add tests. `BuyScope`, `SignalWants`, `needed_signals` need `use crate::analyzer_kit::needed::{SignalWants, needed_signals};` in the test module.

```rust
    struct RunOpts {
        outliers: bool,
        needs: NeededSignals,
        sell_listings: bool,
        sell_stats: bool,
        scope: Option<BuyScope>,
    }

    impl Default for RunOpts {
        fn default() -> Self {
            Self {
                outliers: false,
                needs: NeededSignals::default(),
                sell_listings: true,
                sell_stats: true,
                scope: None,
            }
        }
    }

    fn run_with(cost: PriceSignal, revenue: PriceSignal, o: &RunOpts) -> Vec<RecipeProfitData> {
        let data = xiv_gen_db::data();
        let recipes = fixture_recipes();
        let (buy, sell, stats) = fixture(&recipes);
        let index = stats_index(&stats);
        let empty_index = StatsIndex::new();
        let by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        let raw_sales = HashMap::new();
        let levels = CrafterLevels::default(); // 100 in every job
        // Fixture geography: buy NQ on world 1 (Aether), buy HQ on world 2
        // (Primal), the sell world is 3 (Aether). A closure, not a fn item:
        // a fn item's `Output` is fixed to `Option<&'static str>` and cannot
        // unsize into `dyn Fn(i32) -> Option<&'a str>` while `'a` borrows
        // the locals above.
        let fixture_dc = |w: i32| match w {
            1 | 3 => Some("Aether"),
            2 => Some("Primal"),
            _ => None,
        };
        let inp = PriceInputs {
            recipes: &recipes,
            recipe_level_tables: &data.recipe_level_tables,
            recipes_by_output: &by_output,
            buy_listings: &buy,
            sell_listings: o.sell_listings.then_some(&sell),
            buy_stats: Some(&index),
            sell_stats: if o.sell_stats { &index } else { &empty_index },
            raw_sales: &raw_sales,
            formula: ProfitFormula::recipe_from_query(Some(cost), Some(revenue), o.scope),
            levels: &levels,
            job_filter: None,
            use_subcrafts: false,
            require_hq: false,
            filter_outliers: o.outliers,
            shards: ShardsMode::ExcludeShards,
            on_hand: None,
            needs: &o.needs,
            sell_stats_loaded: o.sell_stats,
            home_world_id: 3,
            dc_of: &fixture_dc,
        };
        price_rows(&inp).0
    }

    fn run(cost: PriceSignal, revenue: PriceSignal, outliers: bool) -> Vec<RecipeProfitData> {
        let f = ProfitFormula::recipe_from_query(Some(cost), Some(revenue), None);
        run_with(
            cost,
            revenue,
            &RunOpts {
                outliers,
                needs: needed_signals(&f, &SignalWants::default(), false),
                ..RunOpts::default()
            },
        )
    }

    fn everything_wanted(cost: PriceSignal) -> NeededSignals {
        let f = ProfitFormula::recipe_from_query(Some(cost), None, None);
        let wants = SignalWants {
            visible_cost: PriceSignal::ALL.to_vec(),
            sort_cost: None,
            hop: true,
            worlds: true,
        };
        needed_signals(&f, &wants, false)
    }

    /// The drop rule, ROI and the row set are the selected pair's alone;
    /// alternative columns are informational.
    #[test]
    fn alt_columns_never_change_row_membership() {
        let base = run(PriceSignal::ListingMin, PriceSignal::ListingMin, false);
        let full = run_with(
            PriceSignal::ListingMin,
            PriceSignal::ListingMin,
            &RunOpts { needs: everything_wanted(PriceSignal::ListingMin), ..RunOpts::default() },
        );
        assert_eq!(base.len(), full.len());
        for (a, b) in base.iter().zip(&full) {
            assert_eq!(a.recipe.key_id, b.recipe.key_id);
            assert_eq!((a.profit, a.cost, a.market_price, a.return_on_investment), (b.profit, b.cost, b.market_price, b.return_on_investment));
            assert_eq!(b.cost_alt[PriceSignal::ListingMin.index()], Some(b.cost), "the selected run is its own alt");
        }
        assert!(full.iter().any(|r| r.cost_alt[PriceSignal::SaleMedian.index()].is_some()));
        assert!(base.iter().all(|r| r.cost_alt[PriceSignal::SaleMedian.index()].is_none()
            && r.hop.is_none()
            && r.worlds.is_none()));
    }

    /// An alternative cost column equals what selecting that signal would
    /// have priced the same row at.
    #[test]
    fn cost_alt_matches_a_dedicated_run() {
        let full = run_with(
            PriceSignal::ListingMin,
            PriceSignal::ListingMin,
            &RunOpts { needs: everything_wanted(PriceSignal::ListingMin), ..RunOpts::default() },
        );
        let median = run(PriceSignal::SaleMedian, PriceSignal::ListingMin, false);
        let by_key: HashMap<i32, i32> = median.iter().map(|r| (r.recipe.key_id.0, r.cost)).collect();
        let mut compared = 0;
        for r in &full {
            if let Some(cost) = by_key.get(&r.recipe.key_id.0) {
                assert_eq!(r.cost_alt[PriceSignal::SaleMedian.index()], Some(*cost), "recipe {}", r.recipe.key_id.0);
                compared += 1;
            }
        }
        assert!(compared > 20, "only {compared} rows compared");
    }

    /// Alternative revenue columns are the bare sell-world statistic (or
    /// listing): nothing falls back, so no sell world means "—" everywhere.
    #[test]
    fn revenue_alt_columns_are_none_without_sell_world_data() {
        let none = run_with(
            PriceSignal::ListingMin,
            PriceSignal::ListingMin,
            &RunOpts { sell_listings: false, sell_stats: false, ..RunOpts::default() },
        );
        assert!(none.len() > 20);
        assert!(none.iter().all(|r| r.rev_alt == [None; 4] && r.revenue_fell_back));
        let some = run(PriceSignal::ListingMin, PriceSignal::ListingMin, false);
        for r in &some {
            let out = r.recipe.item_result;
            let nq = 100 + (out % 97) * 7;
            assert_eq!(r.rev_alt[PriceSignal::ListingMin.index()], Some(nq * 12 / 10), "sell listing, no fallback");
            let expect_stat = out % 3 == 0;
            assert_eq!(r.rev_alt[PriceSignal::SaleMedian.index()], expect_stat.then_some(nq + 5), "recipe {}", r.recipe.key_id.0);
        }
    }

    /// The Price slot's "listing" tell: set exactly when the number shown
    /// is not the selected signal on the sell world.
    #[test]
    fn price_fallback_tell_marks_buy_scope_prices() {
        let rows = run(PriceSignal::ListingMin, PriceSignal::ListingMin, false);
        let mut fell = 0;
        for r in &rows {
            let nq = 100 + (r.recipe.item_result % 97) * 7;
            let sell_price = nq * 12 / 10;
            // The buy scope's HQ listing (nq + 50) undercuts the sell world
            // once nq > 250: that price came from the buy scope.
            assert_eq!(r.revenue_fell_back, r.market_price != sell_price, "recipe {}", r.recipe.key_id.0);
            fell += usize::from(r.revenue_fell_back);
        }
        assert!(fell > 0 && fell < rows.len(), "{fell} of {}", rows.len());
    }

    #[test]
    fn hop_and_worlds_are_computed_only_when_needed() {
        let full = run_with(
            PriceSignal::ListingMin,
            PriceSignal::ListingMin,
            &RunOpts { needs: everything_wanted(PriceSignal::ListingMin), ..RunOpts::default() },
        );
        assert!(full.iter().all(|r| r.hop.is_some() && r.worlds.is_some()));
        // The sell world lists only outputs: every market ingredient is
        // missing at home, so those rows read "needed". Depends on game
        // data: some kept row needs a non-vendor, non-shard ingredient that
        // is not one of the 300 fixture outputs (true for every pack so
        // far; re-check on a game-data bump).
        assert!(full.iter().any(|r| r.hop == Some(HopGain::Needed)));
        // Cheapest ingredient listings sit on buy world 1 (NQ beats HQ + 50).
        let with_trip: Vec<&RecipeProfitData> = full.iter().filter(|r| !r.worlds.as_ref().unwrap().worlds.is_empty()).collect();
        assert!(!with_trip.is_empty());
        for r in with_trip {
            let w = r.worlds.as_ref().unwrap();
            assert!(w.worlds.iter().all(|(id, n)| *id == 1 && *n > 0), "{w:?}");
            assert_eq!(w.dcs, 1);
        }
        // Buy from = This world only: no trip to compute.
        let home_only = run_with(
            PriceSignal::ListingMin,
            PriceSignal::ListingMin,
            &RunOpts { needs: everything_wanted(PriceSignal::ListingMin), scope: Some(BuyScope::World), ..RunOpts::default() },
        );
        assert!(home_only.iter().all(|r| r.hop == Some(HopGain::Unavailable) && r.worlds.is_none()));
        // Unpriced under the selected signal is carried on the row.
        assert!(full.iter().all(|r| r.unpriced == 0), "the fixture lists every ingredient");
    }
```

Update `row()` (the `filter_and_sort` helper) with the new fields:

```rust
            confidence: ConfidenceBand::Unknown,
            cost_alt: [None; 4],
            rev_alt: [None; 4],
            revenue_fell_back: false,
            unpriced: 0,
            hop: None,
            worlds: None,
```

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- routes::recipe_analyzer::test`
Expected: compile errors (fields missing).

- [ ] **Step 4: The row and the inputs**

`RecipeProfitData` gains, after `confidence`:

```rust
    /// Per-unit cost under each cost signal that was run, by
    /// `PriceSignal::index`; `None` = not run (not needed, capped, or a
    /// sale signal with no buy-scope body).
    cost_alt: [Option<i32>; 4],
    /// The bare sell-world statistic (or listing) per revenue signal, no
    /// fallback; `None` = no row.
    rev_alt: [Option<i32>; 4],
    /// `market_price` is not the selected signal on the sell world: the
    /// stat was missing, or the listing fell back to the buy scope.
    revenue_fell_back: bool,
    /// Marketable ingredient lines no listing priced, under the selected
    /// signal. They cost 0 here (row membership unchanged) and are said so.
    unpriced: u16,
    /// `None` when Hop gain was not wanted.
    hop: Option<HopGain>,
    /// `None` when Worlds to visit was not wanted, or Buy from = This world.
    worlds: Option<WorldsToVisit>,
```

`PriceInputs` gains, after `on_hand`:

```rust
    /// Which cost signals to run per recipe, and whether hop / worlds are
    /// wanted. The selected signal is always in the set.
    needs: &'a NeededSignals,
    /// Whether the sell-world stats body was fetched: hop's home side
    /// prices from it under a sale cost signal, else from the listing.
    sell_stats_loaded: bool,
    /// The sell world's id (0 while unresolved) — the "home" that Worlds
    /// to visit excludes.
    home_world_id: i32,
    /// World id → datacenter name, for Worlds to visit.
    dc_of: &'a dyn Fn(i32) -> Option<&'a str>,
```

- [ ] **Step 5: `price_rows`**

Replace the body from `// Ingredients price over the buy scope` through the end of the function. The function now also counts its `compute_cost` calls (a `Cell<u32>` the `cost_run` closure bumps) and returns them beside the rows — `fn price_rows(inp: &PriceInputs<'_>) -> (Vec<RecipeProfitData>, u32)` — so the debug timing log reports real calls rather than `needs.cost.len()`; the memo destructures the pair.

```rust
    let runs_done = std::cell::Cell::new(0u32);
    let selected = inp.formula.cost_signal();
    let scope_is_home = inp.formula.buy_scope() == BuyScope::World;
    // A buy-scope view under `signal`: the listing, or the stat over it.
    // Same two layers the cloned `override_listings` / `overlay_sale_stats`
    // maps used to build, now evaluated per lookup.
    let scope_view = |signal: PriceSignal| SignalView {
        over: None,
        base: inp.buy_listings,
        stats: signal
            .sale_stat()
            .and_then(|stat| inp.buy_stats.map(|idx| (idx, stat))),
    };
    let ingredient_view = scope_view(selected);
    let revenue_view = SignalView {
        over: inp.sell_listings,
        base: inp.buy_listings,
        stats: inp
            .formula
            .revenue_signal()
            .sale_stat()
            .map(|stat| (inp.sell_stats, stat)),
    };
    // Hop's home side: the sell world alone (deliberately not layered over
    // the buy scope, or an ingredient with no home listing would be priced
    // at the scope price and zero the gain for exactly the ingredients
    // that force the trip), under the selected cost signal when its
    // sell-world body is here, else the listing pass on both sides.
    let hop_signal = if inp.sell_stats_loaded {
        selected
    } else {
        PriceSignal::ListingMin
    };
    let home_view = inp.sell_listings.map(|sell| SignalView {
        over: None,
        base: sell,
        stats: hop_signal.sale_stat().map(|stat| (inp.sell_stats, stat)),
    });

    for recipe in inp.recipes.iter().copied() {
        // Filter by job and level
        let required_level = inp
            .recipe_level_tables
            .get(&RecipeLevelTableId(recipe.recipe_level_table))
            .map(|t| t.class_job_level as i32)
            .unwrap_or(0);

        let job_code = craft_type_acronym(recipe.craft_type);
        let user_level = level_for_job_code(inp.levels, job_code).unwrap_or(0);

        if let Some(filter) = inp.job_filter
            && filter != job_code
        {
            continue;
        }

        // Check if the user can realistically craft this recipe.
        // If we have a required_level from RecipeLevelTable, ensure user_level >= required_level.
        // If we don't, fall back to "any non-zero level can craft".
        if user_level == 0 {
            continue;
        }
        if required_level > 0 && user_level < required_level {
            continue;
        }

        let sales_stats = if inp.filter_outliers {
            inp.raw_sales
                .get(&recipe.item_result)
                .map(|sales| analyze_sales(sales, true))
        } else {
            sales_stats_from_rollup(inp.sell_stats, recipe.item_result).or_else(|| {
                inp.raw_sales
                    .get(&recipe.item_result)
                    .map(|sales| analyze_sales(sales, false))
            })
        }
        .unwrap_or(SalesStats {
            daily_sales: 0.0,
            avg_price: 0,
            total_sales: 0,
        });

        let market_price = revenue_view
            .find_matching_listings(recipe.item_result)
            .lowest_gil()
            .unwrap_or(0);

        if market_price == 0 {
            continue;
        }

        // Deliberately the un-overlaid buy-scope listings, not the priced
        // view: `cheapest_world_id` must keep meaning "where the
        // scope-cheapest listing sits" regardless of which pricing bases
        // are selected.
        let scope_summary = inp.buy_listings.find_matching_listings(recipe.item_result);
        let cheapest_world_id = scope_summary
            .lq
            .map(|d| d.world_id)
            .or(scope_summary.hq.map(|d| d.world_id))
            .unwrap_or(0);

        // One `compute_cost` under `view`, over a fresh on-hand snapshot:
        // compute_cost consumes from the snapshot, and reusing one across
        // recipes (or across runs of one recipe) would wrongly deplete the
        // user's stockpile. `runs_done` feeds the debug timing log.
        let cost_run = |view: &SignalView<'_>| -> CostBreakdown {
            runs_done.set(runs_done.get() + 1);
            let active: Box<dyn OnHand> = match inp.on_hand {
                Some(map) => Box::new(LocalOnHand::from_map(map.clone())),
                None => Box::new(EmptyOnHand),
            };
            let opts = CraftingCostOptions {
                require_hq: inp.require_hq,
                max_subcraft_depth: if inp.use_subcrafts { 2 } else { 0 },
                shards: inp.shards,
                on_hand: active.as_ref(),
                vendor_prices: Some(vendor_price_map()),
            };
            compute_cost(recipe, view, inp.recipes_by_output, &opts, &is_shard_item)
        };
        let breakdown = cost_run(&ingredient_view);

        // `breakdown.cost` is the cost of one execution of the recipe, which
        // yields `amount_result` units; the market price is per unit, so
        // compare per unit.
        let cost_per_unit = per_unit_cost(breakdown.cost, recipe.amount_result);

        let (line, dropped) = profit_line(market_price, cost_per_unit, &inp.formula);
        if dropped {
            continue;
        }

        // Alternative cost runs, for kept rows only: the drop rule, ROI and
        // the row set are the selected pair's alone. A sale signal whose
        // buy-scope body is absent is not run — its cell shows "—" rather
        // than a listing number under a sale heading.
        let mut runs: [Option<CostBreakdown>; 4] = [None, None, None, None];
        for s in &inp.needs.cost {
            if *s == selected || (s.sale_stat().is_some() && inp.buy_stats.is_none()) {
                continue;
            }
            runs[s.index()] = Some(cost_run(&scope_view(*s)));
        }
        let run_for = |s: PriceSignal| -> Option<&CostBreakdown> {
            if s == selected {
                Some(&breakdown)
            } else {
                runs[s.index()].as_ref()
            }
        };
        let mut cost_alt = [None; 4];
        for s in PriceSignal::ALL {
            cost_alt[s.index()] = run_for(s).map(|b| per_unit_cost(b.cost, recipe.amount_result));
        }

        let hop = match (&home_view, inp.needs.hop) {
            // Buy from = This world only: no trip to price.
            (Some(_), true) if scope_is_home => Some(HopGain::Unavailable),
            (Some(home), true) => {
                let home_run = cost_run(home);
                let owned;
                let scope_run: &CostBreakdown = match run_for(hop_signal) {
                    Some(b) => b,
                    None => {
                        owned = cost_run(&scope_view(hop_signal));
                        &owned
                    }
                };
                Some(hop_gain(&home_run, scope_run, recipe.amount_result, scope_is_home))
            }
            _ => None,
        };
        // Worlds to visit reads the listing-min scope run whatever the
        // selected signal (`needed_signals` puts ListingMin in the set).
        let worlds = (inp.needs.worlds && !scope_is_home).then(|| {
            let owned;
            let listing_run: &CostBreakdown = match run_for(PriceSignal::ListingMin) {
                Some(b) => b,
                None => {
                    owned = cost_run(&scope_view(PriceSignal::ListingMin));
                    &owned
                }
            };
            worlds_to_visit(listing_run, inp.home_world_id, inp.dc_of)
        });

        // The bare sell-world number per revenue signal: the listing with
        // no buy-scope fallback, or the stat with no listing fallback.
        let item = recipe.item_result;
        let rev_alt = [
            inp.sell_listings
                .and_then(|s| s.find_matching_listings(item).lowest_gil())
                .filter(|p| *p > 0),
            stat_only_cheapest(inp.sell_stats, item, SaleStat::Min),
            stat_only_cheapest(inp.sell_stats, item, SaleStat::Median),
            stat_only_cheapest(inp.sell_stats, item, SaleStat::Avg),
        ];
        let revenue_fell_back =
            rev_alt[inp.formula.revenue_signal().index()] != Some(market_price);

        // Sell-world stats row matching how revenue resolves: prefer
        // the HQ row when the analyzer requires HQ, otherwise NQ, and
        // fall back to whichever quality actually traded.
        let sell_stat = inp
            .sell_stats
            .get(&(recipe.item_result, inp.require_hq))
            .or_else(|| inp.sell_stats.get(&(recipe.item_result, !inp.require_hq)));
        let vwap = sell_stat.map(|s| s.vwap).unwrap_or(0);

        results.push(RecipeProfitData {
            recipe,
            profit: line.profit,
            return_on_investment: line.roi,
            cost: line.cost,
            market_price: line.revenue,
            cheapest_world_id,
            sub_crafts: breakdown.sub_crafts,
            daily_sales: sales_stats.daily_sales,
            avg_price: sales_stats.avg_price,
            total_sales: sales_stats.total_sales,
            required_level,
            last_sold_unix: sell_stat.map(|s| s.last_sold_unix).unwrap_or(0),
            units_sold: sell_stat.map(|s| s.units_sold).unwrap_or(0),
            vwap,
            vwap_pct: vwap_pct(market_price, vwap),
            tax: line.tax,
            confidence: sell_stat.map(|s| s.confidence).unwrap_or_default(),
            cost_alt,
            rev_alt,
            revenue_fell_back,
            unpriced: breakdown.unpriced_market_lines,
            hop,
            worlds,
        });
    }

    (results, runs_done.get())
}
```

(`breakdown.sub_crafts` moves out of `breakdown` after the last use of `run_for`; the borrow checker accepts this because `run_for` is not used past the `worlds` block. `price_rows` now returns `(rows, compute_cost calls)`; the signature line becomes `fn price_rows(inp: &PriceInputs<'_>) -> (Vec<RecipeProfitData>, u32)` and every test caller destructures with `.0` — update `run_with` to `price_rows(&inp).0`.)

- [ ] **Step 6: The priced memo passes the new inputs (compile only)**

In `RecipeAnalyzerTable`'s `priced` memo, the `PriceInputs` literal gains, for now:

```rust
                needs: &NeededSignals::default(),
                sell_stats_loaded,
                home_world_id: 0,
                dc_of: &|_| None,
```

and the call plus debug log become:

```rust
            let (rows, cost_runs) = price_rows(&inp);
            #[cfg(all(debug_assertions, feature = "hydrate"))]
            leptos::logging::log!(
                "price_rows: {} recipes priced in {:.1} ms ({} compute_cost calls, hop {})",
                rows.len(),
                js_sys::Date::now() - t0,
                cost_runs,
                inp.needs.hop
            );
            #[cfg(not(all(debug_assertions, feature = "hydrate")))]
            let _ = cost_runs;
```

Task 10 wires the live values. `NeededSignals::default()` has an empty `cost` set, so only the selected run happens: numbers unchanged, CPU within about seven map lookups per kept row (the `rev_alt` reads), which the timing table in Task 11 will show.

- [ ] **Step 7: Run the route tests**

Run: `cargo test -p ultros-app --lib -- routes::recipe_analyzer::test`
Expected: PASS — including `price_rows_matches_recorded_oracle_on_fixture` **unchanged** (the oracle runs with sub-crafts off and the selected run only; if it moves, stop and find out why before touching the oracle).

- [ ] **Step 8: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "feat(recipe-analyzer): priced rows carry alternative signals, the listing tell, unpriced count, hop gain and worlds"
```

---

### Task 9: Ten table rows, 21 sort modes, none-last sorting and the URL contract

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:493-522` (column consts and the derived orders), `:540-645` (labels and specs), `:649-690` (cells and classes), `:694-857` (`RECIPE_BASE`, `RECIPE_COLUMNS`), `:931-994` (`SortMode`, `compare_recipes`), `:1203-1239` (`filter_and_sort`), and `mod test`

**Interfaces:**
- Consumes: `ColumnKind::{RevSignal, CostSignal, HopGain, HopWorlds}`, `PickerGroup`, `CellCtx.{signal_columns, capped_cost}`, `CellValue::{MutedGil, GilWithNote, Hop}`, `CellNote`, `ToolColumnMeta.lab` (Task 5); `LAB_ANALYZER_SIGNAL_COLUMNS` (Task 1); the row fields (Task 8); `cmp_none_last`.
- Produces: `COL_REV_LISTING_MIN … COL_HOP_WORLDS` (ten consts), `BASE_COLUMN_ORDER: LazyLock<Vec<&'static str>>` (ids with `lab == None`), `RECIPE_COLUMNS: [ToolColumnMeta<RecipeRow, SortMode>; 25]`, `SortMode::{RevSignal(PriceSignal), CostSignal(PriceSignal), HopGain, HopWorlds}`, `SortMode::lab_only(self) -> bool`, `compare_recipes(mode, dir, a, b) -> Ordering` (direction applied inside), `delta_pct(alt: Option<i32>, input: i32) -> Option<f32>`.

- [ ] **Step 1: Write the failing tests**

In `mod test`:

```rust
    const ALL_SORT_MODES: [SortMode; 21] = [
        SortMode::Roi,
        SortMode::Profit,
        SortMode::Velocity,
        SortMode::CostPerUnit,
        SortMode::Price,
        SortMode::AvgPrice,
        SortMode::LastSold,
        SortMode::Volume,
        SortMode::Vwap,
        SortMode::Tax,
        SortMode::Confidence,
        SortMode::RevSignal(PriceSignal::ListingMin),
        SortMode::RevSignal(PriceSignal::SaleMin),
        SortMode::RevSignal(PriceSignal::SaleMedian),
        SortMode::RevSignal(PriceSignal::SaleAvg),
        SortMode::CostSignal(PriceSignal::ListingMin),
        SortMode::CostSignal(PriceSignal::SaleMin),
        SortMode::CostSignal(PriceSignal::SaleMedian),
        SortMode::CostSignal(PriceSignal::SaleAvg),
        SortMode::HopGain,
        SortMode::HopWorlds,
    ];
```

Replace the enumerations inside `sort_mode_round_trips_through_the_url` and `every_recipe_sort_mode_is_catalogued_exactly_once` with `for mode in ALL_SORT_MODES {`, and extend them:

```rust
        // (round-trip test) malformed signal tokens are rejected
        assert!("rev-".parse::<SortMode>().is_err());
        assert!("cost-mars".parse::<SortMode>().is_err());
        assert!("rev-listing-min".parse::<SortMode>().is_ok());
        assert_eq!(SortMode::CostSignal(PriceSignal::SaleAvg).to_string(), "cost-sale-avg");
        assert_eq!(SortMode::HopWorlds.to_string(), "hop-worlds");

        // (catalogue test) new default directions
        assert_eq!(SortMode::HopWorlds.default_dir(), SortDir::Asc);
        assert_eq!(SortMode::HopGain.default_dir(), SortDir::Desc);
        assert_eq!(SortMode::CostSignal(PriceSignal::SaleMin).default_dir(), SortDir::Asc);
        assert_eq!(SortMode::RevSignal(PriceSignal::SaleMin).default_dir(), SortDir::Desc);
```

Replace `recipe_optional_column_order_is_a_stable_url_contract`'s assertions:

```rust
        assert_eq!(
            OPTIONAL_COLUMN_ORDER.as_slice(),
            &[
                "confidence", "last-sold", "volume", "vwap", "tax", "listing-world", "listing-dc",
                "rev-listing-min", "rev-sale-min", "rev-sale-median", "rev-sale-avg",
                "cost-listing-min", "cost-sale-min", "cost-sale-median", "cost-sale-avg",
                "hop-gain", "hop-worlds",
            ]
        );
        // The contract the page uses while the lab is off: the seven of Phase B.
        assert_eq!(
            BASE_COLUMN_ORDER.as_slice(),
            &["confidence", "last-sold", "volume", "vwap", "tax", "listing-world", "listing-dc"]
        );
        assert_eq!(DEFAULT_COLS.as_slice(), &["confidence"]);
```

Add:

```rust
    #[test]
    fn signal_columns_have_unique_ids_and_sort_tokens() {
        let mut ids: Vec<&str> = RECIPE_COLUMNS.iter().map(|c| c.id).filter(|i| !i.is_empty()).collect();
        let mut sorts: Vec<&str> = RECIPE_COLUMNS.iter().map(|c| c.sort_id).filter(|i| !i.is_empty()).collect();
        let (n_ids, n_sorts) = (ids.len(), sorts.len());
        ids.sort_unstable();
        ids.dedup();
        sorts.sort_unstable();
        sorts.dedup();
        assert_eq!((ids.len(), sorts.len()), (n_ids, n_sorts));
        assert_eq!(n_ids, 17);
        assert_eq!(n_sorts, 21, "the eleven sorts at HEAD plus the ten signal and hop columns; listing world/dc do not sort");
        for c in RECIPE_COLUMNS.iter().filter(|c| c.lab.is_some()) {
            assert!(!c.default_on, "{} must start hidden", c.id);
            assert_eq!(c.lab, Some(LAB_ANALYZER_SIGNAL_COLUMNS));
            assert!(c.header_class.contains("hidden md:"), "{}: desktop-only (kit decision 7)", c.id);
        }
        assert_eq!(RECIPE_COLUMNS.iter().filter(|c| c.lab.is_some()).count(), 10);
    }

    fn hop_row(key: i32, hop: Option<HopGain>, alt: Option<i32>) -> Arc<RecipeProfitData> {
        let mut r = Arc::try_unwrap(row(key, 0, 0, 1.0, 1)).ok().unwrap();
        r.hop = hop;
        r.cost_alt[PriceSignal::SaleMedian.index()] = alt;
        Arc::new(r)
    }

    /// `Needed` / `Unavailable` (and an unrun alt signal) sort last in both
    /// directions; `HopWorlds` defaults ascending.
    #[test]
    fn hop_needed_sorts_last_both_directions() {
        let keys: Vec<i32> = fixture_recipes().iter().take(4).map(|r| r.key_id.0).collect();
        let rows = vec![
            hop_row(keys[0], Some(HopGain::Gain(5)), Some(300)),
            hop_row(keys[1], Some(HopGain::Needed), None),
            hop_row(keys[2], Some(HopGain::Gain(-3)), Some(100)),
            hop_row(keys[3], Some(HopGain::Unavailable), Some(200)),
        ];
        let names = HashMap::new();
        let order = |mode: SortMode, dir: SortDir| -> Vec<i32> {
            filter_and_sort(&rows, &Thresholds::default(), &names, mode, dir)
                .into_iter()
                .map(|(_, r)| r.recipe.key_id.0)
                .collect()
        };
        assert_eq!(order(SortMode::HopGain, SortDir::Desc), vec![keys[0], keys[2], keys[1], keys[3]]);
        assert_eq!(order(SortMode::HopGain, SortDir::Asc), vec![keys[2], keys[0], keys[1], keys[3]]);
        let median = SortMode::CostSignal(PriceSignal::SaleMedian);
        assert_eq!(order(median, SortDir::Asc), vec![keys[2], keys[3], keys[0], keys[1]]);
        assert_eq!(order(median, SortDir::Desc), vec![keys[0], keys[3], keys[2], keys[1]]);
        // The pre-existing modes still flip whole.
        assert_eq!(order(SortMode::Profit, SortDir::Desc).len(), 4);
    }

    #[test]
    fn delta_pct_math() {
        assert_eq!(delta_pct(Some(138), 100), Some(38.0));
        assert_eq!(delta_pct(Some(50), 100), Some(-50.0));
        assert_eq!(delta_pct(None, 100), None);
        assert_eq!(delta_pct(Some(0), 100), None, "an unpriced alt has no delta");
        assert_eq!(delta_pct(Some(100), 0), None);
        assert_eq!(delta_pct(Some(100), 100), None, "the duplicate column shows no +0%");
    }

    #[test]
    fn lab_only_sort_modes_are_exactly_the_ten() {
        assert_eq!(ALL_SORT_MODES.iter().filter(|m| m.lab_only()).count(), 10);
        assert!(!SortMode::CostPerUnit.lab_only() && !SortMode::Price.lab_only());
    }

    /// Every picker entry is a `?cols=` token (both derive from the table).
    #[test]
    fn picker_columns_are_a_subset_of_optional_column_order() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let i18n = use_i18n();
            let ctx = PickerContext {
                sell_place: String::new(),
                buy_place: String::new(),
                revenue: PriceSignal::ListingMin,
                cost: PriceSignal::ListingMin,
                capped: BTreeSet::new(),
            };
            let ids: Vec<&str> = grouped_picker_options(&RECIPE_COLUMNS, i18n, &ctx).iter().map(|o| o.id).collect();
            assert_eq!(ids.len(), 17);
            assert!(ids.iter().all(|id| OPTIONAL_COLUMN_ORDER.contains(id)));
            let flat: Vec<&str> = picker_options(&RECIPE_COLUMNS, i18n).iter().map(|o| o.id).collect();
            assert_eq!(flat, BASE_COLUMN_ORDER.as_slice());
        });
    }
```

(`PickerContext` / `grouped_picker_options` are imported in Task 10; import them here too: `use crate::analyzer_kit::columns::{PickerContext, grouped_picker_options};` and `use std::collections::BTreeSet;`.)

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- routes::recipe_analyzer::test`
Expected: compile errors.

- [ ] **Step 3: Column consts and the base order**

After `COL_LISTING_DC`:

```rust
// Phase D, behind `analyzer-signal-columns`: appended after the seven
// above so every serialized old URL stays byte-identical.
const COL_REV_LISTING_MIN: &str = "rev-listing-min";
const COL_REV_SALE_MIN: &str = "rev-sale-min";
const COL_REV_SALE_MEDIAN: &str = "rev-sale-median";
const COL_REV_SALE_AVG: &str = "rev-sale-avg";
const COL_COST_LISTING_MIN: &str = "cost-listing-min";
const COL_COST_SALE_MIN: &str = "cost-sale-min";
const COL_COST_SALE_MEDIAN: &str = "cost-sale-median";
const COL_COST_SALE_AVG: &str = "cost-sale-avg";
const COL_HOP_GAIN: &str = "hop-gain";
const COL_HOP_WORLDS: &str = "hop-worlds";
```

After `OPTIONAL_COLUMN_ORDER`:

```rust
/// The `?cols=` contract while the signal-columns lab is off: every token
/// not gated by a lab. `parse_visible_cols` over this slice drops the
/// Phase D tokens, so a shared `?cols=hop-gain` renders as before the
/// phase for a player without the lab.
static BASE_COLUMN_ORDER: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    RECIPE_COLUMNS
        .iter()
        .filter(|c| !c.id.is_empty() && c.lab.is_none())
        .map(|c| c.id)
        .collect()
});
```

Import `LAB_ANALYZER_SIGNAL_COLUMNS` alongside `LAB_ANALYZER_LEDGER`.

- [ ] **Step 4: Labels, specs, cells, classes**

Labels (after `label_actions`):

```rust
fn label_listing_min(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, price_basis_listing_min).to_string()
}
fn label_sale_min(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, price_basis_sale_min).to_string()
}
fn label_sale_median(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, price_basis_sale_median).to_string()
}
fn label_sale_avg(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, price_basis_sale_avg).to_string()
}
fn label_hop_gain(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, analyzer_col_hop_gain).to_string()
}
fn label_hop_worlds(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, analyzer_col_hop_worlds).to_string()
}
```

Specs (after `SPEC_ACTIONS`; the 15 existing ones already carry `group: PickerGroup::Other` from Task 5):

```rust
static SPEC_REV_LISTING_MIN: ColumnSpec = ColumnSpec { kind: ColumnKind::RevSignal(PriceSignal::ListingMin), label: label_listing_min, group: PickerGroup::Revenue };
static SPEC_REV_SALE_MIN: ColumnSpec = ColumnSpec { kind: ColumnKind::RevSignal(PriceSignal::SaleMin), label: label_sale_min, group: PickerGroup::Revenue };
static SPEC_REV_SALE_MEDIAN: ColumnSpec = ColumnSpec { kind: ColumnKind::RevSignal(PriceSignal::SaleMedian), label: label_sale_median, group: PickerGroup::Revenue };
static SPEC_REV_SALE_AVG: ColumnSpec = ColumnSpec { kind: ColumnKind::RevSignal(PriceSignal::SaleAvg), label: label_sale_avg, group: PickerGroup::Revenue };
static SPEC_COST_LISTING_MIN: ColumnSpec = ColumnSpec { kind: ColumnKind::CostSignal(PriceSignal::ListingMin), label: label_listing_min, group: PickerGroup::Cost };
static SPEC_COST_SALE_MIN: ColumnSpec = ColumnSpec { kind: ColumnKind::CostSignal(PriceSignal::SaleMin), label: label_sale_min, group: PickerGroup::Cost };
static SPEC_COST_SALE_MEDIAN: ColumnSpec = ColumnSpec { kind: ColumnKind::CostSignal(PriceSignal::SaleMedian), label: label_sale_median, group: PickerGroup::Cost };
static SPEC_COST_SALE_AVG: ColumnSpec = ColumnSpec { kind: ColumnKind::CostSignal(PriceSignal::SaleAvg), label: label_sale_avg, group: PickerGroup::Cost };
static SPEC_HOP_GAIN: ColumnSpec = ColumnSpec { kind: ColumnKind::HopGain, label: label_hop_gain, group: PickerGroup::Travel };
static SPEC_HOP_WORLDS: ColumnSpec = ColumnSpec { kind: ColumnKind::HopWorlds, label: label_hop_worlds, group: PickerGroup::Travel };
```

Cells (after `cell_tax`); `cell_price` changes:

```rust
/// The Price slot: under the lab it carries the always-present note
/// sub-line so a price that fell back to a listing says so.
fn cell_price(r: &RecipeRow, ctx: &CellCtx) -> CellValue {
    if ctx.signal_columns {
        CellValue::GilWithNote {
            amount: r.market_price,
            note: if r.revenue_fell_back {
                CellNote::ListingFallback
            } else {
                CellNote::None
            },
        }
    } else {
        CellValue::Gil(r.market_price)
    }
}

/// Percent of an alternative against the same-side formula input; `None`
/// when either is unpriced, or when they are equal (the selected signal's
/// own duplicate column shows no "+0%").
fn delta_pct(alt: Option<i32>, input: i32) -> Option<f32> {
    let alt = alt.filter(|a| *a > 0)?;
    (input > 0 && alt != input).then(|| (alt - input) as f32 / input as f32 * 100.0)
}

fn cost_alt_cell(r: &RecipeRow, ctx: &CellCtx, s: PriceSignal) -> CellValue {
    let alt = r.cost_alt[s.index()];
    CellValue::MutedGil {
        amount: alt,
        pct: delta_pct(alt, r.cost),
        side: TermRole::Cost,
        capped: ctx.capped_cost[s.index()],
    }
}
fn rev_alt_cell(r: &RecipeRow, s: PriceSignal) -> CellValue {
    let alt = r.rev_alt[s.index()];
    CellValue::MutedGil {
        amount: alt,
        pct: delta_pct(alt, r.market_price),
        side: TermRole::Revenue,
        capped: false,
    }
}
// One `fn` per column: the table needs fn pointers, not closures.
fn cell_rev_listing_min(r: &RecipeRow, _: &CellCtx) -> CellValue { rev_alt_cell(r, PriceSignal::ListingMin) }
fn cell_rev_sale_min(r: &RecipeRow, _: &CellCtx) -> CellValue { rev_alt_cell(r, PriceSignal::SaleMin) }
fn cell_rev_sale_median(r: &RecipeRow, _: &CellCtx) -> CellValue { rev_alt_cell(r, PriceSignal::SaleMedian) }
fn cell_rev_sale_avg(r: &RecipeRow, _: &CellCtx) -> CellValue { rev_alt_cell(r, PriceSignal::SaleAvg) }
fn cell_cost_listing_min(r: &RecipeRow, c: &CellCtx) -> CellValue { cost_alt_cell(r, c, PriceSignal::ListingMin) }
fn cell_cost_sale_min(r: &RecipeRow, c: &CellCtx) -> CellValue { cost_alt_cell(r, c, PriceSignal::SaleMin) }
fn cell_cost_sale_median(r: &RecipeRow, c: &CellCtx) -> CellValue { cost_alt_cell(r, c, PriceSignal::SaleMedian) }
fn cell_cost_sale_avg(r: &RecipeRow, c: &CellCtx) -> CellValue { cost_alt_cell(r, c, PriceSignal::SaleAvg) }
fn cell_hop_gain(r: &RecipeRow, _: &CellCtx) -> CellValue {
    CellValue::Hop {
        gain: r.hop.unwrap_or(HopGain::Unavailable),
        daily_sales: r.daily_sales,
    }
}
```

Import `CellNote` from `crate::analyzer_kit::cells`. Classes (after `FORMULA_CELL`):

```rust
/// The alternative-signal columns: two-line headers (sub-label + pill)
/// at the formula width, desktop only. `md:flex`, not `md:block`:
/// `SortableHeaderCell` appends `flex flex-col justify-center` for a
/// two-line header, and a later `md:block` would override it at md+.
const HEAD_40_MD: &str = "w-40 shrink-0 px-3 py-2 leading-tight hidden md:flex";
const CELL_40_MD: &str = "px-3 py-2 w-40 shrink-0 text-right hidden md:block";
```

- [ ] **Step 5: The table rows**

`RECIPE_COLUMNS` becomes `[ToolColumnMeta<RecipeRow, SortMode>; 25]`; insert these ten entries **between** the `SPEC_DC` entry and the `SPEC_ACTIONS` entry (DOM order: signals, then travel, then actions):

```rust
    ToolColumnMeta {
        spec: &SPEC_REV_LISTING_MIN,
        id: COL_REV_LISTING_MIN,
        sort_id: COL_REV_LISTING_MIN,
        sort: sortability_for(Layer::RowLocal, Some(SortMode::RevSignal(PriceSignal::ListingMin))),
        header_class: HEAD_40_MD,
        cell_class: CELL_40_MD,
        default_on: false,
        cell: cell_rev_listing_min,
        lab: Some(LAB_ANALYZER_SIGNAL_COLUMNS),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_REV_SALE_MIN,
        id: COL_REV_SALE_MIN,
        sort_id: COL_REV_SALE_MIN,
        sort: sortability_for(Layer::Bulk, Some(SortMode::RevSignal(PriceSignal::SaleMin))),
        header_class: HEAD_40_MD,
        cell_class: CELL_40_MD,
        default_on: false,
        cell: cell_rev_sale_min,
        lab: Some(LAB_ANALYZER_SIGNAL_COLUMNS),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_REV_SALE_MEDIAN,
        id: COL_REV_SALE_MEDIAN,
        sort_id: COL_REV_SALE_MEDIAN,
        sort: sortability_for(Layer::Bulk, Some(SortMode::RevSignal(PriceSignal::SaleMedian))),
        header_class: HEAD_40_MD,
        cell_class: CELL_40_MD,
        default_on: false,
        cell: cell_rev_sale_median,
        lab: Some(LAB_ANALYZER_SIGNAL_COLUMNS),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_REV_SALE_AVG,
        id: COL_REV_SALE_AVG,
        sort_id: COL_REV_SALE_AVG,
        sort: sortability_for(Layer::Bulk, Some(SortMode::RevSignal(PriceSignal::SaleAvg))),
        header_class: HEAD_40_MD,
        cell_class: CELL_40_MD,
        default_on: false,
        cell: cell_rev_sale_avg,
        lab: Some(LAB_ANALYZER_SIGNAL_COLUMNS),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_COST_LISTING_MIN,
        id: COL_COST_LISTING_MIN,
        sort_id: COL_COST_LISTING_MIN,
        sort: sortability_for(Layer::Computed, Some(SortMode::CostSignal(PriceSignal::ListingMin))),
        default_dir: SortDir::Asc,
        header_class: HEAD_40_MD,
        cell_class: CELL_40_MD,
        default_on: false,
        cell: cell_cost_listing_min,
        lab: Some(LAB_ANALYZER_SIGNAL_COLUMNS),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_COST_SALE_MIN,
        id: COL_COST_SALE_MIN,
        sort_id: COL_COST_SALE_MIN,
        sort: sortability_for(Layer::Computed, Some(SortMode::CostSignal(PriceSignal::SaleMin))),
        default_dir: SortDir::Asc,
        header_class: HEAD_40_MD,
        cell_class: CELL_40_MD,
        default_on: false,
        cell: cell_cost_sale_min,
        lab: Some(LAB_ANALYZER_SIGNAL_COLUMNS),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_COST_SALE_MEDIAN,
        id: COL_COST_SALE_MEDIAN,
        sort_id: COL_COST_SALE_MEDIAN,
        sort: sortability_for(Layer::Computed, Some(SortMode::CostSignal(PriceSignal::SaleMedian))),
        default_dir: SortDir::Asc,
        header_class: HEAD_40_MD,
        cell_class: CELL_40_MD,
        default_on: false,
        cell: cell_cost_sale_median,
        lab: Some(LAB_ANALYZER_SIGNAL_COLUMNS),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_COST_SALE_AVG,
        id: COL_COST_SALE_AVG,
        sort_id: COL_COST_SALE_AVG,
        sort: sortability_for(Layer::Computed, Some(SortMode::CostSignal(PriceSignal::SaleAvg))),
        default_dir: SortDir::Asc,
        header_class: HEAD_40_MD,
        cell_class: CELL_40_MD,
        default_on: false,
        cell: cell_cost_sale_avg,
        lab: Some(LAB_ANALYZER_SIGNAL_COLUMNS),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_HOP_GAIN,
        id: COL_HOP_GAIN,
        sort_id: COL_HOP_GAIN,
        sort: sortability_for(Layer::Computed, Some(SortMode::HopGain)),
        header_class: HEAD_28_MD,
        cell_class: CELL_28_MD,
        default_on: false,
        cell: cell_hop_gain,
        lab: Some(LAB_ANALYZER_SIGNAL_COLUMNS),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_HOP_WORLDS,
        id: COL_HOP_WORLDS,
        sort_id: COL_HOP_WORLDS,
        sort: sortability_for(Layer::Computed, Some(SortMode::HopWorlds)),
        default_dir: SortDir::Asc,
        header_class: HEAD_28_MD,
        // Custom: the tooltip needs the page's world names.
        cell_class: CELL_28_MD,
        default_on: false,
        lab: Some(LAB_ANALYZER_SIGNAL_COLUMNS),
        ..RECIPE_BASE
    },
```

Do not serve or e2e this commit: `?cols=hop-worlds` hits the custom closure's `unreachable!` until Task 10 adds the arm, and the lab gate on `?cols=` also lands in Task 10.

- [ ] **Step 6: `SortMode` and the comparator**

```rust
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum SortMode {
    Roi,
    Profit,
    Velocity,
    CostPerUnit,
    Price,
    AvgPrice,
    LastSold,
    Volume,
    Vwap,
    Tax,
    Confidence,
    /// An alternative revenue column (`rev-‹token›`).
    RevSignal(PriceSignal),
    /// An alternative cost column (`cost-‹token›`).
    CostSignal(PriceSignal),
    HopGain,
    HopWorlds,
}

impl SortMode {
    /// Sorts that exist only under the signal-columns lab. With the lab
    /// off the page treats them as unset, as it did before they existed.
    fn lab_only(self) -> bool {
        matches!(
            self,
            SortMode::RevSignal(_) | SortMode::CostSignal(_) | SortMode::HopGain | SortMode::HopWorlds
        )
    }
}
```

`compare_recipes` takes the direction so the none-last modes can keep their missing values at the bottom in both directions:

```rust
fn hop_sort_key(hop: Option<HopGain>) -> Option<i32> {
    match hop {
        Some(HopGain::Gain(g)) => Some(g),
        _ => None,
    }
}

/// The ordering for `mode` with `dir` already applied. The plain modes
/// flip whole; the alternative-signal and hop modes flip only between two
/// present values (`cmp_none_last`), so "—" / "needed" rows stay last
/// whichever way the header points.
fn compare_recipes(mode: SortMode, dir: SortDir, a: &RecipeProfitData, b: &RecipeProfitData) -> Ordering {
    let oriented = |o: Ordering| match dir {
        SortDir::Asc => o,
        SortDir::Desc => o.reverse(),
    };
    match mode {
        SortMode::Roi => oriented(a.return_on_investment.cmp(&b.return_on_investment)),
        SortMode::Profit => oriented(a.profit.cmp(&b.profit)),
        SortMode::Velocity => oriented(
            a.daily_sales
                .partial_cmp(&b.daily_sales)
                .unwrap_or(Ordering::Equal),
        ),
        SortMode::CostPerUnit => oriented(a.cost.cmp(&b.cost)),
        SortMode::Price => oriented(a.market_price.cmp(&b.market_price)),
        SortMode::AvgPrice => oriented(a.avg_price.cmp(&b.avg_price)),
        // Desc (the default) = most recent first: larger unix is newer.
        SortMode::LastSold => oriented(a.last_sold_unix.cmp(&b.last_sold_unix)),
        SortMode::Volume => oriented(a.units_sold.cmp(&b.units_sold)),
        SortMode::Vwap => oriented(a.vwap.cmp(&b.vwap)),
        SortMode::Tax => oriented(a.tax.cmp(&b.tax)),
        SortMode::Confidence => oriented(confidence_rank(a.confidence).cmp(&confidence_rank(b.confidence))),
        SortMode::RevSignal(s) => cmp_none_last(a.rev_alt[s.index()], b.rev_alt[s.index()], dir, i32::cmp),
        SortMode::CostSignal(s) => cmp_none_last(a.cost_alt[s.index()], b.cost_alt[s.index()], dir, i32::cmp),
        SortMode::HopGain => cmp_none_last(hop_sort_key(a.hop), hop_sort_key(b.hop), dir, i32::cmp),
        SortMode::HopWorlds => cmp_none_last(
            a.worlds.as_ref().map(|w| w.worlds.len()),
            b.worlds.as_ref().map(|w| w.worlds.len()),
            dir,
            usize::cmp,
        ),
    }
}
```

In `filter_and_sort`, the sort becomes:

```rust
    kept.sort_by(|a, b| {
        // Deterministic tiebreak: the input comes from a std HashMap, so
        // without it ties could order differently on the server and the
        // client and mismatch the SSR-rendered rows.
        compare_recipes(mode, dir, a, b).then_with(|| a.recipe.key_id.0.cmp(&b.recipe.key_id.0))
    });
```

Import `cmp_none_last` from `crate::components::sort_header`.

- [ ] **Step 7: Run the route tests**

Run: `cargo test -p ultros-app --lib -- routes::recipe_analyzer::test`
Expected: PASS, including the unchanged `filter_and_sort_is_pure_and_inclusive`.

- [ ] **Step 8: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "feat(recipe-analyzer): ten lab-gated signal and hop columns, 21 sort modes, none-last sorting"
```

---

### Task 10: Page wiring — the lab, the fetch gate, headers, pills, picker and cells

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs`: `RecipeAnalyzer` (`:2307-2360` signals, `:2500-2535` resources, `:2625-2670` info panel, `:2734-2806` Suspense join), `RecipeAnalyzerTable` (`:1281-1360` props and setup, `:1430-1530` indexes and memos, `:1665-1680` picker, `:1737-1960` custom cells, `:2272-2289` grid), `mod test`
- Modify: `ultros-frontend/ultros-app/src/components/tool_help.rs:8-27` (`ToolCalculation.details`) and `:131` (its render)

**Interfaces:**
- Consumes: everything from Tasks 1–9; `grouped_picker_options`, `PickerContext` (Task 5); `HeaderExtras`, `HeaderExtra`, `HeaderLine2`, `HeaderPill` (Task 7); `SignalWants`, `needed_signals` (Task 3).
- Produces (page-private): `signal_wants(visible: &HashSet<&'static str>, sort: Option<SortMode>) -> SignalWants`, `buy_stats_scope_key(formula: &ProfitFormula, needs: &RecipeNeeds, scope_name: String) -> Option<String>`, `pill_param(kind: ColumnKind) -> Option<(TermRole, PriceSignal)>`, `capped_flags(capped: &BTreeSet<PriceSignal>) -> [bool; 4]`, `type WorldLine = (i32, Option<(String, String)>, u16)`, `worlds_tooltip(i18n, entries: &[WorldLine], dcs: u8) -> String`. New `RecipeAnalyzerTable` props: `signal_cols: bool` (read in the Suspense join, so a lab flip remounts the table), `needs: Memo<NeededSignals>`, `buy_stats_aliased: bool`, `#[prop(into)] home_world_id: Signal<i32>`, `on_pill: Callback<ColumnKind>`. `ToolCalculation::new`'s `details` becomes `impl Into<Signal<String>>`.

- [ ] **Step 1: Write the failing tests**

```rust
    /// A pill writes exactly one param: `cost-basis` for a cost column,
    /// `revenue` for a revenue column, nothing for anything else.
    #[test]
    fn use_as_pill_writes_exactly_one_param() {
        assert_eq!(
            pill_param(ColumnKind::CostSignal(PriceSignal::SaleMedian)),
            Some((TermRole::Cost, PriceSignal::SaleMedian))
        );
        assert_eq!(
            pill_param(ColumnKind::RevSignal(PriceSignal::SaleAvg)),
            Some((TermRole::Revenue, PriceSignal::SaleAvg))
        );
        assert_eq!(pill_param(ColumnKind::HopGain), None);
        assert_eq!(pill_param(ColumnKind::CostSlot), None);
    }

    #[test]
    fn signal_wants_reads_visible_columns_and_the_sort_target() {
        let visible: HashSet<&'static str> =
            [COL_CONFIDENCE, COL_COST_SALE_AVG, COL_COST_LISTING_MIN, COL_REV_SALE_MIN].into_iter().collect();
        let w = signal_wants(&visible, Some(SortMode::CostSignal(PriceSignal::SaleMin)));
        assert_eq!(w.visible_cost, vec![PriceSignal::ListingMin, PriceSignal::SaleAvg], "table order");
        assert_eq!(w.sort_cost, Some(PriceSignal::SaleMin));
        assert!(!w.hop && !w.worlds);
        let w = signal_wants(&HashSet::new(), Some(SortMode::HopGain));
        assert!(w.hop && !w.worlds);
        let visible: HashSet<&'static str> = [COL_HOP_WORLDS].into_iter().collect();
        let w = signal_wants(&visible, None);
        assert!(w.worlds && !w.hop);
        assert_eq!(signal_wants(&HashSet::new(), Some(SortMode::Profit)), SignalWants::default());
    }

    #[test]
    fn buy_stats_fetch_only_when_a_sale_cost_signal_is_needed() {
        let listing = ProfitFormula::recipe_from_query(None, None, None);
        let plain = RecipeNeeds::default();
        assert_eq!(buy_stats_scope_key(&listing, &plain, "Aether".into()), None);
        let median = ProfitFormula::recipe_from_query(Some(PriceSignal::SaleMedian), None, None);
        assert_eq!(buy_stats_scope_key(&median, &plain, "Aether".into()), Some("Aether".into()));
        // A visible / sorted sale-cost column forces the body under a listing basis.
        let mut wants_col = RecipeNeeds::default();
        wants_col.cost_signals.insert(PriceSignal::SaleMin);
        assert_eq!(buy_stats_scope_key(&listing, &wants_col, "Aether".into()), Some("Aether".into()));
        // A revenue signal never does: it reads the sell-world body.
        let rev = ProfitFormula::recipe_from_query(None, Some(PriceSignal::SaleMedian), None);
        assert_eq!(buy_stats_scope_key(&rev, &plain, "Aether".into()), None);
    }

    #[test]
    fn buy_stats_key_is_none_when_buy_scope_is_the_sell_world() {
        let f = ProfitFormula::recipe_from_query(Some(PriceSignal::SaleMedian), None, Some(BuyScope::World));
        let same = RecipeNeeds { buy_scope_is_sell_world: true, ..RecipeNeeds::default() };
        assert_eq!(buy_stats_scope_key(&f, &same, "Gilgamesh".into()), None);
        let other = RecipeNeeds::default();
        assert_eq!(buy_stats_scope_key(&f, &other, "Gilgamesh".into()), Some("Gilgamesh".into()));
        // Only a World scope can alias; a datacenter never does.
        let dc = ProfitFormula::recipe_from_query(Some(PriceSignal::SaleMedian), None, None);
        assert_eq!(buy_stats_scope_key(&dc, &same, "Aether".into()), Some("Aether".into()));
    }

    #[test]
    fn capped_flags_index_by_signal() {
        let capped = [PriceSignal::SaleAvg, PriceSignal::SaleMin].into_iter().collect();
        assert_eq!(capped_flags(&capped), [false, true, false, true]);
        assert_eq!(capped_flags(&BTreeSet::new()), [false; 4]);
    }

    #[test]
    fn worlds_tooltip_groups_by_datacenter_in_first_appearance_order() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let i18n = use_i18n();
            let entries = vec![
                (5, Some(("Cactuar".to_string(), "Aether".to_string())), 2),
                (9, Some(("Behemoth".to_string(), "Primal".to_string())), 1),
                (7, Some(("Adamantoise".to_string(), "Aether".to_string())), 1),
                (999, None, 1),
            ];
            let text = worlds_tooltip(i18n, &entries, 2);
            let aether = text.find("Aether").unwrap();
            let primal = text.find("Primal").unwrap();
            let cactuar = text.find("• Cactuar · ingredients: 2").unwrap();
            let adamantoise = text.find("• Adamantoise · ingredients: 1").unwrap();
            assert!(aether < cactuar && cactuar < adamantoise && adamantoise < primal, "{text}");
            assert!(text.contains("• 999 · ingredients: 1"), "{text}");
            assert!(text.contains("Datacenters: 2"), "{text}");
            assert!(text.ends_with("buy side only · sub-craft materials not counted"), "{text}");
        });
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- routes::recipe_analyzer::test`
Expected: compile errors.

- [ ] **Step 3: The pure page helpers**

Near `migrate_legacy_params` (page-level helpers), add:

```rust
/// What the visible columns and the sort target ask of the pricing pass.
/// Visible cost columns come out in table order (the cap claims them in
/// that order).
fn signal_wants(visible: &HashSet<&'static str>, sort: Option<SortMode>) -> SignalWants {
    let visible_cost = RECIPE_COLUMNS
        .iter()
        .filter(|c| !c.id.is_empty() && visible.contains(c.id))
        .filter_map(|c| match c.spec.kind {
            ColumnKind::CostSignal(s) => Some(s),
            _ => None,
        })
        .collect();
    let sort_cost = match sort {
        Some(SortMode::CostSignal(s)) => Some(s),
        _ => None,
    };
    SignalWants {
        visible_cost,
        sort_cost,
        hop: visible.contains(COL_HOP_GAIN) || sort == Some(SortMode::HopGain),
        worlds: visible.contains(COL_HOP_WORLDS) || sort == Some(SortMode::HopWorlds),
    }
}

/// The buy-scope sale-stats resource key: the scope name when the body is
/// needed, `None` (no fetch) otherwise.
fn buy_stats_scope_key(
    formula: &ProfitFormula,
    needs: &RecipeNeeds,
    scope_name: String,
) -> Option<String> {
    needed_bodies(formula, needs)
        .contains(&BodyRole::BuyScopeStats(SALE_STATS_WINDOW_DAYS))
        .then_some(scope_name)
}

/// Which formula side a header pill writes, and the signal it writes.
fn pill_param(kind: ColumnKind) -> Option<(TermRole, PriceSignal)> {
    match kind {
        ColumnKind::RevSignal(s) => Some((TermRole::Revenue, s)),
        ColumnKind::CostSignal(s) => Some((TermRole::Cost, s)),
        _ => None,
    }
}

/// `NeededSignals::capped` as the `[bool; 4]` the cell context carries.
fn capped_flags(capped: &BTreeSet<PriceSignal>) -> [bool; 4] {
    let mut flags = [false; 4];
    for s in capped {
        flags[s.index()] = true;
    }
    flags
}

/// The full picker label of a signal ("Sale median (7d)").
fn signal_label(i18n: I18nContext<Locale, I18nKeys>, s: PriceSignal) -> String {
    match s {
        PriceSignal::ListingMin => t_string!(i18n, price_basis_listing_min).to_string(),
        PriceSignal::SaleMin => t_string!(i18n, price_basis_sale_min).to_string(),
        PriceSignal::SaleMedian => t_string!(i18n, price_basis_sale_median).to_string(),
        PriceSignal::SaleAvg => t_string!(i18n, price_basis_sale_avg).to_string(),
    }
}

/// The one-sentence definition of a signal, for header titles.
fn signal_help(i18n: I18nContext<Locale, I18nKeys>, s: PriceSignal) -> String {
    match s {
        PriceSignal::ListingMin => t_string!(i18n, price_basis_listing_min_help).to_string(),
        PriceSignal::SaleMin => t_string!(i18n, price_basis_sale_min_help).to_string(),
        PriceSignal::SaleMedian => t_string!(i18n, price_basis_sale_median_help).to_string(),
        PriceSignal::SaleAvg => t_string!(i18n, price_basis_sale_avg_help).to_string(),
    }
}

/// One Worlds-to-visit line: (world id, (world name, datacenter) when
/// known, ingredient lines priced there). An alias, or the tuple trips
/// `clippy::type_complexity`.
type WorldLine = (i32, Option<(String, String)>, u16);

/// The Worlds-to-visit tooltip: "• world · ingredients: n" lines grouped
/// by datacenter in first-appearance order (a `Vec`, never a map), then the
/// datacenter count and the buy-side note. An unknown world shows its id.
/// The bullet lives in the locale string, as the sub-craft tooltip's does.
fn worlds_tooltip(i18n: I18nContext<Locale, I18nKeys>, entries: &[WorldLine], dcs: u8) -> String {
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for (id, names, n) in entries {
        let (world, dc) = match names {
            Some((w, d)) => (w.clone(), d.clone()),
            None => (id.to_string(), String::new()),
        };
        let line = t_string!(i18n, analyzer_hop_worlds_row, world = world, n = *n).to_string();
        match groups.iter_mut().find(|(g, _)| *g == dc) {
            Some((_, lines)) => lines.push(line),
            None => groups.push((dc, vec![line])),
        }
    }
    let mut out = String::new();
    for (dc, lines) in groups {
        if !dc.is_empty() {
            out.push_str(&dc);
            out.push('\n');
        }
        for line in lines {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out.push_str(&t_string!(i18n, analyzer_hop_worlds_dcs, n = dcs).to_string());
    out.push('\n');
    // A plain-key `t_string!` is already a `&'static str`.
    out.push_str(t_string!(i18n, analyzer_hop_worlds_note));
    out
}
```

Imports to add: `use crate::analyzer_kit::needed::{SignalWants, needed_signals};`, `use crate::analyzer_kit::grid::{HeaderExtra, HeaderExtras, HeaderLine2, HeaderPill};`, `use crate::analyzer_kit::columns::{PickerContext, grouped_picker_options};`, `use std::collections::BTreeSet;`.

- [ ] **Step 4: The page (`RecipeAnalyzer`)**

After `let ledger = use_lab(LAB_ANALYZER_LEDGER);`:

```rust
    let signal_cols = use_lab(LAB_ANALYZER_SIGNAL_COLUMNS);
    // Sub-crafts drive the cost-column cap; read here so the fetch gate
    // (page level) and the pass (table) agree.
    let (use_subcrafts_page, _) = filter_query_signal::<bool>(FILTER_SUBCRAFTS);
```

Replace the `sort_mode` / `visible_cols` block:

```rust
    let (cols_param, set_cols_param) = query_signal::<String>("cols");
    // `?sort=` / `?dir=` are hoisted for the same reason. A lab-only sort
    // reads as unset while the lab is off, exactly as its token did before
    // the variant existed.
    let (sort_param, _) = query_signal::<SortMode>("sort");
    let sort_mode = Memo::new(move |_| sort_param.get().filter(|m| signal_cols.get() || !m.lab_only()));
    let (sort_dir, _) = query_signal::<SortDir>("dir");
    // The lab widens the `?cols=` contract; off, the Phase D tokens drop
    // like any unknown token.
    let visible_cols = Memo::new(move |_| {
        let all: &'static [&'static str] = if signal_cols.get() {
            &OPTIONAL_COLUMN_ORDER
        } else {
            &BASE_COLUMN_ORDER
        };
        parse_visible_cols(cols_param().as_deref(), all, &DEFAULT_COLS)
    });
```

After the `visible_cols` memo (the block just replaced; `needs_page` reads `visible_cols` and `sort_mode`):

```rust
    // Which cost signals the pass runs per recipe. Computed here because
    // the buy-scope fetch key must see the sort target and the visible
    // columns. Off the lab this is exactly {selected}: today's fetches.
    let needs_page: Memo<NeededSignals> = Memo::new(move |_| {
        let f = formula_page.get();
        if signal_cols.get() {
            needed_signals(
                &f,
                &signal_wants(&visible_cols.get(), sort_mode.get()),
                use_subcrafts_page().unwrap_or(false),
            )
        } else {
            needed_signals(&f, &SignalWants::default(), false)
        }
    });
```

Replace the existing `buy_sale_stats_scope` memo (`recipe_analyzer.rs:2513-2524`) in place; everything it reads is declared above it:

```rust
    // Buy from = This world only means the sell world itself: the
    // sell-world stats body doubles as the buy-scope body (one body, not
    // two identical ones). Lab-gated so the flag-off page fetches as before.
    let buy_scope_is_sell_world = Memo::new(move |_| {
        signal_cols.get()
            && buy_scope().unwrap_or_default() == BuyScope::World
            && selected_world.get().is_some()
    });
    let buy_sale_stats_scope = Memo::new(move |_| {
        let formula = ProfitFormula::recipe_from_query(cost_basis(), None, buy_scope());
        let needs = RecipeNeeds {
            outliers: false,
            buy_scope_is_sell_world: buy_scope_is_sell_world.get(),
            cost_signals: needs_page.get().cost,
        };
        buy_stats_scope_key(&formula, &needs, buy_scope_name.get())
    });
```

Before the `view!`, the pill handler (the setters `set_cost_basis` / `set_revenue_metric` already exist at page level):

```rust
    // A header pill writes exactly one param through the filter signal
    // (no scroll-to-top, no history spam); the default is stripped like
    // the Market popover's setters do.
    let on_pill = Callback::new(move |kind: ColumnKind| match pill_param(kind) {
        Some((TermRole::Cost, s)) => {
            set_cost_basis(Some(s).filter(|s| *s != CostBasis::default()));
        }
        Some((TermRole::Revenue, s)) => {
            set_revenue_metric(Some(s).filter(|s| *s != RevenueMetric::default()));
        }
        _ => {}
    });
    let home_world_id = Memo::new(move |_| selected_world.get().map(|w| w.id).unwrap_or(0));
```

In the Suspense join, the table gains (both `.get()` reads run inside the join closure, so a lab flip or a scope change remounts the table — the header is built once per mount):

```rust
                                        signal_cols=signal_cols.get()
                                        needs=needs_page
                                        buy_stats_aliased=buy_scope_is_sell_world.get()
                                        home_world_id=home_world_id
                                        on_pill=on_pill
```

The info panel: `ToolCalculation.details` becomes reactive so the lab can append the per-signal rules sentence without touching the flag-off text. In `tool_help.rs`, change the field to `details: Signal<String>`, the constructor parameter to `details: impl Into<Signal<String>>` (with `details: details.into()`), and render it the way `formula` is rendered at `:131` (`{move || calculation.details.get()}` inside the same element that held `{calculation.details}`); the six other callers keep passing a `String` (`String: Into<Signal<String>>`). On the page, the third `ToolCalculation::new` argument becomes:

```rust
                        Signal::derive(move || {
                            let mut details = t_string!(i18n, recipe_analyzer_calc_details).to_string();
                            if signal_cols.get() {
                                details.push(' ');
                                details.push_str(t_string!(i18n, recipe_analyzer_calc_signal_semantics));
                            }
                            details
                        }),
```

- [ ] **Step 5: The table (`RecipeAnalyzerTable`)**

Props, after `strip_terms`:

```rust
    /// The analyzer-signal-columns lab: alternative columns, pills, the
    /// grouped picker, the Price tell and the "n unpriced" note. A plain
    /// bool: the page reads the lab inside its Suspense join, so a flip
    /// remounts this table (the grid's header is built once per mount).
    signal_cols: bool,
    /// The cost signals to run per recipe and the hop flags (page-level,
    /// because the fetch gate reads the same value).
    needs: Memo<NeededSignals>,
    /// The buy scope IS the sell world: reuse its stats index as the
    /// buy-scope index instead of a second identical body.
    buy_stats_aliased: bool,
    #[prop(into)]
    home_world_id: Signal<i32>,
    on_pill: Callback<ColumnKind>,
```

Setup (`:1342-1345` and `:1435-1438`) becomes:

```rust
    let sell_stats_loaded = sell_world_sale_stats.is_some();
    // Aliased = the sell body IS the buy body, so its outcome is the buy
    // outcome: a failed sell fetch degrades the cost signal too, and
    // `effective()` must see that (labels never name a signal the numbers
    // fell back from).
    let buy_stats_loaded = sale_stats.is_some() || (buy_stats_aliased && sell_stats_loaded);
    let sale_stats = sale_stats.unwrap_or_default();
    let sell_world_sale_stats = sell_world_sale_stats.unwrap_or_default();
    ...
    // Indexes are built once per payload, not once per recompute.
    let sell_stats_index: Arc<StatsIndex> = Arc::new(stats_index(&sell_world_sale_stats));
    let buy_stats_index: Option<Arc<StatsIndex>> = buy_stats_loaded.then(|| {
        if buy_stats_aliased {
            sell_stats_index.clone()
        } else {
            Arc::new(stats_index(&sale_stats))
        }
    });
```

Also amend two comments that stop being true for lab sorts: `:1201` "Pure, so a header click never re-prices." → "Pure, so a header click never re-prices by itself (a lab sort whose signal the pass has not run changes `needs`, which does)."; `:1471-1472` "a header click or a threshold edit re-runs `filter_and_sort` alone" → "… alone, unless the new sort target adds a signal to `needs`".

The `priced` memo: clone `world_names` in before the memo (`let world_names_for_pricing = world_names.clone();`), and inside the closure, before the `PriceInputs` literal:

```rust
            let needs = needs.get();
            let dc_of = |id: i32| world_names_for_pricing.get(&id).map(|(_, dc)| dc.as_str());
```

and the literal's last four fields become:

```rust
                needs: &needs,
                sell_stats_loaded,
                home_world_id: home_world_id.get(),
                dc_of: &dc_of,
```

`cell_ctx`:

```rust
    let cell_ctx = Signal::derive(move || CellCtx {
        now_unix: chrono::Utc::now().timestamp(),
        signal_columns: signal_cols,
        // `with`, not `get`: this is read once per rendered row and `get`
        // would clone both sets each time.
        capped_cost: needs.with(|n| capped_flags(&n.capped)),
    });
```

Header extras (after `marks`):

```rust
    // Line 2 and titles for the alternative-signal and hop headers. The
    // "(= …)" mark follows the *effective* formula (what the numbers use);
    // the pill's pressed state follows the *selected* one (what pressing
    // it writes). Empty with the lab off: every header renders as before.
    let header_extras = Memo::new(move |_| {
        let mut by_kind = HashMap::new();
        if !signal_cols {
            return HeaderExtras { by_kind };
        }
        let f = formula.get();
        let selected_cost = cost_basis().unwrap_or_default();
        let selected_revenue = revenue_metric().unwrap_or_default();
        for col in RECIPE_COLUMNS.iter() {
            let extra = match col.spec.kind {
                ColumnKind::RevSignal(s) => HeaderExtra {
                    title: signal_help(i18n, s),
                    line2: Some(HeaderLine2 {
                        sub_label: if s == f.revenue_signal() {
                            t_string!(i18n, analyzer_equals_price_slot).to_string()
                        } else {
                            format!("{} · {}", short_signal(i18n, s), sell_place.get())
                        },
                        pill: HeaderPill {
                            aria: t_string!(i18n, analyzer_use_as_revenue_aria, signal = signal_label(i18n, s)).to_string(),
                            pressed: s == selected_revenue,
                        },
                    }),
                },
                ColumnKind::CostSignal(s) => HeaderExtra {
                    title: signal_help(i18n, s),
                    line2: Some(HeaderLine2 {
                        sub_label: if s == f.cost_signal() {
                            t_string!(i18n, analyzer_equals_cost_slot).to_string()
                        } else {
                            format!("{} · {}", short_signal(i18n, s), buy_place.get())
                        },
                        pill: HeaderPill {
                            aria: t_string!(i18n, analyzer_use_as_cost_aria, signal = signal_label(i18n, s)).to_string(),
                            pressed: s == selected_cost,
                        },
                    }),
                },
                ColumnKind::HopGain => HeaderExtra {
                    title: t_string!(i18n, analyzer_hop_gain_help).to_string(),
                    line2: None,
                },
                ColumnKind::HopWorlds => HeaderExtra {
                    title: t_string!(i18n, analyzer_hop_worlds_help).to_string(),
                    line2: None,
                },
                _ => continue,
            };
            by_kind.insert(col.spec.kind, extra);
        }
        HeaderExtras { by_kind }
    });
```

Picker (`:1670`):

```rust
    let column_options = Signal::derive(move || {
        if signal_cols {
            let f = formula.get();
            grouped_picker_options(
                &RECIPE_COLUMNS,
                i18n,
                &PickerContext {
                    sell_place: sell_place.get(),
                    buy_place: buy_place.get(),
                    revenue: f.revenue_signal(),
                    cost: f.cost_signal(),
                    capped: needs.get().capped,
                },
            )
        } else {
            picker_options(&RECIPE_COLUMNS, i18n)
        }
    });
```

Custom cells. The `CostSlot` arm becomes a two-way branch; arm A is the current markup character for character, arm B appends the "n unpriced" line. Do **not** introduce an `Option` child into arm A (a `None` child writes a `<!>` marker and breaks flag-off identity):

```rust
            ColumnKind::CostSlot => {
                let yield_note = {
                    let data_for_yield = data.clone();
                    (data.recipe.amount_result > 1).then(|| view! {
                        <div class="text-xs text-[color:var(--color-text-muted)]">
                            {t!(i18n, recipe_analyzer_yield_note, n = move || data_for_yield.recipe.amount_result)}
                        </div>
                    })
                };
                let sub_badge = {
                    let has_sub_crafts = !data.sub_crafts.is_empty();
                    let sub_crafts = data.sub_crafts.clone();
                    view! {
                        <Show when=move || has_sub_crafts>
                            {
                                let sub_crafts_for_text = sub_crafts.clone();
                                let count = sub_crafts.len();
                                view! {
                                    <Tooltip
                                        tooltip_text={
                                            let sub_crafts_details: Vec<(String, i32, i32)> = sub_crafts_for_text.iter().map(|sub| {
                                                let name = items.get(&sub.item_id).map(|i| i.name.to_string()).unwrap_or("Unknown".to_string());
                                                (name, sub.amount, sub.unit_cost)
                                            }).collect();
                                            Signal::derive(move || {
                                                let mut tooltip = t_string!(i18n, recipe_analyzer_subcraft_header).to_string();
                                                for (name, amount, cost) in &sub_crafts_details {
                                                    tooltip.push_str(
                                                        &t_string!(i18n, recipe_analyzer_subcraft_row, count = *amount, name = name.clone(), gil = *cost).to_string(),
                                                    );
                                                }
                                                tooltip
                                            })
                                        }
                                    >
                                        <div class="text-xs text-brand-300 flex items-center justify-end gap-1 cursor-help">
                                            <Icon icon=i::FaHammerSolid width="0.8em" height="0.8em" />
                                            <span>{count} " " {t!(i18n, recipe_analyzer_sub_suffix)}</span>
                                        </div>
                                    </Tooltip>
                                }
                            }
                        </Show>
                    }
                };
                if signal_cols && data.unpriced > 0 {
                    let n = data.unpriced;
                    view! {
                        <div role="cell" class=class>
                            <Gil amount=data.cost />
                            {yield_note}
                            {sub_badge}
                            <div
                                class="text-[10px] leading-3 text-amber-300 cursor-help"
                                title=t_string!(i18n, analyzer_cost_unpriced_title, n = n).to_string()
                            >
                                {t_string!(i18n, analyzer_cost_unpriced, n = n).to_string()}
                            </div>
                        </div>
                    }
                    .into_any()
                } else {
                    view! {
                        <div role="cell" class=class>
                            <Gil amount=data.cost />
                            {yield_note}
                            {sub_badge}
                        </div>
                    }
                    .into_any()
                }
            }
```

Add the `HopWorlds` arm before `ColumnKind::Actions`:

```rust
            ColumnKind::HopWorlds => {
                let (count, tooltip) = match &data.worlds {
                    Some(w) => {
                        let entries: Vec<WorldLine> = w
                            .worlds
                            .iter()
                            .map(|(id, n)| (*id, world_names_for_cells.get(id).cloned(), *n))
                            .collect();
                        (Some(w.worlds.len()), worlds_tooltip(i18n, &entries, w.dcs))
                    }
                    None => (None, t_string!(i18n, analyzer_hop_worlds_note).to_string()),
                };
                let text = count.map(|c| c.to_string()).unwrap_or_else(|| "—".to_string());
                let muted = if count.is_some() { "" } else { "text-[color:var(--color-text-muted)]" };
                // `Tooltip`'s children are an `Fn` closure: clone, never move.
                view! {
                    <div role="cell" class=class>
                        <Tooltip tooltip_text=Signal::derive(move || tooltip.clone())>
                            <span class=muted>{text.clone()}</span>
                        </Tooltip>
                    </div>
                }
                .into_any()
            }
```

Grid:

```rust
                    marks=marks
                    extras=header_extras
                    on_pill=on_pill
                    lab_columns=signal_cols
```

- [ ] **Step 6: Run the route tests and check both targets**

Run: `cargo test -p ultros-app --lib -- routes::recipe_analyzer`
Expected: PASS.
Run: `cargo check -p ultros-app --features ssr && cargo check -p ultros-app --features hydrate --target wasm32-unknown-unknown`
Expected: both OK (the `dc_of` borrow inside the memo, the `Callback` in the join and the `Memo<Option<SortMode>>` prop type are the likely trip-ups; fix in place).

- [ ] **Step 7: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs ultros-frontend/ultros-app/src/components/tool_help.rs
git commit -m "feat(recipe-analyzer): signal columns lab — fetch gate, header pills, grouped picker, hop and unpriced cells"
```

---

### Task 11: Changelog, e2e route, CI gate, measurements and the PR body

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/changelog.rs:33-39` (new top entry)
- Modify: `integration/runner.cjs:79-86` (`ROUTE_ASSERTS`) and `:132-133` (`getRoutes()`)
- Create (scratchpad, not committed): `phase-d-pr-body.md`

**Interfaces:**
- Consumes: everything above.
- Produces: a green `./check_ci.sh`, the PR text, the recorded measurements.

- [ ] **Step 1: Changelog**

Insert at the top of `CHANGELOG` (above the Phase C entry; same date is fine, `entries_are_sorted_newest_first` accepts equal dates):

```rust
    ChangelogEntry {
        date: "2026-09-02",
        title: "Recipe Analyzer: every price signal is a column you can sort, and Hop gain tells you whether the trip to another world pays (Labs)",
        blurb: "Turn on \"Recipe Analyzer: price signals as columns\" under Settings › Labs. The Columns picker gains a column for every cost and revenue signal, each with a \"use\" pill that makes it the formula's input, plus Hop gain / unit (what buying at home would cost minus buying across the buy scope) and Worlds to visit (which worlds hold the cheapest ingredients). Rows with an ingredient that has no listing and no vendor now say how many are unpriced. For everyone, with sub-crafts on, an unlisted intermediate that can be crafted is now costed as a craft instead of as free, so Cost / unit rises on those rows.",
        link: Some("/settings"),
    },
```

Run: `cargo test -p ultros-app --lib -- changelog`
Expected: PASS.

- [ ] **Step 2: e2e route**

In `ROUTE_ASSERTS`, after the `&labs=analyzer-ledger` entry:

```js
  // Both labs on with the four Phase D columns requested. The new columns
  // are md+ only, so the only cross-device assertion is the title; the
  // sweep still checks console errors and horizontal overflow.
  "/recipe-analyzer?world=Gilgamesh&labs=analyzer-ledger,analyzer-signal-columns&cols=confidence,cost-sale-median,rev-sale-median,hop-gain,hop-worlds": {
    titleIncludes: "Recipe Analyzer",
  },
```

and the same path string in `getRoutes()` after the ledger route. (Asserts only run for routes in `getRoutes()`.)

- [ ] **Step 3: fmt, clippy, tests**

```bash
cargo fmt --all
cargo test -p ultros-app --lib > /tmp/tests.log 2>&1; echo "REAL_EXIT=$?"; tail -5 /tmp/tests.log
cargo test -p ultros-api-types > /tmp/tests2.log 2>&1; echo "REAL_EXIT=$?"; tail -3 /tmp/tests2.log
export PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:$PATH"
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

Expected: every `REAL_EXIT=0`. Clippy runs `--all-targets` with `-D warnings`: fix every warning in place (no `#[allow]`). The usual suspects from this phase: an unused import left by a task; `needless_borrow` / `unnecessary_to_owned` on plain-key `t_string!` results (they are `&'static str`: never `&t_string!(..)` or `&t_string!(..).to_string()` in a `&str` position); `clippy::type_complexity` on any tuple slice or `Vec` that escaped the `WorldLine` alias; `manual_is_multiple_of`. `header_cell` sits exactly at the seven-parameter threshold (`too_many_arguments` fires only above it) — do not add an eighth.

Commit the fixes with `git commit -am "chore(phase-d): fmt and clippy"` if any.

- [ ] **Step 4: Measurements**

Record in the PR body:

1. **Timing (compute_cost calls)** — from the debug log line `price_rows: N recipes priced in X ms (C compute_cost calls, hop H)` in a hydrate debug build (`cargo leptos watch` or the running dev server), on `/recipe-analyzer?world=Gilgamesh&labs=analyzer-signal-columns`: default (K=1), `&cols=confidence,cost-sale-median` (K=2), `&cols=confidence,cost-listing-min,cost-sale-min,cost-sale-median,cost-sale-avg` (K=4), each with and without `&subcrafts=true` (the cap makes the last one K=3 with sub-crafts). If no local build is available in the session, write "owed" next to the table and say so in the PR.
2. **Sub-craft rescue delta** — with `?world=Gilgamesh&subcrafts=true` (no lab needed): the "N recipes" count and the first ten rows sorted by `cost` ascending, on prod (`https://ultros.app`) and on the local build; the difference is the rescue. If no local build: "owed".

- [ ] **Step 5: PR body**

Write `phase-d-pr-body.md` in the scratchpad from this template, filling the measurements:

```markdown
# Analyzer kit phase D: price signals as columns, "use" pills, Hop gain (Labs)

**Base branch: `main`** (#1257, Phase C, merged as 190ea7cd; this branch is rebased onto it).

Part of #1233. Everything here is behind Settings › Labs › "Recipe Analyzer: price signals as columns" (or `?labs=analyzer-signal-columns`). **Flag off = the page renders, requests and computes exactly as the base branch, on every URL without `subcrafts=true`.** The one number change for everyone is the sub-craft rescue (below). Plan: `docs/superpowers/plans/2026-09-02-analyzer-kit-phase-d-signal-columns.md`.

## What's in it

- **Ten columns** in the Columns picker under grouped headings — Revenue · ‹sell world› and Cost · ‹buy scope› (Cheapest listing, Sale minimum, Sale median, Sale average), Travel (Hop gain / unit, Worlds to visit), Other (the existing seven). `?cols=` and `?sort=` accept the ten new tokens appended after the seven; every old URL is byte-identical; `DEFAULT_COLS` stays `[confidence]`.
- **Alternative signal columns** render muted with an always-present delta sub-line against the same-side formula input ("+38%", title "vs the formula's cost input"). The entry equal to the selected signal is marked "(= Cost / unit)" / "(= Price)".
- **"use" pills** on every alternative column's header: `<button type=button aria-pressed>` writing exactly one URL param (`cost-basis` or `revenue`) through `filter_query_signal`. The pressed column stays on screen as a muted duplicate with its pill filled and disabled.
- **Hop gain / unit**: home cost (sell-world listings alone, under the selected cost signal) minus buy-scope cost per unit, signed, never clamped; "needed" when an ingredient has no home listing and no vendor; "—" under Buy from = This world only or when the scope run is unpriced. Title: "≈ 13.5k gil/day at 6.3 sales/day". **Worlds to visit**: distinct non-home worlds holding the cheapest listing of a top-level ingredient, tooltip grouped by datacenter. Both buy side only; sub-craft materials not counted. Zero new network.
- **Fetch gate**: the buy-scope stats body is fetched iff a sale cost signal is selected, visible or the sort target, and not when Buy from = This world only (the sell-world index is reused). `needed_signals` enforces the sub-craft cap (selected + two extra runs) so it holds for bookmarked URLs, identically on SSR and CSR.
- **Price slot "listing" tell** when the shown price is not the selected signal on the sell world; **"n unpriced"** note on Cost / unit for ingredients with no listing and no vendor (they cost 0; row membership unchanged).
- **Pricing core**: `PriceSummary::chosen`, `IngredientLine.world_id`, `CostBreakdown.unpriced_market_lines`, and the **sub-craft rescue** (`sub_unit > 0 && (unit_cost == 0 || sub_unit < unit_cost)`): an unlisted intermediate that can be crafted is no longer free with sub-crafts on. **Not lab-gated** — it is the phase's declared number change, changelog'd, delta below.
- 28 i18n keys × 7 locales (machine-translated; native spot-check wanted, in particular the two `analyzer_hop_*_help` sentences and `recipe_analyzer_calc_signal_semantics`).
- Info panel: under the lab the "Ingredient policy" details gain the per-signal rules sentence (`ToolCalculation.details` is now reactive; the six other tools are unchanged).
- Changelog entry; one e2e route with both labs on.

## Numbers

| View | K | ms (no sub-crafts) | ms (sub-crafts) |
|---|---|---|---|
| default | 1 | … | … |
| + cost-sale-median | 2 | … | … |
| + hop-gain, hop-worlds | 1 (+1 home run) | … | … |
| + four cost columns | 4 (3 with sub-crafts) | … | … |
| + four cost columns + hop-gain | 5 (4 with sub-crafts) | … | … |

Sub-craft rescue delta with `subcrafts=true` on Gilgamesh: prod N rows / local M rows; first ten by cost: …

## Verification

- `cargo test -p ultros-app --lib`: … passed; `cargo test -p ultros-api-types`: … passed.
- `./check_ci.sh`: `REAL_EXIT=0`, no `#[allow]` added.
- Seven locale files parse; every new key present exactly once in each.

## Manual checks (reviewer step; not run in this pass unless stated)

Lab off, `/recipe-analyzer?world=Gilgamesh`:
1. header rowgroup and first-row outerHTML identical to the base branch (both a yield>1 row and a sub-craft row; the Cost / unit cell has no "unpriced" line, the Price cell no sub-line; the header carries no extra `<!>` markers — that is the grid's `lab_columns` filter, not `BASE_COLUMN_ORDER` alone); Columns picker is the flat list of seven.
2. `?cols=hop-gain,cost-sale-median` and `?sort=hop-gain`: nothing new renders, sort falls back to Profit, no extra request.

Lab on (`&labs=analyzer-signal-columns`, then via the Settings toggle):
3. Columns picker shows four groups in order; the Cost heading carries the loads-once title; ticking Sale median under Cost fetches `/api/v1/sale_stats/Aether?window=7` once and remounts; ticking a Revenue column fetches nothing.
4. Alternative columns are muted with a delta sub-line; the selected one reads "(= Cost / unit)" with a filled, disabled pill; pressing another column's pill changes exactly `cost-basis=` (or `revenue=`) in the URL, the badge/tint move to the slot, the row numbers recompute without reload.
5. Buy from = This world only: no buy-scope stats request even with Sale median selected; Hop gain shows "—", Worlds "—".
6. Tick Hop gain and Worlds to visit (no network): a "needed" row has an ingredient with no Gilgamesh listing and no vendor; sort by Hop gain desc then asc: "needed"/"—" rows last both ways; Worlds tooltip groups by datacenter and ends with the buy-side note; HopWorlds sorts ascending by default.
7. `subcrafts=true` with four cost columns ticked: the capped ones show the hint and render "—" with the cap title, and remain untickable; an unticked cost entry is greyed while the cap is full.
8. Block `/api/v1/sale_stats*`: cost-sale-* columns show "—", the selected sale basis degrades with the amber dot (Phase C), Hop gain under a sale basis matches the listing pass. Also with Buy from = This world only: the Cost chip's amber dot lights and the cost-sale-* cells show "—" (the aliased body's failure is the buy side's failure).
9. 375px: no horizontal overflow; new columns hidden; picker groups wrap.
10. Locale de and ja: headers fit w-40 or truncate with the full label on hover.
11. `./scripts/run_e2e.sh` on both devices.

Aaron validates the hop semantics on this PR (no Kosyne ping per the 2026-09-02 decision).
```

- [ ] **Step 6: Commit and push**

```bash
git add ultros-frontend/ultros-app/src/routes/changelog.rs integration/runner.cjs
git commit -m "docs(changelog): Phase D signal columns, hop gain and the sub-craft rescue; e2e route"
git push -u origin claude/issue-1233-phase-d-signal-columns
```

Open the PR against `main` with the body above (`gh pr create --base main --title "Analyzer kit phase D: price signals as columns, use pills, Hop gain (Labs)" --body-file <path>`). If `main` has moved, rebase first (`git fetch origin && git rebase origin/main`), resolve any changelog conflict (keep every entry, ours on top) and re-run `./check_ci.sh`.

---

## Self-review (done while writing; kept for the executor)

**Spec coverage.** Every item of the v1 Phase 2 paragraph maps to a task: `SIGNAL_COLUMNS` + hop tokens → Task 9; `PICKER_COLUMNS` with groups, "(= Price)", loads-once title, cap note → Tasks 5, 6, 10; `SortMode` variants → Task 9; page-level `needed_signals` re-gating fetch 2 and the buy-equals-sell reuse → Tasks 3, 10; `PricedRecipe` alt/hop/unpriced fields → Task 8; muted cells with delta sub-line → Tasks 5, 9; "use" pills → Tasks 7, 10; Price "listing" sub-line → Tasks 5, 9; "n unpriced" note → Task 10; `IngredientLine.world_id` + `PriceSummary::chosen` + `unpriced_market_lines` + the rescue → Task 2; hop cells and tooltips → Tasks 5, 10; i18n → Task 1; changelog → Task 11; the Labs token and its off-state → Tasks 1, 9, 10. Every named test exists under its spec name except `chosen_matches_lowest_gil_and_prefer_hq_with_tie_rule` (api-types crate) and `needed_signals_sets_hop_when_a_hop_column_is_the_sort_target` (split between needed.rs and the route's `signal_wants` test). The route-level flag-off shape test kit §11 names is a manual diff, as in Phase C, because the custom cells live inside `RecipeAnalyzerTable`; the component paths are pinned (`sort_header` unset-trailing, `header_cell` empty-extras, the grid's `lab_columns` test).

**Not in this plan, by decision:** `HopInfo` as one struct (two row fields instead); `signed_delta_class` (the delta sub-line is muted, not coloured); `layers.rs` (Phase E1); Market / Location picker groups (E2); Scope vs home (F); the Kosyne validation ping (dropped).

**Type consistency checklist for the executor:** `CellCtx` has three fields everywhere (Tasks 5, 9, 10); `ToolColumnMeta.lab` exists before Task 9's table; `ColumnSpec.group` before the route's specs; `RecipeNeeds` is `Clone` not `Copy` (Task 3) so the route memo moves a fresh set each time; `compare_recipes` takes `dir` (Task 9) and `filter_and_sort` no longer reverses; `header_cell` takes seven parameters (Task 7) and both header call sites pass the two new ones; `price_rows` returns `(rows, compute_cost calls)` from Task 8 on and every caller destructures it; `PriceInputs` has four new fields and the test runner (`run_with`) sets all of them with a closure for `dc_of`; `RecipeAnalyzerTable.signal_cols` is a `bool` (Task 10), so nothing inside the table calls `.get()` on it.
