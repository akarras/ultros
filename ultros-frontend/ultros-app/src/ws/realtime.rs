use leptos::prelude::*;
use ultros_api_types::websocket::{FilterPredicate, ServerClient, SocketMessageType};

#[cfg(not(feature = "ssr"))]
mod client {
    use super::*;
    use gloo_timers::callback::Timeout;
    use send_wrapper::SendWrapper;
    use std::{
        cell::{Cell, RefCell},
        collections::HashMap,
        rc::{Rc, Weak},
    };
    use ultros_api_types::websocket::ClientMessage;
    use wasm_bindgen::{JsCast, closure::Closure};
    use web_sys::{CloseEvent, Event, MessageEvent, WebSocket};

    type Handler = Box<dyn Fn(ServerClient)>;

    #[derive(Clone)]
    pub(crate) struct RealtimeClient {
        inner: SendWrapper<Rc<RealtimeInner>>,
    }

    pub(crate) struct RealtimeSubscription {
        client: RealtimeClient,
        subscription_id: u64,
    }

    struct RealtimeInner {
        socket: RefCell<Option<WebSocket>>,
        handlers: RefCell<HashMap<u64, Handler>>,
        subscription_messages: RefCell<HashMap<u64, String>>,
        pending_messages: RefCell<Vec<String>>,
        next_subscription_id: Cell<u64>,
        reconnect_attempt: Cell<u32>,
        onopen: RefCell<Option<Closure<dyn FnMut(Event)>>>,
        onmessage: RefCell<Option<Closure<dyn FnMut(MessageEvent)>>>,
        onclose: RefCell<Option<Closure<dyn FnMut(CloseEvent)>>>,
        onerror: RefCell<Option<Closure<dyn FnMut(Event)>>>,
    }

    impl RealtimeClient {
        pub(crate) fn new() -> Self {
            let client = Self {
                inner: SendWrapper::new(Rc::new(RealtimeInner {
                    socket: RefCell::new(None),
                    handlers: RefCell::new(HashMap::new()),
                    subscription_messages: RefCell::new(HashMap::new()),
                    pending_messages: RefCell::new(Vec::new()),
                    next_subscription_id: Cell::new(1),
                    reconnect_attempt: Cell::new(0),
                    onopen: RefCell::new(None),
                    onmessage: RefCell::new(None),
                    onclose: RefCell::new(None),
                    onerror: RefCell::new(None),
                })),
            };
            client.connect();
            client
        }

        pub(crate) fn subscribe_market(
            &self,
            filter: FilterPredicate,
            msg_type: SocketMessageType,
            handler: impl Fn(ServerClient) + 'static,
        ) -> RealtimeSubscription {
            let subscription_id = self.next_subscription_id();
            self.inner
                .handlers
                .borrow_mut()
                .insert(subscription_id, Box::new(handler));
            self.send_subscription(
                subscription_id,
                ClientMessage::AddSubscribe {
                    subscription_id: Some(subscription_id),
                    filter,
                    msg_type,
                },
            );
            RealtimeSubscription {
                client: self.clone(),
                subscription_id,
            }
        }

        pub(crate) fn subscribe_list(
            &self,
            list_id: i32,
            handler: impl Fn(ServerClient) + 'static,
        ) -> RealtimeSubscription {
            let subscription_id = self.next_subscription_id();
            self.inner
                .handlers
                .borrow_mut()
                .insert(subscription_id, Box::new(handler));
            self.send_subscription(
                subscription_id,
                ClientMessage::SubscribeList {
                    subscription_id: Some(subscription_id),
                    list_id,
                },
            );
            RealtimeSubscription {
                client: self.clone(),
                subscription_id,
            }
        }

        fn next_subscription_id(&self) -> u64 {
            let id = self.inner.next_subscription_id.get();
            self.inner.next_subscription_id.set(id + 1);
            id
        }

        fn send_subscription(&self, subscription_id: u64, message: ClientMessage) {
            let Ok(text) = serde_json::to_string(&message) else {
                return;
            };
            self.inner
                .subscription_messages
                .borrow_mut()
                .insert(subscription_id, text.clone());
            if !self.send_text(&text) {
                self.connect();
            }
        }

        fn send_control(&self, message: ClientMessage) {
            let Ok(text) = serde_json::to_string(&message) else {
                return;
            };
            if !self.send_text(&text) {
                self.inner.pending_messages.borrow_mut().push(text);
                self.connect();
            }
        }

        fn send_text(&self, text: &str) -> bool {
            let Some(socket) = self.inner.socket.borrow().as_ref().cloned() else {
                return false;
            };
            if socket.ready_state() == WebSocket::OPEN {
                socket.send_with_str(text).is_ok()
            } else {
                false
            }
        }

        fn connect(&self) {
            if let Some(socket) = self.inner.socket.borrow().as_ref() {
                let ready_state = socket.ready_state();
                if ready_state == WebSocket::OPEN || ready_state == WebSocket::CONNECTING {
                    return;
                }
            }

            let Some(url) = websocket_url() else {
                return;
            };
            let Ok(socket) = WebSocket::new(&url) else {
                self.schedule_reconnect();
                return;
            };

            let weak = Rc::downgrade(&self.inner);
            let onopen = Closure::wrap(Box::new(move |_event: Event| {
                if let Some(inner) = weak.upgrade() {
                    inner.reconnect_attempt.set(0);
                    if let Some(socket) = inner.socket.borrow().as_ref().cloned() {
                        for message in inner.subscription_messages.borrow().values() {
                            let _ = socket.send_with_str(message);
                        }
                        for message in inner.pending_messages.borrow_mut().drain(..) {
                            let _ = socket.send_with_str(&message);
                        }
                    }
                }
            }) as Box<dyn FnMut(_)>);
            socket.set_onopen(Some(onopen.as_ref().unchecked_ref()));
            *self.inner.onopen.borrow_mut() = Some(onopen);

            let weak = Rc::downgrade(&self.inner);
            let onmessage = Closure::wrap(Box::new(move |event: MessageEvent| {
                let Some(text) = event.data().as_string() else {
                    return;
                };
                let Ok(message) = serde_json::from_str::<ServerClient>(&text) else {
                    return;
                };
                dispatch_message(&weak, message);
            }) as Box<dyn FnMut(_)>);
            socket.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
            *self.inner.onmessage.borrow_mut() = Some(onmessage);

            let weak = Rc::downgrade(&self.inner);
            let onclose = Closure::wrap(Box::new(move |_event: CloseEvent| {
                schedule_reconnect(&weak);
            }) as Box<dyn FnMut(_)>);
            socket.set_onclose(Some(onclose.as_ref().unchecked_ref()));
            *self.inner.onclose.borrow_mut() = Some(onclose);

            let weak = Rc::downgrade(&self.inner);
            let onerror = Closure::wrap(Box::new(move |_event: Event| {
                schedule_reconnect(&weak);
            }) as Box<dyn FnMut(_)>);
            socket.set_onerror(Some(onerror.as_ref().unchecked_ref()));
            *self.inner.onerror.borrow_mut() = Some(onerror);

            *self.inner.socket.borrow_mut() = Some(socket);
        }

        fn schedule_reconnect(&self) {
            schedule_reconnect(&Rc::downgrade(&self.inner));
        }

        fn unsubscribe(&self, subscription_id: u64) {
            self.inner.handlers.borrow_mut().remove(&subscription_id);
            self.inner
                .subscription_messages
                .borrow_mut()
                .remove(&subscription_id);
            self.send_control(ClientMessage::Unsubscribe { subscription_id });
        }
    }

    impl Drop for RealtimeSubscription {
        fn drop(&mut self) {
            self.client.unsubscribe(self.subscription_id);
        }
    }

    fn dispatch_message(weak: &Weak<RealtimeInner>, message: ServerClient) {
        let Some(inner) = weak.upgrade() else {
            return;
        };
        match message {
            ServerClient::SubscriptionEvent {
                subscription_id,
                event,
            } => {
                if let Some(handler) = inner.handlers.borrow().get(&subscription_id) {
                    handler(*event);
                }
            }
            ServerClient::Stale { subscription_id } => {
                if let Some(handler) = inner.handlers.borrow().get(&subscription_id) {
                    handler(ServerClient::Stale { subscription_id });
                }
            }
            ServerClient::Subscribed { subscription_id } => {
                if let Some(handler) = inner.handlers.borrow().get(&subscription_id) {
                    handler(ServerClient::Subscribed { subscription_id });
                }
            }
            ServerClient::Unsubscribed { subscription_id } => {
                if let Some(handler) = inner.handlers.borrow().get(&subscription_id) {
                    handler(ServerClient::Unsubscribed { subscription_id });
                }
            }
            ServerClient::Error { .. } => {
                for handler in inner.handlers.borrow().values() {
                    handler(message.clone());
                }
            }
            ServerClient::SocketConnected | ServerClient::SubscriptionCreated => {}
            ServerClient::Sales(_) | ServerClient::Listings(_) | ServerClient::ListUpdate(_) => {
                for handler in inner.handlers.borrow().values() {
                    handler(message.clone());
                }
            }
        }
    }

    fn schedule_reconnect(weak: &Weak<RealtimeInner>) {
        let Some(inner) = weak.upgrade() else {
            return;
        };
        if inner.subscription_messages.borrow().is_empty()
            && inner.pending_messages.borrow().is_empty()
        {
            return;
        }
        let attempt = inner.reconnect_attempt.get().saturating_add(1).min(6);
        inner.reconnect_attempt.set(attempt);
        let delay_ms = 500_u32.saturating_mul(2_u32.saturating_pow(attempt));
        let weak = Rc::downgrade(&inner);
        Timeout::new(delay_ms, move || {
            if let Some(inner) = weak.upgrade() {
                RealtimeClient {
                    inner: SendWrapper::new(inner),
                }
                .connect();
            }
        })
        .forget();
    }

    fn websocket_url() -> Option<String> {
        let window = web_sys::window()?;
        let location = window.location();
        let protocol = location.protocol().ok()?;
        let host = location.host().ok()?;
        let ws_protocol = if protocol == "https:" { "wss" } else { "ws" };
        Some(format!("{ws_protocol}://{host}/api/v1/realtime/events"))
    }
}

#[cfg(feature = "ssr")]
mod client {
    use super::*;

    #[derive(Clone)]
    pub(crate) struct RealtimeClient;

    pub(crate) struct RealtimeSubscription;

    impl RealtimeClient {
        pub(crate) fn new() -> Self {
            Self
        }

        pub(crate) fn subscribe_market(
            &self,
            _filter: FilterPredicate,
            _msg_type: SocketMessageType,
            _handler: impl Fn(ServerClient) + 'static,
        ) -> RealtimeSubscription {
            RealtimeSubscription
        }

        pub(crate) fn subscribe_list(
            &self,
            _list_id: i32,
            _handler: impl Fn(ServerClient) + 'static,
        ) -> RealtimeSubscription {
            RealtimeSubscription
        }
    }
}

pub(crate) use client::{RealtimeClient, RealtimeSubscription};

pub(crate) fn provide_realtime_context() {
    provide_context(RealtimeClient::new());
}

pub(crate) fn use_realtime() -> Option<RealtimeClient> {
    use_context::<RealtimeClient>()
}
