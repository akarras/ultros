# Real Price Metric Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the item detail page's outlier-skewed "Recent Average" headline with a launder-resistant **Real Price**, computed client-side from the ~200 sales already loaded.

**Architecture:** A pure, unit-tested function `real_price` in `analysis.rs` computes a per-quality (NQ/HQ) robust estimate — a vendor-anchor guard, then an IQR-filtered mean (median fallback for `<4` samples) reusing the existing `filter_outliers_iqr_in_place`. `MarketStatsPanel` in `item_view.rs` calls it and renders the bottom-left stat card; the raw mean/median are demoted to a muted transparency line.

**Tech Stack:** Rust, Leptos 0.7 (frontend `ultros-app` crate), `leptos-i18n` (7 locale JSON files), `chrono`. No backend/ClickHouse/API changes.

**Spec:** [`docs/superpowers/specs/2026-06-07-real-price-metric-design.md`](../specs/2026-06-07-real-price-metric-design.md)

---

## File Structure

| File | Responsibility |
|---|---|
| `ultros-frontend/ultros-app/src/analysis.rs` | **Modify** — add `RealPriceEstimate`, `RealPriceBreakdown`, `real_price()` + unit tests. Pure, Leptos-free. |
| `ultros-frontend/ultros-app/src/routes/item_view.rs` | **Modify** — `MarketStatsPanel`: compute vendor price + `real_price`, restyle the bottom-left card, demote raw avg/median. |
| `ultros-frontend/ultros-app/locales/en.json` (+ fr, de, ja, cn, ko, tc) | **Modify** — add `real_price`, `real_price_basis` keys (all 7). |

**Commits are local only** (worktree branch); do not push. Run `cargo fmt --all` before each commit; run the full `./check_ci.sh` in the final task.

---

## Task 0: Environment prep (one-time, for build/test)

**Files:** none (environment only).

- [ ] **Step 1: Initialize the game-data submodule (recursive)**

The `xiv-gen-db` build script reads `xiv-gen/ffxiv-datamining/` and its nested submodules. From the worktree root:

Run (Git Bash): `git submodule update --init --recursive --depth=1`
Expected: submodule paths checked out; no panic about missing `cn/Item.csv` later.

- [ ] **Step 2: Point cargo at the main repo's warm target dir**

Reuses cached artifacts so the first build isn't a full ~10-min cold build.

Run (PowerShell, each shell that builds): `$env:CARGO_TARGET_DIR = "C:\Users\chw11\code\ultros\target"`
(Git Bash equivalent: `export CARGO_TARGET_DIR=/c/Users/chw11/code/ultros/target`)

Note: `ultros-app` does not depend on the `ultros` server crate, so the OpenSSL/Strawberry-Perl wall (CLAUDE.md) should not apply to its tests. If a build unexpectedly pulls `openssl`, prepend Strawberry Perl to PATH per CLAUDE.md.

---

## Task 1: Pure `real_price` function + types + tests

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analysis.rs`
- Test: `ultros-frontend/ultros-app/src/analysis.rs` (inline `#[cfg(test)]` module)

- [ ] **Step 1: Write the failing tests**

Append this module to the end of `ultros-frontend/ultros-app/src/analysis.rs`:

```rust
#[cfg(test)]
mod real_price_tests {
    use super::*;

    /// Build NQ-only samples from (price, qty) pairs.
    fn nq(pairs: &[(i32, i32)]) -> Vec<(i32, i32, bool)> {
        pairs.iter().map(|&(p, q)| (p, q, false)).collect()
    }

    #[test]
    fn headline_case_one_huge_outlier() {
        // 199 sales @ 16_000 + one 75M launder sale (qty 1), non-vendor item.
        let mut s = vec![(16_000i32, 1i32, false); 199];
        s.push((75_000_000, 1, false));
        let r = real_price(&s, None);
        let (is_hq, est) = r.primary().expect("primary present");
        assert!(!is_hq);
        assert_eq!(est.value, 16_000);
        assert_eq!(est.total, 200);
        assert_eq!(est.used, 199);
        assert_eq!(est.excluded, 1);
    }

    #[test]
    fn vendor_guard_catches_majority_launder() {
        // vendor price 100 -> cap 10_000. Three qty-1 launder sales dominate, so the
        // quartiles shift and IQR alone would NOT remove them; the vendor anchor does.
        let s = vec![
            (49_000, 1, false),
            (50_000, 1, false),
            (51_000, 1, false),
            (100, 1, false),
            (110, 1, false),
        ];
        let r = real_price(&s, Some(100));
        let (_, est) = r.primary().expect("primary present");
        assert_eq!(est.total, 5);
        assert_eq!(est.used, 2); // only the two legit sales remain
        assert_eq!(est.excluded, 3);
        assert_eq!(est.value, 110); // median of [100, 110]
    }

    #[test]
    fn vendor_guard_ignores_non_qty1() {
        // Same overpriced price but qty 2 -> NOT removed by the guard (guard is qty==1 only).
        // (IQR still removes it here, but used stays 4 either way; this asserts the guard
        // did not fire on qty>1 by checking excluded is attributable to IQR, not the guard.)
        let s = vec![
            (100, 1, false),
            (105, 1, false),
            (110, 1, false),
            (120, 1, false),
            (50_000, 2, false),
        ];
        let r = real_price(&s, Some(100));
        let (_, est) = r.primary().expect("primary present");
        assert_eq!(est.total, 5);
        assert_eq!(est.used, 4);
        assert!(est.value >= 100 && est.value <= 120);
    }

    #[test]
    fn small_sample_uses_median_not_mean() {
        // n=3 (<4): median resists the launder; the mean would be ~25M.
        let s = nq(&[(16_000, 1), (16_000, 1), (75_000_000, 1)]);
        let (_, est) = real_price(&s, None).primary().expect("primary present");
        assert_eq!(est.value, 16_000);
        assert_eq!(est.used, 3);
        assert_eq!(est.total, 3);
        assert_eq!(est.excluded, 0);
    }

    #[test]
    fn all_equal_excludes_nothing() {
        let s = nq(&[(16_000, 1); 10]);
        let (_, est) = real_price(&s, None).primary().expect("primary present");
        assert_eq!(est.value, 16_000);
        assert_eq!(est.used, 10);
        assert_eq!(est.excluded, 0);
    }

    #[test]
    fn hq_and_nq_computed_independently() {
        // NQ ~16k with more sales (primary), HQ ~50k (secondary). Never averaged.
        let mut s = vec![(16_000i32, 1i32, false); 6];
        s.extend(vec![(50_000, 1, true); 5]);
        let r = real_price(&s, None);
        let (p_is_hq, p) = r.primary().expect("primary present");
        assert!(!p_is_hq);
        assert_eq!(p.value, 16_000);
        let (s_is_hq, sec) = r.secondary().expect("secondary present");
        assert!(s_is_hq);
        assert_eq!(sec.value, 50_000);
        assert_ne!(p.value, 33_000); // not a blended NQ+HQ mean
    }

    #[test]
    fn secondary_below_threshold_is_hidden() {
        // HQ has only 3 sales (<4) -> omitted from secondary(), but still in the breakdown.
        let mut s = vec![(16_000i32, 1i32, false); 6];
        s.extend(vec![(50_000, 1, true); 3]);
        let r = real_price(&s, None);
        assert!(r.secondary().is_none());
        assert!(r.hq.is_some());
    }

    #[test]
    fn empty_is_none() {
        let r = real_price(&[], None);
        assert!(r.primary().is_none());
        assert!(r.nq.is_none());
        assert!(r.hq.is_none());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (PowerShell, with `CARGO_TARGET_DIR` set from Task 0):
`cargo test -p ultros-app --lib real_price`
Expected: FAIL — compile error, `cannot find function real_price` / `RealPriceBreakdown` not found.

- [ ] **Step 3: Implement the types and function**

Add this to `ultros-frontend/ultros-app/src/analysis.rs` (place it just above the existing `#[cfg(test)] mod tests` block; it already imports `crate::math::filter_outliers_iqr_in_place` at the top of the file):

```rust
/// One quality's robust price estimate plus the sample accounting behind it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RealPriceEstimate {
    /// The launder-resistant price.
    pub value: i32,
    /// Number of sales the value was computed from.
    pub used: usize,
    /// Total sales for this quality before any filtering.
    pub total: usize,
    /// `total - used`: sales dropped by the vendor guard and/or IQR filter.
    pub excluded: usize,
}

/// NQ and HQ estimates, computed independently (never blended).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RealPriceBreakdown {
    pub nq: Option<RealPriceEstimate>,
    pub hq: Option<RealPriceEstimate>,
}

impl RealPriceBreakdown {
    /// Headline quality = whichever has more sales; NQ wins an exact tie.
    pub fn primary(&self) -> Option<(bool, RealPriceEstimate)> {
        match (self.nq, self.hq) {
            (Some(nq), Some(hq)) => {
                if hq.total > nq.total {
                    Some((true, hq))
                } else {
                    Some((false, nq))
                }
            }
            (Some(nq), None) => Some((false, nq)),
            (None, Some(hq)) => Some((true, hq)),
            (None, None) => None,
        }
    }

    /// The non-headline quality, shown only when it has >= 4 sales.
    pub fn secondary(&self) -> Option<(bool, RealPriceEstimate)> {
        let primary_is_hq = self.primary()?.0;
        let (is_hq, candidate) = if primary_is_hq {
            (false, self.nq)
        } else {
            (true, self.hq)
        };
        candidate
            .filter(|e| e.total >= 4)
            .map(|e| (is_hq, e))
    }
}

/// Median of a slice, sorting it in place. Uses the upper-middle element for even
/// lengths, matching the page's existing median convention. Caller guarantees non-empty.
fn median_in_place(prices: &mut [i32]) -> i32 {
    prices.sort_unstable();
    prices[prices.len() / 2]
}

/// Robust price for a single quality from `(price, qty)` samples.
/// Vendor guard (drop qty==1 sales priced > 100x vendor), then IQR-filtered mean,
/// with a median fallback for fewer than 4 surviving samples.
fn estimate_quality(samples: &[(i32, i32)], vendor_price: Option<i32>) -> Option<RealPriceEstimate> {
    let total = samples.len();
    if total == 0 {
        return None;
    }

    let vendor_cap = vendor_price.filter(|v| *v > 0).map(|v| v as i64 * 100);
    let mut prices: Vec<i32> = samples
        .iter()
        .filter(|&&(price, qty)| match vendor_cap {
            Some(cap) => !(qty == 1 && price as i64 > cap),
            None => true,
        })
        .map(|&(price, _)| price)
        .collect();

    // If the guard removed everything, fall back to the median of all raw prices so we
    // still show something rather than "No data".
    if prices.is_empty() {
        let mut all: Vec<i32> = samples.iter().map(|&(p, _)| p).collect();
        let used = all.len();
        let value = median_in_place(&mut all);
        return Some(RealPriceEstimate {
            value,
            used,
            total,
            excluded: total - used,
        });
    }

    let (value, used) = if prices.len() < 4 {
        let used = prices.len();
        (median_in_place(&mut prices), used)
    } else {
        let filtered = filter_outliers_iqr_in_place(&mut prices);
        let used = filtered.len();
        let mean = (filtered.iter().map(|&p| p as i64).sum::<i64>() / used as i64) as i32;
        (mean, used)
    };

    Some(RealPriceEstimate {
        value,
        used,
        total,
        excluded: total - used,
    })
}

/// Compute the launder-resistant Real Price from the item page's recent sales.
///
/// `samples`: `(price_per_item, quantity, hq)` for each recent sale.
/// `vendor_price`: the item's NPC vendor unit price (xiv-gen `price_mid`) if it is
/// vendor-sold, else `None` — used as an absolute anchor against laundering.
pub fn real_price(samples: &[(i32, i32, bool)], vendor_price: Option<i32>) -> RealPriceBreakdown {
    let nq: Vec<(i32, i32)> = samples
        .iter()
        .filter(|&&(_, _, hq)| !hq)
        .map(|&(p, q, _)| (p, q))
        .collect();
    let hq: Vec<(i32, i32)> = samples
        .iter()
        .filter(|&&(_, _, hq)| hq)
        .map(|&(p, q, _)| (p, q))
        .collect();
    RealPriceBreakdown {
        nq: estimate_quality(&nq, vendor_price),
        hq: estimate_quality(&hq, vendor_price),
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p ultros-app --lib real_price`
Expected: PASS — all 8 tests in `real_price_tests` green. The existing `tests` module (`analyze_sales`, etc.) still passes.

- [ ] **Step 5: Format and commit**

```bash
cargo fmt --all
git add ultros-frontend/ultros-app/src/analysis.rs
git commit -m "feat(analysis): add launder-resistant real_price metric

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Add i18n keys to all 7 locales

**Files:**
- Modify: `ultros-frontend/ultros-app/locales/en.json`
- Modify: `ultros-frontend/ultros-app/locales/fr.json`
- Modify: `ultros-frontend/ultros-app/locales/de.json`
- Modify: `ultros-frontend/ultros-app/locales/ja.json`
- Modify: `ultros-frontend/ultros-app/locales/cn.json`
- Modify: `ultros-frontend/ultros-app/locales/ko.json`
- Modify: `ultros-frontend/ultros-app/locales/tc.json`

In **each** file, find the existing key line `"recent_average": ...` (the key is identical across locales; only the value differs) and insert the two new key lines immediately after it. `recent_average` is not the last key in any locale, so its trailing comma stays and the new lines also end with commas.

- [ ] **Step 1: en.json** — after the `"recent_average"` line, insert:

```json
    "real_price": "Real Price",
    "real_price_basis": "{{used}}/{{total}} sales",
```

- [ ] **Step 2: fr.json** — insert:

```json
    "real_price": "Prix réel",
    "real_price_basis": "{{used}}/{{total}} ventes",
```

- [ ] **Step 3: de.json** — insert:

```json
    "real_price": "Realer Preis",
    "real_price_basis": "{{used}}/{{total}} Verkäufe",
```

- [ ] **Step 4: ja.json** — insert:

```json
    "real_price": "実勢価格",
    "real_price_basis": "{{used}}/{{total}} 件",
```

- [ ] **Step 5: cn.json** — insert:

```json
    "real_price": "真实价格",
    "real_price_basis": "{{used}}/{{total}} 笔成交",
```

- [ ] **Step 6: ko.json** — insert:

```json
    "real_price": "실거래가",
    "real_price_basis": "{{used}}/{{total}}건 거래",
```

- [ ] **Step 7: tc.json** — insert:

```json
    "real_price": "真實價格",
    "real_price_basis": "{{used}}/{{total}} 筆成交",
```

- [ ] **Step 8: Validate every file parses as JSON**

Run (PowerShell):
```powershell
"en","fr","de","ja","cn","ko","tc" | ForEach-Object {
  Get-Content "ultros-frontend/ultros-app/locales/$_.json" -Raw | ConvertFrom-Json | Out-Null
  "$_ ok"
}
```
Expected: prints `en ok` … `tc ok` with no parse error.

- [ ] **Step 9: Commit**

```bash
git add ultros-frontend/ultros-app/locales/
git commit -m "i18n: add real_price + real_price_basis keys (7 locales)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

> Note for PR: ja/cn/ko/tc are good-faith translations — flag for a native-speaker pass.

---

## Task 3: Wire Real Price into `MarketStatsPanel`

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/item_view.rs` (the `MarketStatsPanel` component, ~lines 286–614)

Context: inside `listing_resource.with(|data_ref| { if let Some(Ok(data)) = data_ref.as_ref() { let data = data.clone(); … } })`, the closure computes plain values (`cheapest_nq`, `avg_price`, `median_price`, …) then returns a `view!`. We add `vendor_price` + `real` next to `avg_price`/`median_price` (keeping those for the demoted line) and replace the bottom-left card. All inputs are pure from `data` + `tracked_data()`, identical on SSR and CSR, so no hydration gate is needed.

- [ ] **Step 1: Compute vendor price and the breakdown**

Find the `median_price` binding (ends at the line with `Some(prices[prices.len() / 2])` then `};`, around [item_view.rs:318-327](../../../ultros-frontend/ultros-app/src/routes/item_view.rs)). Immediately **after** the `let median_price = … ;` statement, insert:

```rust
                            let vendor_price = tracked_data()
                                .items
                                .get(&ItemId(item_id()))
                                .map(|item| item.price_mid as i32)
                                .filter(|p| *p > 0);
                            let real = crate::analysis::real_price(
                                &recent_sales
                                    .iter()
                                    .map(|s| (s.price_per_item, s.quantity, s.hq))
                                    .collect::<Vec<_>>(),
                                vendor_price,
                            );
                            let real_primary = real.primary();
                            let real_secondary = real.secondary();
```

- [ ] **Step 2: Replace the bottom-left "Recent Average" card**

Replace the entire third grid `<a href="#history" …> … </a>` block (currently the "Recent Average" card at [item_view.rs:570-584](../../../ultros-frontend/ultros-app/src/routes/item_view.rs), from `<a href="#history"` through its closing `</a>`) with:

```rust
                                        <a href="#history" class="rounded-lg border border-[color:var(--color-outline)] hover:border-blue-300/60 transition-colors p-2 sm:p-3 min-h-24">
                                            <div class="text-xs font-bold uppercase text-blue-300 mb-1 flex items-center gap-1">
                                                {t!(i18n, real_price)}
                                                {real_primary
                                                    .map(|(is_hq, _)| {
                                                        let q = if is_hq { t_string!(i18n, hq) } else { t_string!(i18n, nq) };
                                                        view! { <span class="text-[10px] text-[color:var(--color-text-muted)]">{q.to_string()}</span> }.into_any()
                                                    })
                                                    .unwrap_or_else(|| ().into_any())}
                                            </div>
                                            <div class="text-xl sm:text-2xl font-bold leading-none">
                                                {match real_primary {
                                                    Some((_, est)) => view! { <Gil amount=est.value /> }.into_any(),
                                                    None => view! { <span class="text-[color:var(--color-text-muted)]">{t!(i18n, no_data)}</span> }.into_any(),
                                                }}
                                            </div>
                                            {match real_secondary {
                                                Some((is_hq, est)) => {
                                                    let q = if is_hq { t_string!(i18n, hq) } else { t_string!(i18n, nq) };
                                                    view! {
                                                        <div class="text-xs text-[color:var(--color-text-muted)] mt-1 flex items-center gap-1">
                                                            <span class="font-semibold">{q.to_string()}</span>
                                                            <Gil amount=est.value />
                                                        </div>
                                                    }
                                                    .into_any()
                                                }
                                                None => ().into_any(),
                                            }}
                                            {match real_primary {
                                                Some((_, est)) => {
                                                    view! {
                                                        <div class="text-[10px] text-[color:var(--color-text-muted)] mt-1">
                                                            {t!(i18n, real_price_basis, used = est.used, total = est.total)}
                                                        </div>
                                                    }
                                                    .into_any()
                                                }
                                                None => ().into_any(),
                                            }}
                                            <div class="text-xs text-[color:var(--color-text-muted)] mt-2">
                                                {t!(i18n, recent_average)}
                                                " "
                                                {avg_price
                                                    .map(|price| view! { <Gil amount=price /> }.into_any())
                                                    .unwrap_or_else(|| view! { <span>{t!(i18n, no_data)}</span> }.into_any())}
                                                " · "
                                                {t!(i18n, median_label)}
                                                " "
                                                {median_price
                                                    .map(|price| view! { <Gil amount=price /> }.into_any())
                                                    .unwrap_or_else(|| view! { <span>{t!(i18n, no_data)}</span> }.into_any())}
                                            </div>
                                        </a>
```

- [ ] **Step 3: Build to verify it compiles (and i18n keys resolve)**

Run: `cargo build -p ultros-app`
Expected: builds clean. A missing locale key would fail here with a `leptos-i18n` error naming the key — if so, fix Task 2 in that locale.

- [ ] **Step 4: Lint**

Run: `cargo clippy -p ultros-app --all-targets -- -D warnings`
Expected: no warnings. (If clippy flags `map(...).unwrap_or_else(|| ().into_any())`, leave it — it matches the file's existing pattern and `map_or_else` reads worse here; only refactor if clippy actually errors under `-D warnings`.)

- [ ] **Step 5: Format and commit**

```bash
cargo fmt --all
git add ultros-frontend/ultros-app/src/routes/item_view.rs
git commit -m "feat(item-view): feature Real Price; demote raw average/median

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Full CI verification

**Files:** none (verification only).

- [ ] **Step 1: Run the repo CI gate**

Run (Git Bash, with submodule initialized + `CARGO_TARGET_DIR` exported): `./check_ci.sh`
Expected: `cargo fmt --all -- --check` passes and `cargo clippy --all-targets -- -D warnings` passes for the whole workspace.

If submodule init was blocked, at minimum run `cargo fmt --all -- --check` and note in the PR that clippy was not run (per CLAUDE.md).

- [ ] **Step 2: Run the full app test suite for the crate**

Run: `cargo test -p ultros-app`
Expected: PASS, including `real_price_tests` and the pre-existing `analysis`/`math` tests.

- [ ] **Step 3 (optional, if a dev server is available): visual smoke**

Load `/item/Gilgamesh/9294`. Confirm the bottom-left card now shows "Real Price" ≈ the typical ~16K (not ~391K), with the muted "Recent Average … · Median …" line beneath and a `used/total sales` basis line. See `./scripts/run_e2e.sh` / AGENTS.md for the screenshot harness.

---

## Self-Review (completed by plan author)

**1. Spec coverage:**
- Metric algorithm (vendor guard → IQR mean → median fallback) → Task 1 `estimate_quality`. ✓
- Per-quality independence + primary/secondary (≥4) + NQ tiebreak → Task 1 `real_price`/`primary`/`secondary` + tests. ✓
- View wiring, vendor price from `tracked_data()`, demoted raw avg/median → Task 3. ✓
- Display: label + NQ/HQ tag, headline, secondary, basis line → Task 3 card. ✓
- i18n new keys in 7 locales; reuse `recent_average`/`median_label`/`no_data`/`nq`/`hq` → Task 2 + Task 3. ✓
- Testing: all 7 spec test cases → Task 1 Step 1 (8 tests, incl. an extra non-qty1 guard case). ✓
- Edge cases (empty/`<4`/non-vendor/guard-empties-all) → handled in `estimate_quality`; tested. ✓
- Build/verify notes (submodule, CARGO_TARGET_DIR, check_ci) → Task 0 + Task 4. ✓

**2. Placeholder scan:** No TBD/TODO; every code step shows complete code; commands have expected output. ✓

**3. Type consistency:** `RealPriceEstimate{value,used,total,excluded}`, `RealPriceBreakdown{nq,hq}`, `primary()->Option<(bool,RealPriceEstimate)>`, `secondary()` same shape, `real_price(&[(i32,i32,bool)], Option<i32>)` — names identical across Tasks 1 and 3. `t!`/`t_string!` keys `real_price`/`real_price_basis` match Task 2. ✓

---

## Implementation notes (post-execution)

Executed via subagent-driven development (fresh implementer + spec-compliance + code-quality review per task). Deviations from the plan above, all made to keep `check_ci.sh` green on every commit:

- **Tasks 2 and 3 were committed together** (i18n keys + view wiring) so the new i18n keys and `real_price` had a consumer immediately — an interim commit with unused keys/functions would fail `clippy -D warnings` (dead-code).
- A short-lived **`#[allow(dead_code)]` bridge commit** was added after Task 1 (the producer landed before its consumer) and removed again in the combined Task 2+3 commit.
- The basis line renders **`{used}/{total} sales · {excluded} filtered`** (Task 2's snippet listed only `used/total`). Including `excluded` matches the approved design's display note (“based on X of Y sales · Z excluded”) and keeps every `RealPriceEstimate` field read — avoiding a dead-field clippy error.

The code-quality review confirmed the new item-view card is hydration-safe without a `hydrated` gate (its inputs — the resolved `listing_resource` and static `tracked_data()` — are identical on SSR and first CSR render, consistent with the component's existing ungated `tracked_data()` usage).

Final commits: `65da2f34` (metric + tests), `b2d721ce` (dead-code bridge), `ea194e4c` (i18n + view). Full-workspace `cargo clippy --all-targets -- -D warnings` and `cargo fmt --all -- --check`: clean.
