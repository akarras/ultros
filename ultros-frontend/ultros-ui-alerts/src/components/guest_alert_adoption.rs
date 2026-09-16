//! Sign-in adoption of browser (guest) price-alert rules into account
//! alerts.
//!
//! A visitor can create price-alert rules before signing in — kept
//! client-only in `localStorage` by
//! [`GuestAlerts`][crate::global_state::guest_alerts::GuestAlerts]. Once they
//! sign in, [`GuestAlertAdoptionBanner`] offers to copy any rules that
//! haven't been adopted yet onto the account's auto-created `InApp`
//! endpoint, via `POST /api/v1/alerts`.
//!
//! **The server does not deduplicate alerts created this way** — two
//! identical `CreateAlertRequest`s for the same guest rule create two
//! separate account alerts. The only guard against double-adoption is the
//! durable
//! [`AdoptionReceipt`][crate::global_state::adoption_receipts::AdoptionReceipt]
//! written to `localStorage` *before* the guest rule is removed (see
//! [`adopt_all`] below) — an interruption between those two steps (tab
//! closed, page reloaded mid-flight) is safely resumed on the next run: the
//! receipt is found, the already-created alert is not duplicated, and only
//! the now-redundant local delete is retried.
//!
//! Visibility is driven entirely by reactive state that's already
//! hydration-safe:
//! [`GuestAlerts::rules`][crate::global_state::guest_alerts::GuestAlerts::rules]
//! reads as empty during SSR and the first client render (see that store's
//! `hydrated` gate), so this banner renders nothing until a real,
//! un-receipted rule set resolves client-side — no separate SSR-only stub
//! component is needed, and no hydration mismatch is possible.

use leptos::prelude::*;
use leptos::task::spawn_local;
use ultros_api_types::alert::{Endpoint, EndpointMethod};

use crate::api::{create_alert, get_login, list_endpoints};
use crate::global_state::adoption_receipts::{read_receipt, write_receipt};
use crate::global_state::guest_alerts::{GuestAlertRule, GuestAlerts, use_guest_alerts};
use crate::global_state::toasts::use_toast;
use crate::global_state::user::BootstrapUser;
use crate::i18n::{t, t_string, use_i18n};

/// Picks the caller's `InApp` delivery endpoint id out of `endpoints`. The
/// server auto-creates exactly one such endpoint per account and
/// `GET /api/v1/endpoints` always includes it, so this looks for the first
/// (only) `EndpointMethod::InApp` row.
pub fn find_inapp_endpoint_id(endpoints: &[Endpoint]) -> Option<i32> {
    endpoints
        .iter()
        .find(|endpoint| matches!(endpoint.method, EndpointMethod::InApp {}))
        .map(|endpoint| endpoint.id)
}

/// Outcome of one [`adopt_all`] run, used to build the closing toast.
struct AdoptionSummary {
    /// Rules that were successfully turned into a new account alert this run
    /// (a rule skipped because it already had a receipt is *not* counted
    /// here — nothing new was adopted for it).
    created: usize,
    first_error: Option<AdoptFailure>,
}

enum AdoptFailure {
    NoInAppEndpoint,
    Message(String),
}

/// Adopts every un-receipted rule currently in `guest` into `expected_user_id`'s
/// account.
///
/// 1. Re-checks `get_login()` and aborts if it fails or returns a different
///    user than `expected_user_id` — `BootstrapUser` can be stale (e.g. a
///    sign-out in another tab).
/// 2. Resolves the account's `InApp` endpoint id via `list_endpoints()`;
///    aborts if none exists yet.
/// 3. Snapshots the current rule list (so rules added mid-run by another tab
///    aren't picked up half-adopted) and, for each rule:
///    - if a receipt already exists, the rule was already adopted on a
///      previous (possibly interrupted) run — just delete it locally;
///    - otherwise, calls `create_alert`. On success, writes the receipt
///      *before* deleting the guest rule (see module docs); on failure,
///      keeps the rule and records the error, then continues with the rest.
///    - After every `create_alert` attempt, re-checks `get_login()` and
///      stops the loop early if the signed-in account changed mid-run.
async fn adopt_all(guest: GuestAlerts, expected_user_id: u64) -> AdoptionSummary {
    let mut summary = AdoptionSummary {
        created: 0,
        first_error: None,
    };

    match get_login().await {
        Ok(user) if user.id == expected_user_id => {}
        Ok(_) => {
            summary.first_error = Some(AdoptFailure::Message(
                "signed-in account changed".to_string(),
            ));
            return summary;
        }
        Err(e) => {
            summary.first_error = Some(AdoptFailure::Message(e.to_string()));
            return summary;
        }
    }

    let endpoints = match list_endpoints().await {
        Ok(endpoints) => endpoints,
        Err(e) => {
            summary.first_error = Some(AdoptFailure::Message(e.to_string()));
            return summary;
        }
    };
    let Some(inapp_id) = find_inapp_endpoint_id(&endpoints) else {
        summary.first_error = Some(AdoptFailure::NoInAppEndpoint);
        return summary;
    };

    let rules = guest.rules().get_untracked();
    for rule in rules {
        if read_receipt(expected_user_id, &rule.id).is_some() {
            guest.remove(&rule.id);
            continue;
        }

        match create_alert(rule.to_create_request(inapp_id)).await {
            Ok(alert) => match write_receipt(expected_user_id, &rule.id, alert.id) {
                Ok(()) => {
                    // Only now — the receipt is durable on disk, so an
                    // interruption right after this delete can never cause
                    // the next run to recreate the alert.
                    guest.remove(&rule.id);
                    summary.created += 1;
                }
                Err(e) => {
                    // Could not persist the receipt: leave the guest rule in
                    // place. The account now has this alert already, but
                    // without a receipt a retry would risk a duplicate — the
                    // rule staying visible at least surfaces that to the
                    // visitor instead of silently losing track of it.
                    if summary.first_error.is_none() {
                        summary.first_error = Some(AdoptFailure::Message(e));
                    }
                }
            },
            Err(e) => {
                if summary.first_error.is_none() {
                    summary.first_error = Some(AdoptFailure::Message(e.to_string()));
                }
            }
        }

        match get_login().await {
            Ok(user) if user.id == expected_user_id => {}
            _ => break,
        }
    }

    summary
}

/// Banner offering to adopt local guest alert rules into the signed-in
/// account. `compact` renders a single row (for the sidebar inbox
/// drop-up); the full variant renders a bordered box (for the `/alerts`
/// page).
#[component]
pub fn GuestAlertAdoptionBanner(#[prop(optional)] compact: bool) -> impl IntoView {
    let i18n = use_i18n();
    let Some(guest) = use_guest_alerts() else {
        return ().into_any();
    };

    // `BootstrapUser` is fixed for the lifetime of the page load (signing in
    // or out is always a full navigation), so a plain (non-reactive) lookup
    // here matches the idiom already used by `NotificationInbox`'s
    // `signed_in`.
    let user_id = match use_context::<BootstrapUser>() {
        Some(BootstrapUser(Some(user))) => Some(user.id),
        _ => None,
    };

    let pending: Memo<Vec<GuestAlertRule>> = Memo::new(move |_| {
        let Some(user_id) = user_id else {
            return Vec::new();
        };
        guest.rules().with(|rules| {
            rules
                .iter()
                .filter(|rule| read_receipt(user_id, &rule.id).is_none())
                .cloned()
                .collect()
        })
    });

    let dismissed = RwSignal::new(false);
    let adopting = RwSignal::new(false);
    let toasts = use_toast();

    let visible = Signal::derive(move || {
        user_id.is_some() && !pending.with(Vec::is_empty) && !dismissed.get()
    });

    let on_adopt = move |_| {
        let Some(user_id) = user_id else {
            return;
        };
        if adopting.get_untracked() {
            return;
        }
        adopting.set(true);
        spawn_local(async move {
            let summary = adopt_all(guest, user_id).await;
            let _ = adopting.try_set(false);
            let Some(toasts) = toasts else { return };
            match summary.first_error {
                Some(AdoptFailure::NoInAppEndpoint) => {
                    toasts.error(t_string!(i18n, guest_alert_adopt_no_inbox_endpoint).to_string());
                }
                Some(AdoptFailure::Message(error)) => {
                    toasts.error(
                        t_string!(i18n, guest_alert_adopt_failed, error = error).to_string(),
                    );
                }
                None if summary.created > 0 => {
                    toasts.success(
                        t_string!(i18n, guest_alert_adopt_done, count = summary.created)
                            .to_string(),
                    );
                }
                None => {}
            }
        });
    };

    view! {
        <Show when=move || visible.get()>
            {move || {
                let count = pending.with(Vec::len);
                if compact {
                    view! {
                        <div
                            class="menu-item guest-alert-adopt-banner guest-alert-adopt-banner-compact"
                            data-testid="guest-alert-adopt-banner-compact"
                        >
                            <span class="flex-1">
                                {t_string!(i18n, guest_alert_adopt_banner, count = count).to_string()}
                            </span>
                            <button
                                class="btn-primary"
                                disabled=move || adopting.get()
                                data-testid="guest-alert-adopt-button"
                                on:click=on_adopt
                            >
                                {t!(i18n, guest_alert_adopt_button)}
                            </button>
                        </div>
                    }
                        .into_any()
                } else {
                    view! {
                        <div
                            class="guest-alert-adopt-banner guest-alert-adopt-banner-full rounded-md border border-[color:var(--color-outline)] p-3 space-y-2"
                            data-testid="guest-alert-adopt-banner"
                        >
                            <p class="text-sm">
                                {t_string!(i18n, guest_alert_adopt_banner, count = count).to_string()}
                            </p>
                            <div class="flex gap-2">
                                <button
                                    class="btn-primary"
                                    disabled=move || adopting.get()
                                    data-testid="guest-alert-adopt-button"
                                    on:click=on_adopt
                                >
                                    {t!(i18n, guest_alert_adopt_button)}
                                </button>
                                <button
                                    class="btn-secondary"
                                    data-testid="guest-alert-adopt-later"
                                    on:click=move |_| dismissed.set(true)
                                >
                                    {t!(i18n, guest_alert_adopt_later)}
                                </button>
                            </div>
                        </div>
                    }
                        .into_any()
                }
            }}
        </Show>
    }
        .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultros_api_types::alert::{Endpoint, EndpointMethod};

    fn endpoint(id: i32, method: EndpointMethod) -> Endpoint {
        Endpoint {
            id,
            name: "test".to_string(),
            method,
            disabled_reason: None,
        }
    }

    #[test]
    fn find_inapp_endpoint_id_picks_in_app_among_others() {
        let endpoints = vec![
            endpoint(1, EndpointMethod::DiscordDm { user_id: 5 }),
            endpoint(2, EndpointMethod::InApp {}),
            endpoint(
                3,
                EndpointMethod::DiscordChannel {
                    channel_id: 7,
                    channel_name: None,
                    guild_id: None,
                    guild_name: None,
                },
            ),
        ];
        assert_eq!(find_inapp_endpoint_id(&endpoints), Some(2));
    }

    #[test]
    fn find_inapp_endpoint_id_none_when_absent() {
        let endpoints = vec![endpoint(1, EndpointMethod::DiscordDm { user_id: 5 })];
        assert_eq!(find_inapp_endpoint_id(&endpoints), None);
    }

    #[test]
    fn find_inapp_endpoint_id_empty_list() {
        assert_eq!(find_inapp_endpoint_id(&[]), None);
    }
}
