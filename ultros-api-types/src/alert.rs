use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ActiveListing;
use crate::item_stats::ItemStatsVariant;
use crate::trends::ConfidenceBand;
use crate::world_helper::{AnySelector, WorldHelper};

/// What kind of condition the alert checks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AlertTrigger {
    /// Fire when any listing for this item drops to or below `price_threshold`.
    BelowThreshold {
        item_id: i32,
        world_selector: AnySelector,
        price_threshold: i32,
        hq_only: bool,
    },
    /// Fire when any item in the referenced list drops to or below that item's
    /// per-row `target_price`. The list-scoped trigger lets a single alert
    /// follow every item in a shopping list (including items added later).
    ListItemThreshold { list_id: i32 },
    /// Fire when one of the user's retainers is undercut by more than
    /// `margin_percent`.
    RetainerUndercut { margin_percent: i32 },
    /// Fire when one of the user's retainers' listings is inferred to have
    /// sold (a removal paired with a matching sale). No parameters.
    RetainerSold {},
    /// Fire when a list or one of its rows changes.
    ListUpdate { list_id: i32 },
    /// Fire when a listing for this item is at least `percent_below`% under
    /// the item's 30-day median for the listing's own quality, in the
    /// `world_selector` scope. See [`below_median_matches`].
    BelowMedian {
        item_id: i32,
        world_selector: AnySelector,
        percent_below: i32,
        hq_only: bool,
    },
    /// Fire when `world_selector` had no listings of this item (HQ listings
    /// only, when `hq_only`) for at least [`BACK_IN_STOCK_MIN_EMPTY_SECS`] and
    /// one appears.
    BackInStock {
        item_id: i32,
        world_selector: AnySelector,
        hq_only: bool,
    },
}

/// Where to send a fired alert.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum AlertDelivery {
    /// Send a Discord DM to the user. The user_id is derived from the auth session, not the request body.
    DiscordDm,
    /// POST a Discord-shaped embed to a user-provided webhook URL (typically a Discord channel webhook).
    Webhook { url: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateAlertRequest {
    pub trigger: AlertTrigger,
    /// Deprecated for new clients; if `endpoint_ids` is empty an endpoint is created from this.
    /// Kept for backward compatibility with existing client-side code until the drawer is migrated.
    #[serde(default)]
    pub delivery: Option<AlertDelivery>,
    /// Endpoints to attach to this alert. Required if `delivery` is None.
    #[serde(default)]
    pub endpoint_ids: Vec<i32>,
    /// Defaults to 3600 (1 hour) if omitted.
    pub cooldown_seconds: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateAlertRequest {
    pub enabled: Option<bool>,
    pub price_threshold: Option<i32>,
    pub endpoint_ids: Option<Vec<i32>>,
    pub cooldown_seconds: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Alert {
    pub id: i32,
    pub trigger: AlertTrigger,
    /// Deprecated; reflects the first endpoint's method for old clients. New clients should use `endpoint_ids`.
    pub delivery: AlertDelivery,
    pub endpoint_ids: Vec<i32>,
    pub enabled: bool,
    pub cooldown_seconds: i32,
    pub last_fired_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertEvent {
    pub id: i64,
    pub alert_id: i32,
    pub fired_at: chrono::DateTime<chrono::Utc>,
    pub item_id: i32,
    pub matched_listing_id: Option<i64>,
    pub matched_price: Option<i32>,
    pub delivered: bool,
    pub delivery_error: Option<String>,
    /// When the user marked this event as read. `None` means unread.
    /// Additive field: defaults to `None` when deserializing payloads from
    /// older servers.
    #[serde(default)]
    pub read_at: Option<DateTime<Utc>>,
    /// Notification-inbox display title. Additive field: defaults to `None`
    /// when deserializing payloads from older servers.
    #[serde(default)]
    pub title: Option<String>,
    /// Notification-inbox display body. Additive field: defaults to `None`
    /// when deserializing payloads from older servers.
    #[serde(default)]
    pub body: Option<String>,
    /// Where clicking the notification should navigate to. Additive field:
    /// defaults to `None` when deserializing payloads from older servers.
    #[serde(default)]
    pub click_url: Option<String>,
}

/// Body for `POST /api/v1/alerts/events/read`. Marks either an explicit set of
/// event ids, or every event with `id <= up_to_id`, as read. Both fields
/// default so a caller can supply just one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkAlertEventsReadRequest {
    #[serde(default)]
    pub ids: Vec<i64>,
    #[serde(default)]
    pub up_to_id: Option<i64>,
}

/// Response for `POST /api/v1/alerts/events/read`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkAlertEventsReadResponse {
    pub updated: u64,
    pub unread_count: u64,
}

/// Body for `POST /api/v1/alerts/events/clear`. Deletes the caller's alert
/// events — every one with `id <= up_to_id` when given, otherwise all of
/// them. The inbox always sends the newest id it has rendered, so an event
/// that fires between the user seeing the list and clicking "Clear" survives
/// instead of being silently discarded unseen.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClearAlertEventsRequest {
    #[serde(default)]
    pub up_to_id: Option<i64>,
}

/// Response for `POST /api/v1/alerts/events/clear`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClearAlertEventsResponse {
    pub deleted: u64,
}

/// Response for `GET /api/v1/alerts/events/unread_count`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnreadAlertEventCount {
    pub unread: u64,
}

/// Delivery channel for a notification endpoint. Mirrors the `method` discriminator
/// stored in the `notification_endpoint.method` DB column.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "PascalCase")]
pub enum EndpointMethod {
    DiscordDm {
        user_id: i64,
    },
    DiscordChannel {
        channel_id: i64,
        /// Resolved channel name (e.g. "general"). Populated by the server when the
        /// endpoint is created via the live serenity context. `None` for legacy rows
        /// that were created before name resolution was added — clients should fall
        /// back to displaying the channel id in that case.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        channel_name: Option<String>,
        /// Discord guild that owns this channel. Populated alongside `channel_name`.
        /// Used by the frontend to show the guild context and by the server to scope
        /// the admin check on update operations.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        guild_id: Option<i64>,
        /// Resolved guild name (e.g. "My Free Company"). Populated alongside
        /// `channel_name`. Display-only — never trusted for permission checks.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        guild_name: Option<String>,
    },
    Webhook {
        url: String,
    },
    /// Browser-side push subscription, owned by a row in `push_subscription`.
    /// Created via `POST /api/v1/push/subscribe`, not the generic endpoints CRUD.
    WebPush {
        subscription_id: i32,
    },
    /// The notification inbox itself (delivered over the websocket + `GET
    /// /api/v1/alerts/events`, not an external channel). Every user gets one
    /// auto-created `InApp` endpoint; it is not created via the generic
    /// endpoints CRUD.
    InApp {},
}

/// Body for `POST /api/v1/push/subscribe`. The browser obtains `endpoint`, `p256dh`,
/// and `auth` from the result of `PushManager.subscribe(...)`; `user_agent` is
/// `navigator.userAgent` (best-effort, used only for the endpoint label).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatePushSubscriptionRequest {
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
    pub user_agent: Option<String>,
}

/// Response shape for `GET /api/v1/push/vapid-public-key`. Single field so the
/// frontend doesn't have to special-case a bare string.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VapidPublicKey {
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    pub id: i32,
    pub name: String,
    #[serde(flatten)]
    pub method: EndpointMethod,
    /// Set when delivery hit a permanent failure (Discord channel deleted, bot
    /// removed) and alerts have stopped being sent here. Carries the reason
    /// Discord gave so the UI can explain it. `None` means the endpoint is
    /// healthy.
    ///
    /// Defaults on deserialize so an older client/server pair doesn't break on
    /// the added field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateEndpointRequest {
    pub name: String,
    #[serde(flatten)]
    pub method: EndpointMethod,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateEndpointRequest {
    pub name: Option<String>,
    pub method: Option<EndpointMethod>,
}

/// Response for `DELETE /api/v1/endpoints/{id}`.
///
/// When the deleted endpoint was `WebPush`, `push_endpoint` carries the
/// push-service URL of the subscription that was removed alongside it. The
/// browser that owns that subscription compares it against its own
/// `PushSubscription.endpoint` and calls `pushManager.unsubscribe()` on a
/// match; every other browser sees a URL that isn't theirs and leaves its own
/// subscription alone. `None` for all other endpoint methods.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteEndpointResponse {
    pub push_endpoint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResendResult {
    pub delivered: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscordWritableChannel {
    pub id: i64,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscordWritableGuild {
    pub id: i64,
    pub name: String,
    pub icon_url: Option<String>,
    pub channels: Vec<DiscordWritableChannel>,
}

/// A resolved item-price-threshold alert rule, shaped for pure matching against
/// a single [`ActiveListing`] via [`threshold_listing_matches`]. Shared between
/// the server (`ultros-alerts/src/price_alert_tracker.rs`) and the browser, so a
/// guest without an account can evaluate the same rule locally.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThresholdRule {
    pub item_id: i32,
    pub world_selector: AnySelector,
    pub price_threshold: i32,
    pub hq_only: bool,
    pub cooldown_seconds: i32,
    pub last_fired_at: Option<DateTime<Utc>>,
}

/// True when an alert with the given `last_fired_at` is free to fire again given
/// `cooldown_seconds` as of the reference timestamp `now`. `None` (never fired)
/// is always off cooldown.
pub fn is_off_cooldown_at(
    last_fired_at: Option<DateTime<Utc>>,
    cooldown_seconds: i32,
    now: DateTime<Utc>,
) -> bool {
    match last_fired_at {
        None => true,
        Some(t) => now.signed_duration_since(t).num_seconds() >= cooldown_seconds as i64,
    }
}

/// Returns true if `listing` satisfies every condition of `rule` and the rule is
/// off cooldown at `now`. Pure: no DB calls, no `Utc::now()` — so it can run
/// identically on the server and in the browser (e.g. for guest price alerts).
///
/// Check order: item id match, then world containment (resolved through
/// `worlds`), then HQ-only, then price, then cooldown. Unlike
/// `FilterPredicate::World`'s fail-open default, an unresolvable
/// `listing.world_id` or `rule.world_selector` never fires — deliberately
/// matching the server's existing `rule_matches_listing` semantics.
pub fn threshold_listing_matches(
    rule: &ThresholdRule,
    listing: &ActiveListing,
    worlds: &WorldHelper,
    now: DateTime<Utc>,
) -> bool {
    if rule.item_id != listing.item_id {
        return false;
    }
    if !world_in_scope(listing.world_id, rule.world_selector, worlds) {
        return false;
    }
    if rule.hq_only && !listing.hq {
        return false;
    }
    if listing.price_per_unit > rule.price_threshold {
        return false;
    }
    is_off_cooldown_at(rule.last_fired_at, rule.cooldown_seconds, now)
}

/// True when `world_id` lies inside `selector`. An unresolvable world or
/// selector is never in scope — alerts fail closed rather than firing on a
/// world they can't place.
pub fn world_in_scope(world_id: i32, selector: AnySelector, worlds: &WorldHelper) -> bool {
    match (
        worlds.lookup_selector(AnySelector::World(world_id)),
        worlds.lookup_selector(selector),
    ) {
        (Some(world), Some(scope)) => world.is_in(&scope),
        _ => false,
    }
}

/// Minimum cleaned 30-day sample a median needs before a below-median alert
/// will act on it. Below this the median is one or two trades, not a market.
pub const BELOW_MEDIAN_MIN_SAMPLES: u32 = 10;

/// Accepted `percent_below` values. Under 5% is ordinary price noise; 100% or
/// more could never match a positive price.
pub const BELOW_MEDIAN_PERCENT_RANGE: std::ops::RangeInclusive<i32> = 5..=90;

/// How long a back-in-stock scope must have been empty before a new listing
/// counts as a restock. Absorbs Universalis reprices (a remove + add of the
/// same listing, which momentarily empties a one-listing board) and the
/// remove/add reordering between independently persisted websocket events.
pub const BACK_IN_STOCK_MIN_EMPTY_SECS: i64 = 600;

/// True when a 30-day baseline is trustworthy enough to alert against: a
/// positive median over at least [`BELOW_MEDIAN_MIN_SAMPLES`] cleaned sales,
/// and not a band the quality scorer marked `unusable`. A zero MAD is fine on
/// its own — a large sample all at one price is a real median, and a small one
/// is already caught by the sample floor. `Unknown` (no quality row yet) is
/// allowed; only an explicit `Unusable` verdict disqualifies.
pub fn baseline_is_usable(baseline: &ItemStatsVariant) -> bool {
    baseline.p50_30d > 0
        && baseline.cleaned_sample_size_30d >= BELOW_MEDIAN_MIN_SAMPLES
        && baseline.confidence_band != ConfidenceBand::Unusable
}

/// The NQ and HQ 30-day baselines for one (item, scope). Unlike
/// [`crate::item_stats::ItemStatsResponse::variant_for`] there is no fallback
/// between qualities: an HQ listing measured against an NQ median would look
/// artificially expensive, and the reverse artificially cheap.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QualityBaselines {
    pub nq: Option<ItemStatsVariant>,
    pub hq: Option<ItemStatsVariant>,
}

impl QualityBaselines {
    pub fn from_variants(variants: impl IntoIterator<Item = ItemStatsVariant>) -> Self {
        let mut out = Self::default();
        for v in variants {
            if v.hq {
                out.hq = Some(v);
            } else {
                out.nq = Some(v);
            }
        }
        out
    }

    pub fn for_quality(&self, hq: bool) -> Option<&ItemStatsVariant> {
        if hq {
            self.hq.as_ref()
        } else {
            self.nq.as_ref()
        }
    }
}

/// A resolved below-median alert rule, shaped for pure matching via
/// [`below_median_matches`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MedianRule {
    pub item_id: i32,
    pub world_selector: AnySelector,
    pub percent_below: i32,
    pub hq_only: bool,
    pub cooldown_seconds: i32,
    pub last_fired_at: Option<DateTime<Utc>>,
}

/// True when `price_per_unit` is at least `percent_below`% under `median`.
/// Integer math, boundary inclusive: 70 against a 100 median at 30% matches.
pub fn is_below_median(price_per_unit: i32, median: u32, percent_below: i32) -> bool {
    price_per_unit > 0
        && (price_per_unit as i64) * 100 <= (median as i64) * (100 - percent_below as i64)
}

/// Returns true if `listing` satisfies `rule` against `baselines` and the rule
/// is off cooldown at `now`. Pure, like [`threshold_listing_matches`].
///
/// The listing is compared with the median of its own quality; `hq_only`
/// skips NQ listings. A missing or unusable baseline (see
/// [`baseline_is_usable`]) never matches.
pub fn below_median_matches(
    rule: &MedianRule,
    listing: &ActiveListing,
    baselines: &QualityBaselines,
    worlds: &WorldHelper,
    now: DateTime<Utc>,
) -> bool {
    if rule.item_id != listing.item_id {
        return false;
    }
    if rule.hq_only && !listing.hq {
        return false;
    }
    if !world_in_scope(listing.world_id, rule.world_selector, worlds) {
        return false;
    }
    let Some(baseline) = baselines.for_quality(listing.hq) else {
        return false;
    };
    if !baseline_is_usable(baseline) {
        return false;
    }
    if !is_below_median(listing.price_per_unit, baseline.p50_30d, rule.percent_below) {
        return false;
    }
    is_off_cooldown_at(rule.last_fired_at, rule.cooldown_seconds, now)
}

#[cfg(test)]
mod threshold_tests {
    use super::*;
    use crate::world::{Datacenter, Region, World, WorldData};

    fn helper() -> WorldHelper {
        WorldData {
            regions: vec![Region {
                id: 1,
                name: "NA".into(),
                datacenters: vec![Datacenter {
                    id: 10,
                    name: "Aether".into(),
                    region_id: 1,
                    worlds: vec![
                        World {
                            id: 100,
                            name: "Adamantoise".into(),
                            datacenter_id: 10,
                        },
                        World {
                            id: 101,
                            name: "Cactuar".into(),
                            datacenter_id: 10,
                        },
                    ],
                }],
            }],
        }
        .into()
    }

    fn listing(world_id: i32, item_id: i32, price: i32, hq: bool) -> ActiveListing {
        ActiveListing {
            id: 1,
            world_id,
            item_id,
            retainer_id: 7,
            price_per_unit: price,
            quantity: 1,
            hq,
            timestamp: chrono::NaiveDateTime::default(),
        }
    }

    fn rule(world_selector: AnySelector, price_threshold: i32, hq_only: bool) -> ThresholdRule {
        ThresholdRule {
            item_id: 42,
            world_selector,
            price_threshold,
            hq_only,
            cooldown_seconds: 3600,
            last_fired_at: None,
        }
    }

    #[test]
    fn threshold_matches_world_inside_datacenter_scope() {
        let h = helper();
        let r = rule(AnySelector::Datacenter(10), 100, false);
        let l = listing(101, 42, 50, false);
        assert!(threshold_listing_matches(&r, &l, &h, Utc::now()));
    }

    #[test]
    fn threshold_rejects_world_outside_scope() {
        let h = helper();
        let r = rule(AnySelector::World(100), 100, false);
        let l = listing(101, 42, 50, false);
        assert!(!threshold_listing_matches(&r, &l, &h, Utc::now()));
    }

    #[test]
    fn threshold_rejects_unknown_world_or_selector() {
        let h = helper();
        // Unknown listing world.
        let r = rule(AnySelector::World(100), 100, false);
        let l = listing(9999, 42, 50, false);
        assert!(!threshold_listing_matches(&r, &l, &h, Utc::now()));
        // Unknown rule selector.
        let r = rule(AnySelector::World(9999), 100, false);
        let l = listing(100, 42, 50, false);
        assert!(!threshold_listing_matches(&r, &l, &h, Utc::now()));
    }

    #[test]
    fn threshold_hq_only_rejects_nq() {
        let h = helper();
        let r = rule(AnySelector::World(100), 100, true);
        let l = listing(100, 42, 50, false);
        assert!(!threshold_listing_matches(&r, &l, &h, Utc::now()));
        let l_hq = listing(100, 42, 50, true);
        assert!(threshold_listing_matches(&r, &l_hq, &h, Utc::now()));
    }

    #[test]
    fn threshold_nq_rule_accepts_hq_and_nq_listings() {
        let h = helper();
        let r = rule(AnySelector::World(100), 100, false);
        let l_hq = listing(100, 42, 50, true);
        let l_nq = listing(100, 42, 50, false);
        assert!(threshold_listing_matches(&r, &l_hq, &h, Utc::now()));
        assert!(threshold_listing_matches(&r, &l_nq, &h, Utc::now()));
    }

    #[test]
    fn threshold_matches_at_exact_price() {
        let h = helper();
        let r = rule(AnySelector::World(100), 100, false);
        let l = listing(100, 42, 100, false);
        assert!(threshold_listing_matches(&r, &l, &h, Utc::now()));
    }

    #[test]
    fn threshold_rejects_above_price() {
        let h = helper();
        let r = rule(AnySelector::World(100), 100, false);
        let l = listing(100, 42, 101, false);
        assert!(!threshold_listing_matches(&r, &l, &h, Utc::now()));
    }

    #[test]
    fn threshold_respects_cooldown() {
        let h = helper();
        let now = Utc::now();
        let mut r = rule(AnySelector::World(100), 100, false);
        r.last_fired_at = Some(now - chrono::Duration::seconds(60));
        let l = listing(100, 42, 50, false);
        assert!(!threshold_listing_matches(&r, &l, &h, now));

        r.last_fired_at = Some(now - chrono::Duration::seconds(7200));
        assert!(threshold_listing_matches(&r, &l, &h, now));
    }

    #[test]
    fn threshold_rejects_other_item() {
        let h = helper();
        let r = rule(AnySelector::World(100), 100, false);
        let l = listing(100, 43, 50, false);
        assert!(!threshold_listing_matches(&r, &l, &h, Utc::now()));
    }

    fn baseline(hq: bool, p50: u32, cleaned: u32, band: ConfidenceBand) -> ItemStatsVariant {
        ItemStatsVariant {
            hq,
            sample_size_30d: cleaned,
            cleaned_sample_size_30d: cleaned,
            vwap_30d: p50,
            p50_30d: p50,
            confidence_band: band,
            launder_suspicion: 0.0,
        }
    }

    fn baselines(nq: Option<u32>, hq: Option<u32>) -> QualityBaselines {
        QualityBaselines {
            nq: nq.map(|p| baseline(false, p, 50, ConfidenceBand::High)),
            hq: hq.map(|p| baseline(true, p, 50, ConfidenceBand::High)),
        }
    }

    fn median_rule(world_selector: AnySelector, percent_below: i32, hq_only: bool) -> MedianRule {
        MedianRule {
            item_id: 42,
            world_selector,
            percent_below,
            hq_only,
            cooldown_seconds: 3600,
            last_fired_at: None,
        }
    }

    #[test]
    fn below_median_boundary_is_inclusive() {
        assert!(is_below_median(70, 100, 30));
        assert!(!is_below_median(71, 100, 30));
        assert!(!is_below_median(0, 100, 30));
    }

    #[test]
    fn below_median_matches_listing_under_its_own_quality_median() {
        let h = helper();
        let r = median_rule(AnySelector::Datacenter(10), 30, false);
        // NQ median 1000, HQ median 5000: an HQ listing at 2000 is cheap for
        // HQ, an NQ listing at 2000 is double the NQ median.
        let b = baselines(Some(1000), Some(5000));
        assert!(below_median_matches(
            &r,
            &listing(101, 42, 2000, true),
            &b,
            &h,
            Utc::now()
        ));
        assert!(!below_median_matches(
            &r,
            &listing(101, 42, 2000, false),
            &b,
            &h,
            Utc::now()
        ));
        assert!(below_median_matches(
            &r,
            &listing(101, 42, 700, false),
            &b,
            &h,
            Utc::now()
        ));
    }

    #[test]
    fn below_median_never_falls_back_across_qualities() {
        let h = helper();
        let r = median_rule(AnySelector::World(100), 30, false);
        // Only an NQ baseline: an HQ listing has nothing to compare against.
        let b = baselines(Some(1000), None);
        assert!(!below_median_matches(
            &r,
            &listing(100, 42, 10, true),
            &b,
            &h,
            Utc::now()
        ));
    }

    #[test]
    fn below_median_hq_only_skips_nq() {
        let h = helper();
        let r = median_rule(AnySelector::World(100), 30, true);
        let b = baselines(Some(1000), Some(1000));
        assert!(!below_median_matches(
            &r,
            &listing(100, 42, 10, false),
            &b,
            &h,
            Utc::now()
        ));
        assert!(below_median_matches(
            &r,
            &listing(100, 42, 10, true),
            &b,
            &h,
            Utc::now()
        ));
    }

    #[test]
    fn below_median_rejects_out_of_scope_and_other_items() {
        let h = helper();
        let r = median_rule(AnySelector::World(100), 30, false);
        let b = baselines(Some(1000), None);
        assert!(!below_median_matches(
            &r,
            &listing(101, 42, 10, false),
            &b,
            &h,
            Utc::now()
        ));
        assert!(!below_median_matches(
            &r,
            &listing(100, 43, 10, false),
            &b,
            &h,
            Utc::now()
        ));
        assert!(!below_median_matches(
            &r,
            &listing(9999, 42, 10, false),
            &b,
            &h,
            Utc::now()
        ));
    }

    #[test]
    fn below_median_ignores_unusable_baselines() {
        let h = helper();
        let r = median_rule(AnySelector::World(100), 30, false);
        let thin = QualityBaselines {
            nq: Some(baseline(
                false,
                1000,
                BELOW_MEDIAN_MIN_SAMPLES - 1,
                ConfidenceBand::High,
            )),
            hq: None,
        };
        assert!(!below_median_matches(
            &r,
            &listing(100, 42, 10, false),
            &thin,
            &h,
            Utc::now()
        ));
        let flagged = QualityBaselines {
            nq: Some(baseline(false, 1000, 500, ConfidenceBand::Unusable)),
            hq: None,
        };
        assert!(!below_median_matches(
            &r,
            &listing(100, 42, 10, false),
            &flagged,
            &h,
            Utc::now()
        ));
    }

    #[test]
    fn baseline_usability_rules() {
        assert!(baseline_is_usable(&baseline(
            false,
            100,
            BELOW_MEDIAN_MIN_SAMPLES,
            ConfidenceBand::Low
        )));
        assert!(baseline_is_usable(&baseline(
            false,
            100,
            20,
            ConfidenceBand::Unknown
        )));
        assert!(!baseline_is_usable(&baseline(
            false,
            0,
            20,
            ConfidenceBand::High
        )));
        assert!(!baseline_is_usable(&baseline(
            false,
            100,
            20,
            ConfidenceBand::Unusable
        )));
    }

    #[test]
    fn below_median_respects_cooldown() {
        let h = helper();
        let now = Utc::now();
        let mut r = median_rule(AnySelector::World(100), 30, false);
        r.last_fired_at = Some(now - chrono::Duration::seconds(60));
        let b = baselines(Some(1000), None);
        assert!(!below_median_matches(
            &r,
            &listing(100, 42, 10, false),
            &b,
            &h,
            now
        ));
        r.last_fired_at = Some(now - chrono::Duration::seconds(3600));
        assert!(below_median_matches(
            &r,
            &listing(100, 42, 10, false),
            &b,
            &h,
            now
        ));
    }

    #[test]
    fn quality_baselines_split_by_hq_flag() {
        let b = QualityBaselines::from_variants([
            baseline(true, 5, 20, ConfidenceBand::High),
            baseline(false, 3, 20, ConfidenceBand::High),
        ]);
        assert_eq!(b.for_quality(true).map(|v| v.p50_30d), Some(5));
        assert_eq!(b.for_quality(false).map(|v| v.p50_30d), Some(3));
    }
}

#[cfg(test)]
mod endpoint_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn retainer_sold_trigger_round_trips_with_type_tag_only() {
        let t = AlertTrigger::RetainerSold {};
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(v, json!({"type": "retainer_sold"}));
        let back: AlertTrigger = serde_json::from_value(v).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn endpoint_carries_disabled_reason_when_broken() {
        let e = Endpoint {
            id: 7,
            name: "Test".to_string(),
            method: EndpointMethod::DiscordChannel {
                channel_id: 5,
                channel_name: None,
                guild_id: None,
                guild_name: None,
            },
            disabled_reason: Some("Unknown Channel".to_string()),
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["disabled_reason"], json!("Unknown Channel"));
    }

    #[test]
    fn endpoint_accepts_payload_without_disabled_reason() {
        // A server that predates the field must still deserialize here.
        let v =
            json!({"id": 7, "name": "Test", "method": "Webhook", "url": "https://example.invalid"});
        let e: Endpoint = serde_json::from_value(v).unwrap();
        assert_eq!(e.disabled_reason, None);
    }

    #[test]
    fn endpoint_method_serializes_with_method_tag() {
        let m = EndpointMethod::DiscordDm { user_id: 42 };
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v, json!({"method": "DiscordDm", "user_id": 42}));
    }

    #[test]
    fn endpoint_serializes_with_flattened_method() {
        let e = Endpoint {
            id: 7,
            name: "Test".to_string(),
            method: EndpointMethod::Webhook {
                url: "https://example.invalid".into(),
            },
            disabled_reason: None,
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(
            v,
            json!({"id": 7, "name": "Test", "method": "Webhook", "url": "https://example.invalid"})
        );
    }

    #[test]
    fn create_endpoint_request_round_trips() {
        let req = CreateEndpointRequest {
            name: "My channel".into(),
            method: EndpointMethod::DiscordChannel {
                channel_id: 9,
                channel_name: None,
                guild_id: None,
                guild_name: None,
            },
        };
        let s = serde_json::to_string(&req).unwrap();
        let back: CreateEndpointRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn update_endpoint_request_all_none_round_trips() {
        let req = UpdateEndpointRequest {
            name: None,
            method: None,
        };
        let s = serde_json::to_string(&req).unwrap();
        let back: UpdateEndpointRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn update_endpoint_request_name_only_round_trips() {
        let req = UpdateEndpointRequest {
            name: Some("renamed".to_string()),
            method: None,
        };
        let s = serde_json::to_string(&req).unwrap();
        let back: UpdateEndpointRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn update_endpoint_request_method_change_round_trips() {
        let req = UpdateEndpointRequest {
            name: None,
            method: Some(EndpointMethod::Webhook {
                url: "https://discord.com/api/webhooks/1/abc".into(),
            }),
        };
        let s = serde_json::to_string(&req).unwrap();
        let back: UpdateEndpointRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn alert_trigger_new_variants_round_trip() {
        for trigger in [
            AlertTrigger::RetainerUndercut { margin_percent: 5 },
            AlertTrigger::ListUpdate { list_id: 42 },
            AlertTrigger::BelowMedian {
                item_id: 7,
                world_selector: AnySelector::Datacenter(4),
                percent_below: 30,
                hq_only: true,
            },
            AlertTrigger::BackInStock {
                item_id: 7,
                world_selector: AnySelector::World(21),
                hq_only: false,
            },
        ] {
            let s = serde_json::to_string(&trigger).unwrap();
            let back: AlertTrigger = serde_json::from_str(&s).unwrap();
            assert_eq!(trigger, back);
        }
    }

    #[test]
    fn new_market_triggers_use_snake_case_type_tags() {
        let v = serde_json::to_value(AlertTrigger::BackInStock {
            item_id: 7,
            world_selector: AnySelector::World(21),
            hq_only: false,
        })
        .unwrap();
        assert_eq!(v["type"], json!("back_in_stock"));
        let v = serde_json::to_value(AlertTrigger::BelowMedian {
            item_id: 7,
            world_selector: AnySelector::World(21),
            percent_below: 30,
            hq_only: false,
        })
        .unwrap();
        assert_eq!(v["type"], json!("below_median"));
    }

    #[test]
    fn update_alert_request_accepts_endpoint_and_cooldown_patch() {
        let req = UpdateAlertRequest {
            enabled: Some(true),
            price_threshold: None,
            endpoint_ids: Some(vec![1, 2]),
            cooldown_seconds: Some(120),
        };
        let s = serde_json::to_string(&req).unwrap();
        let back: UpdateAlertRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn endpoint_method_in_app_round_trips_with_method_tag_only() {
        let m = EndpointMethod::InApp {};
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v, json!({"method": "InApp"}));
        let back: EndpointMethod = serde_json::from_value(v).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn alert_event_deserializes_without_inbox_fields() {
        // A server that predates the notification-inbox fields must still
        // deserialize here; the new fields default to `None`.
        let v = json!({
            "id": 1,
            "alert_id": 2,
            "fired_at": "2026-01-01T00:00:00Z",
            "item_id": 42,
            "matched_listing_id": null,
            "matched_price": 100,
            "delivered": true,
            "delivery_error": null,
        });
        let e: AlertEvent = serde_json::from_value(v).unwrap();
        assert_eq!(e.read_at, None);
        assert_eq!(e.title, None);
        assert_eq!(e.body, None);
        assert_eq!(e.click_url, None);
    }
}
