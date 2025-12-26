use anyhow::{Result, anyhow};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, btree_map::Entry},
    fmt::Display,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use chrono::{Duration, NaiveDateTime, Utc};
use futures::StreamExt;
use itertools::Itertools;
use poise::serenity_prelude::Timestamp;
use serde::{Deserialize, Serialize};
use tokio::fs;
use tracing::log::{error, info};
use ultros_api_types::{ActiveListing, Retainer, websocket::ListingEventData};
use ultros_db::{
    UltrosDb,
    entity::{active_listing, sale_history},
};
use universalis::{ItemId, WorldId};

use crate::event::EventReceivers;
use thiserror::Error;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use ultros_api_types::trends::{TrendItem, TrendsData};
use ultros_db::world_cache::{AnySelector, WorldCache};

pub const SALE_HISTORY_SIZE: usize = 6;

#[derive(Debug, Error)]
pub enum AnalyzerError {
    #[error("Still warming up with data, unable to serve requests.")]
    Uninitialized,
    #[error("Data not found")]
    NotFound,
}

#[derive(
    Hash,
    Eq,
    PartialEq,
    PartialOrd,
    Ord,
    Debug,
    Copy,
    Clone,
    Serialize,
    Archive,
    RkyvDeserialize,
    RkyvSerialize,
)]
#[archive(check_bytes)]
pub(crate) struct ItemKey {
    pub(crate) item_id: i32,
    pub(crate) hq: bool,
}

impl Ord for ArchivedItemKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.item_id
            .cmp(&other.item_id)
            .then_with(|| self.hq.cmp(&other.hq))
    }
}

impl PartialOrd for ArchivedItemKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for ArchivedItemKey {
    fn eq(&self, other: &Self) -> bool {
        self.item_id == other.item_id && self.hq == other.hq
    }
}

impl Eq for ArchivedItemKey {}

impl From<&active_listing::Model> for ItemKey {
    fn from(model: &active_listing::Model) -> Self {
        let active_listing::Model { item_id, hq, .. } = *model;
        Self { item_id, hq }
    }
}

impl From<&ActiveListing> for ItemKey {
    fn from(value: &ActiveListing) -> Self {
        let ActiveListing { item_id, hq, .. } = *value;
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

impl From<&ultros_api_types::SaleHistory> for ItemKey {
    fn from(value: &ultros_api_types::SaleHistory) -> Self {
        Self {
            item_id: value.sold_item_id,
            hq: value.hq,
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

#[derive(
    Debug, PartialEq, PartialOrd, Eq, Ord, Clone, Copy, Archive, RkyvDeserialize, RkyvSerialize,
)]
#[archive(check_bytes)]
pub(crate) struct SaleSummary {
    pub(crate) price_per_item: i32,
    pub(crate) sale_date: NaiveDateTime,
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

impl From<&ultros_api_types::SaleHistory> for SaleSummary {
    fn from(value: &ultros_api_types::SaleHistory) -> Self {
        Self {
            price_per_item: value.price_per_item,
            sale_date: value.sold_date,
        }
    }
}

#[derive(Debug, Default, Clone, Archive, RkyvDeserialize, RkyvSerialize)]
#[archive(check_bytes)]
pub(crate) struct SaleHistory {
    pub(crate) item_map: BTreeMap<ItemKey, arrayvec::ArrayVec<SaleSummary, SALE_HISTORY_SIZE>>,
}

impl SaleHistory {
    pub(crate) fn add_sale<'a, T>(&mut self, sale: &'a T)
    where
        &'a T: Into<SaleSummary> + Into<ItemKey>,
    {
        let entries = self.item_map.entry(sale.into()).or_default();
        let sale: SaleSummary = sale.into();
        if entries.len() == SALE_HISTORY_SIZE {
            let last_entry = entries.last().expect("We just checked len");
            if last_entry.sale_date < sale.sale_date {
                let _ = entries.pop();
                entries.push(sale);
            }
        } else {
            entries.push(sale);
        }
        entries.sort_by_key(|sale| Reverse(sale.sale_date));
    }
}

#[derive(Debug, Copy, Clone, Eq, Serialize, Archive, RkyvDeserialize, RkyvSerialize)]
#[archive(check_bytes)]
pub(crate) struct CheapestListingValue {
    pub(crate) price: i32,
    pub(crate) world_id: i32,
}

impl From<&ultros_db::entity::active_listing::Model> for CheapestListingValue {
    fn from(from: &ultros_db::entity::active_listing::Model) -> Self {
        Self {
            price: from.price_per_unit,
            world_id: from.world_id,
        }
    }
}

impl From<&ActiveListing> for CheapestListingValue {
    fn from(
        ActiveListing {
            world_id,
            price_per_unit,
            ..
        }: &ActiveListing,
    ) -> Self {
        Self {
            price: *price_per_unit,
            world_id: *world_id,
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

impl PartialEq for CheapestListingValue {
    fn eq(&self, other: &Self) -> bool {
        self.price == other.price
    }
}

impl PartialOrd for CheapestListingValue {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CheapestListingValue {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.price.cmp(&other.price)
    }
}

#[derive(Debug, Default, Archive, RkyvDeserialize, RkyvSerialize, Clone)]
#[archive(check_bytes)]
pub(crate) struct CheapestListings {
    pub(crate) item_map: BTreeMap<ItemKey, CheapestListingValue>,
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
        *entry = cheapest_listing.min(*entry);
    }

    async fn remove_listing(
        &mut self,
        listing: &ActiveListing,
        id: AnySelector,
        world_cache: &WorldCache,
        ultros_db: &UltrosDb,
    ) {
        // if this was the cheapest listing we need to ask the database for the new cheapest item
        let key = listing.into();
        match self.item_map.entry(key) {
            Entry::Occupied(entry) => {
                // only remove a listing if we see a lower price
                if listing.price_per_unit <= entry.get().price {
                    entry.remove();
                    let worlds = world_cache
                        .lookup_selector(&id)
                        .map(|r| world_cache.get_all_worlds_in(&r))
                        .ok()
                        .flatten()
                        .expect("Should have worlds");
                    if let Ok(listings) = ultros_db
                        .get_multiple_listings_for_worlds_hq_sensitive(
                            worlds.iter().map(|w| WorldId(*w)),
                            [ItemId(listing.item_id)].into_iter(),
                            key.hq,
                            1,
                        )
                        .await
                    {
                        for db_listing in &listings {
                            if key == ItemKey::from(db_listing) {
                                self.add_listing(db_listing);
                            }
                        }
                    }
                }
            }
            Entry::Vacant(_) => {}
        }
    }
}

#[derive(Archive, RkyvDeserialize, RkyvSerialize)]
#[archive(check_bytes)]
struct AnalyzerState {
    recent_sale_history: BTreeMap<i32, SaleHistory>,
    cheapest_items: BTreeMap<AnySelector, CheapestListings>,
}

/// Build a short list of all the items in the game that we think would sell well.
/// Implemented as an easily cloneable Arc monster
#[derive(Debug, Clone)]
pub(crate) struct AnalyzerService {
    /// world_id -> TopSellers
    recent_sale_history: Arc<BTreeMap<i32, RwLock<SaleHistory>>>,
    /// Cheapest items get stored as any anyselector. Currently exists for WorldID/RegionID, but not datacenter.
    cheapest_items: Arc<BTreeMap<AnySelector, RwLock<CheapestListings>>>,
    initiated: Arc<AtomicBool>,
}

impl AnalyzerService {
    /// Creates a task that will feed the analyzer and returns Self so that data can be read externally
    pub async fn start_analyzer(
        ultros_db: UltrosDb,
        event_receivers: EventReceivers,
        world_cache: Arc<WorldCache>,
        token: CancellationToken,
    ) -> Self {
        tokio::fs::create_dir_all("analyzer-data")
            .await
            .expect("Unable to create directory for analyzer");
        let cheapest_items: BTreeMap<AnySelector, RwLock<CheapestListings>> = world_cache
            .get_inner_data()
            .iter()
            .flat_map(|(region, dcs)| {
                [AnySelector::Region(region.id)]
                    .into_iter()
                    .chain(dcs.iter().flat_map(|(dc, worlds)| {
                        [AnySelector::Datacenter(dc.id)]
                            .into_iter()
                            .chain(worlds.iter().map(|w| AnySelector::World(w.id)))
                    }))
            })
            .map(|s| (s, RwLock::default()))
            .collect();
        let cheapest_items = Arc::new(cheapest_items);
        let recent_sale_history = Arc::new(
            world_cache
                .get_inner_data()
                .iter()
                .flat_map(|(_, dcs)| dcs.iter().flat_map(|(_, w)| w.iter().map(|w| w.id)))
                .map(|w| (w, RwLock::default()))
                .collect::<BTreeMap<i32, RwLock<SaleHistory>>>(),
        );
        let temp = Self {
            recent_sale_history,
            cheapest_items,
            initiated: Arc::default(),
        };

        let task_self = temp.clone();
        let serialize_token = token.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(15 * 60));
            loop {
                tokio::select! {
                    _ = serialize_token.cancelled() => {
                        if let Err(e) = task_self.serialize_state(true).await {
                            error!("Error serializing state {e:?}");
                        }
                        break;
                    }
                    _ = interval.tick() => {
                        if let Err(e) = task_self.serialize_state(false).await {
                            error!("Error serializing state {e:?}");
                        }
                    }
                }
            }
        });

        let task_self = temp.clone();
        tokio::spawn(async move {
            task_self
                .run_worker(ultros_db, event_receivers, world_cache, token)
                .await;
        });
        temp
    }

    async fn serialize_state(&self, is_shutdown: bool) -> Result<()> {
        let state = self.get_analyzer_state().await;
        let bytes = rkyv::to_bytes::<_, 256>(&state).map_err(|e| anyhow!(e.to_string()))?;
        let timestamp = Utc::now().timestamp();
        let filename = format!("analyzer-data/snapshot-{}.bin", timestamp);
        fs::write(&filename, &bytes).await?;
        info!("Wrote snapshot to {}", filename);
        if !is_shutdown {
            let mut dir = fs::read_dir("analyzer-data").await?;
            let mut entries = vec![];
            while let Ok(Some(entry)) = dir.next_entry().await {
                entries.push(entry);
            }
            if entries.len() > 4 {
                entries.sort_by_key(|x| x.file_name());
                for entry in entries.iter().take(entries.len() - 4) {
                    fs::remove_file(entry.path()).await?;
                }
            }
        }
        Ok(())
    }

    async fn get_analyzer_state(&self) -> AnalyzerState {
        let mut cheapest_items = BTreeMap::new();
        for (key, value) in self.cheapest_items.iter() {
            let value = value.read().await;
            cheapest_items.insert(*key, value.clone());
        }
        let mut recent_sale_history = BTreeMap::new();
        for (key, value) in self.recent_sale_history.iter() {
            let value = value.read().await;
            recent_sale_history.insert(*key, value.clone());
        }
        AnalyzerState {
            cheapest_items,
            recent_sale_history,
        }
    }

    async fn try_restore_from_snapshot(&self) -> bool {
        let mut dir = match fs::read_dir("analyzer-data").await {
            Ok(dir) => dir,
            Err(_) => return false,
        };
        let mut entries = vec![];
        while let Ok(Some(entry)) = dir.next_entry().await {
            entries.push(entry);
        }
        entries.sort_by_key(|x| x.file_name());
        for entry in entries.iter().rev() {
            let file = match fs::read(entry.path()).await {
                Ok(f) => f,
                Err(e) => {
                    error!("Error reading file {e:?}");
                    continue;
                }
            };
            let state: AnalyzerState = match rkyv::from_bytes(&file) {
                Ok(s) => s,
                Err(e) => {
                    error!("Error deserializing state {e}");
                    continue;
                }
            };
            for (key, value) in state.cheapest_items {
                if let Some(lock) = self.cheapest_items.get(&key) {
                    let mut write = lock.write().await;
                    *write = value;
                }
            }
            for (key, value) in state.recent_sale_history {
                if let Some(lock) = self.recent_sale_history.get(&key) {
                    let mut write = lock.write().await;
                    *write = value;
                }
            }
            return true;
        }
        false
    }

    async fn populate_from_db(&self, ultros_db: &UltrosDb, world_cache: &WorldCache) {
        // on startup we should try to read through the database to get the spiciest of item listings
        info!("worker starting");
        let (listings, sale_data) = futures::future::join(
            ultros_db.cheapest_listings(),
            ultros_db.last_n_sales(SALE_HISTORY_SIZE as i32),
        )
        .await;
        info!("starting item listings");
        match listings {
            Ok(mut listings) => {
                let writer = &self.cheapest_items;
                while let Some(Ok(value)) = listings.next().await {
                    let world = world_cache
                        .lookup_selector(&AnySelector::World(value.world_id))
                        .unwrap();
                    let region = world_cache.get_region(&world).unwrap();
                    let datacenters = world_cache.get_datacenters(&world).unwrap();
                    let region_listings = writer
                        .get(&AnySelector::Region(region.id))
                        .expect("Region not found");
                    region_listings.write().await.add_listing(&value);
                    for dc in datacenters {
                        let dc_listings = writer
                            .get(&AnySelector::Datacenter(dc.id))
                            .expect("Datacenter not found");
                        dc_listings.write().await.add_listing(&value);
                    }
                    let world_listings = writer
                        .get(&AnySelector::World(value.world_id))
                        .expect("Unable to get world");
                    world_listings.write().await.add_listing(&value);
                }
            }
            Err(e) => {
                error!("Streaming item listings failed {e:?}");
            }
        }
        info!("starting sale data");
        match sale_data {
            Ok(mut history_stream) => {
                while let Some(Ok(value)) = history_stream.next().await {
                    let history = self
                        .recent_sale_history
                        .get(&value.world_id)
                        .expect("Unable to get world");
                    history.write().await.add_sale(&value);
                }
            }
            Err(e) => {
                error!("Streaming item listings failed {e:?}");
            }
        }
    }

    pub async fn rescan_from_db(&self, ultros_db: &UltrosDb, world_cache: &WorldCache) {
        info!("Rescan started");
        // clear old data
        for (_, value) in self.cheapest_items.iter() {
            let mut value = value.write().await;
            value.item_map.clear();
        }
        for (_, value) in self.recent_sale_history.iter() {
            let mut value = value.write().await;
            value.item_map.clear();
        }
        self.populate_from_db(ultros_db, world_cache).await;
        info!("Rescan finished");
    }

    async fn run_worker(
        &self,
        ultros_db: UltrosDb,
        mut event_receivers: EventReceivers,
        world_cache: Arc<WorldCache>,
        token: CancellationToken,
    ) {
        if !self.try_restore_from_snapshot().await {
            self.populate_from_db(&ultros_db, &world_cache).await;
        }
        self.initiated.store(true, Ordering::Relaxed);
        info!("worker primed, now using live data");
        let second_worker_instance = self.clone();
        let history_token = token.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = history_token.cancelled() => {
                        break;
                    }
                    history = event_receivers.history.recv() => {
                        if let Ok(history) = history {
                            match history {
                                crate::event::EventType::Remove(_) => {}
                                crate::event::EventType::Add(sales) => {
                                    for (sale, _) in sales.sales.iter() {
                                        second_worker_instance.add_sale(sale).await;
                                    }
                                }
                                crate::event::EventType::Update(_) => {}
                            }
                        }
                    }
                }
            }
        });
        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    break;
                }
                listings = event_receivers.listings.recv() => {
                    if let Ok(listings) = listings {
                        match listings {
                            crate::event::EventType::Remove(remove) => {
                                let region = if let Some(region) = remove
                                    .listings
                                    .iter()
                                    .flat_map(|(w, _)| {
                                        world_cache
                                            .lookup_selector(&AnySelector::World(w.world_id))
                                            .map(|w| world_cache.get_region(&w))
                                    })
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
                                self.add_listings(&add.listings, &world_cache).await;
                            }
                            crate::event::EventType::Update(_) => todo!(),
                        }
                    }
                }
            }
        }
    }

    pub(crate) async fn get_trends(&self, world_id: i32) -> Option<TrendsData> {
        if !self.initiated.load(Ordering::Relaxed) {
            return None;
        }

        let sale_history_map = self.recent_sale_history.get(&world_id)?.read().await;
        let cheapest_listings = self
            .cheapest_items
            .get(&AnySelector::World(world_id))?
            .read()
            .await;

        let mut high_velocity = Vec::new();
        let mut rising_price = Vec::new();
        let mut falling_price = Vec::new();
        let now = Utc::now().naive_utc();

        for (key, sales) in &sale_history_map.item_map {
            // calculate velocity
            // sales per week
            // we have up to 6 sales
            // time range = oldest to newest (or now)
            if sales.is_empty() {
                continue;
            }

            // Filter out sales older than 30 days to keep "trends" relevant
            let recent_sales: Vec<_> = sales
                .iter()
                .filter(|s| (now - s.sale_date).num_days() < 30)
                .collect();

            if recent_sales.len() < 2 {
                continue;
            }

            let newest = recent_sales.first()?;
            let oldest = recent_sales.last()?;
            let days_diff = (newest.sale_date - oldest.sale_date).num_days().max(1) as f32;
            let sales_count = recent_sales.len() as f32;
            let sales_per_week = (sales_count / days_diff) * 7.0;

            let avg_price = recent_sales
                .iter()
                .map(|s| s.price_per_item as f32)
                .sum::<f32>()
                / sales_count;

            if let Some(cheapest) = cheapest_listings.item_map.get(key) {
                let price_diff_ratio = cheapest.price as f32 / avg_price;

                let trend_item = TrendItem {
                    item_id: key.item_id,
                    hq: key.hq,
                    price: cheapest.price,
                    world_id,
                    average_sale_price: avg_price,
                    sales_per_week,
                };

                if sales_per_week > 10.0 {
                    high_velocity.push(trend_item.clone());
                }

                // Rising: Current price is 50% higher than average
                if price_diff_ratio > 1.5 {
                    rising_price.push(trend_item.clone());
                }
                // Falling: Current price is 50% lower than average
                else if price_diff_ratio < 0.5 {
                    falling_price.push(trend_item.clone());
                }
            }
        }

        high_velocity.sort_by(|a, b| {
            b.sales_per_week
                .partial_cmp(&a.sales_per_week)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        rising_price.sort_by(|a, b| {
            (b.price as f32 / b.average_sale_price)
                .partial_cmp(&(a.price as f32 / a.average_sale_price))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        falling_price.sort_by(|a, b| {
            (a.price as f32 / a.average_sale_price)
                .partial_cmp(&(b.price as f32 / b.average_sale_price))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        high_velocity.truncate(50);
        rising_price.truncate(50);
        falling_price.truncate(50);

        Some(TrendsData {
            high_velocity,
            rising_price,
            falling_price,
        })
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
        let datacenter_filters_worlds = resale_options.filter_datacenter.and_then(|w| {
            world_cache
                .lookup_selector(&AnySelector::Datacenter(w))
                .ok()
                .and_then(|w| world_cache.get_all_worlds_in(&w))
        });
        // figure out what items are selling best on our world first, then figure out what items are available in the region that complement that.
        let sale = self.recent_sale_history.get(&world_id)?;
        let sale_history: BTreeMap<_, _> = sale
            .read()
            .await
            .item_map
            .iter()
            .map(|(i, values)| (i, values, values.iter().collect::<SoldWithin>()))
            .flat_map(|(item, values, sold_within)| {
                values
                    .iter()
                    .filter(|sale| {
                        resale_options
                            .filter_sale
                            .as_ref()
                            .map(|sale_within| {
                                let sale_within = Duration::from(sale_within);
                                Utc::now()
                                    .naive_utc()
                                    .signed_duration_since(sale.sale_date)
                                    .lt(&sale_within)
                            })
                            .unwrap_or(true)
                    })
                    .map(|sale| sale.price_per_item)
                    .min()
                    .map(|price| (*item, (price, sold_within)))
            })
            .collect();

        let region = self
            .cheapest_items
            .get(&AnySelector::Region(region_id))?
            .read()
            .await;
        let sale_world_listings = self
            .cheapest_items
            .get(&AnySelector::World(world_id))?
            .read()
            .await;
        let possible_sales: Vec<_> = region
            .item_map
            .iter()
            .flat_map(|(item_key, cheapest_price)| {
                let (cheapest_history, sold_within) = *sale_history.get(item_key)?;
                let current_cheapest_on_sale_world = sale_world_listings
                    .item_map
                    .get(item_key)
                    .map(|l| l.price)
                    .unwrap_or(cheapest_history);
                let est_sale_price = (cheapest_history).min(current_cheapest_on_sale_world);
                let profit = est_sale_price - cheapest_price.price;
                Some(ResaleStats {
                    profit,
                    item_id: item_key.item_id,
                    return_on_investment: ((est_sale_price as f32) / (cheapest_price.price as f32)
                        * 100.0)
                        - 100.0,
                    world_id: cheapest_price.world_id,
                    sold_within,
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
            .filter(|sale| {
                resale_options
                    .filter_sale
                    .as_ref()
                    .and_then(|sold| {
                        sold.partial_cmp(&sale.sold_within)
                            .map(|c| c.is_gt() || c.is_eq())
                    })
                    .unwrap_or(true)
            })
            .collect();

        Some(possible_sales)
    }

    /// process listings in bulk.
    async fn add_listings(
        &self,
        listings: &[(ActiveListing, Retainer)],
        world_cache: &Arc<WorldCache>,
    ) {
        // process all listings from one world at a time
        let listings = listings
            .iter()
            .into_grouping_map_by(|l| l.0.world_id)
            .min_by_key(|_key, val| val.0.price_per_unit);
        let listings = listings.into_iter().flat_map(|(_, (l, _))| {
            let result = world_cache
                .lookup_selector(&AnySelector::World(l.world_id))
                .ok()?;
            Some((
                AnySelector::World(l.world_id),
                AnySelector::Region(world_cache.get_region(&result)?.id),
                world_cache
                    .get_datacenters(&result)
                    .unwrap_or_default()
                    .first()
                    .map(|d| AnySelector::Datacenter(d.id)),
                l,
            ))
        });
        for (world_selector, region_selector, dc_selector, listing) in listings {
            let entry = self
                .cheapest_items
                .get(&region_selector)
                .expect("Unable to get region");
            entry.write().await.add_listing(listing);
            if let Some(dc_selector) = dc_selector {
                #[allow(clippy::collapsible_if)]
                if let Some(entry) = self.cheapest_items.get(&dc_selector) {
                    entry.write().await.add_listing(listing);
                }
            }
            let entry = self
                .cheapest_items
                .get(&world_selector)
                .expect("Unable to get world");
            entry.write().await.add_listing(listing);
        }
    }

    /// remove listings in bulk. can handle multiple item types, but must have only one region.
    async fn remove_listings(
        &self,
        region_id: i32,
        listings: Arc<ListingEventData>,
        world_cache: &WorldCache,
        ultros_db: &UltrosDb,
    ) {
        let entry = self
            .cheapest_items
            .get(&AnySelector::Region(region_id))
            .expect("Unable to get region");
        let mut entry = entry.write().await;
        for (listing, _) in listings.listings.iter() {
            entry
                .remove_listing(
                    listing,
                    AnySelector::Region(region_id),
                    world_cache,
                    ultros_db,
                )
                .await;
        }
        drop(entry);
        for (listing, _) in listings.listings.iter() {
            let world_result = world_cache.lookup_selector(&AnySelector::World(listing.world_id));
            if let Ok(w) = world_result {
                #[allow(clippy::collapsible_if)]
                if let Some(dcs) = world_cache.get_datacenters(&w) {
                    for dc in dcs {
                        if let Some(entry) =
                            self.cheapest_items.get(&AnySelector::Datacenter(dc.id))
                        {
                            entry
                                .write()
                                .await
                                .remove_listing(
                                    listing,
                                    AnySelector::Datacenter(dc.id),
                                    world_cache,
                                    ultros_db,
                                )
                                .await;
                        }
                    }
                }
            }
            let world = self
                .cheapest_items
                .get(&AnySelector::World(listing.world_id))
                .expect("Unable to find world");
            world
                .write()
                .await
                .remove_listing(
                    listing,
                    AnySelector::World(listing.world_id),
                    world_cache,
                    ultros_db,
                )
                .await;
        }
    }

    async fn add_sale(&self, sale: &ultros_api_types::SaleHistory) {
        let entry = self
            .recent_sale_history
            .get(&sale.world_id)
            .expect("Unknown world");
        entry.write().await.add_sale(sale);
    }

    pub(crate) async fn read_cheapest_items<T, O>(
        &self,
        selector: &AnySelector,
        extract: T,
    ) -> Result<O, AnalyzerError>
    where
        T: FnOnce(&CheapestListings) -> O,
    {
        if self.initiated.load(Ordering::Relaxed) {
            let read = self
                .cheapest_items
                .get(selector)
                .ok_or(AnalyzerError::NotFound)?
                .read()
                .await;
            Ok(extract(&read))
        } else {
            Err(AnalyzerError::Uninitialized)
        }
    }

    pub(crate) async fn read_sale_history<T, O>(
        &self,
        selector: &AnySelector,
        extract: T,
    ) -> Result<O, AnalyzerError>
    where
        T: FnOnce(&SaleHistory) -> O,
    {
        if self.initiated.load(Ordering::Relaxed) {
            let read = self
                .recent_sale_history
                .get(&match selector {
                    AnySelector::World(world) => *world,
                    _ => return Err(AnalyzerError::NotFound),
                })
                .ok_or(AnalyzerError::NotFound)?
                .read()
                .await;
            Ok(extract(&read))
        } else {
            Err(AnalyzerError::Uninitialized)
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SoldAmount(pub(crate) u8);

impl Display for SoldAmount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0 >= SALE_HISTORY_SIZE as u8 {
            write!(f, "{}+", SALE_HISTORY_SIZE)
        } else {
            write!(f, "{}", self.0)
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub(crate) enum SoldWithin {
    NoSales,
    Today(SoldAmount),
    Week(SoldAmount),
    Month(SoldAmount),
    Year(SoldAmount),
    YearsAgo(u8, SoldAmount),
}

impl PartialOrd for SoldWithin {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (SoldWithin::NoSales, SoldWithin::NoSales) => Some(std::cmp::Ordering::Equal),
            (SoldWithin::NoSales, _) => None,
            (_, SoldWithin::NoSales) => None,
            (SoldWithin::Today(a), SoldWithin::Today(b)) => Some(b.cmp(a)),
            (SoldWithin::Today(_), _) => Some(std::cmp::Ordering::Less),
            (SoldWithin::Week(_), SoldWithin::Today(_)) => Some(std::cmp::Ordering::Greater),
            (SoldWithin::Week(a), SoldWithin::Week(b)) => Some(b.cmp(a)),
            (SoldWithin::Week(_), _) => Some(std::cmp::Ordering::Less),
            (SoldWithin::Month(_), SoldWithin::Today(_) | SoldWithin::Week(_)) => {
                Some(std::cmp::Ordering::Greater)
            }
            (SoldWithin::Month(a), SoldWithin::Month(b)) => Some(b.cmp(a)),
            (SoldWithin::Month(_), SoldWithin::Year(_) | SoldWithin::YearsAgo(_, _)) => {
                Some(std::cmp::Ordering::Less)
            }
            (
                SoldWithin::Year(_),
                SoldWithin::Today(_) | SoldWithin::Week(_) | SoldWithin::Month(_),
            ) => Some(std::cmp::Ordering::Greater),
            (SoldWithin::Year(a), SoldWithin::Year(b)) => Some(b.cmp(a)),
            (SoldWithin::Year(_), SoldWithin::YearsAgo(_, _)) => Some(std::cmp::Ordering::Less),
            (SoldWithin::YearsAgo(a, aa), SoldWithin::YearsAgo(b, bb)) => {
                Some(a.cmp(b).then_with(|| aa.cmp(bb)))
            }
            (SoldWithin::YearsAgo(_, _), _) => Some(std::cmp::Ordering::Greater),
        }
    }
}

impl Display for SoldWithin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SoldWithin::NoSales => write!(f, "No sales"),
            SoldWithin::Today(d) => write!(f, "{d} sold today"),
            SoldWithin::Week(w) => write!(f, "{w} sold this week"),
            SoldWithin::Month(m) => write!(f, "{m} sold this month"),
            SoldWithin::Year(y) => write!(f, "{y} sold this year"),
            SoldWithin::YearsAgo(i, y) => write!(f, "{y} sold {i} years ago"),
        }
    }
}

impl From<&SoldWithin> for Duration {
    fn from(sold: &SoldWithin) -> Self {
        match sold {
            SoldWithin::NoSales => Duration::days(0),
            SoldWithin::Today(_) => Duration::days(1),
            SoldWithin::Week(_) => Duration::weeks(1),
            SoldWithin::Month(_) => Duration::weeks(4),
            SoldWithin::Year(_) => Duration::weeks(52),
            SoldWithin::YearsAgo(year, _) => Duration::weeks((*year as i64) * 52),
        }
    }
}

impl SoldWithin {
    fn calculate<'a>(iter: impl IntoIterator<Item = &'a SaleSummary>, now: NaiveDateTime) -> Self {
        let mut iter = iter.into_iter().peekable();
        let first_sale = match iter.peek() {
            Some(s) => s,
            None => return SoldWithin::NoSales,
        };
        let duration_since = now.signed_duration_since(first_sale.sale_date);
        enum SaleMarker {
            Today,
            Week,
            Month,
            Year,
            YearsAgo(i64),
        }
        let (marker, end_date) = if duration_since.num_days() < 1 {
            (SaleMarker::Today, now.checked_sub_signed(Duration::days(1)))
        } else if duration_since.num_weeks() < 1 {
            (SaleMarker::Week, now.checked_sub_signed(Duration::weeks(1)))
        } else if duration_since.num_weeks() < 4 {
            (
                SaleMarker::Month,
                now.checked_sub_signed(Duration::weeks(4)),
            )
        } else if duration_since.num_weeks() < 52 {
            (
                SaleMarker::Year,
                now.checked_sub_signed(Duration::weeks(52)),
            )
        } else {
            let years = duration_since.num_weeks() / 52;
            (
                SaleMarker::YearsAgo(years),
                now.checked_sub_signed(Duration::weeks((years + 1) * 52)),
            )
        };
        let end_date = match end_date {
            Some(d) => d,
            None => return SoldWithin::NoSales,
        };
        let sold_amount = iter.filter(|sale| sale.sale_date.gt(&end_date)).count() as u8;
        let sold_amount = SoldAmount(sold_amount);
        match marker {
            SaleMarker::Today => SoldWithin::Today(sold_amount),
            SaleMarker::Week => SoldWithin::Week(sold_amount),
            SaleMarker::Month => SoldWithin::Month(sold_amount),
            SaleMarker::Year => SoldWithin::Year(sold_amount),
            SaleMarker::YearsAgo(year) => SoldWithin::YearsAgo(year as u8, sold_amount),
        }
    }
}

impl<'a> FromIterator<&'a SaleSummary> for SoldWithin {
    fn from_iter<T: IntoIterator<Item = &'a SaleSummary>>(iter: T) -> Self {
        SoldWithin::calculate(iter, Timestamp::now().naive_utc())
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ResaleStats {
    pub(crate) profit: i32,
    pub(crate) item_id: i32,
    pub(crate) sold_within: SoldWithin,
    pub(crate) return_on_investment: f32,
    pub(crate) world_id: i32,
}

#[derive(Default)]
pub(crate) struct ResaleOptions {
    pub(crate) minimum_profit: Option<i32>,
    pub(crate) filter_world: Option<i32>,
    pub(crate) filter_datacenter: Option<i32>,
    pub(crate) filter_sale: Option<SoldWithin>,
}

#[cfg(test)]
mod test {
    use chrono::{Duration, Utc};
    use ultros_db::sales::AbbreviatedSaleData;

    use crate::analyzer_service::ItemKey;

    use super::{SaleHistory, SaleSummary, SoldAmount, SoldWithin};

    #[test]
    fn test_sale_history_sort() {
        let mut sale_history = SaleHistory::default();
        for i in 0..10 {
            sale_history.add_sale(&AbbreviatedSaleData {
                sold_item_id: 101,
                hq: true,
                price_per_item: i,
                sold_date: Utc::now()
                    .naive_utc()
                    .checked_add_signed(Duration::seconds(i as i64))
                    .unwrap(),
                world_id: 0,
            });
        }
        let map = sale_history
            .item_map
            .get(&ItemKey {
                item_id: 101,
                hq: true,
            })
            .unwrap();
        assert_eq!(map[0].price_per_item, 9);
        assert_eq!(map[1].price_per_item, 8);
    }

    #[test]
    fn test_sold_within_calculation() {
        let now = Utc::now().naive_utc();

        // Helper to create a SaleSummary
        let make_sale = |offset_duration: Duration| -> SaleSummary {
            SaleSummary {
                price_per_item: 100,
                sale_date: now + offset_duration,
            }
        };

        // Case 1: No sales
        let sales: Vec<SaleSummary> = vec![];
        assert_eq!(
            SoldWithin::calculate(&sales, now),
            SoldWithin::NoSales,
            "Empty sales should result in NoSales"
        );

        // Case 2: Sold Today
        // Sale just happened (0 seconds ago)
        let sales = vec![make_sale(Duration::seconds(0))];
        assert_eq!(
            SoldWithin::calculate(&sales, now),
            SoldWithin::Today(SoldAmount(1)),
            "Sale at now should be SoldWithin::Today"
        );

        // Sale 23 hours ago is still "Today" if we consider < 24h as logic (which num_days() < 1 implies, wait check impl)
        // logic: duration_since.num_days() < 1. duration_since is now - first_sale.
        // if first_sale is 23h ago, duration_since is 23h. num_days() is 0. So it is Today.
        let sales = vec![make_sale(-Duration::hours(23))];
        assert_eq!(
            SoldWithin::calculate(&sales, now),
            SoldWithin::Today(SoldAmount(1)),
            "Sale 23 hours ago should be SoldWithin::Today"
        );

        // Case 3: Sold This Week
        // Sale 25 hours ago. num_days() is 1. num_weeks() is 0. So Week.
        let sales = vec![make_sale(-Duration::hours(25))];
        assert_eq!(
            SoldWithin::calculate(&sales, now),
            SoldWithin::Week(SoldAmount(1)),
            "Sale 25 hours ago should be SoldWithin::Week"
        );

        // Sale 6 days ago. num_days() is 6. num_weeks() is 0. So Week.
        let sales = vec![make_sale(-Duration::days(6))];
        assert_eq!(
            SoldWithin::calculate(&sales, now),
            SoldWithin::Week(SoldAmount(1)),
            "Sale 6 days ago should be SoldWithin::Week"
        );

        // Case 4: Sold This Month
        // Sale 8 days ago. num_weeks() is 1. So Month. (logic: < 4 weeks is Month)
        let sales = vec![make_sale(-Duration::days(8))];
        assert_eq!(
            SoldWithin::calculate(&sales, now),
            SoldWithin::Month(SoldAmount(1)),
            "Sale 8 days ago should be SoldWithin::Month"
        );

        // Sale 3 weeks ago. num_weeks() is 3. So Month.
        let sales = vec![make_sale(-Duration::weeks(3))];
        assert_eq!(
            SoldWithin::calculate(&sales, now),
            SoldWithin::Month(SoldAmount(1)),
            "Sale 3 weeks ago should be SoldWithin::Month"
        );

        // Case 5: Sold This Year
        // Sale 5 weeks ago. num_weeks() is 5. So Year. (logic: < 52 weeks is Year)
        let sales = vec![make_sale(-Duration::weeks(5))];
        assert_eq!(
            SoldWithin::calculate(&sales, now),
            SoldWithin::Year(SoldAmount(1)),
            "Sale 5 weeks ago should be SoldWithin::Year"
        );

        // Case 6: Sold Years Ago
        // Sale 53 weeks ago. num_weeks() is 53. 53/52 = 1. So YearsAgo(1).
        let sales = vec![make_sale(-Duration::weeks(53))];
        assert_eq!(
            SoldWithin::calculate(&sales, now),
            SoldWithin::YearsAgo(1, SoldAmount(1)),
            "Sale 53 weeks ago should be SoldWithin::YearsAgo(1)"
        );

        // Case 7: Multiple sales count
        // 3 sales today
        let sales = vec![
            make_sale(-Duration::hours(1)),
            make_sale(-Duration::hours(2)),
            make_sale(-Duration::hours(3)),
        ];
        // The logic uses the first sale (from peek) to determine the "marker".
        // The list is usually sorted by date desc?
        // Wait, SaleHistory.add_sale sorts by date desc (Reverse).
        // Let's assume input is sorted desc (newest first).
        // But `FromIterator` impl takes an iterator. It peeks the first one.
        // In `SoldWithin::calculate`, `iter` is just an iterator.
        // It peeks to find the *most recent* sale to determine the "bucket" (Today/Week/etc).
        // Then it counts how many sales fit in that bucket.
        //
        // Logic detail:
        // marker determined by `now - first_sale`.
        // end_date determined by marker.
        // sold_amount = iter.filter(|sale| sale.sale_date.gt(&end_date)).count()
        //
        // If sales are sorted desc:
        // 1h ago, 2h ago, 3h ago.
        // first = 1h ago. Marker = Today. end_date = now - 1 day.
        // All 3 are > end_date. Count should be 3.
        assert_eq!(
            SoldWithin::calculate(&sales, now),
            SoldWithin::Today(SoldAmount(3)),
            "3 sales today should be counted correctly"
        );

        // Case 8: Mixed sales
        // 1 sale today, 1 sale yesterday (Week bucket).
        // If sorted desc: first is Today. Marker = Today. end_date = now - 1 day.
        // Today sale > end_date. Yesterday sale (say 25h ago) < end_date.
        // Count should be 1.
        let sales = vec![
            make_sale(-Duration::hours(1)),
            make_sale(-Duration::hours(25)),
        ];
        assert_eq!(
            SoldWithin::calculate(&sales, now),
            SoldWithin::Today(SoldAmount(1)),
            "Should only count sales within the 'Today' window"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::tempdir;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_persistence() {
        let dir = tempdir().unwrap();
        let data_dir = dir.path();
        std::env::set_current_dir(data_dir).unwrap();
        tokio::fs::create_dir_all("analyzer-data").await.unwrap();

        // Part 1: Serialization and Deserialization
        let mut cheapest_items = BTreeMap::new();
        let mut recent_sale_history = BTreeMap::new();

        // Add some data to the service
        let mut sale_history = SaleHistory::default();
        sale_history.add_sale(&ultros_api_types::SaleHistory {
            id: 0,
            hq: true,
            price_per_item: 100,
            quantity: 1,
            buyer_name: Some("Test Buyer".to_string()),
            buying_character_id: 0,
            sold_date: Utc::now().naive_utc(),
            world_id: 1,
            sold_item_id: 1,
        });
        recent_sale_history.insert(1, RwLock::new(sale_history));

        let mut cheapest_listings = CheapestListings::default();
        cheapest_listings.add_listing(&ultros_db::listings::ListingSummary {
            item_id: 1,
            world_id: 1,
            price_per_unit: 100,
            hq: true,
        });
        cheapest_items.insert(AnySelector::World(1), RwLock::new(cheapest_listings));

        let analyzer_service = AnalyzerService {
            recent_sale_history: Arc::new(recent_sale_history),
            cheapest_items: Arc::new(cheapest_items),
            initiated: Arc::new(AtomicBool::new(false)),
        };

        // Serialize the state
        analyzer_service.serialize_state(false).await.unwrap();

        // Create a new service and restore from the snapshot
        let mut new_cheapest_items_map = BTreeMap::new();
        new_cheapest_items_map.insert(
            AnySelector::World(1),
            RwLock::new(CheapestListings::default()),
        );
        let new_cheapest_items = Arc::new(new_cheapest_items_map);

        let mut new_recent_sale_history_map = BTreeMap::new();
        new_recent_sale_history_map.insert(1, RwLock::new(SaleHistory::default()));
        let new_recent_sale_history = Arc::new(new_recent_sale_history_map);

        let new_analyzer_service = AnalyzerService {
            recent_sale_history: new_recent_sale_history.clone(),
            cheapest_items: new_cheapest_items.clone(),
            initiated: Arc::new(AtomicBool::new(false)),
        };
        assert!(new_analyzer_service.try_restore_from_snapshot().await);

        // Check that the data was restored correctly
        let sale_history = new_recent_sale_history.get(&1).unwrap().read().await;
        assert_eq!(sale_history.item_map.len(), 1);
        let cheapest_listings = new_cheapest_items
            .get(&AnySelector::World(1))
            .unwrap()
            .read()
            .await;
        assert_eq!(cheapest_listings.item_map.len(), 1);

        // Check Datacenter support
        let mut dc_cheapest_items = BTreeMap::new();
        dc_cheapest_items.insert(
            AnySelector::Datacenter(1),
            RwLock::new(cheapest_listings.clone()),
        );
        let dc_cheapest_items = Arc::new(dc_cheapest_items);
        let dc_analyzer_service = AnalyzerService {
            recent_sale_history: new_recent_sale_history.clone(),
            cheapest_items: dc_cheapest_items.clone(),
            initiated: Arc::new(AtomicBool::new(false)),
        };
        // Serialize
        dc_analyzer_service.serialize_state(false).await.unwrap();
        // Restore
        let mut restore_dc_cheapest_items = BTreeMap::new();
        restore_dc_cheapest_items.insert(
            AnySelector::Datacenter(1),
            RwLock::new(CheapestListings::default()),
        );
        let restore_dc_cheapest_items = Arc::new(restore_dc_cheapest_items);
        let restore_dc_analyzer_service = AnalyzerService {
            recent_sale_history: new_recent_sale_history.clone(),
            cheapest_items: restore_dc_cheapest_items.clone(),
            initiated: Arc::new(AtomicBool::new(false)),
        };
        assert!(
            restore_dc_analyzer_service
                .try_restore_from_snapshot()
                .await
        );
        let restored_listings = restore_dc_cheapest_items
            .get(&AnySelector::Datacenter(1))
            .unwrap()
            .read()
            .await;
        assert_eq!(restored_listings.item_map.len(), 1);

        // Part 2: Snapshot Rotation
        // Create 5 more snapshots (total 6)
        for _ in 0..5 {
            analyzer_service.serialize_state(false).await.unwrap();
            // Sleep for a second to ensure the timestamps are different
            sleep(Duration::from_secs(1)).await;
        }

        // Check that only 4 snapshots remain
        let mut entries = tokio::fs::read_dir("analyzer-data").await.unwrap();
        let mut count = 0;
        while entries.next_entry().await.unwrap().is_some() {
            count += 1;
        }
        assert_eq!(count, 4);
    }
}
