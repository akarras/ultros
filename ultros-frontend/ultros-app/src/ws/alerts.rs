#[cfg(feature = "hydrate")]
use futures::{SinkExt, StreamExt};
#[cfg(feature = "hydrate")]
use gloo_net::websocket::{Message, futures::WebSocket};
use leptos::prelude::*;
use ultros_api_types::alerts::AlertsRx;
#[cfg(feature = "hydrate")]
use ultros_api_types::alerts::AlertsTx;
use ultros_api_types::world_helper::AnySelector;
#[cfg(feature = "hydrate")]
use web_sys::{Notification, NotificationPermission};

#[derive(Clone, Copy)]
pub struct AlertsService {
    tx: RwSignal<Option<futures::channel::mpsc::UnboundedSender<AlertsRx>>>,
}

impl AlertsService {
    pub fn new() -> Self {
        let tx = RwSignal::new(None);

        Effect::new(move |_| {
            #[cfg(feature = "hydrate")]
            spawn_local(async move {
                let window = web_sys::window().unwrap();
                let location = window.location();
                let protocol = location.protocol().unwrap();
                let host = location.host().unwrap();
                let ws_protocol = if protocol == "https:" { "wss" } else { "ws" };
                let url = format!("{}://{}/alerts/websocket", ws_protocol, host);

                match WebSocket::open(&url) {
                    Ok(socket) => {
                        let (mut write, mut read) = socket.split();
                        let (sender, mut receiver) = futures::channel::mpsc::unbounded();
                        tx.set(Some(sender));

                        let mut send_task = async move {
                            while let Some(msg) = receiver.next().await {
                                let json = serde_json::to_string(&msg).unwrap();
                                if let Err(e) = write.send(Message::Text(json)).await {
                                    tracing::error!("Error sending alert message: {e:?}");
                                    break;
                                }
                            }
                        };

                        let mut recv_task = async move {
                            while let Some(msg) = read.next().await {
                                match msg {
                                    Ok(Message::Text(text)) => {
                                        if let Ok(alert) = serde_json::from_str::<AlertsTx>(&text) {
                                            handle_alert(alert);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        };

                        futures::future::select(Box::pin(send_task), Box::pin(recv_task)).await;
                    }
                    Err(e) => {
                        tracing::error!("Failed to open alerts websocket: {e:?}");
                    }
                }
            });
        });

        Self { tx }
    }

    pub fn create_price_alert(
        &self,
        item_id: i32,
        price_threshold: i32,
        travel_amount: AnySelector,
    ) {
        if let Some(tx) = self.tx.get_untracked() {
            let msg = AlertsRx::CreatePriceAlert {
                item_id,
                price_threshold,
                travel_amount,
            };
            let _ = tx.unbounded_send(msg);
        } else {
            // Queue or error? For now just log.
            tracing::warn!("Alerts websocket not connected");
        }
    }

    pub fn request_permission() {
        #[cfg(feature = "hydrate")]
        if let Ok(promise) = Notification::request_permission() {
            let _ = wasm_bindgen_futures::JsFuture::from(promise);
        }
    }
}

#[cfg(feature = "hydrate")]
fn handle_alert(alert: AlertsTx) {
    if Notification::permission() == NotificationPermission::Granted {
        match alert {
            AlertsTx::PriceAlert {
                item_name, price, ..
            } => {
                let _ = Notification::new_with_options(
                    &format!("Price Alert: {}", item_name),
                    web_sys::NotificationOptions::new()
                        .body(&format!("Price dropped to {}!", price))
                        .icon("/favicon.ico"),
                );
            }
            AlertsTx::RetainerUndercut {
                item_name,
                undercut_retainers,
                ..
            } => {
                let _ = Notification::new_with_options(
                    &format!("Undercut Alert: {}", item_name),
                    web_sys::NotificationOptions::new()
                        .body(&format!("{} retainers undercut.", undercut_retainers.len()))
                        .icon("/favicon.ico"),
                );
            }
        }
    }
}
