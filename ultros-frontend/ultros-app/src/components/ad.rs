use std::ops::Deref;

use crate::Cookies;
use crate::api::get_login;
use crate::components::icon::Icon;
use crate::components::language_picker::LanguagePicker;
use crate::components::theme_picker::QuickThemeToggle;
use crate::i18n::{t, use_i18n};
use icondata as i;
use leptos::either::Either;
use leptos::{html::Ins, prelude::*};
use leptos_router::components::A;
use leptos_use::{UseMutationObserverOptions, use_mutation_observer_with_options};
use log::info;

#[component]
pub fn Ad(#[prop(optional)] class: Option<&'static str>) -> impl IntoView {
    let ad_class = class.unwrap_or("h-64");
    let node = NodeRef::<Ins>::new();
    let cookies = use_context::<Cookies>().unwrap();
    let (hide_ads, _) = cookies.use_cookie_typed::<_, bool>("HIDE_ADS");
    let unfilled = RwSignal::new(false);
    let _mutation_observer = use_mutation_observer_with_options(
        node,
        move |mutations, _| {
            if let Some(_ad_fill_status) = mutations.into_iter().find(|record| {
                record
                    .attribute_name()
                    .map(|name| name == "data-ad-status")
                    .unwrap_or_default()
            }) {
                // just looking for data-ad-status="unfilled"
                let node = node.get_untracked().unwrap();
                if let Some(status) = node.deref().get_attribute("data-ad-status") {
                    info!("ad status {status}");
                    unfilled.set(status == "unfilled");
                }
            }
        },
        UseMutationObserverOptions::default().attributes(true),
    );
    // let location = use_location();
    // let pathname = location.pathname;
    // let search = location.search;

    // let _ = pathname(); // reading from the path to reload this component on page load
    // let _ = search();
    let ads_visible = Signal::derive(move || !hide_ads.get().unwrap_or_default());
    view! {
        <Show when=ads_visible>
            <div class:hidden=unfilled class="ad">
                <div class="flex flex-col h-full">
                    <span class="text-sm px-2 py-0.5 rounded-md border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--brand-ring)_14%,_transparent)] text-[color:var(--color-text-muted)] shrink max-w-fit">
                        "Advertisements"
                    </span>
                    <script
                        async
                        src="https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js?client=ca-pub-8789160460804755"
                        crossorigin="anonymous"
                        on:error=move |_e| unfilled.set(true)
                    ></script>
                    // <!-- Ultros-Ad-Main -->
                    <ins
                        class=["adsbygoogle block ", ad_class].concat()

                        data-ad-client="ca-pub-8789160460804755"
                        data-ad-slot="1163555858"
                        // data-adtest="on"
                        node_ref=node
                    ></ins>
                    <script>(adsbygoogle = window.adsbygoogle || []).push({});</script>
                    <span class="text-neutral-500 italic text-sm">
                        "ads are optional. you may disable or enable them under "
                        <A href="/settings">"Settings"</A>
                    </span>
                </div>
            </div>
        </Show>
    }.into_any()
}

#[component]
pub fn DesktopAdRail() -> impl IntoView {
    let cookies = use_context::<Cookies>().unwrap();
    let (hide_ads, _) = cookies.use_cookie_typed::<_, bool>("HIDE_ADS");
    let ads_visible = Signal::derive(move || !hide_ads.get().unwrap_or_default());
    let i18n = use_i18n();
    let user = Resource::new(move || {}, move |_| async move { get_login().await.ok() });

    let library_section = move || {
        user.get().flatten().map(|_| {
            view! {
                <div class="side-rail-section">
                    <div class="side-rail-heading">{t!(i18n, library)}</div>
                    <A href="/list" attr:class="nav-link w-full justify-start">
                        <Icon height="1.1em" width="1.1em" icon=i::AiOrderedListOutlined />
                        <span class="ml-2">{t!(i18n, lists)}</span>
                    </A>
                    <A href="/alerts" attr:class="nav-link w-full justify-start">
                        <Icon height="1.1em" width="1.1em" icon=i::BsBell />
                        <span class="ml-2">{t!(i18n, alerts)}</span>
                    </A>
                    <A href="/retainers/listings" attr:class="nav-link w-full justify-start">
                        <Icon height="1.1em" width="1.1em" icon=i::BiGroupSolid />
                        <span class="ml-2">{t!(i18n, retainers)}</span>
                    </A>
                </div>
            }
        })
    };

    let auth_links = move || match user.get().flatten() {
        Some(_) => Either::Left(view! {
            <a rel="external" href="/invitebot" class="nav-link w-full justify-start">
                <Icon height="1.1em" width="1.1em" icon=i::BsDiscord />
                <span class="ml-2">{t!(i18n, invite_bot)}</span>
            </a>
            <a rel="external" href="/logout" class="nav-link w-full justify-start">
                <Icon height="1.1em" width="1.1em" icon=i::BiLogOutRegular />
                <span class="ml-2">{t!(i18n, logout)}</span>
            </a>
        }),
        None => Either::Right(view! {
            <a rel="external" href="/login" class="nav-link w-full justify-start">
                <Icon height="1.1em" width="1.1em" icon=i::BsDiscord />
                <span class="ml-2">{t!(i18n, login_with_discord)}</span>
            </a>
        }),
    };

    view! {
        <aside class="app-ad-rail" aria-label="Side rail">
            <Suspense>
                {library_section}
            </Suspense>
            <Show when=ads_visible>
                <div class="ad-rail-slot">
                    <Ad class="h-[600px] w-full" />
                </div>
            </Show>
            <div class="side-rail-bottom">
                <A href="/settings" attr:class="nav-link w-full justify-start">
                    <Icon height="1.1em" width="1.1em" icon=i::IoSettingsSharp />
                    <span class="ml-2">{t!(i18n, settings)}</span>
                </A>
                <div class="flex items-center gap-2">
                    <LanguagePicker />
                    <QuickThemeToggle />
                </div>
                <Suspense>
                    {auth_links}
                </Suspense>
            </div>
        </aside>
    }
}
