use crate::api::edit_list_item;
use crate::components::icon::Icon;
use crate::ws::realtime::{RealtimeSubscription, use_realtime};
use icondata as i;
use leptos::prelude::*;
use ultros_api_types::ActiveListing;
use ultros_api_types::list::{ListItem, ListPermission, ListWithPermission};
use ultros_api_types::websocket::{EventType, FilterPredicate, ServerClient, SocketMessageType};

type ListViewResult =
    Result<(ListWithPermission, Vec<(ListItem, Vec<ActiveListing>)>), crate::error::AppError>;

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
        <details class="panel group rounded-lg">
            <summary class="flex cursor-pointer list-none items-center justify-between gap-3 p-3">
                <div class="flex min-w-0 items-center gap-2">
                    <Icon icon=i::BiPurchaseTagSolid />
                    <span class="font-bold">"Auto-mark Purchases"</span>
                    <span class="rounded-md border border-[color:var(--color-outline)] px-2 py-0.5 text-xs text-[color:var(--color-text-muted)]">
                        "Experimental"
                    </span>
                </div>
                <Icon icon=i::BiChevronDownRegular attr:class="shrink-0 transition-transform group-open:rotate-180" />
            </summary>
            <div class="border-t border-[color:var(--color-outline)] p-3">
                <div class="flex flex-col gap-2 sm:flex-row sm:items-center">
                    <input
                        class="input flex-1"
                        placeholder="Character Name"
                        prop:value=watch_character_name
                        on:input=move |e| set_watch_character_name(event_target_value(&e))
                        disabled=move || is_watching.get()
                    />
                    <button
                        class="btn-secondary sm:w-40"
                        class:bg-brand-900=move || is_watching.get()
                        disabled=move || {
                            list_view
                                .get()
                                .and_then(Result::ok)
                                .map(|(list, _)| list.permission < ListPermission::Write)
                                .unwrap_or(true)
                        }
                        on:click=move |_| set_is_watching.update(|w| *w = !*w)
                    >
                        {move || if is_watching.get() { "Watching" } else { "Start" }}
                    </button>
                </div>
            </div>
        </details>
    }
}

fn mark_item_purchased(list_view: Resource<ListViewResult>, item_id: i32) {
    list_view.update(|data: &mut Option<ListViewResult>| {
        if let Some(Ok((list, items))) = data {
            if list.permission < ListPermission::Write {
                return;
            }
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
