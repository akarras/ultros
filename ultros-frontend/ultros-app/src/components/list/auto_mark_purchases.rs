use crate::api::edit_list_item;
use crate::components::icon::Icon;
use crate::ws::realtime::{RealtimeSubscription, use_realtime};
use icondata as i;
use leptos::prelude::*;
use ultros_api_types::ActiveListing;
use ultros_api_types::list::ListItem;
use ultros_api_types::websocket::{EventType, FilterPredicate, ServerClient, SocketMessageType};

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
    let realtime = use_realtime();
    let purchase_subscription = StoredValue::new(None::<RealtimeSubscription>);

    Effect::new(move |_| {
        purchase_subscription.update_value(|sub| *sub = None);
        if !is_watching.get() {
            return;
        }
        let name = watch_character_name.get_untracked();
        if name.is_empty() {
            return;
        }
        let Some(realtime) = realtime.clone() else {
            return;
        };
        let sub = realtime.subscribe_market(
            FilterPredicate::Character(name),
            SocketMessageType::Sales,
            move |message| {
                let ServerClient::Sales(EventType::Added(event)) = message else {
                    return;
                };
                for (sale, _) in event.sales {
                    mark_item_purchased(list_view, sale.sold_item_id);
                }
            },
        );
        purchase_subscription.set_value(Some(sub));
    });
    on_cleanup(move || {
        purchase_subscription.update_value(|sub| *sub = None);
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

fn mark_item_purchased(list_view: Resource<ListViewResult>, item_id: i32) {
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
