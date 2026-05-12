use icondata as i;
use leptos::{prelude::*, task::spawn_local};
use ultros_api_types::alert::{CreateEndpointRequest, Endpoint, EndpointMethod};

use crate::api::{create_endpoint, delete_endpoint, list_endpoints, test_endpoint};
use crate::components::icon::Icon;
use crate::global_state::toasts::use_toast;

#[component]
pub fn EndpointsPanel() -> impl IntoView {
    let version = RwSignal::new(0u64);
    let endpoints = Resource::new(move || version.get(), move |_| list_endpoints());
    let toasts = use_toast();
    let (show_form, set_show_form) = signal(false);

    let on_delete = move |id: i32| {
        spawn_local(async move {
            match delete_endpoint(id).await {
                Ok(()) => {
                    if let Some(t) = toasts {
                        t.success("Endpoint deleted");
                    }
                    version.update(|v| *v += 1);
                }
                Err(e) => {
                    if let Some(t) = toasts {
                        t.error(format!("{e}"));
                    }
                }
            }
        });
    };

    let on_test = move |id: i32| {
        spawn_local(async move {
            match test_endpoint(id).await {
                Ok(r) if r.delivered => {
                    if let Some(t) = toasts {
                        t.success("Test delivered");
                    }
                }
                Ok(r) => {
                    if let Some(t) = toasts {
                        t.error(r.error.unwrap_or_else(|| "Delivery failed".into()));
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
        <div class="space-y-4">
            <div class="flex justify-between items-center">
                <h2 class="text-lg font-semibold">"Endpoints"</h2>
                <button class="btn" on:click=move |_| set_show_form.update(|v| *v = !*v)>
                    {move || if show_form.get() { "Cancel" } else { "Add endpoint" }}
                </button>
            </div>

            <Show when=move || show_form.get()>
                <EndpointCreateForm
                    on_created=Callback::new(move |_| {
                        set_show_form.set(false);
                        version.update(|v| *v += 1);
                    })
                />
            </Show>

            <Suspense fallback=move || view! { <div>"Loading..."</div> }>
                {move || endpoints.get().map(|r| match r {
                    Ok(rows) if rows.is_empty() => view! {
                        <p class="opacity-70">"No endpoints yet. Add one to receive alerts."</p>
                    }.into_any(),
                    Ok(rows) => view! {
                        <ul class="divide-y">
                            <For
                                each=move || rows.clone()
                                key=|e| e.id
                                children=move |e: Endpoint| {
                                    let label = match &e.method {
                                        EndpointMethod::DiscordDm { .. } => "Discord DM",
                                        EndpointMethod::DiscordChannel { .. } => "Discord Channel",
                                        EndpointMethod::Webhook { .. } => "Webhook",
                                    };
                                    let id = e.id;
                                    view! {
                                        <li class="flex items-center justify-between py-2">
                                            <div>
                                                <div class="font-medium">{e.name.clone()}</div>
                                                <div class="text-xs opacity-70">{label}</div>
                                            </div>
                                            <div class="flex gap-1">
                                                <button class="btn-ghost" on:click=move |_| on_test(id)>
                                                    <Icon icon=i::BsSend />
                                                    <span class="ml-1">"Test"</span>
                                                </button>
                                                <button
                                                    class="btn-ghost text-red-400"
                                                    on:click=move |_| on_delete(id)
                                                >
                                                    <Icon icon=i::BiTrashSolid />
                                                </button>
                                            </div>
                                        </li>
                                    }
                                }
                            />
                        </ul>
                    }.into_any(),
                    Err(e) => view! { <div class="text-red-500">{format!("{e}")}</div> }.into_any(),
                })}
            </Suspense>
        </div>
    }
}

#[component]
fn EndpointCreateForm(#[prop(into)] on_created: Callback<()>) -> impl IntoView {
    let (name, set_name) = signal::<String>("".into());
    let (method_kind, set_method_kind) = signal::<&'static str>("discord_dm");
    let (channel_id, set_channel_id) = signal::<String>("".into());
    let (webhook_url, set_webhook_url) = signal::<String>("".into());
    let (error, set_error) = signal::<Option<String>>(None);
    let toasts = use_toast();

    let submit = move |_| {
        set_error.set(None);
        let n = name.get();
        if n.trim().is_empty() {
            set_error.set(Some("Name is required".into()));
            return;
        }
        let method = match method_kind.get() {
            "discord_channel" => {
                let Ok(cid) = channel_id.get().parse::<i64>() else {
                    set_error.set(Some("Channel ID must be a number".into()));
                    return;
                };
                EndpointMethod::DiscordChannel { channel_id: cid }
            }
            "webhook" => {
                let url = webhook_url.get();
                if url.trim().is_empty() {
                    set_error.set(Some("Webhook URL required".into()));
                    return;
                }
                EndpointMethod::Webhook { url }
            }
            // DiscordDm uses the *current user's* discord id. We pass 0 here and let the
            // server fill it in if it sees method=DiscordDm with user_id=0; OR we read
            // the current user via a context. Simpler: pass 0; backend rewrites to user.id.
            _ => EndpointMethod::DiscordDm { user_id: 0 },
        };
        let req = CreateEndpointRequest { name: n, method };
        spawn_local(async move {
            match create_endpoint(req).await {
                Ok(_) => {
                    if let Some(t) = toasts {
                        t.success("Endpoint created");
                    }
                    on_created.run(());
                }
                Err(e) => set_error.set(Some(format!("{e}"))),
            }
        });
    };

    view! {
        <div class="p-3 border rounded space-y-3">
            <div class="space-y-1">
                <label class="text-sm font-semibold">"Name"</label>
                <input class="input w-full" prop:value=name
                    on:input=move |e| set_name.set(event_target_value(&e)) />
            </div>
            <div class="space-y-1">
                <label class="text-sm font-semibold">"Method"</label>
                <select class="input w-full" prop:value=method_kind
                    on:change=move |e| {
                        let v = event_target_value(&e);
                        set_method_kind.set(match v.as_str() {
                            "discord_channel" => "discord_channel",
                            "webhook" => "webhook",
                            _ => "discord_dm",
                        });
                    }>
                    <option value="discord_dm">"Discord DM (me)"</option>
                    <option value="discord_channel">"Discord channel"</option>
                    <option value="webhook">"Webhook URL"</option>
                </select>
            </div>
            <Show when=move || method_kind.get() == "discord_channel">
                <div class="space-y-1">
                    <label class="text-sm font-semibold">"Channel ID"</label>
                    <input class="input w-full" prop:value=channel_id
                        on:input=move |e| set_channel_id.set(event_target_value(&e)) />
                </div>
            </Show>
            <Show when=move || method_kind.get() == "webhook">
                <div class="space-y-1">
                    <label class="text-sm font-semibold">"Webhook URL"</label>
                    <input class="input w-full" prop:value=webhook_url
                        on:input=move |e| set_webhook_url.set(event_target_value(&e)) />
                </div>
            </Show>
            <Show when=move || error.get().is_some()>
                <div class="text-sm text-red-500">{move || error.get().unwrap_or_default()}</div>
            </Show>
            <div class="flex justify-end">
                <button class="btn" on:click=submit>"Create"</button>
            </div>
        </div>
    }
}
