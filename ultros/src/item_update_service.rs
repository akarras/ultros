use std::{sync::Arc, time::Duration};

use futures::{stream, StreamExt};
use tokio::time::Instant;
use tracing::{info, instrument};
use ultros_api_types::websocket::{ListingEventData, SaleEventData};
use ultros_db::{
    entity::{listing_last_updated::Model, world},
    partial_diff_iterator::PartialDiffIterator,
    world_cache::WorldCache,
    UltrosDb,
};
use universalis::{UniversalisClient, WorldId, WorldItemRecencyView};

use crate::event::{EventProducer, EventType};

/// Item update service attempts to keep ultros' data in sync with Universalis' data.
/// It does this primarily by comparing the recently updated items on Universalis with recently updated items on ultros

pub(crate) struct UpdateService {
    pub(crate) db: UltrosDb,
    pub(crate) world_cache: Arc<WorldCache>,
    pub(crate) universalis: UniversalisClient,
    pub(crate) listings: EventProducer<ListingEventData>,
    pub(crate) sales: EventProducer<SaleEventData>,
}

struct CmpListing(Model);

impl PartialOrd<WorldItemRecencyView> for CmpListing {
    fn partial_cmp(&self, other: &WorldItemRecencyView) -> Option<std::cmp::Ordering> {
        self.0.item_id.partial_cmp(&other.item_id)
    }
}

impl PartialEq<WorldItemRecencyView> for CmpListing {
    fn eq(&self, other: &WorldItemRecencyView) -> bool {
        self.0.item_id.eq(&other.item_id)
    }
}

impl UpdateService {
    pub(crate) fn start_service(service: Arc<Self>) {
        tokio::spawn(async move {
            loop {
                // check all worlds
                info!("Checking all worlds");
                // Create this 5 minute duration check now so that our refresh interval includes the time we spent checking
                let next_interval = Instant::now() + tokio::time::Duration::from_secs(60 * 5);
                for world in service.world_cache.get_all_worlds() {
                    info!("{world:?}");
                    let world = service.check_for_missed_items_on_world(world).await;
                    if let Err(w) = world {
                        info!("{w:?}");
                    }
                }
                tokio::time::sleep_until(next_interval).await;
            }
        });
    }

    /// Sweeps over every single marketable item in the game, ignoring the recency cache. Only should be used if data is known to be lost.
    pub(crate) async fn do_full_world_sweep(&self) -> Result<(), anyhow::Error> {
        let all_marketable_items: Box<[i32]> = xiv_gen_db::data()
            .items
            .values()
            .filter(|i| i.item_search_category.0 != 0)
            .map(|i| i.key_id.0)
            .collect();
        for world in self.world_cache.get_all_worlds() {
            tracing::info!("scanning items");
            self.check_items(world, &all_marketable_items).await?;
        }
        Ok(())
    }

    #[instrument(level = "trace", skip(self))]
    async fn check_for_missed_items_on_world(
        &self,
        world: &world::Model,
    ) -> Result<(), anyhow::Error> {
        let updates = self.get_missing_updates(&world.name).await?;
        let item_ids: Box<[i32]> = updates.into_iter().map(|i| i.item_id).collect();
        self.check_items(world, &item_ids).await?;
        Ok(())
    }

    async fn get_missing_updates(
        &self,
        world_name: &str,
    ) -> Result<Vec<WorldItemRecencyView>, anyhow::Error> {
        let world = self
            .world_cache
            .lookup_value_by_name(world_name)?
            .as_world()?;
        let mut recently_updated = self
            .universalis
            .recently_updated_items(universalis::WorldOrDatacenter::World(world_name), 200)
            .await?;
        let mut our_recently_updated = self
            .db
            .get_recently_updated_listings_for_world(
                world.id,
                recently_updated.items.len() as u64 * 2,
            )
            .await?
            .into_iter()
            .map(CmpListing)
            .collect::<Vec<_>>();
        our_recently_updated.sort_by_key(|i| i.0.item_id);
        recently_updated.items.sort_by_key(|i| i.item_id);
        let diff = PartialDiffIterator::new(
            our_recently_updated.into_iter(),
            recently_updated.items.into_iter(),
        )
        .flat_map(|i| i.right())
        .collect();
        Ok(diff)
    }

    async fn check_items(
        &self,
        world::Model {
            id,
            name: world_name,
            ..
        }: &world::Model,
        item_ids: &[i32],
    ) -> Result<(), anyhow::Error> {
        let world_id = WorldId(*id);
        for item_ids in item_ids.chunks(100) {
            let market_data = self
                .universalis
                .marketboard_current_data(world_name, item_ids)
                .await?;
            info!("missing data {item_ids:?}");

            stream::iter(
                market_data
                    .items()
                    .map(|(item_id, listings, sales)| async move {
                        if let Ok((added, removed)) =
                            self.db.update_listings(listings, item_id, world_id).await
                        {
                            let _ =
                                self.listings
                                    .send(EventType::Add(Arc::new(ListingEventData {
                                        item_id: item_id.0,
                                        world_id: world_id.0,
                                        listings: added,
                                    })));
                            let _ =
                                self.listings
                                    .send(EventType::Remove(Arc::new(ListingEventData {
                                        item_id: item_id.0,
                                        world_id: world_id.0,
                                        listings: removed,
                                    })));
                        }
                        if let Ok(added) = self.db.update_sales(sales, item_id, world_id).await {
                            let _ = self
                                .sales
                                .send(EventType::added(SaleEventData { sales: added }));
                        }
                    }),
            )
            .buffer_unordered(50)
            .collect::<Vec<_>>()
            .await;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        Ok(())
    }
}
