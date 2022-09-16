use std::{
    collections::{HashMap, HashSet},
    error,
};

use anyhow::Result;
use poise::serenity_prelude::{self, Color, UserId};
use tracing::{error, instrument};
use ultros_db::{entity::*, UltrosDb};

use crate::event::EventBus;

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
            let mut alerts = Box::pin(alerts.recv());
            let mut retainer_alert_events = Box::pin(undercuts.recv());
            match futures::future::select(alerts, retainer_alert_events).await {
                futures::future::Either::Left(alert) => todo!(),
                futures::future::Either::Right((retainer_alert_create, b)) => {
                    if let Ok(retainer) = &retainer_alert_create {
                        match retainer {
                            crate::event::EventType::Remove(remove) => todo!(),
                            crate::event::EventType::Add(retainer_alert) => {
                                manager
                                    .create_retainer_alert_listener(
                                        &retainer_alert,
                                        &ultros_db,
                                        &ctx,
                                        listings.resubscribe(),
                                        retainers.resubscribe(),
                                    )
                                    .await;
                            }
                            crate::event::EventType::Update(m) => {}
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
                error!("Error creating retainer alert listener");
                return;
            }
        };
        self.current_retainer_alerts.insert(*id, listener);
    }
}

struct RetainerAlertListener {
    retainer_alert_id: i32,
    cancellation_sender: tokio::sync::mpsc::Sender<RetainerAlertTx>,
}

async fn get_user_unique_retainer_ids_and_listing_ids_by_price(
    ultros_db: &UltrosDb,
    discord_user: u64,
) -> Result<(HashSet<i32>, HashMap<i32, i32>)> {
    // this might be better as a sql query
    let retainer_listings = ultros_db
        .get_retainer_listings_for_discord_user(discord_user)
        .await?;
    // get a list of what retainers and items the users have
    let user_retainer_ids: HashSet<i32> = retainer_listings.iter().map(|(r, _)| r.id).collect();
    // map item id -> min(price_per_unit)
    let user_lowest_listings: HashMap<i32, i32> = retainer_listings
        .into_iter()
        .flat_map(|(_, listings)| listings.into_iter().map(|l| (l.item_id, l.price_per_unit)))
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
            })
            .await?;
    }
    Ok(())
}

enum RetainerAlertTx {
    Stop,
    UpdateMargin(i32),
}

impl RetainerAlertListener {
    #[instrument(skip(ultros_db, listings, ctx))]
    pub(crate) async fn create_listener(
        retainer_alert_id: i32,
        alert_id: i32,
        mut margin: i32,
        ultros_db: UltrosDb,
        mut listings: EventBus<Vec<active_listing::Model>>,
        mut active_retainers: EventBus<retainer::Model>,
        ctx: serenity_prelude::Context,
    ) -> Result<Self> {
        let alert = ultros_db
            .get_alert(alert_id)
            .await?
            .ok_or(anyhow::Error::msg("Unable to find retainer"))?;
        let discord_user = alert.owner as u64;
        let (mut user_retainer_ids, mut user_lowest_listings) =
            get_user_unique_retainer_ids_and_listing_ids_by_price(&ultros_db, discord_user).await?;
        let (cancellation_sender, mut receiver) = tokio::sync::mpsc::channel::<RetainerAlertTx>(10);
        tokio::spawn(async move {
            loop {
                let ended =
                    futures::future::select(Box::pin(receiver.recv()), Box::pin(listings.recv()))
                        .await;
                match ended {
                    futures::future::Either::Left((msg, _)) => {
                        if let Some(msg) = msg {
                            match msg {
                                RetainerAlertTx::Stop => {
                                    break;
                                }
                                RetainerAlertTx::UpdateMargin(m) => {
                                    margin = m;
                                }
                            }
                        } else {
                            break;
                        }
                    }
                    futures::future::Either::Right((listing, _)) => {
                        let listing = match listing {
                            Ok(listing) => listing,
                            Err(e) => {
                                tracing::error!("Error receiving listing {e:?}");
                                break;
                            }
                        };
                        match listing {
                            crate::event::EventType::Remove(removed) => {
                                for removed in removed.iter() {
                                    // if we removed our listing, we need to refetch our pricing from the database if the listing was the lowest
                                    if user_retainer_ids.contains(&removed.retainer_id) {
                                        if let Some(value) =
                                            user_lowest_listings.get(&removed.item_id).copied()
                                        {
                                            if value == removed.price_per_unit {
                                                if let Ok((retainer_ids, listings)) = get_user_unique_retainer_ids_and_listing_ids_by_price(&ultros_db, discord_user).await {
                  user_retainer_ids = retainer_ids;
                  user_lowest_listings = listings;
                }
                                            }
                                        }
                                    }
                                }
                            }
                            crate::event::EventType::Add(added) => {
                                // items in an added vec should all be the same type, so lets just find the cheapest item
                                if let Some(added) = added.iter().min_by_key(|a| a.price_per_unit) {
                                    match (
                                        user_retainer_ids.contains(&added.retainer_id),
                                        user_lowest_listings.get(&added.item_id).copied(),
                                    ) {
                                        (true, None) => {
                                            // our listing, do the update thing
                                            let entry = user_lowest_listings
                                                .entry(added.item_id)
                                                .or_insert(added.price_per_unit);
                                            *entry = added.price_per_unit.min(*entry);
                                        }
                                        (false, Some(our_price)) => {
                                            // we have a listing, make sure they didn't just beat our price
                                            if (our_price as f64 * (1.0 - (margin as f32 / 100.0)))
                                                as i32
                                                > added.price_per_unit
                                            {
                                                // they beat our price, raise the alarms
                                                // get the name of the item
                                                let data = xiv_gen_db::decompress_data();
                                                let item_name = data
                                                    .items
                                                    .get(&xiv_gen::ItemId(added.item_id))
                                                    .map(|i| i.name.as_str())
                                                    .unwrap_or_default();
                                                // figure out what retainers have been undercut
                                                let retainers: Vec<_> = ultros_db
                                                    .get_retainer_listings_for_discord_user(
                                                        discord_user,
                                                    )
                                                    .await
                                                    .map(|i| {
                                                        i.into_iter()
                                                            .flat_map(|(r, listings)| {
                                                                (
                                                                    r,
                                                                    listings.iter().find(|i| {
                                                                        i.item_id == added.item_id
                                                                        && added.price_per_unit
                                                                            < (i.price_per_unit
                                                                                as f32
                                                                                * (1.0
                                                                                    - (margin
                                                                                        as f32
                                                                                        / 100.0)))
                                                                                as i32
                                                                    }),
                                                                )
                                                            })
                                                            .map(|(retainer, l)| {
                                                                (retainer.name, l.price_per_unit)
                                                            })
                                                            .collect::<Vec<_>>();
                                                    })
                                                    .unwrap_or_default();
                                                let undercut_msg = format!("<@{discord_user}> your retainers {retainers} have been undercut on {item_name}! Check your retainers!");
                                                let _ = send_discord_alerts(
                                                    alert_id,
                                                    discord_user,
                                                    &ultros_db,
                                                    &ctx,
                                                    &undercut_msg,
                                                )
                                                .await;
                                                break;
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            crate::event::EventType::Update(_) => {}
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
