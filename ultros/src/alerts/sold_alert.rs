//! Single listener for every retainer-sold alert: feeds the shared
//! [`SoldMatcher`] from the listings and history buses and dispatches through
//! the common alert delivery pipeline.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::Result;
use chrono::Utc;
use poise::serenity_prelude;
use tracing::{error, info, warn};
use ultros_api_types::{
    user::OwnedRetainer,
    websocket::{ListingEventData, SaleEventData},
};
use ultros_db::{UltrosDb, entity::alert};

use crate::{
    alerts::{
        delivery::dispatch_alert,
        price_alert_tracker::resolve_item_name,
        sold_matcher::{
            AddedListing, ObservedSale, RemovedListing, SaleKey, SoldEvent, SoldMatcher,
        },
    },
    event::{BusRecv, EventBus, EventType, handle_bus_recv},
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct SaleRule {
    alert_id: i32,
}

/// `retainer_id -> alerts that own it`.
type RulesIndex = HashMap<i32, Vec<SaleRule>>;

/// `(alert_id, owned retainer ids)` pairs into the index.
fn build_rules_index(rows: Vec<(i32, Vec<i32>)>) -> RulesIndex {
    let mut index: RulesIndex = HashMap::new();
    for (alert_id, retainers) in rows {
        for retainer_id in retainers {
            index
                .entry(retainer_id)
                .or_default()
                .push(SaleRule { alert_id });
        }
    }
    index
}

fn owned_union(rules: &RulesIndex) -> HashSet<i32> {
    rules.keys().copied().collect()
}

async fn load_rules(db: &UltrosDb) -> Result<RulesIndex> {
    let alerts = db.get_all_active_retainer_sale_alerts().await?;
    let mut rows = Vec::with_capacity(alerts.len());
    for (alert, _) in alerts {
        let retainers = db.get_owned_retainer_ids(alert.owner).await?;
        rows.push((alert.id, retainers));
    }
    Ok(build_rules_index(rows))
}

/// `(title, body, click_url)` for a sold event.
pub(crate) fn format_sold_alert_message(
    event: &SoldEvent,
    item_name: &str,
) -> (String, String, String) {
    let hq = if event.key.hq { " (HQ)" } else { "" };
    let unit = format_gil(i64::from(event.key.price_per_unit));
    let total = format_gil(i64::from(event.key.price_per_unit) * i64::from(event.key.quantity));
    let title = format!("Retainer sale: {item_name}");
    let body = format!(
        "Your retainer {} sold {}× {item_name}{hq} for {unit} gil each ({total} gil total).\nhttps://ultros.app/retainers/listings",
        event.retainer_name, event.key.quantity,
    );
    (title, body, "/retainers/listings".to_string())
}

fn format_gil(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if value < 0 { format!("-{out}") } else { out }
}

pub(crate) struct RetainerSaleListener {
    #[allow(dead_code)]
    stop_tx: tokio::sync::mpsc::Sender<()>,
}

impl RetainerSaleListener {
    pub(crate) async fn start(
        db: UltrosDb,
        mut listings: EventBus<ListingEventData>,
        mut history: EventBus<SaleEventData>,
        mut retainers: EventBus<OwnedRetainer>,
        mut alert_events: EventBus<alert::Model>,
        ctx: serenity_prelude::Context,
    ) -> Result<Self> {
        let mut rules = load_rules(&db).await?;
        let mut matcher = SoldMatcher::new(owned_union(&rules));
        info!(
            "retainer sale listener started with {} alerts over {} retainers",
            rules
                .values()
                .flatten()
                .map(|r| r.alert_id)
                .collect::<HashSet<_>>()
                .len(),
            rules.len()
        );
        let (stop_tx, mut stop_rx) = tokio::sync::mpsc::channel::<()>(1);
        tokio::spawn(async move {
            // Matching happens here, not on arrival: see `SoldMatcher::settle`.
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(5));
            loop {
                tokio::select! {
                    _ = stop_rx.recv() => break,
                    _ = tick.tick() => {
                        let fired = matcher.settle(Utc::now());
                        fire_all(&db, &ctx, &rules, fired).await;
                    }
                    msg = alert_events.recv() => match handle_bus_recv("sale_alert.alerts", msg) {
                        BusRecv::Msg(_) | BusRecv::Lagged => refresh(&db, &mut rules, &mut matcher).await,
                        BusRecv::Closed => break,
                    },
                    msg = retainers.recv() => match handle_bus_recv("sale_alert.retainers", msg) {
                        BusRecv::Msg(_) | BusRecv::Lagged => refresh(&db, &mut rules, &mut matcher).await,
                        BusRecv::Closed => break,
                    },
                    msg = listings.recv() => match handle_bus_recv("sale_alert.listings", msg) {
                        BusRecv::Msg(event) => apply_listing_event(&mut matcher, &event),
                        // A dropped event is a missed sale, the accepted
                        // failure direction.
                        BusRecv::Lagged => {}
                        BusRecv::Closed => break,
                    },
                    msg = history.recv() => match handle_bus_recv("sale_alert.history", msg) {
                        BusRecv::Msg(event) => apply_sale_event(&mut matcher, &event),
                        BusRecv::Lagged => {}
                        BusRecv::Closed => break,
                    },
                }
            }
        });
        Ok(Self { stop_tx })
    }
}

async fn refresh(db: &UltrosDb, rules: &mut RulesIndex, matcher: &mut SoldMatcher) {
    match load_rules(db).await {
        Ok(new_rules) => {
            *rules = new_rules;
            matcher.set_owned(owned_union(rules));
        }
        Err(e) => error!("retainer sale listener failed to reload rules: {e}"),
    }
}

fn apply_listing_event(matcher: &mut SoldMatcher, event: &EventType<Arc<ListingEventData>>) {
    let now = Utc::now();
    match event {
        EventType::Remove(data) => {
            for (listing, retainer) in data.listings.iter() {
                matcher.on_removed(
                    RemovedListing {
                        key: SaleKey {
                            world_id: listing.world_id,
                            item_id: listing.item_id,
                            hq: listing.hq,
                            price_per_unit: listing.price_per_unit,
                            quantity: listing.quantity,
                        },
                        retainer_id: listing.retainer_id,
                        retainer_name: retainer.name.clone(),
                        listed_at: listing.timestamp,
                    },
                    now,
                );
            }
        }
        EventType::Add(data) => {
            for (listing, _) in data.listings.iter() {
                matcher.on_added(AddedListing {
                    world_id: listing.world_id,
                    item_id: listing.item_id,
                    hq: listing.hq,
                    quantity: listing.quantity,
                    retainer_id: listing.retainer_id,
                });
            }
        }
        EventType::Update(_) => {}
    }
}

fn apply_sale_event(matcher: &mut SoldMatcher, event: &EventType<Arc<SaleEventData>>) {
    let EventType::Add(data) = event else {
        return;
    };
    let now = Utc::now();
    for (sale, buyer) in data.sales.iter() {
        matcher.on_sale(
            ObservedSale {
                key: SaleKey {
                    world_id: sale.world_id,
                    item_id: sale.sold_item_id,
                    hq: sale.hq,
                    price_per_unit: sale.price_per_item,
                    quantity: sale.quantity,
                },
                sold_at: sale.sold_date,
                buyer_name: sale.buyer_name.clone().or_else(|| Some(buyer.name.clone())),
            },
            now,
        );
    }
}

async fn fire_all(
    db: &UltrosDb,
    ctx: &serenity_prelude::Context,
    rules: &RulesIndex,
    events: Vec<SoldEvent>,
) {
    for event in events {
        let Some(alerts) = rules.get(&event.retainer_id) else {
            continue;
        };
        let item_name = resolve_item_name(event.key.item_id);
        let (title, body, click_url) = format_sold_alert_message(&event, &item_name);
        for rule in alerts {
            let result = dispatch_alert(rule.alert_id, &title, &body, &click_url, db, ctx).await;
            let delivered = result.is_ok();
            let delivery_error = result.err().map(|e| e.to_string());
            if let Some(error) = &delivery_error {
                warn!(
                    alert_id = rule.alert_id,
                    "retainer sale alert not delivered: {error}"
                );
            }
            if let Err(e) = db
                .record_alert_event(
                    rule.alert_id,
                    event.key.item_id,
                    None,
                    Some(event.key.price_per_unit),
                    delivered,
                    delivery_error,
                )
                .await
            {
                error!(
                    "failed to record alert_event for sale alert {}: {e}",
                    rule.alert_id
                );
            }
            if delivered && let Err(e) = db.update_alert_last_fired(rule.alert_id).await {
                error!(
                    "failed to update last_fired_at for sale alert {}: {e}",
                    rule.alert_id
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn event(hq: bool) -> SoldEvent {
        SoldEvent {
            retainer_id: 9,
            retainer_name: "Moogle".into(),
            key: SaleKey {
                world_id: 34,
                item_id: 5,
                hq,
                price_per_unit: 1200,
                quantity: 3,
            },
            sold_at: NaiveDate::from_ymd_opt(2026, 9, 7)
                .unwrap()
                .and_hms_opt(1, 2, 3)
                .unwrap(),
            buyer_name: Some("Buyer Name".into()),
        }
    }

    #[test]
    fn message_names_retainer_item_quantity_and_unit_price() {
        let (title, body, click_url) = format_sold_alert_message(&event(false), "Fire Shard");
        assert_eq!(title, "Retainer sale: Fire Shard");
        assert!(
            body.starts_with(
                "Your retainer Moogle sold 3× Fire Shard for 1,200 gil each (3,600 gil total)."
            ),
            "{body}"
        );
        assert!(body.contains("https://ultros.app/retainers/listings"));
        assert_eq!(click_url, "/retainers/listings");
    }

    #[test]
    fn message_marks_hq() {
        let (_, body, _) = format_sold_alert_message(&event(true), "Fire Shard");
        assert!(body.contains("3× Fire Shard (HQ) for"), "{body}");
    }

    #[test]
    fn gil_is_grouped_in_thousands() {
        assert_eq!(format_gil(0), "0");
        assert_eq!(format_gil(999), "999");
        assert_eq!(format_gil(1000), "1,000");
        assert_eq!(format_gil(123_456_789), "123,456,789");
    }

    #[test]
    fn rules_index_maps_each_owned_retainer_to_its_alerts() {
        let rules = build_rules_index(vec![(10, vec![1, 2]), (11, vec![2])]);
        assert_eq!(rules.get(&1).map(Vec::len), Some(1));
        assert_eq!(rules.get(&2).map(Vec::len), Some(2));
        let expected: HashSet<i32> = [1, 2].into_iter().collect();
        assert_eq!(owned_union(&rules), expected);
    }
}
