use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures::{StreamExt, stream};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, instrument, warn};
use ultros_api_types::websocket::{ListingEventData, SaleEventData};
use ultros_db::{
    UltrosDb,
    common::partial_diff_iterator::{DiffItem, PartialDiffIterator},
    entity::{listing_last_updated::Model, world},
    world_data::world_cache::WorldCache,
};
use universalis::{UniversalisClient, WorldId, WorldItemRecencyView};

use crate::event::{EventProducer, EventType};

/// Universalis' most-recently-updated endpoint returns at most this many entries.
const RECENTLY_UPDATED_WINDOW: u8 = 200;
/// Our `listing_last_updated` rows store *ingest* time rather than Universalis'
/// upload time, so an entry only counts as missed when their upload is newer
/// than our ingest by more than this slack.
const UPLOAD_TIME_SLACK_SECONDS: i64 = 120;
/// Minimum time between saturation-triggered full sweeps of a single world.
const FULL_SWEEP_COOLDOWN: Duration = Duration::from_secs(6 * 60 * 60);

/// Item update service attempts to keep ultros' data in sync with Universalis' data.
/// It does this primarily by comparing the recently updated items on Universalis with recently updated items on ultros
pub(crate) struct UpdateService {
    pub(crate) db: UltrosDb,
    pub(crate) world_cache: Arc<WorldCache>,
    pub(crate) universalis: UniversalisClient,
    pub(crate) listings: EventProducer<ListingEventData>,
    pub(crate) sales: EventProducer<SaleEventData>,
    /// Per-world timestamp of the last saturation-triggered full sweep.
    pub(crate) full_sweep_cooldowns: Mutex<HashMap<i32, Instant>>,
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
    pub(crate) fn start_service(service: Arc<Self>, token: CancellationToken) {
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = token.cancelled() => {
                        break;
                    }
                    _ = async {
                // check all worlds
                info!("Checking all worlds");
                // Create this 5 minute duration check now so that our refresh interval includes the time we spent checking
                let next_interval = Instant::now() + tokio::time::Duration::from_secs(60 * 5);
                for world in service.world_cache.get_all_worlds() {
                    info!("{world:?}");
                    let world = service.check_for_missed_items_on_world(world).await;
                    if let Err(w) = world {
                        error!(error = ?w, "check_for_missed_items_on_world failed");
                    }
                }
                tokio::time::sleep_until(next_interval).await;
                    } => {}
                }
            }
        });
    }

    fn all_marketable_items() -> Box<[i32]> {
        xiv_gen_db::data()
            .items
            .values()
            .filter(|i| i.item_search_category != 0)
            .map(|i| i.key_id.0)
            .collect()
    }

    /// Sweeps over every single marketable item in the game, ignoring the recency cache. Only should be used if data is known to be lost.
    pub(crate) async fn do_full_world_sweep(&self) -> Result<(), anyhow::Error> {
        let all_marketable_items = Self::all_marketable_items();
        for world in self.world_cache.get_all_worlds() {
            tracing::info!("scanning items");
            self.check_items(world, &all_marketable_items).await?;
        }
        Ok(())
    }

    /// Claims the full-sweep slot for a world if its cooldown has elapsed.
    fn claim_full_sweep_slot(&self, world_id: i32) -> bool {
        let mut cooldowns = self
            .full_sweep_cooldowns
            .lock()
            .expect("full_sweep_cooldowns poisoned");
        let now = Instant::now();
        match cooldowns.get(&world_id) {
            Some(last) if now.duration_since(*last) < FULL_SWEEP_COOLDOWN => false,
            _ => {
                cooldowns.insert(world_id, now);
                true
            }
        }
    }

    #[instrument(level = "trace", skip(self))]
    async fn check_for_missed_items_on_world(
        &self,
        world: &world::Model,
    ) -> Result<(), anyhow::Error> {
        let updates = self.get_missing_updates(world).await?;
        let item_ids: Box<[i32]> = updates.into_iter().map(|i| i.item_id).collect();
        metrics::counter!("ultros_catchup_items_recovered", "world" => world.name.clone())
            .increment(item_ids.len() as u64);
        self.check_items(world, &item_ids).await?;
        if item_ids.len() >= usize::from(RECENTLY_UPDATED_WINDOW) {
            // Every entry in the recency window was one we missed, so more
            // updates have likely scrolled past where this endpoint can see.
            // The only way to recover those is a full sweep of the world.
            metrics::counter!("ultros_catchup_window_saturated", "world" => world.name.clone())
                .increment(1);
            if self.claim_full_sweep_slot(world.id) {
                warn!(world = %world.name, "recency window saturated, running full item sweep");
                self.check_items(world, &Self::all_marketable_items())
                    .await?;
            } else {
                warn!(world = %world.name, "recency window saturated, full sweep on cooldown");
            }
        }
        Ok(())
    }

    async fn get_missing_updates(
        &self,
        world: &world::Model,
    ) -> Result<Vec<WorldItemRecencyView>, anyhow::Error> {
        let recently_updated = self
            .universalis
            .recently_updated_items(
                universalis::WorldOrDatacenter::World(&world.name),
                RECENTLY_UPDATED_WINDOW,
            )
            .await?;
        let our_recently_updated = self
            .db
            .get_recently_updated_listings_for_world(
                world.id,
                recently_updated.items.len() as u64 * 2,
            )
            .await?;
        Ok(missed_updates(our_recently_updated, recently_updated.items))
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
                        match self.db.update_listings(listings, item_id, world_id).await {
                            Ok((added, removed)) => {
                                let _ =
                                    self.listings
                                        .send(EventType::Add(Arc::new(ListingEventData {
                                            item_id: item_id.0,
                                            world_id: world_id.0,
                                            listings: added,
                                        })));
                                let _ = self.listings.send(EventType::Remove(Arc::new(
                                    ListingEventData {
                                        item_id: item_id.0,
                                        world_id: world_id.0,
                                        listings: removed,
                                    },
                                )));
                            }
                            Err(e) => {
                                error!(error = ?e, item_id = item_id.0, world_id = world_id.0, "catch-up listing update failed");
                                // Storing the sales would bump `listing_last_updated`,
                                // the very marker this service diffs against Universalis
                                // to decide what still needs recovering. Bumping it after
                                // a failed listing write would make the item look freshly
                                // ingested and hide the gap from every later pass, so
                                // leave it untouched and retry on the next cycle.
                                return;
                            }
                        }
                        match self.db.update_sales(sales, item_id, world_id).await {
                            Ok(added) => {
                                let _ = self
                                    .sales
                                    .send(EventType::added(SaleEventData { sales: added }));
                            }
                            Err(e) => {
                                error!(error = ?e, item_id = item_id.0, world_id = world_id.0, "catch-up sale update failed")
                            }
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

/// Items Universalis has seen updates for that we appear to have missed:
/// either absent from our recent-update list entirely, or present but with a
/// Universalis upload newer than our last ingest (allowing
/// [`UPLOAD_TIME_SLACK_SECONDS`] of slack since we store ingest time, not
/// upload time).
fn missed_updates(
    ours: Vec<Model>,
    mut theirs: Vec<WorldItemRecencyView>,
) -> Vec<WorldItemRecencyView> {
    let mut ours: Vec<CmpListing> = ours.into_iter().map(CmpListing).collect();
    ours.sort_by_key(|i| i.0.item_id);
    theirs.sort_by_key(|i| i.item_id);
    PartialDiffIterator::new(ours.into_iter(), theirs.into_iter())
        .filter_map(|entry| match entry {
            DiffItem::Right(theirs) => Some(theirs),
            DiffItem::Same(ours, theirs) => (theirs.last_upload_time.timestamp()
                > ours.0.date_time.and_utc().timestamp() + UPLOAD_TIME_SLACK_SECONDS)
                .then_some(theirs),
            DiffItem::Left(_) => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Local};

    const WORLD_ID: i32 = 34;

    fn ours(item_id: i32, ingested_at: i64) -> Model {
        Model {
            item_id,
            world_id: WORLD_ID,
            date_time: DateTime::from_timestamp(ingested_at, 0)
                .unwrap()
                .naive_utc(),
        }
    }

    fn theirs(item_id: i32, uploaded_at: i64) -> WorldItemRecencyView {
        WorldItemRecencyView {
            item_id,
            last_upload_time: DateTime::from_timestamp(uploaded_at, 0)
                .unwrap()
                .with_timezone(&Local),
            world_id: WORLD_ID,
            world_name: None,
        }
    }

    #[test]
    fn item_unknown_to_us_is_missed() {
        let missed = missed_updates(vec![ours(1, 1_000)], vec![theirs(2, 1_000)]);
        assert_eq!(missed.iter().map(|i| i.item_id).collect::<Vec<_>>(), [2]);
    }

    #[test]
    fn item_we_ingested_after_their_upload_is_in_sync() {
        let missed = missed_updates(vec![ours(1, 1_010)], vec![theirs(1, 1_000)]);
        assert!(missed.is_empty());
    }

    #[test]
    fn item_with_newer_upload_than_our_ingest_is_missed() {
        let missed = missed_updates(
            vec![ours(1, 1_000)],
            vec![theirs(1, 1_000 + UPLOAD_TIME_SLACK_SECONDS + 1)],
        );
        assert_eq!(missed.iter().map(|i| i.item_id).collect::<Vec<_>>(), [1]);
    }

    #[test]
    fn upload_newer_within_slack_is_in_sync() {
        let missed = missed_updates(
            vec![ours(1, 1_000)],
            vec![theirs(1, 1_000 + UPLOAD_TIME_SLACK_SECONDS)],
        );
        assert!(missed.is_empty());
    }

    /// The `listing_last_updated` marker is the *only* signal that an item still
    /// needs recovering, and it is compared against Universalis' upload time. If a
    /// failed write stamps it anyway, the item reads as in-sync from then on and no
    /// later pass retries it. That is why `update_sales` waits until the sales have
    /// landed before writing the marker, and why the catch-up loop skips the sales
    /// update entirely when the listing update failed.
    #[test]
    fn marker_bumped_by_a_failed_write_permanently_hides_the_gap() {
        let their_upload = 1_000;
        // A catch-up attempt runs well after their upload, fails to write, but
        // still stamps `listing_last_updated` with "now".
        let failed_attempt_at = their_upload + UPLOAD_TIME_SLACK_SECONDS + 500;
        let missed = missed_updates(
            vec![ours(1, failed_attempt_at)],
            vec![theirs(1, their_upload)],
        );
        assert!(
            missed.is_empty(),
            "a prematurely bumped marker makes the still-missing item invisible to catch-up"
        );
    }

    #[test]
    fn item_only_on_our_side_is_ignored() {
        let missed = missed_updates(vec![ours(1, 1_000)], vec![]);
        assert!(missed.is_empty());
    }

    #[test]
    fn unsorted_inputs_still_diff_correctly() {
        let missed = missed_updates(
            vec![ours(5, 1_000), ours(1, 1_000), ours(3, 1_000)],
            vec![theirs(3, 500), theirs(7, 1_000), theirs(1, 9_999)],
        );
        assert_eq!(missed.iter().map(|i| i.item_id).collect::<Vec<_>>(), [1, 7]);
    }
}
