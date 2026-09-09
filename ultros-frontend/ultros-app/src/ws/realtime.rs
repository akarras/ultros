use leptos::prelude::*;
use ultros_api_types::websocket::{FilterPredicate, ServerClient, SocketMessageType};

#[cfg(not(feature = "ssr"))]
mod client {
    use super::*;
    use chrono::{DateTime, Utc};
    use gloo_timers::callback::Timeout;
    use send_wrapper::SendWrapper;
    use std::{
        cell::{Cell, RefCell},
        collections::{HashMap, HashSet},
        rc::{Rc, Weak},
    };
    use ultros_api_types::websocket::ClientMessage;
    use wasm_bindgen::{JsCast, closure::Closure};
    use web_sys::{CloseEvent, Event, MessageEvent, WebSocket};

    type Handler = Box<dyn Fn(ServerClient)>;

    /// One entry in `subscription_messages`. Most subscriptions (market
    /// filters, legacy list subscriptions) send the same JSON on every
    /// reconnect, so the serialized text is captured once. A list document
    /// subscription's version changes as the local document advances, so
    /// its replay message must be rebuilt at send time from a factory
    /// rather than replayed verbatim (spec section 5: a stale version in
    /// the replayed handshake would make the server's diff miss whatever
    /// the client committed since the original subscribe).
    enum SubscriptionEntry {
        Static(String),
        Dynamic(Rc<dyn Fn() -> ClientMessage>),
    }

    #[derive(Clone)]
    pub(crate) struct RealtimeClient {
        pub status: Signal<String>,
        pub last_update: Signal<Option<DateTime<Utc>>>,
        inner: SendWrapper<Rc<RealtimeInner>>,
    }

    pub(crate) struct RealtimeSubscription {
        client: RealtimeClient,
        subscription_id: u64,
    }

    struct RealtimeInner {
        socket: RefCell<Option<WebSocket>>,
        handlers: RefCell<HashMap<u64, Handler>>,
        subscription_messages: RefCell<HashMap<u64, SubscriptionEntry>>,
        /// Subscription ids created by `subscribe_list_doc`, so the bare
        /// `ServerClient::Error` broadcast arm in `dispatch_message` can
        /// skip them: a list-doc handler must only ever see an `Error` that
        /// was scoped to it (wrapped in `SubscriptionEvent` by the server),
        /// never one meant for some other subscription attempt on the same
        /// socket. Legacy (market/list) handlers are unaffected and keep
        /// receiving every broadcast `Error` as before.
        list_doc_subscriptions: RefCell<HashSet<u64>>,
        pending_messages: RefCell<Vec<String>>,
        next_subscription_id: Cell<u64>,
        reconnect_attempt: Cell<u32>,
        status: Signal<String>,
        last_update: Signal<Option<DateTime<Utc>>>,
        set_status: WriteSignal<String>,
        set_last_update: WriteSignal<Option<DateTime<Utc>>>,
        onopen: RefCell<Option<Closure<dyn FnMut(Event)>>>,
        onmessage: RefCell<Option<Closure<dyn FnMut(MessageEvent)>>>,
        onclose: RefCell<Option<Closure<dyn FnMut(CloseEvent)>>>,
        onerror: RefCell<Option<Closure<dyn FnMut(Event)>>>,
    }

    impl RealtimeClient {
        pub(crate) fn new() -> Self {
            let (status, set_status) = signal("connecting".to_string());
            let (last_update, set_last_update) = signal(None);
            let client = Self {
                status: status.into(),
                last_update: last_update.into(),
                inner: SendWrapper::new(Rc::new(RealtimeInner {
                    socket: RefCell::new(None),
                    handlers: RefCell::new(HashMap::new()),
                    subscription_messages: RefCell::new(HashMap::new()),
                    list_doc_subscriptions: RefCell::new(HashSet::new()),
                    pending_messages: RefCell::new(Vec::new()),
                    next_subscription_id: Cell::new(1),
                    reconnect_attempt: Cell::new(0),
                    status: status.into(),
                    last_update: last_update.into(),
                    set_status,
                    set_last_update,
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

        /// Spec section 5: subscribe to a list's document. `version` is
        /// called both for the immediate handshake and, if the socket
        /// reconnects, again at replay time — so it must read the *current*
        /// local version (typically `move || handle.version()`), not a
        /// value captured once at subscribe time. The server treats every
        /// reply on this subscription id as a (re)handshake: a fresh
        /// `ListDocSubscribed` also arrives after a `MissingHistory` resync
        /// on the update path, not only on the first reply.
        pub(crate) fn subscribe_list_doc(
            &self,
            list_id: i32,
            version: impl Fn() -> Vec<u8> + 'static,
            handler: impl Fn(ServerClient) + 'static,
        ) -> RealtimeSubscription {
            let subscription_id = self.next_subscription_id();
            self.inner
                .handlers
                .borrow_mut()
                .insert(subscription_id, Box::new(handler));
            self.inner
                .list_doc_subscriptions
                .borrow_mut()
                .insert(subscription_id);
            let factory: Rc<dyn Fn() -> ClientMessage> =
                Rc::new(move || ClientMessage::SubscribeListDoc {
                    subscription_id: Some(subscription_id),
                    list_id,
                    version: version(),
                });
            self.send_dynamic_subscription(subscription_id, factory);
            RealtimeSubscription {
                client: self.clone(),
                subscription_id,
            }
        }

        /// One local commit. Sent only if the socket is open right now;
        /// returns whether it went out. Never queued into
        /// `pending_messages` on failure — an offline edit is not lost, it
        /// just waits in the document itself, and the next handshake's
        /// `export_since` diff covers it along with everything else the
        /// server lacks (spec section 5's "no offline queue").
        pub(crate) fn send_list_doc_update(&self, list_id: i32, update: Vec<u8>) -> bool {
            let Ok(text) = serde_json::to_string(&ClientMessage::ListDocUpdate { list_id, update })
            else {
                return false;
            };
            self.send_text(&text)
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
                .insert(subscription_id, SubscriptionEntry::Static(text.clone()));
            if !self.send_text(&text) {
                self.connect();
            }
        }

        fn send_dynamic_subscription(
            &self,
            subscription_id: u64,
            factory: Rc<dyn Fn() -> ClientMessage>,
        ) {
            let message = factory();
            self.inner
                .subscription_messages
                .borrow_mut()
                .insert(subscription_id, SubscriptionEntry::Dynamic(factory));
            let Ok(text) = serde_json::to_string(&message) else {
                return;
            };
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
                    inner.set_status.set("live".to_string());
                    if let Some(socket) = inner.socket.borrow().as_ref().cloned() {
                        for entry in inner.subscription_messages.borrow().values() {
                            let text = match entry {
                                SubscriptionEntry::Static(text) => Some(text.clone()),
                                // Rebuilt now, not replayed verbatim: a
                                // list-doc subscription's version may have
                                // moved since it was first sent.
                                SubscriptionEntry::Dynamic(factory) => {
                                    serde_json::to_string(&factory()).ok()
                                }
                            };
                            if let Some(text) = text {
                                let _ = socket.send_with_str(&text);
                            }
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
            self.inner
                .list_doc_subscriptions
                .borrow_mut()
                .remove(&subscription_id);
            self.send_control(ClientMessage::Unsubscribe { subscription_id });
        }
    }

    impl RealtimeSubscription {
        /// Re-send this subscription's message, rebuilding a dynamic one
        /// from its factory (so a list-doc handshake goes out with the
        /// *current* local version). Used when the server says the
        /// subscription went `Stale`: spec section 5 wants a fresh
        /// handshake, not just a status change.
        ///
        /// Takes only `subscription_messages.borrow()`, and drops it before
        /// running the factory or touching the socket, so this is safe to
        /// call from anywhere a handler's own callbacks run — but not from
        /// inside `dispatch_message` itself, which holds
        /// `handlers.borrow()` (callers defer it; see `list_doc::sync`).
        pub(crate) fn resubscribe(&self) {
            let entry = self
                .client
                .inner
                .subscription_messages
                .borrow()
                .get(&self.subscription_id)
                .map(|entry| match entry {
                    SubscriptionEntry::Static(text) => Ok(text.clone()),
                    SubscriptionEntry::Dynamic(factory) => Err(factory.clone()),
                });
            let text = match entry {
                Some(Ok(text)) => text,
                Some(Err(factory)) => {
                    let Ok(text) = serde_json::to_string(&factory()) else {
                        return;
                    };
                    text
                }
                None => return,
            };
            if !self.client.send_text(&text) {
                self.client.connect();
            }
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
        match &message {
            ServerClient::Sales(_)
            | ServerClient::Listings(_)
            | ServerClient::ListUpdate(_)
            | ServerClient::ListDocUpdate { .. } => {
                inner.set_last_update.set(Some(Utc::now()));
            }
            _ => {}
        }
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
                // A bare Error is a broadcast: every handler on the socket
                // normally sees it, matching legacy behaviour. List-doc
                // handlers are the exception (see `list_doc_subscriptions`
                // above) — they only ever get an Error that the server
                // scoped to them via SubscriptionEvent, so a broadcast
                // meant for some other subscription attempt on the socket
                // is not confused for one about their own list.
                let list_doc_ids = inner.list_doc_subscriptions.borrow();
                for (subscription_id, handler) in inner.handlers.borrow().iter() {
                    if !list_doc_ids.contains(subscription_id) {
                        handler(message.clone());
                    }
                }
            }
            ServerClient::ListDocSubscribed {
                subscription_id, ..
            } => {
                if let Some(handler) = inner.handlers.borrow().get(&subscription_id) {
                    handler(message.clone());
                }
            }
            ServerClient::ListDocUpdate { .. } => {
                // Relayed updates normally arrive wrapped in SubscriptionEvent;
                // an unwrapped one goes to every handler like ListUpdate does.
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
        inner.set_status.set("reconnecting".to_string());
        if inner.subscription_messages.borrow().is_empty()
            && inner.pending_messages.borrow().is_empty()
        {
            inner.set_status.set("offline".to_string());
            return;
        }
        let attempt = inner.reconnect_attempt.get().saturating_add(1).min(6);
        inner.reconnect_attempt.set(attempt);
        let delay_ms = 500_u32.saturating_mul(2_u32.saturating_pow(attempt));
        let weak = Rc::downgrade(&inner);
        Timeout::new(delay_ms, move || {
            if let Some(inner) = weak.upgrade() {
                let status = inner.status;
                let last_update = inner.last_update;
                RealtimeClient {
                    status,
                    last_update,
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
    use chrono::{DateTime, Utc};

    #[derive(Clone)]
    pub(crate) struct RealtimeClient {
        pub status: Signal<String>,
        pub last_update: Signal<Option<DateTime<Utc>>>,
    }

    pub(crate) struct RealtimeSubscription;

    impl RealtimeSubscription {
        /// No socket on this half, so nothing to re-send.
        pub(crate) fn resubscribe(&self) {}
    }

    impl RealtimeClient {
        pub(crate) fn new() -> Self {
            Self {
                status: Signal::derive(|| "connecting".to_string()),
                last_update: Signal::derive(|| None),
            }
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

        pub(crate) fn subscribe_list_doc(
            &self,
            _list_id: i32,
            _version: impl Fn() -> Vec<u8> + 'static,
            _handler: impl Fn(ServerClient) + 'static,
        ) -> RealtimeSubscription {
            RealtimeSubscription
        }

        pub(crate) fn send_list_doc_update(&self, _list_id: i32, _update: Vec<u8>) -> bool {
            false
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
