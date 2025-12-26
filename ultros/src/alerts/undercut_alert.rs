use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::Result;
use futures::future::{self, Either};
use poise::serenity_prelude::{self, Color, UserId};
use tracing::{debug, error, instrument};
use ultros_api_types::{
    alerts::UndercutRetainer, user::OwnedRetainer, websocket::ListingEventData,
};
use ultros_db::UltrosDb;

use crate::event::{EventBus, EventType};

pub(crate) struct RetainerAlertListener {
    pub(crate) retainer_alert_id: i32,
    pub(crate) cancellation_sender: tokio::sync::mpsc::Sender<RetainerAlertTx>,
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
        let channel_id = serenity_prelude::ChannelId::new(destination.channel_id as u64);
        let _ = channel_id
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
            .await?;
    }
    Ok(())
}

pub(crate) enum RetainerAlertTx {
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

#[derive(Debug)]
pub(crate) struct Undercut {
    pub(crate) item_id: i32,
    pub(crate) undercut_retainers: Vec<UndercutRetainer>,
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

    // pub(crate) fn into_stream<E>(self, listing_events: E) -> impl Stream<Item = Undercut>
    // where
    //     E: Stream<Item = Result<EventType<Arc<ListingEventData>>, anyhow::Error>>,
    // {
    //     let value = Arc::new(Mutex::new(self));
    //     listing_events.filter_map(move |s| {
    //         let value = value.clone();
    //         async move {
    //             let value = value.clone();
    //             let mut lock = value.lock().await;
    //             lock.handle_listing_event(s).await.ok()?
    //         }
    //     })
    // }

    pub(crate) async fn handle_listing_event(
        &mut self,
        listings: Result<EventType<Arc<ListingEventData>>, anyhow::Error>,
    ) -> Result<Option<Undercut>, anyhow::Error> {
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
                        && let Ok((retainer_ids, listings)) =
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
                                .map(|i| {
                                    i.into_iter()
                                        .flat_map(|(_, r, listings)| {
                                            listings
                                                .iter()
                                                .find(|i| {
                                                    i.item_id == added.item_id
                                                        && i.hq == added.hq
                                                        && i.world_id == added.world_id
                                                        && added.price_per_unit
                                                            < (i.price_per_unit as f64
                                                                * (1.0
                                                                    - (self.margin as f64 / 100.0)))
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
                            return Ok(Some(Undercut {
                                item_id: added.item_id,
                                undercut_retainers: retainers,
                            }));
                        }
                    }
                }
            }
            EventType::Update(_) => {}
        }
        Ok(None)
    }
}

impl RetainerAlertListener {
    #[instrument(skip(ultros_db, listings, ctx))]
    pub(crate) async fn create_listener(
        retainer_alert_id: i32,
        alert_id: i32,
        margin: i32,
        ultros_db: UltrosDb,
        mut listings: EventBus<ListingEventData>,
        active_retainers: EventBus<OwnedRetainer>,
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
                            }
                            Ok(undercuts) => match undercuts {
                                None => {}
                                Some(Undercut {
                                    item_id,
                                    undercut_retainers,
                                }) => {
                                    let items = &xiv_gen_db::data().items;
                                    if let Some(item) = items.get(&xiv_gen::ItemId(item_id)) {
                                        let retainer_names = undercut_retainers
                                            .into_iter()
                                            .map(|r| r.name)
                                            .collect::<Vec<_>>()
                                            .join(", ");
                                        let item_name = &item.name;
                                        let undercut_msg = format!(
                                            "Your retainers {retainer_names} have been undercut on {item_name}\n\nhttps://ultros.app/retainers/undercuts"
                                        );
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
                                            break;
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
