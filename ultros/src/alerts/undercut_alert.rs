use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::Result;
use futures::future::{self, Either};
use poise::serenity_prelude::{self, Color, UserId};
use serde::Serialize;
use tracing::{debug, error, instrument};
use ultros_api_types::{user::OwnedRetainer, websocket::ListingEventData};
use ultros_db::UltrosDb;

use crate::event::{EventBus, EventType};

/// Returns true when `competitor_price` undercuts `our_lowest_price` by strictly more than
/// `margin_percent`%. A `margin_percent` of 0 fires on any undercut; values ≥ 100 collapse the
/// threshold to 0 or below and effectively disable the alert.
///
/// The check matches the legacy formula `(our_lowest * (1 - margin/100)) > competitor` so we can
/// preserve existing user-visible behavior. Computed in `f64` then truncated to `i32`.
pub(crate) fn is_undercut_by_more_than_margin(
    our_lowest_price: i32,
    competitor_price: i32,
    margin_percent: i32,
) -> bool {
    let factor = 1.0 - (margin_percent as f64 / 100.0);
    let threshold = (our_lowest_price as f64 * factor) as i32;
    threshold > competitor_price
}

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

#[derive(Debug, Eq, PartialEq, PartialOrd, Ord, Serialize)]
pub(crate) struct UndercutRetainer {
    pub(crate) id: i32,
    pub(crate) name: String,
    pub(crate) undercut_amount: i32,
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
                        return Ok(Some(Undercut {
                            item_id: added.item_id,
                            undercut_retainers: retainers,
                        }));
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

    // ---------- UndercutRetainer ordering ----------

    #[test]
    fn undercut_retainer_orders_by_struct_field_declaration_order() {
        // id < name < undercut_amount via derive(Ord) on tuple of fields.
        let mut v = vec![
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
