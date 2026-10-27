//! Keep live changes made while a replacement price map is being loaded.
//! Record and reconcile keys while holding the corresponding selector's write
//! lock. The tracker never holds its mutex across an await or database call.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
};

use super::{AnySelector, CheapestListings, ItemKey};

/// Stage a complete history stream before the caller publishes any of it.
pub(super) async fn load_history<S, E>(
    worlds: impl Iterator<Item = i32>,
    stream: S,
) -> Result<BTreeMap<i32, super::SaleHistory>, E>
where
    S: futures::Stream<Item = Result<ultros_db::sales::AbbreviatedSaleData, E>>,
{
    use futures::TryStreamExt;

    let mut history: BTreeMap<_, super::SaleHistory> =
        worlds.map(|world| (world, Default::default())).collect();
    futures::pin_mut!(stream);
    while let Some(value) = stream.try_next().await? {
        if let Some(history) = history.get_mut(&value.world_id) {
            history.add_sale(&value);
        } else {
            super::skipped_event("db_reload_sale", "unknown_world");
        }
    }
    Ok(history)
}

type ChangedKeys = BTreeMap<AnySelector, BTreeSet<ItemKey>>;

#[derive(Default)]
pub(super) struct RebuildTracker {
    changed: Mutex<Option<ChangedKeys>>,
}

impl RebuildTracker {
    pub fn begin(&self) -> anyhow::Result<Rebuild<'_>> {
        let mut changed = self.changed.lock().unwrap_or_else(|e| e.into_inner());
        anyhow::ensure!(changed.is_none(), "a price rebuild is already running");
        *changed = Some(BTreeMap::new());
        Ok(Rebuild { tracker: self })
    }

    /// Call under the selector's write lock, before changing its prices.
    pub fn record(&self, selector: AnySelector, key: ItemKey) {
        let mut changed = self.changed.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(changed) = changed.as_mut() {
            changed.entry(selector).or_default().insert(key);
        }
    }
}

pub(super) struct Rebuild<'a> {
    tracker: &'a RebuildTracker,
}

impl Rebuild<'_> {
    /// Call under the selector's write lock immediately before replacement.
    pub fn preserve_live_changes(
        &self,
        selector: AnySelector,
        current: &CheapestListings,
        replacement: &mut CheapestListings,
    ) {
        let keys = self
            .tracker
            .changed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
            .and_then(|changed| changed.remove(&selector))
            .unwrap_or_default();
        for key in keys {
            if let Some(value) = current.item_map.get(&key) {
                replacement.item_map.insert(key, *value);
            } else {
                replacement.item_map.remove(&key);
            }
        }
    }
}

impl Drop for Rebuild<'_> {
    fn drop(&mut self) {
        // Also release the journal when loading fails or is cancelled.
        *self
            .tracker
            .changed
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer_service::CheapestListingValue;

    fn key(id: i32) -> ItemKey {
        ItemKey {
            item_id: id,
            hq: false,
        }
    }

    fn prices(values: &[(i32, i32)]) -> CheapestListings {
        CheapestListings {
            item_map: values
                .iter()
                .map(|(id, price)| {
                    (
                        key(*id),
                        CheapestListingValue {
                            price: *price,
                            world_id: 54,
                        },
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn rebuild_keeps_live_reprices_additions_and_removals_but_heals_unchanged_keys() {
        let tracker = RebuildTracker::default();
        let rebuild = tracker.begin().unwrap();
        let world = AnySelector::World(54);
        // Snapshot taken before a reprice, a new listing, and a sold-out board.
        let mut snapshot = prices(&[(1, 100), (3, 300), (4, 400)]);
        let live = prices(&[(1, 120), (2, 200), (4, 1), (5, 1)]);
        for id in [1, 2, 3] {
            tracker.record(world, key(id));
        }
        rebuild.preserve_live_changes(world, &live, &mut snapshot);
        assert_eq!(snapshot.item_map[&key(1)].price, 120);
        assert_eq!(snapshot.item_map[&key(2)].price, 200);
        assert!(!snapshot.item_map.contains_key(&key(3)));
        assert_eq!(
            snapshot.item_map[&key(4)].price,
            400,
            "heal a phantom minimum"
        );
        assert!(
            !snapshot.item_map.contains_key(&key(5)),
            "clear a vanished key"
        );
    }

    #[test]
    fn selectors_are_independent_and_an_aborted_rebuild_can_be_retried() {
        let tracker = RebuildTracker::default();
        {
            let _aborted = tracker.begin().unwrap();
            assert!(tracker.begin().is_err());
            tracker.record(AnySelector::World(54), key(1));
        }
        let rebuild = tracker.begin().unwrap();
        tracker.record(AnySelector::Region(1), key(1));
        let mut world = prices(&[(1, 100)]);
        rebuild.preserve_live_changes(AnySelector::World(54), &prices(&[(1, 50)]), &mut world);
        assert_eq!(world.item_map[&key(1)].price, 100);
        let mut region = prices(&[(1, 100)]);
        rebuild.preserve_live_changes(AnySelector::Region(1), &prices(&[(1, 50)]), &mut region);
        assert_eq!(region.item_map[&key(1)].price, 50);
    }

    #[tokio::test]
    async fn a_history_stream_error_discards_the_successful_prefix() {
        let sale = ultros_db::sales::AbbreviatedSaleData {
            sold_item_id: 1,
            hq: false,
            price_per_item: 100,
            sold_date: chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc(),
            world_id: 54,
        };
        let rows = futures::stream::iter([Ok(sale), Err("stream disconnected")]);
        let result = load_history([54].into_iter(), rows).await;
        assert_eq!(result.unwrap_err(), "stream disconnected");
        let empty = load_history(
            [54].into_iter(),
            futures::stream::empty::<Result<_, &str>>(),
        )
        .await
        .unwrap();
        assert!(empty[&54].item_map.is_empty());
    }
}
