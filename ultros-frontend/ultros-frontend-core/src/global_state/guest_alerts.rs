//! Guest price-alert rules: client-only alert rules for a visitor without an
//! account, kept in `localStorage` and evaluated locally against realtime
//! listing events (evaluator built in a later task). Mirrors the shape of a
//! server-side [`ThresholdRule`]/[`AlertTrigger::BelowThreshold`] closely
//! enough that a rule can be handed straight to the server (via
//! [`GuestAlertRule::to_create_request`]) if the guest later signs in and
//! wants to keep it.

use chrono::{DateTime, Utc};
use codee::string::JsonSerdeCodec;
use leptos::prelude::*;
use leptos_use::storage::{UseStorageOptions, use_local_storage_with_options};
use serde::{Deserialize, Serialize};
use ultros_api_types::alert::{
    AlertTrigger, CreateAlertRequest, ThresholdRule, is_off_cooldown_at,
};
use ultros_api_types::world_helper::AnySelector;
use uuid::Uuid;

/// `localStorage` key for guest alert rules.
pub const GUEST_ALERTS_KEY: &str = "ultros.guest_alerts.v1";
/// A guest without an account has no server-side row limit to fall back on,
/// so the client enforces one directly.
pub const GUEST_ALERTS_CAP: usize = 50;

/// One guest-configured price-threshold alert rule.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GuestAlertRule {
    pub id: String,
    pub item_id: i32,
    pub world_selector: AnySelector,
    pub price_threshold: i32,
    pub hq_only: bool,
    pub cooldown_seconds: i32,
    pub last_fired_at: Option<DateTime<Utc>>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}

impl GuestAlertRule {
    /// Builds a new enabled rule with a fresh id and `created_at = now`.
    /// `cooldown_seconds` defaults to 3600 (1 hour) when `None`, matching
    /// `CreateAlertRequest::cooldown_seconds`'s documented server default.
    pub fn new(
        item_id: i32,
        world_selector: AnySelector,
        price_threshold: i32,
        hq_only: bool,
        cooldown_seconds: Option<i32>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            item_id,
            world_selector,
            price_threshold,
            hq_only,
            cooldown_seconds: cooldown_seconds.unwrap_or(3600),
            last_fired_at: None,
            enabled: true,
            created_at: Utc::now(),
        }
    }

    /// Shapes this rule for [`ultros_api_types::alert::threshold_listing_matches`],
    /// the same pure matcher the server uses for signed-in users' alerts.
    pub fn to_threshold_rule(&self) -> ThresholdRule {
        ThresholdRule {
            item_id: self.item_id,
            world_selector: self.world_selector,
            price_threshold: self.price_threshold,
            hq_only: self.hq_only,
            cooldown_seconds: self.cooldown_seconds,
            last_fired_at: self.last_fired_at,
        }
    }

    pub fn to_trigger(&self) -> AlertTrigger {
        AlertTrigger::BelowThreshold {
            item_id: self.item_id,
            world_selector: self.world_selector,
            price_threshold: self.price_threshold,
            hq_only: self.hq_only,
        }
    }

    /// Shapes a request to persist this rule server-side against `endpoint_id`
    /// (the caller's auto-created `InApp` endpoint) — used when a guest signs
    /// in and adopts their local rules.
    pub fn to_create_request(&self, endpoint_id: i32) -> CreateAlertRequest {
        CreateAlertRequest {
            trigger: self.to_trigger(),
            delivery: None,
            endpoint_ids: vec![endpoint_id],
            cooldown_seconds: Some(self.cooldown_seconds),
        }
    }
}

/// Appends `rule` to `rules` unless already at `cap`. Returns whether it was
/// added.
pub fn add_rule_bounded(rules: &mut Vec<GuestAlertRule>, rule: GuestAlertRule, cap: usize) -> bool {
    if rules.len() >= cap {
        return false;
    }
    rules.push(rule);
    true
}

/// Item ids covered by every *enabled* rule, sorted and deduplicated — the
/// shape the realtime evaluator wants for building its listing filter.
pub fn item_ids(rules: &[GuestAlertRule]) -> Vec<i32> {
    let mut ids: Vec<i32> = rules
        .iter()
        .filter(|rule| rule.enabled)
        .map(|rule| rule.item_id)
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// True when a rule last fired at `last_fired_at` is free to fire again given
/// `cooldown_seconds`, as of `now`. Delegates to the same pure contract the
/// server uses ([`is_off_cooldown_at`]), so a guest evaluates cooldowns
/// identically to a signed-in user's alerts.
pub fn cooldown_elapsed(
    last_fired_at: Option<DateTime<Utc>>,
    cooldown_seconds: i32,
    now: DateTime<Utc>,
) -> bool {
    is_off_cooldown_at(last_fired_at, cooldown_seconds, now)
}

/// Reactive guest-alert-rules store, backed by `localStorage`.
///
/// `delay_during_hydration(true)` plus a `hydrated` gate on `rules` (an
/// `RwSignal<bool>`, flipped by an `Effect` that only ever runs client-side)
/// follow the same hydration-safety idiom as
/// [`crate::global_state::notifications::Inbox`] — see that module's docs
/// for why both are needed.
#[derive(Clone, Copy)]
pub struct GuestAlerts {
    local_write: WriteSignal<Vec<GuestAlertRule>>,
    rules: Signal<Vec<GuestAlertRule>>,
    item_ids: Memo<Vec<i32>>,
    /// Shared across every mounted `GuestAlertAdoptionBanner` (the compact
    /// sidebar one and the full `/alerts` one render at the same time for a
    /// signed-in visitor with un-adopted rules) so only one `adopt_all` run
    /// is ever in flight — see [`try_start_adoption`][Self::try_start_adoption].
    adoption_in_flight: RwSignal<bool>,
}

impl Default for GuestAlerts {
    fn default() -> Self {
        Self::new()
    }
}

impl GuestAlerts {
    pub fn new() -> Self {
        let (local_read, local_write, _delete_fn) =
            use_local_storage_with_options::<Vec<GuestAlertRule>, JsonSerdeCodec>(
                GUEST_ALERTS_KEY,
                UseStorageOptions::default().delay_during_hydration(true),
            );

        let hydrated = RwSignal::new(false);
        Effect::new(move |_| {
            hydrated.set(true);
        });

        let rules = Signal::derive(move || {
            if hydrated.get() {
                local_read.get()
            } else {
                Vec::new()
            }
        });
        let item_ids_memo = Memo::new(move |_| rules.with(|rules| item_ids(rules)));

        Self {
            local_write,
            rules,
            item_ids: item_ids_memo,
            adoption_in_flight: RwSignal::new(false),
        }
    }

    pub fn rules(&self) -> Signal<Vec<GuestAlertRule>> {
        self.rules
    }

    /// Whether an `adopt_all` run is currently in flight, for any mounted
    /// banner to disable its button against.
    pub fn adoption_in_flight(&self) -> Signal<bool> {
        self.adoption_in_flight.into()
    }

    /// Attempts to claim the adoption in-flight flag. Returns `true` if this
    /// call claimed it (the caller may proceed with an `adopt_all` run) or
    /// `false` if another run — from this banner or the other mounted one —
    /// already holds it, in which case the caller must not start a second
    /// run. `try_update`, matching every other mutator here: this can be
    /// called from event handlers that may fire after the owning component
    /// has been disposed.
    pub fn try_start_adoption(&self) -> bool {
        let mut claimed = false;
        let _ = self.adoption_in_flight.try_update(|in_flight| {
            if !*in_flight {
                *in_flight = true;
                claimed = true;
            }
        });
        claimed
    }

    /// Releases the adoption in-flight flag. Idempotent — safe to call even
    /// when nothing is currently claimed, so every `adopt_all` exit path
    /// (success, early return, or the mid-loop break on a changed account)
    /// can call it unconditionally.
    pub fn finish_adoption(&self) {
        let _ = self.adoption_in_flight.try_set(false);
    }

    /// Adds `rule` unless the guest is already at [`GUEST_ALERTS_CAP`].
    /// Returns whether it was added.
    ///
    /// `try_update` throughout this impl, not `update`: callers include the
    /// realtime evaluator (built in a later task), which reacts to websocket
    /// events and may run after this store's owner has been disposed (e.g.
    /// mid-navigation) — `try_update` no-ops instead of panicking in that
    /// case, same reasoning as `Inbox`'s mutators.
    pub fn add(&self, rule: GuestAlertRule) -> bool {
        let mut added = false;
        let _ = self.local_write.try_update(|rules| {
            added = add_rule_bounded(rules, rule, GUEST_ALERTS_CAP);
        });
        added
    }

    pub fn remove(&self, id: &str) {
        let _ = self
            .local_write
            .try_update(|rules| rules.retain(|rule| rule.id != id));
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) {
        let _ = self.local_write.try_update(|rules| {
            if let Some(rule) = rules.iter_mut().find(|rule| rule.id == id) {
                rule.enabled = enabled;
            }
        });
    }

    pub fn touch_fired(&self, id: &str, at: DateTime<Utc>) {
        let _ = self.local_write.try_update(|rules| {
            if let Some(rule) = rules.iter_mut().find(|rule| rule.id == id) {
                rule.last_fired_at = Some(at);
            }
        });
    }

    pub fn item_ids(&self) -> Memo<Vec<i32>> {
        self.item_ids
    }
}

pub fn provide_guest_alerts() {
    provide_context(GuestAlerts::new());
}

pub fn use_guest_alerts() -> Option<GuestAlerts> {
    use_context::<GuestAlerts>()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a `GuestAlerts` around plain in-memory signals (no
    /// `localStorage` hook), for tests that only exercise logic independent
    /// of the rule list itself — e.g. the adoption in-flight flag.
    fn test_guest_alerts() -> GuestAlerts {
        let (_read, local_write) = signal(Vec::<GuestAlertRule>::new());
        let rules = Signal::derive(Vec::<GuestAlertRule>::new);
        let item_ids = Memo::new(|_| Vec::<i32>::new());
        GuestAlerts {
            local_write,
            rules,
            item_ids,
            adoption_in_flight: RwSignal::new(false),
        }
    }

    #[test]
    fn adoption_in_flight_guards_concurrent_claims() {
        let owner = Owner::new();
        let guest = owner.with(test_guest_alerts);

        // The compact sidebar banner claims it first...
        assert!(guest.try_start_adoption());
        // ...so the full `/alerts` banner (mounted at the same time for a
        // signed-in visitor) must not also start a run.
        assert!(!guest.try_start_adoption());
        assert!(guest.adoption_in_flight().get_untracked());

        guest.finish_adoption();
        assert!(!guest.adoption_in_flight().get_untracked());
        // Idempotent: releasing an already-released flag is a no-op, not a
        // panic — every `adopt_all` exit path calls this unconditionally.
        guest.finish_adoption();

        // Released, so the next run is free to claim it.
        assert!(guest.try_start_adoption());
    }

    fn t(seconds: i64) -> DateTime<Utc> {
        DateTime::<Utc>::UNIX_EPOCH + chrono::Duration::seconds(seconds)
    }

    fn rule(id: &str, item_id: i32, enabled: bool) -> GuestAlertRule {
        GuestAlertRule {
            id: id.to_string(),
            item_id,
            world_selector: AnySelector::World(100),
            price_threshold: 1000,
            hq_only: false,
            cooldown_seconds: 3600,
            last_fired_at: None,
            enabled,
            created_at: t(0),
        }
    }

    #[test]
    fn add_rule_rejects_when_at_cap() {
        let mut rules: Vec<GuestAlertRule> = (0..GUEST_ALERTS_CAP)
            .map(|i| rule(&format!("id-{i}"), i as i32, true))
            .collect();
        let accepted = add_rule_bounded(&mut rules, rule("overflow", 999, true), GUEST_ALERTS_CAP);
        assert!(!accepted);
        assert_eq!(rules.len(), GUEST_ALERTS_CAP);
        assert!(!rules.iter().any(|r| r.id == "overflow"));
    }

    #[test]
    fn add_rule_accepts_below_cap() {
        let mut rules: Vec<GuestAlertRule> = vec![rule("a", 1, true)];
        let accepted = add_rule_bounded(&mut rules, rule("b", 2, true), GUEST_ALERTS_CAP);
        assert!(accepted);
        assert_eq!(rules.len(), 2);
    }

    #[test]
    fn item_ids_excludes_disabled_and_dedupes() {
        let rules = vec![
            rule("a", 3, true),
            rule("b", 1, true),
            rule("c", 3, true),
            rule("d", 2, false),
        ];
        assert_eq!(item_ids(&rules), vec![1, 3]);
    }

    #[test]
    fn cooldown_elapsed_false_inside_window_true_after() {
        let now = t(10_000);
        assert!(!cooldown_elapsed(
            Some(now - chrono::Duration::seconds(60)),
            3600,
            now
        ));
        assert!(cooldown_elapsed(
            Some(now - chrono::Duration::seconds(7200)),
            3600,
            now
        ));
    }

    #[test]
    fn rule_round_trips_through_json_with_datacenter_selector() {
        let mut r = rule("dc-rule", 42, true);
        r.world_selector = AnySelector::Datacenter(10);
        let json = serde_json::to_string(&r).unwrap();
        let back: GuestAlertRule = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn to_create_request_uses_in_app_endpoint_and_cooldown() {
        let mut r = rule("x", 42, true);
        r.cooldown_seconds = 1800;
        let request = r.to_create_request(7);
        assert_eq!(request.endpoint_ids, vec![7]);
        assert_eq!(request.cooldown_seconds, Some(1800));
        assert_eq!(request.delivery, None);
        assert_eq!(
            request.trigger,
            AlertTrigger::BelowThreshold {
                item_id: 42,
                world_selector: AnySelector::World(100),
                price_threshold: 1000,
                hq_only: false,
            }
        );
    }
}
