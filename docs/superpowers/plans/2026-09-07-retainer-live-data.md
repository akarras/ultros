# Retainer Live Data Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `/retainers/listings` and `/retainers/undercuts` refetch themselves when the market changes and show the existing `RealtimeStatus` pill instead of the "data may be stale" notice.

**Architecture:** A new `routes/retainer_live.rs` module holds two pure functions (filter builder, relevance check) and a `use_retainer_live` hook that subscribes through the existing `RealtimeClient`, debounces events 1.5 s, and calls the page's `refetch`. Both pages derive `(world_id, item_id)` pairs from the resource they already own and hand them to the hook. Listing events are deltas, so the design refetches rather than patches.

**Tech Stack:** Rust, Leptos 0.8 (signals, `Effect`, `StoredValue`, `on_cleanup`), `gloo_timers::callback::Timeout` (wasm only), `ultros_api_types::websocket` (`FilterPredicate`, `ServerClient`, `SocketMessageType`, `EventType`, `ListingEventData`), `leptos-i18n`.

Spec: `docs/superpowers/specs/2026-09-07-retainer-live-data-design.md`.

## Global Constraints

- Run `./check_ci.sh` before every commit; it runs fmt-check, clippy `-D warnings`, and `scripts/check_tests.sh`. Read its exit code directly (`./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"`), never through a pipe.
- No new user-facing string literals. This feature adds **no** i18n keys and deletes `retainers_data_notice` from **all seven** locale files (`en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`).
- `RealtimeClient::subscribe_market` is a no-op under the `ssr` feature; wasm-only code (timers) goes under `#[cfg(not(feature = "ssr"))]`.
- Status strings handed to `RealtimeStatus` are exactly `"connecting"`, `"live"`, `"reconnecting"`, `"offline"`.
- Debounce is a **trailing 1.5 s** timer; `Stale`/`Error` refetch immediately and cancel any pending timer.
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.
- Work in `C:/Users/chw11/code/ultros/.claude/worktrees/sweet-bardeen-f8833d` on branch `claude/retainer-live-data-6eb801`. Run cargo with `CARGO_PROFILE_TEST_DEBUG=0` (matches `scripts/check_tests.sh`).

## File Structure

| File | Responsibility |
|---|---|
| `ultros-frontend/ultros-app/src/routes/retainer_live.rs` (create) | `ListedPair`, `retainer_market_filter`, `is_retainer_update_relevant`, `RetainerLive`, `use_retainer_live`, unit tests. |
| `ultros-frontend/ultros-app/src/routes/mod.rs` (modify) | `pub mod retainer_live;` |
| `ultros-frontend/ultros-app/src/routes/retainers.rs` (modify) | Both pages derive pairs, call the hook, render the pill, drop the notice. |
| `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json` (modify) | Delete `retainers_data_notice`. |

---

### Task 1: Pure filter and relevance functions

**Files:**
- Create: `ultros-frontend/ultros-app/src/routes/retainer_live.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/mod.rs` (add one line after line 28 `pub mod retainers;`)

**Interfaces:**
- Consumes: `ultros_api_types::websocket::{FilterPredicate, ServerClient, EventType, ListingEventData}`; `ultros_api_types::world_helper::AnySelector`.
- Produces:
  - `pub(crate) type ListedPair = (i32, i32);` — `(world_id, item_id)`.
  - `pub(crate) fn retainer_market_filter(pairs: &[ListedPair]) -> Option<FilterPredicate>`
  - `pub(crate) fn is_retainer_update_relevant(message: &ServerClient, pairs: &[ListedPair]) -> bool`

- [ ] **Step 1: Register the module and write the failing tests**

Add to `ultros-frontend/ultros-app/src/routes/mod.rs` directly after `pub mod retainers;`:

```rust
pub mod retainer_live;
```

Create `ultros-frontend/ultros-app/src/routes/retainer_live.rs` with only the tests and stub signatures that fail to compile without the bodies:

```rust
//! Live-update wiring for `/retainers/listings` and `/retainers/undercuts`.
//!
//! Listing websocket events are deltas (an `Added`/`Removed`/`Updated` carries
//! only the listings that changed), so a page cannot recompute "cheapest on
//! this world" from an event alone. Both pages therefore refetch their
//! resource when a relevant event lands, debounced so a burst on a busy
//! world becomes one request.

use ultros_api_types::websocket::{FilterPredicate, ServerClient};
use ultros_api_types::world_helper::AnySelector;

/// A `(world_id, item_id)` pair one of the user's retainers currently lists.
pub(crate) type ListedPair = (i32, i32);

/// `Items(<unique item ids>) AND (World(w1) OR World(w2) OR ...)` over the
/// unique worlds in `pairs`. `None` when there is nothing to subscribe to.
pub(crate) fn retainer_market_filter(pairs: &[ListedPair]) -> Option<FilterPredicate> {
    todo!()
}

/// True when `message` should trigger a refetch for a page showing `pairs`:
/// a `Listings` event whose `(world_id, item_id)` is listed, or `Stale`
/// while anything is listed. Server-side filtering already narrows events;
/// this is the client-side guard, the same role
/// `is_list_market_update_relevant` plays for lists.
pub(crate) fn is_retainer_update_relevant(message: &ServerClient, pairs: &[ListedPair]) -> bool {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultros_api_types::websocket::{EventType, ListingEventData, SaleEventData};

    fn listings_event(world_id: i32, item_id: i32) -> ServerClient {
        ServerClient::Listings(EventType::Added(ListingEventData {
            item_id,
            world_id,
            listings: Vec::new(),
        }))
    }

    /// Collects every `World(AnySelector::World(id))` leaf under an `Or` tree.
    fn world_ids(pred: &FilterPredicate) -> Vec<i32> {
        match pred {
            FilterPredicate::World(AnySelector::World(id)) => vec![*id],
            FilterPredicate::Or((a, b)) => {
                let mut ids = world_ids(a);
                ids.extend(world_ids(b));
                ids
            }
            other => panic!("unexpected predicate in world tree: {other:?}"),
        }
    }

    #[test]
    fn empty_pairs_yield_no_filter() {
        assert!(retainer_market_filter(&[]).is_none());
    }

    #[test]
    fn single_pair_is_items_and_world() {
        let filter = retainer_market_filter(&[(34, 5)]).expect("filter");
        let FilterPredicate::And((items, worlds)) = filter else {
            panic!("expected And, got {filter:?}");
        };
        let FilterPredicate::Items(ids) = *items else {
            panic!("expected Items, got {items:?}");
        };
        assert_eq!(ids, vec![5]);
        assert_eq!(world_ids(&worlds), vec![34]);
    }

    #[test]
    fn many_pairs_dedupe_items_and_or_worlds() {
        let filter =
            retainer_market_filter(&[(34, 5), (34, 7), (40, 5), (40, 9)]).expect("filter");
        let FilterPredicate::And((items, worlds)) = filter else {
            panic!("expected And, got {filter:?}");
        };
        let FilterPredicate::Items(mut ids) = *items else {
            panic!("expected Items, got {items:?}");
        };
        ids.sort_unstable();
        assert_eq!(ids, vec![5, 7, 9]);
        let mut worlds = world_ids(&worlds);
        worlds.sort_unstable();
        assert_eq!(worlds, vec![34, 40]);
    }

    #[test]
    fn matching_listing_event_is_relevant() {
        assert!(is_retainer_update_relevant(&listings_event(34, 5), &[(34, 5)]));
    }

    #[test]
    fn listing_event_for_other_world_or_item_is_not_relevant() {
        let pairs = [(34, 5)];
        assert!(!is_retainer_update_relevant(&listings_event(40, 5), &pairs));
        assert!(!is_retainer_update_relevant(&listings_event(34, 6), &pairs));
    }

    #[test]
    fn stale_is_relevant_only_with_pairs() {
        let stale = ServerClient::Stale { subscription_id: 1 };
        assert!(is_retainer_update_relevant(&stale, &[(34, 5)]));
        assert!(!is_retainer_update_relevant(&stale, &[]));
    }

    #[test]
    fn sales_are_never_relevant() {
        let sales = ServerClient::Sales(EventType::Added(SaleEventData { sales: Vec::new() }));
        assert!(!is_retainer_update_relevant(&sales, &[(34, 5)]));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cd "C:/Users/chw11/code/ultros/.claude/worktrees/sweet-bardeen-f8833d" && CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked -p ultros-app --lib retainer_live 2>&1 | tail -20
```

Expected: the tests compile and **panic** with `not yet implemented` (the `todo!()` bodies), so every test reports FAILED. If clippy-style warnings about unused variables appear that is fine at this step.

- [ ] **Step 3: Implement the two functions**

Replace the two `todo!()` bodies:

```rust
pub(crate) fn retainer_market_filter(pairs: &[ListedPair]) -> Option<FilterPredicate> {
    let mut item_ids: Vec<i32> = pairs.iter().map(|(_, item)| *item).collect();
    item_ids.sort_unstable();
    item_ids.dedup();
    let mut world_ids: Vec<i32> = pairs.iter().map(|(world, _)| *world).collect();
    world_ids.sort_unstable();
    world_ids.dedup();

    let mut worlds = world_ids.into_iter();
    let first = FilterPredicate::World(AnySelector::World(worlds.next()?));
    let worlds = worlds.fold(first, |acc, world| {
        acc.or(FilterPredicate::World(AnySelector::World(world)))
    });
    Some(FilterPredicate::Items(item_ids).and(worlds))
}

pub(crate) fn is_retainer_update_relevant(message: &ServerClient, pairs: &[ListedPair]) -> bool {
    match message {
        ServerClient::Listings(event) => {
            let data = match event {
                EventType::Added(data) | EventType::Removed(data) | EventType::Updated(data) => {
                    data
                }
            };
            pairs.contains(&(data.world_id, data.item_id))
        }
        ServerClient::Stale { .. } => !pairs.is_empty(),
        _ => false,
    }
}
```

and extend the top-level import to `use ultros_api_types::websocket::{EventType, FilterPredicate, ServerClient};`.

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cd "C:/Users/chw11/code/ultros/.claude/worktrees/sweet-bardeen-f8833d" && CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked -p ultros-app --lib retainer_live 2>&1 | tail -15
```

Expected: `test result: ok. 7 passed`.

- [ ] **Step 5: Commit**

```bash
cd "C:/Users/chw11/code/ultros/.claude/worktrees/sweet-bardeen-f8833d" && cargo fmt --all && git add ultros-frontend/ultros-app/src/routes/mod.rs ultros-frontend/ultros-app/src/routes/retainer_live.rs && git commit -m "feat(retainers): filter + relevance helpers for live retainer pages

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: `use_retainer_live` hook

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/retainer_live.rs` (append after `is_retainer_update_relevant`, before `#[cfg(test)]`)

**Interfaces:**
- Consumes: Task 1's `ListedPair`, `retainer_market_filter`, `is_retainer_update_relevant`; `crate::ws::realtime::{RealtimeSubscription, use_realtime}` (`use_realtime() -> Option<RealtimeClient>`, `RealtimeClient::subscribe_market(FilterPredicate, SocketMessageType, impl Fn(ServerClient) + 'static) -> RealtimeSubscription`, dropping the subscription unsubscribes).
- Produces:
  ```rust
  pub(crate) struct RetainerLive {
      pub status: Signal<String>,
      pub last_update: Signal<Option<chrono::DateTime<chrono::Utc>>>,
  }
  pub(crate) fn use_retainer_live(
      pairs: Signal<Option<Vec<ListedPair>>>,
      refetch: impl Fn() + Clone + 'static,
  ) -> RetainerLive
  ```

No native unit test covers the hook: the subscription and timer only exist on wasm. It is exercised by Task 3's compile under both features and the post-deploy check.

- [ ] **Step 1: Add the imports**

At the top of `retainer_live.rs`, after the existing `use` lines:

```rust
use crate::ws::realtime::{RealtimeSubscription, use_realtime};
use chrono::{DateTime, Utc};
use leptos::prelude::*;
use ultros_api_types::websocket::SocketMessageType;
```

- [ ] **Step 2: Add the debounce helper**

A tiny wasm-only wrapper so the hook body reads the same under both features:

```rust
/// Trailing debounce for refetches. On wasm this is a `gloo_timers::Timeout`;
/// under `ssr` the whole hook is inert (the realtime client never fires), so
/// the timer is a no-op there.
#[derive(Default)]
struct RefetchDebounce {
    #[cfg(not(feature = "ssr"))]
    pending: Option<gloo_timers::callback::Timeout>,
}

const DEBOUNCE_MS: u32 = 1_500;

impl RefetchDebounce {
    /// Replace any pending timer with a fresh one that runs `refetch` after
    /// [`DEBOUNCE_MS`].
    fn schedule(&mut self, refetch: impl FnOnce() + 'static) {
        #[cfg(not(feature = "ssr"))]
        {
            self.pending = Some(gloo_timers::callback::Timeout::new(DEBOUNCE_MS, refetch));
        }
        #[cfg(feature = "ssr")]
        {
            let _ = refetch;
        }
    }

    /// Drop any pending timer without running it.
    fn cancel(&mut self) {
        #[cfg(not(feature = "ssr"))]
        {
            self.pending = None;
        }
    }
}
```

(Dropping a `gloo_timers::callback::Timeout` cancels it, which is why `pending = None` / replacing the `Option` is enough.)

- [ ] **Step 3: Add the hook**

```rust
/// Signals a page hands to `RealtimeStatus`.
#[derive(Clone, Copy)]
pub(crate) struct RetainerLive {
    pub status: Signal<String>,
    pub last_update: Signal<Option<DateTime<Utc>>>,
}

/// Subscribe a retainer page to listing events for everything its retainers
/// currently list, and call `refetch` (debounced) when one lands.
///
/// `pairs` is derived from the page's resource: `None` while it is loading or
/// errored, `Some(vec)` once it resolves. The subscription is rebuilt every
/// time `pairs` changes, so a newly listed item that shows up after a refetch
/// is covered without a page reload (mirrors the list page).
pub(crate) fn use_retainer_live(
    pairs: Signal<Option<Vec<ListedPair>>>,
    refetch: impl Fn() + Clone + 'static,
) -> RetainerLive {
    let (status, set_status) = signal("connecting".to_string());
    let (last_update, set_last_update) = signal(None::<DateTime<Utc>>);
    let subscription = StoredValue::new(None::<RealtimeSubscription>);
    let debounce = StoredValue::new(RefetchDebounce::default());
    let realtime = use_realtime();

    Effect::new(move |_| {
        let Some(realtime) = realtime.clone() else {
            set_status.set("offline".to_string());
            return;
        };
        // `None` covers loading, errored, and (if the resource clears its
        // value mid-refetch) the window between a refetch and its answer —
        // keep the existing subscription alive across all of those.
        let Some(pairs) = pairs.get() else {
            return;
        };
        subscription.update_value(|sub| *sub = None);
        debounce.update_value(RefetchDebounce::cancel);
        let Some(filter) = retainer_market_filter(&pairs) else {
            return;
        };
        set_status.set("connecting".to_string());
        let refetch = refetch.clone();
        let sub = realtime.subscribe_market(filter, SocketMessageType::Listings, move |message| {
            match message {
                ServerClient::Subscribed { .. } => {
                    set_status.set("live".to_string());
                }
                ServerClient::Listings(_) => {
                    if !is_retainer_update_relevant(&message, &pairs) {
                        return;
                    }
                    set_status.set("live".to_string());
                    set_last_update.set(Some(Utc::now()));
                    let refetch = refetch.clone();
                    debounce.update_value(|d| d.schedule(move || refetch()));
                }
                ServerClient::Stale { .. } | ServerClient::Error { .. } => {
                    set_status.set("reconnecting".to_string());
                    set_last_update.set(Some(Utc::now()));
                    debounce.update_value(RefetchDebounce::cancel);
                    refetch();
                }
                _ => {}
            }
        });
        subscription.set_value(Some(sub));
    });

    on_cleanup(move || {
        subscription.update_value(|sub| *sub = None);
        debounce.update_value(RefetchDebounce::cancel);
    });

    RetainerLive {
        status: status.into(),
        last_update: last_update.into(),
    }
}
```

Notes for the implementer:
- `pairs` is moved into the handler closure by value (it was `.get()`-ed, so it is an owned `Vec`). The `Effect` closure itself only captures the `Signal`, which is `Copy`.
- `StoredValue<RefetchDebounce>` needs `RefetchDebounce: 'static`, which it is; `Timeout` is `!Send` but `StoredValue` in this crate is used with `!Send` payloads already (`RealtimeSubscription` in `list_view.rs`). If the compiler complains about `Send`, use `StoredValue::new_local(...)` for both `subscription` and `debounce` and note it in the commit.
- Under `ssr`, `realtime.subscribe_market` returns the inert `RealtimeSubscription` and the handler is never called, so `RefetchDebounce::schedule` is unreachable there; the `let _ = refetch;` keeps clippy quiet about the unused parameter.

- [ ] **Step 4: Check it compiles under both feature sets**

```bash
cd "C:/Users/chw11/code/ultros/.claude/worktrees/sweet-bardeen-f8833d" && cargo check --locked -p ultros-app 2>&1 | tail -5 && cargo check --locked -p ultros-app --no-default-features --features hydrate --target wasm32-unknown-unknown 2>&1 | tail -5
```

Expected: both end in `Finished` with no errors. If the wasm check fails on an unrelated crate because the target is not installed, run `rustup target add wasm32-unknown-unknown` once and retry.

- [ ] **Step 5: Re-run Task 1's tests and commit**

```bash
cd "C:/Users/chw11/code/ultros/.claude/worktrees/sweet-bardeen-f8833d" && CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked -p ultros-app --lib retainer_live 2>&1 | tail -5 && cargo fmt --all && git add ultros-frontend/ultros-app/src/routes/retainer_live.rs && git commit -m "feat(retainers): use_retainer_live hook with debounced refetch

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

Expected: `7 passed`, then a commit.

---

### Task 3: Wire both pages, show the pill, drop the notice

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/retainers.rs`
  - imports (lines 1–28)
  - `RetainerUndercuts` (starts ~line 389)
  - `RetainerListings` (starts ~line 597)

**Interfaces:**
- Consumes: Task 2's `use_retainer_live(Signal<Option<Vec<ListedPair>>>, impl Fn() + Clone + 'static) -> RetainerLive { status, last_update }`; `crate::components::realtime_status::RealtimeStatus` (`status: Signal<String>`, `last_update: Signal<Option<DateTime<Utc>>>`, both `#[prop(into)]`).
- Produces: nothing downstream.

- [ ] **Step 1: Add imports**

At the top of `retainers.rs`, alongside the other `crate::components` imports:

```rust
use crate::components::realtime_status::RealtimeStatus;
use crate::routes::retainer_live::{ListedPair, use_retainer_live};
```

- [ ] **Step 2: Wire `RetainerUndercuts`**

Directly after the `retainers` resource definition (the `Resource::new(... get_retainer_undercuts ...)` block) and before `let (drawer_visible, set_drawer_visible) = signal(false);`, add:

```rust
    let listed_pairs = Signal::derive(move || {
        retainers.get().and_then(|result| {
            result.ok().map(|characters| {
                characters
                    .iter()
                    .flat_map(|(_, retainers)| retainers.iter())
                    .flat_map(|(_, undercuts)| undercuts.iter())
                    .map(|undercut| (undercut.current.world_id, undercut.current.item_id))
                    .collect::<Vec<ListedPair>>()
            })
        })
    });
    let live = use_retainer_live(listed_pairs, move || retainers.refetch());
```

Then in the `Some(Ok(_)) =>` arm of the view:

1. Change the title row so the pill sits between the title and the alert button:

```rust
                            <div class="flex flex-wrap items-center justify-between gap-3">
                                <span class="content-title">{t!(i18n, retainers_undercuts_title)}</span>
                                <div class="flex items-center gap-3">
                                    <RealtimeStatus status=live.status last_update=live.last_update />
                                    <button class="btn" on:click=move |_| set_drawer_visible.set(true)>
                                        <Icon icon=i::BsBell />
                                        <span class="ml-1">{t!(i18n, add_alert_button)}</span>
                                    </button>
                                </div>
                            </div>
```

2. Delete these four lines (the notice and its trailing `<br />`):

```rust
                            <br />
                            <span>
                                {t!(i18n, retainers_data_notice)}
                            </span>
```

so the `<Show when=move || drawer_visible.get()>` block is followed by a single `<br />` and then the `retainers_undercuts_description` span.

- [ ] **Step 3: Wire `RetainerListings`**

Directly after its `retainers` resource definition (`get_user_retainer_listings`) and before `view! {`, add:

```rust
    let listed_pairs = Signal::derive(move || {
        retainers.get().and_then(|result| {
            result.ok().map(|data| {
                data.retainers
                    .iter()
                    .flat_map(|(_, retainers)| retainers.iter())
                    .flat_map(|(_, listings)| listings.iter())
                    .map(|listing| (listing.world_id, listing.item_id))
                    .collect::<Vec<ListedPair>>()
            })
        })
    });
    let live = use_retainer_live(listed_pairs, move || retainers.refetch());
```

Replace the page's opening `<span class="content-title">{t!(i18n, retainers_all_listings_title)}</span>` (just above `<MetaTitle ...>`) with:

```rust
        <div class="flex flex-wrap items-center justify-between gap-3">
            <span class="content-title">{t!(i18n, retainers_all_listings_title)}</span>
            <RealtimeStatus status=live.status last_update=live.last_update />
        </div>
```

And inside the `Some(Ok(_)) =>` arm delete the notice:

```rust
                            <span>
                                {t!(i18n, retainers_data_notice)}
                            </span>
```

leaving the arm's `view!` starting directly with `{move || { match retainers.get() { ... } }}`.

- [ ] **Step 4: Confirm the notice key is no longer referenced in code**

```bash
cd "C:/Users/chw11/code/ultros/.claude/worktrees/sweet-bardeen-f8833d" && grep -rn "retainers_data_notice" ultros-frontend/ultros-app/src && echo "STILL REFERENCED" || echo "no code references"
```

Expected: `no code references`.

- [ ] **Step 5: Compile under both feature sets**

```bash
cd "C:/Users/chw11/code/ultros/.claude/worktrees/sweet-bardeen-f8833d" && cargo check --locked -p ultros-app 2>&1 | tail -5 && cargo check --locked -p ultros-app --no-default-features --features hydrate --target wasm32-unknown-unknown 2>&1 | tail -5
```

Expected: both `Finished`. A likely error is `RealtimeLive` fields moved into the view closure; `RetainerLive` is `Copy` so `live.status` / `live.last_update` inside `move ||` closures is fine — if the compiler complains, bind `let status = live.status; let last_update = live.last_update;` before the `view!` and use those.

- [ ] **Step 6: Commit**

```bash
cd "C:/Users/chw11/code/ultros/.claude/worktrees/sweet-bardeen-f8833d" && cargo fmt --all && git add ultros-frontend/ultros-app/src/routes/retainers.rs && git commit -m "feat(retainers): live status pill + refetch on listings/undercuts pages

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: Delete the stale-data notice key and run CI

**Files:**
- Modify: `ultros-frontend/ultros-app/locales/en.json` (line 754), `fr.json`, `de.json`, `ja.json`, `cn.json`, `ko.json`, `tc.json` (line 751 in each)

- [ ] **Step 1: Remove the key from all seven files**

Each file has exactly one line of the form `    "retainers_data_notice": "...",`. Delete that whole line in each file. Use the Edit tool per file (the values contain non-ASCII text; do not route them through a shell heredoc). Then verify:

```bash
cd "C:/Users/chw11/code/ultros/.claude/worktrees/sweet-bardeen-f8833d" && grep -c "retainers_data_notice" ultros-frontend/ultros-app/locales/*.json; for f in ultros-frontend/ultros-app/locales/*.json; do python -c "import json,sys; json.load(open(sys.argv[1], encoding='utf-8'))" "$f" && echo "$f ok"; done
```

Expected: every count is `0` (grep prints `:0` per file and exits 1, which is fine) and every file prints `ok` (still valid JSON, i.e. no dangling or missing comma).

- [ ] **Step 2: Run the full CI script**

```bash
cd "C:/Users/chw11/code/ultros/.claude/worktrees/sweet-bardeen-f8833d" && ./check_ci.sh > "$TMPDIR/ci.log" 2>&1; echo "REAL_EXIT=$?"; tail -30 "$TMPDIR/ci.log"
```

(Use the session scratchpad path for the log if `$TMPDIR` is unset.) Expected: `REAL_EXIT=0`. If clippy reports anything in `retainer_live.rs` or `retainers.rs`, fix the code, not with `#[allow]`. If clippy is OOM-killed (exit 137), re-run `cargo clippy --locked --all-targets -j 2 -- -D warnings` on its own.

- [ ] **Step 3: Commit**

```bash
cd "C:/Users/chw11/code/ultros/.claude/worktrees/sweet-bardeen-f8833d" && git add ultros-frontend/ultros-app/locales && git commit -m "i18n: drop retainers_data_notice, pages are live now

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

- [ ] **Step 4: Open the PR**

```bash
cd "C:/Users/chw11/code/ultros/.claude/worktrees/sweet-bardeen-f8833d" && git push -u origin claude/retainer-live-data-6eb801 && gh pr create --base main --title "Live data on retainer listings and undercuts pages" --body "$(cat <<'EOF'
## Summary
- `/retainers/listings` and `/retainers/undercuts` now subscribe to listing events for every (world, item) their retainers list and refetch on a relevant event, debounced 1.5 s (trailing). `Stale`/`Error` refetch immediately.
- Both pages show the existing `RealtimeStatus` pill in the title row; the "data may be stale, refresh" notice is gone and its i18n key removed from all seven locales.
- New `routes/retainer_live.rs`: pure `retainer_market_filter` / `is_retainer_update_relevant` (unit-tested) plus the `use_retainer_live` hook.
- Refetch rather than patch because listing events are deltas; the undercuts page needs the cheapest listing per world, which an event alone can't give.

Spec: `docs/superpowers/specs/2026-09-07-retainer-live-data-design.md`

## Test plan
- [x] `./check_ci.sh` green (fmt, clippy, unit tests incl. 7 new)
- [ ] After deploy, on prod while logged in: both pages show a "Live" pill; relisting an item in-game updates the undercuts table without a reload.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

Expected: a PR URL. Report it, and note the post-deploy check is still owed.
