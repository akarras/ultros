#![recursion_limit = "256"]
pub(crate) mod api;
pub(crate) mod components;
pub(crate) mod error;
pub(crate) mod global_state;
pub(crate) mod routes;
pub(crate) mod ws;

use crate::api::get_login;
use crate::components::recently_viewed::RecentItems;
use crate::global_state::{
    cheapest_prices::CheapestPrices, clipboard_text::GlobalLastCopiedText, cookies::Cookies,
    home_world::use_home_world, theme::provide_theme_settings,
};
pub use crate::global_state::{home_world::GuessedRegion, LocalWorldData};
use crate::{
    components::{
        ad::Ad, patreon::*, profile_display::*, search_box::*, theme_picker::*, tooltip::*,
    },
    routes::{
        analyzer::*,
        currency_exchange::{CurrencyExchange, CurrencySelection, ExchangeItem},
        edit_retainers::*,
        history::*,
        home_page::*,
        item_explorer::*,
        item_view::*,
        legal::{cookie_policy::CookiePolicy, privacy_policy::PrivacyPolicy},
        list_view::*,
        lists::*,
        retainers::*,
        settings::*,
    },
};
use git_const::git_short_hash;
use icondata as i;
use leptos::html::Div;
use leptos::prelude::*;
use leptos_hotkeys::{provide_hotkeys_context, scopes};
// use leptos_animation::AnimationContext;
// use leptos_hotkeys::{provide_hotkeys_context, scopes};
use leptos_icons::*;
use leptos_meta::*;
use leptos_router::components::{ParentRoute, Route, Router, Routes, A};
use leptos_router::path;
use log::info;

pub fn shell(options: LeptosOptions) -> impl IntoView {
    let git_hash = git_short_hash!();
    let sheet_url = ["/pkg/", git_hash, "/ultros.css"].concat();
    view! {
        <!DOCTYPE html>
        <html lang="en" data-theme="dark" data-palette="violet">
            <head>
                <meta charset="utf-8" />
                <script>
    "(function(){try{var d=document.documentElement;var ls=localStorage;var g=function(k){try{return ls.getItem(k)}catch(_){return null}};var gc=function(n){var m=document.cookie.match(new RegExp('(?:^|; )'+n+'=([^;]+)'));return m?decodeURIComponent(m[1]):null};var mode=g('theme.mode')||gc('theme_mode')||'system';if(mode==='system'){mode=(window.matchMedia&&window.matchMedia('(prefers-color-scheme: dark)').matches)?'dark':'light'};d.setAttribute('data-theme',mode==='light'?'light':'dark');var palette=g('theme.palette')||gc('theme_palette')||'violet';d.setAttribute('data-palette',palette)}catch(_){}})();"
                </script>
                <link
                    id="xiv-icons"
                    rel="stylesheet"
                    href="/static/classjob-icons/src/xivicon.css"
                />
                <link id="leptos" rel="stylesheet" href=sheet_url />
                <meta name="twitter:card" content="summary_large_image" />
                <meta name="viewport" content="initial-scale=1.0,width=device-width" />
                <meta name="theme-color" content="#0f0710" />
                <meta property="og:type" content="website" />
                <meta property="og:locale" content="en-US" />
                <meta property="og:site_name" content="Ultros" />
                <AutoReload options=options.clone() />
                <HydrationScripts options />
                <MetaTags />
            </head>
            <body>
                <App />
            </body>
        </html>
    }
}

#[component]
pub fn Footer() -> impl IntoView {
    let git_hash = git_short_hash!();
    view! {
        <footer class="bg-[color:var(--color-background-elevated)] border-t border-[color:var(--color-outline)]">
            <div class="mx-auto max-w-7xl px-4 sm:px-6 lg:px-8 py-10 space-y-6">
                <div class="flex flex-wrap justify-center items-center gap-x-8 gap-y-3">
                    <a
                        href="https://discord.gg/pgdq9nGUP2"
                        class="btn-ghost"
                    >
                        <Icon icon=i::BsDiscord width="1.1em" height="1.1em" /><span>"Discord"</span>
                    </a>
                    <a
                        href="https://github.com/akarras/ultros"
                        class="btn-ghost"
                    >
                        <Icon icon=i::IoLogoGithub width="1.1em" height="1.1em" /><span>"GitHub"</span>
                    </a>
                    <PatreonWrapper>
                        // nobody can tell it's not real.
                        <a class="btn-ghost cursor-pointer">
                            <span>"Patreon"</span>
                        </a>
                    </PatreonWrapper>
                    <a
                        href="https://book.ultros.app"
                        class="btn-ghost"
                    >
                        <Icon icon=i::BsBook width="1.1em" height="1.1em" /><span>"Book"</span>
                    </a>
                </div>
                <div class="divider"></div>
                <div class="text-center space-y-2 muted text-sm max-w-3xl mx-auto">
                    <p>
                        "Ultros is still under constant development. If you have suggestions or feedback,
                            feel free to leave suggestions in the discord."
                    </p>
                    <p>
                        "Made using "
                        <a
                            href="https://universalis.app/"
                            class="text-brand-300 hover:text-[color:var(--brand-fg)] transition-colors"
                        >
                            "universalis"
                        </a>
                        "' API. Please contribute to Universalis to help this site stay up to date."
                    </p>
                    <p>
                        "Version: "
                        <a
                            href=format!("https://github.com/akarras/ultros/commit/{git_hash}")
                            class="text-brand-300 hover:text-[color:var(--brand-fg)] transition-colors"
                        >
                            {git_hash}
                        </a>
                    </p>
                    <p class="text-xs">
                        "FINAL FANTASY XIV © 2010 - 2020 SQUARE ENIX CO., LTD. All Rights Reserved."
                    </p>
                </div>
            </div>
        </footer>
    }.into_any()
}

#[component]
pub fn NavRow() -> impl IntoView {
    let login = Resource::new(move || {}, move |_| async move { get_login().await.ok() });
    let (homeworld, _set_homeworld) = use_home_world();
    view! {
        // Navigation
        <nav class="sticky top-0 z-50 app-nav">
            <div class="mx-auto max-w-7xl px-3 sm:px-4 lg:px-6 py-3 flex flex-row flex-wrap items-center gap-3 text-gray-200">
                // Left section
                <div class="flex items-center gap-2">
                    <A
                        href="/"
                        exact=true
                        attr:class="nav-link"
                    >
                        <Icon icon=i::BiHomeSolid height="1.75em" width="1.75em" />
                        <span class="hidden sm:inline">"Home"</span>
                    </A>

                    {move || {
                        view! {
                            <A
                                href=homeworld()
                                    .map(|w| format!("/analyzer/{}", w.name))
                                    .unwrap_or("/analyzer".to_string())
                                attr:class="nav-link"
                            >
                                <Icon
                                    width="1.75em"
                                    height="1.75em"
                                    icon=i::FaMoneyBillTrendUpSolid
                                />
                                <span class="hidden md:inline">"Analyzer"</span>
                            </A>
                            <A
                                href="/items?menu-open=true"
                                attr:class="nav-link"
                            >
                                <Icon width="1.75em" height="1.75em" icon=i::FaScrewdriverWrenchSolid />
                                <span class="hidden sm:inline">"Explorer"</span>
                            </A>
                            <A
                                href="/currency-exchange"
                                attr:class="nav-link"
                            >
                                <span class="hidden sm:inline">"Exchange"</span>
                            </A>
                        }
                    }}

                    <Suspense fallback=move || {}>
                        {move || {
                            login
                                .get()
                                .flatten()
                                .map(|_| {
                                    view! {
                                        <A
                                            href="/list"
                                            attr:class="nav-link"
                                        >
                                            <Icon
                                                width="1.75em"
                                                height="1.75em"
                                                icon=i::AiOrderedListOutlined
                                            />
                                            <span class="hidden sm:inline">"Lists"</span>
                                        </A>
                                        <A
                                            href="/retainers/listings"
                                            attr:class="nav-link"
                                        >
                                            <Icon width="1.75em" height="1.75em" icon=i::BiGroupSolid />
                                            <span class="hidden sm:inline">"Retainers"</span>
                                        </A>
                                    }
                                })
                        }}
                    </Suspense>
                </div>

                // Center section
                <div class="flex-1 max-w-xl w-full">
                    <SearchBox />
                </div>

                // Right section
                <div class="flex items-center gap-4">


                    <QuickThemeToggle />

                    <a
                        rel="external"
                        href="/invitebot"
                        class="nav-link"
                    >
                        "Invite Bot"
                    </a>

                    <ProfileDisplay />
                </div>
            </div>
        </nav>
    }
}

#[component]
pub fn App() -> impl IntoView {
    info!("app run!");
    let cookies = Cookies::new();
    provide_meta_context();
    provide_context(cookies);
    provide_context(CheapestPrices::new());
    provide_context(GlobalLastCopiedText(RwSignal::new(None)));
    provide_context(RecentItems::new());
    provide_theme_settings();
    // AnimationContext::provide();
    let root_node_ref = NodeRef::<Div>::new();
    provide_hotkeys_context(root_node_ref, false, scopes!());

    view! {
        <Title text="Ultros" />
        // Background gradient
        <div class="fixed inset-0 -z-10" style="background-color: var(--color-background);">
            <div class="absolute inset-0" style="background-image: radial-gradient(80% 60% at 50% 30%, var(--decor-spot), transparent 60%);" />
        </div>
        <div node_ref=root_node_ref class="min-h-screen flex flex-col m-0">
            <Router>
                <NavRow />
                // <AnimatedRoutes outro="route-out" intro="route-in" outro_back="route-out-back" intro_back="route-in-back">
                // https://github.com/leptos-rs/leptos/issues/1754
                <main class="flex-1">
                    <div class="mx-auto max-w-7xl px-2 sm:px-4 lg:px-6 py-4 sm:py-6">
                        <Routes fallback=move || {
                            view! { <div>"Page not found"</div> }
                        }>
                            <Route path=path!("") view=HomePage />
                            <ParentRoute path=path!("retainers") view=Retainers>
                                <Route path=path!("edit") view=EditRetainers />
                                <Route path=path!("undercuts") view=RetainerUndercuts />
                                <Route path=path!("listings") view=RetainerListings />
                                <Route path=path!("listings/:id") view=SingleRetainerListings />
                                <Route path=path!("") view=RetainersBasePath />
                            </ParentRoute>
                            <ParentRoute path=path!("list") view=Lists>
                                <Route path=path!(":id") view=ListView />
                                <Route path=path!("") view=EditLists />
                            </ParentRoute>
                            <ParentRoute path=path!("items") view=ItemExplorer>
                                <Route path=path!("jobset/:jobset") view=JobItems />
                                <Route path=path!("category/:category") view=CategoryItems />
                                <Route
                                    path=path!("")
                                    view=move || view! { "Choose a category to search!" }
                                />
                            </ParentRoute>
                            <Route path=path!("item/:world/:id") view=ItemView />
                            <Route path=path!("item/:id") view=ItemView />
                            <Route path=path!("analyzer") view=Analyzer />
                            <Route path=path!("analyzer/:world") view=AnalyzerWorldView />
                            <Route path=path!("settings") view=Settings />
                            <Route path=path!("profile") view=Profile />
                            <Route path=path!("privacy") view=PrivacyPolicy />
                            <Route path=path!("cookie-policy") view=CookiePolicy />
                            <Route path=path!("history") view=History />
                            <ParentRoute path=path!("currency-exchange") view=CurrencyExchange>
                                <Route path=path!(":id") view=ExchangeItem />
                                <Route path=path!("") view=CurrencySelection />
                            </ParentRoute>
                        </Routes>
                    </div>
                </main>
            </Router>
        </div>
        <Footer />
    }
}
