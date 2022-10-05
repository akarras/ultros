use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
};

use chrono::NaiveDateTime;
use futures::TryStreamExt;
use tracing::log::info;
use ultros_db::{
    entity::{active_listing, sale_history},
    UltrosDb,
};
use universalis::ItemId;

use crate::{
    event::EventReceivers,
    world_cache::{AnyResult, AnySelector, WorldCache},
};
use tokio::sync::RwLock;

#[derive(Hash, Eq, PartialEq, PartialOrd, Ord, Debug, Copy, Clone)]
struct ItemKey {
    item_id: i32,
    hq: bool,
}

impl From<&active_listing::Model> for ItemKey {
    fn from(model: &active_listing::Model) -> Self {
        let active_listing::Model { item_id, hq, .. } = *model;
        Self { item_id, hq }
    }
}

impl From<&sale_history::Model> for ItemKey {
    fn from(model: &sale_history::Model) -> Self {
        let sale_history::Model {
            sold_item_id, hq, ..
        } = *model;
        Self {
            item_id: sold_item_id,
            hq,
        }
    }
}

#[derive(Debug, PartialEq, PartialOrd, Eq, Ord, Clone, Copy)]
struct SaleSummary {
    sale_date: NaiveDateTime,
    price_per_item: i32,
}

#[derive(Debug, Default)]
struct SaleHistory {
    item_map: HashMap<ItemKey, BTreeSet<SaleSummary>>,
}

impl SaleHistory {
    pub(crate) fn add_sale(&mut self, sale: &ultros_db::entity::sale_history::Model) {
        let entries = self.item_map.entry(sale.into()).or_default();
        entries.insert(SaleSummary {
            sale_date: sale.sold_date,
            price_per_item: sale.price_per_item,
        });
        while entries.len() > 10 {
            if let Some(last) = entries.iter().last().copied() {
                entries.remove(&last);
            }
        }
        // TODO: use pop last once stabilized
        //while entries.len() > 10 {
        //    entries.pop_last();
        //}
    }
}

#[derive(Debug, Default)]
struct CheapestListings {
    item_map: HashMap<ItemKey, i32>,
}

impl CheapestListings {
    fn add_listing(&mut self, listing: &active_listing::Model) {
        let entry = self.item_map.entry(listing.into()).or_insert(i32::MAX);
        *entry = (*entry).min(listing.price_per_unit)
    }

    async fn remove_listing(
        &mut self,
        listing: &active_listing::Model,
        region_id: i32,
        world_cache: &WorldCache,
        ultros_db: &UltrosDb,
    ) {
        // if this was the cheapest listing we need to ask the database for the new cheapest item
        let entry = self.item_map.entry(listing.into()).or_insert(i32::MAX);
        if *entry == listing.price_per_unit {
            let worlds = world_cache
                .lookup_selector(&AnySelector::Region(region_id))
                .map(|r| world_cache.get_all_worlds_in(&r))
                .flatten()
                .expect("Should have worlds");
            if let Ok(listings) = ultros_db
                .get_all_listings_in_worlds_with_retainers(&worlds, ItemId(listing.item_id))
                .await
            {
                let listing_key: ItemKey = listing.into();
                for (db_listing, _) in &listings {
                    if listing_key == ItemKey::from(db_listing) {
                        *entry = db_listing.price_per_unit;
                    }
                }
            }
        }
    }
}

/// Build a short list of all the items in the game that we think would sell well.
/// Implemented as an easily cloneable Arc monster
#[derive(Debug, Clone)]
pub(crate) struct AnalyzerService {
    /// world_id -> TopSellers
    recent_sale_history: Arc<RwLock<HashMap<i32, SaleHistory>>>,
    /// Cheapest items by region. Not catering to slackers.
    cheapest_items: Arc<RwLock<HashMap<i32, CheapestListings>>>,
}

impl AnalyzerService {
    /// Creates a task that will feed the analyzer and returns Self so that data can be read externally
    pub async fn start_analyzer(
        ultros_db: UltrosDb,
        event_receivers: EventReceivers,
        world_cache: Arc<WorldCache>,
    ) -> Self {
        let temp = Self {
            recent_sale_history: Default::default(),
            cheapest_items: Default::default(),
        };

        let task_self = temp.clone();
        tokio::spawn(async move {
            task_self
                .run_worker(ultros_db, event_receivers, world_cache)
                .await;
        });
        temp
    }

    async fn run_worker(
        &self,
        ultros_db: UltrosDb,
        mut event_receivers: EventReceivers,
        world_cache: Arc<WorldCache>,
    ) {
        // on startup we should try to read through the database to get the spiciest of item listings
        info!("priming worker");
        for region in world_cache.get_all_regions() {
            info!("starting region {region:?}");
            if let Some(worlds) = world_cache.get_all_worlds_in(&AnyResult::Region(region)) {
                for world in &worlds {
                    // lets keep a lock on our local service for the duration that we have a stream to the database
                    let mut writer = self.cheapest_items.write().await;
                    let world_listings = writer.entry(region.id).or_default();
                    if let Ok(mut listings) = ultros_db.get_all_listings_for_world(*world).await {
                        while let Ok(Some(listing)) = listings.try_next().await {
                            world_listings.add_listing(&listing);
                        }
                    }
                    // let items = &xiv_gen_db::decompress_data().items;
                    // for item in items.values() {

                    // this version performed much slower in the dev database, but that also just could be because there's no data. keeping it around
                    // in case the streaming method kabooms
                    // if let Ok(mut listings) = ultros_db.get_cheapest_listing_by_world(world, id.0, true).await {
                    // if let Some(listing) = listings {
                    // self.add_listing(world, &listing).await;
                    // }
                    // }
                    // if let Ok(mut listings) = ultros_db.get_cheapest_listing_by_world(world, id.0, false).await {
                    // if let Some(listing) = listings {
                    // self.add_listing(world, &listing).await;
                    // }
                    // }
                    // }
                }
                // now prime sale history
                let mut writer = self.recent_sale_history.write().await;
                for world in &worlds {
                    let history = writer.entry(*world).or_default();
                    if let Ok(mut history_stream) =
                        ultros_db.stream_sales_within_days(10, *world).await
                    {
                        while let Ok(Some(sale)) = history_stream.try_next().await {
                            history.add_sale(&sale);
                        }
                    }
                }
            }
        }
        info!("worker primed, now using live data");
        loop {
            if let Ok(history) = event_receivers.history.recv().await {
                match history {
                    crate::event::EventType::Remove(_) => {},
                    crate::event::EventType::Add(sales) => {
                        for sale in sales.iter() {
                            self.add_sale(sale).await;
                        }
                    },
                    crate::event::EventType::Update(_) => {},
                }
            }
            if let Ok(listings) = event_receivers.listings.recv().await {
                match listings {
                    crate::event::EventType::Remove(remove) => {
                        let region = if let Some(region) = remove
                            .iter()
                            .map(|w| {
                                world_cache
                                    .lookup_selector(&AnySelector::World(w.world_id))
                                    .map(|w| world_cache.get_region(&w))
                            })
                            .flatten()
                            .flatten()
                            .next()
                        {
                            region.id
                        } else {
                            continue;
                        };
                        self.remove_listings(region, remove, &world_cache, &ultros_db).await;
                    }
                    crate::event::EventType::Add(add) => {
                        let region = if let Some(region) = add
                            .iter()
                            .map(|w| {
                                world_cache
                                    .lookup_selector(&AnySelector::World(w.world_id))
                                    .map(|w| world_cache.get_region(&w).map(|r| r.id))
                            })
                            .flatten()
                            .flatten()
                            .next()
                        {
                            region
                        } else {
                            continue;
                        };
                        self.add_listings(region, &add).await;
                    }
                    crate::event::EventType::Update(_) => todo!(),
                }
            }
        }
    }

    pub(crate) async fn get_best_resale(
        &self,
        world_id: i32,
        region_id: i32,
    ) -> Option<Vec<ResaleStats>> {
        let recent_sale = self.recent_sale_history.read().await;
        // figure out what items are selling best on our world first, then figure out what items are available in the region that complement that.
        let sale = recent_sale.get(&world_id)?;
        let sale_history: BTreeMap<_, _> = sale
            .item_map
            .iter()
            .flat_map(|(item, values)| {
                values
                    .iter()
                    .map(|sale| sale.price_per_item)
                    .min()
                    .map(|price| (*item, price))
            })
            .collect();
        drop(recent_sale);

        let items = self.cheapest_items.read().await;
        let region = items.get(&region_id)?;
        let mut possible_sales: Vec<_> = region
            .item_map
            .iter()
            .flat_map(|(item_key, cheapest_price)| {
                let history = sale_history.get(item_key)?;
                Some(ResaleStats {
                    profit: *history - *cheapest_price,
                    cheapest: *cheapest_price,
                    item_id: item_key.item_id,
                    hq: item_key.hq,
                })
            })
            .collect();
        drop(items);
        possible_sales.sort_by(|a, b| {
            b.profit
                .cmp(&a.profit)
                .then_with(|| a.cheapest.cmp(&b.cheapest))
        });
        Some(possible_sales)
    }

    /// process listings in bulk. can handle multiple item types, but must have only one region.
    async fn add_listings(&self, region_id: i32, listings: &[active_listing::Model]) {
        let mut lock_guard = self.cheapest_items.write().await;
        let entry = lock_guard.entry(region_id).or_default();
        for listing in listings {
            entry.add_listing(listing);
        }
    }

    /// remove listings in bulk. can handle multiple item types, but must have only one region.
    async fn remove_listings(
        &self,
        region_id: i32,
        listings: Arc<Vec<active_listing::Model>>,
        world_cache: &WorldCache,
        ultros_db: &UltrosDb,
    ) {
        let mut lock_guard = self.cheapest_items.write().await;
        let entry = lock_guard.entry(region_id).or_default();
        for listing in listings.iter() {
            entry.remove_listing(listing, region_id, &world_cache, &ultros_db).await;
        }
    }

    async fn add_sale(&self, sale: &sale_history::Model) {
        let mut lock_guard = self.recent_sale_history.write().await;
        let entry = lock_guard.entry(sale.world_id).or_default();
        entry.add_sale(sale);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Ord, Eq)]
pub(crate) struct ResaleStats {
    pub(crate) profit: i32,
    pub(crate) cheapest: i32,
    pub(crate) item_id: i32,
    pub(crate) hq: bool,
}
