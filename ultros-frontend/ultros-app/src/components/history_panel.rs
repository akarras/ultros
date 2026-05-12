use icondata as i;
use leptos::{prelude::*, task::spawn_local};
use ultros_api_types::alert::AlertEvent;
use xiv_gen::ItemId;

use crate::api::{get_alert_events, resend_alert_event};
use crate::components::icon::Icon;
use crate::global_state::toasts::use_toast;
use crate::global_state::xiv_data::tracked_data;

#[component]
pub fn HistoryPanel() -> impl IntoView {
    let version = RwSignal::new(0u64);
    let events = Resource::new(move || version.get(), move |_| get_alert_events());
    let toasts = use_toast();

    let resend = move |event_id: i64| {
        spawn_local(async move {
            match resend_alert_event(event_id).await {
                Ok(r) if r.delivered => {
                    if let Some(t) = toasts {
                        t.success("Resent");
                    }
                    version.update(|v| *v += 1);
                }
                Ok(r) => {
                    if let Some(t) = toasts {
                        t.error(r.error.unwrap_or_else(|| "Resend failed".into()));
                    }
                }
                Err(e) => {
                    if let Some(t) = toasts {
                        t.error(format!("{e}"));
                    }
                }
            }
        });
    };

    view! {
        <Suspense fallback=move || view! { <div>"Loading..."</div> }>
            {move || events.get().map(|r| match r {
                Ok(rows) if rows.is_empty() => view! {
                    <p class="opacity-70">"No fires yet."</p>
                }.into_any(),
                Ok(rows) => view! {
                    <div class="overflow-x-auto">
                        <table class="w-full text-sm">
                            <thead>
                                <tr>
                                    <th class="text-left p-1">"Time"</th>
                                    <th class="text-left p-1">"Item"</th>
                                    <th class="text-left p-1">"Matched price"</th>
                                    <th class="text-left p-1">"Delivered"</th>
                                    <th class="text-left p-1">"Actions"</th>
                                </tr>
                            </thead>
                            <tbody>
                                <For each=move || rows.clone() key=|e| e.id
                                    children=move |e: AlertEvent| {
                                        let item_name = tracked_data().items.get(&ItemId(e.item_id))
                                            .map(|it| it.name.as_str().to_string())
                                            .unwrap_or_else(|| format!("Item {}", e.item_id));
                                        let fired_str = e.fired_at.to_rfc3339();
                                        let price_str = e.matched_price.map(|p| p.to_string()).unwrap_or_else(|| "\u{2014}".into());
                                        let delivered_str = if e.delivered {
                                            "\u{2713}".to_string()
                                        } else {
                                            e.delivery_error.as_deref().unwrap_or("\u{2717}").to_string()
                                        };
                                        let event_id = e.id;
                                        let delivered = e.delivered;
                                        view! {
                                            <tr class="border-t">
                                                <td class="p-1">{fired_str}</td>
                                                <td class="p-1">{item_name}</td>
                                                <td class="p-1">{price_str}</td>
                                                <td class="p-1">{delivered_str}</td>
                                                <td class="p-1">
                                                    <Show when=move || !delivered>
                                                        <button class="btn-ghost" on:click=move |_| resend(event_id)>
                                                            <Icon icon=i::BsArrowRepeat />
                                                            <span class="ml-1">"Resend"</span>
                                                        </button>
                                                    </Show>
                                                </td>
                                            </tr>
                                        }
                                    }
                                />
                            </tbody>
                        </table>
                    </div>
                }.into_any(),
                Err(e) => view! { <div class="text-red-500">{format!("{e}")}</div> }.into_any(),
            })}
        </Suspense>
    }
}
