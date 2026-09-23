//! Below-median and back-in-stock alerts.
//!
//! Both watch the listing stream for one item in one world scope:
//!
//! - **Below median** fires when an added listing is at least N% under the
//!   item's 30-day median for its own quality (see
//!   [`ultros_api_types::alert::below_median_matches`]). Baselines come from
//!   the [`MedianCache`], which this module keeps refreshed.
//! - **Back in stock** fires when a scope that had no listings for at least
//!   [`BACK_IN_STOCK_MIN_EMPTY_SECS`] gets one. Listing events only prompt a
//!   recount — the Postgres `active_listing` count decides — because
//!   Universalis events can be dropped, duplicated, or processed out of order
//!   (each websocket message is persisted in its own task). The state machine
//!   is [`StockState`].
//!
//! Cooldown is enforced like the price tracker: `last_fired_at` is bumped in
//! memory under the lock before dispatch, and in the DB after delivery.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use anyhow::Result;
use chrono::{DateTime, Utc};
use poise::serenity_prelude;
use tokio::sync::Mutex;
use tracing::{error, info, instrument, warn};
use ultros_api_types::{
    ActiveListing,
    alert::{
        BACK_IN_STOCK_MIN_EMPTY_SECS, MedianRule, QualityBaselines, below_median_matches,
        is_off_cooldown_at,
    },
    item_stats::ItemStatsVariant,
    websocket::ListingEventData,
    world_helper::{AnySelector, WorldHelper},
};
use ultros_db::{
    UltrosDb,
    entity::{alert, alert_back_in_stock, alert_below_median},
};

use crate::alerts::delivery::{AlertKind, PushOptions, dispatch_alert};
use crate::alerts::inbox::{AlertFire, record_fire};
use crate::alerts::median_cache::{BASELINE_REFRESH_SECS, BaselineKey, MedianCache};
use crate::alerts::price_alert_tracker::{item_click_url, resolve_item_name};
use crate::event::{EventBus, EventProducer, EventType, NotificationEvent};

/// How often every back-in-stock rule is recounted from Postgres, to recover
/// from listing events Universalis dropped.
const STOCK_RECONCILE_SECS: u64 = 60 * 60;

/// Back-in-stock state for one rule. Pure: every transition takes `now`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StockState {
    /// When the scope was first seen empty. `Some` = armed.
    pub empty_since: Option<DateTime<Utc>>,
    /// Last time a listing was added in scope. A recount issued before this
    /// is stale — it may have read the board before that listing landed.
    pub last_listing_seen: Option<DateTime<Utc>>,
    /// Issue time of the newest recount applied. Recounts run concurrently
    /// (one per removal), so their results can land out of order; an older
    /// one must not overwrite what a newer one established.
    pub last_count_at: Option<DateTime<Utc>>,
}

impl StockState {
    /// Apply a recount of the scope that was issued at `issued_at`. Zero arms
    /// an unarmed rule (keeping an existing arm's original start); non-zero
    /// disarms. A zero from a recount issued before the last in-scope add is
    /// ignored — it raced that add and no longer describes the board.
    pub fn observe_count(&mut self, count: i64, issued_at: DateTime<Utc>) {
        if self.last_count_at.is_some_and(|newest| issued_at < newest) {
            return;
        }
        self.last_count_at = Some(issued_at);
        if count > 0 {
            self.empty_since = None;
            return;
        }
        if self.last_listing_seen.is_some_and(|seen| seen >= issued_at) {
            return;
        }
        self.empty_since.get_or_insert(issued_at);
    }

    /// A listing was added in scope at `now`. Returns true when this is a
    /// restock worth reporting: armed for at least the minimum empty time and
    /// off cooldown. Any add disarms — an arm younger than the minimum was a
    /// reprice or a quick relist, and one on cooldown is simply swallowed.
    pub fn on_listing_added(
        &mut self,
        now: DateTime<Utc>,
        last_fired_at: Option<DateTime<Utc>>,
        cooldown_seconds: i32,
    ) -> bool {
        self.last_listing_seen = Some(now);
        let Some(empty_since) = self.empty_since.take() else {
            return false;
        };
        now.signed_duration_since(empty_since).num_seconds() >= BACK_IN_STOCK_MIN_EMPTY_SECS
            && is_off_cooldown_at(last_fired_at, cooldown_seconds, now)
    }
}

#[derive(Clone, Debug)]
struct MedianActiveRule {
    alert_id: i32,
    owner: i64,
    rule: MedianRule,
}

#[derive(Clone, Debug)]
struct StockRule {
    alert_id: i32,
    owner: i64,
    item_id: i32,
    world_ids: HashSet<i32>,
    hq_only: bool,
    cooldown_seconds: i32,
    last_fired_at: Option<DateTime<Utc>>,
    state: StockState,
}

impl StockRule {
    fn counts_listing(&self, listing: &ActiveListing) -> bool {
        self.world_ids.contains(&listing.world_id) && (!self.hq_only || listing.hq)
    }

    /// Sum the grouped `(item, world, hq, count)` rows over this rule's scope.
    fn count_in(&self, counts: &[(i32, i32, bool, i64)]) -> i64 {
        counts
            .iter()
            .filter(|(item, world, hq, _)| {
                *item == self.item_id && self.world_ids.contains(world) && (!self.hq_only || *hq)
            })
            .map(|(_, _, _, n)| n)
            .sum()
    }
}

#[derive(Debug, Default)]
struct TrackerState {
    median_by_item: HashMap<i32, Vec<MedianActiveRule>>,
    stock_by_item: HashMap<i32, Vec<StockRule>>,
}

impl TrackerState {
    fn baseline_keys(&self) -> HashSet<BaselineKey> {
        self.median_by_item
            .values()
            .flatten()
            .map(|r| (r.rule.item_id, r.rule.world_selector))
            .collect()
    }

    fn stock_item_ids(&self) -> Vec<i32> {
        self.stock_by_item.keys().copied().collect()
    }

    /// Rebuild both indexes from the DB rows, carrying over the in-memory
    /// state of alerts that already existed: their back-in-stock arm, and the
    /// later of the two `last_fired_at`s (the DB copy only moves on a
    /// successful delivery).
    fn rebuild(
        &mut self,
        median: &[(alert::Model, alert_below_median::Model)],
        stock: &[(alert::Model, alert_back_in_stock::Model)],
        worlds: &WorldHelper,
    ) {
        let old_median: HashMap<i32, Option<DateTime<Utc>>> = self
            .median_by_item
            .values()
            .flatten()
            .map(|r| (r.alert_id, r.rule.last_fired_at))
            .collect();
        let old_stock: HashMap<i32, (Option<DateTime<Utc>>, StockState)> = self
            .stock_by_item
            .values()
            .flatten()
            .map(|r| (r.alert_id, (r.last_fired_at, r.state.clone())))
            .collect();

        self.median_by_item.clear();
        for (a, m) in median {
            if !a.enabled {
                continue;
            }
            let Some(world_selector) = parse_selector(a.id, &m.world_selector, worlds) else {
                continue;
            };
            let db_fired = a.last_fired_at.map(|t| t.with_timezone(&Utc));
            let last_fired_at = db_fired.max(old_median.get(&a.id).copied().flatten());
            self.median_by_item
                .entry(m.item_id)
                .or_default()
                .push(MedianActiveRule {
                    alert_id: a.id,
                    owner: a.owner,
                    rule: MedianRule {
                        item_id: m.item_id,
                        world_selector,
                        percent_below: m.percent_below,
                        hq_only: m.hq_only,
                        cooldown_seconds: a.cooldown_seconds,
                        last_fired_at,
                    },
                });
        }

        self.stock_by_item.clear();
        for (a, s) in stock {
            if !a.enabled {
                continue;
            }
            let Some(world_selector) = parse_selector(a.id, &s.world_selector, worlds) else {
                continue;
            };
            let world_ids: HashSet<i32> = worlds
                .lookup_selector(world_selector)
                .map(|scope| scope.all_worlds().map(|w| w.id).collect())
                .unwrap_or_default();
            let (mem_fired, state) = old_stock.get(&a.id).cloned().unwrap_or_default();
            let db_fired = a.last_fired_at.map(|t| t.with_timezone(&Utc));
            self.stock_by_item
                .entry(s.item_id)
                .or_default()
                .push(StockRule {
                    alert_id: a.id,
                    owner: a.owner,
                    item_id: s.item_id,
                    world_ids,
                    hq_only: s.hq_only,
                    cooldown_seconds: a.cooldown_seconds,
                    last_fired_at: db_fired.max(mem_fired),
                    state,
                });
        }
    }

    /// Apply a grouped recount (issued at `issued_at`) to every back-in-stock
    /// rule on the counted items.
    fn apply_counts(
        &mut self,
        item_ids: &[i32],
        counts: &[(i32, i32, bool, i64)],
        issued_at: DateTime<Utc>,
    ) {
        for item_id in item_ids {
            for rule in self.stock_by_item.get_mut(item_id).into_iter().flatten() {
                let n = rule.count_in(counts);
                rule.state.observe_count(n, issued_at);
            }
        }
    }
}

/// Deserialize and sanity-check a stored selector. An unresolvable one is
/// logged once here rather than failing silently on every listing.
fn parse_selector(
    alert_id: i32,
    json: &serde_json::Value,
    worlds: &WorldHelper,
) -> Option<AnySelector> {
    match serde_json::from_value::<AnySelector>(json.clone()) {
        Ok(selector) => {
            if worlds.lookup_selector(selector).is_none() {
                warn!(
                    alert_id,
                    ?selector,
                    "market-trigger world_selector does not resolve; rule will never match"
                );
            }
            Some(selector)
        }
        Err(e) => {
            warn!(alert_id, "could not deserialize world_selector: {e}");
            None
        }
    }
}

/// Discord embed title + body for a below-median fire. Pure.
pub fn format_below_median_message(
    item_name: &str,
    matched_price: i32,
    percent_below: i32,
    scope_name: &str,
    baseline: &ItemStatsVariant,
    click_url: &str,
) -> (String, String) {
    let quality = if baseline.hq { "HQ" } else { "NQ" };
    let median = baseline.p50_30d;
    let samples = baseline.cleaned_sample_size_30d;
    let title = format!("📉 {item_name} at {matched_price} gil ({percent_below}% below median)");
    let body = format!(
        "30-day median ({scope_name}, {quality}): {median} gil from {samples} sales\nhttps://ultros.app{click_url}"
    );
    (title, body)
}

/// Discord embed title + body for a back-in-stock fire. Pure.
pub fn format_back_in_stock_message(
    item_name: &str,
    world_name: &str,
    price: i32,
    quantity: i32,
    hq: bool,
    click_url: &str,
) -> (String, String) {
    let title = format!("📦 {item_name} is back in stock on {world_name}");
    let hq_suffix = if hq { " (HQ)" } else { "" };
    let body = format!("{quantity}× at {price} gil{hq_suffix}\nhttps://ultros.app{click_url}");
    (title, body)
}

/// Shared handles the listener needs beyond the event buses.
pub struct MarketTriggerServices {
    pub ctx: serenity_prelude::Context,
    pub world_helper: Arc<WorldHelper>,
    pub notifications: EventProducer<NotificationEvent>,
    pub medians: Arc<MedianCache>,
}

pub struct MarketTriggerListener {
    /// Held so dropping the listener ends its loop (see `PriceAlertListener`).
    #[allow(dead_code)]
    stop_tx: tokio::sync::mpsc::Sender<()>,
}

struct Ctx {
    db: UltrosDb,
    state: Arc<Mutex<TrackerState>>,
    services: MarketTriggerServices,
}

impl MarketTriggerListener {
    #[instrument(skip(db, listings, alert_events, services))]
    pub async fn start(
        db: UltrosDb,
        mut listings: EventBus<ListingEventData>,
        mut alert_events: EventBus<alert::Model>,
        services: MarketTriggerServices,
    ) -> Result<Self> {
        let ctx = Arc::new(Ctx {
            db,
            state: Arc::new(Mutex::new(TrackerState::default())),
            services,
        });
        reload(&ctx).await?;
        {
            let guard = ctx.state.lock().await;
            info!(
                "market-trigger tracker started with {} below-median and {} back-in-stock items",
                guard.median_by_item.len(),
                guard.stock_by_item.len()
            );
        }

        let (stop_tx, mut stop_rx) = tokio::sync::mpsc::channel::<()>(1);
        tokio::spawn(async move {
            let mut reconcile_tick =
                tokio::time::interval(Duration::from_secs(STOCK_RECONCILE_SECS));
            reconcile_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let mut baseline_tick =
                tokio::time::interval(Duration::from_secs(BASELINE_REFRESH_SECS));
            baseline_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            // Both first ticks fire immediately; `reload` already reconciled
            // and fetched every baseline.
            reconcile_tick.tick().await;
            baseline_tick.tick().await;
            loop {
                tokio::select! {
                    _ = stop_rx.recv() => break,
                    _ = reconcile_tick.tick() => reconcile_all(&ctx).await,
                    _ = baseline_tick.tick() => {
                        // On its own task so an hourly ClickHouse round trip
                        // never stalls the listing loop.
                        let refresher = ctx.clone();
                        tokio::spawn(async move {
                            let keys = refresher.state.lock().await.baseline_keys();
                            refresher.services.medians.refresh_all(&keys, Utc::now()).await;
                        });
                    }
                    msg = alert_events.recv() => match msg {
                        Ok(_) => {
                            if let Err(e) = reload(&ctx).await {
                                error!("market-trigger tracker reload failed: {e}");
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            error!("market-trigger tracker lagged, dropped {n} alert events");
                            if let Err(e) = reload(&ctx).await {
                                error!("market-trigger tracker reload failed: {e}");
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    },
                    msg = listings.recv() => match msg {
                        Ok(EventType::Add(added)) => handle_added(&ctx, &added).await,
                        Ok(EventType::Remove(removed)) => handle_removed(&ctx, &removed).await,
                        Ok(EventType::Update(_)) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            // Dropped adds/removes may have left an arm stale
                            // or missed an emptying; recount everything.
                            error!("market-trigger tracker lagged, dropped {n} listing events");
                            reconcile_all(&ctx).await;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    },
                }
            }
        });

        Ok(Self { stop_tx })
    }
}

/// Reload rules from the DB, recount every back-in-stock scope, and fetch
/// baselines for any newly referenced key.
async fn reload(ctx: &Ctx) -> Result<()> {
    let median = ctx.db.get_all_active_below_median_alerts().await?;
    let stock = ctx.db.get_all_active_back_in_stock_alerts().await?;
    let keys = {
        let mut guard = ctx.state.lock().await;
        guard.rebuild(&median, &stock, &ctx.services.world_helper);
        guard.baseline_keys()
    };
    reconcile_all(ctx).await;
    // Detached: a slow or unreachable ClickHouse must not hold up startup
    // (the other alert listeners start after this one) or the listing loop.
    let medians = ctx.services.medians.clone();
    tokio::spawn(async move { medians.fill_missing(&keys, Utc::now()).await });
    Ok(())
}

async fn reconcile_all(ctx: &Ctx) {
    let item_ids = ctx.state.lock().await.stock_item_ids();
    recount(ctx, &item_ids).await;
}

/// Recount the given items' listings and feed the result to their rules.
async fn recount(ctx: &Ctx, item_ids: &[i32]) {
    if item_ids.is_empty() {
        return;
    }
    let issued_at = Utc::now();
    match ctx
        .db
        .count_active_listings_by_world_quality(item_ids)
        .await
    {
        Ok(counts) => {
            ctx.state
                .lock()
                .await
                .apply_counts(item_ids, &counts, issued_at);
        }
        Err(e) => warn!(
            items = item_ids.len(),
            "back-in-stock recount failed; leaving state unchanged: {e}"
        ),
    }
}

async fn handle_removed(ctx: &Ctx, removed: &ListingEventData) {
    // Only recount when some rule could have been emptied by this removal.
    let affected = {
        let guard = ctx.state.lock().await;
        guard
            .stock_by_item
            .get(&removed.item_id)
            .is_some_and(|rules| {
                rules.iter().any(|rule| {
                    removed
                        .listings
                        .iter()
                        .any(|(listing, _)| rule.counts_listing(listing))
                })
            })
    };
    if affected {
        recount(ctx, &[removed.item_id]).await;
    }
}

struct MedianFire {
    alert_id: i32,
    owner: i64,
    item_id: i32,
    percent_below: i32,
    world_selector: AnySelector,
    listing: ActiveListing,
    baselines: QualityBaselines,
}

struct StockFire {
    alert_id: i32,
    owner: i64,
    item_id: i32,
    listing: ActiveListing,
}

async fn handle_added(ctx: &Ctx, added: &ListingEventData) {
    let now = Utc::now();
    let worlds = &ctx.services.world_helper;
    let mut median_fires: Vec<MedianFire> = vec![];
    let mut stock_fires: Vec<StockFire> = vec![];
    {
        let mut guard = ctx.state.lock().await;
        if let Some(rules) = guard.median_by_item.get_mut(&added.item_id) {
            for r in rules.iter_mut() {
                let key = (r.rule.item_id, r.rule.world_selector);
                let Some(baselines) = ctx.services.medians.get(&key, now) else {
                    continue;
                };
                // One fire per event: report the cheapest matching listing.
                let best = added
                    .listings
                    .iter()
                    .map(|(l, _)| l)
                    .filter(|l| below_median_matches(&r.rule, l, &baselines, worlds, now))
                    .min_by_key(|l| l.price_per_unit);
                if let Some(listing) = best {
                    r.rule.last_fired_at = Some(now);
                    median_fires.push(MedianFire {
                        alert_id: r.alert_id,
                        owner: r.owner,
                        item_id: r.rule.item_id,
                        percent_below: r.rule.percent_below,
                        world_selector: r.rule.world_selector,
                        listing: listing.clone(),
                        baselines,
                    });
                }
            }
        }
        if let Some(rules) = guard.stock_by_item.get_mut(&added.item_id) {
            for rule in rules.iter_mut() {
                let cheapest = added
                    .listings
                    .iter()
                    .map(|(l, _)| l)
                    .filter(|l| rule.counts_listing(l))
                    .min_by_key(|l| l.price_per_unit);
                let Some(listing) = cheapest else {
                    continue;
                };
                if rule
                    .state
                    .on_listing_added(now, rule.last_fired_at, rule.cooldown_seconds)
                {
                    rule.last_fired_at = Some(now);
                    stock_fires.push(StockFire {
                        alert_id: rule.alert_id,
                        owner: rule.owner,
                        item_id: rule.item_id,
                        listing: listing.clone(),
                    });
                }
            }
        }
    }

    for fire in median_fires {
        let Some(baseline) = fire.baselines.for_quality(fire.listing.hq) else {
            continue;
        };
        let world_name = world_name(worlds, fire.listing.world_id);
        let scope_name = worlds
            .lookup_selector(fire.world_selector)
            .map(|s| s.get_name().to_string())
            .unwrap_or_default();
        let click_url = item_click_url(fire.item_id, world_name.as_deref());
        let (title, body) = format_below_median_message(
            &resolve_item_name(fire.item_id),
            fire.listing.price_per_unit,
            fire.percent_below,
            &scope_name,
            baseline,
            &click_url,
        );
        deliver(
            ctx,
            Outgoing {
                alert_id: fire.alert_id,
                owner: fire.owner,
                item_id: fire.item_id,
                matched_price: fire.listing.price_per_unit,
                title: &title,
                body: &body,
                click_url: &click_url,
            },
        )
        .await;
    }

    for fire in stock_fires {
        let world_name = world_name(worlds, fire.listing.world_id);
        let click_url = item_click_url(fire.item_id, world_name.as_deref());
        let (title, body) = format_back_in_stock_message(
            &resolve_item_name(fire.item_id),
            world_name.as_deref().unwrap_or("?"),
            fire.listing.price_per_unit,
            fire.listing.quantity,
            fire.listing.hq,
            &click_url,
        );
        deliver(
            ctx,
            Outgoing {
                alert_id: fire.alert_id,
                owner: fire.owner,
                item_id: fire.item_id,
                matched_price: fire.listing.price_per_unit,
                title: &title,
                body: &body,
                click_url: &click_url,
            },
        )
        .await;
    }
}

fn world_name(worlds: &WorldHelper, world_id: i32) -> Option<String> {
    worlds
        .lookup_selector(AnySelector::World(world_id))
        .map(|w| w.get_name().to_string())
}

/// One rendered fire, ready to dispatch and record.
struct Outgoing<'a> {
    alert_id: i32,
    owner: i64,
    item_id: i32,
    matched_price: i32,
    title: &'a str,
    body: &'a str,
    click_url: &'a str,
}

async fn deliver(ctx: &Ctx, out: Outgoing<'_>) {
    let Outgoing {
        alert_id,
        owner,
        item_id,
        matched_price,
        title,
        body,
        click_url,
    } = out;
    let push = PushOptions::for_alert(AlertKind::Price, alert_id, click_url);
    let result = dispatch_alert(alert_id, title, body, &push, &ctx.db, &ctx.services.ctx).await;
    let delivered = result.is_ok();
    record_fire(
        &ctx.db,
        &ctx.services.notifications,
        AlertFire {
            alert_id,
            owner,
            item_id,
            matched_listing_id: None,
            matched_price: Some(matched_price),
            title,
            body,
            click_url,
            delivered,
            delivery_error: result.err().map(|e| e.to_string()),
        },
    )
    .await;
    if delivered && let Err(e) = ctx.db.update_alert_last_fired(alert_id).await {
        error!("failed to update last_fired_at for market-trigger alert {alert_id}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultros_api_types::trends::ConfidenceBand;

    fn t(secs: i64) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_800_000_000 + secs, 0).unwrap()
    }

    const MIN: i64 = BACK_IN_STOCK_MIN_EMPTY_SECS;

    #[test]
    fn restock_after_minimum_empty_time_fires_once() {
        let mut s = StockState::default();
        s.observe_count(0, t(0));
        assert!(s.on_listing_added(t(MIN), None, 3600));
        assert_eq!(s.empty_since, None);
        // A second add right after is no longer a restock.
        assert!(!s.on_listing_added(t(MIN + 1), Some(t(MIN)), 3600));
    }

    #[test]
    fn reprice_flicker_disarms_without_firing() {
        let mut s = StockState::default();
        s.observe_count(0, t(0));
        assert!(!s.on_listing_added(t(2), None, 3600));
        assert_eq!(s.empty_since, None);
    }

    #[test]
    fn unarmed_add_never_fires() {
        let mut s = StockState::default();
        assert!(!s.on_listing_added(t(10_000), None, 3600));
    }

    #[test]
    fn restock_on_cooldown_is_swallowed_and_disarms() {
        let mut s = StockState::default();
        s.observe_count(0, t(0));
        assert!(!s.on_listing_added(t(MIN), Some(t(MIN - 60)), 3600));
        assert_eq!(s.empty_since, None);
    }

    #[test]
    fn repeated_zero_counts_keep_the_original_arm_time() {
        let mut s = StockState::default();
        s.observe_count(0, t(0));
        s.observe_count(0, t(500));
        assert_eq!(s.empty_since, Some(t(0)));
    }

    #[test]
    fn nonzero_count_disarms_a_stale_arm() {
        let mut s = StockState::default();
        s.observe_count(0, t(0));
        s.observe_count(3, t(100));
        assert_eq!(s.empty_since, None);
    }

    #[test]
    fn out_of_order_recount_results_keep_the_newest() {
        let mut s = StockState::default();
        // Recount issued at t(20) saw the board empty and landed first…
        s.observe_count(0, t(20));
        // …then an older recount (issued t(10), when a listing still existed)
        // lands late. It must not disarm.
        s.observe_count(1, t(10));
        assert_eq!(s.empty_since, Some(t(20)));
    }

    #[test]
    fn zero_count_issued_before_an_add_is_ignored() {
        let mut s = StockState::default();
        s.on_listing_added(t(10), None, 3600);
        // Recount issued at t(5) read the board before the t(10) add landed.
        s.observe_count(0, t(5));
        assert_eq!(s.empty_since, None);
        // A recount issued after the add is trusted.
        s.observe_count(0, t(20));
        assert_eq!(s.empty_since, Some(t(20)));
    }

    fn stock_rule(hq_only: bool) -> StockRule {
        StockRule {
            alert_id: 1,
            owner: 1,
            item_id: 42,
            world_ids: [100, 101].into_iter().collect(),
            hq_only,
            cooldown_seconds: 3600,
            last_fired_at: None,
            state: StockState::default(),
        }
    }

    #[test]
    fn count_in_sums_only_scope_item_and_quality() {
        let counts = [
            (42, 100, false, 2),
            (42, 101, true, 1),
            (42, 999, true, 7), // outside scope
            (43, 100, true, 5), // other item
        ];
        assert_eq!(stock_rule(false).count_in(&counts), 3);
        assert_eq!(stock_rule(true).count_in(&counts), 1);
    }

    #[test]
    fn hq_only_rule_ignores_nq_listings() {
        let rule = stock_rule(true);
        let mut l = ActiveListing {
            id: 1,
            world_id: 100,
            item_id: 42,
            retainer_id: 1,
            price_per_unit: 10,
            quantity: 1,
            hq: false,
            timestamp: chrono::NaiveDateTime::default(),
        };
        assert!(!rule.counts_listing(&l));
        l.hq = true;
        assert!(rule.counts_listing(&l));
        l.world_id = 999;
        assert!(!rule.counts_listing(&l));
    }

    #[test]
    fn apply_counts_updates_every_rule_on_the_item() {
        let mut state = TrackerState::default();
        state
            .stock_by_item
            .insert(42, vec![stock_rule(false), stock_rule(true)]);
        state.apply_counts(&[42], &[(42, 100, false, 2)], t(0));
        let rules = &state.stock_by_item[&42];
        // NQ-inclusive rule sees 2 listings; the HQ-only rule sees none.
        assert_eq!(rules[0].state.empty_since, None);
        assert_eq!(rules[1].state.empty_since, Some(t(0)));
    }

    #[test]
    fn message_formats_carry_the_numbers() {
        let baseline = ItemStatsVariant {
            hq: true,
            sample_size_30d: 90,
            cleaned_sample_size_30d: 86,
            vwap_30d: 1000,
            p50_30d: 1000,
            confidence_band: ConfidenceBand::High,
            launder_suspicion: 0.0,
        };
        let (title, body) = format_below_median_message(
            "Cordial",
            700,
            30,
            "Aether",
            &baseline,
            "/item/Adamantoise/6141",
        );
        assert!(title.contains("Cordial") && title.contains("700") && title.contains("30%"));
        assert!(body.contains("Aether") && body.contains("HQ") && body.contains("1000"));
        assert!(body.contains("86 sales") && body.contains("ultros.app/item/Adamantoise/6141"));

        let (title, body) =
            format_back_in_stock_message("Cordial", "Cactuar", 250, 3, false, "/item/Cactuar/6141");
        assert!(title.contains("Cordial") && title.contains("Cactuar"));
        assert!(body.contains("3×") && body.contains("250") && !body.contains("HQ"));
    }
}
