use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use chrono::{Duration, Local, NaiveDateTime};
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
use tracing::log::error;

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

impl From<&ultros_db::sales::AbbreviatedSaleData> for ItemKey {
    fn from(sale_data: &ultros_db::sales::AbbreviatedSaleData) -> Self {
        Self {
            item_id: sale_data.sold_item_id,
            hq: sale_data.hq,
        }
    }
}

impl From<&ultros_db::listings::ListingSummary> for ItemKey {
    fn from(sum: &ultros_db::listings::ListingSummary) -> Self {
        Self {
            item_id: sum.item_id,
            hq: sum.hq,
        }
    }
}

#[derive(Debug, PartialEq, PartialOrd, Eq, Ord, Clone, Copy)]
struct SaleSummary {
    sale_date: NaiveDateTime,
    price_per_item: i32,
}

impl From<&ultros_db::sales::AbbreviatedSaleData> for SaleSummary {
    fn from(sale: &ultros_db::sales::AbbreviatedSaleData) -> Self {
        Self {
            sale_date: sale.sold_date,
            price_per_item: sale.price_per_item,
        }
    }
}

impl From<&ultros_db::entity::sale_history::Model> for SaleSummary {
    fn from(sale: &ultros_db::entity::sale_history::Model) -> Self {
        Self {
            sale_date: sale.sold_date,
            price_per_item: sale.price_per_item,
        }
    }
}

#[derive(Debug, Default)]
struct SaleHistory {
    item_map: HashMap<ItemKey, Vec<SaleSummary>>,
}

impl SaleHistory {
    pub(crate) fn add_sale<'a, T>(&mut self, sale: &'a T)
    where
        &'a T: Into<SaleSummary> + Into<ItemKey>,
    {
        let entries = self
            .item_map
            .entry(sale.into())
            .or_insert(Vec::with_capacity(4));

        entries.push(sale.into());
        entries.sort();
        entries.truncate(3);
    }
}

#[derive(Debug, Copy, Clone, Eq, Ord, PartialEq, PartialOrd)]
struct CheapestListingValue {
    price: i32,
    world_id: i32,
}

impl From<&ultros_db::entity::active_listing::Model> for CheapestListingValue {
    fn from(from: &ultros_db::entity::active_listing::Model) -> Self {
        Self {
            price: from.price_per_unit,
            world_id: from.world_id,
        }
    }
}

impl From<&ultros_db::listings::ListingSummary> for CheapestListingValue {
    fn from(from: &ultros_db::listings::ListingSummary) -> Self {
        Self {
            price: from.price_per_unit,
            world_id: from.world_id,
        }
    }
}

#[derive(Debug, Default)]
struct CheapestListings {
    item_map: HashMap<ItemKey, CheapestListingValue>,
}

impl CheapestListings {
    fn add_listing<'a, T>(&mut self, listing: &'a T)
    where
        &'a T: Into<CheapestListingValue> + Into<ItemKey>,
    {
        let cheapest_listing = listing.into();
        let entry = self
            .item_map
            .entry(listing.into())
            .or_insert(cheapest_listing);
        *entry = (*entry).min(cheapest_listing);
    }

    async fn remove_listing(
        &mut self,
        listing: &active_listing::Model,
        region_id: i32,
        world_cache: &WorldCache,
        ultros_db: &UltrosDb,
    ) {
        // if this was the cheapest listing we need to ask the database for the new cheapest item
        let key = listing.into();
        if let Some(entry) = self.item_map.remove(&key) {
            if entry.price == listing.price_per_unit {
                let worlds = world_cache
                    .lookup_selector(&AnySelector::Region(region_id))
                    .map(|r| world_cache.get_all_worlds_in(&r))
                    .ok()
                    .flatten()
                    .expect("Should have worlds");
                if let Ok(listings) = ultros_db
                    .get_all_listings_in_worlds_with_retainers(&worlds, ItemId(listing.item_id))
                    .await
                {
                    for (db_listing, _) in &listings {
                        if key == ItemKey::from(db_listing) {
                            self.add_listing(db_listing);
                        }
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
                    let mut listings = ultros_db
                        .stream_cheapest_listings_on_world(*world)
                        .await
                        .expect("failed to stream listings");
                    let mut stream_result = listings.try_next().await;
                    while let Ok(Some(listing)) = stream_result {
                        world_listings.add_listing(&listing);
                        stream_result = listings.try_next().await;
                    }
                    if let Err(e) = stream_result {
                        error!("Streaming item listings failed {e:?}");
                    }
                }
                // now prime sale history
                let mut writer = self.recent_sale_history.write().await;
                for world in &worlds {
                    let history = writer.entry(*world).or_default();
                    let mut history_stream = ultros_db
                        .stream_last_n_sales_by_world(*world, 4)
                        .await
                        .expect("Failed to stream history");
                    let mut stream_result = history_stream.try_next().await;
                    while let Ok(Some(sale)) = stream_result {
                        history.add_sale(&sale);
                        stream_result = history_stream.try_next().await;
                    }
                    if let Err(e) = stream_result {
                        error!("Streaming sale history failed {e:?}");
                    }
                }
            }
        }
        info!("worker primed, now using live data");
        let second_worker_instance = self.clone();
        tokio::spawn(async move {
            loop {
                if let Ok(history) = event_receivers.history.recv().await {
                    match history {
                        crate::event::EventType::Remove(_) => {}
                        crate::event::EventType::Add(sales) => {
                            for sale in sales.iter() {
                                second_worker_instance.add_sale(sale).await;
                            }
                        }
                        crate::event::EventType::Update(_) => {}
                    }
                }
            }
        });
        loop {
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
                        self.remove_listings(region, remove, &world_cache, &ultros_db)
                            .await;
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
        resale_options: ResaleOptions,
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
                    .filter(|sale| {
                        Local::now().naive_local().signed_duration_since(sale.sale_date)
                            .lt(&Duration::days(resale_options.days as i64))
                    })
                    .map(|sale| sale.price_per_item)
                    .min()
                    .map(|price| (*item, price))
            })
            .collect();
        drop(recent_sale);

        let items = self.cheapest_items.read().await;
        let region = items.get(&region_id)?;
        let possible_sales: Vec<_> = region
            .item_map
            .iter()
            .flat_map(|(item_key, cheapest_price)| {
                let history = sale_history.get(item_key)?;
                Some(ResaleStats {
                    profit: *history - cheapest_price.price,
                    cheapest: cheapest_price.price,
                    item_id: item_key.item_id,
                    hq: item_key.hq,
                    return_on_investment: ((*history as f32) / (cheapest_price.price as f32)
                        * 100.0)
                        - 100.0,
                    world_id: cheapest_price.world_id,
                })
            })
            .filter(|w| resale_options.minimum_profit.map(|m| m.lt(&w.profit)).unwrap_or(true))
            .collect();
        drop(items);

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
            entry
                .remove_listing(listing, region_id, &world_cache, &ultros_db)
                .await;
        }
    }

    async fn add_sale(&self, sale: &sale_history::Model) {
        let mut lock_guard = self.recent_sale_history.write().await;
        let entry = lock_guard.entry(sale.world_id).or_default();
        entry.add_sale(sale);
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ResaleStats {
    pub(crate) profit: i32,
    pub(crate) cheapest: i32,
    pub(crate) item_id: i32,
    pub(crate) return_on_investment: f32,
    pub(crate) hq: bool,
    pub(crate) world_id: i32,
}

pub(crate) struct ResaleOptions {
    pub(crate) days: i32,
    pub(crate) minimum_profit: Option<i32>,
}
