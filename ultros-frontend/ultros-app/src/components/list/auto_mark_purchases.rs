use crate::api::edit_list_item;
use crate::components::icon::Icon;
use icondata as i;
use leptos::prelude::*;
use ultros_api_types::ActiveListing;
use ultros_api_types::list::ListItem;

type ListViewResult = Result<
    (
        ultros_api_types::list::List,
        Vec<(ListItem, Vec<ActiveListing>)>,
    ),
    crate::error::AppError,
>;

#[component]
pub fn AutoMarkPurchases(list_view: Resource<ListViewResult>) -> impl IntoView {
    let (watch_character_name, set_watch_character_name) = signal("".to_string());
    let (is_watching, set_is_watching) = signal(false);

    Effect::new(move |_| {
        use leptos::leptos_dom::helpers::location;
        use leptos::wasm_bindgen::JsCast;
        use ultros_api_types::websocket::{AlertsRx, AlertsTx};
        use web_sys::{MessageEvent, WebSocket};

        if is_watching.get() {
            let name = watch_character_name.get_untracked();
            if name.is_empty() {
                return;
            }

            let protocol = if location().protocol().unwrap() == "https:" {
                "wss"
            } else {
                "ws"
            };
            let host = location().host().unwrap();
            let url = format!("{protocol}://{host}/alerts/websocket");

            if let Ok(ws) = WebSocket::new(&url) {
                let name = name.clone();
                let ws_for_open = ws.clone();
                let onopen_callback =
                    leptos::wasm_bindgen::closure::Closure::wrap(Box::new(move || {
                        let msg = AlertsRx::WatchCharacter { name: name.clone() };
                        match serde_json::to_string(&msg) {
                            Ok(text) => {
                                let _ = ws_for_open.send_with_str(&text);
                            }
                            Err(e) => {
                                leptos::logging::error!(
                                    "Failed to serialize WatchCharacter: {:?}",
                                    e
                                );
                            }
                        }
                    })
                        as Box<dyn FnMut()>);
                ws.set_onopen(Some(onopen_callback.as_ref().unchecked_ref()));
                onopen_callback.forget();

                let onmessage_callback =
                    leptos::wasm_bindgen::closure::Closure::wrap(Box::new(move |e: MessageEvent| {
                        if let Ok(txt) = e.data().dyn_into::<web_sys::js_sys::JsString>() {
                            let txt: String = txt.into();
                            if let Ok(AlertsTx::ItemPurchased { item_id }) =
                                serde_json::from_str::<AlertsTx>(&txt)
                            {
                                list_view.update(|data: &mut Option<ListViewResult>| {
                                    if let Some(Ok((_, items))) = data {
                                        for (item, _) in items.iter_mut() {
                                            if item.item_id == item_id {
                                                let q = item.quantity.unwrap_or(1);
                                                let current = item.acquired.unwrap_or(0);
                                                if current < q {
                                                    item.acquired = Some(current + 1);
                                                    let item_clone = item.clone();
                                                    leptos::task::spawn_local(async move {
                                                        let _ = edit_list_item(item_clone).await;
                                                    });
                                                }
                                            }
                                        }
                                    }
                                });
                            }
                        }
                    })
                        as Box<dyn FnMut(MessageEvent)>);
                ws.set_onmessage(Some(onmessage_callback.as_ref().unchecked_ref()));
                onmessage_callback.forget();

                let ws_clone = send_wrapper::SendWrapper::new(ws.clone());
                on_cleanup(move || {
                    let _ = ws_clone.close();
                });
            }
        }
    });

    view! {
        <div class="flex-row">
            <details class="content-well group w-full mb-4">
                <summary class="flex items-center justify-between p-4 cursor-pointer list-none">
                    <div class="flex items-center gap-2">
                         <Icon icon=i::BiPurchaseTagSolid />
                         <span class="font-bold">"Auto-mark Purchases"</span>
                         <span class="text-xs text-[color:var(--color-text-muted)] ml-2">"Experimental"</span>
                    </div>
                    <Icon icon=i::BiChevronDownRegular attr:class="transition-transform group-open:rotate-180" />
                </summary>
                <div class="p-4 pt-0 border-t border-white/5 mt-2 pt-4 flex flex-col gap-3">
                    <p class="text-sm text-[color:var(--color-text-muted)]">
                        "Enter your character name below. When you purchase an item on the market board, it will automatically be marked as acquired in this list."
                    </p>
                    <div class="join w-full max-w-md">
                        <input
                            class="input input-bordered join-item flex-1"
                            placeholder="Character Name"
                            prop:value=watch_character_name
                            on:input=move |e| set_watch_character_name(event_target_value(&e))
                            disabled=move || is_watching.get()
                        />
                        <button
                            class="btn join-item"
                            class:btn-success=move || is_watching.get()
                            on:click=move |_| set_is_watching.update(|w| *w = !*w)
                        >
                            {move || if is_watching.get() { "Watching..." } else { "Start Watching" }}
                        </button>
                    </div>
                </div>
            </details>
        </div>
    }
}
