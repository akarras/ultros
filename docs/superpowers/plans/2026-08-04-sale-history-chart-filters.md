# Sale History Chart Filters Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the item-page sale-history chart open at the grouping level the user is actually viewing, label its time range legibly, offer 7d/1mo/1y/All quick ranges, and persist all chart state to the query string so charts are shareable.

**Architecture:** All URL encoding/parsing lives in one new pure module (`components/chart_query.rs`) with no reactive dependencies, so it is fully unit-testable. Reactive state stops being plain `signal()`s and becomes *derived reads* over URL params with defaults computed at read time — nothing is seeded on mount. The route component owns only params that gate a fetch (`mode`, `group`, `hq`, time window); the chart component reads its own presentation params directly to avoid a 20-prop signature.

**Tech Stack:** Rust, Leptos 0.8 (`leptos_router` query signals), `chrono`, `leptos-i18n`.

**Spec:** `docs/superpowers/specs/2026-08-04-sale-history-chart-filters-design.md`

## Global Constraints

- **Run `./check_ci.sh` from the repo root before every commit.** It runs `cargo fmt --all -- --check` and `cargo clippy --all-targets -- -D warnings`. Read its exit code explicitly — do NOT pipe into `tail`/`grep` and read `$?`, that reports the pipe's status:
  ```bash
  ./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
  ```
  Clippy exit `137` / `Killed: 9` is an OOM kill, not a lint failure — re-run with `cargo clippy --all-targets -j 2 -- -D warnings`.
- **No hardcoded user-facing strings** in `ultros-frontend/ultros-app/`. Every user-visible string goes through `t!(i18n, key)` (or `t_string!` for attribute values), and the key must be added to **all seven** locale files: `en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`. `leptos-i18n` will not compile with a key missing from any locale.
- **Never `#[allow]` a clippy warning** to silence it unless it is a genuine false positive worth a comment.
- **All URL params use `filter_query_signal`** from `crate::query_defaults` (`replace: true, scroll: false`). A plain `query_signal` pushes a history entry and scrolls to top on every click.
- **Nothing is seeded into the URL on mount.** An absent param means "use the default", computed at read time.
- Tests must not create bare `RwSignal::new` at test scope — an `ultros-app` test doing so panics with "no Arena is active". Every test in this plan is over a pure function, which sidesteps this entirely.
- `cargo test` is **not** run by CI. Green CI only proves the code compiles and lints; run the test commands in this plan yourself.

## File Structure

| File | Responsibility |
|---|---|
| `ultros-frontend/ultros-app/src/components/chart_query.rs` | **new** — pure URL param encode/parse: range presets, overlay set, `show` visibility expression. No reactive code. |
| `ultros-frontend/ultros-charts/src/data/grouping.rs` | `GroupLevel` wire format (`FromStr`/`Display`) + `default_group_level` |
| `ultros-frontend/ultros-charts/src/charts/mod.rs` | `ChartMode` wire format (`FromStr`/`Display`) |
| `ultros-frontend/ultros-app/src/routes/item_view.rs` | fetch-gating params: `mode`, `group`, `hq`, time window; scope-derived grouping default |
| `ultros-frontend/ultros-app/src/components/price_history_chart.rs` | span-adaptive label, quick-range buttons, presentation params (`view`, `overlays`, `show`, `sort`, `cellscale`) |
| `ultros-frontend/ultros-app/src/components/chart_toolbar.rs` | prop types widened to `SignalSetter` |
| `ultros-frontend/ultros-app/locales/*.json` | 5 new range-button keys × 7 locales |

**Line numbers below are as of `342c0b64`** and will drift as tasks land. Locate by the quoted code, not the number.

---

### Task 1: Wire format for `GroupLevel` and `ChartMode`

Both enums need `FromStr`/`Display` before any URL work can reference them. Lowercase on the wire; the existing `label()` methods stay as-is (they are documented as debug/key identifiers, not wire format).

**Files:**
- Modify: `ultros-frontend/ultros-charts/src/data/grouping.rs`
- Modify: `ultros-frontend/ultros-charts/src/charts/mod.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `impl FromStr for GroupLevel { type Err = (); }` and `impl Display for GroupLevel` — wire values `region` / `datacenter` / `world`
  - `impl FromStr for ChartMode { type Err = (); }` and `impl Display for ChartMode` — wire values `price` / `candles` / `range` / `density`
  - `pub fn default_group_level(world_helper: &WorldHelper, scope_name: &str) -> GroupLevel`

- [ ] **Step 1: Write the failing tests**

Append to the `mod tests` block in `ultros-frontend/ultros-charts/src/data/grouping.rs`:

```rust
    #[test]
    fn group_level_wire_format_round_trips() {
        for level in [
            GroupLevel::Region,
            GroupLevel::Datacenter,
            GroupLevel::World,
        ] {
            assert_eq!(level.to_string().parse::<GroupLevel>(), Ok(level));
        }
        assert_eq!(GroupLevel::Region.to_string(), "region");
        assert_eq!(GroupLevel::Datacenter.to_string(), "datacenter");
        assert_eq!(GroupLevel::World.to_string(), "world");
    }

    // URLs get hand-edited and lowercased by tools, so parsing is
    // case- and whitespace-insensitive. `dc` is accepted as a convenience
    // alias for hand-authored links.
    #[test]
    fn group_level_parsing_is_forgiving() {
        assert_eq!("REGION".parse::<GroupLevel>(), Ok(GroupLevel::Region));
        assert_eq!("  world ".parse::<GroupLevel>(), Ok(GroupLevel::World));
        assert_eq!("dc".parse::<GroupLevel>(), Ok(GroupLevel::Datacenter));
        assert_eq!("nonsense".parse::<GroupLevel>(), Err(()));
    }

    // The fix for the original defect: a scope page opens at the broadest
    // level it can offer, not always World.
    #[test]
    fn default_group_level_follows_the_viewed_scope() {
        let h = world_helper();
        assert_eq!(default_group_level(&h, "North-America"), GroupLevel::Region);
        assert_eq!(default_group_level(&h, "Aether"), GroupLevel::Datacenter);
        assert_eq!(default_group_level(&h, "Gilgamesh"), GroupLevel::World);
        // An unknown scope offers everything, so it defaults to the broadest.
        assert_eq!(default_group_level(&h, "Not A Scope"), GroupLevel::Region);
    }
```

Append to the `mod tests` block in `ultros-frontend/ultros-charts/src/charts/mod.rs`:

```rust
    #[test]
    fn chart_mode_wire_format_round_trips() {
        use std::str::FromStr;
        for mode in [
            ChartMode::Price,
            ChartMode::Candles,
            ChartMode::Range,
            ChartMode::Density,
        ] {
            assert_eq!(ChartMode::from_str(&mode.to_string()), Ok(mode));
        }
        assert_eq!(ChartMode::Price.to_string(), "price");
        assert_eq!(ChartMode::Density.to_string(), "density");
    }

    #[test]
    fn chart_mode_parsing_is_forgiving() {
        use std::str::FromStr;
        assert_eq!(ChartMode::from_str("CANDLES"), Ok(ChartMode::Candles));
        assert_eq!(ChartMode::from_str(" range "), Ok(ChartMode::Range));
        assert_eq!(ChartMode::from_str("nonsense"), Err(()));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p ultros-charts --lib grouping
```
Expected: FAIL to compile — `default_group_level` not found, `GroupLevel` doesn't implement `FromStr`/`Display`.

- [ ] **Step 3: Implement `GroupLevel` wire format and default**

In `ultros-frontend/ultros-charts/src/data/grouping.rs`, add `use std::str::FromStr;` at the top, then add after the `impl GroupLevel` block:

```rust
/// Wire format for the `?group=` URL param. Lowercase and stable — this is
/// part of every shared chart link, so the strings must not be changed
/// casually. Distinct from [`GroupLevel::label`], which is a debug/key
/// identifier.
impl std::fmt::Display for GroupLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Region => "region",
            Self::Datacenter => "datacenter",
            Self::World => "world",
        })
    }
}

impl FromStr for GroupLevel {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "region" => Ok(Self::Region),
            "datacenter" | "dc" => Ok(Self::Datacenter),
            "world" => Ok(Self::World),
            _ => Err(()),
        }
    }
}
```

And after `available_group_levels`:

```rust
/// The grouping a scope page should open at: the broadest level that scope
/// can offer. A region page shows regions, a datacenter page datacenters, a
/// world page worlds.
///
/// Previously the item page hardcoded `World` at every scope, so a region
/// page overlaid ~70 world lines when the user had asked to look at a region.
pub fn default_group_level(world_helper: &WorldHelper, scope_name: &str) -> GroupLevel {
    available_group_levels(world_helper, scope_name)
        .first()
        .copied()
        .unwrap_or(GroupLevel::World)
}
```

- [ ] **Step 4: Implement `ChartMode` wire format**

In `ultros-frontend/ultros-charts/src/charts/mod.rs`, add after the `impl ChartMode` block:

```rust
/// Wire format for the `?mode=` URL param. Lowercase and stable — part of
/// every shared chart link. Distinct from [`ChartMode::label`], which is a
/// debug/key identifier.
impl std::fmt::Display for ChartMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Price => "price",
            Self::Candles => "candles",
            Self::Range => "range",
            Self::Density => "density",
        })
    }
}

impl std::str::FromStr for ChartMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "price" => Ok(Self::Price),
            "candles" => Ok(Self::Candles),
            "range" => Ok(Self::Range),
            "density" => Ok(Self::Density),
            _ => Err(()),
        }
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cargo test -p ultros-charts --lib
```
Expected: PASS, including the 5 new tests.

- [ ] **Step 6: Run CI checks and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

```bash
git add ultros-frontend/ultros-charts/src/data/grouping.rs ultros-frontend/ultros-charts/src/charts/mod.rs
git commit -m "feat(charts): wire format for GroupLevel and ChartMode

Adds lowercase FromStr/Display for both enums so they can back URL query
params, plus default_group_level: the broadest grouping a scope can offer.
Parsing is case- and whitespace-insensitive because these values get
hand-edited in shared links."
```

---

### Task 2: `chart_query.rs` — range presets

The quick-range presets and their resolution to an absolute window. Pure functions taking `now` as a parameter rather than reading the clock, so they are deterministic under test.

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/chart_query.rs`
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub enum RangePreset { Week, Month, Year }` with `pub const ALL: [RangePreset; 3]`, `pub fn seconds(self) -> i64`, `FromStr` (`7d`/`1mo`/`1y`), `Display`
  - `pub fn resolve_range(preset: Option<RangePreset>, from_to: Option<(i64, i64)>, now: i64) -> Option<(i64, i64)>`
  - `pub fn preset_has_data(preset: RangePreset, domain_end: i64, now: i64) -> bool`

- [ ] **Step 1: Create the module with its failing tests**

Create `ultros-frontend/ultros-app/src/components/chart_query.rs`:

```rust
//! URL query encoding for the item-page price chart.
//!
//! Every function here is pure: no reactive reads, no clock access (`now` is
//! always a parameter). That keeps the whole encoding layer unit-testable,
//! which matters because on a local debug build `query_signal` *writes* are
//! inert while reads still work — these tests are the only place the
//! round-trip behaviour can actually be verified without a release build.

use std::str::FromStr;

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;
    // 2026-07-05 18:00:00 UTC — a fixed "now" so tests never depend on
    // the wall clock.
    const NOW: i64 = 1_783_360_800;

    #[test]
    fn range_preset_wire_format_round_trips() {
        for preset in RangePreset::ALL {
            assert_eq!(preset.to_string().parse::<RangePreset>(), Ok(preset));
        }
        assert_eq!(RangePreset::Week.to_string(), "7d");
        assert_eq!(RangePreset::Month.to_string(), "1mo");
        assert_eq!(RangePreset::Year.to_string(), "1y");
    }

    #[test]
    fn range_preset_parsing_is_forgiving() {
        assert_eq!("7D".parse::<RangePreset>(), Ok(RangePreset::Week));
        assert_eq!(" 1mo ".parse::<RangePreset>(), Ok(RangePreset::Month));
        assert_eq!("nonsense".parse::<RangePreset>(), Err(()));
    }

    #[test]
    fn a_preset_resolves_to_a_window_ending_now() {
        assert_eq!(
            resolve_range(Some(RangePreset::Week), None, NOW),
            Some((NOW - 7 * DAY, NOW))
        );
    }

    // The spec's precedence rule: a link carrying both shapes is a relative
    // link, because `range` is what a preset click writes.
    #[test]
    fn a_preset_wins_over_absolute_bounds() {
        assert_eq!(
            resolve_range(Some(RangePreset::Week), Some((1, 2)), NOW),
            Some((NOW - 7 * DAY, NOW))
        );
    }

    #[test]
    fn absolute_bounds_are_used_when_no_preset_is_set() {
        assert_eq!(resolve_range(None, Some((1, 2)), NOW), Some((1, 2)));
    }

    #[test]
    fn no_params_means_full_range() {
        assert_eq!(resolve_range(None, None, NOW), None);
    }

    // The dead-item case: an item whose newest sale predates the whole
    // window would render blank, so the button is disabled instead.
    #[test]
    fn a_preset_with_no_data_in_window_is_unavailable() {
        assert!(!preset_has_data(RangePreset::Week, NOW - 30 * DAY, NOW));
        assert!(preset_has_data(RangePreset::Month, NOW - 30 * DAY + 1, NOW));
    }

    // A domain ending exactly at the window's start still contains that
    // boundary bucket — off-by-one here silently disables a usable button.
    #[test]
    fn a_domain_ending_exactly_at_the_window_start_is_available() {
        assert!(preset_has_data(RangePreset::Week, NOW - 7 * DAY, NOW));
    }
}
```

- [ ] **Step 2: Register the module and run the tests to verify they fail**

Add to `ultros-frontend/ultros-app/src/components/mod.rs`, keeping the existing alphabetical ordering (immediately after the `chart_toolbar` entry):

```rust
pub mod chart_query;
```

```bash
cargo test -p ultros-app --lib chart_query
```
Expected: FAIL to compile — `RangePreset`, `resolve_range`, `preset_has_data` not found.

- [ ] **Step 3: Implement the range presets**

Insert into `chart_query.rs`, above the `mod tests` block:

```rust
/// A quick-range button. Anchored to *now*, not to the newest data point, so
/// a shared `?range=7d` link means the same thing to every viewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangePreset {
    Week,
    Month,
    Year,
}

impl RangePreset {
    /// Display order for the button row.
    pub const ALL: [RangePreset; 3] = [Self::Week, Self::Month, Self::Year];

    /// Window length in seconds. A month is 30 days and a year 365; these
    /// are button labels, not calendar arithmetic.
    pub fn seconds(self) -> i64 {
        const DAY: i64 = 86_400;
        match self {
            Self::Week => 7 * DAY,
            Self::Month => 30 * DAY,
            Self::Year => 365 * DAY,
        }
    }
}

/// Wire format for `?range=`. Stable — part of every shared chart link.
impl std::fmt::Display for RangePreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Week => "7d",
            Self::Month => "1mo",
            Self::Year => "1y",
        })
    }
}

impl FromStr for RangePreset {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "7d" => Ok(Self::Week),
            "1mo" => Ok(Self::Month),
            "1y" => Ok(Self::Year),
            _ => Err(()),
        }
    }
}

/// Resolve the URL's range params into an absolute window, or `None` for
/// full range.
///
/// A preset wins over absolute bounds: `?range=` is what a preset click
/// writes, so its presence means the link is deliberately relative.
/// `normalize_time_range` clamps the result to the available domain later,
/// which is what makes `1y` on a six-month-old item show those six months
/// rather than erroring.
pub fn resolve_range(
    preset: Option<RangePreset>,
    from_to: Option<(i64, i64)>,
    now: i64,
) -> Option<(i64, i64)> {
    match preset {
        Some(preset) => Some((now - preset.seconds(), now)),
        None => from_to,
    }
}

/// Whether a preset's window contains any data at all.
///
/// False means the newest sale predates the whole window, so clicking the
/// button would blank the chart — the button is disabled with a reason
/// instead.
pub fn preset_has_data(preset: RangePreset, domain_end: i64, now: i64) -> bool {
    domain_end >= now - preset.seconds()
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p ultros-app --lib chart_query
```
Expected: PASS, 8 tests.

- [ ] **Step 5: Run CI checks and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

```bash
git add ultros-frontend/ultros-app/src/components/chart_query.rs ultros-frontend/ultros-app/src/components/mod.rs
git commit -m "feat(chart): range preset parsing and resolution

Pure encoding layer for the ?range= param: 7d/1mo/1y presets anchored to
now, precedence over absolute ?from/?to bounds, and the has-data predicate
that disables a preset whose window would be empty."
```

---

### Task 3: `chart_query.rs` — the `show` visibility expression

The base+delta grammar. This is the subtlest task in the plan; the two safety rules are not optional.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/chart_query.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces:
  - `pub fn parse_show(expr: &str, series: &[String]) -> Vec<String>` — returns the names that should be **hidden**
  - `pub fn encode_show(hidden: &[String], series: &[String]) -> Option<String>` — `None` when nothing is hidden (param omitted)

- [ ] **Step 1: Write the failing tests**

Append inside the existing `mod tests` block in `chart_query.rs`:

```rust
    fn series() -> Vec<String> {
        ["Gilgamesh", "Sargatanas", "Faerie", "Siren"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn hidden(expr: &str) -> Vec<String> {
        parse_show(expr, &series())
    }

    #[test]
    fn an_all_base_treats_listed_names_as_exclusions() {
        assert_eq!(hidden("all,-Gilgamesh"), vec!["Gilgamesh".to_string()]);
        // The sign is optional under `all`.
        assert_eq!(hidden("all,Gilgamesh"), vec!["Gilgamesh".to_string()]);
    }

    #[test]
    fn a_none_base_treats_listed_names_as_inclusions() {
        assert_eq!(
            hidden("none,+Gilgamesh,+Sargatanas"),
            vec!["Faerie".to_string(), "Siren".to_string()]
        );
        // The sign is optional under `none` too.
        assert_eq!(
            hidden("none,Gilgamesh,Sargatanas"),
            vec!["Faerie".to_string(), "Siren".to_string()]
        );
    }

    // Convenience for hand-authored links: a bare list implies `all`.
    #[test]
    fn an_omitted_base_implies_all() {
        assert_eq!(hidden("Gilgamesh"), vec!["Gilgamesh".to_string()]);
    }

    #[test]
    fn names_match_case_insensitively() {
        assert_eq!(hidden("all,-gilgamesh"), vec!["Gilgamesh".to_string()]);
        assert_eq!(hidden("ALL,-GILGAMESH"), vec!["Gilgamesh".to_string()]);
    }

    #[test]
    fn an_empty_expression_hides_nothing() {
        assert!(hidden("").is_empty());
        assert!(hidden("all").is_empty());
    }

    // Unmatched exclusions under `all` are simply inert — this is why `all`
    // is the safer base and wins ties in encode_show.
    #[test]
    fn unmatched_exclusions_are_inert() {
        assert!(hidden("all,-Nonexistent").is_empty());
    }

    // SAFETY RULE 1. The series set depends on the group level, so a link
    // authored at World grouping carries world names that match nothing at
    // Region grouping. `none` plus zero matches would blank the chart, and a
    // blank chart from a stale link is indistinguishable from a bug.
    #[test]
    fn a_stale_none_expression_fails_open() {
        assert!(hidden("none,+Europe,+Japan").is_empty());
    }

    // ...but a `none` base with NO deltas is an explicit, unambiguous
    // "hide everything", not a stale link. It must round-trip honestly.
    #[test]
    fn an_explicit_none_with_no_deltas_hides_everything() {
        assert_eq!(hidden("none"), series());
    }

    // A partially-stale expression is not stale: one match is enough to
    // prove the link still refers to this series set.
    #[test]
    fn a_partially_matching_none_expression_is_honoured() {
        assert_eq!(
            hidden("none,+Gilgamesh,+Europe"),
            vec![
                "Sargatanas".to_string(),
                "Faerie".to_string(),
                "Siren".to_string()
            ]
        );
    }

    // SAFETY RULE 2. leptos_router unescapes with decodeURIComponent /
    // percent_decode, NOT form-urlencoding, so `+` survives as a literal
    // rather than becoming a space. This test pins that assumption: if the
    // decoder ever changes, every `none,+...` link silently breaks.
    #[test]
    fn a_literal_plus_prefix_parses_as_an_inclusion() {
        assert_eq!(
            hidden("none,+Gilgamesh"),
            vec![
                "Sargatanas".to_string(),
                "Faerie".to_string(),
                "Siren".to_string()
            ]
        );
    }

    #[test]
    fn nothing_hidden_encodes_to_no_param() {
        assert_eq!(encode_show(&[], &series()), None);
    }

    #[test]
    fn a_minority_hidden_encodes_with_an_all_base() {
        assert_eq!(
            encode_show(&["Gilgamesh".to_string()], &series()),
            Some("all,-Gilgamesh".to_string())
        );
    }

    #[test]
    fn a_majority_hidden_encodes_with_a_none_base() {
        let hidden_names = [
            "Gilgamesh".to_string(),
            "Sargatanas".to_string(),
            "Faerie".to_string(),
        ];
        assert_eq!(
            encode_show(&hidden_names, &series()),
            Some("none,+Siren".to_string())
        );
    }

    // The user's requirement: never list more than about half the series.
    #[test]
    fn a_tie_favours_the_all_base() {
        let hidden_names = ["Gilgamesh".to_string(), "Sargatanas".to_string()];
        assert_eq!(
            encode_show(&hidden_names, &series()),
            Some("all,-Gilgamesh,-Sargatanas".to_string())
        );
    }

    #[test]
    fn deltas_are_emitted_alphabetically() {
        let hidden_names = ["Sargatanas".to_string(), "Gilgamesh".to_string()];
        assert_eq!(
            encode_show(&hidden_names, &series()),
            Some("all,-Gilgamesh,-Sargatanas".to_string())
        );
    }

    // Hidden names that aren't in the current series set must not reach the
    // URL, or the expression would grow without bound as the user switches
    // grouping levels.
    #[test]
    fn encoding_ignores_hidden_names_outside_the_series_set() {
        let hidden_names = ["Gilgamesh".to_string(), "Europe".to_string()];
        assert_eq!(
            encode_show(&hidden_names, &series()),
            Some("all,-Gilgamesh".to_string())
        );
    }

    #[test]
    fn show_round_trips_through_encode_and_parse() {
        let cases: Vec<Vec<String>> = vec![
            vec!["Gilgamesh".to_string()],
            vec!["Gilgamesh".to_string(), "Sargatanas".to_string()],
            vec![
                "Gilgamesh".to_string(),
                "Sargatanas".to_string(),
                "Faerie".to_string(),
            ],
        ];
        for hidden_names in cases {
            let encoded = encode_show(&hidden_names, &series()).unwrap();
            let mut round_tripped = parse_show(&encoded, &series());
            let mut expected = hidden_names.clone();
            round_tripped.sort();
            expected.sort();
            assert_eq!(round_tripped, expected, "via {encoded}");
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p ultros-app --lib chart_query
```
Expected: FAIL to compile — `parse_show` and `encode_show` not found.

- [ ] **Step 3: Implement the `show` grammar**

Insert into `chart_query.rs`, above the `mod tests` block:

```rust
// ── `show`: a visibility expression ──────────────────────────────────────
//
//   show := base ("," item)*
//   base := "all" | "none"
//   item := ("+" | "-")? name
//
// Named `show` rather than `hide` because `hide=all` would read as "hide
// everything" while meaning the opposite. Under base `all` a bare or
// `-`-prefixed name excludes; under `none` a bare or `+`-prefixed name
// includes. The base may be omitted, in which case `all` is assumed, so a
// bare list still means "hide these".
//
// The encoder picks whichever base is shorter, which bounds the parameter to
// ceil(n/2) + 1 tokens — on a region page with 70 worlds that is the
// difference between a usable link and an unusable one.

/// Which series are visible before deltas are applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShowBase {
    All,
    None,
}

/// Resolve a `show` expression against the series currently on the chart,
/// returning the names that should be **hidden**.
///
/// Unknown names are ignored rather than rejected: the series set depends on
/// the grouping level, so a perfectly valid link can name series that don't
/// exist at the current level.
pub fn parse_show(expr: &str, series: &[String]) -> Vec<String> {
    let tokens: Vec<&str> = expr
        .split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .collect();
    let Some(first) = tokens.first() else {
        return Vec::new();
    };

    let (base, deltas) = match first.to_ascii_lowercase().as_str() {
        "all" => (ShowBase::All, &tokens[1..]),
        "none" => (ShowBase::None, &tokens[1..]),
        // No recognised base: treat the whole list as exclusions.
        _ => (ShowBase::All, &tokens[..]),
    };

    let mut visible = vec![base == ShowBase::All; series.len()];
    // Tracked so a *stale* `none` link can be told apart from a deliberate
    // "hide everything" — see the fail-open rule below.
    let mut includes = 0usize;
    let mut matched_includes = 0usize;

    for token in deltas {
        let (include, name) = match token.strip_prefix('+') {
            Some(name) => (true, name),
            None => match token.strip_prefix('-') {
                Some(name) => (false, name),
                // A bare name takes its polarity from the base.
                None => (base == ShowBase::None, *token),
            },
        };
        let name = name.trim();
        if include {
            includes += 1;
        }

        let mut matched = false;
        for (index, series_name) in series.iter().enumerate() {
            if series_name.eq_ignore_ascii_case(name) {
                visible[index] = include;
                matched = true;
            }
        }
        if include && matched {
            matched_includes += 1;
        }
    }

    // FAIL OPEN. A `none` base whose includes matched nothing is a link
    // authored against a different series set — most often a different
    // grouping level. Honouring it would render a blank chart, which is
    // indistinguishable from a bug. A `none` base with no deltas at all is
    // different: that is an explicit "hide everything" and round-trips
    // honestly.
    if base == ShowBase::None && includes > 0 && matched_includes == 0 {
        return Vec::new();
    }

    series
        .iter()
        .zip(&visible)
        .filter(|(_, visible)| !**visible)
        .map(|(name, _)| name.clone())
        .collect()
}

/// Encode the hidden set as the shortest valid `show` expression, or `None`
/// when nothing is hidden (the param is then omitted from the URL entirely).
///
/// Hidden names outside the current series set are dropped — otherwise the
/// expression would accumulate stale names as the user switches grouping.
pub fn encode_show(hidden: &[String], series: &[String]) -> Option<String> {
    let is_hidden =
        |name: &String| hidden.iter().any(|entry| entry.eq_ignore_ascii_case(name));

    let mut hidden_names: Vec<&str> = series
        .iter()
        .filter(|name| is_hidden(name))
        .map(String::as_str)
        .collect();
    if hidden_names.is_empty() {
        return None;
    }
    let mut visible_names: Vec<&str> = series
        .iter()
        .filter(|name| !is_hidden(name))
        .map(String::as_str)
        .collect();

    // Ties favour `all`: unmatched exclusions are inert, so an `all`
    // expression can never fail open the way a stale `none` list can.
    let (base, sign, names) = if hidden_names.len() <= visible_names.len() {
        hidden_names.sort_unstable();
        ("all", '-', hidden_names)
    } else {
        visible_names.sort_unstable();
        ("none", '+', visible_names)
    };

    let mut encoded = String::from(base);
    for name in names {
        encoded.push(',');
        encoded.push(sign);
        encoded.push_str(name);
    }
    Some(encoded)
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p ultros-app --lib chart_query
```
Expected: PASS, 25 tests.

- [ ] **Step 5: Run CI checks and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

```bash
git add ultros-frontend/ultros-app/src/components/chart_query.rs
git commit -m "feat(chart): show visibility expression for the ?show= param

Base+delta grammar (all,-X / none,+X) so a region page's 70-world filter
never needs more than half the names in the URL. Two safety rules are
tested explicitly: a stale none-expression whose includes match nothing
fails open rather than blanking the chart, and a literal + parses as an
inclusion (leptos_router percent-decodes rather than form-decodes, so + is
not a space -- if that ever changes, this test catches it)."
```

---

### Task 4: `chart_query.rs` — the overlay set

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/chart_query.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `pub struct Overlays { pub market_average: bool, pub trend: bool, pub quantity: bool, pub percent_change: bool, pub patches: bool }` with `Default` (`market_average` and `patches` on), `FromStr`, `Display`.

- [ ] **Step 1: Write the failing tests**

Append inside the `mod tests` block in `chart_query.rs`:

```rust
    #[test]
    fn overlay_defaults_are_market_average_and_patches() {
        let overlays = Overlays::default();
        assert!(overlays.market_average);
        assert!(overlays.patches);
        assert!(!overlays.trend);
        assert!(!overlays.quantity);
        assert!(!overlays.percent_change);
    }

    #[test]
    fn overlays_round_trip() {
        let overlays = Overlays {
            market_average: true,
            trend: true,
            quantity: false,
            percent_change: false,
            patches: true,
        };
        assert_eq!(overlays.to_string(), "avg,trend,patches");
        assert_eq!(overlays.to_string().parse::<Overlays>(), Ok(overlays));
    }

    // Without a sentinel, "everything off" would encode to an empty value
    // and parse back as the default set — the one state that cannot survive
    // a round trip.
    #[test]
    fn all_overlays_off_round_trips_via_the_none_sentinel() {
        let overlays = Overlays {
            market_average: false,
            trend: false,
            quantity: false,
            percent_change: false,
            patches: false,
        };
        assert_eq!(overlays.to_string(), "none");
        assert_eq!("none".parse::<Overlays>(), Ok(overlays));
    }

    // Unknown tokens are ignored rather than rejected, so a link written by
    // a newer build that gained an overlay still applies the tokens this
    // build understands instead of falling back to the default set.
    #[test]
    fn unknown_overlay_tokens_are_ignored() {
        let parsed = "avg,newthing".parse::<Overlays>().unwrap();
        assert!(parsed.market_average);
        assert!(!parsed.patches);
    }

    #[test]
    fn overlay_parsing_is_forgiving() {
        let parsed = " AVG , trend ".parse::<Overlays>().unwrap();
        assert!(parsed.market_average);
        assert!(parsed.trend);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p ultros-app --lib chart_query
```
Expected: FAIL to compile — `Overlays` not found.

- [ ] **Step 3: Implement `Overlays`**

Insert into `chart_query.rs`, above the `mod tests` block:

```rust
// ── `overlays`: which overlay toggles are on ─────────────────────────────

/// The chart's overlay toggles as one URL param.
///
/// A single comma-separated param rather than five booleans: five params
/// would dominate the query string, and they are read and written together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Overlays {
    pub market_average: bool,
    pub trend: bool,
    pub quantity: bool,
    pub percent_change: bool,
    pub patches: bool,
}

impl Default for Overlays {
    /// Market average and patch bands on; the rest off. Matches the
    /// component defaults these params replace.
    fn default() -> Self {
        Self {
            market_average: true,
            trend: false,
            quantity: false,
            percent_change: false,
            patches: true,
        }
    }
}

impl std::fmt::Display for Overlays {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut tokens = Vec::with_capacity(5);
        if self.market_average {
            tokens.push("avg");
        }
        if self.trend {
            tokens.push("trend");
        }
        if self.quantity {
            tokens.push("qty");
        }
        if self.percent_change {
            tokens.push("pct");
        }
        if self.patches {
            tokens.push("patches");
        }
        // "Everything off" needs a sentinel: an empty value would parse back
        // as the default set, so it is the one state that could not survive
        // a round trip.
        if tokens.is_empty() {
            f.write_str("none")
        } else {
            f.write_str(&tokens.join(","))
        }
    }
}

impl FromStr for Overlays {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut overlays = Self {
            market_average: false,
            trend: false,
            quantity: false,
            percent_change: false,
            patches: false,
        };
        for token in s.split(',') {
            match token.trim().to_ascii_lowercase().as_str() {
                "avg" => overlays.market_average = true,
                "trend" => overlays.trend = true,
                "qty" => overlays.quantity = true,
                "pct" => overlays.percent_change = true,
                "patches" => overlays.patches = true,
                // "none", empty, and anything unrecognised: ignored, so a
                // link from a build with more overlays still applies the
                // tokens this build understands.
                _ => {}
            }
        }
        Ok(overlays)
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p ultros-app --lib chart_query
```
Expected: PASS, 30 tests.

- [ ] **Step 5: Run CI checks and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

```bash
git add ultros-frontend/ultros-app/src/components/chart_query.rs
git commit -m "feat(chart): overlay set encoding for the ?overlays= param

Five toggles as one comma-separated param, with a 'none' sentinel so
everything-off round-trips instead of parsing back as the default set.
Unknown tokens are ignored so links from newer builds degrade gracefully."
```

---

### Task 5: Span-adaptive timeline label

Ships a visible fix on its own: the chart's range label and its `aria-label` stop omitting the year.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/price_history_chart.rs` (`format_timeline_ts` at ~:102, its test at ~:312, `range_label` at ~:383, `aria-label` at ~:1266)

**Interfaces:**
- Consumes: nothing.
- Produces: `fn format_timeline_ts(ts: i64, utc_offset_minutes: i32, span_seconds: i64) -> String` — note the **new third parameter**; all callers must pass the selected span.

- [ ] **Step 1: Rewrite the existing test to drive the new signature**

In `price_history_chart.rs`, **replace** the existing `test_format_timeline_ts` with:

```rust
    #[test]
    fn test_format_timeline_ts() {
        const DAY: i64 = 86_400;
        // Under 30 days: full precision, including the year. A 7-day drag
        // into a past year is exactly where the old fixed "%m-%d %H:%M"
        // misled most.
        // 1609459200 is 2021-01-01 00:00:00 UTC.
        assert_eq!(
            format_timeline_ts(1609459200, 0, 7 * DAY),
            "2021-01-01 00:00"
        );
        assert_eq!(
            format_timeline_ts(1609459200, 60, 7 * DAY),
            "2021-01-01 01:00"
        );
        assert_eq!(
            format_timeline_ts(1609459200, -120, 7 * DAY),
            "2020-12-31 22:00"
        );

        // 30 days and over: the clock stops carrying information.
        assert_eq!(format_timeline_ts(1609459200, 0, 60 * DAY), "2021-01-01");

        // Two years and over: the day stops carrying information too. This
        // is the reported case — a 2023..2026 domain used to render as
        // "02-21 18:00", which reads as the current year.
        assert_eq!(format_timeline_ts(1609459200, 0, 1200 * DAY), "2021-01");
    }

    #[test]
    fn timeline_format_tiers_switch_at_their_boundaries() {
        const DAY: i64 = 86_400;
        assert_eq!(timeline_format(30 * DAY - 1), "%Y-%m-%d %H:%M");
        assert_eq!(timeline_format(30 * DAY), "%Y-%m-%d");
        assert_eq!(timeline_format(2 * 365 * DAY - 1), "%Y-%m-%d");
        assert_eq!(timeline_format(2 * 365 * DAY), "%Y-%m");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p ultros-app --lib price_history_chart
```
Expected: FAIL to compile — `format_timeline_ts` takes 2 arguments, `timeline_format` not found.

- [ ] **Step 3: Implement the adaptive format**

**Replace** `format_timeline_ts` (~:102) with:

```rust
/// Timestamp format for a label describing a window of `span_seconds`.
///
/// The old fixed `%m-%d %H:%M` rendered a three-year domain as
/// `02-21 18:00 - 07-05 18:00`, which reads as a four-month window in the
/// current year. Each tier carries exactly the precision its span needs, and
/// none of them omit the year.
fn timeline_format(span_seconds: i64) -> &'static str {
    const DAY: i64 = 86_400;
    if span_seconds >= 2 * 365 * DAY {
        "%Y-%m"
    } else if span_seconds >= 30 * DAY {
        "%Y-%m-%d"
    } else {
        "%Y-%m-%d %H:%M"
    }
}

fn format_timeline_ts(ts: i64, utc_offset_minutes: i32, span_seconds: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(ts, 0)
        .map(|dt| {
            (dt + chrono::TimeDelta::minutes(utc_offset_minutes as i64))
                .format(timeline_format(span_seconds))
                .to_string()
        })
        .unwrap_or_default()
}
```

- [ ] **Step 4: Update the two call sites**

In `TimelineSlicer`, **replace** `range_label` (~:383) with:

```rust
    let range_label = move || {
        selected_domain
            .get()
            .map(|(start, end)| {
                let offset = utc_offset_minutes.get();
                let span = end - start;
                format!(
                    "{} - {}",
                    format_timeline_ts(start, offset, span),
                    format_timeline_ts(end, offset, span)
                )
            })
            .unwrap_or_default()
    };
```

The sub-30-day form is long for a truncating container, so give the label element a `title`. **Replace** the label `<div>` (~:441) with:

```rust
                        <div
                            class="truncate text-xs tabular-nums text-[color:var(--color-text)]/75"
                            title=range_label
                        >
                            {range_label}
                        </div>
```

In the main component's `aria-label` closure (~:1266), **replace** the two `format_timeline_ts` calls:

```rust
                        .map(|(start, end)| {
                            let offset = utc_offset.get();
                            let span = end - start;
                            (
                                format_timeline_ts(start, offset, span),
                                format_timeline_ts(end, offset, span),
                            )
                        })
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cargo test -p ultros-app --lib price_history_chart
```
Expected: PASS, 11 tests.

- [ ] **Step 6: Run CI checks and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

```bash
git add ultros-frontend/ultros-app/src/components/price_history_chart.rs
git commit -m "fix(chart): timeline label picks its format from the selected span

A 2023..2026 domain rendered as '02-21 18:00 - 07-05 18:00' because the
format was hardcoded to %m-%d %H:%M, so a three-year range read as four
months in the current year. Three tiers now carry the precision their span
needs and none omit the year; the label also gains a title attribute since
the sub-30-day form can truncate. Fixes the aria-label too, which used the
same helper."
```

---

### Task 6: Grouping defaults to the viewed scope

The original defect, plus the `?group=` param that replaces the signal.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/item_view.rs` (~:1258, ~:1441)
- Modify: `ultros-frontend/ultros-app/src/components/price_history_chart.rs` (prop at ~:720, dead Effect at ~:779)
- Modify: `ultros-frontend/ultros-app/src/components/chart_toolbar.rs` (prop at ~:54)

**Interfaces:**
- Consumes: `default_group_level` and `GroupLevel: FromStr + Display` (Task 1); `filter_query_signal` from `crate::query_defaults`.
- Produces: `group: Signal<GroupLevel>` derived from `?group=`; `set_group: SignalSetter<GroupLevel>` prop on both `PriceHistoryChart` and `ChartToolbar`.

- [ ] **Step 1: Widen the setter props to `SignalSetter`**

`SignalSetter<T>` has `impl From<WriteSignal<T>>`, so marking the prop `#[prop(into)]` keeps every existing `WriteSignal` call site working while allowing a mapped setter.

In `chart_toolbar.rs`, **replace** the `set_group` prop (~:54):

```rust
    #[prop(into)] set_group: SignalSetter<GroupLevel>,
```

In `price_history_chart.rs`, **replace** the `set_group` prop (~:721):

```rust
    #[prop(into)] set_group: SignalSetter<GroupLevel>,
```

- [ ] **Step 2: Delete the now-unreachable corrective Effect**

`group` becomes a derived read that already filters invalid levels, so this write-back loop can never fire. In `price_history_chart.rs`, **delete** the whole block at ~:775-786:

```rust
    // If the scope changes underneath an existing selection (e.g. navigating
    // from a datacenter page to a single-world page) and the current group
    // no longer makes sense, snap it to the narrowest still-valid option
    // rather than requesting a grouping the scope can't offer.
    Effect::new(move |_| {
        let options = color_by_options.get();
        if !options.contains(&group.get_untracked())
            && let Some(first) = options.first()
        {
            set_group.set(*first);
        }
    });
```

- [ ] **Step 3: Derive `group` from the URL in `item_view.rs`**

Add to the imports at the top of `item_view.rs`:

```rust
use crate::query_defaults::filter_query_signal;
use ultros_charts::data::grouping::default_group_level;
```

**Replace** `let (group, set_group) = signal(GroupLevel::World);` (~:1258) with:

```rust
    // Grouping is a derived read over `?group=`, not a signal: an absent
    // param means "the broadest level this scope offers", computed at read
    // time. That gives a region page region lines instead of ~70 world lines
    // (it used to hardcode World, which is valid at every scope, so the
    // corrective Effect in the chart never fired).
    //
    // Filtering by the scope's available levels means a shared `?group=region`
    // link opened on a *world* page degrades to World rather than requesting
    // a grouping the scope cannot serve. Deriving rather than seeding also
    // means navigating region -> world needs no write and cannot lose a race
    // with the world picker's mount-time rebuild.
    let (group_param, set_group_param) = filter_query_signal::<GroupLevel>("group");
    // `world_data` is the Arc<WorldHelper> already bound near the top of this
    // component (`let world_data = use_context::<LocalWorldData>()...`).
    let group_helper = world_data.clone();
    let group_default_helper = world_data.clone();
    let group = Signal::derive(move || {
        let scope = world.get();
        group_param
            .get()
            .filter(|level| {
                ultros_charts::data::grouping::available_group_levels(&group_helper, &scope)
                    .contains(level)
            })
            .unwrap_or_else(|| default_group_level(&group_default_helper, &scope))
    });
    let set_group = SignalSetter::map(move |level: GroupLevel| {
        set_group_param.set(Some(level));
    });
```

`world_data` is bound at `item_view.rs:1148` in this same component, so no new context lookup is needed.

- [ ] **Step 4: Verify it compiles**

```bash
cargo check -p ultros-app --lib
```
Expected: clean. If `SignalSetter::map` is not in scope, it comes from `leptos::prelude::*`, which `item_view.rs` already imports.

- [ ] **Step 5: Run the full app test suite**

```bash
cargo test -p ultros-app --lib
```
Expected: PASS — no test regressions.

- [ ] **Step 6: Run CI checks and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

```bash
git add ultros-frontend/ultros-app/src/routes/item_view.rs ultros-frontend/ultros-app/src/components/price_history_chart.rs ultros-frontend/ultros-app/src/components/chart_toolbar.rs
git commit -m "fix(chart): group by the scope being viewed, and persist it

The item page hardcoded GroupLevel::World, and the chart's corrective
Effect only fired when the current level was *invalid* for the scope --
World is valid everywhere, so a region page overlaid ~70 world lines.

Grouping is now a derived read over ?group=: absent means the broadest
level the scope offers, and a level the scope cannot serve is filtered out
rather than requested. That makes the corrective Effect unreachable, so it
is deleted along with its write-back loop."
```

---

### Task 7: Persist mode, HQ, and the time window

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/item_view.rs` (~:1240, ~:1263, ~:1271, ~:1439)
- Modify: `ultros-frontend/ultros-app/src/components/price_history_chart.rs` (`set_mode` prop ~:719)
- Modify: `ultros-frontend/ultros-app/src/components/chart_toolbar.rs` (`set_mode` prop ~:51)

**Interfaces:**
- Consumes: `ChartMode: FromStr + Display` (Task 1); `resolve_range`, `RangePreset` (Task 2).
- Produces: `?mode=`, `?hq=`, `?range=`, `?from=`/`?to=` params; `set_mode: SignalSetter<ChartMode>`; `range_preset: Signal<Option<RangePreset>>` and `set_range: Callback<Option<(i64, i64)>>` available to pass into the chart in Task 8.

- [ ] **Step 1: Widen the `set_mode` props**

In `chart_toolbar.rs`, **replace** the `set_mode` prop (~:51):

```rust
    #[prop(into)] set_mode: SignalSetter<ChartMode>,
```

In `price_history_chart.rs`, **replace** the `set_mode` prop (~:719):

```rust
    #[prop(into)] set_mode: SignalSetter<ChartMode>,
```

- [ ] **Step 2: Replace the `mode` and `hq_only` signals with URL reads**

In `item_view.rs`, add to the imports:

```rust
use crate::components::chart_query::{RangePreset, resolve_range};
```

**Replace** `let (hq_only, set_hq_only) = signal(false);` (~:1240) with:

```rust
    // `?hq=true`, absent means off. Only written when true, so the default
    // never appears in the URL.
    let (hq_param, set_hq_param) = filter_query_signal::<bool>("hq");
    let hq_only = Signal::derive(move || hq_param.get().unwrap_or(false));
    let set_hq_only = SignalSetter::map(move |on: bool| {
        set_hq_param.set(on.then_some(true));
    });
```

**Replace** `let (mode, set_mode) = signal(ChartMode::Price);` (~:1263) with:

```rust
    // `?mode=`, absent means Price. Deriving rather than seeding keeps the
    // URL clean until the user actually picks a mode, and means a shared
    // link and a fresh visit agree on what the chart shows. Mode switches
    // never touch the time window or grouping -- spec: "switching mode
    // preserves the time window and grouping".
    let (mode_param, set_mode_param) = filter_query_signal::<ChartMode>("mode");
    let mode = Signal::derive(move || mode_param.get().unwrap_or_default());
    let set_mode = SignalSetter::map(move |next: ChartMode| {
        set_mode_param.set(Some(next));
    });
```

Note `hq_only` and `mode` change from `ReadSignal`/`WriteSignal` pairs to `Signal`/`SignalSetter`. Fix any resulting call sites the compiler flags — `hq_only.get()` still works; `set_hq_only.set(v)` still works.

- [ ] **Step 3: Replace the `selected_range` signal with URL reads**

**Replace** the `selected_range` block (~:1271-1281) with:

```rust
    // The time window has two URL shapes. A preset click writes `?range=1mo`,
    // so the link keeps meaning "the last month" indefinitely; a slicer drag
    // has no relative meaning, so it writes absolute `?from=&to=` epoch
    // seconds. `resolve_range` applies the precedence.
    let (range_param, set_range_param) = filter_query_signal::<RangePreset>("range");
    let (from_param, set_from_param) = filter_query_signal::<i64>("from");
    let (to_param, set_to_param) = filter_query_signal::<i64>("to");

    // Resolved once per mount rather than continuously: a chart does not
    // need to slide in real time, and re-resolving on every tick would
    // refetch. Client-only, so no SSR/CSR divergence -- the slicer that
    // consumes this only renders once `series` (a LocalResource) resolves.
    let now = StoredValue::new(chrono::Utc::now().timestamp());
    let selected_range = Signal::derive(move || {
        let from_to = from_param.get().zip(to_param.get());
        resolve_range(range_param.get(), from_to, now.get_value())
    });

    // A drag commits absolute bounds and clears any preset; "All" clears
    // everything. Writing all three together keeps the two shapes from
    // coexisting in one URL.
    let set_selected_range = Callback::new(move |next: Option<(i64, i64)>| {
        set_range_param.set(None);
        match next {
            Some((from, to)) => {
                set_from_param.set(Some(from));
                set_to_param.set(Some(to));
            }
            None => {
                set_from_param.set(None);
                set_to_param.set(None);
            }
        }
    });

    // Selecting a preset clears the absolute bounds for the same reason.
    let set_range_preset = Callback::new(move |preset: Option<RangePreset>| {
        set_from_param.set(None);
        set_to_param.set(None);
        set_range_param.set(preset);
    });

    // A different item/world makes any absolute-timestamp selection from the
    // previous item meaningless (and possibly outside the new item's data
    // entirely) -- drop back to full range before the next request goes out.
    // Deliberately does *not* track `group`/`hq`: changing those shouldn't
    // discard an in-progress zoom.
    Effect::new(move |_| {
        item_id.track();
        world.track();
        set_from_param.set(None);
        set_to_param.set(None);
        set_range_param.set(None);
    });
```

- [ ] **Step 4: Make the chart read the window instead of owning it**

**This step is load-bearing — skipping it makes shared links erase themselves.**
`PriceHistoryChart` currently owns `let (selected_range, set_selected_range) = signal(None)`
and mirrors it out through `on_range_change`. If it keeps that local signal
now that the URL is the source of truth, then loading a `?from=&to=` link
goes: item_view resolves the window and fetches correctly → the chart's local
signal is still `None` → the mirror effect fires `on_range_change(None)` →
`from`/`to` are cleared. The link destroys itself on load.

So the window becomes a prop. In `price_history_chart.rs`, add to the
`PriceHistoryChart` signature (~:722, next to `on_range_change`):

```rust
    /// The committed time window, owned by the route and backed by the URL.
    /// The chart renders and requests changes to it but does not own it —
    /// otherwise a link's window would be overwritten by the local default
    /// on mount.
    #[prop(into)]
    selected_range: Signal<Option<(i64, i64)>>,
```

**Delete** the local signal (~:746) and its doc comment:

```rust
    let (selected_range, set_selected_range) = signal::<Option<(i64, i64)>>(None);
```

...replacing it with a setter that writes straight through to the caller:

```rust
    // Every commit goes to the caller, which persists it to the URL and
    // debounces it into a refetch. Undebounced here so the slicer handles
    // track the pointer at full rate.
    let set_selected_range = Callback::new(move |next: Option<(i64, i64)>| {
        on_range_change.run(next);
    });
```

**Delete** the now-redundant mirror effect (~:816-819):

```rust
    // Mirror every commit to the caller so it can (debounced) refetch.
    Effect::new(move |_| {
        on_range_change.run(selected_range.get());
    });
```

In the `range_is_stale` effect (~:804-815), **replace** `set_selected_range.set(None);` with:

```rust
            set_selected_range.run(None);
```

- [ ] **Step 5: Convert `TimelineSlicer`'s setter to a `Callback`**

**Replace** the `set_selected_range` prop in the `TimelineSlicer` signature (~:341):

```rust
    #[prop(into)] set_selected_range: Callback<Option<(i64, i64)>>,
```

Then **replace** its three call sites inside `TimelineSlicer` — `.set(x)` becomes `.run(x)`:

- in `update_drag` (~:413): `set_selected_range.run(Some(next));`
- in the track's `on:pointerdown` (~:473): `set_selected_range.run(Some(normalize_time_range(ts, ts, domain)));`
- in the full-range button's `on:click` (~:449): `set_selected_range.run(None);`

- [ ] **Step 6: Update the chart call site**

**Replace** the `on_range_change` prop at the `<PriceHistoryChart>` call site (~:1443), adding the window prop next to it:

```rust
                                    selected_range=selected_range
                                    on_range_change=set_selected_range
```

- [ ] **Step 7: Verify it compiles and tests pass**

```bash
cargo check -p ultros-app --lib && cargo test -p ultros-app --lib
```
Expected: clean, all tests PASS.

- [ ] **Step 8: Run CI checks and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

```bash
git add ultros-frontend/ultros-app/src/routes/item_view.rs ultros-frontend/ultros-app/src/components/price_history_chart.rs ultros-frontend/ultros-app/src/components/chart_toolbar.rs
git commit -m "feat(chart): persist mode, HQ filter and time window to the URL

Adds ?mode=, ?hq=, and the two time-window shapes: ?range= for a preset
(so a shared link keeps meaning 'the last month') and ?from=/?to= for a
slicer drag, which has no relative meaning. Setting either shape clears
the other so they never coexist. Absent params mean the default, so URLs
stay clean until something is actually changed."
```

---

### Task 8: Quick-range buttons

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/price_history_chart.rs` (`TimelineSlicer` ~:335, its header ~:436-453)
- Modify: `ultros-frontend/ultros-app/src/routes/item_view.rs` (chart call site ~:1435)
- Modify: all seven of `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json`

**Interfaces:**
- Consumes: `RangePreset`, `preset_has_data` (Task 2); `set_range_preset`, `range_param` (Task 7).
- Produces: `range_preset` and `set_range_preset` props on `TimelineSlicer` and `PriceHistoryChart`.

- [ ] **Step 1: Add the five i18n keys to all seven locales**

Insert next to the existing `chart_timeline_full_range` key in each file (`en.json:1002`, the others at `:999`). The existing `chart_timeline_full_range` key stays — it is no longer rendered, but removing it from seven files is churn with no benefit.

`en.json`:
```json
    "chart_range_7d": "7d",
    "chart_range_1mo": "1mo",
    "chart_range_1y": "1y",
    "chart_range_all": "All",
    "chart_range_unavailable": "No sales in this period",
```

`fr.json`:
```json
    "chart_range_7d": "7 j",
    "chart_range_1mo": "1 mois",
    "chart_range_1y": "1 an",
    "chart_range_all": "Tout",
    "chart_range_unavailable": "Aucune vente sur cette période",
```

`de.json`:
```json
    "chart_range_7d": "7 T",
    "chart_range_1mo": "1 Mon",
    "chart_range_1y": "1 J",
    "chart_range_all": "Alle",
    "chart_range_unavailable": "Keine Verkäufe in diesem Zeitraum",
```

`ja.json`:
```json
    "chart_range_7d": "7日",
    "chart_range_1mo": "1か月",
    "chart_range_1y": "1年",
    "chart_range_all": "全期間",
    "chart_range_unavailable": "この期間に販売はありません",
```

`cn.json`:
```json
    "chart_range_7d": "7天",
    "chart_range_1mo": "1个月",
    "chart_range_1y": "1年",
    "chart_range_all": "全部",
    "chart_range_unavailable": "此期间没有销售记录",
```

`ko.json`:
```json
    "chart_range_7d": "7일",
    "chart_range_1mo": "1개월",
    "chart_range_1y": "1년",
    "chart_range_all": "전체",
    "chart_range_unavailable": "이 기간에 판매가 없습니다",
```

`tc.json`:
```json
    "chart_range_7d": "7天",
    "chart_range_1mo": "1個月",
    "chart_range_1y": "1年",
    "chart_range_all": "全部",
    "chart_range_unavailable": "此期間沒有銷售紀錄",
```

- [ ] **Step 2: Add the props to `TimelineSlicer`**

In `price_history_chart.rs`, **replace** the `TimelineSlicer` signature (~:335-342):

```rust
#[component]
fn TimelineSlicer(
    #[prop(into)] series: Signal<PriceSeries>,
    #[prop(into)] available_domain: Signal<Option<(i64, i64)>>,
    #[prop(into)] selected_domain: Signal<Option<(i64, i64)>>,
    #[prop(into)] selected_range: Signal<Option<(i64, i64)>>,
    #[prop(into)] utc_offset_minutes: Signal<i32>,
    // Converted to a Callback in Task 7 — the window is owned by the route
    // and backed by the URL, not by this component.
    #[prop(into)] set_selected_range: Callback<Option<(i64, i64)>>,
    /// The active quick-range preset, read straight off `?range=` so the
    /// pressed button is exact rather than inferred from the window.
    #[prop(into)]
    range_preset: Signal<Option<RangePreset>>,
    #[prop(into)] set_range_preset: Callback<Option<RangePreset>>,
) -> impl IntoView {
```

Add to the imports at the top of the file:

```rust
use crate::components::chart_query::{RangePreset, preset_has_data};
```

- [ ] **Step 3: Replace the "Full range" button with the preset row**

**Replace** the `<button>` for `chart_timeline_full_range` (~:445-452) with:

```rust
                    <div
                        role="group"
                        aria-label=move || t_string!(i18n, chart_timeline_label).to_string()
                        class="inline-flex shrink-0 overflow-hidden rounded-md border border-[color:var(--color-outline)]"
                    >
                        {RangePreset::ALL
                            .into_iter()
                            .map(|preset| {
                                let label = move || match preset {
                                    RangePreset::Week => {
                                        t_string!(i18n, chart_range_7d).to_string()
                                    }
                                    RangePreset::Month => {
                                        t_string!(i18n, chart_range_1mo).to_string()
                                    }
                                    RangePreset::Year => {
                                        t_string!(i18n, chart_range_1y).to_string()
                                    }
                                };
                                // A window ending before the item's newest
                                // sale would blank the chart; disable with a
                                // reason rather than rendering nothing.
                                let disabled = Signal::derive(move || {
                                    let now = chrono::Utc::now().timestamp();
                                    available_domain
                                        .get()
                                        .is_some_and(|(_, end)| {
                                            !preset_has_data(preset, end, now)
                                        })
                                });
                                view! {
                                    <button
                                        type="button"
                                        aria-pressed=move || {
                                            (range_preset.get() == Some(preset)).to_string()
                                        }
                                        prop:disabled=disabled
                                        title=move || {
                                            if disabled.get() {
                                                t_string!(i18n, chart_range_unavailable).to_string()
                                            } else {
                                                String::new()
                                            }
                                        }
                                        class=move || {
                                            let active = range_preset.get() == Some(preset);
                                            [
                                                "border-l border-[color:var(--color-outline)] px-2.5 py-1 text-xs transition-colors first:border-l-0 disabled:cursor-not-allowed disabled:opacity-45",
                                                if active {
                                                    "bg-brand-600/30 text-brand-100"
                                                } else {
                                                    "bg-[color:color-mix(in_srgb,_var(--color-text)_4%,_transparent)] text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]"
                                                },
                                            ]
                                                .join(" ")
                                        }
                                        on:click=move |_| set_range_preset.run(Some(preset))
                                    >
                                        {label}
                                    </button>
                                }
                            })
                            .collect_view()}
                        <button
                            type="button"
                            aria-pressed=move || {
                                (range_preset.get().is_none() && selected_range.get().is_none())
                                    .to_string()
                            }
                            class=move || {
                                let active = range_preset.get().is_none()
                                    && selected_range.get().is_none();
                                [
                                    "border-l border-[color:var(--color-outline)] px-2.5 py-1 text-xs transition-colors",
                                    if active {
                                        "bg-brand-600/30 text-brand-100"
                                    } else {
                                        "bg-[color:color-mix(in_srgb,_var(--color-text)_4%,_transparent)] text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]"
                                    },
                                ]
                                    .join(" ")
                            }
                            on:click=move |_| {
                                set_range_preset.run(None);
                                set_selected_range.run(None);
                            }
                        >
                            {move || t_string!(i18n, chart_range_all).to_string()}
                        </button>
                    </div>
```

- [ ] **Step 4: Thread the props through `PriceHistoryChart`**

Add to the `PriceHistoryChart` signature (~:722, before `on_range_change`):

```rust
    #[prop(into)] range_preset: Signal<Option<RangePreset>>,
    #[prop(into)] set_range_preset: Callback<Option<RangePreset>>,
```

**Replace** the `<TimelineSlicer .../>` call (~:1256-1263):

```rust
            <TimelineSlicer
                series=resolved_series
                available_domain=available_domain
                selected_domain=selected_domain
                selected_range=selected_range
                utc_offset_minutes=utc_offset
                set_selected_range=set_selected_range
                range_preset=range_preset
                set_range_preset=set_range_preset
            />
```

In `item_view.rs`, add the two props to the `<PriceHistoryChart>` call site (~:1443):

```rust
                                    range_preset=range_param
                                    set_range_preset=set_range_preset
```

- [ ] **Step 5: Verify it compiles and tests pass**

```bash
cargo check -p ultros-app --lib && cargo test -p ultros-app --lib
```
Expected: clean, all tests PASS. A locale key missing from any of the seven files fails the build here.

- [ ] **Step 6: Run CI checks and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

```bash
git add ultros-frontend/ultros-app/src/components/price_history_chart.rs ultros-frontend/ultros-app/src/routes/item_view.rs ultros-frontend/ultros-app/locales/
git commit -m "feat(chart): 7d/1mo/1y/All quick-range buttons

Replaces the lone 'Full range' button with a segmented row. Presets are
anchored to now, so a shared ?range=7d link means the same thing to every
viewer, and a preset whose window predates the item's newest sale is
disabled with a reason instead of blanking the chart. The pressed state is
read off ?range= rather than inferred from the window, so a dragged
30-day window is not mistaken for the 1mo preset."
```

---

### Task 9: Persist the chart's own presentation params

The chart reads these directly rather than taking them as props — otherwise its signature grows from 10 props to ~20.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/price_history_chart.rs` (state block ~:727-749)

**Interfaces:**
- Consumes: `Overlays` (Task 4), `parse_show`/`encode_show` (Task 3), `filter_query_signal`.
- Produces: `?view=`, `?overlays=`, `?show=`, `?sort=`, `?cellscale=` params. No new public API.

- [ ] **Step 1: Add wire formats for `ChartView` and `GridSort`**

In `chart_toolbar.rs`, add after the `ChartView` enum (~:27):

```rust
/// Wire format for the `?view=` URL param.
impl std::fmt::Display for ChartView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Overlay => "overlay",
            Self::Grid => "grid",
        })
    }
}

impl std::str::FromStr for ChartView {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "overlay" => Ok(Self::Overlay),
            "grid" => Ok(Self::Grid),
            _ => Err(()),
        }
    }
}
```

`GridSort` lives in `ultros-charts`. Add the same pair to `ultros-frontend/ultros-charts/src/charts/grid.rs`, next to the `GridSort` enum, with wire values `name` and `change`.

- [ ] **Step 2: Replace the presentation signals with URL reads**

In `price_history_chart.rs`, **replace** the block of `signal(...)` declarations (~:727-749) — keeping `selected_range` and `hidden_series` handling as described below:

```rust
    // Presentation params are read here rather than passed down: the chart
    // owns them, and threading five more through the route would take this
    // component past twenty props. Nothing is seeded, so a Suspense remount
    // is harmless -- reads and writes are idempotent against the URL.
    let (overlays_param, set_overlays_param) = filter_query_signal::<Overlays>("overlays");
    let overlays = Signal::derive(move || overlays_param.get().unwrap_or_default());
    let update_overlays = move |f: fn(&mut Overlays, bool), on: bool| {
        let mut next = overlays.get_untracked();
        f(&mut next, on);
        set_overlays_param.set(Some(next));
    };

    let show_market_average = Signal::derive(move || overlays.get().market_average);
    let set_show_market_average =
        SignalSetter::map(move |on| update_overlays(|o, v| o.market_average = v, on));
    let show_trend = Signal::derive(move || overlays.get().trend);
    let set_show_trend = SignalSetter::map(move |on| update_overlays(|o, v| o.trend = v, on));
    let show_quantity = Signal::derive(move || overlays.get().quantity);
    let set_show_quantity = SignalSetter::map(move |on| update_overlays(|o, v| o.quantity = v, on));
    let percent_change = Signal::derive(move || overlays.get().percent_change);
    let set_percent_change =
        SignalSetter::map(move |on| update_overlays(|o, v| o.percent_change = v, on));
    let show_patches = Signal::derive(move || overlays.get().patches);
    let set_show_patches = SignalSetter::map(move |on| update_overlays(|o, v| o.patches = v, on));

    let (view_param, set_view_param) = filter_query_signal::<ChartView>("view");
    let view = Signal::derive(move || view_param.get().unwrap_or_default());
    let set_view = SignalSetter::map(move |next: ChartView| set_view_param.set(Some(next)));

    let (sort_param, set_sort_param) = filter_query_signal::<GridSort>("sort");
    let grid_sort = Signal::derive(move || sort_param.get().unwrap_or(GridSort::Name));
    let set_grid_sort = SignalSetter::map(move |next: GridSort| set_sort_param.set(Some(next)));

    let (cellscale_param, set_cellscale_param) = filter_query_signal::<bool>("cellscale");
    let grid_per_cell_scale = Signal::derive(move || cellscale_param.get().unwrap_or(false));
    let set_grid_per_cell_scale =
        SignalSetter::map(move |on: bool| set_cellscale_param.set(on.then_some(true)));

    // Lifted so the grid's "+N more" affordance can open the toolbar's
    // world-filter popover.
    let world_filter_open = RwSignal::new(false);
```

Add to the imports:

```rust
use crate::components::chart_query::{Overlays, encode_show, parse_show};
use crate::query_defaults::filter_query_signal;
use ultros_charts::charts::grid::GridSort;
```

(`GridSort` may already be imported via the existing `grid::{...}` line — extend that instead of duplicating.)

- [ ] **Step 3: Back `hidden_series` with the `show` param**

`hidden_series` is an `RwSignal<Vec<String>>` written by both the legend and the filter popover. Keep that interface so neither call site changes, and sync it to the URL at the edges. **Replace** `let hidden_series = RwSignal::new(Vec::<String>::new());` with:

```rust
    // The series names currently on the chart, in model order. `show` is
    // resolved against these: an expression naming series that don't exist at
    // this grouping level is stale, and `parse_show` fails it open rather
    // than blanking the chart.
    let (show_param, set_show_param) = filter_query_signal::<String>("show");
    let hidden_series = RwSignal::new(Vec::<String>::new());
```

Immediately **after** `model` is declared (it is the source of series names), add:

```rust
    let series_names =
        Memo::new(move |_| model.with(|m| m.series.iter().map(|s| s.name.clone()).collect::<Vec<_>>()));

    // URL -> state. Runs whenever the expression or the series set changes,
    // which is what re-resolves a `show` written at a different grouping
    // level.
    Effect::new(move |_| {
        let names = series_names.get();
        let next = show_param
            .get()
            .map(|expr| parse_show(&expr, &names))
            .unwrap_or_default();
        if hidden_series.get_untracked() != next {
            hidden_series.set(next);
        }
    });

    // State -> URL. Guarded on inequality so this and the effect above
    // cannot drive each other in a loop.
    //
    // The apparent cycle (hidden_series -> model -> series_names -> effect ->
    // hidden_series) is broken by two things: `build_price_history_chart`
    // keeps hidden series in `m.series` with `hidden: true` rather than
    // dropping them, so `series_names` does not change when something is
    // hidden and the Memo's PartialEq halts propagation; and both effects
    // no-op when the value already matches.
    Effect::new(move |_| {
        let hidden = hidden_series.get();
        let names = series_names.get_untracked();
        if names.is_empty() {
            return;
        }
        let next = encode_show(&hidden, &names);
        if show_param.get_untracked() != next {
            set_show_param.set(next);
        }
    });
```

- [ ] **Step 4: Verify it compiles and tests pass**

```bash
cargo check -p ultros-app --lib && cargo test -p ultros-app --lib
```
Expected: clean, all tests PASS. The compiler will flag any remaining `set_*.set(...)` call whose type changed; `SignalSetter` supports `.set()`, so most should be unaffected.

- [ ] **Step 5: Run CI checks and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

```bash
git add ultros-frontend/ultros-app/src/components/price_history_chart.rs ultros-frontend/ultros-app/src/components/chart_toolbar.rs ultros-frontend/ultros-charts/src/charts/grid.rs
git commit -m "feat(chart): persist view, overlays, visible series and grid options

Adds ?view=, ?overlays=, ?show=, ?sort= and ?cellscale=. The chart reads
these itself rather than taking them as props, which would have pushed its
signature past twenty. hidden_series keeps its RwSignal interface so the
legend and filter popover are unchanged; two guarded effects sync it to
?show= at the edges, re-resolving the expression whenever the series set
changes under it."
```

---

## Verification

- [ ] **Full test suite**

```bash
cargo test -p ultros-app --lib && cargo test -p ultros-charts --lib
```
Expected: PASS.

- [ ] **Full CI gate**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```
Expected: `REAL_EXIT=0`.

- [ ] **Manual check of the two behaviours unit tests cannot reach**

On a **region** item page (e.g. `/item/North-America/44096`):
1. The chart opens grouped by **Region**, not World — three lines, not seventy.
2. The timeline label reads `2023-02 – 2026-07`, not `02-21 18:00 - 07-05 18:00`.
3. Clicking `1mo` narrows the chart and puts `range=1mo` in the URL.
4. Reloading that URL restores the same chart.

**Known verification gap — state this in the PR, do not claim otherwise.** On a local **debug** build every `query_signal` URL *write* is inert while reads still work, so steps 3 and 4 above cannot be confirmed locally in debug. The parse/encode layer is fully covered by unit tests; the round trip needs a release build or prod. If a release build is not run, say so explicitly in the PR description.

Also note that `check_ci.sh` never lints `#[cfg(feature = "hydrate")]` blocks, so the `on_click_outside` call in `price_history_chart.rs` remains unlinted by CI — do not add hydrate-gated code in these tasks without compiling with `--features hydrate` locally.
