use icondata as i;
use leptos::{prelude::*, task::spawn_local};
use ultros_api_types::alert::AlertEvent;
use xiv_gen::ItemId;

use crate::api::{get_alert_events, resend_alert_event};
use crate::components::icon::Icon;
use crate::components::loading::Loading;
use crate::global_state::toasts::use_toast;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::{t, t_string, use_i18n};

#[component]
pub fn HistoryPanel() -> impl IntoView {
    let i18n = use_i18n();
    let version = RwSignal::new(0u64);
    let events = Resource::new(move || version.get(), move |_| get_alert_events());
    let toasts = use_toast();

    view! {
        <Suspense fallback=move || view! { <div>{t!(i18n, loading)}</div> }>
            {move || events.get().map(|r| match r {
                Ok(rows) if rows.is_empty() => view! {
                    <p class="opacity-70">{t!(i18n, history_no_fires)}</p>
                }.into_any(),
                Ok(rows) => view! {
                    <div class="overflow-x-auto">
                        <table class="w-full text-sm">
                            <thead>
                                <tr>
                                    <th scope="col" class="text-left p-1">{t!(i18n, col_time)}</th>
                                    <th scope="col" class="text-left p-1">{t!(i18n, item)}</th>
                                    <th scope="col" class="text-left p-1">{t!(i18n, history_col_matched_price)}</th>
                                    <th scope="col" class="text-left p-1">{t!(i18n, history_col_delivered)}</th>
                                    <th scope="col" class="text-left p-1">{t!(i18n, actions)}</th>
                                </tr>
                            </thead>
                            <tbody>
                                // ⚡ Bolt Optimization: Using collect_view() instead of <For> to prevent unnecessary cloning of rows inside a conditional block that completely recreates the view.
                                {rows.into_iter().map(|e: AlertEvent| {
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
                                        let label_item_name = item_name.clone();
                                        let (is_resending, set_is_resending) = RwSignal::new(false).split();
                                        view! {
                                            <tr class="border-t">
                                                <td class="p-1">{fired_str}</td>
                                                <td class="p-1">{item_name}</td>
                                                <td class="p-1">{price_str}</td>
                                                <td class="p-1">{delivered_str}</td>
                                                <td class="p-1">
                                                    <Show when=move || !delivered>
                                                        {
                                                            let label = label_item_name.clone();
                                                            view! {
                                                                <button
                                                                    class="btn-ghost"
                                                                    aria-label=move || format!("{} {}", t_string!(i18n, history_resend_button), label)
                                                                    disabled=move || is_resending.get()
                                                                    on:click=move |_| {
                                                                        set_is_resending.set(true);
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
                                                                                    set_is_resending.set(false);
                                                                                }
                                                                                Err(e) => {
                                                                                    if let Some(t) = toasts {
                                                                                        t.error(format!("{e}"));
                                                                                    }
                                                                                    set_is_resending.set(false);
                                                                                }
                                                                            }
                                                                        });
                                                                    }
                                                                >
                                                                    <Show when=move || !is_resending.get() fallback=|| view! { <Loading /> }>
                                                                        <Icon icon=i::BsArrowRepeat />
                                                                        <span class="ml-1">{t!(i18n, history_resend_button)}</span>
                                                                    </Show>
                                                                </button>
                                                            }
                                                        }
                                                    </Show>
                                                </td>
                                            </tr>
                                        }
                                    }).collect_view()}
                            </tbody>
                        </table>
                    </div>
                }.into_any(),
                Err(e) => view! { <div class="text-red-500">{format!("{e}")}</div> }.into_any(),
            })}
        </Suspense>
    }
}
