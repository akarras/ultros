# Analyzer Kit — Phase C: Labs Toggle, Formula Strip, Marked Headers — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the first player-visible piece of the design, the live profit formula as a control with the columns that feed it marked, behind a "Labs" toggle so it can run on prod for testers before it becomes the default; with the toggle off the page is pixel-identical to Phase B.

**Architecture:** A cookie-backed `Labs` set (`global_state/labs.rs`) with a Settings section and a `?labs=` URL override gates experiments; the server reads the cookie, so SSR and hydration agree. `components/term_badge.rs` holds the operator vocabulary shared by the strip and the headers. `analyzer_kit/strip.rs` renders the formula ledger from a list of terms in an inline or stacked layout. `SortableHeaderCell` gains optional `title`, `sub_label`, `badge` and `emphasized` props. The recipe analyzer, under the flag, adds the strip row, swaps the Market popover body for the stacked strip, marks Profit / Cost / Price, adds a per-row arithmetic readout, clamps ROI, and makes the info panel's formula sentence live. Spec: `docs/superpowers/specs/2026-09-01-analyzer-kit-design.md` sections 4 and 11, Phase C; UI details in `2026-09-01-recipe-analyzer-profit-formula-columns-design.md` "UI".

**Tech Stack:** Rust 2024, Leptos 0.8, leptos_i18n 0.6 (7 locales), `Cookies::use_cookie_typed`, Tailwind v4 utilities already in the repo-root `style/tailwind.css`.

## Global Constraints

- Requires Phases A and B merged. PR against `main`.
- Every new user-facing string in all 7 locale files (`en, fr, de, ja, cn, ko, tc`) with real translations; `snake_case` keys, prefixes `labs_`, `formula_`, `recipe_analyzer_`.
- Flag off ⇒ pixel-identical to Phase B (the Phase B manual parity checklist is re-run with the flag off). Flag on ⇒ the design's Phase C surface. The ROI clamp applies only with the flag on.
- Nothing enters row 1 of the 76 px ControlBar; the strip is its own row under "Sell on" (`hidden md:flex flex-wrap`) and the Market popover widens from `16rem` to `20rem`.
- Hydration: the strip derives only from URL params and the static world tree; the degraded amber dot appears post-hydration via an Effect-written signal; no resource is read outside Suspense.
- No red/green: marks use `--brand-ring` / `--brand-fg` tints and a bottom hairline.
- Changelog entry at the top of `CHANGELOG` in `routes/changelog.rs` (player-facing, mentions the Labs toggle and the ROI clamp).
- `./check_ci.sh` clean; `cargo test -p ultros-app --lib` green locally.

---

## File map

| File | Responsibility |
|---|---|
| `ultros-frontend/ultros-app/src/global_state/labs.rs` (new) | `Labs` cookie type, flag tokens, `use_lab` |
| `ultros-frontend/ultros-app/src/global_state/mod.rs` (modify) | register `labs` |
| `ultros-frontend/ultros-app/src/routes/settings.rs` (modify) | `LabsSettings` section |
| `ultros-frontend/ultros-app/src/components/term_badge.rs` (new) | `TermRole`, `TermBadge` |
| `ultros-frontend/ultros-app/src/components/mod.rs` (modify) | register `term_badge` |
| `ultros-frontend/ultros-app/src/components/sort_header.rs` (modify) | optional header props |
| `ultros-frontend/ultros-app/src/components/tool_help.rs` (modify) | reactive `ToolCalculation.formula` |
| `ultros-frontend/ultros-app/src/analyzer_kit/strip.rs` (new) | `StripTerm`, `FormulaStrip`, `StripLayout` |
| `ultros-frontend/ultros-app/src/analyzer_kit/formula.rs` (modify) | `RoiMath::ClampedF64`, `FormulaMarks` |
| `ultros-frontend/ultros-app/src/analyzer_kit/columns.rs` (modify) | `ToolColumnMeta.side`, `formula_header_class`, `formula_cell_class` |
| `ultros-frontend/ultros-app/src/analyzer_kit/grid.rs` (modify) | `marks` prop → marked headers and widened cells |
| `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs` (modify) | strip row, popover, readout, live sentence, degraded dot, dead keys wired |
| `ultros-frontend/ultros-app/locales/*.json` (modify) | new keys |
| `integration/runner.cjs` (modify) | `/recipe-analyzer` routes |
| `ultros-frontend/ultros-app/src/routes/changelog.rs` (modify) | entry |

---

### Task 1: The Labs toggle

**Files:**
- Create: `ultros-frontend/ultros-app/src/global_state/labs.rs`
- Modify: `ultros-frontend/ultros-app/src/global_state/mod.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/settings.rs:340-361` (`Settings`)
- Modify: all 7 `locales/*.json`

**Interfaces:**
- Consumes: `Cookies::use_cookie_typed` (`global_state/cookies.rs:73`), `Toggle` (`components/toggle.rs`), `use_query_map`.
- Produces:
  - `pub const LABS_COOKIE: &str = "LABS"`, `pub const LAB_ANALYZER_LEDGER: &str = "analyzer-ledger"`, `pub const LABS: &[LabInfo]` with `pub struct LabInfo { pub token: &'static str }` (each entry's comment names the phase that deletes it)
  - `pub struct Labs { pub enabled: BTreeSet<String> }` with `FromStr` (comma list, unknown tokens dropped) and `Display`
  - `pub fn use_lab(token: &'static str) -> Signal<bool>` (cookie set OR `?labs=` list contains the token)
  - `#[component] fn LabsSettings()` in `settings.rs`

- [ ] **Step 1: Write the failing tests** (in `labs.rs`)

```rust
//! Experiments a player can switch on before they become the default.
//! A cookie, not localStorage: the analyzers render on the server, so a
//! client-only flag would hydrate a different page than it served.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labs_cookie_round_trips_known_tokens_only() {
        let labs: Labs = "analyzer-ledger,bogus,,analyzer-ledger".parse().unwrap();
        assert_eq!(labs.enabled.len(), 1);
        assert!(labs.has(LAB_ANALYZER_LEDGER));
        assert_eq!(labs.to_string(), "analyzer-ledger");
        let empty: Labs = "".parse().unwrap();
        assert!(!empty.has(LAB_ANALYZER_LEDGER));
        assert_eq!(empty.to_string(), "");
    }

    #[test]
    fn the_experiment_list_stays_short() {
        assert!(LABS.len() <= 3, "keep the experiment list short");
    }
}
```

- [ ] **Step 2: Register and run to verify failure**

Add `pub mod labs;` to `global_state/mod.rs`. Run: `cargo test -p ultros-app --lib global_state::labs`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

```rust
use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use leptos::prelude::*;
use leptos_router::hooks::use_query_map;

use super::cookies::Cookies;

pub const LABS_COOKIE: &str = "LABS";

/// The recipe analyzer's formula strip, marked headers and live info
/// panel (kit Phase C).
pub const LAB_ANALYZER_LEDGER: &str = "analyzer-ledger";

pub struct LabInfo {
    pub token: &'static str,
}

/// Every live experiment. Adding one here is what makes it appear in
/// Settings; deleting it is part of shipping the feature. Each entry's
/// comment names the phase that deletes it (a struct field for that would
/// have no non-test reader, which `-D warnings` rejects).
pub const LABS: &[LabInfo] = &[
    // Deleted in kit Phase D, after Kosyne has used the strip.
    LabInfo { token: LAB_ANALYZER_LEDGER },
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Labs {
    pub enabled: BTreeSet<String>,
}

impl Labs {
    pub fn has(&self, token: &str) -> bool {
        self.enabled.contains(token)
    }
}

fn is_known(token: &str) -> bool {
    LABS.iter().any(|l| l.token == token)
}

impl FromStr for Labs {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self {
            enabled: s
                .split(',')
                .map(str::trim)
                .filter(|t| !t.is_empty() && is_known(t))
                .map(String::from)
                .collect(),
        })
    }
}

impl fmt::Display for Labs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.enabled.iter().cloned().collect::<Vec<_>>().join(","))
    }
}

/// Whether an experiment is on for this view: the cookie set, or the
/// `?labs=` list in the URL (for sharing a link with a tester).
pub fn use_lab(token: &'static str) -> Signal<bool> {
    let cookie = use_context::<Cookies>().map(|c| c.use_cookie_typed::<_, Labs>(LABS_COOKIE).0);
    let query = use_query_map();
    Signal::derive(move || {
        let from_cookie = cookie.is_some_and(|c| c.get().is_some_and(|l| l.has(token)));
        let from_url = query.with(|q| {
            q.get("labs")
                .and_then(|v| v.parse::<Labs>().ok())
                .is_some_and(|l| l.has(token))
        });
        from_cookie || from_url
    })
}
```

- [ ] **Step 4: The Settings section**

In `settings.rs`, add `use leptos_i18n::I18nContext;` to the imports (the `crate::i18n::*` glob does not carry it), then add before `pub fn Settings`:

```rust
#[component]
fn LabsSettings() -> impl IntoView {
    use crate::global_state::labs::{LABS, LABS_COOKIE, Labs};
    let cookies = use_context::<Cookies>().unwrap();
    let (labs, set_labs) = cookies.use_cookie_typed::<_, Labs>(LABS_COOKIE);
    let i18n = use_i18n();
    view! {
        <div class="panel p-6 rounded-xl">
            <h3 class="text-2xl font-bold text-[color:var(--brand-fg)] mb-2">{t!(i18n, labs_title)}</h3>
            <p class="text-sm text-[color:var(--color-text-muted)] mb-4">{t!(i18n, labs_desc)}</p>
            <div class="flex flex-col gap-4">
                {LABS.iter().map(|lab| {
                    let token = lab.token;
                    view! {
                        <div class="grid md:grid-cols-3 gap-4 items-center">
                            <div class="col-span-2">
                                <div class="font-semibold text-[color:var(--color-text)]">{lab_title(i18n, token)}</div>
                                <div class="text-sm text-[color:var(--color-text-muted)]">{lab_desc(i18n, token)}</div>
                            </div>
                            <Toggle
                                checked=Signal::derive(move || labs().unwrap_or_default().has(token))
                                set_checked=(move |checked: bool| {
                                    let mut current = labs.get_untracked().unwrap_or_default();
                                    if checked { current.enabled.insert(token.to_string()); } else { current.enabled.remove(token); }
                                    set_labs(if current.enabled.is_empty() { None } else { Some(current) });
                                }).into_signal_setter()
                                checked_label=t_string!(i18n, labs_on)
                                unchecked_label=t_string!(i18n, labs_off)
                            />
                        </div>
                    }
                }).collect_view()}
            </div>
        </div>
    }
    .into_any()
}

fn lab_title(i18n: I18nContext<Locale, I18nKeys>, token: &str) -> String {
    match token {
        crate::global_state::labs::LAB_ANALYZER_LEDGER => t_string!(i18n, labs_analyzer_ledger_title).to_string(),
        _ => token.to_string(),
    }
}

fn lab_desc(i18n: I18nContext<Locale, I18nKeys>, token: &str) -> String {
    match token {
        crate::global_state::labs::LAB_ANALYZER_LEDGER => t_string!(i18n, labs_analyzer_ledger_desc).to_string(),
        _ => String::new(),
    }
}
```

and mount it in `Settings` after `<AdChoice />`: `<LabsSettings />`.

- [ ] **Step 5: i18n keys** (add to every locale; English values below, translate the other six)

```
labs_title                    "Labs"
labs_desc                     "Features still being finished. Turn one on to try it before everyone gets it."
labs_on                       "On"
labs_off                      "Off"
labs_analyzer_ledger_title    "Recipe Analyzer: profit formula strip"
labs_analyzer_ledger_desc     "Shows the profit formula as a control above the table and marks the columns that feed it."
```

- [ ] **Step 6: Run tests, clippy, commit**

Run: `cargo test -p ultros-app --lib global_state::labs && cargo fmt --all && cargo clippy -p ultros-app --all-targets -- -D warnings`
Expected: tests PASS and fmt clean; clippy reports exactly one warning, `function \`use_lab\` is never used`, which Task 5 removes — do not silence it.

```bash
git add ultros-frontend/ultros-app/src/global_state ultros-frontend/ultros-app/src/routes/settings.rs ultros-frontend/ultros-app/locales
git commit -m "feat(settings): Labs toggle for experiments (cookie + ?labs= override)"
```

---

### Task 2: `TermBadge` and the header props

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/term_badge.rs`
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs`
- Modify: `ultros-frontend/ultros-app/src/components/sort_header.rs:189-220`
- Modify: all 7 locales

**Interfaces:**
- Produces:
  - `pub enum TermRole { Result, Revenue, Tax, Cost }` (derives `Hash`: it keys `MarkLabels`) with `fn glyph(self) -> &'static str` (`=`, `+`, `−`, `−`)
  - `#[component] pub fn TermBadge(role: TermRole) -> impl IntoView` — a 16 px bordered mono square with the glyph `aria-hidden` and an `sr-only` role name
  - `SortableHeaderCell` optional props: `#[prop(optional, into)] title: Option<String>`, `#[prop(optional, into)] sub_label: Option<Signal<String>>`, `#[prop(optional)] badge: Option<TermRole>`, `#[prop(optional, into)] emphasized: Option<Signal<bool>>`

- [ ] **Step 1: Write the failing test** (in `sort_header.rs` tests, next to `renders_a_relative_href_without_router_context`)

```rust
    #[test]
    fn header_cell_renders_badge_sub_label_and_emphasis() {
        // `TermBadge` builds an I18nContext (spawns an Effect) and `<Gil>`
        // reads it: stand up the executor and the context, as
        // components/list/filter_row.rs's tests do.
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let html = view! {
                <SortableHeaderCell
                    mode=Col::Cost
                    label="Cost"
                    class="w-40"
                    sort_mode=Signal::derive(|| None::<Col>)
                    sort_dir=Signal::derive(|| None::<SortDir>)
                    badge=crate::components::term_badge::TermRole::Cost
                    sub_label=Signal::derive(|| "listing · Aether".to_string())
                    emphasized=Signal::derive(|| true)
                />
            }
            .to_html();
            assert!(html.contains("listing · Aether"), "{html}");
            assert!(html.contains("aria-hidden=\"true\""), "{html}");
            assert!(html.contains("sr-only"), "{html}");
            assert!(html.contains("shadow-[inset_0_-2px_0_var(--brand-ring)]"), "{html}");
            // Without the props the markup is what it was.
            let plain = view! {
                <SortableHeaderCell
                    mode=Col::Cost
                    label="Cost"
                    class="w-32"
                    sort_mode=Signal::derive(|| None::<Col>)
                    sort_dir=Signal::derive(|| None::<SortDir>)
                />
            }
            .to_html();
            assert!(!plain.contains("sr-only"), "{plain}");
            assert!(!plain.contains("min-w-0"), "{plain}");
        });
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p ultros-app --lib sort_header::test::header_cell_renders_badge`
Expected: FAIL to compile (unknown props).

- [ ] **Step 3: Implement `term_badge.rs`**

```rust
//! The formula's own arithmetic as a legend: `=` result, `+` revenue,
//! `−` tax, `−` cost. Palette-safe (brand tokens only) and readable by
//! screen readers through an sr-only role name.

use leptos::prelude::*;

use crate::i18n::*;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TermRole {
    Result,
    Revenue,
    Tax,
    Cost,
}

impl TermRole {
    pub fn glyph(self) -> &'static str {
        match self {
            TermRole::Result => "=",
            TermRole::Revenue => "+",
            TermRole::Tax | TermRole::Cost => "−",
        }
    }
}

#[component]
pub fn TermBadge(role: TermRole) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let name = move || match role {
        TermRole::Result => t_string!(i18n, formula_role_result).to_string(),
        TermRole::Revenue => t_string!(i18n, formula_role_revenue).to_string(),
        TermRole::Tax => t_string!(i18n, formula_role_tax).to_string(),
        TermRole::Cost => t_string!(i18n, formula_role_cost).to_string(),
    };
    view! {
        <span class="inline-flex items-center justify-center w-4 h-4 rounded border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_20%,transparent)] text-[color:var(--brand-fg)] font-mono text-[10px] font-bold leading-none shrink-0">
            <span aria-hidden="true">{role.glyph()}</span>
            <span class="sr-only">{name}</span>
        </span>
    }
}
```

Register `pub mod term_badge;` in `components/mod.rs`.

- [ ] **Step 4: Extend `SortableHeaderCell`**

```rust
#[component]
pub fn SortableHeaderCell<M>(
    mode: M,
    #[prop(into)] label: String,
    #[prop(into, optional)] class: String,
    #[prop(into)] sort_mode: Signal<Option<M>>,
    #[prop(into)] sort_dir: Signal<Option<SortDir>>,
    #[prop(optional)] reset_keys: &'static [&'static str],
    /// Hover text for the whole header cell.
    #[prop(optional, into)]
    title: Option<String>,
    /// A muted 10px second line ("listing · Aether"). When set the cell
    /// lays out as two lines; pass `px-3 py-2 leading-tight` in `class`.
    #[prop(optional, into)]
    sub_label: Option<Signal<String>>,
    /// The formula operator this column plays, rendered before the label.
    #[prop(optional)]
    badge: Option<TermRole>,
    /// Brand tint plus a bottom hairline: this column feeds Profit.
    #[prop(optional, into)]
    emphasized: Option<Signal<bool>>,
) -> impl IntoView
where
    M: SortColumn,
{
    let two_line = sub_label.is_some();
    let cell_class = move || {
        let mut c = class.clone();
        if two_line {
            c.push_str(" flex flex-col justify-center gap-0.5");
        }
        if emphasized.is_some_and(|e| e.get()) {
            c.push_str(" bg-[color:color-mix(in_srgb,var(--brand-ring)_18%,transparent)] shadow-[inset_0_-2px_0_var(--brand-ring)]");
        }
        c
    };
    view! {
        <div
            role="columnheader"
            class=cell_class
            title=title
            aria-sort=move || column_aria_sort(mode, sort_mode, sort_dir)
        >
            {match badge {
                Some(role) => view! {
                    <div class="flex items-center gap-2 min-w-0">
                        <TermBadge role=role />
                        <SortHeader mode label sort_mode sort_dir reset_keys />
                    </div>
                }
                .into_any(),
                None => view! { <SortHeader mode label sort_mode sort_dir reset_keys /> }.into_any(),
            }}
            {sub_label.map(|s| view! {
                <div class="text-[10px] leading-3 font-normal normal-case text-[color:var(--color-text-muted)] truncate max-w-full">{move || s.get()}</div>
            })}
        </div>
    }
    .into_any()
}
```

Add `use crate::components::term_badge::{TermBadge, TermRole};` at the top. `title=title` on an `Option<String>` renders no attribute when `None`, and the badge wrapper exists only when a badge is passed, so the 35 existing call sites produce byte-identical markup (the Step 1 test's `plain` assertions pin this).

- [ ] **Step 5: i18n keys**

```
formula_role_result    "result"
formula_role_revenue   "adds to profit"
formula_role_tax       "market board tax"
formula_role_cost      "subtracted cost"
```

- [ ] **Step 6: Run tests, commit**

Run: `cargo test -p ultros-app --lib sort_header && cargo fmt --all`

```bash
git add ultros-frontend/ultros-app/src/components ultros-frontend/ultros-app/locales
git commit -m "feat(ui): TermBadge and optional title/sub-label/badge/emphasis on SortableHeaderCell"
```

---

### Task 3: `FormulaStrip`, `FormulaMarks`, the live sentence and the ROI clamp

**Files:**
- Create: `ultros-frontend/ultros-app/src/analyzer_kit/strip.rs`
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/formula.rs` (add `RoiMath::ClampedF64`, `FormulaMarks`, `sentence`)
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/mod.rs` (add `pub mod strip;`)
- Modify: `ultros-frontend/ultros-app/src/components/tool_help.rs:8-27`

**Interfaces:**
- Produces:
  - `RoiMath::ClampedF64` (uses `crate::analysis::return_on_investment`)
  - `pub struct FormulaMarks { pub revenue: PriceSignal, pub cost: PriceSignal, pub sell_place: String, pub buy_place: String }` with `ProfitFormula::marks(&self, sell_place: String, buy_place: String) -> FormulaMarks`
  - `pub struct StripSelect { pub value: Signal<String>, pub options: Vec<(&'static str, String)>, pub on_change: Callback<String>, pub aria: String }`
  - `pub struct StripTerm { pub role: TermRole, pub label: Signal<String>, pub place: Option<Signal<String>>, pub select: Option<StripSelect>, pub place_select: Option<StripSelect>, pub degraded: Signal<bool> }`
  - `pub enum StripLayout { Inline, Stacked }`
  - `#[component] pub fn FormulaStrip(terms: Vec<StripTerm>, layout: StripLayout) -> impl IntoView`
  - `ToolCalculation::new(title, formula: impl Into<Signal<String>>, details)` with `formula: Signal<String>`

- [ ] **Step 1: Write the failing tests** (`formula.rs` tests)

```rust
    #[test]
    fn roi_is_clamped_at_display_ceiling_when_asked() {
        let mut f = recipe_default();
        f.roi = RoiMath::ClampedF64;
        let (line, _) = profit_line(999_999, 261, &f);
        assert_eq!(line.roi, 100_000);
        let (line, _) = profit_line(12_560, 11_300, &f);
        assert_eq!(line.roi, 5);
    }
```

and in `strip.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_terms_render_static_chips_and_select_terms_render_selects() {
        // `TermBadge` builds an I18nContext (spawns an Effect) and `<Gil>`
        // reads it: stand up the executor and the context, as
        // components/list/filter_row.rs's tests do.
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let terms = vec![
                StripTerm::fixed(TermRole::Result, Signal::derive(|| "Profit / unit".to_string())),
                StripTerm {
                    role: TermRole::Revenue,
                    label: Signal::derive(|| "Cheapest listing".to_string()),
                    place: Some(Signal::derive(|| "Gilgamesh".to_string())),
                    select: Some(StripSelect {
                        value: Signal::derive(|| "listing-min".to_string()),
                        options: vec![("listing-min", "Cheapest listing".into()), ("sale-median", "Sale median (7d)".into())],
                        on_change: Callback::new(|_| {}),
                        aria: "Change revenue signal".into(),
                    }),
                    place_select: None,
                    degraded: Signal::derive(|| false),
                },
            ];
            let html = view! { <FormulaStrip terms=terms layout=StripLayout::Inline /> }.to_html();
            assert_eq!(html.matches("<select").count(), 1, "{html}");
            assert!(html.contains("Profit / unit"), "{html}");
            assert!(html.contains("Gilgamesh"), "{html}");
            assert!(html.contains("aria-label=\"Change revenue signal\""), "{html}");
        });
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p ultros-app --lib analyzer_kit`
Expected: FAIL to compile.

- [ ] **Step 3: Implement the formula additions**

In `formula.rs`:

```rust
pub enum RoiMath {
    UnclampedF64,
    /// `analysis::return_on_investment`: f64 ratio, clamped at ±100,000
    /// and truncated to i32.
    ClampedF64,
}
```

and in `profit_line`:

```rust
    let roi = match f.roi {
        RoiMath::UnclampedF64 => { /* unchanged */ }
        RoiMath::ClampedF64 => crate::analysis::return_on_investment(profit, cost_per_unit),
    };
```

```rust
/// What the header marks and the readout need to know about the
/// selected formula, with places already resolved to names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormulaMarks {
    pub revenue: PriceSignal,
    pub cost: PriceSignal,
    pub sell_place: String,
    pub buy_place: String,
}

impl ProfitFormula {
    pub fn marks(&self, sell_place: String, buy_place: String) -> FormulaMarks {
        FormulaMarks { revenue: self.revenue_signal(), cost: self.cost_signal(), sell_place, buy_place }
    }
}
```

- [ ] **Step 4: Implement `strip.rs`**

```rust
//! The formula ledger as a row of chips: `[=] Profit / unit  [+] revenue
//! · place  [−] 5% tax  [−] cost · place`. A term is fixed (static chip)
//! or selectable (a native `<select>` inside the chip writing one URL
//! param). Inline for the row under "Sell on"; Stacked for popovers.

use leptos::prelude::*;

use crate::components::term_badge::{TermBadge, TermRole};
use crate::i18n::*;

pub struct StripSelect {
    pub value: Signal<String>,
    pub options: Vec<(&'static str, String)>,
    pub on_change: Callback<String>,
    pub aria: String,
}

pub struct StripTerm {
    pub role: TermRole,
    pub label: Signal<String>,
    /// "· Gilgamesh" / "· Aether".
    pub place: Option<Signal<String>>,
    pub select: Option<StripSelect>,
    /// A second select for the place (Buy from).
    pub place_select: Option<StripSelect>,
    /// Show the amber dot: the numbers fell back to the listing.
    pub degraded: Signal<bool>,
}

impl StripTerm {
    pub fn fixed(role: TermRole, label: Signal<String>) -> Self {
        Self { role, label, place: None, select: None, place_select: None, degraded: Signal::derive(|| false) }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum StripLayout {
    Inline,
    Stacked,
}

fn select_view(s: StripSelect) -> AnyView {
    let StripSelect { value, options, on_change, aria } = s;
    view! {
        <select
            class="filter-chip-value"
            aria-label=aria
            prop:value=move || value.get()
            on:change=move |ev| on_change.run(event_target_value(&ev))
        >
            {options.into_iter().map(|(val, lab)| view! {
                <option value=val selected=move || value.get() == val>{lab}</option>
            }).collect_view()}
        </select>
    }
    .into_any()
}

#[component]
pub fn FormulaStrip(terms: Vec<StripTerm>, layout: StripLayout) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let container = match layout {
        StripLayout::Inline => "flex flex-wrap items-center gap-2",
        StripLayout::Stacked => "flex flex-col items-stretch gap-1.5",
    };
    view! {
        <div class=container>
            {terms.into_iter().map(|term| {
                let chip_class = if term.select.is_some() { "filter-chip" } else { "filter-chip bg-transparent" };
                let degraded = term.degraded;
                view! {
                    <span class=chip_class>
                        <TermBadge role=term.role />
                        {match term.select {
                            Some(s) => select_view(s),
                            None => view! { <span>{move || term.label.get()}</span> }.into_any(),
                        }}
                        {term.place.map(|p| view! { <span class="filter-chip-label">"· " {move || p.get()}</span> })}
                        {term.place_select.map(select_view)}
                        <span
                            class=move || if degraded.get() { "inline-block w-1.5 h-1.5 rounded-full bg-amber-300" } else { "hidden" }
                            role="img"
                            title=move || degraded.get().then(|| t_string!(i18n, formula_degraded_listing_fallback).to_string())
                            aria-label=move || degraded.get().then(|| t_string!(i18n, formula_degraded_listing_fallback).to_string())
                        ></span>
                    </span>
                }
            }).collect_view()}
        </div>
    }
}
```

Register `pub mod strip;` in `mod.rs`.

- [ ] **Step 5: Make `ToolCalculation.formula` reactive**

In `tool_help.rs`:

```rust
#[derive(Clone)]
pub struct ToolCalculation {
    title: String,
    formula: Signal<String>,
    details: String,
}

impl ToolCalculation {
    pub fn new(title: impl Into<String>, formula: impl Into<Signal<String>>, details: impl Into<String>) -> Self {
        Self { title: title.into(), formula: formula.into(), details: details.into() }
    }
}
```

and leave the render as it is — `<code …>{calculation.formula}</code>` — a `Signal<String>` renders reactively on its own. The six static callers pass `String`s; `reactive_graph` 0.2.14 has `impl<T> From<T> for Signal<T>`, so they compile unchanged. If a caller passes `Oco` or `&str`, wrap it in `.to_string()`.

- [ ] **Step 6: Run tests, commit**

Run: `cargo test -p ultros-app --lib -- analyzer_kit tool_help && cargo fmt --all`

```bash
git add ultros-frontend/ultros-app/src/analyzer_kit ultros-frontend/ultros-app/src/components/tool_help.rs
git commit -m "feat(analyzer-kit): FormulaStrip, FormulaMarks, live ToolCalculation formula, clamped ROI policy"
```

---

### Task 4: Marks in the column table and the grid

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/columns.rs` (add fields)
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/grid.rs` (add `marks` prop)

**Interfaces:**
- `ToolColumnMeta` gains `pub side: Option<TermRole>`, `pub formula_header_class: &'static str`, `pub formula_cell_class: &'static str` (the widened two-line classes used when marks apply; `""` for columns that are never marked)
- `AnalyzerGrid` gains `#[prop(optional, into)] marks: Option<Signal<Option<MarkLabels>>>` where `pub struct MarkLabels { pub labels: HashMap<TermRole, String> }` gives the sub-label per role (`"listing · Aether"`, `"per unit · after 5% tax"`); `None` ⇒ Phase B rendering

- [ ] **Step 1: Write the failing test** (`grid.rs` tests; extend the existing fixture: give column B `side: Some(TermRole::Revenue)` and `formula_header_class: "w-40 px-3 py-2 leading-tight"`, `formula_cell_class: "w-40"`; all others `side: None` and empty formula classes)

```rust
    #[test]
    fn marks_switch_the_formula_columns_to_the_wide_two_line_variant() {
        // `TermBadge` builds an I18nContext (spawns an Effect) and `<Gil>`
        // reads it: stand up the executor and the context, as
        // components/list/filter_row.rs's tests do.
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let labels = MarkLabels { labels: [(TermRole::Revenue, "listing · Gilgamesh".to_string())].into_iter().collect() };
            let html = view! {
                <AnalyzerGrid
                    columns=&COLS
                    rows=Signal::derive(|| vec![(0usize, Row(7))])
                    visible_cols=Signal::derive(HashSet::new)
                    sort_mode=Signal::derive(|| None::<Col>)
                    sort_dir=Signal::derive(|| None::<SortDir>)
                    ctx=Signal::derive(|| CellCtx { now_unix: 0 })
                    custom=Arc::new(|_: &Row, _: &'static str| view! { <div role="cell"></div> }.into_any())
                    layout=GridLayout { viewport_height: 720.0, row_height: 60.0, header_height: 64.0, overscan: 8 }
                    header_class="thead"
                    row_class=stripe
                    marks=Signal::derive(move || Some(labels.clone()))
                />
            }
            .to_html();
            assert!(html.contains("listing · Gilgamesh"), "{html}");
            assert!(html.contains("w-40 px-3 py-2 leading-tight"), "{html}");
            assert!(html.contains("shadow-[inset_0_-2px_0_var(--brand-ring)]"), "{html}");
        });
    }
```

- [ ] **Step 2: Implement**

In `columns.rs` add the three fields to `ToolColumnMeta` (and to every table literal in the tests and in `recipe_analyzer.rs`; for the recipe: Profit/Cost/Price get `side: Some(TermRole::Result | Cost | Revenue)`, `formula_header_class: "w-40 shrink-0 px-3 py-2 leading-tight"`, `formula_cell_class: "px-3 py-2 w-40 shrink-0 text-right"`; all others `side: None`, `""`, `""`).

In `grid.rs`:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkLabels {
    pub labels: HashMap<TermRole, String>,
}
```

`header_cell` takes `marks: Option<Signal<Option<MarkLabels>>>` and, when `col.side` is `Some(role)` and the current marks contain that role, renders `SortableHeaderCell` with `class=col.formula_header_class`, `badge=role`, `sub_label=Signal::derive(move || marks-label-for-role)`, `emphasized=Signal::derive(|| true)`; otherwise the Phase B call. Wrap the always-on header cells in `move ||` so a marks change re-renders them. The row closure picks `col.formula_cell_class` instead of `col.cell_class` for marked columns on the `render_cell` path (Price); custom cells (Profit and Cost) pick their class themselves, see Task 5 Step 4.

- [ ] **Step 3: Run tests, commit**

Run: `cargo test -p ultros-app --lib -- analyzer_kit recipe_analyzer && cargo fmt --all`

```bash
git add ultros-frontend/ultros-app/src/analyzer_kit ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "feat(analyzer-kit): formula marks on column tables and the grid"
```

---

### Task 5: The recipe analyzer under the flag

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs` (page: strip row, ToolHeader calculation, Market popover; table: marks, readout, degraded signal, dead keys)
- Modify: all 7 locales
- Modify: `integration/runner.cjs:56-85`
- Modify: `ultros-frontend/ultros-app/src/routes/changelog.rs` (top of `CHANGELOG`)

**Interfaces:**
- Consumes: `use_lab(LAB_ANALYZER_LEDGER)`, `FormulaStrip`, `StripTerm`, `StripSelect`, `StripLayout`, `FormulaMarks`, `sentence`, `MarkLabels`, `RoiMath::ClampedF64`.

- [ ] **Step 1: Write the failing tests** (`recipe_analyzer.rs` tests)

```rust
    #[test]
    fn formula_marks_labels_name_signal_and_place() {
        let f = ProfitFormula::recipe_from_query(Some(PriceSignal::SaleMedian), None, None);
        let m = f.marks("Gilgamesh".into(), "Aether".into());
        let labels = mark_labels(&m, "7d median", "listing", "per unit · after 5% tax");
        assert_eq!(labels.labels[&TermRole::Cost], "7d median · Aether");
        assert_eq!(labels.labels[&TermRole::Revenue], "listing · Gilgamesh");
        assert_eq!(labels.labels[&TermRole::Result], "per unit · after 5% tax");
    }
```

- [ ] **Step 2: Implement the pure helpers** (route-private; import `PriceSignal` and `FormulaMarks` from `crate::analyzer_kit::formula` and `MarkLabels` from `crate::analyzer_kit::grid`)

```rust
fn mark_labels(m: &FormulaMarks, cost_short: &str, revenue_short: &str, profit_sub: &str) -> MarkLabels {
    MarkLabels {
        labels: [
            (TermRole::Result, profit_sub.to_string()),
            (TermRole::Revenue, format!("{revenue_short} · {}", m.sell_place)),
            (TermRole::Cost, format!("{cost_short} · {}", m.buy_place)),
        ]
        .into_iter()
        .collect(),
    }
}

fn short_signal(i18n: I18nContext<Locale, I18nKeys>, s: PriceSignal) -> String {
    match s {
        PriceSignal::ListingMin => t_string!(i18n, signal_short_listing_min).to_string(),
        PriceSignal::SaleMin => t_string!(i18n, signal_short_sale_min).to_string(),
        PriceSignal::SaleMedian => t_string!(i18n, signal_short_sale_median).to_string(),
        PriceSignal::SaleAvg => t_string!(i18n, signal_short_sale_avg).to_string(),
    }
}
```

The readout is the i18n template `recipe_analyzer_profit_readout` interpolated in the cell — `t_string!(i18n, recipe_analyzer_profit_readout, price = data.market_price.separate_with_commas(), tax = data.tax.separate_with_commas(), cost = data.cost.separate_with_commas(), profit = data.profit.separate_with_commas()).to_string()` (`thousands::Separable` is what `Gil` uses; reuse its import). No English lives in Rust, and there is no readout helper to test: the arithmetic is `profit_line`'s, already pinned in Phase A.

- [ ] **Step 3: Page-level wiring** (in `RecipeAnalyzer`)

```rust
    let ledger = use_lab(LAB_ANALYZER_LEDGER);
    // The page already declares `buy_scope` and `cost_basis` with their
    // setters discarded (`let (buy_scope, _) = …`, `let (cost_basis, _) = …`):
    // keep those lines but bind the setters as `set_buy_scope` and
    // `set_cost_basis`. Only the revenue signal is new at page level.
    let (revenue_metric, set_revenue_metric) = filter_query_signal::<RevenueMetric>(FILTER_REVENUE);
    let stats_degraded = RwSignal::new((false, false)); // (buy, sell) — written by an Effect inside the table
    let formula = Memo::new(move |_| ProfitFormula::recipe_from_query(cost_basis(), revenue_metric(), buy_scope()));
    let sell_place = Memo::new(move |_| selected_world.get().map(|w| w.name).unwrap_or_else(|| "…".to_string()));
    let buy_place = Memo::new(move |_| buy_scope_name.get());

    let strip_terms = move || {
        vec![
            StripTerm::fixed(TermRole::Result, Signal::derive(move || t_string!(i18n, formula_term_profit_per_unit).to_string())),
            StripTerm {
                role: TermRole::Revenue,
                label: Signal::derive(move || String::new()),
                place: Some(sell_place.into()),
                select: Some(StripSelect {
                    value: Signal::derive(move || revenue_metric().unwrap_or_default().to_string()),
                    options: cost_basis_options(i18n),
                    on_change: Callback::new(move |v: String| {
                        let parsed = v.parse::<RevenueMetric>().ok();
                        set_revenue_metric(parsed.filter(|m| *m != RevenueMetric::default()));
                    }),
                    aria: t_string!(i18n, formula_change_revenue_aria).to_string(),
                }),
                place_select: None,
                degraded: Signal::derive(move || stats_degraded.get().1),
            },
            StripTerm::fixed(TermRole::Tax, Signal::derive(move || t_string!(i18n, formula_term_tax).to_string())),
            StripTerm {
                role: TermRole::Cost,
                label: Signal::derive(move || String::new()),
                place: None,
                select: Some(StripSelect {
                    value: Signal::derive(move || cost_basis().unwrap_or_default().to_string()),
                    options: cost_basis_options(i18n),
                    on_change: Callback::new(move |v: String| {
                        let parsed = v.parse::<CostBasis>().ok();
                        set_cost_basis(parsed.filter(|b| *b != CostBasis::default()));
                    }),
                    aria: t_string!(i18n, formula_change_cost_aria).to_string(),
                }),
                place_select: Some(StripSelect {
                    value: Signal::derive(move || buy_scope().unwrap_or_default().to_string()),
                    options: buy_scope_options(i18n),
                    on_change: Callback::new(move |v: String| {
                        let parsed = v.parse::<BuyScope>().ok();
                        set_buy_scope(parsed.filter(|s| *s != BuyScope::default()));
                    }),
                    aria: t_string!(i18n, formula_change_scope_aria).to_string(),
                }),
                degraded: Signal::derive(move || stats_degraded.get().0),
            },
        ]
    };
```

Render the strip row directly under the "Sell on" row (outside Suspense):

```rust
                <Show when=move || ledger.get()>
                    <div class="hidden md:flex flex-wrap items-center gap-2">
                        <FormulaStrip terms=strip_terms() layout=StripLayout::Inline />
                    </div>
                </Show>
```

Live calculation: replace the `calculation=ToolCalculation::new(…)` prop with

```rust
                    calculation=ToolCalculation::new(
                        t_string!(i18n, recipe_analyzer_calc_title).to_string(),
                        Signal::derive(move || {
                            if ledger.get() {
                                // The EFFECTIVE formula: a failed stats body
                                // downgrades the signal, and the sentence must
                                // never name a signal the numbers ignore.
                                let (buy_failed, sell_failed) = stats_degraded.get();
                                let f = formula.get().effective(!buy_failed, !sell_failed);
                                let label_of = |s: PriceSignal| {
                                    cost_basis_options(i18n)
                                        .into_iter()
                                        .find(|(t, _)| *t == s.to_string())
                                        .map(|(_, l)| l)
                                        .unwrap_or_default()
                                };
                                // The connectives are translated: this is a
                                // template, never a `format!` in Rust.
                                t_string!(
                                    i18n,
                                    recipe_analyzer_calc_formula_live,
                                    revenue = label_of(f.revenue_signal()),
                                    sell = sell_place.get(),
                                    tax = t_string!(i18n, formula_term_tax).to_string(),
                                    cost = label_of(f.cost_signal()),
                                    buy = buy_place.get()
                                )
                                .to_string()
                            } else {
                                t_string!(i18n, recipe_analyzer_calc_formula).to_string()
                            }
                        }),
                        t_string!(i18n, recipe_analyzer_calc_details).to_string(),
                    )
```

Pass `ledger`, `stats_degraded`, the two place names and the strip terms into the table as props (the table builds the header marks from its own effective formula, so a header never names a signal the numbers fell back from):

```rust
                                        ledger=ledger
                                        stats_degraded=stats_degraded
                                        sell_place=sell_place
                                        buy_place=buy_place
                                        strip_terms=Callback::new(move |()| strip_terms())
```

- [ ] **Step 4: Table-level wiring** (in `RecipeAnalyzerTable`)

- New props: `ledger: Signal<bool>`, `stats_degraded: RwSignal<(bool, bool)>`, `sell_place: Signal<String>`, `buy_place: Signal<String>`, `strip_terms: Callback<(), Vec<StripTerm>>`, `buy_stats_error: bool`, `sell_stats_error: bool` (replacing `sale_stats_error`).
- Write the degraded flags once the table mounts (this is where the resource outcomes are known): `Effect::new(move |_| stats_degraded.set((buy_stats_error, sell_stats_error)));` — today's single `sale_stats_error` prop becomes `buy_stats_error` and `sell_stats_error`; the page passes `buy_stats_error=buy_stats_error sell_stats_error=history.stats_failed` (Phase A's join computes both), and the amber banner shows on either.
- The formula memo from Phase A gets the clamp under the flag: after `.effective(...)`, `if ledger.get() { f.roi = RoiMath::ClampedF64; }`.
- The `MarketMenu` popover body: keep the three `PricingSelect`s when `ledger` is off; when on, render `<FormulaStrip terms=… layout=StripLayout::Stacked />` followed by the four help lines (`price_basis_listing_min_help` … `price_basis_sale_avg_help`, muted `text-xs`), and widen the popover class to `w-[min(92vw,20rem)]`. `MarketMenu` is rendered inside the table's ControlBar `actions` closure, so the terms travel as a table prop: `RecipeAnalyzerTable` gains `strip_terms: Callback<(), Vec<StripTerm>>` (the page passes `strip_terms=Callback::new(move |()| strip_terms())`; the closure captures only `Copy` signals, so it is `Send + Sync`), and the signature becomes `fn MarketMenu(terms: Callback<(), Vec<StripTerm>>, ledger: Signal<bool>)`, rendering `<FormulaStrip terms=terms.run(()) layout=StripLayout::Stacked />` when `ledger.get()`.
- Profit cell: switch `SPEC_PROFIT`'s extractor to `cell_custom` and render it in `custom` under `ColumnKind::Profit`: `<div role="cell" class=cell_class title=move || ledger.get().then(|| t_string!(i18n, recipe_analyzer_profit_readout, price = …, tax = …, cost = …, profit = …).to_string())><Gil amount=data.profit /></div>`. The `title` is an `Option<String>`, so the flag-off cell carries no attribute at all (an empty `String` would render `title=""`). `cell_class` is `col.formula_cell_class` when `marks.get().is_some()` and today's `px-4 py-2 w-32 shrink-0 text-right` otherwise, looked up in `RECIPE_COLUMNS` by kind; the Cost cell does the same — the grid's `formula_cell_class` switch only reaches `render_cell` cells (Price), so the `custom` closure captures `marks` and widens its own two.
- The marks are built in the table from Phase A's `formula` memo, which is already the effective formula: `let marks = Signal::derive(move || ledger.get().then(|| { let f = formula.get(); let m = f.marks(sell_place.get(), buy_place.get()); mark_labels(&m, &short_signal(i18n, m.cost), &short_signal(i18n, m.revenue), &t_string!(i18n, recipe_analyzer_profit_sub).to_string()) }));` and `AnalyzerGrid` gets `marks=marks`.
- Wire the five dead keys over the hardcoded English (check each key's current English value and `{{…}}` placeholder names in `en.json` first and keep them): the item sub-line becomes `{t_string!(i18n, recipe_analyzer_item_level_label, level = data.required_level, ilvl = item_level)} " " {job_abbrev}`; the subcraft tooltip uses `recipe_analyzer_subcraft_header` for its first line and `recipe_analyzer_subcraft_row` per sub-craft; the `" sub"` suffix becomes `{count} " " {t!(i18n, recipe_analyzer_sub_suffix)}` (the key has no leading space); `"{:.1} / day"` becomes `t_string!(i18n, recipe_analyzer_sales_per_day, sales = format!("{:.1}", data.daily_sales))`; and the sales tooltip becomes `t_string!(i18n, recipe_analyzer_sales_tooltip, count = data.total_sales, days = format!("{:.1}", data.total_sales as f32 / data.daily_sales.max(0.001))).to_string()` with the new key `recipe_analyzer_sales_tooltip`.

- [ ] **Step 5: i18n keys** (all 7 locales; English below)

```
formula_term_profit_per_unit        "Profit / unit"
formula_term_tax                    "5% tax"
formula_change_revenue_aria         "Change revenue signal"
formula_change_cost_aria            "Change cost signal"
formula_change_scope_aria           "Change where ingredients are bought"
signal_short_listing_min            "listing"
signal_short_sale_min               "7d min"
signal_short_sale_median            "7d median"
signal_short_sale_avg               "7d avg"
recipe_analyzer_profit_sub          "per unit · after 5% tax"
recipe_analyzer_profit_readout      "{{price}} (price) − {{tax}} (tax) − {{cost}} (cost / unit) = {{profit}}"
recipe_analyzer_calc_formula_live   "profit / unit = {{revenue}} on {{sell}} − {{tax}} − {{cost}} across {{buy}}"
formula_degraded_listing_fallback   "Sale history unavailable — using cheapest listing"
price_basis_listing_min_help        "The cheapest listing up right now. Fast to act on, easy to fake with one odd listing."
price_basis_sale_median_help        "The middle price of the last 7 days of sales. The most realistic guess for a normal market."
price_basis_sale_min_help           "The lowest price anything sold for in the last 7 days. Cautious."
price_basis_sale_avg_help           "The average of the last 7 days of sales. Pulled up by a few expensive sales."
recipe_analyzer_sales_tooltip       "Based on {{count}} sales over {{days}} days"
```

Reword `recipe_analyzer_calc_formula` is NOT needed: the live sentence replaces it when the flag is on and the old string stays for the off state.

- [ ] **Step 6: e2e routes and changelog**

In `integration/runner.cjs` add to `ROUTE_ASSERTS`:

```js
  "/recipe-analyzer?world=Gilgamesh": { titleIncludes: "Recipe Analyzer" },
  // With the lab on, the Profit header carries the "per unit · after 5% tax"
  // sub-label at every width. The strip row itself is md+ only, and the
  // mobile pass reads innerText, which drops display:none content.
  "/recipe-analyzer?world=Gilgamesh&labs=analyzer-ledger": {
    titleIncludes: "Recipe Analyzer",
    bodyIncludesAny: ["after 5% tax"],
  },
```

In `changelog.rs`, prepend to `CHANGELOG`:

```rust
    ChangelogEntry {
        date: "2026-09-XX",
        title: "Recipe Analyzer: try the profit formula as a control (Labs)",
        blurb: "Turn on \"Recipe Analyzer: profit formula strip\" under Settings › Labs and the formula behind every row becomes a control above the table: pick the revenue signal and the cost signal, and the columns that feed Profit are marked so you can see exactly what the number is made of. Hover a profit for the row's arithmetic. While it's on, absurd ROIs from one fake listing are capped at 100,000%.",
        link: Some("/settings"),
    },
```

- [ ] Set `date` to the ISO date of the day this task is committed (keep newest-first).

- [ ] **Step 7: Run everything, manual checks, PR**

Run: `cargo test -p ultros-app --lib && cargo fmt --all && ./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log`
Expected: green and `REAL_EXIT=0`.

Manual: with the flag off, re-run the Phase B parity checklist (identical). With the flag on (`?labs=analyzer-ledger`), at 375 / 768 / 1024 / 1280 in `en`, `fr`, `de`: the strip wraps under "Sell on" without widening the page; the Market popover shows the stacked strip and the four help lines at 92vw; the three marked headers show badge, sub-label and hairline; the Profit cell hover shows the readout; Terminus Putty reads 100,000%; the info panel sentence follows the selects; the amber dot appears when `/api/v1/sale_stats` is blocked in devtools. Then `./scripts/run_e2e.sh` for the two new routes.

```bash
git add -A ultros-frontend/ultros-app/src ultros-frontend/ultros-app/locales integration/runner.cjs
git commit -m "feat(recipe-analyzer): profit formula strip, marked headers and live info panel behind the analyzer-ledger lab"
git push -u origin HEAD
gh pr create --base main --title "Analyzer kit phase C: formula strip and marked headers (Labs)" --body "Part of #1233. Behind Settings › Labs › \"Recipe Analyzer: profit formula strip\" (or ?labs=analyzer-ledger). Flag off = pixel-identical to main. See docs/superpowers/plans/2026-09-01-analyzer-kit-phase-c-ledger-ui.md.

- Labs toggle (cookie + ?labs= override) with a Settings section
- FormulaStrip (inline under Sell on, stacked in the Market popover) driving the existing revenue / cost-basis / buy-scope params
- Profit / Cost per unit / Price headers marked with operator badges, signal·place sub-labels, brand tint and bottom hairline; per-row arithmetic readout; live info-panel sentence; amber degraded dot
- ROI clamped at ±100,000 while the lab is on
- statistic definitions restored (dropped by #1214); five dead recipe keys wired over hardcoded English
- e2e: /recipe-analyzer with and without the lab

Tests green locally, ./check_ci.sh clean; manual pass at 375/768/1024/1280 in en/fr/de recorded above."
```

---

## Self-review

**Spec coverage (kit spec Phase C + section 11):** Labs cookie, `?labs=`, Settings section, removal-phase comment on each `LABS` entry → Task 1; `TermBadge` + `SortableHeaderCell` props → Task 2 (`trailing` deferred to Phase D, its consumer); `FormulaStrip` Inline/Stacked, `FormulaMarks`, reactive `ToolCalculation`, `RoiMath::ClampedF64` → Task 3; the live sentence as the translated `recipe_analyzer_calc_formula_live` template over the effective formula → Task 5; marks on the table and grid → Task 4; strip row under Sell on, popover body, header marks, readout, degraded dot via Effect-written signal, help lines under the stacked strip, dead keys wired, e2e route, changelog → Task 5. The item-link change is a decision point and is not in this plan.

**Placeholder scan:** the changelog `date` is a checkbox in Task 5 Step 6; nothing else.

**Type consistency:** `StripTerm`/`StripSelect` fields used in Task 5 match Task 3; `MarkLabels { labels: HashMap<TermRole, String> }` (so `TermRole: Hash`) in Tasks 4 and 5; `ProfitFormula::marks` returns `FormulaMarks` consumed by `mark_labels` inside the table; `strip_terms: Callback<(), Vec<StripTerm>>` and `sell_place`/`buy_place: Signal<String>` are the table props Task 5 Step 3 passes and Step 4 declares; `use_lab` returns `Signal<bool>` passed as the `ledger` prop.
