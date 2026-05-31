use icondata as i;
use leptos::{prelude::*, task::spawn_local};
use ultros_api_types::alert::{CreateEndpointRequest, Endpoint, EndpointMethod};

use crate::api::{
    create_endpoint, delete_endpoint, list_discord_writable_guilds, list_endpoints, test_endpoint,
};
use crate::components::icon::Icon;
use crate::components::push_subscribe::enable_browser_notifications;
use crate::global_state::toasts::use_toast;
use crate::i18n::{t, t_string, use_i18n};

#[component]
pub fn EndpointsPanel() -> impl IntoView {
    let i18n = use_i18n();
    let version = RwSignal::new(0u64);
    let endpoints = Resource::new(move || version.get(), move |_| list_endpoints());
    let toasts = use_toast();
    let (show_form, set_show_form) = signal(false);

    let on_delete = move |id: i32| {
        spawn_local(async move {
            match delete_endpoint(id).await {
                Ok(()) => {
                    if let Some(t) = toasts {
                        t.success(t_string!(i18n, endpoints_deleted_toast));
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
                        t.success(t_string!(i18n, endpoints_test_delivered_toast));
                    }
                }
                Ok(r) => {
                    if let Some(t) = toasts {
                        t.error(r.error.unwrap_or_else(|| {
                            t_string!(i18n, endpoints_delivery_failed).to_string()
                        }));
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

    let on_enable_push = move |_| {
        spawn_local(async move {
            match enable_browser_notifications().await {
                Ok(_endpoint) => {
                    if let Some(t) = toasts {
                        t.success(t_string!(i18n, endpoints_browser_push_enabled_toast));
                    }
                    version.update(|v| *v += 1);
                }
                Err(msg) => {
                    if let Some(t) = toasts {
                        t.error(msg);
                    }
                }
            }
        });
    };

    view! {
        <div class="space-y-4">
            <div class="flex justify-between items-center">
                <h2 class="text-lg font-semibold">{t!(i18n, endpoints_heading)}</h2>
                <div class="flex gap-2">
                    <button class="btn" on:click=on_enable_push
                            title=t_string!(i18n, endpoints_enable_browser_push_title)>
                        {t!(i18n, endpoints_enable_browser_push)}
                    </button>
                    <button class="btn" on:click=move |_| set_show_form.update(|v| *v = !*v)>
                        {move || if show_form.get() {
                            t_string!(i18n, cancel).to_string()
                        } else {
                            t_string!(i18n, endpoints_add_endpoint).to_string()
                        }}
                    </button>
                </div>
            </div>

            <Show when=move || show_form.get()>
                <EndpointCreateForm
                    on_created=Callback::new(move |_| {
                        set_show_form.set(false);
                        version.update(|v| *v += 1);
                    })
                />
            </Show>

            <Suspense fallback=move || view! { <div>{t!(i18n, loading)}</div> }>
                {move || endpoints.get().map(|r| match r {
                    Ok(rows) if rows.is_empty() => view! {
                        <p class="opacity-70">{t!(i18n, endpoints_empty_state)}</p>
                    }.into_any(),
                    Ok(rows) => view! {
                        <ul class="divide-y">
                            // ⚡ Bolt Optimization: Using collect_view() instead of <For> to prevent unnecessary cloning of rows inside a conditional block that completely recreates the view.
                            {rows.into_iter().map(|e: Endpoint| {
                                    // Sub-label shows the method, plus enough context for
                                    // Discord channels to identify which server/channel it is.
                                    // Falls back to the raw id for legacy rows that pre-date
                                    // channel-name resolution.
                                    let label: String = match &e.method {
                                        EndpointMethod::DiscordDm { .. } => {
                                            t_string!(i18n, endpoints_method_discord_dm).to_string()
                                        }
                                        EndpointMethod::DiscordChannel {
                                            channel_id,
                                            channel_name,
                                            guild_name,
                                            ..
                                        } => match (channel_name, guild_name) {
                                            (Some(cn), Some(gn)) => {
                                                t_string!(
                                                    i18n,
                                                    endpoints_method_discord_channel_in_guild,
                                                    channel = cn,
                                                    guild = gn,
                                                )
                                                .to_string()
                                            }
                                            (Some(cn), None) => {
                                                t_string!(
                                                    i18n,
                                                    endpoints_method_discord_channel,
                                                    channel = cn,
                                                )
                                                .to_string()
                                            }
                                            _ => t_string!(
                                                i18n,
                                                endpoints_method_discord_channel_id,
                                                channel_id = channel_id,
                                            )
                                            .to_string(),
                                        },
                                        EndpointMethod::Webhook { .. } => {
                                            t_string!(i18n, endpoints_method_webhook).to_string()
                                        }
                                        EndpointMethod::WebPush { .. } => {
                                            t_string!(i18n, endpoints_method_web_push).to_string()
                                        }
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
                                                    <span class="ml-1">{t!(i18n, endpoints_test_button)}</span>
                                                </button>
                                                <button
                                                    class="btn-ghost text-red-400"
                                                    aria-label=t_string!(i18n, endpoints_delete_aria)
                                                    on:click=move |_| on_delete(id)
                                                >
                                                    <Icon icon=i::BiTrashSolid />
                                                </button>
                                            </div>
                                        </li>
                                    }
                            }).collect_view()}
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
    let i18n = use_i18n();
    let (name, set_name) = signal::<String>("".into());
    let (method_kind, set_method_kind) = signal::<&'static str>("discord_dm");
    let guilds = Resource::new(|| (), |_| list_discord_writable_guilds());
    let (selected_guild_id, set_selected_guild_id) = signal::<Option<i64>>(None);
    let (selected_channel_id, set_selected_channel_id) = signal::<Option<i64>>(None);
    let (webhook_url, set_webhook_url) = signal::<String>("".into());
    let (error, set_error) = signal::<Option<String>>(None);
    let toasts = use_toast();

    let choose_channel = move || {
        let guilds = guilds.get()?.ok()?;
        let selected_guild = selected_guild_id.get();
        let guild = selected_guild
            .and_then(|id| guilds.iter().find(|g| g.id == id))
            .or_else(|| guilds.iter().find(|g| !g.channels.is_empty()))?;
        let selected_channel = selected_channel_id.get();
        selected_channel
            .and_then(|id| guild.channels.iter().find(|c| c.id == id))
            .or_else(|| guild.channels.first())
            .map(|c| c.id)
    };

    let submit = move |_| {
        set_error.set(None);
        let n = name.get();
        let kind = method_kind.get();
        // For DiscordChannel the server replaces an empty/stub name with the
        // resolved "#channel (guild)" string after looking it up — so we don't
        // require the user to have typed anything.
        if n.trim().is_empty() && kind != "discord_channel" {
            set_error.set(Some(
                t_string!(i18n, endpoints_err_name_required).to_string(),
            ));
            return;
        }
        let method = match kind {
            "discord_channel" => {
                let Some(cid) = choose_channel() else {
                    set_error.set(Some(
                        t_string!(i18n, endpoints_err_channel_required).to_string(),
                    ));
                    return;
                };
                EndpointMethod::DiscordChannel {
                    channel_id: cid,
                    channel_name: None,
                    guild_id: None,
                    guild_name: None,
                }
            }
            "webhook" => {
                let url = webhook_url.get();
                if url.trim().is_empty() {
                    set_error.set(Some(
                        t_string!(i18n, alert_drawer_err_webhook_required).to_string(),
                    ));
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
                        t.success(t_string!(i18n, endpoints_created_toast));
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
                <label class="text-sm font-semibold" for="endpoint-name">{t!(i18n, endpoints_name_label)}</label>
                <input id="endpoint-name" class="input w-full" prop:value=name
                    on:input=move |e| set_name.set(event_target_value(&e)) />
            </div>
            <div class="space-y-1">
                <label class="text-sm font-semibold" for="endpoint-method">{t!(i18n, endpoints_method_label)}</label>
                <select id="endpoint-method" class="input w-full" prop:value=method_kind
                    on:change=move |e| {
                        let v = event_target_value(&e);
                        set_method_kind.set(match v.as_str() {
                            "discord_channel" => "discord_channel",
                            "webhook" => "webhook",
                            _ => "discord_dm",
                        });
                    }>
                    <option value="discord_dm">{t!(i18n, endpoints_discord_dm_me)}</option>
                    <option value="discord_channel">{t!(i18n, endpoints_discord_channel)}</option>
                    <option value="webhook">{t!(i18n, endpoints_webhook_url)}</option>
                </select>
            </div>
            <Show when=move || method_kind.get() == "discord_channel">
                <Suspense fallback=move || view! {
                    <div class="text-sm opacity-70">{t!(i18n, endpoints_loading_discord_servers)}</div>
                }>
                    {move || guilds.get().map(|r| match r {
                        Ok(guilds) if guilds.is_empty() => view! {
                            <div class="rounded border border-[color:var(--color-outline)] p-3 text-sm opacity-80">
                                {t!(i18n, endpoints_no_discord_servers)}
                            </div>
                        }.into_any(),
                        Ok(guilds) => {
                            let selected_guild = selected_guild_id
                                .get()
                                .and_then(|id| guilds.iter().find(|g| g.id == id))
                                .or_else(|| guilds.iter().find(|g| !g.channels.is_empty()))
                                .or_else(|| guilds.first())
                                .cloned();
                            let selected_id = selected_guild.as_ref().map(|g| g.id);
                            let selected_channels = selected_guild
                                .as_ref()
                                .map(|g| g.channels.clone())
                                .unwrap_or_default();
                            let default_channel_id = selected_channels.first().map(|c| c.id);
                            view! {
                                <div class="space-y-3">
                                    <div class="grid gap-2 sm:grid-cols-2">
                                        {guilds.into_iter().map(|guild| {
                                            let gid = guild.id;
                                            let disabled = guild.channels.is_empty();
                                            let selected = move || selected_guild_id.get().or(selected_id) == Some(gid);
                                            let icon_url = guild.icon_url.clone();
                                            let guild_name = guild.name.clone();
                                            view! {
                                                <button
                                                    type="button"
                                                    class="flex items-center gap-2 rounded border border-[color:var(--color-outline)] p-2 text-left hover:bg-[color:var(--color-background-panel)] disabled:opacity-50"
                                                    class:border-brand-400=selected
                                                    disabled=disabled
                                                    on:click=move |_| {
                                                        set_selected_guild_id.set(Some(gid));
                                                        set_selected_channel_id.set(None);
                                                    }
                                                >
                                                    {match icon_url {
                                                        Some(url) => view! {
                                                            <img src=url class="h-8 w-8 rounded object-cover" alt="" />
                                                        }.into_any(),
                                                        None => view! {
                                                            <span class="flex h-8 w-8 shrink-0 items-center justify-center rounded bg-[color:var(--color-background-panel)] text-xs font-bold">
                                                                {guild_name.chars().next().unwrap_or('?').to_string()}
                                                            </span>
                                                        }.into_any(),
                                                    }}
                                                    <span class="min-w-0 truncate font-medium">{guild.name}</span>
                                                </button>
                                            }
                                        }).collect_view()}
                                    </div>

                                    <div class="space-y-1">
                                        <label class="text-sm font-semibold" for="endpoint-channel-select">
                                            {t!(i18n, endpoints_channel_label)}
                                        </label>
                                        <select
                                            id="endpoint-channel-select"
                                            class="input w-full"
                                            prop:value=move || {
                                                selected_channel_id
                                                    .get()
                                                    .or(default_channel_id)
                                                    .map(|id| id.to_string())
                                                    .unwrap_or_default()
                                            }
                                            on:change=move |e| {
                                                set_selected_channel_id.set(event_target_value(&e).parse().ok());
                                            }
                                        >
                                            {selected_channels.into_iter().map(|channel| {
                                                view! {
                                                    <option value=channel.id.to_string()>
                                                        {format!("#{}", channel.name)}
                                                    </option>
                                                }
                                            }).collect_view()}
                                        </select>
                                    </div>
                                </div>
                            }.into_any()
                        }
                        Err(e) => view! {
                            <div class="text-red-500">{format!("{e}")}</div>
                        }.into_any(),
                    })}
                </Suspense>
            </Show>
            <Show when=move || method_kind.get() == "webhook">
                <div class="space-y-1">
                    <label class="text-sm font-semibold" for="endpoint-webhook-url">{t!(i18n, alert_drawer_webhook_url_label)}</label>
                    <input id="endpoint-webhook-url" class="input w-full" prop:value=webhook_url
                        on:input=move |e| set_webhook_url.set(event_target_value(&e)) />
                </div>
            </Show>
            <Show when=move || error.get().is_some()>
                <div class="text-sm text-red-500">{move || error.get().unwrap_or_default()}</div>
            </Show>
            <div class="flex justify-end">
                <button class="btn" on:click=submit>{t!(i18n, endpoints_create_button)}</button>
            </div>
        </div>
    }
}
