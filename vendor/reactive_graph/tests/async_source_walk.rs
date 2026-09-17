//! Regression test for akarras/ultros#1511: a memo that reads an async
//! derived must not eat the async derived's dirty flag.
//!
//! The graph is the one `ArcResource` builds — an `ArcAsyncDerived` with
//! manual dependencies on a source `ArcMemo` that reads a chain of memos —
//! plus the readers a page hangs off it: an effect on one of the chain's
//! memos (a label), an effect reading the async value directly (a `Suspense`
//! child), and an effect reading a memo that reads *both* the async value
//! and another memo of the same root signal (a grid over `rows`).
//!
//! When the grid effect runs before the async task, its walk of `rows`
//! reaches the async derived's `update_if_necessary`, which walks and
//! recomputes the source memo; that marks the async derived dirty and the
//! walk reports the change. Walking the same sources a second time — which
//! the vendored `MemoInner::update_if_necessary` used to do after taking its
//! compute lock — calls the async derived's `update_if_necessary` again,
//! and that call *consumes* the dirty flag. The async task then wakes to a
//! clean node over an already-clean source and never reruns its future:
//! the resource's key changed and nothing was fetched.

use any_spawner::Executor;
use reactive_graph::{
    computed::{ArcAsyncDerived, ArcMemo, Memo},
    effect::Effect,
    graph::{Source, ToAnySubscriber},
    owner::Owner,
    signal::ArcRwSignal,
    traits::{Get, Set, WithUntracked},
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

async fn settle() {
    for _ in 0..8 {
        Executor::tick().await;
    }
}

#[derive(Clone, Copy, Debug)]
struct Shape {
    label: bool,
    view: bool,
    grid: bool,
    /// `true` mirrors hydration: the value is already present and the
    /// future is not run at creation.
    hydrated: bool,
}

async fn run(shape: Shape) {
    let owner = Owner::new();
    owner.set();

    // The router's URL, and its query-map memo.
    let url = ArcRwSignal::new((30u16, 0i32));
    let query_map = Memo::new({
        let url = url.clone();
        move |_| url.get()
    });
    // The window param, the resolved window, and its day count.
    let window_raw = Memo::new(move |_| Some(query_map.get().0));
    let selected = Memo::new(move |_| window_raw.get().unwrap_or(30));
    let days = Memo::new(move |_| selected.get());
    // A second query param, read by `rows` alongside the async value.
    let category = Memo::new(move |_| query_map.get().1);

    // `ArcResource::new_with_options`, minus serialization.
    let refetch = ArcRwSignal::new(0);
    let source = ArcMemo::new({
        let refetch = refetch.clone();
        move |_| (refetch.get(), ("world".to_string(), days.get()))
    });
    let runs = Arc::new(AtomicUsize::new(0));
    let fun = {
        let source = source.clone();
        let runs = runs.clone();
        move || {
            let (_, key) = source.get();
            let runs = runs.clone();
            async move {
                runs.fetch_add(1, Ordering::SeqCst);
                Executor::tick().await;
                key
            }
        }
    };
    let initial = shape.hydrated.then(|| ("world".to_string(), 30u16));
    let data =
        ArcAsyncDerived::new_with_manual_dependencies(initial, fun, &source);
    if shape.hydrated {
        source.with_untracked(|_| ());
        source.add_subscriber(data.to_any_subscriber());
    }

    let rows = Memo::new({
        let data = data.clone();
        move |_| (data.get(), category.get())
    });

    if shape.label {
        Effect::new(move |_| {
            let _ = selected.get();
        });
    }
    if shape.view {
        Effect::new({
            let data = data.clone();
            move |_| {
                let _ = data.get();
            }
        });
    }
    if shape.grid {
        Effect::new(move |_| {
            let _ = rows.get();
        });
    }
    settle().await;
    let base = runs.load(Ordering::SeqCst);

    url.set((7, 0));
    settle().await;
    assert_eq!(
        runs.load(Ordering::SeqCst),
        base + 1,
        "{shape:?}: the future must rerun after the key changes to 7"
    );
    assert_eq!(data.clone().await.1, 7, "{shape:?}");

    url.set((90, 0));
    settle().await;
    assert_eq!(
        runs.load(Ordering::SeqCst),
        base + 2,
        "{shape:?}: the future must rerun after the key changes to 90"
    );
    assert_eq!(data.clone().await.1, 90, "{shape:?}");

    // A change that only `rows` sees must not rerun the future.
    url.set((90, 5));
    settle().await;
    assert_eq!(
        runs.load(Ordering::SeqCst),
        base + 2,
        "{shape:?}: a change outside the key must not rerun the future"
    );
    assert_eq!(rows.get().1, 5, "{shape:?}: rows still follow the change");
}

macro_rules! shape_test {
    ($name:ident, $label:expr, $view:expr, $grid:expr, $hydrated:expr) => {
        #[tokio::test]
        async fn $name() {
            _ = Executor::init_tokio();
            tokio::task::LocalSet::new()
                .run_until(run(Shape {
                    label: $label,
                    view: $view,
                    grid: $grid,
                    hydrated: $hydrated,
                }))
                .await;
        }
    };
}

// The shapes that never failed, kept as the control group.
shape_test!(no_readers_hydrated, false, false, false, true);
shape_test!(no_readers_fresh, false, false, false, false);
shape_test!(label_only_hydrated, true, false, false, true);
shape_test!(label_only_fresh, true, false, false, false);
shape_test!(view_only_hydrated, false, true, false, true);
shape_test!(view_only_fresh, false, true, false, false);
// The shapes that reproduced #1511: a memo reader over the async value.
shape_test!(grid_only_hydrated, false, false, true, true);
shape_test!(grid_only_fresh, false, false, true, false);
shape_test!(label_and_grid_hydrated, true, false, true, true);
shape_test!(all_readers_hydrated, true, true, true, true);
shape_test!(all_readers_fresh, true, true, true, false);
