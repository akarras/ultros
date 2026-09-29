//! Hydrate-only live wiring for the notification inbox.
//!
//! For a signed-in visitor, fetches the first page of `AlertEvent`s once on
//! mount, then holds a `subscribe_notifications` websocket subscription open
//! for the rest of the session so new fires land in [`Inbox`] immediately —
//! surfaced as a toast, and (browser notification permission permitting) an
//! OS-level `Notification` too.
//!
//! Renders no DOM: this is a side-effect-only component, mounted once near
//! the app root beside `<ToastContainer/>`. The whole body is gated on the
//! `hydrate` feature (same idiom as `ultros-app`'s `ReloadWhenStale`) — SSR
//! has no websocket and no browser `Notification` API, and the initial page
//! fetch is deliberately client-only so SSR responses stay free of the extra
//! request. `BootstrapUser` is fixed for the lifetime of the page load
//! (`/logout` is a full navigation), so there's no sign-in/sign-out
//! transition to handle mid-session.
use leptos::prelude::*;

#[component]
pub fn InboxLive() -> impl IntoView {
    #[cfg(feature = "hydrate")]
    {
        use leptos::task::spawn_local;
        use ultros_api_types::websocket::ServerClient;
        use xiv_gen::ItemId;

        use crate::api::get_alert_events_page;
        use crate::global_state::notifications::{inbox_item_from_event, use_inbox};
        use crate::global_state::toasts::use_toast;
        use crate::global_state::user::BootstrapUser;
        use crate::global_state::xiv_data::tracked_data;
        use crate::i18n::{t_string, use_i18n};
        use crate::ws::realtime::{RealtimeSubscription, use_realtime};

        let signed_in = matches!(use_context::<BootstrapUser>(), Some(BootstrapUser(Some(_))));
        let inbox = use_inbox();

        if let (true, Some(inbox)) = (signed_in, inbox) {
            let i18n = use_i18n();

            spawn_local(async move {
                if let Ok(events) = get_alert_events_page(50, None).await {
                    inbox.ingest_server_events(
                        events,
                        move |item_id| {
                            tracked_data()
                                .items
                                .get(&ItemId(item_id))
                                .map(|item| item.name.as_str().to_string())
                                .unwrap_or_else(|| {
                                    t_string!(i18n, lists_workspace_item_fallback, id = item_id)
                                        .to_string()
                                })
                        },
                        move |event| match event.matched_price {
                            Some(price) => {
                                t_string!(i18n, inbox_fallback_price_body, price = price)
                                    .to_string()
                            }
                            None => t_string!(i18n, inbox_fallback_generic_body).to_string(),
                        },
                    );
                }
            });

            let realtime = use_realtime();
            let toasts = use_toast();
            let subscription = StoredValue::new(None::<RealtimeSubscription>);

            Effect::new(move |_| {
                let Some(realtime) = realtime.clone() else {
                    subscription.set_value(None);
                    return;
                };
                let sub = realtime.subscribe_notifications(move |message| {
                    let ServerClient::Notification(event) = message else {
                        return;
                    };
                    let item_name = move |item_id: i32| {
                        tracked_data()
                            .items
                            .get(&ItemId(item_id))
                            .map(|item| item.name.as_str().to_string())
                            .unwrap_or_else(|| {
                                t_string!(i18n, lists_workspace_item_fallback, id = item_id)
                                    .to_string()
                            })
                    };
                    let fallback_body = move |event: &ultros_api_types::alert::AlertEvent| {
                        match event.matched_price {
                            Some(price) => {
                                t_string!(i18n, inbox_fallback_price_body, price = price)
                                    .to_string()
                            }
                            None => t_string!(i18n, inbox_fallback_generic_body).to_string(),
                        }
                    };
                    let item = inbox_item_from_event(&event, item_name, fallback_body);
                    inbox.ingest_server_events(vec![event], item_name, fallback_body);
                    if let Some(toasts) = toasts {
                        toasts.info(item.body.clone());
                    }
                    maybe_show_browser_notification(&item.title, &item.body);
                });
                subscription.set_value(Some(sub));
            });

            on_cleanup(move || {
                subscription.set_value(None);
            });
        }
    }
}

/// Shows a browser `Notification` for a fired alert, but only when the
/// visitor already granted permission *and* the tab is not currently
/// visible — this never itself prompts, since a permission prompt must come
/// from a user gesture (see `ultros-ui-alerts/src/components/push_subscribe.rs`'s
/// `enable_browser_notifications`, which is where that prompt lives).
///
/// The visibility check matters most for a web-push-subscribed user: with
/// `Notification.permission == "granted"` (a precondition of having
/// subscribed to push in the first place) and the tab open, a fire would
/// otherwise show *both* this in-page OS notification *and* a second one
/// delivered through the service worker's push handler — a visible tab
/// already shows the toast and the sidebar unread pill, so the OS
/// notification is redundant there regardless. A push-endpoint-aware skip
/// (suppressing this entirely for a push subscriber even in a hidden tab,
/// to avoid relying on visibility alone) is a follow-up, not done here.
///
/// `pub` (rather than crate-private): the non-hydrate stub below would
/// otherwise be unreachable dead code on every build that doesn't enable
/// `hydrate` (i.e. both `clippy` invocations this crate is checked with), the
/// same reason `push_subscribe::enable_browser_notifications` is `pub`.
#[cfg(all(feature = "hydrate", target_arch = "wasm32"))]
pub fn maybe_show_browser_notification(title: &str, body: &str) {
    use web_sys::{Notification, NotificationOptions, NotificationPermission, VisibilityState};

    if Notification::permission() != NotificationPermission::Granted {
        return;
    }
    let tab_visible = web_sys::window()
        .and_then(|window| window.document())
        .map(|document| document.visibility_state() == VisibilityState::Visible)
        .unwrap_or(false);
    if tab_visible {
        return;
    }
    let options = NotificationOptions::new();
    options.set_body(body);
    let _ = Notification::new_with_options(title, &options);
}

#[cfg(not(all(feature = "hydrate", target_arch = "wasm32")))]
pub fn maybe_show_browser_notification(_title: &str, _body: &str) {}
