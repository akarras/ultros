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
    home_world::use_home_world,
};
pub use crate::global_state::{home_world::GuessedRegion, LocalWorldData};
use crate::{
    components::{ad::Ad, patreon::*, profile_display::*, search_box::*, tooltip::*},
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
        <html lang="en">
            <head>
                <meta charset="utf-8" />
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
        <footer class="bg-black/40  border-t border-white/5">
            <div class="container mx-auto px-6 py-8 space-y-6">
                <div class="flex flex-wrap justify-center gap-x-6 gap-y-2">
                    <a
                        href="https://discord.gg/pgdq9nGUP2"
                        class="text-gray-400 hover:text-violet-300 transition-colors"
                    >
                        "Discord"
                    </a>
                    <a
                        href="https://github.com/akarras/ultros"
                        class="text-gray-400 hover:text-violet-300 transition-colors"
                    >
                        "GitHub"
                    </a>
                    <PatreonWrapper>
                        // nobody can tell it's not real.
                        <a class="text-gray-400 hover:text-violet-300 transition-colors">
                            "Patreon"
                        </a>
                    </PatreonWrapper>
                    <a
                        href="https://book.ultros.app"
                        class="text-gray-400 hover:text-violet-300 transition-colors"
                    >
                        "Book"
                    </a>
                </div>

                <div class="text-center space-y-2 text-gray-500 text-sm">
                    <p>
                        "Ultros is still under constant development. If you have suggestions or feedback,
                            feel free to leave suggestions in the discord."
                    </p>
                    <p>
                        "Made using "
                        <a
                            href="https://universalis.app/"
                            class="text-violet-300 hover:text-violet-200 transition-colors"
                        >
                            "universalis"
                        </a>
                        "' API. Please contribute to Universalis to help this site stay up to date."
                    </p>
                    <p>
                        "Version: "
                        <a
                            href=format!("https://github.com/akarras/ultros/commit/{git_hash}")
                            class="text-violet-300 hover:text-violet-200 transition-colors"
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
        <nav class="sticky top-0 z-50  border-b border-white/5 bg-black/40">
            <div class="mx-auto max-w-7xl px-2 sm:px-4 lg:px-6 py-2 flex flex-col md:flex-row items-center">
                // Left section
                <div class="flex items-center space-x-2">
                    <A
                        href="/"
                        exact=true
                        attr:class="flex items-center gap-2 px-3 py-2 rounded-lg
                        hover:bg-white/5 transition-colors
                        text-gray-200 hover:text-violet-300"
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
                                attr:class="flex items-center gap-2 px-3 py-2 rounded-lg
                                hover:bg-white/5 transition-colors
                                text-gray-200 hover:text-violet-300"
                            >
                                <Icon
                                    width="1.75em"
                                    height="1.75em"
                                    icon=i::FaMoneyBillTrendUpSolid
                                />
                                <span class="hidden md:inline">"Analyzer"</span>
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
                                            attr:class="flex items-center gap-2 px-3 py-2 rounded-lg
                                            hover:bg-white/5 transition-colors
                                            text-gray-200 hover:text-violet-300"
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
                                            attr:class="flex items-center gap-2 px-3 py-2 rounded-lg
                                            hover:bg-white/5 transition-colors
                                            text-gray-200 hover:text-violet-300"
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
                    <A
                        href="/items?menu-open=true"
                        attr:class="flex items-center gap-2 px-3 py-2 rounded-lg
                        hover:bg-white/5 transition-colors
                        text-gray-200 hover:text-violet-300"
                    >
                        <Tooltip tooltip_text="Item Explorer">
                            <Icon width="1.75em" height="1.75em" icon=i::FaScrewdriverWrenchSolid />
                        </Tooltip>
                        <span class="sr-only">"Item Explorer"</span>
                    </A>

                    <a
                        rel="external"
                        href="/invitebot"
                        class="px-4 py-2 rounded-lg bg-violet-600/20 hover:bg-violet-600/30
                         border border-violet-400/10 hover:border-violet-400/20
                         transition-all duration-300 text-gray-200 hover:text-violet-300"
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
    // AnimationContext::provide();
    let root_node_ref = NodeRef::<Div>::new();
    provide_hotkeys_context(root_node_ref, false, scopes!());

    view! {
        <Title text="Ultros" />
        // Background gradient
        <div class="fixed inset-0 -z-10 bg-black">
            <div class="absolute inset-0 bg-gradient-to-br from-violet-950/30 via-black/95 to-violet-950/30" />
            <div class="absolute inset-0 bg-[radial-gradient(circle_at_center,rgba(139,92,246,0.03),transparent_60%)]" />
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
