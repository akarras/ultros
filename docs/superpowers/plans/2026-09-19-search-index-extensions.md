# Search Index Extensions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make currencies searchable for real (bug fix), add gil-shop NPCs, venture rewards and crafted scrip turn-ins to the global search, with deep links that highlight the matching row on the venture / scrip pages, and a "kind nudge" so `<name> recipe` / `<name> venture` lifts that record type.

**Architecture:** Three pure `&xiv_gen::Data` helpers live in a new `ultros-app::game_sources` module and are used by both the pages and the tantivy indexer in `ultros/src/search_service.rs`, so page and index can never disagree about what a currency/venture/turn-in is. The index gains a searchable `kind` field and per-type weights. `VirtualGrid` gains an optional `reveal_index` prop (forwarded through `QueryGrid` → `MarketGrid`) that reuses the keyboard-navigation `reveal()`; the two pages derive it once per `?item=` value from the rows the grid hands back via `on_rows`.

**Tech Stack:** Rust nightly-2026-08-20, tantivy 0.26, Leptos 0.8 (SSR + hydrate), leptos-i18n (7 locales), Puppeteer e2e in `integration/`.

## Global Constraints

- Before every commit run `./check_ci.sh > "$CLAUDE_SCRATCHPAD/ci.log" 2>&1; echo "REAL_EXIT=$?"` (fmt + clippy `-D warnings`). Log to the scratchpad, never `/tmp`.
- Set `CARGO_PROFILE_DEV_DEBUG=0` and `OPENSSL_RUST_USE_NASM=0` in the environment for every cargo command (Windows: also prepend `/c/Strawberry/perl/bin:/c/Strawberry/c/bin:` to `PATH` in Git Bash).
- Every new user-facing string goes through `leptos-i18n`: add the key to **all seven** of `ultros-frontend/ultros-i18n/locales/{en,fr,de,ja,cn,ko,tc}.json` with a real translation.
- Spec: `docs/superpowers/specs/2026-09-19-search-index-extensions-design.md`. Type weights: recipe 0.8, scrip source 0.75, venture 0.7, everything else 1.0. `SEARCH_CANDIDATES` = 60. `kind` exact boost starts at 2.0.
- Deep-link URLs: `/npc/:id`, `/venture-analyzer?item=:id`, `/scrip-sources?item=:id`, `/scrip-sources?scrip=OrangeCrafters|PurpleCrafters`.
- Test commands: backend `cargo test -p ultros --lib search_service`; app helpers `cargo test -p ultros-app --lib game_sources`; grid `cargo test -p ultros-ui-grid --lib`.
- Commit messages end with `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`.

---

## File structure

| File | Responsibility |
|---|---|
| `ultros-frontend/ultros-app/src/game_sources.rs` (new) | Pure `&Data` → records: `exchange_currencies`, `venture_rewards`, re-export of scrip helpers. No Leptos. |
| `ultros-frontend/ultros-app/src/lib.rs` | `pub mod game_sources;` |
| `ultros-frontend/ultros-app/src/routes/currency_exchange.rs` | `CurrencySelection` calls `exchange_currencies`. |
| `ultros-frontend/ultros-app/src/routes/venture_analyzer.rs` | Row loop iterates `venture_rewards`; `?item=` reveal. |
| `ultros-frontend/ultros-app/src/routes/scrip_sources.rs` | `ScripType`, `ScripTurnIn`, `scrip_turn_ins` become `pub`; `ScripType::filter_key`, `ScripType::item_name`, `scrip_item`; `?item=` reveal. |
| `ultros-frontend/ultros-app/src/components/reveal_once.rs` (new) | `RevealOnce` — one reveal per `?item=` value. |
| `ultros/src/search_service.rs` | Currency fix, `kind` field, npc / venture / scrip docs, weights, tests. |
| `ultros-frontend/ultros-ui-grid/src/components/virtual_grid/{mod.rs,query_grid.rs}` | `reveal_index` prop. |
| `ultros-frontend/ultros-app/src/analyzer_kit/market.rs` | Forward `reveal_index`. |
| `ultros-frontend/ultros-ui-shell/src/components/search_box.rs` | i18n type labels, icons, static tools, hints. |
| `ultros-frontend/ultros-i18n/locales/*.json` | New keys. |
| `integration/search-deep-links.cjs` (new) | e2e: `?item=` highlights the row. |

---

### Task 1: `game_sources::exchange_currencies` shared by page and (later) index

**Files:**
- Create: `ultros-frontend/ultros-app/src/game_sources.rs`
- Modify: `ultros-frontend/ultros-app/src/lib.rs:16` (add `pub mod game_sources;` next to `pub(crate) mod routes;`)
- Modify: `ultros-frontend/ultros-app/src/routes/currency_exchange.rs:834-878` (`CurrencySelection`)

**Interfaces:**
- Produces: `pub fn exchange_currencies(data: &xiv_gen::Data) -> Vec<xiv_gen::ItemId>` — sorted ascending by id, unique, Gil/MGP excluded, UI category ∈ {100, 61, 63}.
- Produces: `pub const CURRENCY_UI_CATEGORIES: [ItemUiCategoryId; 3]`.

- [ ] **Step 1: Write the failing tests**

```rust
// ultros-frontend/ultros-app/src/game_sources.rs
//! Game-data record sets shared by the pages that display them and the
//! search index that indexes them (`ultros/src/search_service.rs`).
//!
//! Pure functions over `&xiv_gen::Data`, no Leptos: the indexer runs on the
//! server at startup and the pages run in the browser, and both must agree
//! on what a currency, a venture reward or a scrip turn-in *is*. The currency
//! search was wrong for a long time precisely because the indexer re-derived
//! the page's logic instead of calling it.

use xiv_gen::{Data, ItemId, ItemUiCategoryId};

/// `ItemUICategory` rows a special-shop cost can sit in and still count as a
/// currency: Currency = 100, Miscellany = 61, Other = 63. Row ids are stable
/// across game locales; the `name` column is not (GlitchTip #6849).
pub const CURRENCY_UI_CATEGORIES: [ItemUiCategoryId; 3] = [
    ItemUiCategoryId(100),
    ItemUiCategoryId(61),
    ItemUiCategoryId(63),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> &'static Data {
        xiv_gen_db::data()
    }

    fn name(id: ItemId) -> &'static str {
        data().items.get(&id).map(|i| i.name.as_str()).unwrap_or("")
    }

    #[test]
    fn exchange_currencies_are_the_cost_side_not_the_received_side() {
        let currencies = exchange_currencies(data());
        let names: Vec<_> = currencies.iter().map(|id| name(*id)).collect();
        // Paid in special shops that sell marketable goods.
        assert!(names.contains(&"Wolf Mark"), "{names:?}");
        assert!(names.contains(&"Fae Fancy"), "{names:?}");
        // Received from a special shop; sits in the Currency UI category but
        // is never a cost. This is what the old indexer listed instead.
        assert!(!names.contains(&"Blue Crafters' Scrip Token"), "{names:?}");
    }

    #[test]
    fn exchange_currencies_exclude_gil_and_mgp_and_are_unique_sorted() {
        let currencies = exchange_currencies(data());
        let names: Vec<_> = currencies.iter().map(|id| name(*id)).collect();
        assert!(!names.contains(&"Gil"));
        assert!(!names.contains(&"MGP"));
        let mut sorted = currencies.clone();
        sorted.sort_by_key(|id| id.0);
        sorted.dedup();
        assert_eq!(currencies, sorted);
    }
}
```

- [ ] **Step 2: Register the module and run the tests to verify they fail**

Add `pub mod game_sources;` to `ultros-frontend/ultros-app/src/lib.rs` after line 16.

Run: `cargo test -p ultros-app --lib game_sources`
Expected: compile error `cannot find function exchange_currencies`.

- [ ] **Step 3: Implement `exchange_currencies`**

Add above the tests in `game_sources.rs`:

```rust
/// Items the special shops take as payment for something marketable — the
/// set the Currency Exchange landing page lists.
///
/// Walks every special-shop row, keeps rows whose *received* item is
/// marketable, and collects the *cost* items of those rows (three cost slots
/// per row). `SpecialShop.item` is the received item, not the cost — reading
/// it here is how the search index once listed "Blue Crafters' Scrip Token"
/// as a currency and never "Wolf Mark".
pub fn exchange_currencies(data: &Data) -> Vec<ItemId> {
    let marketable = |id: u16| {
        id != 0
            && data
                .items
                .get(&ItemId(id as i32))
                .is_some_and(|item| item.item_search_category != 0)
    };
    let mut currencies: Vec<ItemId> = data
        .special_shops
        .values()
        .flat_map(|shop| {
            (0..shop.item_receive_0.len()).flat_map(move |row| {
                let sells_marketable = shop.item_receive_0.get(row).is_some_and(|&id| marketable(id))
                    || shop.item_receive_1.get(row).is_some_and(|&id| marketable(id));
                let costs = [
                    (shop.item_cost_0.get(row), shop.count_cost_0.get(row)),
                    (shop.item_cost_1.get(row), shop.count_cost_1.get(row)),
                    (shop.item_cost_2.get(row), shop.count_cost_2.get(row)),
                ];
                costs.into_iter().filter_map(move |(item, count)| {
                    let (&item, &count) = (item?, count?);
                    (sells_marketable && item != 0 && count != 0).then_some(ItemId(item as i32))
                })
            })
        })
        .filter(|id| {
            data.items.get(id).is_some_and(|item| {
                CURRENCY_UI_CATEGORIES.contains(&ItemUiCategoryId(item.item_ui_category))
                    && item.name != "Gil"
                    && item.name != "MGP"
            })
        })
        .collect();
    currencies.sort_by_key(|id| id.0);
    currencies.dedup();
    currencies
}
```

Check `Item.item_ui_category`'s type in `xiv-gen/src/lib.rs` (grep `pub item_ui_category`); if it is not `i32`, cast with `as i32` inside `ItemUiCategoryId(...)`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ultros-app --lib game_sources`
Expected: 2 passed.

- [ ] **Step 5: Make `CurrencySelection` use it**

In `currency_exchange.rs`, replace lines 837–864 (from `let ui_categories = ...` through `.collect::<Vec<_>>();`) with:

```rust
    let ui_categories = &data.item_ui_categorys;
    let currencies = crate::game_sources::exchange_currencies(data);
```

and the following block (`let items = &data.items; let currencies = currencies.into_iter().sorted_by_key(...)...`) becomes:

```rust
    let items = &data.items;
    let currencies = currencies
        .into_iter()
        .filter_map(|c| {
            let item = items.get(&c)?;
            let ui_category = ItemUiCategoryId(item.item_ui_category as i32);
            let category = ui_categories.get(&ui_category)?;
            Some((item.key_id.0, item.name.as_str(), category.name.as_str()))
        })
        .collect::<Vec<_>>();
```

Delete the now-unused `disallowed_items` and `allowed_item_ui_categories` locals. If `shop_items`/`ItemUiCategoryId`/`Itertools` imports become unused, clippy will say so — remove only what it flags.

- [ ] **Step 6: Build and lint**

Run: `cargo check -p ultros-app --features ssr && ./check_ci.sh > "$CLAUDE_SCRATCHPAD/ci.log" 2>&1; echo "REAL_EXIT=$?"; tail -20 "$CLAUDE_SCRATCHPAD/ci.log"`
Expected: `REAL_EXIT=0`.

- [ ] **Step 7: Commit**

```bash
git add ultros-frontend/ultros-app/src/game_sources.rs ultros-frontend/ultros-app/src/lib.rs ultros-frontend/ultros-app/src/routes/currency_exchange.rs
git commit -m "refactor(currency): share exchange_currencies between page and index

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 2: `game_sources::venture_rewards`

**Files:**
- Modify: `ultros-frontend/ultros-app/src/game_sources.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/venture_analyzer.rs:302-330` (task loop)

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct VentureReward {
      pub task: xiv_gen::RetainerTaskId,
      pub item: ItemId,
      pub quantity: i32,
      pub level: u8,
      pub class_job_category: xiv_gen::ClassJobCategoryId,
  }
  pub fn venture_rewards(data: &Data) -> Vec<VentureReward>  // sorted by task id
  ```

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests`:

```rust
    #[test]
    fn venture_rewards_skip_random_and_empty_tasks_and_are_sorted() {
        let rewards = venture_rewards(data());
        assert!(!rewards.is_empty());
        for reward in &rewards {
            let task = &data().retainer_tasks[&reward.task];
            assert!(!task.is_random, "random task {} listed", reward.task.0);
            assert!(reward.item.0 != 0 && reward.quantity != 0);
            assert!(data().items.contains_key(&reward.item));
        }
        let ids: Vec<_> = rewards.iter().map(|r| r.task.0).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted);
    }

    #[test]
    fn a_known_hunting_venture_is_listed() {
        // Coeurl Skin is a classic hunting venture reward.
        let rewards = venture_rewards(data());
        assert!(rewards.iter().any(|r| name(r.item) == "Coeurl Skin"));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p ultros-app --lib game_sources`
Expected: compile error `cannot find function venture_rewards`.

- [ ] **Step 3: Implement**

Add to `game_sources.rs`:

```rust
use xiv_gen::{ClassJobCategoryId, RetainerTaskId, RetainerTaskNormalId};

/// One non-random retainer venture and what it brings back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VentureReward {
    pub task: RetainerTaskId,
    pub item: ItemId,
    /// Base quantity (`RetainerTaskNormal.Quantity[0]`).
    pub quantity: i32,
    pub level: u8,
    pub class_job_category: ClassJobCategoryId,
}

/// Every fixed-reward venture, sorted by task id so callers and SSR agree on
/// order. Random ventures (quick explorations) have no fixed item and are
/// skipped, as are placeholder rows with no item or quantity.
pub fn venture_rewards(data: &Data) -> Vec<VentureReward> {
    let mut rewards: Vec<VentureReward> = data
        .retainer_tasks
        .iter()
        .filter(|(_, task)| !task.is_random)
        .filter_map(|(id, task)| {
            let normal = data
                .retainer_task_normals
                .get(&RetainerTaskNormalId(task.task))?;
            if normal.item == 0 || normal.quantity_0 == 0 {
                return None;
            }
            data.items.get(&ItemId(normal.item))?;
            Some(VentureReward {
                task: *id,
                item: ItemId(normal.item),
                quantity: normal.quantity_0,
                level: task.retainer_level,
                class_job_category: ClassJobCategoryId(task.class_job_category),
            })
        })
        .collect();
    rewards.sort_by_key(|r| r.task.0);
    rewards
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ultros-app --lib game_sources`
Expected: 4 passed.

- [ ] **Step 5: Make the venture analyzer iterate it**

In `venture_analyzer.rs`, the loop at lines 302–330 currently reads:

```rust
        for (task_id, task) in retainer_tasks.iter() {
            if task.is_random { continue; }
            if let Some(ids) = &selected_ids && !ids.contains(&task.class_job_category) { continue; }
            let normal_id = xiv_gen::RetainerTaskNormalId(task.task);
            if let Some(normal_task) = retainer_task_normals.get(&normal_id) {
                let item_id = normal_task.item;
                if item_id == 0 { continue; }
                let quantity = normal_task.quantity_0;
                if quantity == 0 { continue; }
                let task_level = task.retainer_level as i32;
                ... (pricing, push VentureProfitData { task_id: task_id.0, task_level, item_id, quantity, ... })
            }
        }
```

Replace the head of the loop so the body is unchanged from `// Market Price` onward:

```rust
        for reward in crate::game_sources::venture_rewards(data) {
            if let Some(ids) = &selected_ids
                && !ids.contains(&reward.class_job_category.0)
            {
                continue;
            }
            let task_id = reward.task;
            let item_id = reward.item.0;
            let quantity = reward.quantity;
            let task_level = reward.level as i32;
            {
                // Market Price
                ...
```

Close the extra `{` where the old `if let Some(normal_task)` block closed. Remove the now-unused `retainer_task_normals` local (line 236). Keep `retainer_tasks` — the `categories` memo still uses it.

- [ ] **Step 6: Build, lint, commit**

Run: `cargo check -p ultros-app --features ssr && ./check_ci.sh > "$CLAUDE_SCRATCHPAD/ci.log" 2>&1; echo "REAL_EXIT=$?"`
Expected: `REAL_EXIT=0`.

```bash
git add ultros-frontend/ultros-app/src/game_sources.rs ultros-frontend/ultros-app/src/routes/venture_analyzer.rs
git commit -m "refactor(ventures): share venture_rewards between page and index

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 3: Expose the scrip helpers and add `filter_key` / `scrip_item`

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/scrip_sources.rs:84-187, 240-275`
- Modify: `ultros-frontend/ultros-app/src/game_sources.rs`

**Interfaces:**
- Produces (re-exported from `game_sources`):
  ```rust
  pub enum ScripType { OrangeCrafters, OrangeGatherers, WhiteCrafters, PurpleCrafters, WhiteGatherers, PurpleGatherers, Other(u32) }
  impl ScripType {
      pub fn from_filter_key(key: &str) -> Option<Self>;
      pub fn filter_key(self) -> Option<&'static str>;      // inverse; None for Other
      pub fn is_gatherer(&self) -> bool;
      pub fn item_name(self) -> Option<&'static str>;       // "Orange Crafters' Scrip" etc.
  }
  pub struct ScripTurnIn { pub item_id: i32, pub scrip_type: ScripType, pub scrip_amount: u32 }
  pub fn scrip_turn_ins(data: &Data) -> Vec<ScripTurnIn>;
  pub fn scrip_item(data: &Data, scrip: ScripType) -> Option<ItemId>;
  ```

- [ ] **Step 1: Write the failing tests (in `game_sources.rs`)**

```rust
    #[test]
    fn filter_key_round_trips() {
        for scrip in [
            ScripType::OrangeCrafters,
            ScripType::OrangeGatherers,
            ScripType::PurpleCrafters,
            ScripType::PurpleGatherers,
            ScripType::WhiteCrafters,
            ScripType::WhiteGatherers,
        ] {
            let key = scrip.filter_key().expect("named scrip has a key");
            assert_eq!(ScripType::from_filter_key(key), Some(scrip));
        }
        assert_eq!(ScripType::Other(9).filter_key(), None);
    }

    #[test]
    fn every_live_crafter_scrip_resolves_to_one_currency_item() {
        for scrip in [ScripType::OrangeCrafters, ScripType::PurpleCrafters] {
            let id = scrip_item(data(), scrip).expect("scrip item exists");
            let item = &data().items[&id];
            assert_eq!(Some(item.name.as_str()), scrip.item_name());
            assert_eq!(ItemUiCategoryId(item.item_ui_category as i32), ItemUiCategoryId(100));
        }
        assert_eq!(scrip_item(data(), ScripType::Other(9)), None);
    }

    #[test]
    fn crafted_turn_ins_exist_and_gatherer_ones_are_marked() {
        let turn_ins = scrip_turn_ins(data());
        assert!(turn_ins.iter().any(|t| !t.scrip_type.is_gatherer()));
        assert!(turn_ins.iter().any(|t| t.scrip_type.is_gatherer()));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p ultros-app --lib game_sources`
Expected: compile errors about `ScripType`, `scrip_item`, `scrip_turn_ins` not found.

- [ ] **Step 3: Make the scrip page's types public and add the two methods**

In `scrip_sources.rs`:
- `enum ScripType` → `pub enum ScripType` (line 85).
- `fn from_filter_key` → `pub fn from_filter_key`; `fn is_gatherer` → `pub fn is_gatherer`.
- `struct ScripTurnIn` → `pub struct ScripTurnIn` with `pub` on its three fields (line ~188).
- `fn scrip_turn_ins` → `pub fn scrip_turn_ins` (line 240).
- Add to `impl ScripType` after `from_filter_key`:

```rust
    /// Inverse of [`from_filter_key`](Self::from_filter_key): the `?scrip=`
    /// value that selects this type, or `None` for a currency index the page
    /// doesn't name.
    pub fn filter_key(self) -> Option<&'static str> {
        Some(match self {
            ScripType::OrangeCrafters => "OrangeCrafters",
            ScripType::OrangeGatherers => "OrangeGatherers",
            ScripType::WhiteCrafters => "WhiteCrafters",
            ScripType::PurpleCrafters => "PurpleCrafters",
            ScripType::WhiteGatherers => "WhiteGatherers",
            ScripType::PurpleGatherers => "PurpleGatherers",
            ScripType::Other(_) => return None,
        })
    }

    /// English item name of the scrip currency itself, for looking the item
    /// up in the (English) game-data pack. The white scrips were retired in
    /// 7.0 and have no item.
    pub fn item_name(self) -> Option<&'static str> {
        Some(match self {
            ScripType::OrangeCrafters => "Orange Crafters' Scrip",
            ScripType::OrangeGatherers => "Orange Gatherers' Scrip",
            ScripType::PurpleCrafters => "Purple Crafters' Scrip",
            ScripType::PurpleGatherers => "Purple Gatherers' Scrip",
            ScripType::WhiteCrafters
            | ScripType::WhiteGatherers
            | ScripType::Other(_) => return None,
        })
    }
```

In `game_sources.rs` add:

```rust
pub use crate::routes::scrip_sources::{ScripTurnIn, ScripType, scrip_turn_ins};

/// The scrip currency item for `scrip`, matched by name inside the Currency
/// UI category (the only place the exact name is unambiguous).
pub fn scrip_item(data: &Data, scrip: ScripType) -> Option<ItemId> {
    let name = scrip.item_name()?;
    data.items
        .values()
        .filter(|item| ItemUiCategoryId(item.item_ui_category as i32) == ItemUiCategoryId(100))
        .find(|item| item.name == name)
        .map(|item| item.key_id)
}
```

`routes` is `pub(crate)` in `lib.rs`; re-exporting `pub` items from it through a `pub mod` is allowed.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ultros-app --lib game_sources`
Expected: 7 passed. If `every_live_crafter_scrip_resolves_to_one_currency_item` fails on the name, grep the pack: `cargo test -p ultros-app --lib game_sources -- --nocapture` after temporarily printing `data().items.values().filter(|i| i.name.contains("Scrip")).map(|i| &i.name)`; fix `item_name` to the pack's spelling and remove the print.

- [ ] **Step 5: Lint and commit**

Run: `./check_ci.sh > "$CLAUDE_SCRATCHPAD/ci.log" 2>&1; echo "REAL_EXIT=$?"`
Expected: `REAL_EXIT=0`.

```bash
git add ultros-frontend/ultros-app/src/game_sources.rs ultros-frontend/ultros-app/src/routes/scrip_sources.rs
git commit -m "refactor(scrips): expose ScripType and scrip_turn_ins for the search index

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 4: Fix currency indexing

**Files:**
- Modify: `ultros/src/search_service.rs:207-262` (the `// Index Currencies` block) and tests.

- [ ] **Step 1: Write the failing test**

Append inside `mod tests` of `search_service.rs`:

```rust
    fn titles_of(results: &[SearchResult], result_type: &str) -> Vec<String> {
        results
            .iter()
            .filter(|r| r.result_type == result_type)
            .map(|r| r.title.clone())
            .collect()
    }

    /// Currencies are what a special shop *takes*, not what it hands out.
    /// `SpecialShop.item` is the received item, and indexing it listed
    /// "Blue Crafters' Scrip Token" as a currency while "Wolf Mark" was
    /// unreachable.
    #[test]
    fn currencies_are_what_the_shops_take() {
        let service = SearchService::new().expect("index builds from embedded data");
        for name in ["Wolf Mark", "Fae Fancy"] {
            let results = service.search(name);
            assert!(
                titles_of(&results, "currency").iter().any(|t| t == name),
                "{name:?} not a currency result: {:?}",
                results.iter().map(|r| (&r.title, &r.result_type)).collect::<Vec<_>>()
            );
        }
        let results = service.search("Blue Crafters' Scrip Token");
        assert!(titles_of(&results, "currency").is_empty(), "{results:?}");
    }

    /// Every currency the exchange page lists is reachable by its full name.
    #[test]
    fn every_exchange_currency_is_findable() {
        let service = SearchService::new().expect("index builds from embedded data");
        let data = xiv_gen_db::data();
        for id in ultros_app::game_sources::exchange_currencies(data) {
            let name = &data.items[&id].name;
            let results = service.search(name);
            assert!(
                results.iter().any(|r| r.result_type == "currency" && &r.title == name),
                "{name:?} missing: {:?}",
                results.iter().map(|r| (&r.title, &r.result_type)).collect::<Vec<_>>()
            );
        }
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p ultros --lib search_service::tests::currencies_are_what_the_shops_take`
Expected: FAIL — `"Wolf Mark" not a currency result`.

- [ ] **Step 3: Replace the currency block**

Delete lines 207–262 (`// Index Currencies` through the closing `}` of `for id in currency_ids { ... }`) and put in their place:

```rust
        // Index Currencies — the same set the Currency Exchange page lists.
        for id in ultros_app::game_sources::exchange_currencies(data) {
            let Some(item) = data.items.get(&id) else {
                continue;
            };
            let category_name = data
                .item_ui_categorys
                .get(&ItemUiCategoryId(item.item_ui_category as i32))
                .map(|c| c.name.as_str())
                .unwrap_or("");
            index_writer.add_document(doc!(
                title_field => item.name.as_str(),
                type_field => "currency",
                url_field => format!("/currency-exchange/{}", id.0),
                icon_id_field => id.0 as i64,
                category_field => "",
                display_category_field => category_name,
            ))?;
        }
```

Remove `ItemUiCategoryId` from the `use xiv_gen::{...}` line if it is now only used here (it still is — keep it). Remove `std::collections::HashSet` usage if it became dead.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ultros --lib search_service`
Expected: all pass, including the two new ones.

- [ ] **Step 5: Lint and commit**

Run: `./check_ci.sh > "$CLAUDE_SCRATCHPAD/ci.log" 2>&1; echo "REAL_EXIT=$?"`

```bash
git add ultros/src/search_service.rs
git commit -m "fix(search): index the currencies shops take, not the items they hand out

SpecialShop.item is the received item; the indexer read it as the cost
side, so Wolf Mark and Fae Fancy were unreachable while scrip tokens
were listed as currencies. Use the exchange page's own set.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 5: `kind` field, type weights, candidate width

**Files:**
- Modify: `ultros/src/search_service.rs` (schema, `type_weight`, `SEARCH_CANDIDATES`, `search`, every `doc!`, tests)

**Interfaces:**
- Produces: `fn kind_terms(result_type: &str) -> &'static str` and a `kind_field: Field` on `SearchService`; `const KIND_BOOST: f32 = 2.0`.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn kind_terms_cover_every_indexed_type() {
        for t in ["item", "category", "job equipment", "currency", "recipe", "venture", "npc", "scrip source"] {
            assert!(!kind_terms(t).is_empty(), "{t} has no kind terms");
        }
    }

    /// Naming the type in the query lifts that type over the item page.
    #[test]
    fn adding_recipe_to_the_query_lifts_the_recipe_over_the_item() {
        let service = SearchService::new().expect("index builds from embedded data");
        let names = craftable_item_names(true);
        let mut checked = 0;
        for name in spread(&names, 40) {
            let plain = service.search(name);
            let Some(item_at) = plain.iter().position(|r| r.title == name && r.result_type == "item") else { continue };
            let Some(recipe_at) = plain.iter().position(|r| r.title == name && r.result_type == "recipe") else { continue };
            assert!(item_at < recipe_at, "{name:?}: plain query should list item first");
            let nudged = service.search(&format!("{name} recipe"));
            let item_at = nudged.iter().position(|r| r.title == name && r.result_type == "item");
            let recipe_at = nudged
                .iter()
                .position(|r| r.title == name && r.result_type == "recipe")
                .unwrap_or_else(|| panic!("{name:?} recipe: recipe fell off the list"));
            assert!(
                item_at.is_none_or(|i| recipe_at < i),
                "{name:?} recipe: recipe ({recipe_at}) still below item ({item_at:?})"
            );
            checked += 1;
        }
        assert!(checked > 0, "no sample had both documents");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p ultros --lib search_service::tests::kind_terms`
Expected: compile error `cannot find function kind_terms`.

- [ ] **Step 3: Implement**

Near `type_weight`:

```rust
/// Documents scored before weighting ... (existing doc comment)
const SEARCH_CANDIDATES: usize = 60;

/// Exact-match boost on the `kind` field. High enough that one extra matched
/// term overturns a 0.7 type weight; pinned by
/// `adding_recipe_to_the_query_lifts_the_recipe_over_the_item` and the
/// venture equivalent.
const KIND_BOOST: f32 = 2.0;

/// Words that name a result type. Indexed in the `kind` field so that typing
/// one of them ("iron ingot recipe", "coeurl skin venture") is an extra term
/// match only that type's document gets — a nudge, not a filter.
fn kind_terms(result_type: &str) -> &'static str {
    match result_type {
        "item" => "item",
        "category" => "category",
        "job equipment" => "job gear equipment",
        "currency" => "currency exchange",
        "recipe" => "recipe craft crafting",
        "venture" => "venture retainer",
        "npc" => "npc vendor shop merchant",
        "scrip source" => "scrip collectable turnin",
        _ => "",
    }
}

fn type_weight(result_type: &str) -> f32 {
    match result_type {
        "recipe" => 0.8,
        "scrip source" => 0.75,
        "venture" => 0.7,
        _ => 1.0,
    }
}
```

Schema: after `category_field`, add

```rust
        let kind_options = TextOptions::default().set_indexing_options(
            tantivy::schema::TextFieldIndexing::default()
                .set_tokenizer("en_stem")
                .set_index_option(tantivy::schema::IndexRecordOption::WithFreqs),
        );
        let kind_field = schema_builder.add_text_field("kind", kind_options);
```

Add `kind_field: tantivy::schema::Field` to the struct and the `Ok(Self { .. })`.

Every existing `doc!(...)` gains `kind_field => kind_terms("<that type>"),` — item, category, job equipment, currency, recipe.

In `search`, add `self.kind_field` to the **exact** parser only:

```rust
        let mut exact_parser = QueryParser::for_index(
            &self.index,
            vec![self.title_field, self.category_field, self.kind_field],
        );
        exact_parser.set_field_boost(self.title_field, 5.0);
        exact_parser.set_field_boost(self.category_field, 1.0);
        exact_parser.set_field_boost(self.kind_field, KIND_BOOST);
```

Update the `type_weight` doc comment's table sentence to mention the new types, and the existing `rank` tests stay as they are.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ultros --lib search_service`
Expected: all pass. If `adding_recipe_to_the_query_lifts_the_recipe_over_the_item` fails with the recipe still below the item, raise `KIND_BOOST` to 3.0 and re-run; record the final value in the constant's comment.

- [ ] **Step 5: Lint and commit**

```bash
./check_ci.sh > "$CLAUDE_SCRATCHPAD/ci.log" 2>&1; echo "REAL_EXIT=$?"
git add ultros/src/search_service.rs
git commit -m "feat(search): kind field so naming a result type nudges it up

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 6: Index gil-shop NPCs

**Files:**
- Modify: `ultros/src/search_service.rs`

**Interfaces:**
- Produces: `fn title_case(name: &str) -> String`, `fn npc_zone(data: &Data, npc: ENpcResidentId) -> String`.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn title_case_capitalises_each_word_and_leaves_the_rest() {
        assert_eq!(title_case("merchant & mender"), "Merchant & Mender");
        assert_eq!(title_case("Syneyhil"), "Syneyhil");
        assert_eq!(title_case("o'bhari"), "O'bhari");
        assert_eq!(title_case(""), "");
    }

    /// Every NPC with a gil shop has a page and is findable by name.
    #[test]
    fn gil_shop_npcs_are_findable_with_their_zone() {
        let service = SearchService::new().expect("index builds from embedded data");
        let data = xiv_gen_db::data();
        let mut ids: Vec<i32> = data.gil_shop_npcs.values().flatten().map(|id| id.0).collect();
        ids.sort_unstable();
        ids.dedup();
        assert!(!ids.is_empty());
        let mut checked = 0;
        for id in ids.iter().step_by((ids.len() / 30).max(1)).take(30) {
            let Some(resident) = data.e_npc_residents.get(&ENpcResidentId(*id)) else { continue };
            if resident.singular.is_empty() { continue; }
            let title = title_case(&resident.singular);
            let zone = npc_zone(data, ENpcResidentId(*id));
            let query = if zone.is_empty() { title.clone() } else { format!("{title} {zone}") };
            let results = service.search(&query);
            assert!(
                results.iter().any(|r| r.result_type == "npc" && r.url == format!("/npc/{id}")),
                "{query:?} did not find /npc/{id}: {:?}",
                results.iter().map(|r| (&r.title, &r.url)).collect::<Vec<_>>()
            );
            checked += 1;
        }
        assert!(checked > 0);
    }
```

Add `ENpcResidentId` to the `use xiv_gen::{...}` import.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p ultros --lib search_service::tests::title_case`
Expected: compile error.

- [ ] **Step 3: Implement**

Helpers near `recipe_label`:

```rust
/// `ENpcResident.Singular` stores generic vendors lowercase ("merchant &
/// mender") and named ones as written. Capitalise the first letter of each
/// whitespace-separated word and leave everything else alone, so names with
/// internal capitals or apostrophes come through untouched.
fn title_case(name: &str) -> String {
    name.split(' ')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Zone of the NPC's first placement, as the NPC page labels it, or empty
/// when the client layout files place it nowhere.
fn npc_zone(data: &Data, npc: ENpcResidentId) -> String {
    data.npc_placements
        .get(&npc)
        .and_then(|placements| placements.first())
        .map(|placement| {
            ultros_ui_game::components::npc_locations::placement_label(data, placement)
        })
        .unwrap_or_default()
}
```

Check `ultros/Cargo.toml` has `ultros-ui-game` as a dependency; if not, use `ultros_app`'s re-export or add `ultros-ui-game = { path = "../ultros-frontend/ultros-ui-game", features = ["ssr"] }` matching how `ultros-app` is declared.

Indexing block, after recipes and before `index_writer.commit()`:

```rust
        // Index NPCs — every vendor with a gil shop has a `/npc/:id` page.
        // Generic names ("Merchant & Mender") repeat across the world, so
        // the zone is both the subtitle and a low-weight searchable term.
        let mut npc_ids: Vec<ENpcResidentId> =
            data.gil_shop_npcs.values().flatten().copied().collect();
        npc_ids.sort_by_key(|id| id.0);
        npc_ids.dedup();
        for id in npc_ids {
            let Some(resident) = data.e_npc_residents.get(&id) else {
                continue;
            };
            if resident.singular.is_empty() {
                continue;
            }
            let zone = npc_zone(data, id);
            index_writer.add_document(doc!(
                title_field => title_case(&resident.singular),
                type_field => "npc",
                url_field => format!("/npc/{}", id.0),
                icon_id_field => 0i64,
                category_field => zone.as_str(),
                display_category_field => zone.as_str(),
                kind_field => kind_terms("npc"),
            ))?;
        }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ultros --lib search_service`
Expected: all pass.

- [ ] **Step 5: Lint and commit**

```bash
./check_ci.sh > "$CLAUDE_SCRATCHPAD/ci.log" 2>&1; echo "REAL_EXIT=$?"
git add ultros/src/search_service.rs ultros/Cargo.toml Cargo.lock
git commit -m "feat(search): index gil-shop NPCs with their zone

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 7: Index venture rewards

**Files:**
- Modify: `ultros/src/search_service.rs`

**Interfaces:**
- Produces: `fn venture_label(data: &Data, reward: &VentureReward) -> String` (`"MIN Lv. 30"`).

- [ ] **Step 1: Write the failing tests**

```rust
    /// Venture rewards, one doc per item (lowest task id), ordered by item id.
    fn venture_reward_names() -> Vec<&'static str> {
        let data = xiv_gen_db::data();
        let mut names: Vec<(i32, &str)> = ultros_app::game_sources::venture_rewards(data)
            .into_iter()
            .map(|r| (r.item.0, data.items[&r.item].name.as_str()))
            .collect();
        names.sort_unstable();
        names.dedup();
        names.into_iter().map(|(_, n)| n).collect()
    }

    #[test]
    fn venture_rewards_are_findable_and_sit_below_the_item() {
        let service = SearchService::new().expect("index builds from embedded data");
        let names = venture_reward_names();
        assert!(!names.is_empty());
        for name in spread(&names, 40) {
            let results = service.search(name);
            let venture_at = results
                .iter()
                .position(|r| r.result_type == "venture" && r.title == name)
                .unwrap_or_else(|| panic!("{name:?}: no venture row in {:?}", results.iter().map(|r| (&r.title, &r.result_type)).collect::<Vec<_>>()));
            if let Some(item_at) = results.iter().position(|r| r.result_type == "item" && r.title == name) {
                assert!(item_at < venture_at, "{name:?}: venture above its item page");
            }
            let nudged = service.search(&format!("{name} venture"));
            assert_eq!(
                nudged.first().map(|r| (r.result_type.as_str(), r.title.as_str())),
                Some(("venture", name)),
                "{name:?} venture: {:?}",
                nudged.iter().map(|r| (&r.title, &r.result_type)).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn a_venture_result_names_its_job_and_level() {
        let data = xiv_gen_db::data();
        let reward = ultros_app::game_sources::venture_rewards(data)
            .into_iter()
            .find(|r| data.items[&r.item].name == "Coeurl Skin")
            .expect("Coeurl Skin venture");
        let label = venture_label(data, &reward);
        assert!(label.ends_with(&format!("Lv. {}", reward.level)), "{label}");
        assert!(!label.starts_with("Lv."), "{label}: job missing");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p ultros --lib search_service::tests::a_venture_result_names_its_job_and_level`
Expected: compile error `venture_label`.

- [ ] **Step 3: Implement**

```rust
use ultros_app::game_sources::VentureReward;

/// `MIN Lv. 30` — the job category and retainer level under a venture result.
fn venture_label(data: &Data, reward: &VentureReward) -> String {
    let job = data
        .class_job_categorys
        .get(&reward.class_job_category)
        .map(|c| c.name.as_str())
        .filter(|name| !name.is_empty());
    match job {
        Some(job) => format!("{job} Lv. {}", reward.level),
        None => format!("Lv. {}", reward.level),
    }
}
```

Indexing block after NPCs:

```rust
        // Index Ventures — one doc per reward item (lowest task id, like
        // recipes), landing on the analyzer with that row revealed.
        let mut venture_by_item: BTreeMap<i32, VentureReward> = BTreeMap::new();
        for reward in ultros_app::game_sources::venture_rewards(data) {
            venture_by_item.entry(reward.item.0).or_insert(reward);
        }
        for (item_id, reward) in venture_by_item {
            let Some(item) = data.items.get(&ItemId(item_id)) else {
                continue;
            };
            if item.name.is_empty() {
                continue;
            }
            index_writer.add_document(doc!(
                title_field => item.name.as_str(),
                type_field => "venture",
                url_field => format!("/venture-analyzer?item={item_id}"),
                icon_id_field => item_id as i64,
                category_field => "",
                display_category_field => venture_label(data, &reward),
                kind_field => kind_terms("venture"),
            ))?;
        }
```

(`venture_rewards` is sorted by task id, so `or_insert` keeps the lowest.)

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ultros --lib search_service`
Expected: all pass. If the nudge assertion fails, raise `KIND_BOOST` (Task 5) and re-run both nudge tests.

- [ ] **Step 5: Lint and commit**

```bash
./check_ci.sh > "$CLAUDE_SCRATCHPAD/ci.log" 2>&1; echo "REAL_EXIT=$?"
git add ultros/src/search_service.rs
git commit -m "feat(search): index venture rewards

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 8: Index scrip sources

**Files:**
- Modify: `ultros/src/search_service.rs`

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn crafted_turn_ins_are_scrip_sources_and_gathered_ones_are_not() {
        use ultros_app::game_sources::{ScripType, scrip_turn_ins};
        let service = SearchService::new().expect("index builds from embedded data");
        let data = xiv_gen_db::data();
        let turn_ins = scrip_turn_ins(data);
        let mut crafted: Vec<(i32, &str)> = turn_ins
            .iter()
            .filter(|t| !t.scrip_type.is_gatherer() && !matches!(t.scrip_type, ScripType::Other(_)))
            .filter_map(|t| Some((t.item_id, data.items.get(&ItemId(t.item_id))?.name.as_str())))
            .collect();
        crafted.sort_unstable();
        crafted.dedup();
        let names: Vec<&str> = crafted.iter().map(|(_, n)| *n).collect();
        for name in spread(&names, 40) {
            let results = service.search(name);
            assert!(
                results.iter().any(|r| r.result_type == "scrip source" && r.title == name),
                "{name:?}: {:?}",
                results.iter().map(|r| (&r.title, &r.result_type)).collect::<Vec<_>>()
            );
        }
        let gathered = turn_ins
            .iter()
            .find(|t| t.scrip_type.is_gatherer())
            .and_then(|t| data.items.get(&ItemId(t.item_id)))
            .expect("a gathered turn-in");
        let results = service.search(&gathered.name);
        assert!(titles_of(&results, "scrip source").iter().all(|t| t != &gathered.name), "{results:?}");
    }

    #[test]
    fn crafter_scrips_land_on_the_sources_page_and_the_exchange_page() {
        use ultros_app::game_sources::{ScripType, scrip_item};
        let service = SearchService::new().expect("index builds from embedded data");
        let data = xiv_gen_db::data();
        for scrip in [ScripType::OrangeCrafters, ScripType::PurpleCrafters] {
            let id = scrip_item(data, scrip).unwrap();
            let name = &data.items[&id].name;
            let results = service.search(name);
            let urls: Vec<_> = results.iter().filter(|r| &r.title == name).map(|r| r.url.as_str()).collect();
            assert!(urls.contains(&format!("/scrip-sources?scrip={}", scrip.filter_key().unwrap()).as_str()), "{urls:?}");
            assert!(urls.contains(&format!("/currency-exchange/{}", id.0).as_str()), "{urls:?}");
        }
        for scrip in [ScripType::OrangeGatherers, ScripType::PurpleGatherers] {
            let id = scrip_item(data, scrip).unwrap();
            let name = &data.items[&id].name;
            let results = service.search(name);
            assert!(results.iter().all(|r| !r.url.starts_with("/scrip-sources?scrip=")), "{results:?}");
        }
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p ultros --lib search_service::tests::crafted_turn_ins`
Expected: FAIL — no `scrip source` results.

- [ ] **Step 3: Implement**

After the venture block:

```rust
        // Index Scrip sources — crafted collectable turn-ins (the page can't
        // price gathered ones yet) and the crafter scrips themselves.
        {
            use ultros_app::game_sources::{ScripType, scrip_item, scrip_turn_ins};
            let mut best: BTreeMap<i32, (ScripType, u32)> = BTreeMap::new();
            for turn_in in scrip_turn_ins(data) {
                if turn_in.scrip_type.is_gatherer() || turn_in.scrip_type.filter_key().is_none() {
                    continue;
                }
                let entry = best
                    .entry(turn_in.item_id)
                    .or_insert((turn_in.scrip_type, turn_in.scrip_amount));
                if turn_in.scrip_amount > entry.1 {
                    *entry = (turn_in.scrip_type, turn_in.scrip_amount);
                }
            }
            for (item_id, (scrip, amount)) in best {
                let Some(item) = data.items.get(&ItemId(item_id)) else {
                    continue;
                };
                if item.name.is_empty() {
                    continue;
                }
                let scrip_name = scrip.item_name().unwrap_or("Scrip");
                index_writer.add_document(doc!(
                    title_field => item.name.as_str(),
                    type_field => "scrip source",
                    url_field => format!("/scrip-sources?item={item_id}"),
                    icon_id_field => item_id as i64,
                    category_field => "",
                    display_category_field => format!("{amount} {scrip_name}"),
                    kind_field => kind_terms("scrip source"),
                ))?;
            }
            for scrip in [ScripType::OrangeCrafters, ScripType::PurpleCrafters] {
                let (Some(id), Some(key)) = (scrip_item(data, scrip), scrip.filter_key()) else {
                    continue;
                };
                let Some(item) = data.items.get(&id) else {
                    continue;
                };
                index_writer.add_document(doc!(
                    title_field => item.name.as_str(),
                    type_field => "scrip source",
                    url_field => format!("/scrip-sources?scrip={key}"),
                    icon_id_field => id.0 as i64,
                    category_field => "",
                    display_category_field => "Scrip sources",
                    kind_field => kind_terms("scrip source"),
                ))?;
            }
        }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ultros --lib search_service`
Expected: all pass. Also re-run `finds_a_craft_whose_result_is_not_marketable` explicitly — the rarefied crafts now have a competing scrip-source doc; if a recipe fell off the list, the 0.75 weight is too close to 0.8 and must drop to 0.7 with the venture weight to 0.65 (update the spec table in the same commit).

- [ ] **Step 5: Lint and commit**

```bash
./check_ci.sh > "$CLAUDE_SCRATCHPAD/ci.log" 2>&1; echo "REAL_EXIT=$?"
git add ultros/src/search_service.rs
git commit -m "feat(search): index crafted scrip turn-ins and the crafter scrips

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 9: `reveal_index` prop on VirtualGrid → QueryGrid → MarketGrid

**Files:**
- Modify: `ultros-frontend/ultros-ui-grid/src/components/virtual_grid/mod.rs:145-161` (props) and after line ~476 (after `let reveal = ...;`)
- Modify: `ultros-frontend/ultros-ui-grid/src/components/virtual_grid/query_grid.rs:13-30, 224-225`
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/market.rs:836-852, 1084`

**Interfaces:**
- Produces: `#[prop(optional, into)] reveal_index: Option<Signal<Option<usize>>>` on all three components. Semantics: whenever the signal yields `Some(i)` with `i < row count`, the grid scrolls row `i` (0-based data row) into view and makes it the active row. The grid does **not** debounce repeated identical values — callers are expected to emit each index once (Task 10's `RevealOnce`).

- [ ] **Step 1: Add the prop to `VirtualGrid`**

In `mod.rs` props, after `visible_range`:

```rust
    /// Data-row index to scroll into view and make active when it changes.
    /// Emitted by pages that deep-link to a row (`?item=`); see
    /// `ultros_app::components::reveal_once`.
    #[prop(optional, into)]
    reveal_index: Option<Signal<Option<usize>>>,
```

Directly after `let reveal = move |r: usize, c: usize| { ... };` add:

```rust
    if let Some(reveal_index) = reveal_index {
        Effect::new(move |_| {
            let Some(index) = reveal_index.get() else {
                return;
            };
            if index < count.get() {
                reveal(index + 1, active.get_untracked().1);
            }
        });
    }
```

(`reveal` is a `Copy` closure — every capture is a `Copy` signal/`StoredValue`/`RowSource` — so it is still usable by the keydown handler below.)

- [ ] **Step 2: Forward through `QueryGrid`**

Props: add `#[prop(optional, into)] reveal_index: Option<Signal<Option<usize>>>,` after `visible_range`. At the `<VirtualGrid ... />` call add `reveal_index`.

- [ ] **Step 3: Forward through `MarketGrid`**

Props: same line after `visible_range`. At `<QueryGrid each columns=all_columns ...` add `reveal_index`.

- [ ] **Step 4: Compile everything**

Run: `cargo check -p ultros-ui-grid && cargo check -p ultros-app --features ssr && cargo check -p ultros-app --features hydrate --target wasm32-unknown-unknown`
Expected: clean. (Leptos optional `Option<Signal<..>>` props accept being omitted at every existing call site.)

- [ ] **Step 5: Lint and commit**

```bash
./check_ci.sh > "$CLAUDE_SCRATCHPAD/ci.log" 2>&1; echo "REAL_EXIT=$?"
git add ultros-frontend/ultros-ui-grid/src/components/virtual_grid/mod.rs ultros-frontend/ultros-ui-grid/src/components/virtual_grid/query_grid.rs ultros-frontend/ultros-app/src/analyzer_kit/market.rs
git commit -m "feat(grid): reveal_index prop to scroll a deep-linked row into view

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 10: `RevealOnce` and `?item=` on the venture and scrip pages

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/reveal_once.rs`
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs` (add `pub mod reveal_once;`)
- Modify: `ultros-frontend/ultros-app/src/routes/venture_analyzer.rs` (`VentureAnalyzerTable`, `<MarketGrid ...>` at ~495)
- Modify: `ultros-frontend/ultros-app/src/routes/scrip_sources.rs` (`ScripSourcesTable` or wherever `<MarketGrid` at ~780 lives)

**Interfaces:**
- Produces:
  ```rust
  #[derive(Default, Debug)]
  pub struct RevealOnce { item: Option<i32>, done: bool }
  impl RevealOnce {
      /// Returns Some(index) at most once per distinct `item` value, the first
      /// time `position` resolves it. A new `item` re-arms.
      pub fn next(&mut self, item: Option<i32>, position: impl FnOnce(i32) -> Option<usize>) -> Option<usize>;
  }
  ```

- [ ] **Step 1: Write the failing tests**

```rust
// ultros-frontend/ultros-app/src/components/reveal_once.rs
//! One scroll-to-row per deep link.
//!
//! `?item=<id>` asks a grid page to reveal that item's row. The row's index
//! moves every time live prices re-sort the table, and re-revealing on each
//! move would yank the scroll position out from under the reader — so the
//! reveal fires once per `item` value, the first time the row is present,
//! and re-arms only when the parameter changes.

#[derive(Default, Debug)]
pub struct RevealOnce {
    item: Option<i32>,
    done: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveals_once_when_the_row_appears() {
        let mut once = RevealOnce::default();
        assert_eq!(once.next(Some(7), |_| None), None, "row not there yet");
        assert_eq!(once.next(Some(7), |_| Some(3)), Some(3));
        assert_eq!(once.next(Some(7), |_| Some(9)), None, "re-sort must not re-reveal");
    }

    #[test]
    fn a_new_item_re_arms() {
        let mut once = RevealOnce::default();
        assert_eq!(once.next(Some(7), |_| Some(3)), Some(3));
        assert_eq!(once.next(Some(8), |_| Some(1)), Some(1));
        assert_eq!(once.next(None, |_| Some(0)), None, "no param, nothing to reveal");
        assert_eq!(once.next(Some(8), |_| Some(1)), Some(1), "param came back: reveal again");
    }
}
```

- [ ] **Step 2: Register and run**

Add `pub mod reveal_once;` to `components/mod.rs`.
Run: `cargo test -p ultros-app --lib reveal_once`
Expected: compile error `no method named next`.

- [ ] **Step 3: Implement**

```rust
impl RevealOnce {
    pub fn next(
        &mut self,
        item: Option<i32>,
        position: impl FnOnce(i32) -> Option<usize>,
    ) -> Option<usize> {
        if item != self.item {
            self.item = item;
            self.done = false;
        }
        let item = item?;
        if self.done {
            return None;
        }
        let index = position(item)?;
        self.done = true;
        Some(index)
    }
}
```

Run: `cargo test -p ultros-app --lib reveal_once` → 2 passed.

- [ ] **Step 4: Wire the venture analyzer**

In `VentureAnalyzerTable`, after `let (sort_dir, _set_sort_dir) = query_signal::<SortDir>("dir");` add:

```rust
    // `?item=` is a navigation target from search, not a filter: plain
    // `query_signal`, revealed once per value (see `RevealOnce`).
    let (reveal_item, _set_reveal_item) = query_signal::<i32>("item");
    let shown_rows = RwSignal::new(Vec::<(usize, Arc<VentureProfitData>)>::new());
    let reveal_once = StoredValue::new(crate::components::reveal_once::RevealOnce::default());
    let reveal_index = RwSignal::new(None::<usize>);
    Effect::new(move |_| {
        let item = reveal_item.get();
        let hit = shown_rows.with(|rows| {
            reveal_once.update_value(|once| {
                once.next(item, |item| rows.iter().position(|(_, row)| row.item_id == item))
            })
        });
        if hit.is_some() {
            reveal_index.set(hit);
        }
    });
```

`StoredValue::update_value` returns `()`; write it as:

```rust
        let mut hit = None;
        shown_rows.with(|rows| {
            reveal_once.update_value(|once| {
                hit = once.next(item, |item| rows.iter().position(|(_, row)| row.item_id == item));
            })
        });
```

On the `<MarketGrid ...>` element add `on_rows=Callback::new(move |rows| shown_rows.set(rows)) reveal_index=Signal::from(reveal_index)`.

- [ ] **Step 5: Wire scrip sources**

Same block in the component that renders `<MarketGrid show_saved_views=false id="scrip-sources-grid" ...>` (find it with `grep -n "scrip-sources-grid" scrip_sources.rs` and add the signals next to its other `query_signal` calls). Row type is `(usize, Arc<ScripSourceData>)`, match is `row.item_id.0 == item`.

- [ ] **Step 6: Compile both targets and lint**

Run: `cargo check -p ultros-app --features ssr && cargo check -p ultros-app --features hydrate --target wasm32-unknown-unknown && ./check_ci.sh > "$CLAUDE_SCRATCHPAD/ci.log" 2>&1; echo "REAL_EXIT=$?"`
Expected: clean, `REAL_EXIT=0`.

- [ ] **Step 7: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/reveal_once.rs ultros-frontend/ultros-app/src/components/mod.rs ultros-frontend/ultros-app/src/routes/venture_analyzer.rs ultros-frontend/ultros-app/src/routes/scrip_sources.rs
git commit -m "feat(ventures,scrips): ?item= reveals the deep-linked row once

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 11: Search box — translated type labels, icons, missing tools, hints

**Files:**
- Modify: `ultros-frontend/ultros-ui-shell/src/components/search_box.rs`
- Modify: `ultros-frontend/ultros-i18n/locales/{en,fr,de,ja,cn,ko,tc}.json`

**Interfaces:**
- Produces: `fn result_type_label(i18n, result_type: &str) -> String` (private).

- [ ] **Step 1: Add the locale keys (all seven files)**

English values (place next to the existing `search_hint_*` keys):

```json
    "search_type_item": "Item",
    "search_type_category": "Category",
    "search_type_job_equipment": "Job gear",
    "search_type_currency": "Currency",
    "search_type_recipe": "Recipe",
    "search_type_venture": "Venture",
    "search_type_npc": "Vendor",
    "search_type_scrip_source": "Scrip source",
    "search_type_tool": "Tool",
    "search_type_page": "Page",
    "search_type_help": "Help",
    "search_hint_sources": "Vendors, ventures and scrip turn-ins",
```

Translations:

| key | fr | de | ja | cn | ko | tc |
|---|---|---|---|---|---|---|
| item | Objet | Gegenstand | アイテム | 物品 | 아이템 | 物品 |
| category | Catégorie | Kategorie | カテゴリー | 分类 | 카테고리 | 分類 |
| job_equipment | Équipement de job | Job-Ausrüstung | ジョブ装備 | 职业装备 | 잡 장비 | 職業裝備 |
| currency | Monnaie | Währung | 通貨 | 货币 | 통화 | 貨幣 |
| recipe | Recette | Rezept | レシピ | 配方 | 제작법 | 配方 |
| venture | Mission de servant | Gehilfen-Auftrag | リテイナーベンチャー | 雇员探险 | 집사 탐험 | 雇員探險 |
| npc | Marchand | Händler | ショップNPC | 商人 | 상인 | 商人 |
| scrip_source | Source de scrips | Skrip-Quelle | スクリップ納品 | 工票来源 | 스크립 출처 | 工票來源 |
| tool | Outil | Werkzeug | ツール | 工具 | 도구 | 工具 |
| page | Page | Seite | ページ | 页面 | 페이지 | 頁面 |
| help | Aide | Hilfe | ヘルプ | 帮助 | 도움말 | 說明 |
| hint_sources | Marchands, missions de servant et livraisons de collectables | Händler, Gehilfen-Aufträge und Sammlerstück-Abgaben | ショップNPC、ベンチャー、収集品納品 | 商人、雇员探险与收藏品交纳 | 상인, 집사 탐험, 수집품 납품 | 商人、雇員探險與收藏品繳納 |

- [ ] **Step 2: Type label helper and use it**

Add near `get_static_pages`:

```rust
/// Backend result types are English identifiers; show the reader their
/// language. Unknown types fall through untranslated rather than blank.
fn result_type_label(i18n: I18nContext<Locale, I18nKeys>, result_type: &str) -> String {
    match result_type {
        "item" => t_string!(i18n, search_type_item).to_string(),
        "category" => t_string!(i18n, search_type_category).to_string(),
        "job equipment" => t_string!(i18n, search_type_job_equipment).to_string(),
        "currency" => t_string!(i18n, search_type_currency).to_string(),
        "recipe" => t_string!(i18n, search_type_recipe).to_string(),
        "venture" => t_string!(i18n, search_type_venture).to_string(),
        "npc" => t_string!(i18n, search_type_npc).to_string(),
        "scrip source" => t_string!(i18n, search_type_scrip_source).to_string(),
        "Tool" => t_string!(i18n, search_type_tool).to_string(),
        "Page" => t_string!(i18n, search_type_page).to_string(),
        "Help" => t_string!(i18n, search_type_help).to_string(),
        other => other.to_string(),
    }
}
```

Check what type `use_i18n()` returns in this crate (`grep -rn "I18nContext" ultros-frontend/ultros-ui-shell/src | head -3`) and import accordingly (`use leptos_i18n::I18nContext;` plus `Locale`, `I18nKeys` from `crate::i18n`).

In the result row, replace the subtitle block:

```rust
                                        <span class="text-xs text-[color:var(--color-text-muted)]">
                                            {
                                                let label = result_type_label(i18n, &result.result_type);
                                                match result.category.as_deref() {
                                                    Some(cat) if !cat.is_empty() => format!("{label} - {cat}"),
                                                    _ => label,
                                                }
                                            }
                                        </span>
```

- [ ] **Step 3: Icons**

Both `match result.result_type.as_str()` arms (the `icon_id == 0` branch and the `None` branch) gain:

```rust
                                                    "npc" => view! { <Icon icon=i::FaStoreSolid /> }.into_any(),
                                                    "venture" => view! { <Icon icon=i::FaBriefcaseSolid /> }.into_any(),
                                                    "scrip source" => view! { <Icon icon=i::FaCoinsSolid /> }.into_any(),
```

Deduplicate while there: extract `fn type_icon(result_type: &str) -> AnyView` holding the single match and call it from both branches.

- [ ] **Step 4: Static tools**

Append to `STATIC_PAGES` (before `My Lists`):

```rust
        SearchResult { score: 100.0, title: "Venture Analyzer".to_string(), result_type: "Tool".to_string(), url: "/venture-analyzer".to_string(), icon_id: None, category: Some("Retainers".to_string()) },
        SearchResult { score: 100.0, title: "Scrip Sources".to_string(), result_type: "Tool".to_string(), url: "/scrip-sources".to_string(), icon_id: None, category: Some("Crafting".to_string()) },
        SearchResult { score: 100.0, title: "Vendor Resale".to_string(), result_type: "Tool".to_string(), url: "/vendor-resale".to_string(), icon_id: None, category: Some("Market Analysis".to_string()) },
        SearchResult { score: 100.0, title: "FC Crafting Analyzer".to_string(), result_type: "Tool".to_string(), url: "/fc-crafting-analyzer".to_string(), icon_id: None, category: Some("Crafting".to_string()) },
        SearchResult { score: 100.0, title: "Market Trends".to_string(), result_type: "Tool".to_string(), url: "/trends".to_string(), icon_id: None, category: Some("Market Analysis".to_string()) },
        SearchResult { score: 100.0, title: "Leve Analyzer".to_string(), result_type: "Tool".to_string(), url: "/leve-analyzer".to_string(), icon_id: None, category: Some("Leveling".to_string()) },
```

(Titles here are pre-existing hardcoded English by design of this list; translating the list is out of scope per the spec.)

- [ ] **Step 5: Hint line**

After the recipes hint `<div>` add:

```rust
                <div class="flex items-center gap-2 text-sm">
                    <Icon icon=i::FaStoreSolid attr:class="text-[color:var(--color-text-muted)]" />
                    <span>{t!(i18n, search_hint_sources)}</span>
                </div>
```

- [ ] **Step 6: Build both targets, lint, commit**

Run: `cargo check -p ultros-ui-shell --features ssr && cargo check -p ultros-app --features hydrate --target wasm32-unknown-unknown && ./check_ci.sh > "$CLAUDE_SCRATCHPAD/ci.log" 2>&1; echo "REAL_EXIT=$?"`
Expected: clean; the i18n build warns about nothing (a missing key in any locale is a warning here and a compile error in the macro).

```bash
git add ultros-frontend/ultros-ui-shell/src/components/search_box.rs ultros-frontend/ultros-i18n/locales/
git commit -m "feat(search-box): translated type labels, new result icons, missing tools, sources hint

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 12: E2E — deep link highlights the row

**Files:**
- Create: `integration/search-deep-links.cjs`

- [ ] **Step 1: Write the script**

```js
// Search deep links: /venture-analyzer?item=<id> and /scrip-sources?item=<id>
// scroll that row into view and make it the grid's active row. Market data
// comes from the shared fixture; the rows themselves come from the real app.
const assert = require('node:assert/strict');
const puppeteer = require('puppeteer');
const { marketFixture } = require('./shared-analyzer-market-fixture.cjs');
const BASE = process.env.BASE_URL || 'http://127.0.0.1:8080';
const WORLD = 'Gilgamesh';

async function main() {
  const fixture = marketFixture(Array.from({ length: 55000 }, (_, i) => i + 1));
  const browser = await puppeteer.launch({ headless: true, args: ['--no-sandbox'] });
  const page = await browser.newPage();
  page.setDefaultTimeout(90000);
  const errors = [];
  page.on('pageerror', e => errors.push(e.message));
  await page.evaluateOnNewDocument(() => window.addEventListener('ultros:hydrated', () => { window.__hydrated = true; }));
  await page.setCookie({ name: 'HOME_WORLD', value: WORLD, url: BASE }, { name: 'HIDE_ADS', value: 'true', url: BASE });
  await page.setRequestInterception(true);
  page.on('request', request => {
    if (request.isInterceptResolutionHandled()) return;
    const response = fixture.reply(request);
    return response ? request.respond(response) : request.continue();
  });
  await page.setViewport({ width: 1400, height: 700 });

  async function firstItemId(route) {
    // Land on the unfiltered page, take an item id from a row far enough down
    // that revealing it must scroll.
    await page.goto(`${BASE}${route}?lang=en`, { waitUntil: 'domcontentloaded' });
    await page.waitForFunction(() => window.__hydrated);
    await page.waitForFunction(() => Number(document.querySelector('.virtual-grid')?.getAttribute('aria-rowcount')) > 20);
    return page.evaluate(() => {
      const grid = document.querySelector('.virtual-grid');
      grid.scrollTop = 15 * 40;
      return new Promise(resolve => setTimeout(() => {
        const row = [...document.querySelectorAll('.virtual-grid-row')].find(r => Number(r.getAttribute('aria-rowindex')) === 17);
        const link = row.querySelector('a[href^="/item/"]');
        resolve({ id: Number(link.getAttribute('href').split('/').pop()), name: link.textContent.trim() });
      }, 300));
    });
  }

  async function expectRevealed(route, target) {
    await page.goto(`${BASE}${route}?lang=en&item=${target.id}`, { waitUntil: 'domcontentloaded' });
    await page.waitForFunction(() => window.__hydrated);
    await page.waitForFunction(() => /-r[1-9]\d*-c\d+$/.test(document.querySelector('.virtual-grid')?.getAttribute('aria-activedescendant') || ''));
    const active = await page.evaluate(() => {
      const grid = document.querySelector('.virtual-grid');
      const r = Number(grid.getAttribute('aria-activedescendant').match(/-r(\d+)-c/)[1]);
      const row = [...grid.querySelectorAll('.virtual-grid-row')].find(el => Number(el.getAttribute('aria-rowindex')) === r + 1);
      return { r, text: row?.textContent || '', scrollTop: grid.scrollTop };
    });
    assert.ok(active.text.includes(target.name), `${route}: active row ${active.r} is "${active.text}", wanted ${target.name}`);
    assert.ok(active.scrollTop > 0, `${route}: grid did not scroll to reveal row ${active.r}`);
  }

  try {
    for (const route of [`/venture-analyzer/${WORLD}`, `/scrip-sources/${WORLD}`]) {
      const target = await firstItemId(route);
      await expectRevealed(route, target);
    }
    assert.deepEqual(errors, []);
    console.log('search-deep-links: ok');
  } finally {
    await browser.close();
  }
}

main().catch(e => { console.error(e); process.exit(1); });
```

- [ ] **Step 2: Run it against a local build**

Build and serve per `AGENTS.md` (`cargo leptos build`, then run the binary with `PORT=8093 HOSTNAME=127.0.0.1 METRICS_PORT=9093`), then:

Run: `BASE_URL=http://127.0.0.1:8093 node integration/search-deep-links.cjs`
Expected: `search-deep-links: ok`. If `firstItemId` finds no `a[href^="/item/"]` in the row, inspect the row markup with `page.evaluate` and adjust the selector to the item-cell link the page renders.

- [ ] **Step 3: Manually spot-check search** with the browser pane on the same server: type `wolf mark` (Currency row present), `merchant limsa` (NPC rows with Limsa zone first), `coeurl skin` (item first, venture below), `coeurl skin venture` (venture first). Click the venture row and confirm the analyzer scrolls to and highlights Coeurl Skin.

- [ ] **Step 4: Commit**

```bash
git add integration/search-deep-links.cjs
git commit -m "test(e2e): search deep links reveal the venture and scrip rows

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 13: Final verification and PR

- [ ] **Step 1: Full CI check**

Run: `./check_ci.sh > "$CLAUDE_SCRATCHPAD/ci.log" 2>&1; echo "REAL_EXIT=$?"; tail -30 "$CLAUDE_SCRATCHPAD/ci.log"`
Expected: `REAL_EXIT=0`.

- [ ] **Step 2: Run every test touched**

Run: `cargo test -p ultros --lib search_service && cargo test -p ultros-app --lib game_sources && cargo test -p ultros-app --lib reveal_once && cargo test -p ultros-app --lib scrip_sources && cargo test -p ultros-ui-grid --lib`
Expected: all green.

- [ ] **Step 3: Push and open the PR**

```bash
git push -u origin claude/search-feature-extensions-de5198
gh pr create --title "Search: fix currencies, index NPCs, ventures and scrip sources with row deep links" --body-file "$CLAUDE_SCRATCHPAD/pr.md"
```

PR body: the spec's Problem + Goals sections condensed, the weight table, a note that the `kind` boost value was pinned by tests, the e2e script name, and the `🤖 Generated with [Claude Code](https://claude.com/claude-code)` footer.
