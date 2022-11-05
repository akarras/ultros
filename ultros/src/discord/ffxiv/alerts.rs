use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::Result;
use futures::future::{self, Either};
use poise::serenity_prelude::{self, Color, UserId};
use serde::Serialize;
use tracing::{debug, error, instrument};
use ultros_db::{entity::*, UltrosDb};

use crate::event::{EventBus, EventType};

pub(crate) struct AlertManager {
    /// Hashmap of the current retainer alerts where the id of the alert is the key
    current_retainer_alerts: HashMap<i32, RetainerAlertListener>,
}

impl AlertManager {
    pub(crate) async fn start_manager(
        ultros_db: UltrosDb,
        (retainers, listings): (
            EventBus<retainer::Model>,
            EventBus<Vec<active_listing::Model>>,
        ),
        (mut alerts, mut undercuts): (
            EventBus<alert::Model>,
            EventBus<alert_retainer_undercut::Model>,
        ),
        ctx: serenity_prelude::Context,
    ) {
        // start all alerts we know about from the db, then use the alert busses to monitor for new alerts being spawned
        let mut manager = AlertManager {
            current_retainer_alerts: HashMap::new(),
        };
        match ultros_db.get_all_alerts().await {
            Ok(all_alerts) => {
                for alert in all_alerts {
                    if let Ok(alert) = ultros_db
                        .get_retainer_alerts_for_related_alert_id(alert.id)
                        .await
                    {
                        for alert in alert {
                            manager
                                .create_retainer_alert_listener(
                                    &alert,
                                    &ultros_db,
                                    &ctx,
                                    listings.resubscribe(),
                                    retainers.resubscribe(),
                                )
                                .await;
                        }
                    }
                }
            }
            Err(e) => error!("Error creating all alerts {e:?}"),
        }
        loop {
            let alerts = Box::pin(alerts.recv());
            let retainer_alert_events = Box::pin(undercuts.recv());
            match future::select(alerts, retainer_alert_events).await {
                Either::Left(_alert) => {
                    /*if let Ok(alert) = alert {
                        manager.remove_retainer_alert(alert);
                    }*/
                }
                Either::Right((retainer_alert_create, _)) => {
                    if let Ok(retainer) = &retainer_alert_create {
                        match retainer {
                            EventType::Remove(removed) => {
                                manager.remove_retainer_alert(removed).await;
                            }
                            EventType::Add(retainer_alert) => {
                                manager
                                    .create_retainer_alert_listener(
                                        retainer_alert,
                                        &ultros_db,
                                        &ctx,
                                        listings.resubscribe(),
                                        retainers.resubscribe(),
                                    )
                                    .await;
                            }
                            EventType::Update(m) => {
                                manager.update_alert(m, m.margin_percent).await;
                            }
                        }
                    }
                }
            }
        }
    }

    async fn create_retainer_alert_listener(
        &mut self,
        alert: &alert_retainer_undercut::Model,
        ultros_db: &UltrosDb,
        ctx: &serenity_prelude::Context,
        listings: EventBus<Vec<active_listing::Model>>,
        active_retainers: EventBus<retainer::Model>,
    ) {
        let alert_retainer_undercut::Model {
            id,
            alert_id,
            margin_percent,
        } = alert;
        let listener = match RetainerAlertListener::create_listener(
            *id,
            *alert_id,
            *margin_percent,
            ultros_db.clone(),
            listings,
            active_retainers,
            ctx.clone(),
        )
        .await
        {
            Ok(l) => l,
            Err(e) => {
                error!("Error creating retainer alert listener {e}");
                return;
            }
        };
        self.current_retainer_alerts.insert(*id, listener);
    }

    async fn remove_retainer_alert(&mut self, alert: &alert_retainer_undercut::Model) {
        if let Some(listener) = self.current_retainer_alerts.remove(&alert.id) {
            let _ = listener
                .cancellation_sender
                .send(RetainerAlertTx::Stop)
                .await;
        }
    }

    async fn update_alert(&self, alert: &alert_retainer_undercut::Model, margin: i32) {
        if let Some(listener) = self.current_retainer_alerts.get(&alert.id) {
            let _ = listener
                .cancellation_sender
                .send(RetainerAlertTx::UpdateMargin(margin));
        }
    }
}

struct RetainerAlertListener {
    retainer_alert_id: i32,
    cancellation_sender: tokio::sync::mpsc::Sender<RetainerAlertTx>,
}

async fn get_user_unique_retainer_ids_and_listing_ids_by_price(
    ultros_db: &UltrosDb,
    discord_user: u64,
) -> Result<(HashSet<i32>, HashMap<ListingKey, ListingValue>)> {
    // this might be better as a sql query
    let (_, retainer_listings) = ultros_db
        .get_retainer_listings_for_discord_user(discord_user)
        .await?;
    // get a list of what retainers and items the users have
    let user_retainer_ids: HashSet<i32> = retainer_listings.iter().map(|(r, _)| r.id).collect();
    // map item id -> min(price_per_unit)
    let user_lowest_listings: HashMap<_, _> = retainer_listings
        .into_iter()
        .flat_map(|(_, listings)| {
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

#[instrument(skip(ultros_db, ctx))]
async fn send_discord_alerts(
    alert_id: i32,
    discord_user_id: u64,
    ultros_db: &UltrosDb,
    ctx: &serenity_prelude::Context,
    undercut_msg: &str,
) -> Result<()> {
    let destinations = ultros_db.get_alert_discord_destinations(alert_id).await?;
    for destination in &destinations {
        let channel_id = serenity_prelude::ChannelId(destination.channel_id as u64);
        let _ = channel_id
            .send_message(ctx, |msg| {
                msg.embed(|e| {
                    e.color(Color::from_rgb(255, 0, 0))
                        .title("🔔😔 Undercut Alert")
                        .description(&undercut_msg)
                })
                .allowed_mentions(|mentions| mentions.users([UserId(discord_user_id)]))
                .content(format!("<@{discord_user_id}>"))
            })
            .await?;
    }
    Ok(())
}

enum RetainerAlertTx {
    Stop,
    UpdateMargin(i32),
}

#[derive(Debug, Hash, Eq, PartialEq, PartialOrd, Ord, Copy, Clone)]
struct ListingKey {
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
pub(crate) struct UndercutRetainer {
    pub(crate) id: i32,
    pub(crate) name: String,
    pub(crate) undercut_amount: i32,
}

#[derive(Debug)]
pub(crate) enum UndercutResult {
    None,
    Undercut {
        item_id: i32,
        undercut_retainers: Vec<UndercutRetainer>,
    },
}

#[derive(Debug)]
pub(crate) struct UndercutTracker {
    retainer_ids: HashSet<i32>,
    user_lowest_listings: HashMap<ListingKey, ListingValue>,
    discord_user_id: u64,
    margin: i32,
    db: UltrosDb,
}

impl UndercutTracker {
    pub(crate) async fn new(
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

    pub(crate) async fn handle_listing_event(
        &mut self,
        listings: Result<EventType<Arc<Vec<active_listing::Model>>>, anyhow::Error>,
    ) -> Result<UndercutResult, anyhow::Error> {
        let listing = listings?;
        match listing {
            EventType::Remove(removed) => {
                for removed in removed.iter() {
                    // if we removed our listing, we need to refetch our pricing from the database if the listing was the lowest
                    if self.retainer_ids.contains(&removed.retainer_id) {
                        if let Some(value) = self
                            .user_lowest_listings
                            .get(&ListingKey {
                                item_id: removed.item_id,
                                world_id: removed.world_id,
                                hq: removed.hq,
                            })
                            .filter(|v| v.lowest_price >= removed.price_per_unit)
                            .copied()
                        {
                            if value.lowest_price >= removed.price_per_unit {
                                if let Ok((retainer_ids, listings)) =
                                    get_user_unique_retainer_ids_and_listing_ids_by_price(
                                        &self.db,
                                        self.discord_user_id,
                                    )
                                    .await
                                {
                                    self.retainer_ids = retainer_ids;
                                    self.user_lowest_listings = listings;
                                }
                            }
                        }
                    }
                }
            }
            EventType::Add(added) => {
                // update our own data from the added list
                if let Some(retainer_listing) = added
                    .iter()
                    .filter(|added| self.retainer_ids.contains(&added.retainer_id))
                    .min_by_key(|i| i.price_per_unit)
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
                if let Some(added) = added.iter().min_by_key(|a| a.price_per_unit) {
                    if let Some(our_price) = self.user_lowest_listings.get_mut(&ListingKey {
                        item_id: added.item_id,
                        world_id: added.world_id,
                        hq: added.hq,
                    }) {
                        let margin_price =
                            our_price.lowest_price as f64 * (1.0 - (self.margin as f64 / 100.0));
                        debug!("comparing our_price {our_price:?} {margin_price} {added:?}");
                        // we have a listing, make sure they didn't just beat our price
                        if margin_price as i32 > added.price_per_unit {
                            // they beat our price, raise the alarms
                            if !our_price.has_alerted {
                                our_price.has_alerted = true;
                                // figure out what retainers have been undercut
                                let retainers = self
                                    .db
                                    .get_retainer_listings_for_discord_user(self.discord_user_id)
                                    .await
                                    .map(|(_o, i)| {
                                        i.into_iter()
                                            .flat_map(|(r, listings)| {
                                                listings
                                                    .iter()
                                                    .find(|i| {
                                                        i.item_id == added.item_id
                                                            && i.hq == added.hq
                                                            && i.world_id == added.world_id
                                                            && added.price_per_unit
                                                                < (i.price_per_unit as f64
                                                                    * (1.0
                                                                        - (self.margin as f64
                                                                            / 100.0)))
                                                                    as i32
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
                                return Ok(UndercutResult::Undercut {
                                    item_id: added.item_id,
                                    undercut_retainers: retainers,
                                });
                            }
                        }
                    }
                }
            }
            EventType::Update(_) => {}
        }
        Ok(UndercutResult::None)
    }
}

impl RetainerAlertListener {
    #[instrument(skip(ultros_db, listings, ctx))]
    pub(crate) async fn create_listener(
        retainer_alert_id: i32,
        alert_id: i32,
        margin: i32,
        ultros_db: UltrosDb,
        mut listings: EventBus<Vec<active_listing::Model>>,
        active_retainers: EventBus<retainer::Model>,
        ctx: serenity_prelude::Context,
    ) -> Result<Self> {
        let alert = ultros_db
            .get_alert(alert_id)
            .await?
            .ok_or_else(|| anyhow::Error::msg("Unable to find retainer"))?;
        let discord_user = alert.owner as u64;

        let (cancellation_sender, mut receiver) = tokio::sync::mpsc::channel::<RetainerAlertTx>(10);
        let mut undercut_tracker = UndercutTracker::new(discord_user, &ultros_db, margin).await?;
        tokio::spawn(async move {
            loop {
                let ended =
                    future::select(Box::pin(receiver.recv()), Box::pin(listings.recv())).await;
                match ended {
                    Either::Left((msg, _)) => {
                        if let Some(msg) = msg {
                            match msg {
                                RetainerAlertTx::Stop => {
                                    break;
                                }
                                RetainerAlertTx::UpdateMargin(m) => {
                                    undercut_tracker.margin = m;
                                }
                            }
                        } else {
                            break;
                        }
                    }
                    Either::Right((listing, _)) => {
                        match undercut_tracker
                            .handle_listing_event(listing.map_err(|e| e.into()))
                            .await
                        {
                            Err(e) => {
                                error!("{e:?}");
                                break;
                            }
                            Ok(undercuts) => match undercuts {
                                UndercutResult::None => {}
                                UndercutResult::Undercut {
                                    item_id,
                                    undercut_retainers,
                                } => {
                                    let items = &xiv_gen_db::decompress_data().items;
                                    if let Some(item) = items.get(&xiv_gen::ItemId(item_id)) {
                                        let retainer_names = undercut_retainers
                                            .into_iter()
                                            .map(|r| r.name)
                                            .collect::<Vec<_>>()
                                            .join(", ");
                                        let item_name = &item.name;
                                        let undercut_msg = format!("Your retainers {retainer_names} have been undercut on {item_name}");
                                        if let Err(e) = send_discord_alerts(
                                            alert_id,
                                            discord_user,
                                            &ultros_db,
                                            &ctx,
                                            &undercut_msg,
                                        )
                                        .await
                                        {
                                            error!("Error sending discord alerts {e}");
                                        }
                                    }
                                }
                            },
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
