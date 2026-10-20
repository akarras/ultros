use crate::i18n::*;
use leptos::prelude::*;
use leptos_meta::Title;
use leptos_router::{components::Outlet, hooks::use_location};
use ultros_api_types::plugin::{BrowserSelection, PluginWebsiteInfo, SCOPES};
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
                <header class="space-y-3">
                    <p class="text-sm font-semibold text-[color:var(--color-text-muted)]">{t!(i18n, plugin_brand)}</p>
                    <h1 class="text-3xl sm:text-4xl font-bold">{t!(i18n, plugin_heading)}</h1>
                    <p class="text-[color:var(--color-text-muted)] max-w-prose">{t!(i18n, plugin_intro)}</p>
                </header>
                <nav class="plugin-tabs" aria-label=move || t_string!(i18n, plugin_nav)>
                    <a href="/plugin" aria-current=move || matches!(location.pathname.get().as_str(), "/plugin" | "/plugin/").then_some("page")>{t!(i18n, plugin_nav_guide)}</a>
                    <a href="/plugin/verify" aria-current=move || (location.pathname.get() == "/plugin/verify").then_some("page")>{t!(i18n, plugin_nav_connect)}</a>
                    <a href="/plugin/devices" aria-current=move || (location.pathname.get() == "/plugin/devices").then_some("page")>{t!(i18n, plugin_nav_clients)}</a>
                </nav>
                <Suspense fallback=move || view! { <p role="status">{t!(i18n, plugin_loading)}</p> }>
                    {move || match info.get().flatten() {
                        Some(config) if config.localhost => Some(view! {
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
    let PluginContext { info, account } = expect_context::<PluginContext>();
    view! {
        <Title text="Connect your game • Ultros" />
        <section class="panel p-5 sm:p-7 space-y-5">
            <h2 class="text-2xl font-bold">"Connect your game to Ultros"</h2>
            <Suspense fallback=|| view! { <p role="status">"Checking your account…"</p> }>
                {move || {
                    if !info.get().flatten().is_some_and(|config| config.enabled) {
                        return view! { <p>"Game connections are not available on this server yet."</p> }.into_any();
                    }
                    if account.get().flatten().is_none() {
                        return view! { <ConnectionControls signin=true /> }.into_any();
                    }
                    view! { <ConnectionControls /> }.into_any()
                }}
            </Suspense>
        </section>
        <p class="text-sm text-[color:var(--color-text-muted)]">"Need to install the plugin first? "<a class="underline" href="/plugin">"Follow the setup guide."</a></p>
    }
}

#[component]
fn ConnectionControls(
    #[prop(optional)] devices: bool,
    #[prop(optional)] signin: bool,
) -> impl IntoView {
    // The surrounding account resource is server-rendered. Mount its private
    // LocalResource subtree only after hydration, rather than nesting a deferred
    // client Suspense inside the server Suspense's hydration cursor.
    let mounted = RwSignal::new(false);
    Effect::new(move |_| mounted.set(browser::document_ready()));
    view! {
        <div class="space-y-4">
            <Show when=move || mounted.get() fallback=|| view! { <p role="status">"Loading connection controls…"</p> }>
                {if devices { view! { <DeviceList /> }.into_any() } else if signin { view! { <PairingSignIn /> }.into_any() } else { view! { <PairingForm /> }.into_any() }}
            </Show>
        </div>
    }
}

#[component]
fn PairingSignIn() -> impl IntoView {
    let prepared = LocalResource::new(browser::prepare_link);
    view! {
        <Suspense fallback=|| view! { <p role="status">"Preparing your connection request…"</p> }>
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
                <p>"In game, open "<code>"/ultros"</code>" → Account → Connect to Ultros. Enter the code shown there."</p>
                <Suspense fallback=|| view! { <p role="status">"Preparing a secure connection…"</p> }>
                    {move || approval.get().map(|result| match result {
                        Ok(session) => view! {
                            <p>"Connecting as "<strong>{session.account_name}</strong>"."</p>
                            <form class="space-y-4" method="post" action="/api/v1/plugin/pairings/select" on:submit=move |event| {
                                event.prevent_default();
                                if !select.pending().get_untracked() {
                                    select.dispatch((session.csrf.clone(), Some(code.get_untracked())));
                                }
                            }>
                                <label class="block space-y-2" for="plugin-code">
                                    <span class="font-semibold">"Code from your game"</span>
                                    <input id="plugin-code" class="input block w-full max-w-xs font-mono tracking-widest text-xl" name="user_code" required maxlength="12" autocomplete="off" autocapitalize="characters" spellcheck="false" placeholder="ABCD-EFGH" prop:value=move || code.get() on:input=move |event| code.set(event_target_value(&event)) />
                                </label>
                                <button class="btn-primary" type="submit" data-testid="plugin-select" disabled=move || select.pending().get()>{move || if select.pending().get() { "Opening request…" } else { "Review connection" }}</button>
                            </form>
                        }.into_any(),
                        Err(error) => view! { <p role="alert">{error}</p> }.into_any(),
                    })}
                </Suspense>
                {move || select.value().get().and_then(Result::err).map(|error| view! { <p role="alert">{error}</p> })}
                <a class="underline text-sm inline-block" href="/plugin/verify" rel="external">"Start again"</a>
            }.into_any(),
        }}
    }
}

fn permission_description(scope: &str) -> &'static str {
    match scope {
        "notifications" => "Receive alerts in game and mark delivered notifications read.",
        "lists:read" => "View your lists and their items.",
        "lists:write" => "Add, edit, and remove items in your lists. Includes list reading.",
        "retainers:read" => "View your private characters, retainer roster, and saved listings.",
        "retainers:register" => {
            "Save observed characters and retainers to your account. Includes retainer reading."
        }
        "retainers:upload" => {
            "Upload observed sale listings for characters you opt in separately. Includes retainer reading."
        }
        _ => "",
    }
}

fn permission_label(scope: &str) -> &'static str {
    if scope == "notifications" {
        return "In-game notifications";
    }
    SCOPES
        .iter()
        .find(|(id, _)| *id == scope)
        .map(|(_, label)| *label)
        .unwrap_or("Unknown permission")
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
                <Title text=if result.approved { "Game connection approved • Ultros" } else { "Game connection declined • Ultros" } />
                <div class="space-y-4" data-testid="plugin-decision">
                    <h3 class="text-xl font-bold">{if result.approved { "Game connection approved" } else { "Game connection declined" }}</h3>
                    <p role="status">{if result.approved { "Return to your game. The plugin will finish connecting automatically. You can close this page." } else { "This request was declined. No new client has been connected. You can close this page." }}</p>
                    <div class="flex flex-wrap gap-3"><a class="btn-secondary" href="/plugin/devices">"Connected clients"</a><a class="btn-secondary" href="/plugin">"Back to setup"</a></div>
                </div>
            }.into_any(),
            _ => {
                let requested = requested.clone();
                view! {
                    <Title text="Review game connection • Ultros" />
                    <div class="space-y-5" data-testid="plugin-review">
                        <h3 class="text-xl font-bold">"Review game connection"</h3>
                        <p>"Connect "<strong>{selection.with_value(|value| value.label.clone())}</strong>" to "<strong>{selection.with_value(|value| value.account_name.clone())}</strong>"?"</p>
                        <div class="plugin-notice"><p>"Compare this code with the one in your own game:"</p><p class="text-2xl font-mono tracking-widest mt-2">{selection.with_value(|value| value.code.clone())}</p></div>
                        <fieldset class="space-y-3" disabled=move || decide.pending().get()>
                            <legend class="font-semibold mb-3">"Choose what this client can do"</legend>
                            {requested.into_iter().map(|scope| {
                                let checked_scope = scope.clone();
                                let change_scope = scope.clone();
                                view! {
                                    <label class="plugin-permission">
                                        <input type="checkbox" name="scope" value=scope.clone() prop:checked=move || permissions.with(|values| values.contains(&checked_scope)) on:change=move |event| permissions.update(|values| toggle_permission(values, &change_scope, event_target_checked(&event))) />
                                        <span><strong class="block">{permission_label(&scope)}</strong><span class="text-sm text-[color:var(--color-text-muted)]">{permission_description(&scope)}</span></span>
                                    </label>
                                }
                            }).collect_view()}
                        </fieldset>
                        <p class="text-sm text-[color:var(--color-text-muted)]">"You can disconnect this client at any time. Private retainer uploads also need a separate opt-in in game."</p>
                        <div class="flex flex-wrap gap-3">
                            <button class="btn-primary" type="button" value="approve" data-testid="plugin-approve" disabled=move || decide.pending().get() || permissions.with(Vec::is_empty) on:click=move |_| { decide.dispatch(false); }>{move || if decide.pending().get() { "Saving…" } else { "Approve connection" }}</button>
                            <button class="btn-secondary" type="button" value="deny" data-testid="plugin-deny" disabled=move || decide.pending().get() on:click=move |_| { decide.dispatch(true); }>"Decline"</button>
                        </div>
                        {move || decide.value().get().and_then(Result::err).map(|error| view! { <p role="alert">{error}</p> })}
                        <a class="underline text-sm inline-block" href="/plugin/verify" rel="external">"Start again"</a>
                    </div>
                }.into_any()
            },
        }}
    }
}

#[component]
pub fn PluginDevices() -> impl IntoView {
    let PluginContext { info, account } = expect_context::<PluginContext>();
    view! {
        <Title text="Connected game clients • Ultros" />
        <section class="panel p-5 sm:p-7 space-y-5">
            <h2 class="text-2xl font-bold">"Connected game clients"</h2>
            <p>"Manage the game clients that can use your Ultros account. Disconnecting a client also removes its in-game notification destination."</p>
            <Suspense fallback=|| view! { <p role="status">"Checking your account…"</p> }>
                {move || {
                    if !info.get().flatten().is_some_and(|config| config.enabled) {
                        return view! { <p>"Game connections are not available on this server yet."</p> }.into_any();
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
        <Suspense fallback=|| view! { <p role="status">"Loading connected clients…"</p> }>
            {move || devices.get().map(|result| match result {
                Err(error) => view! { <p role="alert">{error}</p> }.into_any(),
                Ok(data) => {
                    let csrf = data.csrf;
                    view! {
                        <div class="space-y-4" data-testid="plugin-devices">
                            <p>"Connections for "<strong>{data.account_name}</strong>"."</p>
                            {data.sessions.is_empty().then(|| view! { <p>"No game clients are connected yet. Start a connection from your game to add one."</p> })}
                            {data.sessions.into_iter().map(|session| {
                                let csrf = csrf.clone();
                                let id = session.id.clone();
                                let scopes = session.scopes.iter().filter(|scope| !scope.starts_with("notifications:")).map(|scope| permission_label(scope)).chain(session.scopes.iter().any(|scope| scope.starts_with("notifications:")).then_some("In-game notifications")).collect::<Vec<_>>().join(" · ");
                                view! {
                                    <article class="plugin-device space-y-3" data-device=session.id>
                                        <h3 class="font-bold text-lg break-words">{session.label}</h3>
                                        <p class="text-sm text-[color:var(--color-text-muted)]">{scopes}</p>
                                        <p class="text-sm">"Connected "{session.created_at.format("%b %-d, %Y").to_string()}" · Last used "{session.last_seen_at.format("%b %-d, %Y at %H:%M UTC").to_string()}</p>
                                        <button class="btn-secondary" type="button" data-testid="plugin-revoke" disabled=move || revoke.pending().get() on:click=move |_| { revoke.dispatch((csrf.clone(), id.clone())); }>"Disconnect client"</button>
                                    </article>
                                }
                            }).collect_view()}
                        </div>
                    }.into_any()
                },
            })}
        </Suspense>
        {move || revoke.value().get().map(|result| match result {
            Ok(_) => view! { <p role="status">"Client disconnected."</p> }.into_any(),
            Err(error) => view! { <p role="alert">{error}</p> }.into_any(),
        })}
        <div class="flex flex-wrap gap-3">
            <a class="btn-primary" href="/plugin/verify">"Connect a game"</a>
            <button class="btn-secondary" type="button" disabled=move || revoke.pending().get() on:click=move |_| devices.refetch()>"Refresh clients"</button>
        </div>
    }
}

#[component]
pub fn PluginNotFound() -> impl IntoView {
    view! { <Title text="Game plugin • Ultros" /><p>"That plugin page doesn’t exist. "<a class="underline" href="/plugin">"Open the setup guide."</a></p> }
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
