#![recursion_limit = "256"]
pub(crate) mod api;
pub(crate) mod components;
pub(crate) mod error;
pub(crate) mod global_state;
pub(crate) mod routes;
pub(crate) mod ws;

use crate::components::recently_viewed::RecentItems;
pub use crate::global_state::{LocalWorldData, home_world::GuessedRegion};
use crate::global_state::{
    cheapest_prices::CheapestPrices, clipboard_text::GlobalLastCopiedText, cookies::Cookies,
    theme::provide_theme_settings, toasts::provide_toast_context,
};
use crate::{
    components::{
        ad::Ad, apps_menu::*, patreon::*, search_box::*, theme_picker::*, toast::*, tooltip::*,
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
        leve_analyzer::*,
        list_view::*,
        lists::*,
        recipe_analyzer::*,
        retainers::*,
        settings::*,
        trends::*,
    },
};
use git_const::git_short_hash;
use icondata as i;
use leptos::html::Div;
use leptos::prelude::*;
#[cfg(feature = "hydrate")]
use leptos_hotkeys::{provide_hotkeys_context, scopes};
// use leptos_animation::AnimationContext;
// use leptos_hotkeys::{provide_hotkeys_context, scopes};
use crate::components::icon::Icon;
use leptos_meta::*;
use leptos_router::components::{A, ParentRoute, Route, Router, Routes};
use leptos_router::path;
use log::info;

pub fn shell(options: LeptosOptions) -> impl IntoView {
    let sheet_url = ["/", options.site_pkg_dir.as_ref(), "/ultros.css"].concat();
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
        <footer class="bg-black/20 backdrop-blur-md border-t border-[color:var(--color-outline)] mt-12">
            <div class="mx-auto max-w-7xl px-4 sm:px-6 lg:px-8 py-12 space-y-8">
                <div class="flex flex-wrap justify-center items-center gap-x-8 gap-y-4">
                    <a
                        href="https://discord.gg/pgdq9nGUP2"
                        class="btn-ghost opacity-80 hover:opacity-100"
                    >
                        <Icon icon=i::BsDiscord width="1.2em" height="1.2em" /><span>"Discord"</span>
                    </a>
                    <a
                        href="https://github.com/akarras/ultros"
                        class="btn-ghost opacity-80 hover:opacity-100"
                    >
                        <Icon icon=i::IoLogoGithub width="1.2em" height="1.2em" /><span>"GitHub"</span>
                    </a>
                    <PatreonWrapper>
                        // nobody can tell it's not real.
                        <a class="btn-ghost cursor-pointer opacity-80 hover:opacity-100">
                            <span>"Patreon"</span>
                        </a>
                    </PatreonWrapper>
                    <a
                        href="https://book.ultros.app"
                        class="btn-ghost opacity-80 hover:opacity-100"
                    >
                        <Icon icon=i::BsBook width="1.2em" height="1.2em" /><span>"Book"</span>
                    </a>
                </div>
                <div class="divider opacity-50"></div>
                <div class="text-center space-y-3 muted text-sm max-w-3xl mx-auto opacity-75 hover:opacity-100 transition-opacity">
                    <p>
                        "Ultros is still under constant development. If you have suggestions or feedback,
                            feel free to leave suggestions in the discord."
                    </p>
                    <p>
                        "Made using "
                        <a
                            href="https://universalis.app/"
                            class="text-brand-300 hover:text-[color:var(--brand-fg)] transition-colors underline decoration-dotted underline-offset-4"
                        >
                            "universalis"
                        </a>
                        "' API. Please contribute to Universalis to help this site stay up to date."
                    </p>
                    <p>
                        "Version: "
                        <a
                            href=format!("https://github.com/akarras/ultros/commit/{git_hash}")
                            class="text-brand-300 hover:text-[color:var(--brand-fg)] transition-colors font-mono"
                        >
                            {git_hash}
                        </a>
                    </p>
                    <p class="text-xs pt-4 opacity-50">
                        "FINAL FANTASY XIV © 2010 - 2020 SQUARE ENIX CO., LTD. All Rights Reserved."
                    </p>
                </div>
            </div>
        </footer>
    }.into_any()
}

#[component]
pub fn NavRow() -> impl IntoView {
    // mobile: inline search (no modal)
    view! {
        // Navigation
        <nav class="sticky top-0 z-50 app-nav">
            <div class="mx-auto max-w-7xl px-3 sm:px-4 lg:px-6 py-3 flex flex-row flex-wrap items-center gap-2 text-gray-200">
                // Left section
                                <div class="hidden lg:flex items-center gap-2">
                    <A
                        href="/"
                        exact=true
                        attr:class="nav-link"
                    >
                        <Icon icon=i::BiHomeSolid />
                        <span class="hidden sm:inline">"Home"</span>
                    </A>

                    <AppsMenu />
                </div>

                // Center section
                                <div class="hidden lg:block flex-1 w-full">
                                    <SearchBox />
                                </div>
                                // Mobile: search row on top, actions row below
                                <div class="block lg:hidden w-full">
                                    <div class="w-full">
                                        <SearchBox />
                                    </div>
                                    <div class="mt-2 flex items-center justify-between w-full">
                                        <A href="/" exact=true attr:class="nav-link">
                                            <Icon icon=i::BiHomeSolid />
                                            <span class="hidden sm:inline">"Home"</span>
                                        </A>
                                        <AppsMenu />
                                        <UserMenu />
                                    </div>
                                </div>

                // Right section
                                <div class="hidden lg:flex items-center gap-3">
                    <div class="hidden lg:block">
                        <QuickThemeToggle />
                    </div>
                    <UserMenu />
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
    provide_toast_context();
    // AnimationContext::provide();
    let root_node_ref = NodeRef::<Div>::new();
    #[cfg(feature = "hydrate")]
    {
        provide_hotkeys_context(root_node_ref, false, scopes!());
    }

    view! {
        <Title text="Ultros" />
        // Background gradient
        <div class="fixed inset-0 -z-10" style="background-color: var(--color-background);">
            <div class="absolute inset-0" style="background-image: radial-gradient(80% 60% at 50% 30%, var(--decor-spot), transparent 60%);" />
        </div>
        <div node_ref=root_node_ref class="min-h-screen flex flex-col m-0">
            <ToastContainer />
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
                            <Route path=path!("flip-finder") view=Analyzer />
                            <Route path=path!("analyzer") view=move || {
                                let nav = leptos_router::hooks::use_navigate();
                                Effect::new(move |_| { nav("/flip-finder", Default::default()); });
                                view! { <div /> }
                            } />
                            <Route path=path!("flip-finder/:world") view=AnalyzerWorldView />
                            <Route path=path!("recipe-analyzer") view=RecipeAnalyzer />
                            <Route path=path!("leve-analyzer") view=LeveAnalyzer />
                            <Route path=path!("analyzer/:world") view=move || {
                                let nav = leptos_router::hooks::use_navigate();
                                let params = leptos_router::hooks::use_params_map();
                                Effect::new(move |_| {
                                    let w = params.with_untracked(|p| p.get("world").clone().unwrap_or_default());
                                    let to = format!("/flip-finder/{}", w);
                                    nav(&to, Default::default());
                                });
                                view! { <div /> }
                            } />
                            <Route path=path!("trends/:world") view=Trends />
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
