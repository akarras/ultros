//! Durable receipts guarding sign-in adoption of browser (guest) price-alert
//! rules into account alerts — see `GuestAlertAdoptionBanner` in
//! `ultros-ui-alerts` for the flow that reads and writes these.
//!
//! The server does not deduplicate alerts created from adopted guest rules —
//! two `POST /api/v1/alerts` calls built from the same
//! [`GuestAlertRule`][crate::global_state::guest_alerts::GuestAlertRule] would
//! create two separate account alerts. This `localStorage` receipt, keyed by
//! `(user_id, rule_id)`, is the *only* guard against that: the adoption flow
//! writes a receipt immediately after a successful `create_alert` call and
//! *before* deleting the source guest rule, so an interruption between those
//! two steps (tab closed, page reloaded) is safely resumed on the next visit
//! — the receipt is found, the (already-created) alert is not duplicated,
//! and only the now-redundant local delete is retried.
//!
//! Mirrors the shape of `ultros-app`'s `guest_list_adoption::legacy_receipt`
//! (a plain `localStorage` read keyed by owner+id), lifted here so it can be
//! shared and unit-tested, and extended with a write path since this flow —
//! unlike list adoption — writes the receipt itself rather than only reading
//! one the server already recorded.

use serde::{Deserialize, Serialize};

/// `localStorage` key for one guest rule's adoption receipt.
pub fn receipt_key(user_id: u64, rule_id: &str) -> String {
    format!("ultros:guest-alert-adoption:v1:{user_id}:{rule_id}")
}

/// What's recorded once a guest alert rule has been turned into an account
/// alert.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdoptionReceipt {
    pub alert_id: i32,
}

/// Reads back the adoption receipt for `(user_id, rule_id)`, if one was
/// recorded.
///
/// Only meaningful in the browser (hydrate + wasm32 — `localStorage` isn't
/// reachable, and doesn't need to be, from SSR or native tooling). Every
/// other target always returns `None`; this is safe because callers only
/// ever see a non-empty guest-rule list client-side in the first place (see
/// `GuestAlerts`'s `hydrated` gate), so the SSR/no-op answer is never acted
/// on.
#[cfg(all(feature = "hydrate", target_arch = "wasm32"))]
pub fn read_receipt(user_id: u64, rule_id: &str) -> Option<AdoptionReceipt> {
    let text = web_sys::window()?
        .local_storage()
        .ok()??
        .get_item(&receipt_key(user_id, rule_id))
        .ok()??;
    serde_json::from_str(&text).ok()
}

#[cfg(not(all(feature = "hydrate", target_arch = "wasm32")))]
pub fn read_receipt(_user_id: u64, _rule_id: &str) -> Option<AdoptionReceipt> {
    None
}

/// Durably records that `rule_id` became `alert_id` for `user_id`.
///
/// Callers must write this receipt *before* deleting the source guest
/// rule — see the module docs above for why.
#[cfg(all(feature = "hydrate", target_arch = "wasm32"))]
pub fn write_receipt(user_id: u64, rule_id: &str, alert_id: i32) -> Result<(), String> {
    let storage = web_sys::window()
        .ok_or_else(|| "no window".to_string())?
        .local_storage()
        .map_err(|_| "localStorage unavailable".to_string())?
        .ok_or_else(|| "no localStorage".to_string())?;
    let text = serde_json::to_string(&AdoptionReceipt { alert_id }).map_err(|e| e.to_string())?;
    storage
        .set_item(&receipt_key(user_id, rule_id), &text)
        .map_err(|_| "failed to write adoption receipt".to_string())
}

#[cfg(not(all(feature = "hydrate", target_arch = "wasm32")))]
pub fn write_receipt(_user_id: u64, _rule_id: &str, _alert_id: i32) -> Result<(), String> {
    Err("adoption receipts are only available in the browser".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_key_has_expected_format() {
        assert_eq!(
            receipt_key(42, "rule-abc"),
            "ultros:guest-alert-adoption:v1:42:rule-abc"
        );
    }

    #[test]
    fn receipt_key_distinguishes_users_and_rules() {
        assert_ne!(receipt_key(1, "a"), receipt_key(2, "a"));
        assert_ne!(receipt_key(1, "a"), receipt_key(1, "b"));
    }
}
