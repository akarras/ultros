# Flip Finder Column Filters Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add five filters to the Flip Finder so every data column is filterable: Quality (HQ/NQ), item-name search, min drift %, min confidence band, and min 30-day volume.

**Architecture:** All five slot into the existing chip-based filter registry in `analyzer.rs` (const id = URL key → `query_signal` → predicate in the `sorted_data` memo → chip in the sticky bar → row in the `+ Filter` menu). `FilterChip` gains a select variant and a start-in-editing flag. Predicates are extracted as plain functions (like the existing `passes_velocity_floor`) so they unit-test without a reactive runtime. The name filter is gated behind the codebase's Effect-driven `hydrated` flag because SSR renders 20 fallback rows with **English** item names while the client hydrates localized ones.

**Tech Stack:** Rust / Leptos 0.8 (edition 2024), leptos-i18n, leptos_router `query_signal`.

**Spec:** `docs/superpowers/specs/2026-07-30-flip-finder-column-filters-design.md`

## Global Constraints

- Every new user-facing string goes into **all seven** locale files (`en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`) with real translations — leptos-i18n will not compile with a key missing from any locale.
- Run `./check_ci.sh` (fmt-check + clippy `-D warnings`) before every commit. From Git Bash prepend Strawberry Perl: `export PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:$PATH"`. If clippy cannot run (submodule trouble), minimum bar is `cargo fmt --all -- --check` and say so in the PR.
- Reuse the main checkout's warm build cache: `export CARGO_TARGET_DIR=/c/Users/chw11/code/ultros/target` (Git Bash) before any cargo command.
- Worktree submodule init (if `xiv-gen/ffxiv-datamining/csv/en/Item.csv` is missing) must follow the `--reference` procedure in `CLAUDE.md` — **never** plain `--init --recursive`.
- Test command for this work: `cargo test -p ultros-app --lib` (ultros-app lib tests link fine on Windows/MSVC; only the `ultros` bin has the linker wall).
- No `#[allow]` to silence clippy. Do not read exit codes through a pipe (`cmd | tail` masks them).

---

### Task 1: FilterChip select variant + start-in-editing flag

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/filter_chip.rs`
- Test: same file, existing `mod tests`

**Interfaces:**
- Consumes: nothing new.
- Produces (used by Tasks 3–4):
  - new optional props on `FilterChip`: `options: Option<Vec<(&'static str, String)>>` (value → localized label; renders an inline `<select>` when editing and shows the current value's *label* when resting) and `start_editing: bool` (chip mounts already in edit state).
  - `pub fn option_label(options: Option<&[(&'static str, String)]>, raw: String) -> String` — resting-state display helper.

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `filter_chip.rs`:

```rust
    #[test]
    fn option_label_maps_value_to_its_label() {
        let opts = vec![("hq", "HQ".to_string()), ("nq", "NQ".to_string())];
        assert_eq!(option_label(Some(&opts), "nq".to_string()), "NQ");
    }

    #[test]
    fn option_label_falls_back_to_raw_value_when_unknown() {
        let opts = vec![("hq", "HQ".to_string())];
        // A stale URL value the options no longer contain still renders
        // something rather than a blank chip.
        assert_eq!(option_label(Some(&opts), "zz".to_string()), "zz");
    }

    #[test]
    fn option_label_passes_plain_values_through() {
        assert_eq!(option_label(None, "5000".to_string()), "5000");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
export CARGO_TARGET_DIR=/c/Users/chw11/code/ultros/target
cargo test -p ultros-app --lib filter_chip 2>&1 | tail -20
```
Expected: compile error — `option_label` not found.

- [ ] **Step 3: Implement**

Add the helper above the component:

```rust
/// Resting-state display for a chip value. Select-variant chips store a
/// machine token (`hq`, `medium`) in the URL; the chip shows the localized
/// label for it. Plain chips show the raw value unchanged.
pub fn option_label(options: Option<&[(&'static str, String)]>, raw: String) -> String {
    match options {
        Some(opts) => opts
            .iter()
            .find(|(v, _)| *v == raw)
            .map(|(_, l)| l.clone())
            .unwrap_or(raw),
        None => raw,
    }
}
```

Add the two props to the component signature (after `step`):

```rust
    /// (value, localized label) pairs. When set, the chip edits via an
    /// inline `<select>` instead of a text input, and the resting state
    /// shows the current value's label rather than the raw token.
    /// `into` so call sites pass a bare `vec![...]` (std's
    /// `From<T> for Option<T>` provides the conversion).
    #[prop(optional, into)]
    options: Option<Vec<(&'static str, String)>>,
    /// Mount already in edit state. Used by chips whose seed value is
    /// empty (name search): a resting chip with no value is just a label.
    #[prop(optional)]
    start_editing: bool,
```

In the body:
1. `let editing = RwSignal::new(false);` → `let editing = RwSignal::new(start_editing);`
2. After `let input_ref = …` add:
   ```rust
   let select_ref = NodeRef::<leptos::html::Select>::new();
   // StoredValue: both `Show` branches are `Fn` closures and both need the
   // options; storing once avoids a clone per render.
   let options = StoredValue::new(options);
   // Same treatment for the input attributes: the input now sits inside an
   // extra `move ||` closure (the select/input dispatch), and a `move`
   // closure inside an `Fn` closure cannot take the raw `Option<String>`s
   // by value. StoredValue is Copy, so both layers can capture it freely.
   let min = StoredValue::new(min);
   let max = StoredValue::new(max);
   let step = StoredValue::new(step);
   ```
3. Extend the focus effect to also try the select:
   ```rust
   Effect::new(move |_| {
       if editing.get() {
           if let Some(el) = input_ref.get() {
               let _ = el.focus();
           } else if let Some(el) = select_ref.get() {
               let _ = el.focus();
           }
       }
   });
   ```
4. In BOTH resting branches (readonly `filter-chip-static` span and the `filter-chip-value` button), replace
   `{move || value.get().unwrap_or_default()}` with
   ```rust
   {move || options.with_value(|o| option_label(o.as_deref(), value.get().unwrap_or_default()))}
   ```
5. Replace the editing branch's bare `<input …/>` with a dispatch on the variant (keep the existing `<input>` attributes verbatim in the `None` arm):
   ```rust
   {move || {
       match options.with_value(|opts| opts.clone()) {
           Some(opts) => Either::Left(view! {
               <select
                   node_ref=select_ref
                   class="input input-sm"
                   on:change=move |ev| {
                       on_commit.run(committed_value(&event_target_value(&ev)));
                       editing.set(false);
                   }
                   on:keydown=move |ev| {
                       if ev.key() == "Escape" {
                           editing.set(false);
                       }
                   }
                   prop:value=move || value.get().unwrap_or_default()
               >
                   {opts
                       .into_iter()
                       .map(|(val, lab)| view! { <option value=val>{lab}</option> })
                       .collect_view()}
               </select>
           }),
           None => Either::Right(view! {
               <input
                   node_ref=input_ref
                   class="input input-sm w-24"
                   type=if numeric { "number" } else { "text" }
                   min=min.get_value()
                   max=max.get_value()
                   step=step.get_value()
                   prop:value=move || value.get().unwrap_or_default()
                   on:blur=commit_from_blur
                   on:keydown=move |ev| {
                       if ev.key() == "Enter" {
                           commit_from(&event_target::<web_sys::HtmlInputElement>(&ev));
                       } else if ev.key() == "Escape" {
                           editing.set(false);
                       }
                   }
               />
           }),
       }
   }}
   ```
   Note: `min`/`max`/`step` are read via `StoredValue::get_value()` (set up in sub-step 2) because this inner closure re-runs reactively. `event_target_value` comes from `leptos::prelude::*`, already glob-imported.

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p ultros-app --lib filter_chip 2>&1 | tail -10
```
Expected: all `filter_chip` tests PASS (3 new + 3 existing).

- [ ] **Step 5: fmt + commit**

```bash
cargo fmt --all
git add ultros-frontend/ultros-app/src/components/filter_chip.rs
git commit -m "feat(filter-chip): select variant and start-in-editing flag"
```

---

### Task 2: Pure filter predicates + parse types

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs` (types near `SortMode` ~line 186; functions near `passes_velocity_floor` ~line 497; tests in the `mod tests` at ~line 2505)

**Interfaces:**
- Consumes: `DerivedConfidence` (`crate::analysis`, variants `High | Medium | Low`), `ConfidenceBand` (`ultros_api_types::trends`, variants `Unknown | High | Medium | Low | Unusable`, `Copy`) — both already imported in analyzer.rs.
- Produces (used by Tasks 3–4):
  - `enum QualityFilter { Hq, Nq }` with `FromStr` (`"hq"`/`"nq"`) + `Display`, derives `Debug, Clone, Copy, PartialEq, Eq`
  - `enum ConfidenceFloor { Low, Medium, High }` with `FromStr` (`"low"`/`"medium"`/`"high"`) + `Display`, same derives
  - `fn passes_quality(filter: QualityFilter, hq: bool) -> bool`
  - `fn matches_item_name(query: &str, item_name: &str) -> bool`
  - `fn passes_drift_floor(min: f32, drift: Option<f32>) -> bool`
  - `fn passes_confidence_floor(floor: ConfidenceFloor, ch: Option<ConfidenceBand>, derived: DerivedConfidence) -> bool`
  - `fn passes_volume_floor(min: u32, ch_volume: Option<u32>) -> bool`

- [ ] **Step 1: Write the failing tests**

Append inside the existing `mod tests` in `analyzer.rs`:

```rust
    #[test]
    fn quality_filter_round_trips_its_url_tokens() {
        assert_eq!("hq".parse::<QualityFilter>(), Ok(QualityFilter::Hq));
        assert_eq!("nq".parse::<QualityFilter>(), Ok(QualityFilter::Nq));
        assert!("HQ".parse::<QualityFilter>().is_err());
        assert_eq!(QualityFilter::Hq.to_string(), "hq");
        assert_eq!(QualityFilter::Nq.to_string(), "nq");
    }

    #[test]
    fn quality_filter_selects_matching_rows_only() {
        assert!(passes_quality(QualityFilter::Hq, true));
        assert!(!passes_quality(QualityFilter::Hq, false));
        assert!(passes_quality(QualityFilter::Nq, false));
        assert!(!passes_quality(QualityFilter::Nq, true));
    }

    #[test]
    fn name_match_is_case_insensitive_substring() {
        assert!(matches_item_name("grade", "Grade 8 Tincture of Strength"));
        assert!(matches_item_name("TINCTURE", "Grade 8 Tincture of Strength"));
        assert!(!matches_item_name("potion", "Grade 8 Tincture of Strength"));
    }

    #[test]
    fn blank_or_whitespace_name_query_matches_everything() {
        // The chip seeds empty and the user may commit whitespace; neither
        // should silently empty the table.
        assert!(matches_item_name("", "Anything"));
        assert!(matches_item_name("   ", "Anything"));
    }

    #[test]
    fn drift_floor_keeps_rows_at_or_above_the_floor() {
        assert!(passes_drift_floor(-10.0, Some(-5.0)));
        assert!(passes_drift_floor(-10.0, Some(-10.0)));
        assert!(!passes_drift_floor(-10.0, Some(-25.0)));
    }

    #[test]
    fn drift_floor_drops_rows_with_uncomputable_drift() {
        // Universal-coverage metric: same unknown-fails rule as the
        // velocity floor (spec: Unknown-data semantics).
        assert!(!passes_drift_floor(-10.0, None));
    }

    #[test]
    fn confidence_floor_prefers_the_clickhouse_band() {
        // CH says Low; the derived band saying High must not override it.
        assert!(!passes_confidence_floor(
            ConfidenceFloor::Medium,
            Some(ConfidenceBand::Low),
            DerivedConfidence::High,
        ));
        assert!(passes_confidence_floor(
            ConfidenceFloor::Medium,
            Some(ConfidenceBand::High),
            DerivedConfidence::Low,
        ));
    }

    #[test]
    fn confidence_unknown_band_falls_back_to_derived() {
        // CH `Unknown` is "no deep-scan yet", not a verdict.
        assert!(passes_confidence_floor(
            ConfidenceFloor::Medium,
            Some(ConfidenceBand::Unknown),
            DerivedConfidence::Medium,
        ));
        assert!(passes_confidence_floor(
            ConfidenceFloor::High,
            None,
            DerivedConfidence::High,
        ));
        assert!(!passes_confidence_floor(
            ConfidenceFloor::High,
            None,
            DerivedConfidence::Medium,
        ));
    }

    #[test]
    fn confidence_unusable_fails_any_floor() {
        assert!(!passes_confidence_floor(
            ConfidenceFloor::Low,
            Some(ConfidenceBand::Unusable),
            DerivedConfidence::High,
        ));
    }

    #[test]
    fn confidence_floor_round_trips_its_url_tokens() {
        assert_eq!("medium".parse::<ConfidenceFloor>(), Ok(ConfidenceFloor::Medium));
        assert!("Medium".parse::<ConfidenceFloor>().is_err());
        assert_eq!(ConfidenceFloor::High.to_string(), "high");
    }

    #[test]
    fn volume_floor_keeps_rows_without_clickhouse_coverage() {
        // CH-only metric (~7% coverage, lazily enriched): unknown-fails
        // would empty the un-enriched table and deadlock the lazy fetch
        // (spec: Unknown-data semantics). Unknown rows pass.
        assert!(passes_volume_floor(10, None));
    }

    #[test]
    fn volume_floor_drops_rows_with_known_volume_below_it() {
        assert!(!passes_volume_floor(10, Some(3)));
        assert!(passes_volume_floor(10, Some(10)));
        assert!(passes_volume_floor(10, Some(250)));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p ultros-app --lib analyzer 2>&1 | tail -20
```
Expected: compile errors — `QualityFilter`, `passes_quality`, etc. not found.

- [ ] **Step 3: Implement**

Below the `SortDir` impls (~line 218), add:

```rust
/// `?quality=` — show only HQ or only NQ rows. Param absent = both.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum QualityFilter {
    Hq,
    Nq,
}

impl FromStr for QualityFilter {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "hq" => Ok(QualityFilter::Hq),
            "nq" => Ok(QualityFilter::Nq),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for QualityFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            QualityFilter::Hq => "hq",
            QualityFilter::Nq => "nq",
        })
    }
}

/// `?confidence=` — minimum confidence band a row must reach.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum ConfidenceFloor {
    Low,
    Medium,
    High,
}

impl FromStr for ConfidenceFloor {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "low" => Ok(ConfidenceFloor::Low),
            "medium" => Ok(ConfidenceFloor::Medium),
            "high" => Ok(ConfidenceFloor::High),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for ConfidenceFloor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ConfidenceFloor::Low => "low",
            ConfidenceFloor::Medium => "medium",
            ConfidenceFloor::High => "high",
        })
    }
}
```

Below `passes_velocity_floor` (~line 499), add:

```rust
fn passes_quality(filter: QualityFilter, hq: bool) -> bool {
    match filter {
        QualityFilter::Hq => hq,
        QualityFilter::Nq => !hq,
    }
}

/// Case-insensitive substring match for the `?name=` filter. A blank query
/// matches everything — the chip seeds empty so the user can type into it,
/// and that state must not blank the table.
fn matches_item_name(query: &str, item_name: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    item_name.to_lowercase().contains(&q)
}

/// Does a row clear the `?drift=` floor? Drift comes off the row's own
/// price buffer, so coverage is near-universal; a row with too few sales
/// to compute a drift fails an explicit floor — the velocity floor's rule.
fn passes_drift_floor(min: f32, drift: Option<f32>) -> bool {
    drift.map(|d| d >= min).unwrap_or(false)
}

/// Bands on one scale: Unusable=0 < Low=1 < Medium=2 < High=3. The CH
/// `Unknown` variant is "no deep scan yet", not a verdict, so it defers to
/// the derived band — the same preference the Confidence column renders.
fn confidence_rank(ch: Option<ConfidenceBand>, derived: DerivedConfidence) -> u8 {
    match ch {
        Some(ConfidenceBand::High) => 3,
        Some(ConfidenceBand::Medium) => 2,
        Some(ConfidenceBand::Low) => 1,
        Some(ConfidenceBand::Unusable) => 0,
        Some(ConfidenceBand::Unknown) | None => match derived {
            DerivedConfidence::High => 3,
            DerivedConfidence::Medium => 2,
            DerivedConfidence::Low => 1,
        },
    }
}

fn passes_confidence_floor(
    floor: ConfidenceFloor,
    ch: Option<ConfidenceBand>,
    derived: DerivedConfidence,
) -> bool {
    let floor_rank = match floor {
        ConfidenceFloor::Low => 1,
        ConfidenceFloor::Medium => 2,
        ConfidenceFloor::High => 3,
    };
    confidence_rank(ch, derived) >= floor_rank
}

/// Does a row clear the `?min-volume=` floor? 30-day volume is ClickHouse-
/// only (~7% item coverage) AND lazily enriched per visible window — if
/// unknown failed, the un-enriched initial table would filter to zero rows
/// and the visible-window fetch would never fire. So unknown rows pass,
/// and only a *known* volume below the floor drops a row (the suspicious
/// filter's rule).
fn passes_volume_floor(min: u32, ch_volume: Option<u32>) -> bool {
    ch_volume.map(|v| v >= min).unwrap_or(true)
}
```

**Expected interim state:** until Tasks 3–4 wire these into the component, the *lib* target will emit `dead_code` warnings for them (test-module usage does not count for the lib compilation), so `./check_ci.sh` would fail between Task 2 and Task 3. That is why this task's gate is fmt + tests only. Do NOT add `#[allow(dead_code)]` to quiet it — and do NOT push until Task 3's check_ci passes; only the branch tip needs a green clippy.

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p ultros-app --lib analyzer 2>&1 | tail -15
```
Expected: all analyzer tests PASS (12 new + existing).

- [ ] **Step 5: fmt + commit**

```bash
cargo fmt --all
git add ultros-frontend/ultros-app/src/routes/analyzer.rs
git commit -m "feat(analyzer): pure predicates for quality/name/drift/confidence/volume filters"
```

---

### Task 3: Wire Quality + Item-name filters (registry, chips, menu, i18n, hydration gate)

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs`
- Modify: `ultros-frontend/ultros-app/locales/en.json`, `fr.json`, `de.json`, `ja.json`, `cn.json`, `ko.json`, `tc.json`

**Interfaces:**
- Consumes: `QualityFilter`, `passes_quality`, `matches_item_name` (Task 2); `FilterChip` `options` + `start_editing` props (Task 1).
- Produces: URL params `?quality=hq|nq` and `?name=<substring>`; consts `FILTER_QUALITY = "quality"`, `FILTER_NAME = "name"`.

- [ ] **Step 1: Registry consts + ADDABLE_FILTERS**

After `const FILTER_SHOW_SUSPICIOUS: &str = "show-suspicious";` add:

```rust
const FILTER_QUALITY: &str = "quality";
const FILTER_NAME: &str = "name";
```

Append both to the END of `ADDABLE_FILTERS` (after `FILTER_SHOW_SUSPICIOUS`).

In `default_filter_value`, add arms before the catch-all and update its doc comment:

```rust
        FILTER_QUALITY => "hq",
        // Name search deliberately seeds empty: its chip mounts in edit
        // state (`start_editing`) so there is never an empty resting chip.
        FILTER_NAME => "",
```

- [ ] **Step 2: Signals + hydration gate**

Next to the other `query_signal` declarations in `AnalyzerTable` (after the `cols_param` line):

```rust
    let (quality_filter, set_quality_filter) = query_signal::<QualityFilter>("quality");
    let (name_filter, set_name_filter) = query_signal::<String>("name");
    // Keeps the name chip mounted (in edit state) between "picked from the
    // + Filter menu" and "first committed value" — an empty ?name= URL
    // param is not relied on to round-trip.
    let name_chip_pending = RwSignal::new(false);
    // SSR renders SSR_FALLBACK_ROWS rows with *English* item names; the
    // client hydrates localized ones. Localized-name matching therefore
    // must not run until after hydration or an active ?name= produces
    // different row sets and trips the tachys hydration panic. Same
    // Effect-driven gate as item_explorer.rs / job_set_card.rs.
    let hydrated = RwSignal::new(false);
    Effect::new(move |_| {
        hydrated.set(true);
    });
```

- [ ] **Step 3: Predicates in `sorted_data`**

Insert two `.filter` calls immediately after the existing `category_filter` filter closure (the one calling `item.item_search_category == cat_id`):

```rust
            .filter(move |data| {
                quality_filter()
                    .map(|q| passes_quality(q, data.inner.sale_summary.hq))
                    .unwrap_or(true)
            })
            .filter(move |data| {
                // Hydration gate — see the comment at the `hydrated` signal.
                if !hydrated.get() {
                    return true;
                }
                name_filter()
                    .map(|query| {
                        items
                            .get(&ItemId(data.inner.sale_summary.item_id))
                            .map(|item| matches_item_name(&query, &item.name))
                            .unwrap_or(false)
                    })
                    .unwrap_or(true)
            })
```

- [ ] **Step 4: active_filters, add_filter, filter_label, clear_all_filters**

In `active_filters` add (after the datacenter `push_if`):

```rust
        push_if(quality_filter().is_some(), FILTER_QUALITY);
        push_if(
            name_filter().is_some() || name_chip_pending.get(),
            FILTER_NAME,
        );
```

In `filter_label` add arms before `_ =>`:

```rust
            FILTER_QUALITY => t_string!(i18n, analyzer_filter_quality_label).to_string(),
            FILTER_NAME => t_string!(i18n, analyzer_filter_name_label).to_string(),
```

In `add_filter` add arms before `_ => {}`:

```rust
            FILTER_QUALITY => set_quality_filter(value.parse().ok()),
            FILTER_NAME => name_chip_pending.set(true),
```

In `clear_all_filters` add:

```rust
        set_quality_filter(None);
        set_name_filter(None);
        name_chip_pending.set(false);
```

- [ ] **Step 5: Chips**

In the chip row (`filter-chip-row` div), after the `show_suspicious_active` chip block, add:

```rust
                        {move || {
                            quality_filter()
                                .map(|_| {
                                    view! {
                                        <FilterChip
                                            label=t_string!(i18n, analyzer_quality_label).to_string()
                                            value=Signal::derive(move || {
                                                quality_filter().map(|q| q.to_string())
                                            })
                                            options=vec![
                                                ("hq", t_string!(i18n, analyzer_col_hq).to_string()),
                                                ("nq", t_string!(i18n, analyzer_quality_nq).to_string()),
                                            ]
                                            on_commit=Callback::new(move |v: Option<String>| {
                                                set_quality_filter(v.and_then(|s| s.parse().ok()));
                                            })
                                        />
                                    }
                                })
                        }}
                        {move || {
                            (name_filter().is_some() || name_chip_pending.get())
                                .then(|| {
                                    // Fresh from the menu (no committed value yet) the
                                    // chip mounts editing so the user can type at once.
                                    let start_editing = name_filter().is_none();
                                    view! {
                                        <FilterChip
                                            label=t_string!(i18n, analyzer_name_contains).to_string()
                                            value=Signal::derive(name_filter)
                                            start_editing=start_editing
                                            on_commit=Callback::new(move |v: Option<String>| {
                                                set_name_filter(v);
                                                name_chip_pending.set(false);
                                            })
                                        />
                                    }
                                })
                        }}
```

- [ ] **Step 6: i18n keys (ALL seven locales)**

Insert these keys alphabetically near the other `analyzer_*` keys in each file:

`en.json`:
```json
    "analyzer_filter_name_label": "Item name",
    "analyzer_filter_quality_label": "Quality (HQ/NQ)",
    "analyzer_name_contains": "Name contains",
    "analyzer_quality_label": "Quality",
    "analyzer_quality_nq": "NQ",
```
`fr.json`:
```json
    "analyzer_filter_name_label": "Nom de l'objet",
    "analyzer_filter_quality_label": "Qualité (HQ/NQ)",
    "analyzer_name_contains": "Nom contient",
    "analyzer_quality_label": "Qualité",
    "analyzer_quality_nq": "NQ",
```
`de.json`:
```json
    "analyzer_filter_name_label": "Gegenstandsname",
    "analyzer_filter_quality_label": "Qualität (HQ/NQ)",
    "analyzer_name_contains": "Name enthält",
    "analyzer_quality_label": "Qualität",
    "analyzer_quality_nq": "NQ",
```
`ja.json`:
```json
    "analyzer_filter_name_label": "アイテム名",
    "analyzer_filter_quality_label": "品質（HQ/NQ）",
    "analyzer_name_contains": "名前に含む",
    "analyzer_quality_label": "品質",
    "analyzer_quality_nq": "NQ",
```
`cn.json`:
```json
    "analyzer_filter_name_label": "物品名称",
    "analyzer_filter_quality_label": "品质（HQ/NQ）",
    "analyzer_name_contains": "名称包含",
    "analyzer_quality_label": "品质",
    "analyzer_quality_nq": "NQ",
```
`ko.json`:
```json
    "analyzer_filter_name_label": "아이템 이름",
    "analyzer_filter_quality_label": "품질(HQ/NQ)",
    "analyzer_name_contains": "이름 포함",
    "analyzer_quality_label": "품질",
    "analyzer_quality_nq": "NQ",
```
`tc.json`:
```json
    "analyzer_filter_name_label": "物品名稱",
    "analyzer_filter_quality_label": "品質（HQ/NQ）",
    "analyzer_name_contains": "名稱包含",
    "analyzer_quality_label": "品質",
    "analyzer_quality_nq": "NQ",
```

- [ ] **Step 7: Build + test**

```bash
cargo test -p ultros-app --lib analyzer 2>&1 | tail -10
```
Expected: PASS. (This also proves the i18n keys compile — leptos-i18n macro expansion fails the build on a missing key.)

- [ ] **Step 8: fmt-check + clippy + commit**

```bash
export PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:$PATH"
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```
Expected: `REAL_EXIT=0`. Then:

```bash
git add ultros-frontend/ultros-app/src/routes/analyzer.rs ultros-frontend/ultros-app/locales/
git commit -m "feat(analyzer): quality (HQ/NQ) and item-name filters"
```

---

### Task 4: Wire Drift + Confidence + Min-volume filters

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs`
- Modify: all seven files in `ultros-frontend/ultros-app/locales/`

**Interfaces:**
- Consumes: `ConfidenceFloor`, `passes_drift_floor`, `passes_confidence_floor`, `passes_volume_floor` (Task 2); `FilterChip` `options` prop (Task 1); `price_drift_pct`, `derived_confidence` (already imported); `enrichment` RwSignal + `normalize_velocity_floor` (existing).
- Produces: URL params `?drift=<f32>`, `?confidence=low|medium|high`, `?min-volume=<u32>`; consts `FILTER_DRIFT = "drift"`, `FILTER_CONFIDENCE = "confidence"`, `FILTER_MIN_VOLUME = "min-volume"`.

- [ ] **Step 1: Registry consts + ADDABLE_FILTERS + defaults**

After the Task 3 consts:

```rust
const FILTER_DRIFT: &str = "drift";
const FILTER_CONFIDENCE: &str = "confidence";
const FILTER_MIN_VOLUME: &str = "min-volume";
```

Append all three to the END of `ADDABLE_FILTERS`. Add default seeds:

```rust
        FILTER_DRIFT => "-10",
        FILTER_CONFIDENCE => "medium",
        FILTER_MIN_VOLUME => "10",
```

- [ ] **Step 2: Signals**

Next to the Task 3 signals:

```rust
    let (min_drift, set_min_drift) = query_signal::<f32>("drift");
    // Same NaN guard as ?vel= — "NaN".parse::<f32>() succeeds and would
    // silently empty the table (every comparison with NaN is false).
    let drift_floor = Memo::new(move |_| normalize_velocity_floor(min_drift()));
    let (min_confidence, set_min_confidence) = query_signal::<ConfidenceFloor>("confidence");
    let (min_volume, set_min_volume) = query_signal::<u32>("min-volume");
```

- [ ] **Step 3: Predicates in `sorted_data`**

Insert after the Task 3 name-filter closure:

```rust
            .filter(move |data| {
                drift_floor()
                    .map(|min| passes_drift_floor(min, price_drift_pct(&data.inner.prices)))
                    .unwrap_or(true)
            })
            .filter(move |data| {
                // CH band first, derived fallback — the same preference the
                // Confidence column renders, so the label shown is the label
                // filtered. Reading `enrichment` here follows the velocity
                // filter's pattern; the non-reactive `requested` dedupe is
                // what keeps recompute -> refetch from looping.
                min_confidence()
                    .map(|floor| {
                        let key = (data.inner.sale_summary.item_id, data.inner.sale_summary.hq);
                        let ch = enrichment
                            .with(|maps| maps.quality_for(&key).map(|q| q.confidence_band));
                        passes_confidence_floor(
                            floor,
                            ch,
                            derived_confidence(&data.inner.sale_summary),
                        )
                    })
                    .unwrap_or(true)
            })
            .filter(move |data| {
                min_volume()
                    .map(|min| {
                        let key = (data.inner.sale_summary.item_id, data.inner.sale_summary.hq);
                        let ch =
                            enrichment.with(|maps| maps.quality_for(&key).map(|q| q.sample_size));
                        passes_volume_floor(min, ch)
                    })
                    .unwrap_or(true)
            })
```

- [ ] **Step 4: active_filters, add_filter, filter_label, clear_all_filters**

`active_filters`:

```rust
        push_if(drift_floor().is_some(), FILTER_DRIFT);
        push_if(min_confidence().is_some(), FILTER_CONFIDENCE);
        push_if(min_volume().is_some(), FILTER_MIN_VOLUME);
```

`filter_label`:

```rust
            FILTER_DRIFT => t_string!(i18n, analyzer_filter_drift_min_label).to_string(),
            FILTER_CONFIDENCE => {
                t_string!(i18n, analyzer_filter_confidence_min_label).to_string()
            }
            FILTER_MIN_VOLUME => t_string!(i18n, analyzer_filter_volume_min_label).to_string(),
```

`add_filter`:

```rust
            FILTER_DRIFT => set_min_drift(value.parse().ok()),
            FILTER_CONFIDENCE => set_min_confidence(value.parse().ok()),
            FILTER_MIN_VOLUME => set_min_volume(value.parse().ok()),
```

`clear_all_filters`:

```rust
        set_min_drift(None);
        set_min_confidence(None);
        set_min_volume(None);
```

- [ ] **Step 5: Chips**

After the Task 3 name chip block:

```rust
                        {move || {
                            drift_floor()
                                .map(|_| {
                                    view! {
                                        <FilterChip
                                            label=t_string!(i18n, analyzer_drift_gte).to_string()
                                            value=Signal::derive(move || {
                                                drift_floor().map(format_velocity_floor)
                                            })
                                            numeric=true
                                            step="1"
                                            on_commit=Callback::new(move |v: Option<String>| {
                                                set_min_drift(
                                                    commit_numeric(drift_floor.get_untracked(), v),
                                                );
                                            })
                                        />
                                    }
                                })
                        }}
                        {move || {
                            min_confidence()
                                .map(|_| {
                                    view! {
                                        <FilterChip
                                            label=t_string!(i18n, analyzer_confidence_gte).to_string()
                                            value=Signal::derive(move || {
                                                min_confidence().map(|c| c.to_string())
                                            })
                                            options=vec![
                                                ("low", t_string!(i18n, analyzer_confidence_low).to_string()),
                                                ("medium", t_string!(i18n, analyzer_confidence_medium).to_string()),
                                                ("high", t_string!(i18n, analyzer_confidence_high).to_string()),
                                            ]
                                            on_commit=Callback::new(move |v: Option<String>| {
                                                set_min_confidence(v.and_then(|s| s.parse().ok()));
                                            })
                                        />
                                    }
                                })
                        }}
                        {move || {
                            min_volume()
                                .map(|_| {
                                    view! {
                                        <FilterChip
                                            label=t_string!(i18n, analyzer_volume_gte).to_string()
                                            value=Signal::derive(move || {
                                                min_volume().map(|v| v.to_string())
                                            })
                                            numeric=true
                                            min="0"
                                            step="10"
                                            on_commit=Callback::new(move |v: Option<String>| {
                                                set_min_volume(
                                                    commit_numeric(min_volume.get_untracked(), v),
                                                );
                                            })
                                        />
                                    }
                                })
                        }}
```

Note: `format_velocity_floor` renders `-10` cleanly (it only trims trailing zeros after a decimal point) — reused for the drift value.

- [ ] **Step 6: i18n keys (ALL seven locales)**

`en.json`:
```json
    "analyzer_confidence_gte": "Confidence ≥",
    "analyzer_drift_gte": "Drift ≥",
    "analyzer_filter_confidence_min_label": "Min confidence",
    "analyzer_filter_drift_min_label": "Min drift %",
    "analyzer_filter_volume_min_label": "Min 30-day volume",
    "analyzer_volume_gte": "30d volume ≥",
```
`fr.json`:
```json
    "analyzer_confidence_gte": "Confiance ≥",
    "analyzer_drift_gte": "Dérive ≥",
    "analyzer_filter_confidence_min_label": "Confiance min",
    "analyzer_filter_drift_min_label": "Dérive min %",
    "analyzer_filter_volume_min_label": "Volume min sur 30 j",
    "analyzer_volume_gte": "Vol. 30 j ≥",
```
`de.json`:
```json
    "analyzer_confidence_gte": "Konfidenz ≥",
    "analyzer_drift_gte": "Drift ≥",
    "analyzer_filter_confidence_min_label": "Min. Konfidenz",
    "analyzer_filter_drift_min_label": "Min. Drift %",
    "analyzer_filter_volume_min_label": "Min. 30-Tage-Volumen",
    "analyzer_volume_gte": "30-T-Vol. ≥",
```
`ja.json`:
```json
    "analyzer_confidence_gte": "信頼度 ≥",
    "analyzer_drift_gte": "変動率 ≥",
    "analyzer_filter_confidence_min_label": "最低信頼度",
    "analyzer_filter_drift_min_label": "最低変動率 %",
    "analyzer_filter_volume_min_label": "最低30日販売数",
    "analyzer_volume_gte": "30日販売数 ≥",
```
`cn.json`:
```json
    "analyzer_confidence_gte": "置信度 ≥",
    "analyzer_drift_gte": "漂移 ≥",
    "analyzer_filter_confidence_min_label": "最低置信度",
    "analyzer_filter_drift_min_label": "最低漂移 %",
    "analyzer_filter_volume_min_label": "最低30天销量",
    "analyzer_volume_gte": "30天销量 ≥",
```
`ko.json`:
```json
    "analyzer_confidence_gte": "신뢰도 ≥",
    "analyzer_drift_gte": "변동률 ≥",
    "analyzer_filter_confidence_min_label": "최소 신뢰도",
    "analyzer_filter_drift_min_label": "최소 변동률 %",
    "analyzer_filter_volume_min_label": "최소 30일 판매량",
    "analyzer_volume_gte": "30일 판매량 ≥",
```
`tc.json`:
```json
    "analyzer_confidence_gte": "信賴度 ≥",
    "analyzer_drift_gte": "漂移 ≥",
    "analyzer_filter_confidence_min_label": "最低信賴度",
    "analyzer_filter_drift_min_label": "最低漂移 %",
    "analyzer_filter_volume_min_label": "最低30天銷量",
    "analyzer_volume_gte": "30天銷量 ≥",
```

- [ ] **Step 7: Build + test**

```bash
cargo test -p ultros-app --lib analyzer 2>&1 | tail -10
```
Expected: PASS.

- [ ] **Step 8: fmt-check + clippy + commit**

```bash
export PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:$PATH"
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```
Expected: `REAL_EXIT=0`. Then:

```bash
git add ultros-frontend/ultros-app/src/routes/analyzer.rs ultros-frontend/ultros-app/locales/
git commit -m "feat(analyzer): drift, confidence, and 30d-volume filters"
```

---

### Task 5: Full verification pass

**Files:** none new — verification only, plus any fixes it forces.

- [ ] **Step 1: Full test suite**

```bash
export CARGO_TARGET_DIR=/c/Users/chw11/code/ultros/target
cargo test -p ultros-app --lib 2>&1 | tail -15
```
Expected: everything green (CI does NOT run tests, so this local run is the only test gate).

- [ ] **Step 2: Full check_ci**

```bash
export PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:$PATH"
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```
Expected: `REAL_EXIT=0`. If clippy is OOM-killed (exit 137), rerun with `-j 2` per CLAUDE.md.

- [ ] **Step 3: Optional browser smoke (if a local run is feasible)**

Per `reference_ultros_local_browser_test` memory, a local SSR run on Windows needs the `bin-features=[]` / ClickHouse-creds setup. If run: load `/analyzer/<world>?quality=hq&name=tincture&drift=-10&confidence=medium&min-volume=10`, confirm (a) chips render for all five, (b) no hydration panic in the console with a non-EN locale cookie set, (c) `Clear all` removes all five, (d) the `+ Filter` menu lists all five when unset. If not run, note in the PR that verification was unit-test + CI-check only.

- [ ] **Step 4: Commit any fixes**

```bash
git add -A && git commit -m "test(analyzer): verification fixes for column filters"
```
(Skip if nothing changed.)
