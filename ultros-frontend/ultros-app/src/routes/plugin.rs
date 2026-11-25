use crate::global_state::labs::use_dalamud_labs;
use crate::i18n::*;
use browser::setup_available;
use leptos::prelude::*;
use leptos_i18n::I18nContext;
use leptos_meta::Title;
use leptos_router::{components::Outlet, hooks::use_location};
use ultros_api_types::plugin::{BrowserSelection, PluginWebsiteInfo};
use ultros_frontend_core::plugin_browser as browser;
mod setup;
pub use setup::PluginSetup;

#[derive(Clone, Copy)]
struct PluginContext {
    info: Resource<Option<PluginWebsiteInfo>>,
    account: Resource<Option<String>>,
}

pub fn is_plugin_path(path: &str) -> bool {
    path == "/plugin" || path.starts_with("/plugin/")
}

#[component]
pub fn PluginLayout() -> impl IntoView {
    let i18n = use_i18n();
    let (dalamud_labs, _) = use_dalamud_labs();
    let info = Resource::new_blocking(|| (), |_| async { browser::info().await.ok() });
    let account = Resource::new_blocking(
        || (),
        |_| async { crate::api::get_login().await.ok().map(|user| user.username) },
    );
    provide_context(PluginContext { info, account });
    let location = use_location();
    // Entering this section from an existing SPA document must first acquire
    // its CSP and omit third-party scripts. Never mount consent controls there.
    let ready = RwSignal::new(true);
    #[cfg(feature = "hydrate")]
    Effect::new(move |_| {
        if !browser::document_ready() {
            ready.set(false);
            let _ = window().location().reload();
        }
    });

    view! {
        <div class="main-content plugin-page p-2 sm:p-6">
            <div class="mx-auto max-w-4xl space-y-6">
                <Suspense fallback=|| ()>
                    {move || {
                        if dalamud_labs.get() && info.get().flatten().as_ref().is_some_and(setup_available) {
                            view! {
                                <header>
                                    <h1 class="text-3xl sm:text-4xl font-bold">{t!(i18n, plugin_heading)}</h1>
                                </header>
                                <nav class="plugin-tabs" aria-label=move || t_string!(i18n, plugin_nav)>
                                    <a href="/plugin" aria-current=move || matches!(location.pathname.get().as_str(), "/plugin" | "/plugin/").then_some("page")>{t!(i18n, plugin_nav_guide)}</a>
                                    <a href="/plugin/verify" aria-current=move || (location.pathname.get() == "/plugin/verify").then_some("page")>{t!(i18n, plugin_nav_connect)}</a>
                                    <a href="/plugin/devices" aria-current=move || (location.pathname.get() == "/plugin/devices").then_some("page")>{t!(i18n, plugin_nav_clients)}</a>
                                </nav>
                            }.into_any()
                        } else {
                            view! { <h1 class="text-3xl sm:text-4xl font-bold">{t!(i18n, plugin_nav)}</h1> }.into_any()
                        }
                    }}
                </Suspense>
                <Show when=move || dalamud_labs.get()>
                    <p class="plugin-notice" role="note">{t!(i18n, plugin_labs_warning)}</p>
                </Show>
                <Suspense fallback=move || view! { <p role="status">{t!(i18n, plugin_loading)}</p> }>
                    {move || match info.get().flatten() {
                        Some(config) if config.localhost && dalamud_labs.get() => Some(view! {
                            <div class="plugin-notice" role="note">
                                <strong>{t!(i18n, plugin_guide_development)}</strong>
                                <p>{t!(i18n, plugin_local_body, origin=config.origin.clone())}</p>
                            </div>
                        }),
                        _ => None,
                    }}
                </Suspense>
                <Show when=move || ready.get() fallback=move || view! { <p role="status">{t!(i18n, plugin_opening)}</p> }>
                    <Outlet />
                </Show>
            </div>
        </div>
    }
}

#[component]
fn PluginLabsNotice() -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <div class="plugin-notice space-y-3" data-testid="plugin-labs-required">
            <p>{t!(i18n, plugin_labs_required)}</p>
            <a class="btn-secondary inline-flex" href="/settings#labs">{t!(i18n, plugin_labs_settings)}</a>
        </div>
    }
}

#[component]
fn SignInNotice(#[prop(default = "/plugin/verify")] next: &'static str) -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <div class="space-y-3">
            <p>{t!(i18n, plugin_signin_intro)}</p>
            <a class="btn-primary inline-flex" href=format!("/login?next={next}") rel="external">{t!(i18n, plugin_signin_button)}</a>
        </div>
    }
}

#[component]
pub fn PluginVerify() -> impl IntoView {
    let i18n = use_i18n();
    let (dalamud_labs, _) = use_dalamud_labs();
    view! {
        <Title text=move || t_string!(i18n, plugin_verify_title) />
        <Show when=move || dalamud_labs.get() fallback=|| view! { <PluginLabsNotice /> }>
            <PluginVerifyFlow />
        </Show>
    }
}

#[component]
fn PluginVerifyFlow() -> impl IntoView {
    let i18n = use_i18n();
    let PluginContext { info, account } = expect_context::<PluginContext>();
    view! {
        <section class="panel p-5 sm:p-7 space-y-5">
            <h2 class="text-2xl font-bold">{t!(i18n, plugin_verify_heading)}</h2>
            <Suspense fallback=move || view! { <p role="status">{t!(i18n, plugin_checking_account)}</p> }>
                {move || {
                    if !info.get().flatten().is_some_and(|config| config.enabled) {
                        return view! { <p>{t!(i18n, plugin_connections_unavailable)}</p> }.into_any();
                    }
                    if account.get().flatten().is_none() {
                        return view! { <ConnectionControls signin=true /> }.into_any();
                    }
                    view! { <ConnectionControls /> }.into_any()
                }}
            </Suspense>
        </section>
        <p class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, plugin_need_install)}" "<a class="underline" href="/plugin">{t!(i18n, plugin_follow_setup_guide)}</a></p>
    }
}

#[component]
fn ConnectionControls(
    #[prop(optional)] devices: bool,
    #[prop(optional)] signin: bool,
) -> impl IntoView {
    let i18n = use_i18n();
    // The surrounding account resource is server-rendered. Mount its private
    // LocalResource subtree only after hydration, rather than nesting a deferred
    // client Suspense inside the server Suspense's hydration cursor.
    let mounted = RwSignal::new(false);
    Effect::new(move |_| mounted.set(browser::document_ready()));
    view! {
        <div class="space-y-4">
            <Show when=move || mounted.get() fallback=move || view! { <p role="status">{t!(i18n, plugin_loading_controls)}</p> }>
                {if devices { view! { <DeviceList /> }.into_any() } else if signin { view! { <PairingSignIn /> }.into_any() } else { view! { <PairingForm /> }.into_any() }}
            </Show>
        </div>
    }
}

#[component]
fn PairingSignIn() -> impl IntoView {
    let i18n = use_i18n();
    let prepared = LocalResource::new(browser::prepare_link);
    view! {
        <Suspense fallback=move || view! { <p role="status">{t!(i18n, plugin_preparing_request)}</p> }>
            {move || prepared.get().map(|result| view! {
                <div class="space-y-4" data-testid="plugin-link-ready">
                    {result.err().map(|error| view! { <p role="alert">{error}</p> })}
                    <SignInNotice />
                </div>
            })}
        </Suspense>
    }
}

#[component]
fn PairingForm() -> impl IntoView {
    let i18n = use_i18n();
    let approval = LocalResource::new(browser::approval);
    let selected = RwSignal::<Option<BrowserSelection>>::new(None);
    let code = RwSignal::new(String::new());
    let link_started = RwSignal::new(false);
    let select = Action::new(|input: &(String, Option<String>)| {
        let (csrf, code) = input.clone();
        async move {
            match code {
                Some(code) => browser::select(csrf, code).await,
                None => browser::select_link(csrf).await,
            }
        }
    });
    Effect::new(move |_| {
        if !link_started.get()
            && let Some(Ok(session)) = approval.get()
            && session.has_pending_link
        {
            link_started.set(true);
            select.dispatch((session.csrf, None));
        }
    });
    Effect::new(move |_| {
        if let Some(Ok(selection)) = select.value().get() {
            selected.set(Some(selection));
        }
    });
    view! {
        {move || match selected.get() {
            Some(selection) => view! { <ReviewPairing selection /> }.into_any(),
            None => view! {
                <p>{t!(i18n, plugin_code_instructions_prefix)}<code>"/ultros"</code>{t!(i18n, plugin_code_instructions_suffix)}</p>
                <Suspense fallback=move || view! { <p role="status">{t!(i18n, plugin_preparing_connection)}</p> }>
                    {move || approval.get().map(|result| match result {
                        Ok(session) => {
                            let account_name = session.account_name.clone();
                            view! {
                                <p>{t!(i18n, plugin_connecting_as, name = account_name)}</p>
                                <form class="space-y-4" method="post" action="/api/v1/plugin/pairings/select" on:submit=move |event| {
                                    event.prevent_default();
                                    if !select.pending().get_untracked() {
                                        select.dispatch((session.csrf.clone(), Some(code.get_untracked())));
                                    }
                                }>
                                    <label class="block space-y-2" for="plugin-code">
                                        <span class="font-semibold">{t!(i18n, plugin_code_label)}</span>
                                        <input id="plugin-code" class="input block w-full max-w-xs font-mono tracking-widest text-xl" name="user_code" required maxlength="12" autocomplete="off" autocapitalize="characters" spellcheck="false" placeholder="ABCD-EFGH" prop:value=move || code.get() on:input=move |event| code.set(event_target_value(&event)) />
                                    </label>
                                    <button class="btn-primary" type="submit" data-testid="plugin-select" disabled=move || select.pending().get()>{move || if select.pending().get() { t_string!(i18n, plugin_opening_request) } else { t_string!(i18n, plugin_review_connection_button) }}</button>
                                </form>
                            }.into_any()
                        },
                        Err(error) => view! { <p role="alert">{error}</p> }.into_any(),
                    })}
                </Suspense>
                {move || select.value().get().and_then(Result::err).map(|error| view! { <p role="alert">{error}</p> })}
                <a class="underline text-sm inline-block" href="/plugin/verify" rel="external">{t!(i18n, plugin_start_again)}</a>
            }.into_any(),
        }}
    }
}

fn permission_description(i18n: I18nContext<Locale, I18nKeys>, scope: &str) -> &'static str {
    match scope {
        "notifications" => t_string!(i18n, plugin_scope_notifications_desc),
        "lists:read" => t_string!(i18n, plugin_scope_lists_read_desc),
        "lists:write" => t_string!(i18n, plugin_scope_lists_write_desc),
        "retainers:read" => t_string!(i18n, plugin_scope_retainers_read_desc),
        "retainers:register" => t_string!(i18n, plugin_scope_retainers_register_desc),
        "retainers:upload" => t_string!(i18n, plugin_scope_retainers_upload_desc),
        _ => "",
    }
}

fn permission_label(i18n: I18nContext<Locale, I18nKeys>, scope: &str) -> &'static str {
    match scope {
        "notifications" => t_string!(i18n, plugin_scope_notifications_label),
        "lists:read" => t_string!(i18n, plugin_scope_lists_read_label),
        "lists:write" => t_string!(i18n, plugin_scope_lists_write_label),
        "retainers:read" => t_string!(i18n, plugin_scope_retainers_read_label),
        "retainers:register" => t_string!(i18n, plugin_scope_retainers_register_label),
        "retainers:upload" => t_string!(i18n, plugin_scope_retainers_upload_label),
        _ => t_string!(i18n, plugin_scope_unknown_label),
    }
}

// UI checkbox changes keep dependent reading permissions consistent. The
// server independently validates dependencies and the requested scope subset.
fn toggle_permission(permissions: &mut Vec<String>, scope: &str, enabled: bool) {
    let dependency = match scope {
        "lists:write" => Some("lists:read"),
        "retainers:register" | "retainers:upload" => Some("retainers:read"),
        _ => None,
    };
    permissions.retain(|value| value != scope);
    if enabled {
        permissions.push(scope.into());
        if let Some(dependency) = dependency
            && !permissions.iter().any(|value| value == dependency)
        {
            permissions.push(dependency.into());
        }
    } else if scope == "lists:read" {
        permissions.retain(|value| value != "lists:write");
    } else if scope == "retainers:read" {
        permissions
            .retain(|value| !matches!(value.as_str(), "retainers:register" | "retainers:upload"));
    }
}

#[component]
fn ReviewPairing(selection: BrowserSelection) -> impl IntoView {
    let i18n = use_i18n();
    let requested = selection
        .scopes
        .iter()
        .filter(|scope| !scope.starts_with("notifications:"))
        .cloned()
        .chain(
            selection
                .scopes
                .iter()
                .any(|scope| scope.starts_with("notifications:"))
                .then(|| "notifications".to_string()),
        )
        .collect::<Vec<_>>();
    let permissions = RwSignal::new(requested.clone());
    let selection = StoredValue::new(selection);
    let decide = Action::new(move |deny: &bool| {
        let deny = *deny;
        let selection = selection.get_value();
        let scopes = permissions.get_untracked();
        async move { browser::approve(selection.csrf, selection.nonce, scopes, deny).await }
    });
    view! {
        {move || match decide.value().get() {
            Some(Ok(result)) => view! {
                <Title text=move || if result.approved { t_string!(i18n, plugin_approved_title) } else { t_string!(i18n, plugin_declined_title) } />
                <div class="space-y-4" data-testid="plugin-decision">
                    <h3 class="text-xl font-bold">{move || if result.approved { t_string!(i18n, plugin_approved_heading) } else { t_string!(i18n, plugin_declined_heading) }}</h3>
                    <p role="status">{move || if result.approved { t_string!(i18n, plugin_approved_body) } else { t_string!(i18n, plugin_declined_body) }}</p>
                    <div class="flex flex-wrap gap-3"><a class="btn-secondary" href="/plugin/devices">{t!(i18n, plugin_connected_clients_link)}</a><a class="btn-secondary" href="/plugin">{t!(i18n, plugin_back_to_setup)}</a></div>
                </div>
            }.into_any(),
            _ => {
                let requested = requested.clone();
                let label = selection.with_value(|value| value.label.clone());
                let account_name = selection.with_value(|value| value.account_name.clone());
                view! {
                    <Title text=move || t_string!(i18n, plugin_review_title) />
                    <div class="space-y-5" data-testid="plugin-review">
                        <h3 class="text-xl font-bold">{t!(i18n, plugin_review_heading)}</h3>
                        <p>{t!(i18n, plugin_review_prompt, label = label, account = account_name)}</p>
                        <div class="plugin-notice"><p>{t!(i18n, plugin_review_compare_code)}</p><p class="text-2xl font-mono tracking-widest mt-2">{selection.with_value(|value| value.code.clone())}</p></div>
                        <fieldset class="space-y-3" disabled=move || decide.pending().get()>
                            <legend class="font-semibold mb-3">{t!(i18n, plugin_review_choose_permissions)}</legend>
                            {requested.into_iter().map(|scope| {
                                let checked_scope = scope.clone();
                                let change_scope = scope.clone();
                                let label_scope = scope.clone();
                                let desc_scope = scope.clone();
                                view! {
                                    <label class="plugin-permission">
                                        <input type="checkbox" name="scope" value=scope.clone() prop:checked=move || permissions.with(|values| values.contains(&checked_scope)) on:change=move |event| permissions.update(|values| toggle_permission(values, &change_scope, event_target_checked(&event))) />
                                        <span><strong class="block">{move || permission_label(i18n, &label_scope)}</strong><span class="text-sm text-[color:var(--color-text-muted)]">{move || permission_description(i18n, &desc_scope)}</span></span>
                                    </label>
                                }
                            }).collect_view()}
                        </fieldset>
                        <p class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, plugin_review_uploads_note)}</p>
                        <div class="flex flex-wrap gap-3">
                            <button class="btn-primary" type="button" value="approve" data-testid="plugin-approve" disabled=move || decide.pending().get() || permissions.with(Vec::is_empty) on:click=move |_| { decide.dispatch(false); }>{move || if decide.pending().get() { t_string!(i18n, plugin_saving) } else { t_string!(i18n, plugin_approve_button) }}</button>
                            <button class="btn-secondary" type="button" value="deny" data-testid="plugin-deny" disabled=move || decide.pending().get() on:click=move |_| { decide.dispatch(true); }>{t!(i18n, plugin_decline_button)}</button>
                        </div>
                        {move || decide.value().get().and_then(Result::err).map(|error| view! { <p role="alert">{error}</p> })}
                        <a class="underline text-sm inline-block" href="/plugin/verify" rel="external">{t!(i18n, plugin_start_again)}</a>
                    </div>
                }.into_any()
            },
        }}
    }
}

#[component]
pub fn PluginDevices() -> impl IntoView {
    let i18n = use_i18n();
    let PluginContext { info, account } = expect_context::<PluginContext>();
    view! {
        <Title text=move || t_string!(i18n, plugin_devices_title) />
        <section class="panel p-5 sm:p-7 space-y-5">
            <h2 class="text-2xl font-bold">{t!(i18n, plugin_devices_heading)}</h2>
            <p>{t!(i18n, plugin_devices_intro)}</p>
            <Suspense fallback=move || view! { <p role="status">{t!(i18n, plugin_checking_account)}</p> }>
                {move || {
                    if !info.get().flatten().is_some_and(|config| config.enabled) {
                        return view! { <p>{t!(i18n, plugin_connections_unavailable)}</p> }.into_any();
                    }
                    if account.get().flatten().is_none() {
                        return view! { <SignInNotice next="/plugin/devices" /> }.into_any();
                    }
                    view! { <ConnectionControls devices=true /> }.into_any()
                }}
            </Suspense>
        </section>
    }
}

#[component]
fn DeviceList() -> impl IntoView {
    let i18n = use_i18n();
    let plugin_available = browser::use_setup_available();
    let devices = LocalResource::new(browser::devices);
    let revoke = Action::new(|input: &(String, String)| {
        let (csrf, id) = input.clone();
        async move { browser::revoke(csrf, id).await }
    });
    Effect::new(move |_| {
        if matches!(revoke.value().get(), Some(Ok(true))) {
            devices.refetch();
        }
    });
    view! {
        <Suspense fallback=move || view! { <p role="status">{t!(i18n, plugin_devices_loading)}</p> }>
            {move || devices.get().map(|result| match result {
                Err(error) => view! { <p role="alert">{error}</p> }.into_any(),
                Ok(data) => {
                    let csrf = data.csrf;
                    let account_name = data.account_name;
                    view! {
                        <div class="space-y-4" data-testid="plugin-devices">
                            <p>{t!(i18n, plugin_devices_for, name = account_name)}</p>
                            {data.sessions.is_empty().then(|| view! { <p>{t!(i18n, plugin_devices_empty)}</p> })}
                            {data.sessions.into_iter().map(|session| {
                                let csrf = csrf.clone();
                                let id = session.id.clone();
                                let scopes = session.scopes.iter().filter(|scope| !scope.starts_with("notifications:")).map(|scope| permission_label(i18n, scope)).chain(session.scopes.iter().any(|scope| scope.starts_with("notifications:")).then(|| permission_label(i18n, "notifications"))).collect::<Vec<_>>().join(" · ");
                                let created = session.created_at.format("%b %-d, %Y").to_string();
                                let last_seen = session.last_seen_at.format("%b %-d, %Y at %H:%M UTC").to_string();
                                view! {
                                    <article class="plugin-device space-y-3" data-device=session.id>
                                        <h3 class="font-bold text-lg break-words">{session.label}</h3>
                                        <p class="text-sm text-[color:var(--color-text-muted)]">{scopes}</p>
                                        <p class="text-sm">{t!(i18n, plugin_devices_meta, created = created, last_seen = last_seen)}</p>
                                        <button class="btn-secondary" type="button" data-testid="plugin-revoke" disabled=move || revoke.pending().get() on:click=move |_| { revoke.dispatch((csrf.clone(), id.clone())); }>{t!(i18n, plugin_devices_disconnect)}</button>
                                    </article>
                                }
                            }).collect_view()}
                        </div>
                    }.into_any()
                },
            })}
        </Suspense>
        {move || revoke.value().get().map(|result| match result {
            Ok(_) => view! { <p role="status">{t!(i18n, plugin_devices_disconnected)}</p> }.into_any(),
            Err(error) => view! { <p role="alert">{error}</p> }.into_any(),
        })}
        <div class="flex flex-wrap gap-3">
            <Suspense fallback=|| ()>
                <Show when=move || plugin_available.get()>
                    <a class="btn-primary" href="/plugin/verify">{t!(i18n, plugin_devices_connect_button)}</a>
                </Show>
            </Suspense>
            <button class="btn-secondary" type="button" disabled=move || revoke.pending().get() on:click=move |_| devices.refetch()>{t!(i18n, plugin_devices_refresh)}</button>
        </div>
    }
}

#[component]
pub fn PluginNotFound() -> impl IntoView {
    let i18n = use_i18n();
    view! { <Title text=move || t_string!(i18n, plugin_not_found_title) /><p>{t!(i18n, plugin_not_found_body)}" "<a class="underline" href="/plugin">{t!(i18n, plugin_not_found_link)}</a></p> }
}

#[cfg(test)]
mod tests {
    use super::toggle_permission;

    #[test]
    fn permission_changes_keep_reading_dependencies_consistent() {
        let mut permissions = vec![];
        toggle_permission(&mut permissions, "lists:write", true);
        assert!(permissions.iter().any(|scope| scope == "lists:read"));
        toggle_permission(&mut permissions, "lists:read", false);
        assert!(permissions.is_empty());
        toggle_permission(&mut permissions, "retainers:register", true);
        toggle_permission(&mut permissions, "retainers:upload", true);
        assert_eq!(
            permissions
                .iter()
                .filter(|scope| *scope == "retainers:read")
                .count(),
            1
        );
        toggle_permission(&mut permissions, "retainers:read", false);
        assert!(permissions.is_empty());
    }
}
