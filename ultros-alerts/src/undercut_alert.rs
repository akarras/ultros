use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, btree_map::Entry},
    sync::Arc,
    time::Duration,
};

use anyhow::Result;
use chrono::{DateTime, Utc};
use poise::serenity_prelude::{self, Color, UserId};
use serde::Serialize;
use tokio::time::Instant;
use tracing::{debug, error, instrument, warn};
use ultros_api_types::{user::OwnedRetainer, websocket::ListingEventData};
use ultros_db::UltrosDb;

use crate::{
    alerts::{
        delivery::{
            AlertKind, DispatchOutcome, PushOptions, dispatch_alert_detailed,
            permanent_failure_reason,
        },
        inbox::{AlertFire, record_fire},
    },
    event::{EventBus, EventProducer, EventType, NotificationEvent},
};

/// Returns true when `competitor_price` undercuts `our_lowest_price` by strictly more than
/// `margin_percent`%. A `margin_percent` of 0 fires on any undercut; values ≥ 100 collapse the
/// threshold to 0 or below and effectively disable the alert.
///
/// The check matches the legacy formula `(our_lowest * (1 - margin/100)) > competitor` so we can
/// preserve existing user-visible behavior. Computed in `f64` then truncated to `i32`.
pub fn is_undercut_by_more_than_margin(
    our_lowest_price: i32,
    competitor_price: i32,
    margin_percent: i32,
) -> bool {
    let factor = 1.0 - (margin_percent as f64 / 100.0);
    let threshold = (our_lowest_price as f64 * factor) as i32;
    threshold > competitor_price
}

fn retainer_undercut_click_url(retainer_id: Option<i32>) -> String {
    retainer_id.map_or_else(
        || "/retainers/undercuts".to_string(),
        |retainer_id| format!("/retainers/undercuts#retainer-{retainer_id}"),
    )
}

pub struct RetainerAlertListener {
    pub retainer_alert_id: i32,
    pub cancellation_sender: tokio::sync::mpsc::Sender<RetainerAlertTx>,
}

/// Keep the "already alerted" flag across a refetch for every listing whose
/// lowest price is unchanged.
///
/// A refetch happens whenever one of our own listings is removed, and the
/// fresh rows all come back `has_alerted: false`. With several copies of one
/// item up (six halfgloves at the same price), one copy selling — or the
/// catch-up feed re-adding the rest — used to reset the flag while the
/// competitor that undercut us was still there, so the next listing event for
/// that item fired the identical alert again. If our lowest price didn't
/// move, nothing the user cares about changed, so the flag stays set. A
/// changed lowest price (we repriced, or the cheap copy sold leaving dearer
/// ones) is a new situation and re-arms the alert as before.
fn carry_over_alerted(
    previous: &HashMap<ListingKey, ListingValue>,
    refreshed: &mut HashMap<ListingKey, ListingValue>,
) {
    for (key, value) in refreshed.iter_mut() {
        if let Some(prev) = previous.get(key)
            && prev.has_alerted
            && prev.lowest_price == value.lowest_price
        {
            value.has_alerted = true;
        }
    }
}

async fn get_user_unique_retainer_ids_and_listing_ids_by_price(
    ultros_db: &UltrosDb,
    discord_user: u64,
) -> Result<(HashSet<i32>, HashMap<ListingKey, ListingValue>)> {
    // this might be better as a sql query
    let retainer_listings = ultros_db
        .get_retainer_listings_for_discord_user(discord_user)
        .await?;
    // get a list of what retainers and items the users have
    let user_retainer_ids: HashSet<i32> = retainer_listings.iter().map(|(_, r, _)| r.id).collect();
    // map item id -> min(price_per_unit)
    let user_lowest_listings: HashMap<_, _> = retainer_listings
        .into_iter()
        .flat_map(|(_, _, listings)| {
            listings.into_iter().map(|l| {
                (
                    ListingKey {
                        item_id: l.item_id,
                        world_id: l.world_id,
                        hq: l.hq,
                    },
                    ListingValue {
                        lowest_price: l.price_per_unit,
                        has_alerted: false,
                    },
                )
            })
        })
        .fold(HashMap::new(), |mut map, (item_id, price)| {
            let entry = map.entry(item_id).or_insert(price);
            *entry = price.min(*entry);
            map
        });
    Ok((user_retainer_ids, user_lowest_listings))
}

/// Result of the legacy `alert_discord_destination` fallback.
enum LegacyOutcome {
    /// At least one legacy channel accepted the message.
    Delivered,
    /// This alert has no legacy destinations at all. Distinct from success:
    /// sending to nothing used to report `Ok(())`, which marked the alert
    /// event as delivered even though nobody was notified.
    NoDestinations,
    /// Every legacy destination failed.
    Failed(anyhow::Error),
}

#[instrument(skip(ultros_db, ctx))]
async fn send_discord_alerts(
    alert_id: i32,
    discord_user_id: u64,
    ultros_db: &UltrosDb,
    ctx: &serenity_prelude::Context,
    undercut_msg: &str,
) -> Result<LegacyOutcome> {
    let destinations = ultros_db.get_alert_discord_destinations(alert_id).await?;
    if destinations.is_empty() {
        return Ok(LegacyOutcome::NoDestinations);
    }
    let mut last_err = None;
    let mut any_ok = false;
    for destination in &destinations {
        let channel_id = serenity_prelude::ChannelId::new(destination.channel_id as u64);
        // Keep going after a failure. One deleted channel used to abort the
        // whole loop, so every destination after it silently went unnotified.
        match channel_id
            .send_message(
                ctx,
                serenity_prelude::CreateMessage::new()
                    .embed(
                        serenity_prelude::CreateEmbed::new()
                            .color(Color::from_rgb(255, 0, 0))
                            .title("🔔😔 Undercut Alert")
                            .description(undercut_msg),
                    )
                    .allowed_mentions(
                        serenity_prelude::CreateAllowedMentions::new()
                            .users([UserId::new(discord_user_id)]),
                    )
                    .content(format!("<@{discord_user_id}>")),
            )
            .await
        {
            Ok(_) => any_ok = true,
            Err(e) => last_err = Some(e),
        }
    }
    Ok(match last_err {
        Some(e) if !any_ok => LegacyOutcome::Failed(e.into()),
        _ => LegacyOutcome::Delivered,
    })
}

pub enum RetainerAlertTx {
    Stop,
    UpdateMargin(i32),
    UpdateCooldown(i32),
}

/// Shared handles `RetainerAlertListener::create_listener` needs beyond its
/// identifying ids and event buses. Grouped so adding `notifications` for the
/// notification inbox didn't push the function's argument count past
/// clippy's `too_many_arguments` threshold.
pub struct RetainerAlertServices {
    pub ctx: serenity_prelude::Context,
    pub notifications: EventProducer<NotificationEvent>,
}

/// One of our listings as undercut detection sees it: an item on a world,
/// NQ or HQ.
#[derive(Debug, Hash, Eq, PartialEq, PartialOrd, Ord, Copy, Clone)]
pub struct ListingKey {
    item_id: i32,
    world_id: i32,
    hq: bool,
}

#[derive(Debug, PartialEq, PartialOrd, Ord, Eq, Copy, Clone)]
struct ListingValue {
    lowest_price: i32,
    has_alerted: bool,
}

#[derive(Debug, Eq, PartialEq, PartialOrd, Ord, Serialize)]
pub struct UndercutRetainer {
    pub id: i32,
    pub name: String,
    pub undercut_amount: i32,
}

#[derive(Debug)]
pub struct Undercut {
    pub item_id: i32,
    pub undercut_retainers: Vec<UndercutRetainer>,
}

/// An undercut as the tracker detected it: which of our listings (item,
/// world, HQ) the competitor went under, so the roll-up can check at send
/// time that it still stands.
#[derive(Debug)]
pub struct DetectedUndercut {
    listing: ListingKey,
    pub undercut: Undercut,
}

#[derive(Debug)]
pub struct UndercutTracker {
    retainer_ids: HashSet<i32>,
    user_lowest_listings: HashMap<ListingKey, ListingValue>,
    discord_user_id: u64,
    margin: i32,
    db: UltrosDb,
}

impl UndercutTracker {
    pub async fn new(
        discord_user: u64,
        ultros_db: &UltrosDb,
        margin: i32,
    ) -> Result<Self, anyhow::Error> {
        let (user_retainer_ids, user_lowest_listings) =
            get_user_unique_retainer_ids_and_listing_ids_by_price(ultros_db, discord_user).await?;

        Ok(Self {
            retainer_ids: user_retainer_ids,
            user_lowest_listings,
            discord_user_id: discord_user,
            margin,
            db: ultros_db.clone(),
        })
    }

    /// Whether an undercut detected on `listing` still stands: the listing is
    /// still ours and its lowest price hasn't moved since. Our lowest price
    /// changing (we repriced, or the cheap copy sold) clears `has_alerted`,
    /// and a competitor listing under the new price is detected afresh.
    fn is_still_alerted(&self, listing: &ListingKey) -> bool {
        self.user_lowest_listings
            .get(listing)
            .is_some_and(|value| value.has_alerted)
    }

    pub async fn handle_listing_event(
        &mut self,
        listings: Result<EventType<Arc<ListingEventData>>, anyhow::Error>,
    ) -> Result<Option<DetectedUndercut>, anyhow::Error> {
        let listing = listings?;
        match listing {
            EventType::Remove(removed) => {
                for (removed, _) in removed.listings.iter() {
                    // if we removed our listing, we need to refetch our pricing from the database if the listing was the lowest
                    if self.retainer_ids.contains(&removed.retainer_id)
                        && let Some(value) = self
                            .user_lowest_listings
                            .get(&ListingKey {
                                item_id: removed.item_id,
                                world_id: removed.world_id,
                                hq: removed.hq,
                            })
                            .filter(|v| v.lowest_price >= removed.price_per_unit)
                            .copied()
                        && value.lowest_price >= removed.price_per_unit
                        && let Ok((retainer_ids, mut listings)) =
                            get_user_unique_retainer_ids_and_listing_ids_by_price(
                                &self.db,
                                self.discord_user_id,
                            )
                            .await
                    {
                        carry_over_alerted(&self.user_lowest_listings, &mut listings);
                        self.retainer_ids = retainer_ids;
                        self.user_lowest_listings = listings;
                    }
                }
            }
            EventType::Add(added) => {
                // update our own data from the added list
                if let Some((retainer_listing, _)) = added
                    .listings
                    .iter()
                    .filter(|(added, _)| self.retainer_ids.contains(&added.retainer_id))
                    .min_by_key(|(i, _)| i.price_per_unit)
                {
                    let entry = self
                        .user_lowest_listings
                        .entry(ListingKey {
                            item_id: retainer_listing.item_id,
                            world_id: retainer_listing.world_id,
                            hq: retainer_listing.hq,
                        })
                        .or_insert(ListingValue {
                            lowest_price: retainer_listing.price_per_unit,
                            has_alerted: false,
                        });
                    if retainer_listing.price_per_unit < entry.lowest_price {
                        *entry = ListingValue {
                            lowest_price: retainer_listing.price_per_unit,
                            has_alerted: false,
                        };
                    }
                }
                // items in an added vec should all be the same type, so lets just find the cheapest item
                if let Some((added, _)) =
                    added.listings.iter().min_by_key(|(a, _)| a.price_per_unit)
                    && let Some(our_price) = self.user_lowest_listings.get_mut(&ListingKey {
                        item_id: added.item_id,
                        world_id: added.world_id,
                        hq: added.hq,
                    })
                {
                    debug!(
                        "comparing our_price {our_price:?} margin={} {added:?}",
                        self.margin
                    );
                    if is_undercut_by_more_than_margin(
                        our_price.lowest_price,
                        added.price_per_unit,
                        self.margin,
                    ) && !our_price.has_alerted
                    {
                        our_price.has_alerted = true;
                        // figure out what retainers have been undercut
                        let retainers = self
                            .db
                            .get_retainer_listings_for_discord_user(self.discord_user_id)
                            .await
                            .map(|i| {
                                i.into_iter()
                                    .flat_map(|(_, r, listings)| {
                                        listings
                                            .iter()
                                            .find(|i| {
                                                i.item_id == added.item_id
                                                    && i.hq == added.hq
                                                    && i.world_id == added.world_id
                                                    && is_undercut_by_more_than_margin(
                                                        i.price_per_unit,
                                                        added.price_per_unit,
                                                        self.margin,
                                                    )
                                            })
                                            .map(|l| (r, l.price_per_unit))
                                    })
                                    .map(|(retainer, price)| UndercutRetainer {
                                        id: retainer.id,
                                        name: retainer.name,
                                        undercut_amount: price,
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        return Ok(Some(DetectedUndercut {
                            listing: ListingKey {
                                item_id: added.item_id,
                                world_id: added.world_id,
                                hq: added.hq,
                            },
                            undercut: Undercut {
                                item_id: added.item_id,
                                undercut_retainers: retainers,
                            },
                        }));
                    }
                }
            }
            EventType::Update(_) => {}
        }
        Ok(None)
    }
}

/// How long the roll-up waits after the most recent undercut for more to
/// arrive before sending. Universalis pushes a competitor's relist of a whole
/// gear set as one listing event per item over a few seconds, so a short
/// quiet window catches the burst without noticeably delaying the alert.
pub const ROLLUP_QUIET_WINDOW: Duration = Duration::from_secs(30);

/// Upper bound on how long the first undercut in a batch is held, however
/// steadily more keep trickling in. Undercut alerts are time-sensitive — the
/// user wants to reprice — so a busy market must not defer them indefinitely.
/// The alert's own cooldown can still hold a batch longer: see [`send_at`].
pub const ROLLUP_MAX_WINDOW: Duration = Duration::from_secs(120);

/// Undercuts one alert's tracker has detected but not yet sent.
///
/// Held back briefly so a burst becomes a single notification instead of one
/// per listing event. That matters for the user's inbox (six identical rows
/// in a minute) and for Discord and Web Push, where one message per event
/// walks straight into rate limits. Items are keyed by id, so a second
/// undercut on an item already waiting merges into it — the retainer sets are
/// unioned — rather than queueing a duplicate.
#[derive(Debug, Default)]
pub struct UndercutRollup {
    /// Keyed by item id, sorted so the summary lists items in a stable order.
    pending: BTreeMap<i32, PendingItem>,
    first_at: Option<Instant>,
    last_at: Option<Instant>,
}

#[derive(Debug, Default)]
struct PendingItem {
    /// `retainer_id -> retainer`, sorted for a stable retainer order.
    retainers: BTreeMap<i32, UndercutRetainer>,
    /// Our listings of the item that were undercut, re-checked at send time.
    listings: BTreeSet<ListingKey>,
}

impl UndercutRollup {
    pub fn push(&mut self, detected: DetectedUndercut, now: Instant) {
        let DetectedUndercut { listing, undercut } = detected;
        let item = self.pending.entry(undercut.item_id).or_default();
        item.listings.insert(listing);
        for retainer in undercut.undercut_retainers {
            match item.retainers.entry(retainer.id) {
                Entry::Vacant(slot) => {
                    slot.insert(retainer);
                }
                Entry::Occupied(mut slot) => {
                    let existing = slot.get_mut();
                    existing.undercut_amount =
                        existing.undercut_amount.min(retainer.undercut_amount);
                }
            }
        }
        self.first_at.get_or_insert(now);
        self.last_at = Some(now);
    }

    /// When the pending batch should be sent, or `None` while nothing is
    /// waiting. Extends with each new undercut up to [`ROLLUP_MAX_WINDOW`]
    /// after the first.
    pub fn deadline(&self) -> Option<Instant> {
        let first = self.first_at?;
        let last = self.last_at?;
        Some((last + ROLLUP_QUIET_WINDOW).min(first + ROLLUP_MAX_WINDOW))
    }

    /// Take everything pending, in item-id order, and reset the timer.
    ///
    /// Items none of whose undercut listings pass `still_undercut` are
    /// dropped. A batch can wait out a long cooldown, and by the time it goes
    /// out the user may already have repriced or sold the item — telling
    /// them about it then is noise.
    pub fn drain(&mut self, still_undercut: impl Fn(&ListingKey) -> bool) -> Vec<Undercut> {
        self.first_at = None;
        self.last_at = None;
        std::mem::take(&mut self.pending)
            .into_iter()
            .filter(|(_, item)| item.listings.iter().any(&still_undercut))
            .map(|(item_id, item)| Undercut {
                item_id,
                undercut_retainers: item.retainers.into_values().collect(),
            })
            .collect()
    }
}

/// An alert's cooldown: the minimum time between two deliveries.
#[derive(Debug, Clone, Copy)]
struct AlertCooldown {
    seconds: i32,
    last_fired_at: Option<DateTime<Utc>>,
}

impl AlertCooldown {
    /// When the alert may deliver again, or `None` if it never has.
    fn ends_at(&self) -> Option<DateTime<Utc>> {
        self.last_fired_at
            .map(|at| at + chrono::Duration::seconds(i64::from(self.seconds)))
    }
}

/// When a batch that's ready at `deadline` actually goes out: no earlier than
/// the end of the alert's cooldown. Undercuts detected meanwhile keep merging
/// into the held batch, so a cooldown turns a busy hour into one message
/// rather than dropping what it held back.
fn send_at(deadline: Option<Instant>, cooldown_ends: Option<Instant>) -> Option<Instant> {
    let deadline = deadline?;
    Some(cooldown_ends.map_or(deadline, |end| deadline.max(end)))
}

/// The tokio instant matching wall-clock time `at` (now, if `at` has passed).
fn instant_at(at: DateTime<Utc>) -> Instant {
    Instant::now() + (at - Utc::now()).to_std().unwrap_or_default()
}

/// Resolves once `deadline` passes, or never when there is no batch waiting —
/// the idle arm of the listener's `select!`.
async fn sleep_until_or_never(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

pub const UNDERCUT_ALERT_TITLE: &str = "Undercut Alert";

/// Items named in a summary before it falls back to "and N more".
const MAX_LISTED_ITEMS: usize = 10;

/// One rolled-up batch of undercuts rendered for every delivery channel.
#[derive(Debug, PartialEq, Eq)]
pub struct UndercutMessage {
    /// Representative item for the inbox row: the lowest item id in the batch.
    pub item_id: i32,
    pub body: String,
    pub click_url: String,
}

fn retainer_names(undercut: &Undercut) -> String {
    undercut
        .undercut_retainers
        .iter()
        .map(|r| r.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Render a batch as one message. Items whose name can't be resolved are
/// skipped (as a single unknown item always was); `None` when nothing is left.
///
/// A single item keeps the long-standing wording. Several items become one
/// sentence listing them, attributing each to its retainers only when the
/// retainer sets differ. Everything stays on one line because the in-app
/// inbox collapses newlines; the bare link line at the end is what Discord
/// and Web Push readers click, and the inbox strips it.
pub fn format_undercut_message(
    undercuts: &[Undercut],
    item_name: impl Fn(i32) -> Option<String>,
) -> Option<UndercutMessage> {
    let named: Vec<(&Undercut, String)> = undercuts
        .iter()
        .filter_map(|undercut| item_name(undercut.item_id).map(|name| (undercut, name)))
        .collect();
    let item_id = named.iter().map(|(undercut, _)| undercut.item_id).min()?;
    let click_url = retainer_undercut_click_url(
        named
            .iter()
            .flat_map(|(undercut, _)| undercut.undercut_retainers.iter().map(|r| r.id))
            .min(),
    );
    let summary = match named.as_slice() {
        [(undercut, name)] => format!(
            "Your retainers {} have been undercut on {name}",
            retainer_names(undercut)
        ),
        _ => {
            let retainer_sets: HashSet<String> = named
                .iter()
                .map(|(undercut, _)| retainer_names(undercut))
                .collect();
            let shared = (retainer_sets.len() == 1).then(|| retainer_names(named[0].0));
            let mut listed: Vec<String> = named
                .iter()
                .take(MAX_LISTED_ITEMS)
                .map(|(undercut, name)| match &shared {
                    Some(_) => name.clone(),
                    None => {
                        let retainers = retainer_names(undercut);
                        if retainers.is_empty() {
                            name.clone()
                        } else {
                            format!("{name} ({retainers})")
                        }
                    }
                })
                .collect();
            if named.len() > MAX_LISTED_ITEMS {
                listed.push(format!("and {} more", named.len() - MAX_LISTED_ITEMS));
            }
            let count = named.len();
            match shared {
                Some(retainers) => format!(
                    "Your retainers {retainers} have been undercut on {count} items: {}",
                    listed.join(", ")
                ),
                None => format!(
                    "Your retainers have been undercut on {count} items: {}",
                    listed.join(", ")
                ),
            }
        }
    };
    Some(UndercutMessage {
        item_id,
        body: format!("{summary}\n\nhttps://ultros.app{click_url}"),
        click_url,
    })
}

/// Everything the listener task needs to push one message out and record it.
struct UndercutDeliverer {
    alert_id: i32,
    discord_user: u64,
    db: UltrosDb,
    ctx: serenity_prelude::Context,
    notifications: EventProducer<NotificationEvent>,
}

impl UndercutDeliverer {
    /// Send a batch as one message; true when it reached at least one
    /// destination.
    async fn deliver_batch(&self, undercuts: Vec<Undercut>) -> bool {
        let items = &xiv_gen_db::data().items;
        let Some(message) = format_undercut_message(&undercuts, |item_id| {
            items
                .get(&xiv_gen::ItemId(item_id))
                .map(|item| item.name.clone())
        }) else {
            return false;
        };
        self.deliver(&message).await
    }

    async fn deliver(&self, message: &UndercutMessage) -> bool {
        let UndercutMessage {
            item_id,
            body,
            click_url,
        } = message;
        let alert_id = self.alert_id;
        let title = UNDERCUT_ALERT_TITLE;
        let mut delivered = false;
        let mut delivery_error = None;
        // Tracks whether every destination failed for a reason a retry can't
        // change (the channel is gone, the bot was removed). Those are
        // recorded on the event but not re-reported as a new error every fire.
        let mut permanent = false;
        let push = PushOptions::for_alert(AlertKind::Undercut, alert_id, click_url);
        let endpoint_failure = match dispatch_alert_detailed(
            alert_id, title, body, &push, &self.db, &self.ctx,
        )
        .await
        {
            DispatchOutcome::Delivered => {
                delivered = true;
                None
            }
            DispatchOutcome::PermanentFailure(reason) => {
                permanent = true;
                Some(reason)
            }
            DispatchOutcome::TransientFailure(e) => Some(format!("{e}")),
        };
        // Always give the legacy destinations a turn — they're a separate set
        // of channels, and some pre-endpoint alerts still have nothing else.
        if let Some(endpoint_failure) = endpoint_failure {
            match send_discord_alerts(alert_id, self.discord_user, &self.db, &self.ctx, body).await
            {
                Ok(LegacyOutcome::Delivered) => {
                    delivered = true;
                    permanent = false;
                }
                Ok(LegacyOutcome::NoDestinations) => {
                    // Nothing else to try, so the endpoint verdict stands.
                    delivery_error = Some(endpoint_failure);
                }
                Ok(LegacyOutcome::Failed(legacy_error)) => {
                    // Only stays "permanent" if the fallback is dead too.
                    permanent = permanent && permanent_failure_reason(&legacy_error).is_some();
                    delivery_error = Some(format!(
                        "{endpoint_failure}; legacy Discord destinations failed: {legacy_error}"
                    ));
                }
                Err(lookup_error) => {
                    // Couldn't even read the destinations — transient.
                    permanent = false;
                    delivery_error = Some(format!(
                        "{endpoint_failure}; legacy Discord destinations failed: {lookup_error}"
                    ));
                }
            }
        }
        record_fire(
            &self.db,
            &self.notifications,
            AlertFire {
                alert_id,
                owner: self.discord_user as i64,
                item_id: *item_id,
                matched_listing_id: None,
                matched_price: None,
                title,
                body,
                click_url,
                delivered,
                delivery_error: delivery_error.clone(),
            },
        )
        .await;
        if delivered {
            if let Err(e) = self.db.update_alert_last_fired(alert_id).await {
                error!("failed to update undercut alert last_fired_at: {e}");
            }
        } else if let Some(error) = delivery_error {
            if permanent {
                // Steady state we've already acted on (endpoint disabled).
                // Keep it out of the error reporter — it fired ~150 times a
                // day for six alerts.
                warn!("undercut alert {alert_id} has no working destinations: {error}");
            } else {
                error!("Error sending undercut alerts {error}");
            }
        }
        delivered
    }
}

impl RetainerAlertListener {
    #[instrument(skip(ultros_db, listings, services))]
    pub async fn create_listener(
        retainer_alert_id: i32,
        alert_id: i32,
        margin: i32,
        ultros_db: UltrosDb,
        mut listings: EventBus<ListingEventData>,
        active_retainers: EventBus<OwnedRetainer>,
        services: RetainerAlertServices,
    ) -> Result<Self> {
        let RetainerAlertServices { ctx, notifications } = services;
        let alert = ultros_db
            .get_alert(alert_id)
            .await?
            .ok_or_else(|| anyhow::Error::msg("Unable to find retainer"))?;
        let discord_user = alert.owner as u64;
        let mut cooldown = AlertCooldown {
            seconds: alert.cooldown_seconds,
            last_fired_at: alert.last_fired_at.map(|at| at.with_timezone(&Utc)),
        };

        let (cancellation_sender, mut receiver) = tokio::sync::mpsc::channel::<RetainerAlertTx>(10);
        let mut undercut_tracker = UndercutTracker::new(discord_user, &ultros_db, margin).await?;
        let deliverer = UndercutDeliverer {
            alert_id,
            discord_user,
            db: ultros_db,
            ctx,
            notifications,
        };
        tokio::spawn(async move {
            let mut rollup = UndercutRollup::default();
            loop {
                // Every arm is cancel-safe (mpsc and broadcast `recv`, a
                // sleep), so an undercut is never lost when another arm wins.
                tokio::select! {
                    msg = receiver.recv() => match msg {
                        // Stop means the alert was disabled or deleted, so
                        // anything still batched is dropped with it.
                        Some(RetainerAlertTx::Stop) | None => break,
                        Some(RetainerAlertTx::UpdateMargin(m)) => {
                            undercut_tracker.margin = m;
                        }
                        Some(RetainerAlertTx::UpdateCooldown(seconds)) => {
                            cooldown.seconds = seconds;
                        }
                    },
                    listing = listings.recv() => {
                        match undercut_tracker
                            .handle_listing_event(listing.map_err(|e| e.into()))
                            .await
                        {
                            Err(e) => error!("{e:?}"),
                            Ok(None) => {}
                            Ok(Some(detected)) => rollup.push(detected, Instant::now()),
                        }
                    }
                    _ = sleep_until_or_never(send_at(
                        rollup.deadline(),
                        cooldown.ends_at().map(instant_at),
                    )) => {
                        let batch =
                            rollup.drain(|listing| undercut_tracker.is_still_alerted(listing));
                        if deliverer.deliver_batch(batch).await {
                            cooldown.last_fired_at = Some(Utc::now());
                        }
                    }
                }
            }
        });
        Ok(Self {
            retainer_alert_id,
            cancellation_sender,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- is_undercut_by_more_than_margin ----------

    #[test]
    fn margin_zero_fires_on_any_strict_undercut() {
        assert!(is_undercut_by_more_than_margin(100, 99, 0));
        assert!(is_undercut_by_more_than_margin(100, 1, 0));
    }

    #[test]
    fn margin_zero_does_not_fire_when_competitor_matches_or_exceeds_us() {
        assert!(!is_undercut_by_more_than_margin(100, 100, 0));
        assert!(!is_undercut_by_more_than_margin(100, 101, 0));
        assert!(!is_undercut_by_more_than_margin(100, 9999, 0));
    }

    #[test]
    fn margin_ten_requires_competitor_below_ninety_percent_of_our_price() {
        // our 100, margin 10 → threshold = 90. Competitor must be < 90 to trigger.
        assert!(!is_undercut_by_more_than_margin(100, 90, 10));
        assert!(is_undercut_by_more_than_margin(100, 89, 10));
        assert!(is_undercut_by_more_than_margin(100, 0, 10));
    }

    #[test]
    fn margin_fifty_requires_competitor_at_least_half_off() {
        // our 1000, margin 50 → threshold = 500.
        assert!(!is_undercut_by_more_than_margin(1000, 500, 50));
        assert!(is_undercut_by_more_than_margin(1000, 499, 50));
    }

    #[test]
    fn margin_100_collapses_threshold_to_zero_and_effectively_disables_alert() {
        // (1 - 1.0) = 0; threshold = 0; 0 > competitor only when competitor < 0,
        // which never happens with valid listings.
        assert!(!is_undercut_by_more_than_margin(1000, 0, 100));
        assert!(!is_undercut_by_more_than_margin(1000, 100, 100));
    }

    #[test]
    fn margin_over_100_yields_negative_threshold_and_disables_alert() {
        // (1 - 2.0) = -1; threshold = -1000; never triggers for non-negative competitor.
        assert!(!is_undercut_by_more_than_margin(1000, 0, 200));
        assert!(!is_undercut_by_more_than_margin(1000, 999, 200));
    }

    #[test]
    fn margin_works_against_very_cheap_listings_via_i32_truncation() {
        // our 5, margin 10 → 5 * 0.9 = 4.5 → truncates to 4. Competitor must be < 4.
        assert!(!is_undercut_by_more_than_margin(5, 4, 10));
        assert!(is_undercut_by_more_than_margin(5, 3, 10));
    }

    #[test]
    fn retainer_click_url_targets_the_retainer_anchor() {
        assert_eq!(
            retainer_undercut_click_url(Some(42)),
            "/retainers/undercuts#retainer-42"
        );
    }

    #[test]
    fn retainer_click_url_falls_back_when_no_retainer_was_found() {
        assert_eq!(retainer_undercut_click_url(None), "/retainers/undercuts");
    }

    // ---------- ListingValue Ord behavior (used by the fold in user-listings aggregation) ----------

    #[test]
    fn listing_value_orders_by_lowest_price_then_has_alerted() {
        let cheaper = ListingValue {
            lowest_price: 100,
            has_alerted: true,
        };
        let pricier = ListingValue {
            lowest_price: 200,
            has_alerted: false,
        };
        assert!(cheaper < pricier, "lower price should sort first");

        // Same price: has_alerted=false beats has_alerted=true (false < true).
        let fresh = ListingValue {
            lowest_price: 100,
            has_alerted: false,
        };
        let stale = ListingValue {
            lowest_price: 100,
            has_alerted: true,
        };
        assert!(fresh < stale);
        // Therefore .min() of the two keeps the fresh entry. This is the property the
        // user-listings aggregation fold relies on.
        assert_eq!(fresh.min(stale), fresh);
    }

    #[test]
    fn listing_key_hash_and_eq_account_for_all_three_dimensions() {
        let a = ListingKey {
            item_id: 1,
            world_id: 1,
            hq: false,
        };
        let b = ListingKey {
            item_id: 1,
            world_id: 1,
            hq: true,
        };
        let c = ListingKey {
            item_id: 1,
            world_id: 2,
            hq: false,
        };
        let d = ListingKey {
            item_id: 2,
            world_id: 1,
            hq: false,
        };
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
        assert_eq!(a, a);
    }

    // ---------- carry_over_alerted ----------

    fn key(item_id: i32) -> ListingKey {
        ListingKey {
            item_id,
            world_id: 1,
            hq: true,
        }
    }

    #[test]
    fn refetch_keeps_alerted_flag_when_our_lowest_price_is_unchanged() {
        // Six copies at 254,668, already alerted; one sells and the refetch
        // still finds the same lowest price. The competitor is still there,
        // so the alert must not re-arm.
        let previous = HashMap::from([(
            key(1),
            ListingValue {
                lowest_price: 254_668,
                has_alerted: true,
            },
        )]);
        let mut refreshed = HashMap::from([(
            key(1),
            ListingValue {
                lowest_price: 254_668,
                has_alerted: false,
            },
        )]);
        carry_over_alerted(&previous, &mut refreshed);
        assert!(refreshed[&key(1)].has_alerted);
    }

    #[test]
    fn refetch_rearms_when_our_lowest_price_changed_and_leaves_new_keys_alone() {
        let previous = HashMap::from([
            (
                key(1),
                ListingValue {
                    lowest_price: 100,
                    has_alerted: true,
                },
            ),
            (
                key(2),
                ListingValue {
                    lowest_price: 100,
                    has_alerted: false,
                },
            ),
        ]);
        let mut refreshed = HashMap::from([
            // Our cheap copy sold; the remaining one is dearer — new situation.
            (
                key(1),
                ListingValue {
                    lowest_price: 120,
                    has_alerted: false,
                },
            ),
            // Never alerted before: stays un-alerted.
            (
                key(2),
                ListingValue {
                    lowest_price: 100,
                    has_alerted: false,
                },
            ),
            // Not present before: stays un-alerted.
            (
                key(3),
                ListingValue {
                    lowest_price: 50,
                    has_alerted: false,
                },
            ),
        ]);
        carry_over_alerted(&previous, &mut refreshed);
        assert!(!refreshed[&key(1)].has_alerted);
        assert!(!refreshed[&key(2)].has_alerted);
        assert!(!refreshed[&key(3)].has_alerted);
    }

    // ---------- UndercutRollup ----------

    fn retainer(id: i32, name: &str, amount: i32) -> UndercutRetainer {
        UndercutRetainer {
            id,
            name: name.into(),
            undercut_amount: amount,
        }
    }

    fn undercut(item_id: i32, retainers: Vec<UndercutRetainer>) -> Undercut {
        Undercut {
            item_id,
            undercut_retainers: retainers,
        }
    }

    fn detected(item_id: i32, retainers: Vec<UndercutRetainer>) -> DetectedUndercut {
        DetectedUndercut {
            listing: key(item_id),
            undercut: undercut(item_id, retainers),
        }
    }

    #[test]
    fn rollup_is_idle_until_something_is_pushed() {
        let mut rollup = UndercutRollup::default();
        assert_eq!(rollup.deadline(), None);
        assert!(rollup.drain(|_| true).is_empty());
        assert_eq!(rollup.deadline(), None);
    }

    #[test]
    fn rollup_merges_repeat_undercuts_on_the_same_item() {
        // The spam case: the same item fires three times in a minute. It
        // must come out as one entry with the retainers unioned.
        let now = Instant::now();
        let mut rollup = UndercutRollup::default();
        rollup.push(detected(7, vec![retainer(1, "Seeba", 254_668)]), now);
        rollup.push(detected(7, vec![retainer(1, "Seeba", 254_668)]), now);
        rollup.push(
            detected(
                7,
                vec![
                    retainer(1, "Seeba", 250_000),
                    retainer(2, "Giltastrophe", 300_000),
                ],
            ),
            now,
        );
        let drained = rollup.drain(|_| true);
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].item_id, 7);
        assert_eq!(
            drained[0].undercut_retainers,
            vec![
                retainer(1, "Seeba", 250_000),
                retainer(2, "Giltastrophe", 300_000)
            ]
        );
    }

    #[test]
    fn rollup_drains_distinct_items_in_item_id_order_and_resets() {
        let now = Instant::now();
        let mut rollup = UndercutRollup::default();
        rollup.push(detected(30, vec![retainer(1, "Seeba", 1)]), now);
        rollup.push(detected(10, vec![retainer(1, "Seeba", 1)]), now);
        rollup.push(detected(20, vec![retainer(1, "Seeba", 1)]), now);
        let ids: Vec<i32> = rollup.drain(|_| true).iter().map(|u| u.item_id).collect();
        assert_eq!(ids, vec![10, 20, 30]);
        assert!(rollup.drain(|_| true).is_empty());
        assert_eq!(rollup.deadline(), None);
    }

    #[test]
    fn rollup_deadline_extends_with_each_undercut_within_the_quiet_window() {
        let start = Instant::now();
        let mut rollup = UndercutRollup::default();
        rollup.push(detected(1, vec![]), start);
        assert_eq!(rollup.deadline(), Some(start + ROLLUP_QUIET_WINDOW));

        let later = start + Duration::from_secs(20);
        rollup.push(detected(2, vec![]), later);
        assert_eq!(rollup.deadline(), Some(later + ROLLUP_QUIET_WINDOW));
    }

    #[test]
    fn rollup_deadline_never_exceeds_the_max_window_after_the_first_undercut() {
        let start = Instant::now();
        let mut rollup = UndercutRollup::default();
        rollup.push(detected(1, vec![]), start);
        // Keep trickling in right up to the cap: the batch still goes out at
        // first + max, not last + quiet.
        let late = start + ROLLUP_MAX_WINDOW - Duration::from_secs(1);
        rollup.push(detected(2, vec![]), late);
        assert_eq!(rollup.deadline(), Some(start + ROLLUP_MAX_WINDOW));
    }

    #[test]
    fn rollup_drops_items_whose_undercut_no_longer_stands() {
        // Held through a cooldown: item 1 was repriced in the meantime, item
        // 2 is still undercut.
        let now = Instant::now();
        let mut rollup = UndercutRollup::default();
        rollup.push(detected(1, vec![retainer(1, "Seeba", 1)]), now);
        rollup.push(detected(2, vec![retainer(1, "Seeba", 1)]), now);
        let ids: Vec<i32> = rollup
            .drain(|listing| *listing == key(2))
            .iter()
            .map(|u| u.item_id)
            .collect();
        assert_eq!(ids, vec![2]);
        assert_eq!(rollup.deadline(), None);
    }

    #[test]
    fn rollup_keeps_an_item_while_any_of_its_listings_is_still_undercut() {
        let now = Instant::now();
        let mut rollup = UndercutRollup::default();
        rollup.push(detected(1, vec![retainer(1, "Seeba", 1)]), now);
        let other_world = ListingKey {
            world_id: 2,
            ..key(1)
        };
        rollup.push(
            DetectedUndercut {
                listing: other_world,
                undercut: undercut(1, vec![retainer(2, "Giltastrophe", 1)]),
            },
            now,
        );
        let drained = rollup.drain(|listing| *listing == other_world);
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].undercut_retainers.len(), 2);
    }

    // ---------- cooldown ----------

    #[test]
    fn cooldown_ends_cooldown_seconds_after_the_last_fire() {
        let fired = DateTime::parse_from_rfc3339("2026-09-22T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let cooldown = AlertCooldown {
            seconds: 3600,
            last_fired_at: Some(fired),
        };
        assert_eq!(cooldown.ends_at(), Some(fired + chrono::Duration::hours(1)));
        let never = AlertCooldown {
            seconds: 3600,
            last_fired_at: None,
        };
        assert_eq!(never.ends_at(), None);
    }

    #[test]
    fn send_at_holds_a_ready_batch_until_the_cooldown_ends() {
        let now = Instant::now();
        let ready = now + ROLLUP_QUIET_WINDOW;
        let cooldown_end = now + Duration::from_secs(3600);
        assert_eq!(send_at(Some(ready), Some(cooldown_end)), Some(cooldown_end));
        // Off cooldown (or never fired): the roll-up window alone decides.
        assert_eq!(send_at(Some(ready), Some(now)), Some(ready));
        assert_eq!(send_at(Some(ready), None), Some(ready));
        // Nothing waiting: nothing to send, cooldown or not.
        assert_eq!(send_at(None, Some(cooldown_end)), None);
    }

    #[test]
    fn instant_at_a_past_time_is_now() {
        let before = Instant::now();
        let at = instant_at(Utc::now() - chrono::Duration::hours(1));
        assert!(at >= before && at <= Instant::now());
    }

    // ---------- format_undercut_message ----------

    fn names(id: i32) -> Option<String> {
        match id {
            1 => Some("Fire Shard".into()),
            2 => Some("Ice Shard".into()),
            3 => Some("Wind Shard".into()),
            id if id >= 100 => Some(format!("Item {id}")),
            _ => None,
        }
    }

    #[test]
    fn single_item_message_keeps_the_original_wording() {
        let batch = vec![undercut(
            1,
            vec![retainer(5, "Seeba", 1), retainer(9, "Giltastrophe", 2)],
        )];
        let message = format_undercut_message(&batch, names).unwrap();
        assert_eq!(
            message,
            UndercutMessage {
                item_id: 1,
                body: "Your retainers Seeba, Giltastrophe have been undercut on Fire Shard\n\nhttps://ultros.app/retainers/undercuts#retainer-5".into(),
                click_url: "/retainers/undercuts#retainer-5".into(),
            }
        );
    }

    #[test]
    fn several_items_with_the_same_retainers_become_one_sentence() {
        let batch = vec![
            undercut(1, vec![retainer(5, "Seeba", 1)]),
            undercut(2, vec![retainer(5, "Seeba", 1)]),
            undercut(3, vec![retainer(5, "Seeba", 1)]),
        ];
        let message = format_undercut_message(&batch, names).unwrap();
        assert_eq!(message.item_id, 1);
        assert_eq!(message.click_url, "/retainers/undercuts#retainer-5");
        assert_eq!(
            message.body,
            "Your retainers Seeba have been undercut on 3 items: Fire Shard, Ice Shard, Wind Shard\n\nhttps://ultros.app/retainers/undercuts#retainer-5"
        );
    }

    #[test]
    fn several_items_with_different_retainers_attribute_each_item() {
        let batch = vec![
            undercut(1, vec![retainer(5, "Seeba", 1)]),
            undercut(
                2,
                vec![retainer(5, "Seeba", 1), retainer(9, "Giltastrophe", 1)],
            ),
            // Retainer lookup failed for this one: no attribution, no "()".
            undercut(3, vec![]),
        ];
        let message = format_undercut_message(&batch, names).unwrap();
        assert_eq!(
            message.body,
            "Your retainers have been undercut on 3 items: Fire Shard (Seeba), Ice Shard (Seeba, Giltastrophe), Wind Shard\n\nhttps://ultros.app/retainers/undercuts#retainer-5"
        );
    }

    #[test]
    fn long_batches_are_truncated_with_a_count() {
        let batch: Vec<Undercut> = (100..115)
            .map(|id| undercut(id, vec![retainer(5, "Seeba", 1)]))
            .collect();
        let message = format_undercut_message(&batch, names).unwrap();
        let summary = message.body.lines().next().unwrap();
        assert!(
            summary.starts_with("Your retainers Seeba have been undercut on 15 items: Item 100, "),
            "{summary}"
        );
        assert!(summary.contains("Item 109, and 5 more"), "{summary}");
        assert!(!summary.contains("Item 110"), "{summary}");
    }

    #[test]
    fn unknown_items_are_skipped_and_an_all_unknown_batch_sends_nothing() {
        let batch = vec![
            undercut(-1, vec![retainer(5, "Seeba", 1)]),
            undercut(1, vec![retainer(5, "Seeba", 1)]),
        ];
        let message = format_undercut_message(&batch, names).unwrap();
        assert_eq!(message.item_id, 1);
        assert!(
            message
                .body
                .starts_with("Your retainers Seeba have been undercut on Fire Shard\n")
        );

        let unknown = vec![undercut(-1, vec![retainer(5, "Seeba", 1)])];
        assert_eq!(format_undercut_message(&unknown, names), None);
        assert_eq!(format_undercut_message(&[], names), None);
    }

    #[test]
    fn batch_without_any_retainer_falls_back_to_the_generic_page() {
        let batch = vec![undercut(1, vec![]), undercut(2, vec![])];
        let message = format_undercut_message(&batch, names).unwrap();
        assert_eq!(message.click_url, "/retainers/undercuts");
        assert!(
            message
                .body
                .ends_with("\n\nhttps://ultros.app/retainers/undercuts")
        );
    }

    // ---------- UndercutRetainer ordering ----------

    #[test]
    fn undercut_retainer_orders_by_struct_field_declaration_order() {
        // id < name < undercut_amount via derive(Ord) on tuple of fields.
        let mut v = [
            UndercutRetainer {
                id: 2,
                name: "A".into(),
                undercut_amount: 0,
            },
            UndercutRetainer {
                id: 1,
                name: "Z".into(),
                undercut_amount: 1000,
            },
        ];
        v.sort();
        assert_eq!(v[0].id, 1);
        assert_eq!(v[1].id, 2);
    }
}
