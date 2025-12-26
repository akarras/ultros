use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use automerge::{AutoCommit, ReadDoc, sync::{State as SyncState, Message as SyncMessage}};
use futures::{StreamExt, SinkExt};
use gloo_net::websocket::{futures::WebSocket, Message};
use leptos::logging::log;
use std::rc::Rc;
use std::cell::RefCell;

#[component]
pub fn CollaborativeList() -> impl IntoView {
    let params = use_params_map();
    let list_id = move || params.get().get("id").cloned().unwrap_or_default();

    // Use a RefCell to hold the doc, wrapped in a signal to notify updates
    let doc = Rc::new(RefCell::new(AutoCommit::new()));
    let (sync_trigger, set_sync_trigger) = signal(0); // Dummy signal to force re-render

    let doc_clone = doc.clone();
    Effect::new(move |_| {
        let id = list_id();
        if id.is_empty() { return; }

        let location = window().location();
        let host = location.host().unwrap_or_else(|_| "localhost:8080".to_string());
        let protocol = if location.protocol().unwrap_or_default() == "https:" { "wss" } else { "ws" };
        let url = format!("{}://{}/ws/collaborative_list/{}", protocol, host, id);

        let ws = WebSocket::open(&url).unwrap();
        let (mut sender, mut receiver) = ws.split();

        spawn_local(async move {
            let mut sync_state = SyncState::new();

            // Loop to handle incoming messages
            while let Some(msg) = receiver.next().await {
                match msg {
                    Ok(Message::Bytes(bytes)) => {
                        let sync_msg = SyncMessage::decode(&bytes).unwrap();
                        let mut doc = doc_clone.borrow_mut();
                        doc.receive_sync_message(&mut sync_state, sync_msg).unwrap();

                        // Check if we need to reply
                        if let Some(msg) = doc.generate_sync_message(&mut sync_state) {
                            let bytes = msg.encode();
                             sender.send(Message::Bytes(bytes)).await.unwrap();
                        }

                        set_sync_trigger.update(|n| *n += 1);
                    }
                    _ => {}
                }
            }
        });
    });

    let update_name = {
        let doc = doc.clone();
        move |name: String| {
            let mut doc = doc.borrow_mut();
            doc.put(automerge::ROOT, "name", name).unwrap();
            // In a real app, we'd trigger a sync here via the websocket
            set_sync_trigger.update(|n| *n += 1);
        }
    };

    let name = move || {
        sync_trigger.get(); // Depend on trigger
        let doc = doc.borrow();
        doc.get(automerge::ROOT, "name").unwrap().map(|(v, _)| v.to_string()).unwrap_or_default()
    };

    view! {
        <div class="p-4">
            <h1 class="text-2xl font-bold mb-4">"Collaborative List: " {list_id}</h1>
            <div class="mb-4">
                <label class="block mb-2">"List Name"</label>
                <input
                    type="text"
                    class="border p-2 rounded w-full"
                    prop:value=name
                    on:input=move |ev| {
                        update_name(event_target_value(&ev));
                    }
                />
            </div>
            <div>
                <p>"Current Value from CRDT: " {name}</p>
            </div>
        </div>
    }
}
