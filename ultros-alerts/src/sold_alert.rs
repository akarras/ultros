//! Single listener for every retainer-sold alert: feeds the shared
//! [`SoldMatcher`] from the listings and history buses and dispatches through
//! the common alert delivery pipeline.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::Result;
use chrono::{DateTime, Utc};
use poise::serenity_prelude;
use tracing::{error, info, warn};
use ultros_api_types::{
    user::OwnedRetainer,
    websocket::{ListingEventData, SaleEventData},
};
use ultros_db::{UltrosDb, entity::alert};

use crate::{
    alerts::{
        delivery::{AlertKind, PushOptions, dispatch_alert},
        inbox::{AlertFire, record_fire},
        price_alert_tracker::{is_off_cooldown_at, resolve_item_name},
        sold_matcher::{
            AddedListing, ObservedSale, RemovedListing, SaleKey, SoldEvent, SoldMatcher,
        },
    },
    event::{BusRecv, EventBus, EventProducer, EventType, NotificationEvent, handle_bus_recv},
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct SaleAlert {
    owner: i64,
    cooldown_seconds: i32,
    last_fired_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Default)]
struct Rules {
    /// `retainer_id -> ids of the alerts that own it`.
    by_retainer: HashMap<i32, Vec<i32>>,
    alerts: HashMap<i32, SaleAlert>,
}

impl Rules {
    /// `(alert_id, alert, owned retainer ids)` rows into the index.
    fn build(rows: Vec<(i32, SaleAlert, Vec<i32>)>) -> Self {
        let mut rules = Rules::default();
        for (alert_id, alert, retainers) in rows {
            for retainer_id in retainers {
                rules
                    .by_retainer
                    .entry(retainer_id)
                    .or_default()
                    .push(alert_id);
            }
            rules.alerts.insert(alert_id, alert);
        }
        rules
    }

    fn owned_union(&self) -> HashSet<i32> {
        self.by_retainer.keys().copied().collect()
    }

    /// Keep the later of each alert's stored and in-memory `last_fired_at`.
    /// The listener records a fire in memory before the database write, and
    /// that write can fail; a reload must not reopen the cooldown.
    fn carry_last_fired(&mut self, previous: &Rules) {
        for (alert_id, alert) in &mut self.alerts {
            if let Some(prev) = previous.alerts.get(alert_id) {
                alert.last_fired_at = alert.last_fired_at.max(prev.last_fired_at);
            }
        }
    }
}

async fn load_rules(db: &UltrosDb) -> Result<Rules> {
    let alerts = db.get_all_active_retainer_sale_alerts().await?;
    let mut rows = Vec::with_capacity(alerts.len());
    for (alert, _) in alerts {
        let retainers = db.get_owned_retainer_ids(alert.owner).await?;
        rows.push((
            alert.id,
            SaleAlert {
                owner: alert.owner,
                cooldown_seconds: alert.cooldown_seconds,
                last_fired_at: alert.last_fired_at.map(|at| at.with_timezone(&Utc)),
            },
            retainers,
        ));
    }
    Ok(Rules::build(rows))
}

/// Sales matched to an alert but not sent yet, keyed by alert id.
///
/// A sale lands here and goes out on the next tick when its alert is off
/// cooldown. Otherwise it waits, and everything that piled up is sent as one
/// message when the cooldown ends. Dropping them instead would lose sales for
/// good: unlike an undercut, a sale never happens again.
type PendingSales = HashMap<i32, Vec<SoldEvent>>;

fn queue_sales(rules: &Rules, pending: &mut PendingSales, events: Vec<SoldEvent>) {
    for event in events {
        let Some(alert_ids) = rules.by_retainer.get(&event.retainer_id) else {
            continue;
        };
        for alert_id in alert_ids {
            pending.entry(*alert_id).or_default().push(event.clone());
        }
    }
}

/// Take the queued sales of every alert that is off cooldown at `now`, in
/// alert-id order. Sales of alerts that are gone (disabled or deleted) are
/// dropped with them.
fn take_ready(
    rules: &Rules,
    pending: &mut PendingSales,
    now: DateTime<Utc>,
) -> Vec<(i32, Vec<SoldEvent>)> {
    pending.retain(|alert_id, _| rules.alerts.contains_key(alert_id));
    let mut ready: Vec<i32> = pending
        .keys()
        .copied()
        .filter(|alert_id| {
            rules.alerts.get(alert_id).is_some_and(|alert| {
                is_off_cooldown_at(alert.last_fired_at, alert.cooldown_seconds, now)
            })
        })
        .collect();
    ready.sort_unstable();
    ready
        .into_iter()
        .filter_map(|alert_id| pending.remove(&alert_id).map(|sales| (alert_id, sales)))
        .collect()
}

/// `(title, body, click_url)` for a sold event.
pub fn format_sold_alert_message(event: &SoldEvent, item_name: &str) -> (String, String, String) {
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

/// Sales named in a digest before it falls back to "and N more".
const MAX_LISTED_SALES: usize = 10;

/// `(title, body, click_url)` for several sales sent as one message — what
/// piles up while an alert is on cooldown. One line before the link, since
/// the in-app inbox collapses newlines.
pub fn format_sold_digest_message(sales: &[(SoldEvent, String)]) -> (String, String, String) {
    let total: i64 = sales
        .iter()
        .map(|(event, _)| i64::from(event.key.price_per_unit) * i64::from(event.key.quantity))
        .sum();
    let mut listed: Vec<String> = sales
        .iter()
        .take(MAX_LISTED_SALES)
        .map(|(event, item_name)| {
            let hq = if event.key.hq { " (HQ)" } else { "" };
            let amount =
                format_gil(i64::from(event.key.price_per_unit) * i64::from(event.key.quantity));
            format!(
                "{}× {item_name}{hq} for {amount} gil ({})",
                event.key.quantity, event.retainer_name
            )
        })
        .collect();
    if sales.len() > MAX_LISTED_SALES {
        listed.push(format!("and {} more", sales.len() - MAX_LISTED_SALES));
    }
    let count = sales.len();
    let title = format!("Retainer sales: {count} sales");
    let body = format!(
        "Your retainers made {count} sales for {} gil: {}.\nhttps://ultros.app/retainers/listings",
        format_gil(total),
        listed.join(", ")
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

pub struct RetainerSaleListener {
    #[allow(dead_code)]
    stop_tx: tokio::sync::mpsc::Sender<()>,
}

impl RetainerSaleListener {
    pub async fn start(
        db: UltrosDb,
        mut listings: EventBus<ListingEventData>,
        mut history: EventBus<SaleEventData>,
        mut retainers: EventBus<OwnedRetainer>,
        mut alert_events: EventBus<alert::Model>,
        ctx: serenity_prelude::Context,
        notifications: EventProducer<NotificationEvent>,
    ) -> Result<Self> {
        let mut rules = load_rules(&db).await?;
        let mut matcher = SoldMatcher::new(rules.owned_union());
        let mut pending = PendingSales::new();
        info!(
            "retainer sale listener started with {} alerts over {} retainers",
            rules.alerts.len(),
            rules.by_retainer.len()
        );
        let (stop_tx, mut stop_rx) = tokio::sync::mpsc::channel::<()>(1);
        tokio::spawn(async move {
            // Matching happens here, not on arrival: see `SoldMatcher::settle`.
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(5));
            loop {
                tokio::select! {
                    _ = stop_rx.recv() => break,
                    _ = tick.tick() => {
                        let now = Utc::now();
                        queue_sales(&rules, &mut pending, matcher.settle(now));
                        for (alert_id, sales) in take_ready(&rules, &mut pending, now) {
                            if let Some(alert) = rules.alerts.get_mut(&alert_id)
                                && fire(&db, &ctx, alert_id, alert.owner, &sales, &notifications).await
                            {
                                alert.last_fired_at = Some(now);
                            }
                        }
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

async fn refresh(db: &UltrosDb, rules: &mut Rules, matcher: &mut SoldMatcher) {
    match load_rules(db).await {
        Ok(mut new_rules) => {
            new_rules.carry_last_fired(rules);
            *rules = new_rules;
            matcher.set_owned(rules.owned_union());
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

/// Send one alert's sales as a single message; true when it was delivered.
async fn fire(
    db: &UltrosDb,
    ctx: &serenity_prelude::Context,
    alert_id: i32,
    owner: i64,
    sales: &[SoldEvent],
    notifications: &EventProducer<NotificationEvent>,
) -> bool {
    let (first, matched_price, (title, body, click_url)) = match sales {
        [] => return false,
        [event] => (
            event,
            Some(event.key.price_per_unit),
            format_sold_alert_message(event, &resolve_item_name(event.key.item_id)),
        ),
        [first, ..] => {
            let named: Vec<(SoldEvent, String)> = sales
                .iter()
                .map(|event| (event.clone(), resolve_item_name(event.key.item_id)))
                .collect();
            (first, None, format_sold_digest_message(&named))
        }
    };
    let push = PushOptions::for_alert(AlertKind::Sold, alert_id, &click_url);
    let result = dispatch_alert(alert_id, &title, &body, &push, db, ctx).await;
    let delivered = result.is_ok();
    let delivery_error = result.err().map(|e| e.to_string());
    if let Some(error) = &delivery_error {
        warn!(alert_id, "retainer sale alert not delivered: {error}");
    }
    record_fire(
        db,
        notifications,
        AlertFire {
            alert_id,
            owner,
            item_id: first.key.item_id,
            matched_listing_id: None,
            matched_price,
            title: &title,
            body: &body,
            click_url: &click_url,
            delivered,
            delivery_error,
        },
    )
    .await;
    if delivered && let Err(e) = db.update_alert_last_fired(alert_id).await {
        error!("failed to update last_fired_at for sale alert {alert_id}: {e}");
    }
    delivered
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

    fn sale_alert(last_fired_at: Option<DateTime<Utc>>) -> SaleAlert {
        SaleAlert {
            owner: 100,
            cooldown_seconds: 3600,
            last_fired_at,
        }
    }

    fn at(hour: u32, minute: u32) -> DateTime<Utc> {
        NaiveDate::from_ymd_opt(2026, 9, 22)
            .unwrap()
            .and_hms_opt(hour, minute, 0)
            .unwrap()
            .and_utc()
    }

    #[test]
    fn rules_index_maps_each_owned_retainer_to_its_alerts() {
        let rules = Rules::build(vec![
            (10, sale_alert(None), vec![1, 2]),
            (11, sale_alert(None), vec![2]),
        ]);
        assert_eq!(rules.by_retainer.get(&1).map(Vec::len), Some(1));
        assert_eq!(rules.by_retainer.get(&2).map(Vec::len), Some(2));
        let expected: HashSet<i32> = [1, 2].into_iter().collect();
        assert_eq!(rules.owned_union(), expected);
    }

    #[test]
    fn sales_go_out_at_once_for_an_alert_off_cooldown() {
        let rules = Rules::build(vec![(10, sale_alert(None), vec![9])]);
        let mut pending = PendingSales::new();
        queue_sales(&rules, &mut pending, vec![event(false)]);
        let ready = take_ready(&rules, &mut pending, at(12, 0));
        assert_eq!(ready, vec![(10, vec![event(false)])]);
        assert!(pending.is_empty());
    }

    #[test]
    fn sales_wait_out_the_cooldown_and_go_out_together() {
        // Fired at 12:00 with a one-hour cooldown: two sales at 12:10 and
        // 12:40 are held, then sent as one batch once 13:00 passes.
        let rules = Rules::build(vec![(10, sale_alert(Some(at(12, 0))), vec![9])]);
        let mut pending = PendingSales::new();
        queue_sales(&rules, &mut pending, vec![event(false)]);
        assert!(take_ready(&rules, &mut pending, at(12, 10)).is_empty());
        queue_sales(&rules, &mut pending, vec![event(true)]);
        assert!(take_ready(&rules, &mut pending, at(12, 40)).is_empty());
        let ready = take_ready(&rules, &mut pending, at(13, 0));
        assert_eq!(ready, vec![(10, vec![event(false), event(true)])]);
        assert!(pending.is_empty());
    }

    #[test]
    fn cooldown_is_per_alert() {
        let rules = Rules::build(vec![
            (10, sale_alert(Some(at(12, 0))), vec![9]),
            (11, sale_alert(None), vec![9]),
        ]);
        let mut pending = PendingSales::new();
        queue_sales(&rules, &mut pending, vec![event(false)]);
        let ready = take_ready(&rules, &mut pending, at(12, 30));
        assert_eq!(ready, vec![(11, vec![event(false)])]);
        assert_eq!(pending.get(&10).map(Vec::len), Some(1));
    }

    #[test]
    fn sales_of_unwatched_retainers_are_ignored() {
        let rules = Rules::build(vec![(10, sale_alert(None), vec![1])]);
        let mut pending = PendingSales::new();
        queue_sales(&rules, &mut pending, vec![event(false)]);
        assert!(pending.is_empty());
    }

    #[test]
    fn held_sales_of_a_removed_alert_are_dropped() {
        let held = Rules::build(vec![(10, sale_alert(Some(at(12, 0))), vec![9])]);
        let mut pending = PendingSales::new();
        queue_sales(&held, &mut pending, vec![event(false)]);
        // The alert was disabled while its sales waited.
        let reloaded = Rules::default();
        assert!(take_ready(&reloaded, &mut pending, at(14, 0)).is_empty());
        assert!(pending.is_empty());
    }

    #[test]
    fn reload_keeps_the_later_last_fired_at() {
        // The database write after a fire failed: the reload must not forget
        // the in-memory fire and reopen the cooldown.
        let previous = Rules::build(vec![(10, sale_alert(Some(at(12, 30))), vec![9])]);
        let mut reloaded = Rules::build(vec![(10, sale_alert(Some(at(11, 0))), vec![9])]);
        reloaded.carry_last_fired(&previous);
        assert_eq!(reloaded.alerts[&10].last_fired_at, Some(at(12, 30)));

        let mut newer = Rules::build(vec![(10, sale_alert(Some(at(13, 0))), vec![9])]);
        newer.carry_last_fired(&previous);
        assert_eq!(newer.alerts[&10].last_fired_at, Some(at(13, 0)));
    }

    #[test]
    fn digest_lists_each_sale_with_its_retainer_and_the_total() {
        let sales = vec![
            (event(false), "Fire Shard".to_string()),
            (event(true), "Ice Shard".to_string()),
        ];
        let (title, body, click_url) = format_sold_digest_message(&sales);
        assert_eq!(title, "Retainer sales: 2 sales");
        assert_eq!(
            body,
            "Your retainers made 2 sales for 7,200 gil: 3× Fire Shard for 3,600 gil (Moogle), 3× Ice Shard (HQ) for 3,600 gil (Moogle).\nhttps://ultros.app/retainers/listings"
        );
        assert_eq!(click_url, "/retainers/listings");
    }

    #[test]
    fn long_digests_are_truncated_with_a_count() {
        let sales: Vec<(SoldEvent, String)> = (0..13)
            .map(|i| (event(false), format!("Item {i}")))
            .collect();
        let (_, body, _) = format_sold_digest_message(&sales);
        assert!(
            body.contains("Item 9 for 3,600 gil (Moogle), and 3 more."),
            "{body}"
        );
        assert!(!body.contains("Item 10 "), "{body}");
    }
}
