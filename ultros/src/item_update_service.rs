use std::sync::Arc;

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
    pub(crate) fn start_service(self) {
        tokio::spawn(async move {
            loop {
                // check all worlds
                info!("Checking all worlds");
                // Create this 30 minute duration check now so that our refresh interval includes the time we spent checking
                let next_interval = Instant::now() + tokio::time::Duration::from_secs(60 * 30);
                for world in self.world_cache.get_all_worlds() {
                    info!("{world:?}");
                    let world = self.do_full_world_update(world).await;
                    if let Err(w) = world {
                        info!("{w:?}");
                    }
                }
                tokio::time::sleep_until(next_interval).await;
            }
        });
    }
    #[instrument(skip(self))]
    async fn do_full_world_update(&self, world: &world::Model) -> Result<(), anyhow::Error> {
        let world_name = &world.name;
        let world_id = WorldId(world.id);
        let updates = self.get_missing_updates(world_name).await?;
        let item_ids: Box<[i32]> = updates.into_iter().map(|i| i.item_id).collect();
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
                                .send(EventType::Add(Arc::new(SaleEventData { sales: added })));
                        }
                    }),
            )
            .buffer_unordered(50)
            .collect::<Vec<_>>()
            .await;
        }
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
}
