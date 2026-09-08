# Recipe Planner Travel Frontier Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the recipe planner's "top five routes by rank" cards with a Pareto frontier over (travel, gil): one card per travel shape, shortest trip on the left, cheapest on the right, Stay home always first, with Best value / Cheapest badges and a green "saved vs staying home" line.

**Architecture:** The deterministic engine `ultros-frontend/ultros-app/src/recipe_planner.rs` keeps its route search unchanged and swaps the final "collapse by world set, top five by rank" step for a frontier collapse that returns a `RouteComparison`. The page `ultros-frontend/ultros-app/src/routes/recipe_view.rs` renders those cards, derives badges and subtext through small pure functions, and keeps URL/selection semantics. Strings go through `leptos-i18n` in all seven locales.

**Tech Stack:** Rust, Leptos 0.8 (CSR/SSR), leptos-i18n, Tailwind, Puppeteer e2e in `integration/`.

Spec: `docs/superpowers/specs/2026-09-07-recipe-planner-travel-frontier-design.md`.

## Global Constraints

- Work only inside this worktree. Before every shell step run `git rev-parse --show-toplevel` and confirm it prints `C:/Users/chw11/code/ultros/.claude/worktrees/sweet-bardeen-f8833d` (per `reference_session_worktree_never_registered`).
- Run cargo in the FOREGROUND and re-run on timeout; never background a build (per `reference_subagent_backgrounded_build_returns_early`).
- Windows env for every cargo command (Git Bash): `export PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:$PATH" OPENSSL_RUST_USE_NASM=0 CARGO_PROFILE_DEV_DEBUG=0`.
- Every new user-facing string is a `leptos-i18n` key present in ALL of `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json` with a real translation. `snake_case`, prefixed `recipe_planner_route_`.
- `ROUTE_LIMIT` stays `5`. `rank` stays `(missing, effective, cost, worlds)` and public. The route search (beam, single additions, DC seeds, full scope) is not changed.
- Before committing run `./check_ci.sh > "$SCRATCH/ci.log" 2>&1; echo REAL_EXIT=$?` where `SCRATCH` is the session scratchpad, not `/tmp` (per `reference_shared_tmp_ci_log_collision`). Fix everything it reports.
- Unit tests: `cargo test -p ultros-app --lib <filter>` (default feature `ssr`).
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

---

### Task 1: Frontier collapse in the engine

**Files:**
- Modify: `ultros-frontend/ultros-app/src/recipe_planner.rs` (`TravelWeights` ~line 265, `RouteContext::travel_cost` ~line 333, `compare_routes` ~lines 458-547, tests ~lines 640-1030)

**Interfaces:**
- Consumes: existing `ShoppingPlan`, `Travel`, `TravelWeights`, `rank`, `ROUTE_LIMIT`.
- Produces (Task 2 relies on these exact names):
  - `impl TravelWeights { pub fn distance(&self, travel: Travel) -> i64 }`
  - `pub fn frontier(plans: impl IntoIterator<Item = ShoppingPlan>, weights: &TravelWeights, limit: usize) -> Vec<ShoppingPlan>`
  - `pub struct RouteComparison { pub cards: Vec<ShoppingPlan> }` deriving `Clone, Debug, Default, PartialEq, Eq`, with `baseline(&self) -> Option<&ShoppingPlan>`, `cheapest(&self) -> Option<&ShoppingPlan>`, `best_value(&self) -> Option<usize>`.
  - `pub fn compare_routes(...) -> RouteComparison` (same parameters as today).

- [ ] **Step 1: Write the failing frontier tests**

Add inside `mod tests` in `recipe_planner.rs`, after the `leaf` helper:

```rust
    fn plan(travel: (usize, usize), cost: i64, missing: i64, worlds: &[i32]) -> ShoppingPlan {
        ShoppingPlan {
            travel: Travel {
                dc_hops: travel.0,
                world_hops: travel.1,
            },
            cost,
            missing,
            effective: cost,
            worlds: worlds.iter().copied().collect(),
            ..Default::default()
        }
    }

    #[test]
    fn frontier_keeps_the_cheaper_plan_per_shape() {
        let w = TravelWeights::default();
        let cards = frontier(
            [plan((0, 0), 100, 0, &[]), plan((0, 1), 90, 0, &[2]), plan((0, 1), 80, 0, &[3])],
            &w,
            5,
        );
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[1].worlds, BTreeSet::from([3]));
    }

    #[test]
    fn frontier_drops_further_and_dearer_but_keeps_further_and_cheaper() {
        let w = TravelWeights::default();
        let cards = frontier(
            [
                plan((0, 0), 100, 0, &[]),
                plan((0, 1), 80, 0, &[2]),
                plan((0, 2), 85, 0, &[2, 3]),
                plan((1, 0), 60, 0, &[9]),
            ],
            &w,
            5,
        );
        let costs: Vec<i64> = cards.iter().map(|p| p.cost).collect();
        assert_eq!(costs, vec![100, 80, 60]);
    }

    #[test]
    fn frontier_always_keeps_the_baseline() {
        let w = TravelWeights::default();
        // Nothing beats home.
        let cards = frontier([plan((0, 0), 100, 0, &[]), plan((0, 1), 100, 0, &[2])], &w, 5);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].travel, Travel::default());
        // Home is incomplete and dearer-looking plans that complete are kept.
        let cards = frontier([plan((0, 0), 10, 3, &[]), plan((0, 1), 500, 0, &[2])], &w, 5);
        assert_eq!(cards.len(), 2);
        assert_eq!((cards[0].missing, cards[1].missing), (3, 0));
    }

    #[test]
    fn frontier_trims_to_the_limit_keeping_ends_and_largest_savings() {
        let w = TravelWeights::default();
        // Seven strict steps; marginal savings: 1, 50, 2, 40, 3, 30.
        let cards = frontier(
            [
                plan((0, 0), 1000, 0, &[]),
                plan((0, 1), 999, 0, &[2]),
                plan((0, 2), 949, 0, &[2, 3]),
                plan((0, 3), 947, 0, &[2, 3, 4]),
                plan((0, 4), 907, 0, &[2, 3, 4, 5]),
                plan((1, 0), 904, 0, &[9]),
                plan((1, 1), 874, 0, &[9, 10]),
            ],
            &w,
            5,
        );
        let costs: Vec<i64> = cards.iter().map(|p| p.cost).collect();
        assert_eq!(costs, vec![1000, 949, 907, 904, 874]);
    }

    #[test]
    fn frontier_orders_by_travel_weight_then_hops() {
        let three_hops = plan((0, 3), 80, 0, &[2, 3, 4]);
        let one_dc = plan((1, 0), 70, 0, &[9]);
        let home = plan((0, 0), 100, 0, &[]);
        // Default weights: 3 world hops (6,000) sort before 1 DC hop (10,000).
        let cards = frontier(
            [home.clone(), three_hops.clone(), one_dc.clone()],
            &TravelWeights::default(),
            5,
        );
        assert_eq!(cards.iter().map(|p| p.cost).collect::<Vec<_>>(), vec![100, 80, 70]);
        // A cheap DC hop sorts first, and then 3 world hops are dearer-and-further.
        let cheap_dc = TravelWeights {
            world_hop: 2_000,
            dc_hop: 1_000,
        };
        let cards = frontier([home, three_hops, one_dc], &cheap_dc, 5);
        assert_eq!(cards.iter().map(|p| p.cost).collect::<Vec<_>>(), vec![100, 70]);
        assert_eq!(cheap_dc.distance(Travel { dc_hops: 1, world_hops: 2 }), 5_000);
    }

    #[test]
    fn best_value_follows_rank_not_position() {
        let mut middle = plan((0, 1), 80, 0, &[2]);
        middle.effective = 82;
        let mut last = plan((1, 0), 70, 0, &[9]);
        last.effective = 170;
        let c = RouteComparison {
            cards: vec![plan((0, 0), 100, 0, &[]), middle, last],
        };
        assert_eq!(c.best_value(), Some(1));
        assert_eq!(c.baseline().map(|p| p.cost), Some(100));
        assert_eq!(c.cheapest().map(|p| p.cost), Some(70));
        assert_eq!(RouteComparison::default().best_value(), None);
    }
```

- [ ] **Step 2: Run the tests to verify they fail to compile**

Run: `cargo test -p ultros-app --lib recipe_planner::tests::frontier 2>&1 | tail -20`
Expected: compile errors: `cannot find function frontier`, `cannot find type RouteComparison`, no method `distance`.

- [ ] **Step 3: Implement `distance`, `frontier`, `RouteComparison`, and switch `compare_routes`**

Add to `impl TravelWeights` (create the impl block right after `impl Default for TravelWeights`):

```rust
impl TravelWeights {
    /// Gil-equivalent distance of a travel shape: the card ordering axis and
    /// the travel part of `effective`.
    pub fn distance(&self, travel: Travel) -> i64 {
        (travel.dc_hops as i64)
            .saturating_mul(self.dc_hop)
            .saturating_add((travel.world_hops as i64).saturating_mul(self.world_hop))
    }
}
```

Replace the body of `RouteContext::travel_cost` with `self.weights.distance(travel)`.

Replace the whole tail of `compare_routes` from the comment `// Several allowed sets usually collapse ...` to the end of the function with:

```rust
    RouteComparison {
        cards: frontier(pool.into_values(), &ctx.weights, ROUTE_LIMIT),
    }
}

/// The route cards: a Pareto frontier over (travel distance, gil), shortest
/// trip first. `cards[0]` is the no-new-travel baseline and the last card is
/// the cheapest plan found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RouteComparison {
    pub cards: Vec<ShoppingPlan>,
}

impl RouteComparison {
    pub fn baseline(&self) -> Option<&ShoppingPlan> {
        self.cards.first()
    }

    pub fn cheapest(&self) -> Option<&ShoppingPlan> {
        self.cards.last()
    }

    /// Index of the card `rank` prefers given the travel weights: the
    /// default selection and the "Best value" badge.
    pub fn best_value(&self) -> Option<usize> {
        (0..self.cards.len()).min_by_key(|i| rank(&self.cards[*i]))
    }
}

/// Collapse candidate plans onto the travel frontier. One card per travel
/// shape (the plan with the fewest missing units, then the least gil), sorted
/// by `weights.distance` then hop counts, keeping only cards that strictly
/// improve on the card to their left (fewer missing, or equal missing and
/// less gil). The baseline shape (no new travel) has distance zero and sorts
/// first, so it is always kept. Longer frontiers are trimmed to `limit`
/// (at least two) keeping the first and last cards and then the steps with
/// the largest marginal improvement over their left neighbour.
pub fn frontier(
    plans: impl IntoIterator<Item = ShoppingPlan>,
    weights: &TravelWeights,
    limit: usize,
) -> Vec<ShoppingPlan> {
    let mut by_shape: BTreeMap<Travel, ShoppingPlan> = BTreeMap::new();
    for plan in plans {
        let better = by_shape
            .get(&plan.travel)
            .is_none_or(|cur| (plan.missing, plan.cost, &plan.worlds) < (cur.missing, cur.cost, &cur.worlds));
        if better {
            by_shape.insert(plan.travel, plan);
        }
    }
    let mut shapes: Vec<ShoppingPlan> = by_shape.into_values().collect();
    shapes.sort_by_key(|p| (weights.distance(p.travel), p.travel));
    let mut kept: Vec<ShoppingPlan> = Vec::new();
    for plan in shapes {
        let improves = kept
            .last()
            .is_none_or(|last| (plan.missing, plan.cost) < (last.missing, last.cost));
        if improves {
            kept.push(plan);
        }
    }
    let limit = limit.max(2);
    if kept.len() <= limit {
        return kept;
    }
    let last = kept.len() - 1;
    // Marginal improvement over the left neighbour: completing more of the
    // recipe outranks saving gil. Ties keep the shorter trip.
    let mut middle: Vec<(usize, (i64, i64))> = (1..last)
        .map(|i| {
            (
                i,
                (
                    kept[i - 1].missing - kept[i].missing,
                    kept[i - 1].cost - kept[i].cost,
                ),
            )
        })
        .collect();
    middle.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut keep: BTreeSet<usize> = middle.into_iter().take(limit - 2).map(|(i, _)| i).collect();
    keep.insert(0);
    keep.insert(last);
    kept.into_iter()
        .enumerate()
        .filter(|(i, _)| keep.contains(i))
        .map(|(_, p)| p)
        .collect()
}
```

Update the `compare_routes` doc comment to:

```rust
/// Best-found routes collapsed onto the travel frontier (see `frontier`).
/// Single-world additions are exhaustive; larger routes come from a bounded
/// beam plus whole-datacenter and full-scope seeds, so the UI calls these
/// best-found, not optimal. The no-travel plan is always evaluated and is
/// always the first card; the full-scope plan is always evaluated, and since
/// allowing more worlds never raises gil, the cheapest plan is always found
/// and is the last card.
```

- [ ] **Step 4: Update the existing `compare_routes` tests to the new return type**

Apply these edits in `mod tests`:

In the test around line 671 (asserting `plans[0].cost == 40`), replace from `let plans = compare_routes(` through the end of that test's assertions with:

```rust
        let c = compare_routes(
            &lines,
            &market,
            &BTreeMap::new(),
            &ctx(1, &[(1, 1), (2, 1)]),
        );
        // The incomplete home plan is the baseline; the complete plan is the
        // best value even though home looks cheaper.
        assert_eq!(c.cards[0].missing, 1);
        let best = &c.cards[c.best_value().unwrap()];
        assert_eq!((best.cost, best.missing), (40, 0));
        assert_eq!(best.worlds, BTreeSet::from([2]));
        assert_eq!(
            best.travel,
            Travel {
                dc_hops: 0,
                world_hops: 1
            }
        );
```

In `full_scope_can_reuse_a_better_home_plan_for_large_stacks`:

```rust
        let c = compare_routes(&materials, &market, &BTreeMap::new(), &ctx(1, &[]));
        // The best value is never worse than the no-travel plan, even when the
        // large-batch greedy path gets worse with more offers to choose from.
        assert_eq!(c.best_value(), Some(0));
        let home = c.baseline().unwrap();
        assert_eq!(home.cost, 20_002);
        assert!(home.worlds.is_empty());
        assert!(home.approximate);
```

In `datacenter_weight_can_outrank_a_cheaper_foreign_listing`, replace each `let plans = compare_routes(...)` + `plans[0]` block with:

```rust
        let c = compare_routes(&materials, &market, &BTreeMap::new(), &c_ctx);
        let best = &c.cards[c.best_value().unwrap()];
        assert_eq!(best.worlds, BTreeSet::from([2]));
        assert_eq!((best.cost, best.effective), (1_000, 1_100));
        // Both travel shapes are on the frontier regardless of weights.
        assert_eq!(c.cards.len(), 3);
```
and for the zero-weight half:
```rust
        let c = compare_routes(&materials, &market, &BTreeMap::new(), &c_ctx);
        assert_eq!(c.cards[c.best_value().unwrap()].worlds, BTreeSet::from([3]));
```
(rename the test's `c` context variable to `c_ctx` so the comparison can be `c`).

In `locked_offer_is_kept_counted_and_makes_its_world_free`:
```rust
        let c = compare_routes(&materials, &market, &BTreeMap::new(), &c_ctx);
        let best = &c.cards[c.best_value().unwrap()];
```
(then the existing assertions on `best`, with the context variable renamed to `c_ctx`).

In `unavailable_pair_is_skipped_without_blocking_the_world`:
```rust
        let c = compare_routes(&materials, &market, &BTreeMap::new(), &c_ctx);
        assert!(
            c.cards
                .iter()
                .flat_map(|p| p.purchases.get(&1))
                .flat_map(|p| &p.offers)
                .all(|o| o.world != 2)
        );
        let best = &c.cards[c.best_value().unwrap()];
        assert_eq!(best.purchases[&1].offers[0].id, 12);
        assert_eq!(best.purchases[&2].offers[0].id, 21);
        assert_eq!(best.missing, 0);
```

Replace `routes_are_distinct_ranked_and_capped` entirely with:

```rust
    #[test]
    fn same_shape_routes_collapse_and_home_stays_best_value_for_small_savings() {
        let materials = [leaf(1, 1)];
        let market = BTreeMap::from([(
            1,
            (1..=8)
                .map(|w| offer(w, w, 1, 100 - i64::from(w)))
                .collect::<Vec<_>>(),
        )]);
        let c_ctx = ctx(1, &(1..=8).map(|w| (w, 10)).collect::<Vec<_>>());
        let c = compare_routes(&materials, &market, &BTreeMap::new(), &c_ctx);
        // Seven one-hop options collapse to the cheapest; two hops never help
        // a one-unit purchase, so the frontier is home and one hop.
        assert_eq!(c.cards.len(), 2);
        assert!(c.cards[0].worlds.is_empty());
        assert_eq!(c.cards[0].cost, 99);
        assert_eq!(c.cards[1].worlds, BTreeSet::from([8]));
        assert_eq!(c.cheapest().map(|p| p.cost), Some(92));
        // A few gil never pays for a 2,000 gil hop.
        assert_eq!(c.best_value(), Some(0));
    }
```

In `a_single_world_scope_yields_one_route`:
```rust
        let c = compare_routes(&materials, &market, &BTreeMap::new(), &ctx(1, &[(1, 10)]));
        assert_eq!(c.cards.len(), 1);
        assert_eq!(c.cards[0].cost, 10);
```

In `ranking_is_deterministic_under_ties_and_input_order`, keep `assert_eq!(a, b);` and replace the last assertion with:
```rust
        // Equal gil and travel: the lower world set wins.
        assert_eq!(a.cards[a.best_value().unwrap()].worlds, BTreeSet::from([2]));
```

- [ ] **Step 5: Run the engine tests**

Run: `cargo test -p ultros-app --lib recipe_planner 2>&1 | tail -30`
Expected: all `recipe_planner::tests` pass. (The `routes/recipe_view.rs` call site still fails to compile until Task 2; if the whole lib fails to build because of it, temporarily confirm with `cargo check -p ultros-app 2>&1 | grep -c "compare_routes"` and proceed to Task 2 before committing. Do not commit a non-compiling tree.)

- [ ] **Step 6: Do not commit yet** — Task 2 must land in the same commit because `recipe_view.rs` consumes `compare_routes`. Continue.

---

### Task 2: Render the frontier cards

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_view.rs` (`legacy_visits` ~line 132, `route_stops` ~line 380, memos ~lines 584-635, card section ~lines 771-790, details paragraph ~line 867, tests ~lines 995-1015)
- Modify: `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json` (route keys ~lines 1939-1946)

**Interfaces:**
- Consumes: `planner::RouteComparison`, `planner::frontier` (not directly), `planner::rank`, `TravelWeights::distance` from Task 1.
- Produces: `enum SavingLine`, `fn saving_line(baseline: &planner::ShoppingPlan, plan: &planner::ShoppingPlan, is_baseline: bool, cards: usize) -> Option<SavingLine>`, `struct Cards` memo value; e2e-visible text "Stay home", "Best value", "Cheapest", "saved vs staying home".

- [ ] **Step 1: Add the locale keys**

In every locale file, replace the two heading values and add six keys directly after `recipe_planner_route_home_only`. Keep JSON valid (commas).

`en.json`:
```json
    "recipe_planner_route_heading": "Stay home, or travel?",
    "recipe_planner_route_subheading": "Shortest trip on the left, cheapest on the right · best-found",
    ...
    "recipe_planner_route_home_only": "Everything from your starting world",
    "recipe_planner_route_best_value": "Best value",
    "recipe_planner_route_cheapest": "Cheapest",
    "recipe_planner_route_saved_vs_home": "{{gil}} saved vs staying home",
    "recipe_planner_route_completes": "Completes the recipe",
    "recipe_planner_route_just_stay_home": "Just stay home.",
    "recipe_planner_route_more_worlds": "+{{count}} more",
```

`fr.json`:
```json
    "recipe_planner_route_heading": "Rester chez soi ou voyager ?",
    "recipe_planner_route_subheading": "Trajet le plus court à gauche, le moins cher à droite · meilleur trouvé",
    "recipe_planner_route_best_value": "Meilleur rapport",
    "recipe_planner_route_cheapest": "Le moins cher",
    "recipe_planner_route_saved_vs_home": "{{gil}} économisés par rapport à rester chez soi",
    "recipe_planner_route_completes": "Complète la recette",
    "recipe_planner_route_just_stay_home": "Restez simplement chez vous.",
    "recipe_planner_route_more_worlds": "+{{count}} autres",
```

`de.json`:
```json
    "recipe_planner_route_heading": "Zu Hause bleiben oder reisen?",
    "recipe_planner_route_subheading": "Kürzeste Reise links, günstigste rechts · beste gefundene",
    "recipe_planner_route_best_value": "Bestes Verhältnis",
    "recipe_planner_route_cheapest": "Günstigste",
    "recipe_planner_route_saved_vs_home": "{{gil}} gespart gegenüber Zuhausebleiben",
    "recipe_planner_route_completes": "Vervollständigt das Rezept",
    "recipe_planner_route_just_stay_home": "Bleib einfach zu Hause.",
    "recipe_planner_route_more_worlds": "+{{count}} weitere",
```

`ja.json`:
```json
    "recipe_planner_route_heading": "ホームに留まるか、移動するか？",
    "recipe_planner_route_subheading": "左が最短移動、右が最安 · 見つかった最良ルート",
    "recipe_planner_route_best_value": "バランス最良",
    "recipe_planner_route_cheapest": "最安",
    "recipe_planner_route_saved_vs_home": "ホームに留まるより {{gil}} の節約",
    "recipe_planner_route_completes": "レシピを完成できます",
    "recipe_planner_route_just_stay_home": "そのままホームにいましょう。",
    "recipe_planner_route_more_worlds": "他 {{count}} ワールド",
```

`cn.json`:
```json
    "recipe_planner_route_heading": "留在本服，还是出发？",
    "recipe_planner_route_subheading": "左侧旅程最短，右侧价格最低 · 找到的最佳方案",
    "recipe_planner_route_best_value": "最划算",
    "recipe_planner_route_cheapest": "最便宜",
    "recipe_planner_route_saved_vs_home": "比留在本服节省 {{gil}}",
    "recipe_planner_route_completes": "可凑齐全部材料",
    "recipe_planner_route_just_stay_home": "还是留在本服吧。",
    "recipe_planner_route_more_worlds": "另外 {{count}} 个",
```

`ko.json`:
```json
    "recipe_planner_route_heading": "홈에 머무를까, 이동할까?",
    "recipe_planner_route_subheading": "왼쪽이 최단 이동, 오른쪽이 최저가 · 찾은 최적 경로",
    "recipe_planner_route_best_value": "최고 가치",
    "recipe_planner_route_cheapest": "최저가",
    "recipe_planner_route_saved_vs_home": "홈에 머무를 때보다 {{gil}} 절약",
    "recipe_planner_route_completes": "레시피를 완성할 수 있음",
    "recipe_planner_route_just_stay_home": "그냥 홈에 머무르세요.",
    "recipe_planner_route_more_worlds": "외 {{count}}개",
```

`tc.json`:
```json
    "recipe_planner_route_heading": "留在本服，還是出發？",
    "recipe_planner_route_subheading": "左側旅程最短，右側價格最低 · 找到的最佳方案",
    "recipe_planner_route_best_value": "最划算",
    "recipe_planner_route_cheapest": "最便宜",
    "recipe_planner_route_saved_vs_home": "比留在本服節省 {{gil}}",
    "recipe_planner_route_completes": "可湊齊全部材料",
    "recipe_planner_route_just_stay_home": "還是留在本服吧。",
    "recipe_planner_route_more_worlds": "另外 {{count}} 個",
```

Verify: `for l in en fr de ja cn ko tc; do node -e "JSON.parse(require('fs').readFileSync('ultros-frontend/ultros-app/locales/$l.json','utf8'))" && grep -c recipe_planner_route_ ultros-frontend/ultros-app/locales/$l.json; done` prints `13` seven times with no parse error.

- [ ] **Step 2: Write the failing UI tests**

Add to `mod tests` in `recipe_view.rs`:

```rust
    #[test]
    fn saving_line_frames_each_card_against_the_baseline() {
        let plan = |cost: i64, missing: i64| planner::ShoppingPlan {
            cost,
            missing,
            ..Default::default()
        };
        let home = plan(1_000, 0);
        assert_eq!(
            saving_line(&home, &home, true, 1),
            Some(SavingLine::Baseline { alone: true })
        );
        assert_eq!(
            saving_line(&home, &home, true, 3),
            Some(SavingLine::Baseline { alone: false })
        );
        assert_eq!(
            saving_line(&home, &plan(800, 0), false, 2),
            Some(SavingLine::Saved(200))
        );
        assert_eq!(
            saving_line(&home, &plan(800, 2), false, 2),
            Some(SavingLine::Partial(2))
        );
        assert_eq!(
            saving_line(&plan(10, 4), &plan(900, 0), false, 2),
            Some(SavingLine::Completes)
        );
        // A pinned shared route can be dearer than home: no claim is made.
        assert_eq!(saving_line(&home, &plan(1_200, 0), false, 3), None);
    }
```

Also change the existing `legacy_visits_selects_by_world_count_when_no_route_is_given` so plans carry a distinct `cost` too (rank reads `effective` first, so expectations are unchanged):

```rust
        let plan = |worlds: &[i32], effective: i64| planner::ShoppingPlan {
            worlds: worlds.iter().copied().collect(),
            effective,
            cost: effective,
            ..Default::default()
        };
        // Frontier order: home first, then by distance.
        let plans = vec![
            plan(&[], 300),
            plan(&[63], 200),
            plan(&[63, 79], 100),
            plan(&[63, 79, 80], 400),
        ];
        assert_eq!(legacy_visits(&plans, 0), Some(0));
        assert_eq!(legacy_visits(&plans, 1), Some(1));
        assert_eq!(legacy_visits(&plans, 2), Some(2));
        assert_eq!(legacy_visits(&plans, 4), Some(2));
        assert_eq!(legacy_visits(&[], 0), None);
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p ultros-app --lib routes::recipe_view 2>&1 | tail -20`
Expected: compile error `cannot find function saving_line` / `SavingLine`.

- [ ] **Step 4: Implement the pure helpers**

Replace `legacy_visits` with:

```rust
/// Legacy `visits=N` links: the best-ranked card visiting at most `visits`
/// extra worlds. `4` was "full scope" and means the best-ranked card overall.
fn legacy_visits(plans: &[planner::ShoppingPlan], visits: usize) -> Option<usize> {
    plans
        .iter()
        .enumerate()
        .filter(|(_, p)| visits >= 4 || p.worlds.len() <= visits)
        .min_by_key(|(_, p)| planner::rank(p))
        .map(|(i, _)| i)
}
```

Add after `plan_summary`:

```rust
/// The card's comparison line against the no-new-travel baseline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SavingLine {
    /// The baseline card itself; `alone` when no travel route beats it.
    Baseline { alone: bool },
    /// Complete on both sides and this card is cheaper by this much gil.
    Saved(i64),
    /// The baseline is short and this card completes the recipe.
    Completes,
    /// This card is still short by this many units.
    Partial(i64),
}

fn saving_line(
    baseline: &planner::ShoppingPlan,
    plan: &planner::ShoppingPlan,
    is_baseline: bool,
    cards: usize,
) -> Option<SavingLine> {
    if is_baseline {
        return Some(SavingLine::Baseline { alone: cards == 1 });
    }
    if plan.missing > 0 {
        return Some(SavingLine::Partial(plan.missing));
    }
    if baseline.missing > 0 {
        return Some(SavingLine::Completes);
    }
    (baseline.cost > plan.cost).then(|| SavingLine::Saved(baseline.cost - plan.cost))
}
```

- [ ] **Step 5: Run the UI unit tests**

Run: `cargo test -p ultros-app --lib routes::recipe_view 2>&1 | tail -20`
Expected: `saving_line_frames_each_card_against_the_baseline` and `legacy_visits_...` pass (other compile errors from the `compare_routes` call site may remain; fix them in the next step).

- [ ] **Step 6: Rewire the memos to `RouteComparison`**

Add near the other small structs at module level (above `pub fn RecipeView` or wherever `plan_summary` lives):

```rust
/// What the card row renders: frontier cards, plus a pinned shared route
/// inserted by travel distance, and the badge positions after insertion.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Cards {
    plans: Vec<planner::ShoppingPlan>,
    pinned: Option<usize>,
    best_value: Option<usize>,
    cheapest: Option<usize>,
}
```

Replace the `plans` memo (the one calling `planner::compare_routes`) so it returns `planner::RouteComparison`:

```rust
    let comparison = Memo::new(move |_| {
        if loaded.get().is_none() || home_id.get() == 0 {
            return planner::RouteComparison::default();
        }
        materials
            .get()
            .map(|m| planner::compare_routes(&m, &offers.get(), &vendors.get(), &context.get()))
            .unwrap_or_default()
    });
```

Replace the `cards` memo with:

```rust
    // Frontier cards plus, when a shared link names a route that is not on
    // the frontier, that route as a pinned card inserted by travel distance.
    // Badges follow the frontier's best-value and cheapest cards by identity.
    let cards = Memo::new(move |_| {
        let comparison = comparison.get();
        let best_worlds = comparison
            .best_value()
            .map(|i| comparison.cards[i].worlds.clone());
        let cheapest_worlds = comparison.cheapest().map(|p| p.worlds.clone());
        let mut plans = comparison.cards;
        let mut pinned = None;
        let unranked = route_set(route.get().as_deref())
            .filter(|set| !plans.is_empty() && !plans.iter().any(|p| p.worlds == *set));
        if let (Some(mut allowed), Ok(m)) = (unranked, materials.get()) {
            allowed.insert(home_id.get());
            let ctx = context.get();
            let plan = planner::shop(&m, &offers.get(), &vendors.get(), &allowed, &ctx);
            let key = (ctx.weights.distance(plan.travel), plan.travel);
            let at = plans.partition_point(|p| (ctx.weights.distance(p.travel), p.travel) <= key);
            plans.insert(at, plan);
            pinned = Some(at);
        }
        let find = |worlds: Option<BTreeSet<i32>>| {
            worlds.and_then(|w| plans.iter().position(|p| p.worlds == w))
        };
        let badges = if plans.len() > 1 { (find(best_worlds), find(cheapest_worlds)) } else { (None, None) };
        Cards {
            plans,
            pinned,
            best_value: badges.0,
            cheapest: badges.1,
        }
    });
```

Replace `selected_index` and `selected`:

```rust
    let selected_index = Memo::new(move |_| {
        let cards = cards.get();
        if cards.plans.is_empty() {
            return None;
        }
        let default = cards.best_value.or(Some(0));
        if let Some(set) = route_set(route.get().as_deref()) {
            return cards
                .plans
                .iter()
                .position(|p| p.worlds == set)
                .or(cards.pinned)
                .or(default);
        }
        visits
            .get()
            .and_then(|v| legacy_visits(&cards.plans, v))
            .or(default)
    });
    let selected = Memo::new(move |_| {
        selected_index
            .get()
            .and_then(|i| cards.get().plans.get(i).cloned())
    });
```

Note: `best_value` is `None` for a single-card row (no badges), which is why `.or(Some(0))` is kept.

- [ ] **Step 7: Truncate long stop lists**

Replace `route_stops` with:

```rust
    let route_stops = move |worlds: &BTreeSet<i32>| {
        const SHOWN: usize = 6;
        let mut by_dc: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for w in worlds.iter().take(SHOWN) {
            by_dc.entry(dc_name(*w)).or_default().push(world_name(*w));
        }
        let mut text = by_dc
            .into_iter()
            .map(|(dc, worlds)| format!("{dc} · {}", worlds.join(", ")))
            .collect::<Vec<_>>()
            .join(" / ");
        if worlds.len() > SHOWN {
            text.push_str(" · ");
            text.push_str(
                &t_string!(
                    i18n,
                    recipe_planner_route_more_worlds,
                    count = worlds.len() - SHOWN
                )
                .to_string(),
            );
        }
        text
    };
```

- [ ] **Step 8: Render the cards**

Replace the card `Suspense` body (the `{move || { let (cards,pinned)=cards.get(); ... }}` block inside `<section aria-label="World visit comparison">`) with:

```rust
                    {move || {
                        let Cards{plans,pinned,best_value,cheapest}=cards.get();
                        let total=plans.len();
                        let baseline=plans.first().cloned().unwrap_or_default();
                        plans.into_iter().enumerate().map(|(index,p)| {
                            let mut label=route_label(p.travel,p.worlds.is_empty());
                            if pinned==Some(index) { label=format!("{} · {label}",t_string!(i18n, recipe_planner_shared_route)); }
                            let line=saving_line(&baseline,&p,index==0,total);
                            let stops=if p.worlds.is_empty() { t_string!(i18n, recipe_planner_route_home_only).to_string() } else { route_stops(&p.worlds) };
                            let badge=match (best_value==Some(index),cheapest==Some(index)) {
                                (true,true)=>Some((format!("{} · {}",t_string!(i18n, recipe_planner_route_best_value),t_string!(i18n, recipe_planner_route_cheapest)),true)),
                                (true,false)=>Some((t_string!(i18n, recipe_planner_route_best_value).to_string(),true)),
                                (false,true)=>Some((t_string!(i18n, recipe_planner_route_cheapest).to_string(),false)),
                                (false,false)=>None,
                            };
                            let worlds=p.worlds.clone();
                            view!{<button class="panel rounded-xl p-4 text-left space-y-1 hover:border-brand-500 focus-visible:ring-2 focus-visible:ring-brand-400" class:border-brand-400=move ||selected_index.get()==Some(index) aria-pressed=move ||(selected_index.get()==Some(index)).to_string() on:click=move |_|{ set_route.set(Some(write_route(&worlds))); set_visits.set(None); }>
                                <span class="flex flex-wrap items-center justify-between gap-2"><span class="text-sm text-[color:var(--color-text-muted)]">{label}</span>{badge.map(|(text,brand)|view!{<span class="rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide" class:bg-brand-500/20=brand class:text-brand-300=brand class:bg-[color:var(--color-outline)]=!brand class:text-[color:var(--color-text-muted)]=!brand data-testid="route-badge">{text}</span>})}</span>
                                <strong class="block text-xl tabular-nums">{gil(p.cost)}</strong>
                                <span class="block text-xs">{match line { Some(SavingLine::Partial(n))=>format!("{n} units unavailable · partial cost"), _=>stops }}</span>
                                {match line {
                                    Some(SavingLine::Saved(s))=>Some(view!{<span class="block text-xs text-emerald-400">{t_string!(i18n, recipe_planner_route_saved_vs_home, gil = gil(s)).to_string()}</span>}),
                                    Some(SavingLine::Completes)=>Some(view!{<span class="block text-xs text-emerald-400">{t!(i18n, recipe_planner_route_completes)}</span>}),
                                    Some(SavingLine::Baseline{alone:true})=>Some(view!{<span class="block text-xs text-emerald-400">{t!(i18n, recipe_planner_route_just_stay_home)}</span>}),
                                    _=>None,
                                }}
                            </button>}
                        }).collect_view()
                    }}
```

If `class:bg-brand-500/20=brand` does not parse in `view!` (the `/` in a class name), switch the badge to two prebuilt class strings instead:

```rust
let badge_class = if brand { "rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide bg-brand-500/20 text-brand-300" } else { "rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide bg-[color:var(--color-outline)] text-[color:var(--color-text-muted)]" };
```
and use `class=badge_class`. Both literal strings must appear in source so Tailwind emits them.

If `t_string!(..., gil = gil(s))` rejects a `String` argument, bind `let saved = gil(s);` first and pass `gil = saved`; if it still rejects, pass `gil = saved.as_str()`.

- [ ] **Step 9: Update the in-page calculation details paragraph**

Replace the sentence starting `"Routes are ranked by gil plus a travel cost per world hop ...` (inside the `<details>` near line 867) with:

```
"Route cards are the travel frontier: one card per travel shape, shortest trip on the left, each card cheaper than the one before it, and the cheapest plan found always last. Best value is the card the gil-plus-travel weighting prefers (adjustable in Planner settings). Adding a single world is checked exhaustively; larger routes search promising combinations, so they are best-found, not guaranteed global minima. Worlds already on your itinerary are free to revisit. Only market worlds are counted; vendor stops are separate."
```

- [ ] **Step 10: Build and run all frontend unit tests**

Run: `cargo test -p ultros-app --lib 2>&1 | tail -30`
Expected: all tests pass, zero warnings about unused `ROUTE_LIMIT`/`rank`.

Run: `cargo check -p ultros-app --no-default-features --features hydrate 2>&1 | tail -5`
Expected: clean (the client build compiles the same view code).

- [ ] **Step 11: Commit Tasks 1 and 2 together**

```bash
git add ultros-frontend/ultros-app/src/recipe_planner.rs ultros-frontend/ultros-app/src/routes/recipe_view.rs ultros-frontend/ultros-app/locales/*.json
git commit -m "feat(recipe-planner): travel-frontier route cards

One card per travel shape, shortest trip on the left, each strictly cheaper than the last, the cheapest plan always last and Stay home always first. Best value and Cheapest badges; every travel card shows what it saves vs staying home, and a lone home card says Just stay home.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: E2E assertions, docs, changelog, CI

**Files:**
- Modify: `integration/recipe-planner.cjs` (~lines 104-118)
- Modify: `docs/recipe-planner.md` (the "Routes are ranked by ..." paragraph)
- Create: `ultros-changelog/changes/2026-09-07-recipe-planner-travel-frontier.json`

**Interfaces:**
- Consumes: DOM text "Stay home", "Cheapest", "saved vs staying home", `data-testid="route-badge"` from Task 2.

- [ ] **Step 1: Extend the e2e probe**

In `integration/recipe-planner.cjs`, directly after the line `assert.ok(stayHome >= 0, 'a Stay home card must be offered');` add:

```js
    assert.equal(stayHome, 0, 'Stay home is always the first card');
    assert.equal(await page.$$eval(`${cardSelector} [data-testid="route-badge"]`, badges => badges.filter(b => b.textContent.includes('Cheapest')).length), 1, 'exactly one card is badged Cheapest');
    assert.ok(await page.$$eval(cardSelector, buttons => buttons.at(-1).textContent.includes('Cheapest')), 'the cheapest card is the last card');
```

Directly after `assert.ok(routeCard >= 0, 'fixtures on two worlds must offer a one-hop route');` add:

```js
      assert.ok(await page.$$eval(cardSelector, (buttons, i) => buttons[i].textContent.includes('saved vs staying home'), routeCard), 'the cheaper hop card shows its saving vs staying home');
```

(The fixture prices world 79 at 50 gil against 100 gil at home, so the hop card is cheaper.)

- [ ] **Step 2: Update `docs/recipe-planner.md`**

Replace the paragraph beginning `Routes are ranked by \`(missing, effective, cost, worlds)\`` through `...labels routes **best-found**, not globally optimal.` with:

```
Route cards are the travel frontier. Each candidate plan has a travel shape
(datacenter hops and world hops beyond the worlds already on the itinerary)
and a distance, the gil-equivalent travel cost from the craft-options cookie
(`world_hop_gil`, default 2,000; `dc_hop_gil`, default 10,000; adjustable in
Planner settings). The engine keeps the best plan per shape, sorts shapes by
distance, and keeps only cards that strictly improve on the card to their
left (fewer missing units, or equal missing and less gil). "Stay home" (the
no-new-travel plan) is therefore always the first card, and because the full
scope is always evaluated and more worlds never cost more gil, the cheapest
plan found is always the last card, however many hops it takes. At most five
cards are shown; longer frontiers keep the first and last cards and the steps
with the largest marginal saving. The card `rank` `(missing, effective, cost,
worlds)` prefers is badged **Best value** and is the default selection; the
last card is badged **Cheapest**. The search evaluates every single-world
addition exhaustively, then a beam of five promising sets for larger routes,
plus whole-datacenter and full-scope seeds, so the UI labels routes
**best-found**, not globally optimal.
```

Also, in the URL state paragraph, change `Absent means the best-ranked route.` to `Absent means the Best value card.` and `"the first ranked route with at most that many extra worlds"` to `"the best-ranked card with at most that many extra worlds"`.

- [ ] **Step 3: Add the changelog entry**

Create `ultros-changelog/changes/2026-09-07-recipe-planner-travel-frontier.json`:

```json
{
  "category": "features",
  "importance": "medium",
  "title": "Recipe planner routes read left to right: stay home, or travel?",
  "blurb": "The recipe planner's route cards now line up by trip length: Stay home is always first, every card to the right is a real saving over the one before it, and the cheapest plan found is always last, even if it takes a dozen hops for a big bulk order. Each travel card says how much it saves over staying home, the gil-plus-travel pick is badged Best value, the last card Cheapest, and if nothing beats your own market board the planner just tells you to stay home.",
  "link": "/recipe/5057"
}
```

- [ ] **Step 4: Run CI checks**

```bash
SCRATCH="C:/Users/chw11/AppData/Local/Temp/claude/C--Users-chw11-code-ultros--claude-worktrees-sweet-bardeen-f8833d/6b08ee75-0fa6-401b-a0a2-8d9fbc6d3e8f/scratchpad"
./check_ci.sh > "$SCRATCH/ci.log" 2>&1; echo "REAL_EXIT=$?"; tail -30 "$SCRATCH/ci.log"
```
Expected: `REAL_EXIT=0`. Fix any fmt/clippy finding (run `cargo fmt --all` for formatting) and re-run until clean.

- [ ] **Step 5: Run the focused e2e probe**

Build and serve this worktree's own server, then run the probe (see `docs/recipe-planner.md` Validation; Windows needs `--bin-features test-auth` and its own `METRICS_PORT`):

```bash
cargo leptos build --bin-features test-auth 2>&1 | tail -5
```
Then start the server from `target/` per `scripts/run_e2e.sh` conventions and run:
```bash
npm --prefix integration run test:recipe-planner
```
Expected: passes, including the three new assertions. If the server cannot be started locally (DB pool exhaustion or port 8080 held by another session per `reference_ultros_port_8080_contention`), record that in the PR body and rely on CI's e2e job.

- [ ] **Step 6: Commit**

```bash
git add integration/recipe-planner.cjs docs/recipe-planner.md ultros-changelog/changes/2026-09-07-recipe-planner-travel-frontier.json
git commit -m "docs(recipe-planner): frontier cards in docs, changelog and e2e

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

- [ ] **Step 7: Open the PR**

```bash
git push -u origin claude/recipe-tool-baseline-135d6b
gh pr create --title "Recipe planner: travel-frontier route cards" --body "$(cat <<'EOF'
## What
Route cards are now the Pareto frontier over (travel, gil): one card per travel shape, shortest trip on the left, each card strictly cheaper than the last, Stay home always first and the cheapest plan always last. Best value / Cheapest badges; every travel card shows its saving vs staying home; a lone home card says "Just stay home."

Spec: docs/superpowers/specs/2026-09-07-recipe-planner-travel-frontier-design.md

## Why
#1319's top-five-by-rank cards could show several near-identical routes, lose the Stay home baseline (and with it the savings line), and drop the cheapest route when it needed many hops.

## Verification
- `./check_ci.sh` clean
- `cargo test -p ultros-app --lib` (new frontier + saving_line tests)
- `npm --prefix integration run test:recipe-planner` (state result here)

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```
