use icondata as i;
use leptos::{prelude::*, reactive::wrappers::write::SignalSetter, task::spawn_local};
use ultros_api_types::{
    alert::{AlertDelivery, AlertTrigger, CreateAlertRequest},
    world_helper::AnySelector,
};

use crate::api::create_alert;
use crate::components::{icon::Icon, modal::Modal, world_picker::WorldPicker};
use crate::global_state::toasts::use_toast;

#[component]
pub fn AlertConfigDrawer(
    item_id: i32,
    item_name: String,
    /// Default world selector for the form (e.g., from the user's home world). If None, user must pick.
    #[prop(into)]
    default_world: Signal<Option<AnySelector>>,
    set_visible: SignalSetter<bool>,
) -> impl IntoView {
    let (world, set_world) = signal::<Option<AnySelector>>(default_world.get_untracked());
    let (price_threshold, set_price_threshold) = signal::<String>("".to_string());
    let (hq_only, set_hq_only) = signal(false);
    let (delivery_kind, set_delivery_kind) = signal::<&'static str>("discord_dm");
    let (webhook_url, set_webhook_url) = signal::<String>("".to_string());
    let (error, set_error) = signal::<Option<String>>(None);
    let toasts = use_toast();

    let submit = move |_| {
        set_error.set(None);
        let Some(world_selector) = world.get() else {
            set_error.set(Some("Pick a world or DC".into()));
            return;
        };
        let Ok(threshold) = price_threshold.get().parse::<i32>() else {
            set_error.set(Some("Price threshold must be a positive integer".into()));
            return;
        };
        if threshold <= 0 {
            set_error.set(Some("Price threshold must be positive".into()));
            return;
        }
        let delivery = match delivery_kind.get() {
            "webhook" => {
                let url = webhook_url.get();
                if url.trim().is_empty() {
                    set_error.set(Some("Webhook URL required".into()));
                    return;
                }
                AlertDelivery::Webhook { url }
            }
            _ => AlertDelivery::DiscordDm,
        };
        let req = CreateAlertRequest {
            trigger: AlertTrigger::BelowThreshold {
                item_id,
                world_selector,
                price_threshold: threshold,
                hq_only: hq_only.get(),
            },
            delivery,
            cooldown_seconds: None,
        };
        spawn_local(async move {
            match create_alert(req).await {
                Ok(_) => {
                    if let Some(t) = toasts {
                        t.success("Alert created");
                    }
                    set_visible.set(false);
                }
                Err(e) => {
                    set_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    view! {
        <Modal set_visible>
            <div class="p-4 space-y-4 w-[28rem]">
                <h2 class="text-xl font-bold">"Create price alert: " {item_name.clone()}</h2>

                <div class="space-y-1">
                    <label class="text-sm font-semibold">"World / DC / Region"</label>
                    <WorldPicker
                        current_world=world.into()
                        set_current_world=set_world.into()
                    />
                </div>

                <div class="space-y-1">
                    <label class="text-sm font-semibold">"Price threshold (gil)"</label>
                    <input
                        class="input w-full"
                        type="number"
                        min="1"
                        placeholder="e.g. 150000"
                        prop:value=price_threshold
                        on:input=move |e| set_price_threshold.set(event_target_value(&e))
                    />
                </div>

                <label class="flex items-center gap-2">
                    <input
                        type="checkbox"
                        prop:checked=hq_only
                        on:change=move |e| set_hq_only.set(event_target_checked(&e))
                    />
                    <span class="text-sm">"HQ only"</span>
                </label>

                <div class="space-y-1">
                    <label class="text-sm font-semibold">"Delivery"</label>
                    <div class="flex gap-3">
                        <label class="flex items-center gap-1">
                            <input
                                type="radio"
                                name="delivery"
                                prop:checked=move || delivery_kind.get() == "discord_dm"
                                on:change=move |_| set_delivery_kind.set("discord_dm")
                            />
                            "Discord DM"
                        </label>
                        <label class="flex items-center gap-1">
                            <input
                                type="radio"
                                name="delivery"
                                prop:checked=move || delivery_kind.get() == "webhook"
                                on:change=move |_| set_delivery_kind.set("webhook")
                            />
                            "Webhook"
                        </label>
                    </div>
                </div>

                <Show when=move || delivery_kind.get() == "webhook">
                    <div class="space-y-1">
                        <label class="text-sm font-semibold">"Discord webhook URL"</label>
                        <input
                            class="input w-full"
                            type="url"
                            placeholder="https://discord.com/api/webhooks/..."
                            prop:value=webhook_url
                            on:input=move |e| set_webhook_url.set(event_target_value(&e))
                        />
                        <p class="text-xs opacity-70">
                            "Get a webhook URL from a channel's Integrations settings in Discord."
                        </p>
                    </div>
                </Show>

                <Show when=move || error.get().is_some()>
                    <div class="text-sm text-red-500">{move || error.get().unwrap_or_default()}</div>
                </Show>

                <div class="flex justify-end gap-2 pt-2">
                    <button class="btn-ghost" on:click=move |_| set_visible.set(false)>
                        "Cancel"
                    </button>
                    <button class="btn" on:click=submit>
                        <Icon icon=i::BsBell width="1em" height="1em" />
                        <span class="ml-1">"Create alert"</span>
                    </button>
                </div>
            </div>
        </Modal>
    }
}
