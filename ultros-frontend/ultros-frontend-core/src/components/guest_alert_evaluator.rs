//! Hydrate-only realtime evaluator for guest (no-account) price-alert rules.
//!
//! For a visitor without an account, holds one `subscribe_market` websocket
//! subscription open over every item id an *enabled* guest rule watches
//! ([`GuestAlerts::item_ids`]), and on each `Added`/`Updated` listing batch
//! runs [`evaluate_rules`] to find rules that just matched. A hit is pushed
//! into the local half of the notification inbox, surfaced as a toast, and
//! (browser notification permission permitting) an OS-level `Notification`
//! too — the same three effects `InboxLive` produces for a signed-in user's
//! server-delivered alert, just sourced locally instead of over
//! `subscribe_notifications`.
//!
//! Renders no DOM: side-effect-only, mounted once near the app root beside
//! `<InboxLive/>`. The whole body is gated on the `hydrate` feature, same
//! idiom as `InboxLive` — SSR has no websocket, and a guest's rules live in
//! `localStorage`, which doesn't exist server-side either.
use leptos::prelude::*;

#[component]
pub fn GuestAlertEvaluator() -> impl IntoView {
    #[cfg(feature = "hydrate")]
    {
        use std::cell::RefCell;
        use std::rc::Rc;

        use uuid::Uuid;
        use xiv_gen::ItemId;

        use ultros_api_types::websocket::{
            EventType, FilterPredicate, ServerClient, SocketMessageType,
        };
        use ultros_api_types::world_helper::AnySelector;

        use crate::components::inbox_live::maybe_show_browser_notification;
        use crate::global_state::guest_alert_evaluator::{DedupeWindow, evaluate_rules};
        use crate::global_state::guest_alerts::use_guest_alerts;
        use crate::global_state::local_world_data::use_world_helper;
        use crate::global_state::notifications::{InboxId, InboxItem, use_inbox};
        use crate::global_state::toasts::use_toast;
        use crate::global_state::user::BootstrapUser;
        use crate::global_state::xiv_data::tracked_data;
        use crate::i18n::{t_string, use_i18n};
        use crate::ws::realtime::{RealtimeSubscription, use_realtime};

        // Guest-gated: evaluate only for a visitor we know isn't signed in
        // (or haven't heard from the bootstrap either way — treated the
        // same as "not signed in" here, matching every other guest-only
        // surface in this crate).
        let is_guest = !matches!(use_context::<BootstrapUser>(), Some(BootstrapUser(Some(_))));

        if let (true, Some(guest), Some(inbox)) = (is_guest, use_guest_alerts(), use_inbox()) {
            let i18n = use_i18n();
            let realtime = use_realtime();
            let toasts = use_toast();
            let item_ids = guest.item_ids();
            let subscription = StoredValue::new(None::<RealtimeSubscription>);
            // Shared across resubscribes (not reset per-Effect-run) so a
            // rule set edit that adds/removes an item id doesn't re-open the
            // window for pairs that already fired under the old
            // subscription.
            let dedupe = Rc::new(RefCell::new(DedupeWindow::new()));

            Effect::new(move |_| {
                // The only tracked read in this Effect: threshold edits
                // (price, hq_only, cooldown, enabled) must not resubscribe,
                // only a change in the *set of watched item ids* should.
                let ids = item_ids.get();

                // Drop the previous subscription unconditionally — either
                // we're about to open a new one, or there's nothing left to
                // watch.
                subscription.set_value(None);

                if ids.is_empty() {
                    return;
                }
                let Some(realtime) = realtime.clone() else {
                    return;
                };
                let Ok(worlds) = use_world_helper() else {
                    return;
                };

                let dedupe = dedupe.clone();
                let handler = move |message: ServerClient| {
                    let ServerClient::Listings(event) = message else {
                        return;
                    };
                    let data = match event {
                        EventType::Added(data) | EventType::Updated(data) => data,
                        EventType::Removed(_) => return,
                    };

                    // Rules read untracked: this closure runs from a raw
                    // websocket callback, outside any reactive scope, and a
                    // threshold edit must not force a resubscribe (handled
                    // above by only tracking `item_ids`).
                    let rules = guest.rules().get_untracked();
                    let now = chrono::Utc::now();
                    let hits = {
                        let mut dedupe = dedupe.borrow_mut();
                        evaluate_rules(&rules, &data, &worlds, now, &mut dedupe)
                    };

                    for hit in hits {
                        guest.touch_fired(&hit.rule_id, now);

                        let item_id = hit.listing.item_id;
                        let item_name = tracked_data()
                            .items
                            .get(&ItemId(item_id))
                            .map(|item| item.name.as_str().to_string())
                            .unwrap_or_else(|| {
                                t_string!(i18n, lists_workspace_item_fallback, id = item_id)
                                    .to_string()
                            });
                        let world_name = worlds
                            .lookup_selector(AnySelector::World(hit.listing.world_id))
                            .map(|resolved| resolved.get_name().to_string());
                        let url = match &world_name {
                            Some(world_name) => format!("/item/{world_name}/{item_id}"),
                            None => format!("/item/{item_id}"),
                        };

                        let title =
                            t_string!(i18n, inbox_guest_hit_title, item = item_name.clone())
                                .to_string();
                        let body = t_string!(
                            i18n,
                            inbox_guest_hit_body,
                            price = hit.listing.price_per_unit,
                            world = world_name
                                .clone()
                                .unwrap_or_else(|| hit.listing.world_id.to_string())
                        )
                        .to_string();

                        inbox.push_local(InboxItem {
                            id: InboxId::Local(Uuid::new_v4().to_string()),
                            title: title.clone(),
                            body: body.clone(),
                            url: Some(url),
                            item_id,
                            at: now,
                            read: false,
                            source_key: Some(format!("{}:{}", hit.rule_id, hit.listing.id)),
                        });

                        if let Some(toasts) = toasts {
                            toasts.info(body.clone());
                        }
                        maybe_show_browser_notification(&title, &body);
                    }
                };

                let sub = realtime.subscribe_market(
                    FilterPredicate::Items(ids),
                    SocketMessageType::Listings,
                    handler,
                );
                subscription.set_value(Some(sub));
            });

            on_cleanup(move || {
                subscription.set_value(None);
            });
        }
    }
}
