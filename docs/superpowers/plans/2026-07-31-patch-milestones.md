# Patch Milestone Bands Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement spec 4 of the chart revamp (`docs/superpowers/specs/2026-07-26-game-history-milestones-design.md`): per-track patch/expansion background bands on the price chart, LOD-filtered by zoom, off (with a caption reason) when the visible series span multiple patch tracks.

**Architecture:** A checked-in seed table (`ultros-api-types::game_history`) is the source of truth — append-only historical facts, readable by both the server (a `/api/v1/game-history` endpoint for external consumers) and the WASM chart directly (no fetch). The chart layouts take a prepared `Vec<MilestoneSpec>`; the app builds it from the seed via region→track mapping and the zoom-tier filter.

**Base branch:** `origin/main` (spec 4 is structurally independent).

**Decisions pinned here (deviations from the spec, all caused by PR #1033 landing after the spec was written, or by repo query rules):**

- **No expansion-name labels in v1.** The spec's "ExVersion costs nothing" premise predates #1033: the rkyv packs are now prebuilt LFS artifacts whose shape depends on enabled xiv-gen features, so `ex_version` needs a `game-data-pack` sheet addition plus regenerated packs for seven languages — a separate PR. Bands are hue-coded per expansion and labeled with patch numbers ("7.2"), which carry the era information. Same reason defers the spec's `PatchMark` cross-check test.
- **Coverage-start milestone deferred.** "Earliest sale we hold for the scope's worlds at all" is an unfiltered `min(sold_date)` over `sales`, off the `(item_id, …)` primary-key prefix — exactly the query shape this repo's ClickHouse rules exist to prevent. Needs its own design (probably a tiny rollup). "First recorded sale" needs nothing: the chart already starts at the first data bucket.
- **CN/KR tracks ship empty.** The seed can be incomplete but never wrong; my CN/KR patch dates are not confident enough to hand-enter. Global is seeded majors 2.0–7.3 plus the Dawntrail-era point patches. Appending is a one-line diff; flagged in the PR.
- **Grid cells don't render bands** (cells are ~150px tall; bands would be noise). Overlay + density only.

---

### Task 1: `game_history` module in `ultros-api-types`

**Files:** Create `ultros-api-types/src/game_history.rs`; register in `lib.rs`.

Contents:

```rust
pub enum PatchTrack { Global, China, Korea }         // Copy, serde lowercase
pub struct GamePatch { pub track, pub version: u16,  // 700 = 7.0, 715 = 7.15
                       pub released: NaiveDate, pub ex_version: u8 }
pub const GAME_PATCHES: &[GamePatch]                 // Global 2.0..=7.3 majors + 6.55/7.05/7.15/7.25
pub fn track_for_region(region_name: &str) -> PatchTrack   // "中国"→China, "한국"→Korea, else Global
pub fn patches_for_track(track) -> impl Iterator<Item = &'static GamePatch>
pub enum MarkTier { ExpansionsOnly, Major, Point, None }   // from visible span
pub fn mark_tier(span_secs: i64) -> MarkTier   // >2y / 6mo..2y / 30d..6mo / <30d
pub fn visible_patches(track, span_secs) -> Vec<&'static GamePatch>
pub fn version_label(version: u16) -> String   // 700→"7.0", 715→"7.15", 705→"7.05"
```

Seed dates (Global; all verified-in-memory major releases): 200/2013-08-27, 210/2013-12-17, 220/2014-03-27, 230/2014-07-08, 240/2014-10-28, 250/2015-02-24, 300/2015-06-23, 310/2015-11-10, 320/2016-02-23, 330/2016-06-07, 340/2016-09-27, 350/2017-01-17, 400/2017-06-20, 410/2017-10-10, 420/2018-01-30, 430/2018-05-22, 440/2018-09-18, 450/2019-01-08, 500/2019-07-02, 510/2019-10-29, 520/2020-02-18, 530/2020-08-11, 540/2020-12-08, 550/2021-04-13, 600/2021-12-07, 610/2022-04-12, 620/2022-08-23, 630/2023-01-10, 640/2023-05-23, 650/2023-10-03, 655/2024-01-16, 700/2024-07-02, 705/2024-07-30, 710/2024-11-12, 715/2024-12-17, 720/2025-03-25, 725/2025-05-27, 730/2025-08-05. `ex_version` = major digit − 2 (ARR=0 … DT=5).

Tests: track mapping incl. invented-region fallback; seed sorted by `(track, released)` with unique `(track, version)`; `ex_version` consistent with `version / 100 - 2`; mark_tier boundaries; visible_patches per tier; version_label formats; serde round-trip of `GamePatch`.

### Task 2: `/api/v1/game-history` endpoint

**Files:** `ultros/src/web.rs`.

`GET /api/v1/game-history?track=` → JSON of the (optionally filtered) seed, `Cache-Control: public, max-age=86400`. A few KB, changes ~4×/year. Register route next to `price_density`.

### Task 3: Band rendering in the chart layouts

**Files:** `ultros-frontend/ultros-charts/src/theme.rs` (expansion hue table), `charts/price_history.rs`, `charts/price_density.rs`.

- `Theme` gains `expansion_hues: Vec<Color>` (6 muted hues, one per expansion).
- New shared type in `charts/mod.rs`:

```rust
pub struct MilestoneSpec {
    pub start: chrono::NaiveDateTime,   // patch release, UTC midnight
    pub version: u16,
    pub ex_version: u8,
}
```

- `PriceChartOptions.milestones: Vec<MilestoneSpec>` (empty = off) and `DensityChartOptions.milestones`.
- **Overlay:** immediately after the scene's geometry is known and BEFORE grid lines are pushed: bands tile `[first_ts, last_ts]` — band N runs from `max(spec.start, first_ts)` to `min(next spec.start, last_ts)`, plus a leading band from `first_ts` to the first in-window boundary using the latest patch *before* the window. `Node::Rect` per band, hue = `expansion_hues[ex_version % len]`, alpha alternating 0.05/0.09 by band parity within the expansion; vertical `Node::Line` at expansion starts (`version % 100 == 0` and start inside the window); `Node::Text` label (`version_label`) at band centre when the band is ≥ 48px wide, drawn at `plot_top + 12`.
- **Density:** boundary lines only (every spec start inside the window), zero band rects.

Tests: bands tile the window with no gaps/overlaps (sum of widths ≈ plot width); band nodes precede every data node; consecutive bands differ in alpha, different expansions differ in hue; ≥48px bands carry a label; density emits lines and zero rects; empty `milestones` renders identically to before (regression).

### Task 4: i18n + app wiring

**Files:** locales ×7, `components/chart_toolbar.rs`, `components/price_history_chart.rs`.

- Keys: `chart_toggle_patches` (en "Patch bands"; fr "Bandes de patch"; de "Patch-Bänder"; ja "パッチ帯"; cn "版本区间"; ko "패치 구간"; tc "版本區間") and `chart_milestones_mixed_tracks` (en "Patch bands are off: the visible series span regions on different patch schedules."; fr "Les bandes de patch sont désactivées : les séries visibles couvrent des régions aux calendriers de patch différents."; de "Patch-Bänder sind aus: die sichtbaren Serien umfassen Regionen mit unterschiedlichen Patch-Zeitplänen."; ja "パッチ帯は無効です：表示中の系列は異なるパッチ日程のリージョンにまたがっています。"; cn "版本区间已关闭：可见系列跨越了版本日程不同的区域。"; ko "패치 구간이 꺼져 있습니다: 표시 중인 시리즈가 서로 다른 패치 일정의 지역에 걸쳐 있습니다."; tc "版本區間已關閉：可見系列跨越了版本日程不同的區域。")
- Toolbar: fourth `OverlayRow` "Patch bands" (`show_patches`, default **on**, disabled never — bands vanish naturally under 30 days per the LOD table).
- `PriceHistoryChart`:
  - `milestone_track: Memo<Option<PatchTrack>>` — at `GroupLevel::Region`, map every *visible* series name through `track_for_region`; >1 distinct → `None` (off + caption reason). Otherwise resolve the scope's region name (world→dc→region walk via the helper) → its track.
  - `milestones: Memo<Vec<MilestoneSpec>>` — empty when toggle off or track `None`; else `visible_patches(track, span)` over the selected/available domain, mapped to specs (UTC midnight).
  - Pass into both the overlay `model` memo and `density_model`; caption appends the mixed-tracks reason when the toggle is on but the track is `None`.

### Task 5: Verification

Charts + api-types tests green; fmt; `cargo clippy --all-targets -j 16 -- -D warnings`; push; PR against main. Browser pass deferred (no ClickHouse locally), same note as #1042/#1046.

## Self-review notes

Spec coverage: seed-first data model ✅ · region→track as data with Global fallback ✅ · endpoint with long cache ✅ · bands per patch, hue per expansion, alternating lightness, expansion-boundary lines, centre labels ✅ (minus expansion-name row — deferred, documented) · LOD mark tiers ✅ · multi-track → off with caption reason ✅ · density degrades to boundary lines ✅ · bands behind data (draw-order test) ✅ · zero new i18n keys for expansion names ✅ (trivially — no names) · honesty milestones: first-sale is the existing data-domain start; coverage-start deferred with rationale ✅ · poller explicitly deferred by the spec itself ✅.
