use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use chrono::{Duration, Local, NaiveDateTime};
use futures::StreamExt;
use tracing::log::info;
use ultros_db::{
    entity::{active_listing, sale_history},
    UltrosDb,
};
use universalis::ItemId;

use crate::{
    event::EventReceivers,
    world_cache::{AnySelector, WorldCache},
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
    price_per_item: i32,
    sale_date: NaiveDateTime,
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
        entries.shrink_to(4);
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
        id: AnySelector,
        world_cache: &WorldCache,
        ultros_db: &UltrosDb,
    ) {
        // if this was the cheapest listing we need to ask the database for the new cheapest item
        let key = listing.into();
        if let Some(entry) = self.item_map.remove(&key) {
            if entry.price == listing.price_per_unit {
                let worlds = world_cache
                    .lookup_selector(&id)
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
    cheapest_items: Arc<RwLock<HashMap<AnySelector, CheapestListings>>>,
    initiated: Arc<AtomicBool>,
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
            initiated: Arc::default(),
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
        info!("worker starting");
        let listings = ultros_db.cheapest_listings().await;
        info!("starting item listings");
        match listings {
            Ok(mut listings) => {
                let mut writer = self.cheapest_items.write().await;
                while let Some(Ok(value)) = listings.next().await {
                    let world = world_cache
                        .lookup_selector(&AnySelector::World(value.world_id))
                        .unwrap();
                    let region = world_cache.get_region(&world).unwrap();
                    let region_listings = writer.entry(AnySelector::Region(region.id)).or_default();
                    region_listings.add_listing(&value);
                    let world_listings = writer
                        .entry(AnySelector::World(value.world_id))
                        .or_default();
                    world_listings.add_listing(&value);
                }
            }
            Err(e) => {
                error!("Streaming item listings failed {e:?}");
            }
        }
        info!("starting sale data");
        let sale_data = ultros_db.last_n_sales(3).await;
        match sale_data {
            Ok(mut history_stream) => {
                let mut writer = self.recent_sale_history.write().await;
                while let Some(Ok(value)) = history_stream.next().await {
                    let history = writer.entry(value.world_id).or_default();
                    history.add_sale(&value);
                }
            }
            Err(e) => {
                error!("Streaming item listings failed {e:?}");
            }
        }
        self.initiated.store(true, Ordering::Relaxed);
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
                        self.add_listings(&add, &world_cache).await;
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
        world_cache: &Arc<WorldCache>,
    ) -> Option<Vec<ResaleStats>> {
        if !self.initiated.load(Ordering::Relaxed) {
            return None;
        }
        let recent_sale = self.recent_sale_history.read().await;
        let datacenter_filters_worlds = resale_options
            .filter_datacenter
            .map(|w| {
                world_cache
                    .lookup_selector(&AnySelector::Datacenter(w))
                    .ok()
                    .map(|w| world_cache.get_all_worlds_in(&w))
                    .flatten()
            })
            .flatten();
        // figure out what items are selling best on our world first, then figure out what items are available in the region that complement that.
        let sale = recent_sale.get(&world_id)?;
        let sale_history: BTreeMap<_, _> = sale
            .item_map
            .iter()
            .flat_map(|(item, values)| {
                values
                    .iter()
                    .filter(|sale| {
                        // TODO this date doesn't seem correct
                        Local::now()
                            .naive_utc()
                            .signed_duration_since(sale.sale_date)
                            .lt(&Duration::days(resale_options.days as i64))
                    })
                    .map(|sale| sale.price_per_item)
                    .min()
                    .map(|price| (*item, price))
            })
            .collect();
        drop(recent_sale);

        let items = self.cheapest_items.read().await;
        let region = items.get(&AnySelector::Region(region_id))?;
        let sale_world_listings = items.get(&AnySelector::World(world_id))?;
        let possible_sales: Vec<_> = region
            .item_map
            .iter()
            .flat_map(|(item_key, cheapest_price)| {
                let history = sale_history.get(item_key)?;
                let current_cheapest_on_sale_world = sale_world_listings
                    .item_map
                    .get(item_key)
                    .map(|l| l.price)
                    .unwrap_or(*history);
                let est_sale_price = (*history).min(current_cheapest_on_sale_world);
                let profit = est_sale_price - cheapest_price.price;
                Some(ResaleStats {
                    profit,
                    cheapest: cheapest_price.price,
                    item_id: item_key.item_id,
                    hq: item_key.hq,
                    return_on_investment: ((est_sale_price as f32) / (cheapest_price.price as f32)
                        * 100.0)
                        - 100.0,
                    world_id: cheapest_price.world_id,
                })
            })
            .filter(|w| {
                resale_options
                    .minimum_profit
                    .map(|m| m.lt(&w.profit))
                    .unwrap_or(true)
            })
            .filter(|sale| {
                resale_options
                    .filter_world
                    .map(|w| sale.world_id.eq(&w))
                    .unwrap_or(true)
            })
            .filter(|sale| {
                datacenter_filters_worlds
                    .as_ref()
                    .map(|dc| dc.contains(&sale.world_id))
                    .unwrap_or(true)
            })
            .collect();
        drop(items);

        Some(possible_sales)
    }

    /// process listings in bulk.
    async fn add_listings(
        &self,
        listings: &[active_listing::Model],
        world_cache: &Arc<WorldCache>,
    ) {
        let mut lock_guard = self.cheapest_items.write().await;
        // process all listings from one world at a time
        let listings = listings.iter().flat_map(|l| {
            let result = world_cache
                .lookup_selector(&AnySelector::World(l.world_id))
                .ok()?;
            Some((
                AnySelector::World(l.world_id),
                AnySelector::Region(world_cache.get_region(&result)?.id),
                l,
            ))
        });

        for (world_selector, region_selector, listing) in listings {
            let entry = lock_guard.entry(region_selector).or_default();
            entry.add_listing(listing);
            let entry = lock_guard.entry(world_selector).or_default();
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
        let entry = lock_guard
            .entry(AnySelector::Region(region_id))
            .or_default();
        for listing in listings.iter() {
            entry
                .remove_listing(
                    listing,
                    AnySelector::Region(region_id),
                    &world_cache,
                    &ultros_db,
                )
                .await;
        }

        for listing in listings.iter() {
            let world = lock_guard
                .entry(AnySelector::World(listing.world_id))
                .or_default();
            world
                .remove_listing(
                    listing,
                    AnySelector::World(listing.world_id),
                    &world_cache,
                    &ultros_db,
                )
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
    pub(crate) filter_world: Option<i32>,
    pub(crate) filter_datacenter: Option<i32>,
}
