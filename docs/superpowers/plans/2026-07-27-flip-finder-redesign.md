# Flip Finder Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild `/flip-finder/:world` as a window-scrolled spreadsheet with a 76px sticky control bar, metrics derived from data present on every row, and localStorage saved views.

**Architecture:** Pure metric functions land in `analysis.rs` first (fully unit-testable, no DOM). `VirtualScroller` gains a `ScrollSource` mode so the list can virtualize against window scroll while keeping its existing container mode untouched for 7 other call sites. The route then rebuilds its chrome on top of both.

**Tech Stack:** Rust, Leptos 0.8 (SSR + hydration), leptos-i18n, leptos-use (localStorage), Tailwind v4.

**Spec:** `docs/superpowers/specs/2026-07-27-flip-finder-redesign-design.md`

## Global Constraints

- **No hardcoded user-facing strings.** Every user-facing string goes through `t!(i18n, key)` or `t_string!(i18n, key)`. New keys land in **all 7** locale files: `en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc` (`ultros-frontend/ultros-app/locales/`). Real translations, not English stubs. `snake_case`, prefixed `analyzer_`.
- **Run `./check_ci.sh` before every commit** (`cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`). If the submodule is not initialized, at minimum run `cargo fmt --all -- --check` and note it.
- **CI does not run `cargo test`** (commented out in `rust.yml`). Tests must be run locally. Green CI proves compilation only.
- `ultros-app` lib tests link on Windows/MSVC. The leptos linker wall applies to the `ultros` binary only — use `cargo test -p ultros-app` freely.
- **Any HashMap iteration whose order reaches the DOM is an SSR/CSR hydration bug.** Sort before rendering.
- Edition 2024: `gen` is a reserved keyword.

---

### Task 1: Derived metrics and the ROI overflow fix

Pure functions, no DOM, no reactivity. Everything here is unit-testable and is the
foundation the columns in Task 4 render.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analysis.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs:500` (ROI call site)
- Test: `ultros-frontend/ultros-app/src/analysis.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: `SaleSummary` (already in `analysis.rs`), with fields `num_sold: usize`, `avg_sale_duration: Option<Duration>`, `days_since_last_sale: Option<Duration>`.
- Produces:
  - `pub fn velocity_per_day(summary: &SaleSummary) -> Option<f32>`
  - `pub fn price_drift_pct(prices: &[i32]) -> Option<f32>`
  - `pub fn return_on_investment(profit: i32, cheapest_price: i32) -> i32`
  - `pub enum DerivedConfidence { High, Medium, Low }` (derives `Debug, Clone, Copy, PartialEq, Eq`)
  - `pub fn derived_confidence(summary: &SaleSummary) -> DerivedConfidence`
  - `pub const ROI_DISPLAY_CEILING: i32 = 100_000;`
  - `pub const MIN_VELOCITY_SPAN_DAYS: f32 = 1.0 / 24.0;`

- [ ] **Step 1: Write the failing tests**

Append to the existing `mod tests` at the bottom of `ultros-frontend/ultros-app/src/analysis.rs`. If no `mod tests` exists, create one at the end of the file with `use super::*;`.

```rust
    fn summary_with(num_sold: usize, avg_secs: i64) -> SaleSummary {
        SaleSummary {
            item_id: 1,
            hq: false,
            num_sold,
            avg_sale_duration: Some(Duration::seconds(avg_secs)),
            days_since_last_sale: Some(Duration::hours(1)),
            max_price: 0,
            avg_price: 0,
            median_price: 0,
            min_price: 0,
        }
    }

    #[test]
    fn velocity_full_buffer_over_three_days() {
        // 6 sales spanning 3 days => avg gap = 3d/6 = 12h => 2 sales/day.
        let s = summary_with(6, 12 * 3600);
        let v = velocity_per_day(&s).unwrap();
        assert!((v - 2.0).abs() < 0.001, "expected 2.0, got {v}");
    }

    #[test]
    fn velocity_partial_buffer() {
        // 2 sales spanning 4 days => avg gap = 2 days => 0.5 sales/day.
        let s = summary_with(2, 2 * 86_400);
        let v = velocity_per_day(&s).unwrap();
        assert!((v - 0.5).abs() < 0.001, "expected 0.5, got {v}");
    }

    #[test]
    fn velocity_clamps_zero_span() {
        // Observed in prod: 6 sales sharing one timestamp (one buyer clearing
        // six listings). Span 0 must not divide by zero or return infinity.
        let s = summary_with(6, 0);
        let v = velocity_per_day(&s).unwrap();
        assert!(v.is_finite(), "velocity must stay finite, got {v}");
        assert!((v - 6.0 / MIN_VELOCITY_SPAN_DAYS).abs() < 0.001);
    }

    #[test]
    fn velocity_decade_old_buffer_is_near_zero() {
        // Observed max span: 94,041 hours. 6 sales over ~10.7 years.
        let s = summary_with(6, 94_041 * 3600 / 6);
        let v = velocity_per_day(&s).unwrap();
        assert!(v < 0.01, "expected near-zero velocity, got {v}");
    }

    #[test]
    fn velocity_none_when_no_sales() {
        let mut s = summary_with(0, 0);
        s.avg_sale_duration = None;
        assert_eq!(velocity_per_day(&s), None);
    }

    #[test]
    fn drift_detects_rising_price() {
        // newest-first: newest 3 mean 200, oldest 3 mean 100 => +100%.
        let prices = [200, 200, 200, 100, 100, 100];
        let d = price_drift_pct(&prices).unwrap();
        assert!((d - 100.0).abs() < 0.01, "expected +100.0, got {d}");
    }

    #[test]
    fn drift_detects_falling_price() {
        let prices = [50, 50, 50, 100, 100, 100];
        let d = price_drift_pct(&prices).unwrap();
        assert!((d + 50.0).abs() < 0.01, "expected -50.0, got {d}");
    }

    #[test]
    fn drift_flat_is_zero() {
        let prices = [100, 100, 100, 100, 100, 100];
        assert!(price_drift_pct(&prices).unwrap().abs() < 0.01);
    }

    #[test]
    fn drift_none_below_four_samples() {
        assert_eq!(price_drift_pct(&[100, 100, 100]), None);
        assert_eq!(price_drift_pct(&[100]), None);
        assert_eq!(price_drift_pct(&[]), None);
    }

    #[test]
    fn drift_with_five_samples_skips_the_middle() {
        // len 5 => take 2 from each end, index 2 ignored.
        let prices = [200, 200, 999_999, 100, 100];
        let d = price_drift_pct(&prices).unwrap();
        assert!((d - 100.0).abs() < 0.01, "expected +100.0, got {d}");
    }

    #[test]
    fn roi_does_not_saturate_at_i32_max() {
        // The prod bug: buy 2 gil, profit 213,749,998 previously produced
        // i32::MAX (2147483647) via an f32 -> i32 saturating cast.
        let roi = return_on_investment(213_749_998, 2);
        assert_eq!(roi, ROI_DISPLAY_CEILING);
        assert_ne!(roi, i32::MAX);
    }

    #[test]
    fn roi_normal_range_is_exact() {
        assert_eq!(return_on_investment(50, 100), 50);
        assert_eq!(return_on_investment(300, 100), 300);
    }

    #[test]
    fn roi_zero_price_is_zero() {
        assert_eq!(return_on_investment(1000, 0), 0);
        assert_eq!(return_on_investment(1000, -5), 0);
    }

    #[test]
    fn roi_negative_profit_is_negative() {
        assert_eq!(return_on_investment(-50, 100), -50);
    }

    #[test]
    fn confidence_bands_track_buffer_and_velocity() {
        // Full buffer + brisk velocity (6 sales over 3 days = 2/day).
        assert_eq!(derived_confidence(&summary_with(6, 12 * 3600)), DerivedConfidence::High);
        // Mid buffer.
        assert_eq!(derived_confidence(&summary_with(4, 86_400)), DerivedConfidence::Medium);
        // Thin buffer.
        assert_eq!(derived_confidence(&summary_with(1, 86_400)), DerivedConfidence::Low);
        // Full buffer but glacial (6 sales over ~10 years) is not High.
        assert_eq!(
            derived_confidence(&summary_with(6, 94_041 * 3600 / 6)),
            DerivedConfidence::Low
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p ultros-app --lib analysis
```

Expected: FAIL — `cannot find function velocity_per_day in this scope` and similar for each new symbol.

- [ ] **Step 3: Write the implementation**

Add to `ultros-frontend/ultros-app/src/analysis.rs`, above the existing `mod tests`:

```rust
/// Minimum span used as the velocity denominator. Guards the degenerate
/// case observed in prod of six sales sharing one timestamp (one buyer
/// clearing six listings at once), which would otherwise divide by zero.
pub const MIN_VELOCITY_SPAN_DAYS: f32 = 1.0 / 24.0;

/// Display ceiling for ROI. Beyond this the exact figure carries no
/// decision value, and the previous `as i32` cast saturated at `i32::MAX`
/// for tiny buy prices (a 2-gil buy against a laundered sale price).
pub const ROI_DISPLAY_CEILING: i32 = 100_000;

/// Recent sales per day, derived from the bounded `RecentSales` buffer.
///
/// `avg_sale_duration` is `(now - oldest_sale) / num_sold`, so the total
/// span is `avg * num_sold` and velocity is `num_sold / span`. Because the
/// buffer holds the *most recent* sales, this estimates the current rate
/// rather than a lifetime average; resolution degrades only at the high
/// end, which does not matter for a floor-style filter.
pub fn velocity_per_day(summary: &SaleSummary) -> Option<f32> {
    if summary.num_sold == 0 {
        return None;
    }
    let avg = summary.avg_sale_duration?;
    let span_days = (avg.num_seconds() as f32 * summary.num_sold as f32) / 86_400.0;
    Some(summary.num_sold as f32 / span_days.max(MIN_VELOCITY_SPAN_DAYS))
}

/// Percent change between the mean of the newest samples and the mean of
/// the oldest samples. `prices` is newest-first, matching the wire order
/// of `RecentSales`.
///
/// Returns `None` below 4 samples — a two-point "trend" is noise wearing a
/// percentage sign. With an odd count the middle sample is skipped so the
/// two windows never overlap.
pub fn price_drift_pct(prices: &[i32]) -> Option<f32> {
    if prices.len() < 4 {
        return None;
    }
    let take = 3.min(prices.len() / 2);
    let newest: i64 = prices[..take].iter().map(|p| *p as i64).sum();
    let oldest: i64 = prices[prices.len() - take..].iter().map(|p| *p as i64).sum();
    if oldest == 0 {
        return None;
    }
    Some(((newest - oldest) as f32 / oldest as f32) * 100.0)
}

/// Return on investment as a percentage, computed in f64 and clamped to
/// [`ROI_DISPLAY_CEILING`].
pub fn return_on_investment(profit: i32, cheapest_price: i32) -> i32 {
    if cheapest_price <= 0 {
        return 0;
    }
    let roi = (profit as f64 / cheapest_price as f64) * 100.0;
    roi.clamp(-(ROI_DISPLAY_CEILING as f64), ROI_DISPLAY_CEILING as f64) as i32
}

/// Trustworthiness of a row's numbers when ClickHouse has no rollup for it.
/// Replaces the page-level disclaimer copy with a per-row statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerivedConfidence {
    High,
    Medium,
    Low,
}

/// Band a row from its buffer depth and observed velocity. A full buffer
/// only earns `High` if the sales are actually recent — six sales spread
/// over a decade is a dead item, not a confident one.
pub fn derived_confidence(summary: &SaleSummary) -> DerivedConfidence {
    let velocity = velocity_per_day(summary).unwrap_or(0.0);
    if summary.num_sold >= 6 && velocity >= 1.0 {
        DerivedConfidence::High
    } else if summary.num_sold >= 4 && velocity >= 0.2 {
        DerivedConfidence::Medium
    } else {
        DerivedConfidence::Low
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p ultros-app --lib analysis
```

Expected: PASS, 15 new tests.

- [ ] **Step 5: Route the ROI call site through the new function**

In `ultros-frontend/ultros-app/src/routes/analyzer.rs`, inside the `sorted_data` memo, replace:

```rust
                let return_on_investment = if data.cheapest_price > 0 {
                    ((profit as f32 / data.cheapest_price as f32) * 100.0) as i32
                } else {
                    0
                };
```

with:

```rust
                let return_on_investment =
                    crate::analysis::return_on_investment(profit, data.cheapest_price);
```

Add `return_on_investment` to the existing `use crate::analysis::{...}` import at the top of the file (which currently imports `SaleSummary, roi_badge_class`).

- [ ] **Step 6: Verify the whole suite and lints still pass**

```bash
cargo test -p ultros-app --lib
```

Expected: PASS, including the pre-existing `analyzer.rs` tests (`median_price_is_middle_of_clamped_sales`, `troll_region_floor_drops_row_entirely`, `visible_keys_*`, etc.) with no regressions.

```bash
./check_ci.sh
```

Expected: clean. If the submodule blocks clippy, run `cargo fmt --all -- --check` at minimum and note it.

- [ ] **Step 7: Commit**

```bash
git add ultros-frontend/ultros-app/src/analysis.rs ultros-frontend/ultros-app/src/routes/analyzer.rs
git commit -m "feat(analyzer): derived velocity/drift/confidence metrics, fix ROI i32 saturation"
```

---

### Task 2: `ScrollSource` mode on `VirtualScroller`

Adds window-scroll virtualization without disturbing the 7 existing call sites.
This task ships independently so a hydration regression has an obvious culprit.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/virtual_scroller.rs`
- Test: `ultros-frontend/ultros-app/src/components/virtual_scroller.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: nothing from Task 1.
- Produces:
  - `pub enum ScrollSource { Container { viewport_height: f64 }, Window { sticky_offset: f64 } }` (derives `Debug, Clone, Copy, PartialEq`)
  - New optional prop on `VirtualScroller`: `#[prop(optional)] scroll_source: Option<ScrollSource>`
  - `pub const SSR_FALLBACK_ROWS: usize = 20;`
  - `fn effective_viewport_for(source: ScrollSource, measured_window_height: f64) -> f64`

The existing `viewport_height: f64` prop is **retained and unchanged**. When
`scroll_source` is `None`, behavior is byte-identical to today. All 7 other call
sites (`search_box.rs`, `fc_crafting_analyzer.rs`, `leve_analyzer.rs`,
`recipe_analyzer.rs`, `scrip_sources.rs`, `vendor_resale.rs`,
`venture_analyzer.rs`) are untouched.

- [ ] **Step 1: Write the failing tests**

Add a `mod tests` at the end of `virtual_scroller.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_viewport_ignores_window_height() {
        let s = ScrollSource::Container { viewport_height: 720.0 };
        assert_eq!(effective_viewport_for(s, 1080.0), 720.0);
    }

    #[test]
    fn window_viewport_subtracts_sticky_offset() {
        let s = ScrollSource::Window { sticky_offset: 76.0 };
        assert_eq!(effective_viewport_for(s, 900.0), 824.0);
    }

    #[test]
    fn window_viewport_never_negative() {
        // A short window with tall sticky chrome must not produce a
        // negative viewport, which would make children_shown underflow.
        let s = ScrollSource::Window { sticky_offset: 200.0 };
        assert_eq!(effective_viewport_for(s, 120.0), 0.0);
    }

    #[test]
    fn ssr_fallback_row_count_is_positive() {
        // The SSR render must emit a stable, non-zero row count so the
        // first client render can match it byte-for-byte.
        assert!(SSR_FALLBACK_ROWS > 0);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p ultros-app --lib virtual_scroller
```

Expected: FAIL — `cannot find type ScrollSource in this scope`.

- [ ] **Step 3: Add the enum and the pure helper**

Near the top of `virtual_scroller.rs`, after the `Fenwick` impl:

```rust
/// Where a [`VirtualScroller`] reads its scroll position and viewport size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollSource {
    /// The component owns a fixed-height `overflow-y-auto` container.
    /// This is the historical behavior and remains the default.
    Container { viewport_height: f64 },
    /// The page scrolls; the list measures against the window. Keeps
    /// native scrolling on mobile (no nested scroll trap, browser chrome
    /// auto-hides). `sticky_offset` is the height of sticky chrome above
    /// the list, so rows hidden behind it are not counted as visible.
    Window { sticky_offset: f64 },
}

/// Rows rendered during SSR and on the first client render in
/// [`ScrollSource::Window`] mode.
///
/// Window mode cannot measure `innerHeight` on the server. Rendering a
/// measured count on the client while the server rendered a different one
/// is an SSR/CSR mismatch, which surfaces as the tachys `hydration.rs`
/// panic this repo has hit repeatedly. Both sides therefore render exactly
/// this many rows until an `Effect` flips the `hydrated` flag.
pub const SSR_FALLBACK_ROWS: usize = 20;

/// Usable viewport height for a scroll source. Extracted so the geometry
/// is testable without a browser.
fn effective_viewport_for(source: ScrollSource, measured_window_height: f64) -> f64 {
    match source {
        ScrollSource::Container { viewport_height } => viewport_height,
        ScrollSource::Window { sticky_offset } => {
            (measured_window_height - sticky_offset).max(0.0)
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p ultros-app --lib virtual_scroller
```

Expected: PASS, 4 tests.

- [ ] **Step 5: Commit the pure layer**

```bash
git add ultros-frontend/ultros-app/src/components/virtual_scroller.rs
git commit -m "feat(virtual-scroller): add ScrollSource enum and viewport geometry helper"
```

- [ ] **Step 6: Wire window mode into the component**

Add the prop to the `VirtualScroller` signature, immediately after `viewport_height: f64`:

```rust
    /// Opt into window-scroll virtualization. `None` preserves the
    /// historical container behavior driven by `viewport_height`.
    #[prop(optional)]
    scroll_source: Option<ScrollSource>,
```

Resolve it near the top of the body, next to the existing `let header_h` line:

```rust
    let source = scroll_source
        .unwrap_or(ScrollSource::Container { viewport_height });
    let is_window = matches!(source, ScrollSource::Window { .. });

    // Hydration gate. Effects run client-only and after hydration, so the
    // first client render still sees `false` and matches the server's.
    let hydrated = RwSignal::new(false);
    Effect::new(move |_| hydrated.set(true));

    // Measured window height, only meaningful once hydrated.
    let window_height = RwSignal::new(0.0f64);
    if is_window {
        Effect::new(move |_| {
            let Some(w) = window() else { return };
            let read = move || {
                w.inner_height()
                    .ok()
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0)
            };
            window_height.set(read());
        });
    }
```

Replace the existing line `let effective_viewport = (viewport_height - header_h).max(0.0);` with a reactive memo:

```rust
    let effective_viewport = Memo::new(move |_| {
        if is_window && !hydrated.get() {
            // Deterministic pre-hydration height so SSR and the first
            // client render agree. SSR_FALLBACK_ROWS is converted back
            // into a height so children_shown lands on the same count.
            return SSR_FALLBACK_ROWS as f64 * row_height;
        }
        let measured = if is_window { window_height.get() } else { 0.0 };
        (effective_viewport_for(source, measured) - header_h).max(0.0)
    });
```

Update `children_shown` to read it as a signal:

```rust
    let children_shown = Memo::new(move |_| {
        ((effective_viewport.get() / avg_row_height()).ceil() as u32).max(1) + render_ahead
    });
```

Search the file for every other use of `effective_viewport` (the
`scroll_to_index` effect uses it twice) and append `.get()` at those sites.

- [ ] **Step 7: Add the window scroll listener**

In window mode the `on:scroll` handler on the container is inert — the container
no longer scrolls. Register a window listener instead, reusing the existing
`last_scroll` / `raf_pending` coalescing signals. Add after the `window_height`
effect:

```rust
    if is_window {
        Effect::new(move |_| {
            let Some(w) = window() else { return };
            let Some(el) = scroller.get() else { return };
            let list_top = el.get_bounding_client_rect().top()
                + w.scroll_y().unwrap_or(0.0);
            let cb = Closure::wrap(Box::new(move || {
                if let Some(w) = window() {
                    let y = w.scroll_y().unwrap_or(0.0) - list_top;
                    set_scroll_offset(y.max(0.0) as i32);
                }
            }) as Box<dyn FnMut()>);
            let _ = w.add_event_listener_with_callback(
                "scroll",
                cb.as_ref().unchecked_ref(),
            );
            let _ = w.add_event_listener_with_callback(
                "resize",
                cb.as_ref().unchecked_ref(),
            );
            cb.forget();
        });
    }
```

- [ ] **Step 8: Make the container element mode-aware**

The outer `<div>` currently hardcodes `overflow-y-auto` and a pixel height. Make
both conditional:

```rust
            class=move || if is_window {
                "w-full overflow-x-auto".to_string()
            } else {
                "overflow-y-auto overflow-x-auto w-full will-change-scroll contain-paint forced-layer".to_string()
            }
            style=move || if is_window {
                String::new()
            } else {
                format!("height: {}px;", viewport_height.ceil() as u32)
            }
```

And make the header's sticky offset mode-aware, replacing
`view! { <div class="sticky top-0 z-10">{h}</div> }`:

```rust
    {header_opt.map(|h| {
        let top = match source {
            ScrollSource::Container { .. } => 0.0,
            ScrollSource::Window { sticky_offset } => sticky_offset,
        };
        view! {
            <div class="sticky z-10" style=format!("top: {}px;", top.round() as i32)>
                {h}
            </div>
        }
    })}
```

- [ ] **Step 9: Verify no existing call site regressed**

```bash
cargo test -p ultros-app --lib
```

Expected: PASS, all tests including `virtual_scroller` and `analyzer`.

```bash
./check_ci.sh
```

Expected: clean.

- [ ] **Step 10: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/virtual_scroller.rs
git commit -m "feat(virtual-scroller): window-scroll mode with hydration-safe row count"
```

---

### Task 3: Sort direction

Today `sorted_data` hardcodes `Reverse(...)`, so every sort is descending. The
spreadsheet gesture needs both directions, round-tripped through the URL.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs` (`SortMode`, `sorted_data`, header cells)
- Test: `ultros-frontend/ultros-app/src/routes/analyzer.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: `return_on_investment` from Task 1.
- Produces:
  - `pub enum SortDir { Asc, Desc }` with `FromStr` and `Display` (`"asc"` / `"desc"`)
  - `fn sort_rows(rows: &mut [CalculatedProfitData], mode: SortMode, dir: SortDir)`
  - URL param `?dir=asc|desc`, absent means `Desc`

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `analyzer.rs`:

```rust
    fn calc(profit: i32, roi: i32, ppd: i32) -> CalculatedProfitData {
        CalculatedProfitData {
            inner: Arc::new(ProfitData {
                estimated_sale_price: 0,
                cheapest_price: 0,
                cheapest_world_id: 0,
                sale_summary: SaleSummary {
                    item_id: 1,
                    hq: false,
                    num_sold: 6,
                    avg_sale_duration: None,
                    days_since_last_sale: None,
                    max_price: 0,
                    avg_price: 0,
                    median_price: 0,
                    min_price: 0,
                },
            }),
            profit,
            return_on_investment: roi,
            profit_per_day: ppd,
        }
    }

    #[test]
    fn sort_desc_puts_largest_first() {
        let mut rows = vec![calc(10, 0, 0), calc(30, 0, 0), calc(20, 0, 0)];
        sort_rows(&mut rows, SortMode::Profit, SortDir::Desc);
        assert_eq!(
            rows.iter().map(|r| r.profit).collect::<Vec<_>>(),
            vec![30, 20, 10]
        );
    }

    #[test]
    fn sort_asc_puts_smallest_first() {
        let mut rows = vec![calc(10, 0, 0), calc(30, 0, 0), calc(20, 0, 0)];
        sort_rows(&mut rows, SortMode::Profit, SortDir::Asc);
        assert_eq!(
            rows.iter().map(|r| r.profit).collect::<Vec<_>>(),
            vec![10, 20, 30]
        );
    }

    #[test]
    fn sort_by_profit_per_day_is_independent_of_profit() {
        let mut rows = vec![calc(100, 0, 1), calc(10, 0, 99)];
        sort_rows(&mut rows, SortMode::ProfitPerDay, SortDir::Desc);
        assert_eq!(rows[0].profit_per_day, 99);
    }

    #[test]
    fn sort_dir_round_trips_through_string() {
        assert_eq!("asc".parse::<SortDir>(), Ok(SortDir::Asc));
        assert_eq!("desc".parse::<SortDir>(), Ok(SortDir::Desc));
        assert_eq!(SortDir::Asc.to_string(), "asc");
        assert_eq!(SortDir::Desc.to_string(), "desc");
        assert!("sideways".parse::<SortDir>().is_err());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p ultros-app --lib analyzer
```

Expected: FAIL — `cannot find type SortDir in this scope`.

- [ ] **Step 3: Implement `SortDir` and `sort_rows`**

Add near `SortMode` in `analyzer.rs`:

```rust
#[derive(Debug, PartialEq, Eq, Clone, Copy, Default)]
enum SortDir {
    Asc,
    #[default]
    Desc,
}

impl FromStr for SortDir {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "asc" => Ok(SortDir::Asc),
            "desc" => Ok(SortDir::Desc),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for SortDir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SortDir::Asc => "asc",
            SortDir::Desc => "desc",
        })
    }
}

/// Sort rows in place. Extracted from the `sorted_data` memo so the
/// ordering is unit-testable without a reactive runtime.
fn sort_rows(rows: &mut [CalculatedProfitData], mode: SortMode, dir: SortDir) {
    let key = |d: &CalculatedProfitData| -> i32 {
        match mode {
            SortMode::Roi => d.return_on_investment,
            SortMode::Profit => d.profit,
            SortMode::ProfitPerDay => d.profit_per_day,
        }
    };
    match dir {
        SortDir::Desc => rows.sort_by_key(|d| Reverse(key(d))),
        SortDir::Asc => rows.sort_by_key(key),
    }
}
```

- [ ] **Step 4: Replace the inline sort in `sorted_data`**

Add the query signal alongside the existing `sort` signal:

```rust
    let (sort_dir, _set_sort_dir) = query_signal::<SortDir>("dir");
```

Replace the `match sort_mode().unwrap_or(SortMode::Roi) { ... }` block with:

```rust
        sort_rows(
            &mut sorted_data,
            sort_mode().unwrap_or(SortMode::ProfitPerDay),
            sort_dir().unwrap_or_default(),
        );
```

Note the default sort mode changes from `Roi` to `ProfitPerDay` here — that is the
spec's default-query change and is intentional.

- [ ] **Step 5: Run tests to verify they pass**

```bash
cargo test -p ultros-app --lib analyzer
```

Expected: PASS, 4 new tests plus all pre-existing ones.

- [ ] **Step 6: Verify and commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-app/src/routes/analyzer.rs
git commit -m "feat(analyzer): ascending/descending sort, default to profit-per-day"
```

---

### Task 4: New columns and the default velocity floor

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs` (column consts, header, row cells, filters)
- Modify: all 7 files in `ultros-frontend/ultros-app/locales/`
- Test: `ultros-frontend/ultros-app/src/routes/analyzer.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: `velocity_per_day`, `price_drift_pct`, `derived_confidence`, `DerivedConfidence` from Task 1.
- Produces:
  - Column IDs `COL_VELOCITY`, `COL_DRIFT`, `COL_CONFIDENCE` (`&'static str`, values `"velocity"`, `"drift"`, `"confidence"`)
  - Reordered `ALL_OPTIONAL_COLS` and `DEFAULT_VISIBLE_COLS`
  - `COL_ROI` — ROI becomes optional and off by default
  - URL params `?vel=<f32>` (velocity floor) and `?sales=<usize>` (already exists)

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn roi_is_optional_and_off_by_default() {
        assert!(ALL_OPTIONAL_COLS.contains(&COL_ROI));
        assert!(!DEFAULT_VISIBLE_COLS.contains(&COL_ROI));
    }

    #[test]
    fn new_columns_are_on_by_default() {
        for col in [COL_VELOCITY, COL_DRIFT, COL_CONFIDENCE] {
            assert!(ALL_OPTIONAL_COLS.contains(&col), "{col} missing from ALL");
            assert!(DEFAULT_VISIBLE_COLS.contains(&col), "{col} not default-on");
        }
    }

    #[test]
    fn ch_only_columns_are_off_by_default() {
        for col in [COL_TREND, COL_VOLUME_30D, COL_SALES_PER_DAY, COL_DATACENTER] {
            assert!(
                !DEFAULT_VISIBLE_COLS.contains(&col),
                "{col} should be opt-in (ClickHouse covers ~7% of items)"
            );
        }
    }

    #[test]
    fn visible_cols_round_trip_with_new_ids() {
        let set = parse_visible_cols(Some("velocity,drift,confidence"));
        assert_eq!(set.len(), 3);
        let s = serialize_visible_cols(&set);
        assert_eq!(parse_visible_cols(Some(&s)), set);
    }

    #[test]
    fn explicit_empty_cols_param_is_respected() {
        // Regression guard: an explicit "" must mean "no optional columns",
        // not "fall back to defaults".
        assert!(parse_visible_cols(Some("")).is_empty());
        assert!(!parse_visible_cols(None).is_empty());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p ultros-app --lib analyzer
```

Expected: FAIL — `cannot find value COL_VELOCITY in this scope`.

- [ ] **Step 3: Update the column registry**

Replace the column consts block in `analyzer.rs` (currently lines 59-88):

```rust
const COL_PROFIT_PER_DAY: &str = "profit_per_day";
const COL_VELOCITY: &str = "velocity";
const COL_DRIFT: &str = "drift";
const COL_CONFIDENCE: &str = "confidence";
const COL_WORLD: &str = "world";
const COL_LAST_SOLD: &str = "last_sold";
const COL_ROI: &str = "roi";
const COL_DATACENTER: &str = "datacenter";
const COL_TREND: &str = "trend";
const COL_SALES_PER_DAY: &str = "sales_per_day";
const COL_VOLUME_30D: &str = "volume_30d";

const ALL_OPTIONAL_COLS: &[&str] = &[
    COL_PROFIT_PER_DAY,
    COL_VELOCITY,
    COL_DRIFT,
    COL_CONFIDENCE,
    COL_WORLD,
    COL_LAST_SOLD,
    COL_ROI,
    COL_DATACENTER,
    COL_TREND,
    COL_SALES_PER_DAY,
    COL_VOLUME_30D,
];

/// Default visible set when `?cols=` is absent. ClickHouse-only columns
/// (trend, sales/day, 30d volume) are off because the rollup covers ~7% of
/// traded items — see the spec's Finding 1. ROI is off because it ranks by
/// ratio, which is the wrong objective when retainer slots are the scarce
/// resource.
const DEFAULT_VISIBLE_COLS: &[&str] = &[
    COL_PROFIT_PER_DAY,
    COL_VELOCITY,
    COL_DRIFT,
    COL_CONFIDENCE,
    COL_WORLD,
    COL_LAST_SOLD,
];
```

`parse_visible_cols` and `serialize_visible_cols` need no changes — they already
iterate `ALL_OPTIONAL_COLS`.

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p ultros-app --lib analyzer
```

Expected: PASS, 5 new tests.

- [ ] **Step 5: Add locale keys to all 7 files**

Add to each of `en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc` in
`ultros-frontend/ultros-app/locales/`, beside the existing `analyzer_col_*` keys:

| key | en | fr | de | ja | cn | ko | tc |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `analyzer_col_velocity` | Velocity | Vitesse | Tempo | 回転率 | 流通速度 | 판매 속도 | 流通速度 |
| `analyzer_col_drift` | Drift | Tendance | Trend | 価格推移 | 价格走势 | 가격 추이 | 價格走勢 |
| `analyzer_col_confidence` | Confidence | Fiabilité | Verlässlichkeit | 信頼度 | 可信度 | 신뢰도 | 可信度 |
| `analyzer_confidence_high` | High | Élevée | Hoch | 高 | 高 | 높음 | 高 |
| `analyzer_confidence_medium` | Medium | Moyenne | Mittel | 中 | 中 | 보통 | 中 |
| `analyzer_confidence_low` | Low | Faible | Gering | 低 | 低 | 낮음 | 低 |
| `analyzer_filter_velocity_min_label` | Min velocity | Vitesse min. | Min. Tempo | 最低回転率 | 最低流通速度 | 최소 판매 속도 | 最低流通速度 |
| `analyzer_velocity_per_day` | %count%/day | %count%/jour | %count%/Tag | %count%/日 | %count%/天 | %count%/일 | %count%/日 |
| `analyzer_drift_unavailable` | Not enough sales | Ventes insuffisantes | Zu wenige Verkäufe | 販売数が不足 | 销售数据不足 | 판매 데이터 부족 | 銷售資料不足 |

- [ ] **Step 6: Render the three new columns**

In the `VirtualScroller` `header=` block, add a header cell per column, following
the existing `{move || visible_cols().contains(COL_X).then(|| view! { ... })}`
pattern. Velocity and Drift are right-aligned numerics
(`class="w-[88px] px-3 py-2 hidden md:flex items-center justify-end"`), Confidence
is centered (`class="w-[72px] px-3 py-2 hidden md:flex items-center justify-center"`).

In the row `view=` closure, hoist the values once before the per-column closures —
the existing code already does this for `row_key` / `row_cheapest_price` /
`row_days_since` because `data.inner` is an `Arc` and not `Copy`:

```rust
                                        let row_velocity = crate::analysis::velocity_per_day(
                                            &data.inner.sale_summary,
                                        );
                                        let row_drift = row_prices
                                            .as_ref()
                                            .and_then(|p| crate::analysis::price_drift_pct(p));
                                        let row_confidence = crate::analysis::derived_confidence(
                                            &data.inner.sale_summary,
                                        );
```

Velocity prefers the ClickHouse value when present:

```rust
                                            {move || visible_cols().contains(COL_VELOCITY).then(|| {
                                                let maps = enrichment.get();
                                                let v = maps
                                                    .quality_for(&row_key)
                                                    .map(|q| q.sales_per_day)
                                                    .or(row_velocity);
                                                let text = match v {
                                                    Some(v) => t_string!(i18n, analyzer_velocity_per_day)
                                                        .to_string()
                                                        .replace("%count%", &format!("{v:.1}")),
                                                    None => "—".to_string(),
                                                };
                                                view! {
                                                    <div role="cell" class="px-3 py-2 w-[88px] hidden md:flex items-center justify-end font-mono tabular-nums">
                                                        {text}
                                                    </div>
                                                }
                                            })}
```

Drift is colored by sign, using the same `color-mix` idiom as the existing chips:

```rust
                                            {move || visible_cols().contains(COL_DRIFT).then(|| {
                                                let (text, class) = match row_drift {
                                                    Some(d) if d > 1.0 => (format!("+{d:.0}%"), "text-emerald-300"),
                                                    Some(d) if d < -1.0 => (format!("{d:.0}%"), "text-red-300"),
                                                    Some(d) => (format!("{d:+.0}%"), "text-[color:var(--color-text-muted)]"),
                                                    None => ("—".to_string(), "text-[color:var(--color-text-muted)]"),
                                                };
                                                view! {
                                                    <div role="cell" class=format!("px-3 py-2 w-[88px] hidden md:flex items-center justify-end font-mono tabular-nums {class}")>
                                                        {text}
                                                    </div>
                                                }
                                            })}
```

Confidence prefers the ClickHouse band, falling back to the derived one:

```rust
                                            {move || visible_cols().contains(COL_CONFIDENCE).then(|| {
                                                let maps = enrichment.get();
                                                let (label, class) = match maps.quality_for(&row_key).map(|q| q.confidence_band) {
                                                    Some(ConfidenceBand::High) => (t_string!(i18n, analyzer_confidence_high).to_string(), "text-emerald-300"),
                                                    Some(ConfidenceBand::Medium) => (t_string!(i18n, analyzer_confidence_medium).to_string(), "text-amber-300"),
                                                    Some(ConfidenceBand::Low) | Some(ConfidenceBand::Unusable) => (t_string!(i18n, analyzer_confidence_low).to_string(), "text-red-300"),
                                                    Some(ConfidenceBand::Unknown) | None => match row_confidence {
                                                        DerivedConfidence::High => (t_string!(i18n, analyzer_confidence_high).to_string(), "text-emerald-300"),
                                                        DerivedConfidence::Medium => (t_string!(i18n, analyzer_confidence_medium).to_string(), "text-amber-300"),
                                                        DerivedConfidence::Low => (t_string!(i18n, analyzer_confidence_low).to_string(), "text-red-300"),
                                                    },
                                                };
                                                view! {
                                                    <div role="cell" class="px-3 py-2 w-[72px] hidden md:flex items-center justify-center">
                                                        <span class=format!("text-xs font-semibold {class}")>{label}</span>
                                                    </div>
                                                }
                                            })}
```

`row_prices` requires the raw price vector on the row. Add
`prices: Vec<i32>` to `ProfitData` and populate it in `ProfitTable::new` from the
already-sorted-by-recency `sale.sales` before `compute_summary` consumes the
`SaleData` — capture `sale.sales.iter().map(|s| s.price_per_unit).collect()` first.
Then read it in the row closure as `let row_prices = data.inner.prices.clone();`
hoisted alongside `row_key`.

Adding that field breaks the `calc()` test helper written in Task 3, which
constructs `ProfitData` literally. Update it in the same commit:

```rust
            inner: Arc::new(ProfitData {
                estimated_sale_price: 0,
                cheapest_price: 0,
                cheapest_world_id: 0,
                prices: Vec::new(),
                sale_summary: SaleSummary {
```

Extend the `use crate::analysis::{...}` import at the top of `analyzer.rs` to
include `DerivedConfidence`, `derived_confidence`, `price_drift_pct`, and
`velocity_per_day` so the row closures can name them unqualified.

- [ ] **Step 7: Add the velocity floor filter**

Add the query signal beside the existing filter signals:

```rust
    let (min_velocity, set_min_velocity) = query_signal::<f32>("vel");
```

Add the filter to the `sorted_data` chain, after the existing `minimum_sales` filter:

```rust
            .filter(move |data| {
                min_velocity()
                    .map(|min| {
                        crate::analysis::velocity_per_day(&data.inner.sale_summary)
                            .map(|v| v >= min)
                            .unwrap_or(false)
                    })
                    .unwrap_or(true)
            })
```

- [ ] **Step 8: Verify**

```bash
cargo test -p ultros-app --lib
./check_ci.sh
```

Expected: PASS and clean. `leptos-i18n` fails to compile if any key is missing from
any locale, so a build failure here means a missing translation.

- [ ] **Step 9: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/analyzer.rs ultros-frontend/ultros-app/locales/
git commit -m "feat(analyzer): velocity, drift and confidence columns; demote ROI and CH-only columns"
```

---

### Task 5: Sticky bar with editable filter chips

Collapses the toolbar (146px) and the chip summary (52px) into one 76px sticky bar,
deleting the duplicate representation of every filter.

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/filter_chip.rs`
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs` (register the module)
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs` (replace both toolbars)
- Modify: `ultros-frontend/ultros-app/style/../../../style/tailwind.css` — actual path `style/tailwind.css` at repo root (sticky bar utility)
- Modify: all 7 locale files

**Interfaces:**
- Consumes: `SortDir` (Task 3), the column registry (Task 4).
- Produces:
  - `pub const STICKY_BAR_HEIGHT: f64 = 76.0;` in `filter_chip.rs` — feeds `ScrollSource::Window { sticky_offset }`
  - `#[component] pub fn FilterChip(label: String, value: Signal<Option<String>>, on_commit: Callback<Option<String>>, numeric: bool) -> impl IntoView`

- [ ] **Step 1: Create the chip component**

`ultros-frontend/ultros-app/src/components/filter_chip.rs`:

```rust
//! Editable filter chip. Resting state shows `label value`; clicking the
//! value turns it into an inline input. The `x` clears the filter.
//!
//! This is the *only* representation of a filter on the Flip Finder — the
//! page previously rendered each filter twice (a toolbar input plus a chip
//! echoing it), which cost 198px of vertical space for one piece of state.

use crate::components::icon::Icon;
use crate::i18n::*;
use leptos::prelude::*;

/// Height reserved for the sticky control bar. Feeds
/// `ScrollSource::Window { sticky_offset }` so rows hidden behind the bar
/// are not counted as visible.
pub const STICKY_BAR_HEIGHT: f64 = 76.0;

#[component]
pub fn FilterChip(
    #[prop(into)] label: String,
    #[prop(into)] value: Signal<Option<String>>,
    #[prop(into)] on_commit: Callback<Option<String>>,
    #[prop(optional)] numeric: bool,
) -> impl IntoView {
    let i18n = use_i18n();
    let editing = RwSignal::new(false);
    let label_for_display = label.clone();

    view! {
        <Show
            when=move || editing.get()
            fallback=move || {
                let label = label_for_display.clone();
                view! {
                    <span class="filter-chip">
                        <button class="filter-chip-value" on:click=move |_| editing.set(true)>
                            {label.clone()} " " {move || value.get().unwrap_or_default()}
                        </button>
                        <button
                            aria-label=t_string!(i18n, aria_remove_filter)
                            on:click=move |_| on_commit.run(None)
                        >
                            <Icon icon=icondata::MdiClose />
                        </button>
                    </span>
                }
            }
        >
            <span class="filter-chip filter-chip-editing">
                <span class="filter-chip-label">{label.clone()}</span>
                <input
                    class="input input-sm w-24"
                    type=if numeric { "number" } else { "text" }
                    prop:value=move || value.get().unwrap_or_default()
                    on:blur=move |ev| {
                        let v = event_target_value(&ev);
                        on_commit.run((!v.is_empty()).then_some(v));
                        editing.set(false);
                    }
                    on:keydown=move |ev| {
                        if ev.key() == "Enter" {
                            let v = event_target_value(&ev);
                            on_commit.run((!v.is_empty()).then_some(v));
                            editing.set(false);
                        } else if ev.key() == "Escape" {
                            editing.set(false);
                        }
                    }
                />
            </span>
        </Show>
    }
    .into_any()
}
```

- [ ] **Step 2: Register the module**

Add to `ultros-frontend/ultros-app/src/components/mod.rs`, keeping the existing
alphabetical ordering:

```rust
pub mod filter_chip;
```

- [ ] **Step 3: Add the CSS**

Append to `style/tailwind.css`, beside the existing `@utility toolbar` block
(around line 1937):

```css
/* ----- Flip Finder sticky control bar ----- */
@utility sticky-bar {
    position: sticky;
    top: 0;
    z-index: 20;
    background-color: var(--color-background-panel);
    border-bottom: 1px solid var(--color-outline);
}
.filter-chip {
    display: inline-flex;
    align-items: center;
    gap: 0.375rem;
    border-radius: 0.5rem;
    padding: 0.25rem 0.5rem;
    font-size: 0.85rem;
    color: var(--color-text);
    background-color: color-mix(in srgb, var(--brand-ring) 14%, transparent);
    border: 1px solid var(--color-outline);
}
.filter-chip-editing {
    background-color: transparent;
    border-color: var(--brand-ring);
}
.filter-chip-label {
    color: var(--color-text-muted);
}
.filter-chip-value {
    background: transparent;
    color: inherit;
}
```

- [ ] **Step 4: Replace both toolbars in the route**

Delete the primary `<Toolbar>` block, the secondary `show_more` `<Toolbar>` block,
and the results-summary `<div class="panel px-4 py-3 ...">` chip panel. Replace
with a single sticky bar of two rows: row 1 holds the world picker, saved-views
menu (Task 6), row count, Columns and Save view buttons; row 2 holds the chips and
`+ Filter`.

Each active filter renders one `FilterChip`. Example wiring for minimum profit:

```rust
                    {move || minimum_profit().map(|p| view! {
                        <FilterChip
                            label=t_string!(i18n, analyzer_profit_gte).to_string()
                            value=Signal::derive(move || minimum_profit().map(|v| v.to_string()))
                            numeric=true
                            on_commit=Callback::new(move |v: Option<String>| {
                                set_minimum_profit(v.and_then(|s| s.parse::<i32>().ok()));
                            })
                        />
                    })}
```

Keep `show_more` and the `Toolbar` import only if still referenced; otherwise
remove both so clippy does not flag unused imports.

- [ ] **Step 5: Switch the table to window scrolling**

Replace the `VirtualScroller` props on the analyzer's call site:

```rust
                <VirtualScroller
                        scroll_source=ScrollSource::Window { sticky_offset: STICKY_BAR_HEIGHT }
                        viewport_height=720.0
                        row_height=40.0
```

`viewport_height` stays because the prop is still required by the signature; it is
ignored in window mode.

Remove the `min-h-screen` wrapper and the `overflow-x-auto` container's fixed
framing so the page itself scrolls.

- [ ] **Step 6: Add locale keys to all 7 files**

| key | en | fr | de | ja | cn | ko | tc |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `analyzer_add_filter` | Filter | Filtre | Filter | フィルター | 筛选 | 필터 | 篩選 |
| `analyzer_rows_count` | %count% rows | %count% lignes | %count% Zeilen | %count%件 | %count% 行 | %count%개 | %count% 列 |

- [ ] **Step 7: Verify**

```bash
cargo test -p ultros-app --lib
./check_ci.sh
```

Expected: PASS and clean.

- [ ] **Step 8: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/filter_chip.rs ultros-frontend/ultros-app/src/components/mod.rs ultros-frontend/ultros-app/src/routes/analyzer.rs style/tailwind.css ultros-frontend/ultros-app/locales/
git commit -m "feat(analyzer): sticky control bar with editable filter chips"
```

---

### Task 6: Saved views in localStorage

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/saved_views.rs`
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs` (mount the menu)
- Modify: all 7 locale files
- Test: `ultros-frontend/ultros-app/src/components/saved_views.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: `STICKY_BAR_HEIGHT` (Task 5).
- Produces:
  - `pub struct SavedView { pub name: String, pub query: String, pub world: Option<String> }` deriving `Clone, Debug, PartialEq, Serialize, Deserialize`
  - `pub const SAVED_VIEWS_KEY: &str = "ultros.flipfinder.views";`
  - `pub fn built_in_views() -> Vec<SavedView>`
  - `pub fn view_href(view: &SavedView, current_world: &str) -> String`
  - `#[component] pub fn SavedViewsMenu(current_world: Signal<String>) -> impl IntoView`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpinned_view_keeps_the_current_world() {
        let v = SavedView {
            name: "Slot savers".into(),
            query: "?sort=profit-per-day&vel=0.2".into(),
            world: None,
        };
        assert_eq!(
            view_href(&v, "Sargatanas"),
            "/flip-finder/Sargatanas?sort=profit-per-day&vel=0.2"
        );
    }

    #[test]
    fn pinned_view_navigates_to_its_own_world() {
        let v = SavedView {
            name: "Gilg dashboard".into(),
            query: "?sort=profit".into(),
            world: Some("Gilgamesh".into()),
        };
        assert_eq!(view_href(&v, "Sargatanas"), "/flip-finder/Gilgamesh?sort=profit");
    }

    #[test]
    fn empty_query_produces_a_clean_path() {
        let v = SavedView { name: "All".into(), query: String::new(), world: None };
        assert_eq!(view_href(&v, "Gilgamesh"), "/flip-finder/Gilgamesh");
    }

    #[test]
    fn saved_view_round_trips_through_json() {
        let v = SavedView {
            name: "Slot savers".into(),
            query: "?vel=0.2".into(),
            world: Some("Gilgamesh".into()),
        };
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(serde_json::from_str::<SavedView>(&json).unwrap(), v);
    }

    #[test]
    fn every_built_in_view_requires_a_sale_within_one_day() {
        // A sale a week old is weak evidence anyone is buying the item
        // today, which is the question a flip turns on.
        for v in built_in_views() {
            assert!(
                v.query.contains("last-sold=1d"),
                "built-in view {:?} must use last-sold=1d, got {:?}",
                v.name,
                v.query
            );
        }
    }

    #[test]
    fn built_in_views_are_unpinned() {
        for v in built_in_views() {
            assert_eq!(v.world, None, "built-in {:?} must not pin a world", v.name);
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p ultros-app --lib saved_views
```

Expected: FAIL — `cannot find type SavedView in this scope`.

- [ ] **Step 3: Implement the data layer**

`ultros-frontend/ultros-app/src/components/saved_views.rs`:

```rust
//! Saved filter views, persisted to localStorage.
//!
//! Every Flip Finder filter is already a `query_signal`, so the complete
//! filter state *is* the URL query string. A saved view is therefore just
//! a name plus that string — no schema to migrate, and filters added later
//! are captured automatically because the payload is opaque.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

pub const SAVED_VIEWS_KEY: &str = "ultros.flipfinder.views";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedView {
    pub name: String,
    /// Query string including the leading `?`, or empty for no filters.
    pub query: String,
    /// `Some(world)` pins the view to that world, making it a destination
    /// that navigates. `None` applies the filters to whatever world is
    /// open. Unpinned is the default; pinning is opt-in at save time and
    /// exists for players with characters on different worlds.
    pub world: Option<String>,
}

/// Resolve a view to an href. Pinned views navigate worlds; unpinned views
/// stay put.
pub fn view_href(view: &SavedView, current_world: &str) -> String {
    let world = view.world.as_deref().unwrap_or(current_world);
    format!("/flip-finder/{world}{}", view.query)
}

/// The former preset buttons, now the built-in entries of the same menu.
/// All six require a sale within 24 hours.
pub fn built_in_views() -> Vec<SavedView> {
    [
        ("analyzer_preset_realistic", "?min-buy=5000&last-sold=1d&roi=30&sort=profit-per-day"),
        ("analyzer_preset_big_ticket", "?min-buy=100000&last-sold=1d&roi=20&sort=profit"),
        ("analyzer_preset_volume", "?min-buy=1000&last-sold=1d&sort=profit-per-day"),
        ("analyzer_preset_300_return", "?min-buy=1000&last-sold=1d&roi=300&profit=0&sort=profit"),
        ("analyzer_preset_500_return", "?min-buy=10000&last-sold=1d&roi=500&profit=200000"),
        ("analyzer_preset_100k_profit", "?min-buy=1000&last-sold=1d&profit=100000"),
    ]
    .into_iter()
    .map(|(name, query)| SavedView {
        name: name.to_string(),
        query: query.to_string(),
        world: None,
    })
    .collect()
}
```

Built-in `name` values are i18n **keys**, resolved at render time; user-saved views
store literal names. The menu distinguishes them by membership in `built_in_views()`.

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p ultros-app --lib saved_views
```

Expected: PASS, 6 tests.

- [ ] **Step 5: Add the menu component**

Append to `saved_views.rs`, using the same storage call as
`recently_viewed.rs:39`:

```rust
use codee::string::JsonSerdeCodec;
use leptos_use::storage::{UseStorageOptions, use_local_storage_with_options};

#[component]
pub fn SavedViewsMenu(#[prop(into)] current_world: Signal<String>) -> impl IntoView {
    let i18n = use_i18n();
    let (views, set_views, _) = use_local_storage_with_options::<Vec<SavedView>, JsonSerdeCodec>(
        SAVED_VIEWS_KEY,
        // Private-browsing / storage-disabled must degrade to session-only,
        // never panic.
        UseStorageOptions::default().delay_during_hydration(true),
    );
    let open = RwSignal::new(false);
    view! {
        <div class="relative">
            <button class="btn-secondary" on:click=move |_| open.update(|v| *v = !*v)>
                {t!(i18n, analyzer_saved_views)}
            </button>
            <Show when=move || open.get()>
                <div class="panel absolute z-30 mt-1 min-w-56 rounded-lg p-2 flex flex-col gap-1">
                    {move || built_in_views().into_iter().map(|v| {
                        let href = view_href(&v, &current_world.get());
                        view! { <a class="btn-ghost text-left" href=href>{v.name.clone()}</a> }
                    }).collect_view()}
                    {move || views.get().into_iter().enumerate().map(|(i, v)| {
                        let href = view_href(&v, &current_world.get());
                        view! {
                            <div class="flex items-center gap-1">
                                <a class="btn-ghost flex-1 text-left" href=href>{v.name.clone()}</a>
                                <button
                                    aria-label=t_string!(i18n, analyzer_delete_view)
                                    on:click=move |_| set_views.update(|vs| { vs.remove(i); })
                                >
                                    <Icon icon=icondata::MdiClose />
                                </button>
                            </div>
                        }
                    }).collect_view()}
                </div>
            </Show>
        </div>
    }
    .into_any()
}
```

Built-in names render through `t_string!` by key; wire that with the same
`match`-on-key helper style used by `col_label` in `analyzer.rs`.

- [ ] **Step 6: Register and mount**

Add `pub mod saved_views;` to `components/mod.rs`. Mount `<SavedViewsMenu
current_world=world />` in the sticky bar's first row, and delete the
`PresetFilterButton` component and its six call sites from `analyzer.rs`.

- [ ] **Step 7: Add locale keys to all 7 files**

| key | en | fr | de | ja | cn | ko | tc |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `analyzer_saved_views` | Views | Vues | Ansichten | ビュー | 视图 | 뷰 | 檢視 |
| `analyzer_save_view` | Save view | Enregistrer | Ansicht sichern | ビューを保存 | 保存视图 | 뷰 저장 | 儲存檢視 |
| `analyzer_delete_view` | Delete view | Supprimer la vue | Ansicht löschen | ビューを削除 | 删除视图 | 뷰 삭제 | 刪除檢視 |
| `analyzer_pin_view_to_world` | Pin to this world | Épingler à ce monde | An diese Welt binden | このワールドに固定 | 固定到此服务器 | 이 월드에 고정 | 釘選到此伺服器 |
| `analyzer_view_name_placeholder` | Slot savers | Flips rapides | Schnelle Flips | 回転重視 | 快速周转 | 빠른 회전 | 快速周轉 |

- [ ] **Step 8: Retune the two preset labels that embed a window**

`analyzer_preset_300_return` and `analyzer_preset_500_return` name their old
windows in every locale. Update all 7:

| key | en | fr | de | ja | cn | ko | tc |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `analyzer_preset_300_return` | 300% return - 1 day | 300 % de rendement - 1 jour | 300 % Rendite — 1 Tag | 利益率300% - 1日 | 利润率300% - 1天 | 수익률 300% - 1일 | 利潤率300% - 1天 |
| `analyzer_preset_500_return` | 500% return - 200K min profit - 1 day | 500 % de rendement - 200K de profit min. - 1 jour | 500 % Rendite — 200K Mindestgewinn — 1 Tag | 利益率500% - 最低利益20万 - 1日 | 利润率500% - 最低利润20万 - 1天 | 수익률 500% - 최소 수익 20만 - 1일 | 利潤率500% - 最低利潤20萬 - 1天 |

- [ ] **Step 9: Verify**

```bash
cargo test -p ultros-app --lib
./check_ci.sh
```

Expected: PASS and clean.

- [ ] **Step 10: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/saved_views.rs ultros-frontend/ultros-app/src/components/mod.rs ultros-frontend/ultros-app/src/routes/analyzer.rs ultros-frontend/ultros-app/locales/
git commit -m "feat(analyzer): saved views in localStorage, presets become built-in views"
```

---

### Task 7: Delete the copy

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs` (`AnalyzerWorldView`, `Analyzer`)
- Modify: all 7 locale files
- Modify: `ultros-frontend/ultros-app/src/components/tool_help.rs` (only if `ToolHeader` becomes unused)

- [ ] **Step 1: Remove the blocks from `AnalyzerWorldView`**

Delete, in order:

1. The `<ToolHeader ... />` element (title moves into the sticky bar).
2. The entire `<details>` calc explainer, including both `<AssumptionBadge>` children.
3. The now-empty `panel p-4 sm:p-6 rounded-2xl` controls wrapper — the world picker and the two toggles move into the sticky bar and the Columns popover.

Keep `<MetaTitle>` and `<MetaDescription>` — those are SEO, not on-page copy.

- [ ] **Step 2: Remove the index-page blocks from `Analyzer`**

Delete the Features grid (three `card p-6` divs) and the Tips `<ul>`. Keep the hero
`h1`, the one-line description, and `AnalyzerWorldNavigator` — a world must be
chosen before the table means anything.

- [ ] **Step 3: Delete the orphaned locale keys from all 7 files**

```
analyzer_tool_summary        analyzer_tool_context        analyzer_tool_help
analyzer_calc_title          analyzer_calc_formula        analyzer_calc_details
analyzer_assumption_cross_region                          analyzer_assumption_hq_nq
analyzer_feature_profit_tracking                          analyzer_feature_profit_tracking_desc
analyzer_feature_market_analysis                          analyzer_feature_market_analysis_desc
analyzer_feature_custom_filters                           analyzer_feature_custom_filters_desc
analyzer_tips_title          analyzer_tip_1               analyzer_tip_2
analyzer_tip_3               analyzer_tip_4
```

Before deleting each, confirm no other route still uses it:

```bash
grep -rn "analyzer_tip_1\|analyzer_calc_title\|analyzer_feature_profit_tracking" --include=*.rs ultros-frontend/
```

Expected: no matches after Steps 1-2.

- [ ] **Step 4: Check whether `ToolHeader` is now dead**

```bash
grep -rn "<ToolHeader" --include=*.rs ultros-frontend/
```

If other routes still use it, leave `tool_help.rs` alone. If this was the last call
site, delete the `ToolHeader` component and its `tool_help_*` keys from all 7
locales — clippy's `-D warnings` will flag it as dead code otherwise.

- [ ] **Step 5: Verify**

```bash
cargo test -p ultros-app --lib
./check_ci.sh
```

Expected: PASS and clean. A missing-key panic here means a deleted key is still
referenced somewhere.

- [ ] **Step 6: Confirm the fold budget in a real browser**

```bash
./scripts/run_e2e.sh
```

Then load `/flip-finder/Gilgamesh` at 1280x720 and confirm the first data row sits
at roughly y≈180 rather than the y=827 measured before this work.

- [ ] **Step 7: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/analyzer.rs ultros-frontend/ultros-app/locales/ ultros-frontend/ultros-app/src/components/tool_help.rs
git commit -m "refactor(analyzer): delete explanatory copy superseded by per-row columns"
```

---

## Self-review notes

**Spec coverage.** Window-scrolled shell → Task 2 + Task 5 Step 5. Derived metrics
→ Task 1, rendered in Task 4. Column table → Task 4 Step 3. Default query → Task 3
Step 4 (sort) + Task 4 Step 7 (velocity floor). Sticky bar → Task 5. Saved views
and built-in views → Task 6. Copy removal → Task 7. i18n → Tasks 4, 5, 6, 7. ROI
overflow → Task 1 Steps 3+5.

**Deliberate ordering.** Tasks 1 and 2 are independently shippable and carry all
the unit-testable logic; Task 2 lands the hydration-sensitive change alone so a
regression has one obvious culprit. Tasks 3-4 change behavior with the old chrome
still in place, so the data layer can be verified before the layout moves. Tasks
5-7 are the visual rebuild.

**Known follow-up, out of scope.** The ClickHouse ingest gap (spec Finding 1). No
task depends on it, and none of the defaults regress when it is fixed — CH values
supersede derived ones wherever both exist.
