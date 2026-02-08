use anyhow::{Result, anyhow};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use std::{
    collections::{BTreeMap, btree_map::Entry},
    io::{Read, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use chrono::{Duration, Utc};
use futures::StreamExt;
use itertools::Itertools;
use tokio::fs;
use tracing::log::{error, info};
use ultros_api_types::{ActiveListing, Retainer, websocket::ListingEventData};
use ultros_db::{
    UltrosDb,
    world_cache::{AnySelector, WorldCache},
};
use universalis::{ItemId, WorldId};

use crate::event::EventReceivers;
use thiserror::Error;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use ultros_api_types::trends::{TrendItem, TrendsData};

pub mod types;
pub(crate) use types::*;

#[derive(Debug, Error)]
pub enum AnalyzerError {
    #[error("Still warming up with data, unable to serve requests.")]
    Uninitialized,
    #[error("Data not found")]
    NotFound,
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
        if !self.initiated.load(Ordering::Relaxed) {
            info!("Analyzer not initialized, skipping serialization");
            return Ok(());
        }
        let state = self.get_analyzer_state().await;
        let bytes = rkyv::to_bytes::<_, 256>(&state).map_err(|e| anyhow!(e.to_string()))?;

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&bytes)?;
        let compressed_bytes = encoder.finish()?;

        let timestamp = Utc::now().timestamp();
        let filename = format!("analyzer-data/snapshot-{}.bin.gz", timestamp);
        fs::write(&filename, &compressed_bytes).await?;
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
            let path = entry.path();
            let file = match fs::read(&path).await {
                Ok(f) => f,
                Err(e) => {
                    error!("Error reading file {e:?}");
                    continue;
                }
            };

            let decompressed_data = if path.to_string_lossy().ends_with(".gz") {
                let mut decoder = GzDecoder::new(&file[..]);
                let mut s = Vec::new();
                if let Err(e) = decoder.read_to_end(&mut s) {
                    error!("Error decompressing file {path:?}: {e}");
                    continue;
                }
                s
            } else {
                file
            };

            let state: AnalyzerState = match rkyv::from_bytes(&decompressed_data) {
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

    async fn run_worker(
        &self,
        ultros_db: UltrosDb,
        mut event_receivers: EventReceivers,
        world_cache: Arc<WorldCache>,
        token: CancellationToken,
    ) {
        if !self.try_restore_from_snapshot().await {
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

            // Calculate Standard Deviation
            let variance = recent_sales
                .iter()
                .map(|s| {
                    let diff = s.price_per_item as f32 - avg_price;
                    diff * diff
                })
                .sum::<f32>()
                / sales_count;
            let std_dev = variance.sqrt();

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

                // Rising: Current price is significantly higher than average (using SD or fixed ratio)
                // Use 1.5 ratio OR > 2 SDs if SD is significant relative to price
                // If SD is small, 1.5 ratio is safer. If SD is huge, 1.5 ratio might be within noise.
                // Let's keep 1.5 ratio as a base, but maybe refine it?
                // Actually, let's use SD if available and meaningful.
                // If price > avg + 1.0 * SD (and ratio > 1.2), it's rising.
                // But to keep it simple and consistent with "Rising Prices" meaning "Buy Low Sell High later" or "Market Spiking":
                // If we want to find "Rising" markets to maybe invest in? No, usually "Rising" means "Don't buy now".
                // Or does it mean "It's trending up, buy now before it goes higher"?
                // Let's stick to the previous logic but maybe make it a bit more statistically sound if possible.
                // For now, I'll stick to the requested improvement: Use Standard Deviation.

                // If price is > 1 standard deviation above average, and at least 20% higher.
                if (cheapest.price as f32 > avg_price + std_dev) && price_diff_ratio > 1.2 {
                    rising_price.push(trend_item.clone());
                }
                // Falling: Price < 1 SD below average, and at least 20% lower.
                else if (cheapest.price as f32) < (avg_price - std_dev) && price_diff_ratio < 0.8
                {
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
                let mut prices: smallvec::SmallVec<[i32; SALE_HISTORY_SIZE]> = values
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
                    .collect();
                if prices.is_empty() {
                    return None;
                }
                prices.sort_unstable();
                let len = prices.len();
                // Get median. If even, pick the lower one to be conservative?
                // Actually, let's pick the one at len / 2.
                // 1 item: idx 0. 2 items: idx 1. 3 items: idx 1. 4 items: idx 2.
                // This essentially picks the slightly higher one in even cases, or middle in odd.
                // Let's pick len / 2.
                let price = prices[len / 2];
                Some((*item, (price, sold_within)))
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

        // Verify .bin.gz does not exist because we aren't initiated
        let mut entries = tokio::fs::read_dir("analyzer-data").await.unwrap();
        let mut found = false;
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.file_name().to_string_lossy().ends_with(".bin.gz") {
                found = true;
            }
        }
        assert!(!found, "Should not have created a .bin.gz file");

        analyzer_service.initiated.store(true, Ordering::Relaxed);
        analyzer_service.serialize_state(false).await.unwrap();

        // Verify .bin.gz exists
        let mut entries = tokio::fs::read_dir("analyzer-data").await.unwrap();
        let mut found = false;
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.file_name().to_string_lossy().ends_with(".bin.gz") {
                found = true;
            }
        }
        assert!(found, "Should have created a .bin.gz file");

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
            initiated: Arc::new(AtomicBool::new(true)),
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
