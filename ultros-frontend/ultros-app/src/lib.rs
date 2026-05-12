#![recursion_limit = "256"]
pub(crate) mod analysis;
pub(crate) mod api;
pub(crate) mod components;
pub(crate) mod error;
pub(crate) mod global_state;
pub(crate) mod math;
pub(crate) mod routes;
pub(crate) mod ws;

include!(concat!(env!("OUT_DIR"), "/i18n/mod.rs"));
use i18n::*;

use crate::components::icon::Icon;
use crate::components::recently_viewed::RecentItems;
pub use crate::global_state::{LocalWorldData, home_world::GuessedRegion};
use crate::global_state::{
    cheapest_prices::CheapestPrices, clipboard_text::GlobalLastCopiedText, cookies::Cookies,
    side_nav::provide_side_nav_settings, theme::provide_theme_settings,
    toasts::provide_toast_context, xiv_data::provide_xiv_data_revision,
};
use crate::{
    components::{app_shell::AppShell, patreon::*, toast::*, tooltip::*},
    routes::{
        about::*,
        alerts::Alerts,
        analyzer::*,
        currency_exchange::{CurrencyExchange, CurrencySelection, ExchangeItem},
        edit_retainers::*,
        fc_crafting_analyzer::*,
        help::*,
        history::*,
        home_page::*,
        item_explorer::*,
        item_view::*,
        legal::{cookie_policy::CookiePolicy, privacy_policy::PrivacyPolicy},
        leve_analyzer::*,
        list_view::*,
        lists::*,
        not_found::NotFound,
        recipe_analyzer::*,
        retainers::*,
        scrip_sources::*,
        settings::*,
        trends::*,
        vendor_resale::*,
        venture_analyzer::*,
        welcome::*,
    },
};
use git_const::git_short_hash;
use icondata as i;
use leptos::html::Div;
use leptos::prelude::*;
#[cfg(feature = "hydrate")]
use leptos_hotkeys::{provide_hotkeys_context, scopes};
use leptos_meta::*;
use leptos_router::components::{A, ParentRoute, Route, Router, Routes};
use leptos_router::path;
use log::info;

fn error_reporting_script() -> Option<String> {
    let dsn = std::env::var("ULTROS_ERROR_REPORTING_DSN").ok()?;
    if dsn.trim().is_empty() {
        return None;
    }

    let environment = std::env::var("ULTROS_ERROR_REPORTING_ENVIRONMENT")
        .or_else(|_| std::env::var("LEPTOS_ENVIRONMENT"))
        .unwrap_or_else(|_| "production".to_string());
    let sample_rate = std::env::var("ULTROS_ERROR_REPORTING_SAMPLE_RATE")
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(1.0);
    let traces_sample_rate = std::env::var("ULTROS_ERROR_REPORTING_TRACES_SAMPLE_RATE")
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(0.0);
    let release = format!("ultros@{}", git_short_hash!());

    let config = serde_json::json!({
        "dsn": dsn,
        "release": release,
        "environment": environment,
        "sampleRate": sample_rate.clamp(0.0, 1.0),
        "tracesSampleRate": traces_sample_rate.clamp(0.0, 1.0),
        "autoSessionTracking": false,
        "sendDefaultPii": false,
        "attachStacktrace": true,
    });
    let sdk_url = std::env::var("ULTROS_ERROR_REPORTING_SDK_URL")
        .unwrap_or_else(|_| "https://browser.sentry-cdn.com/10.52.0/bundle.min.js".to_string());
    let config = serde_json::to_string(&config).expect("Sentry config should serialize");
    let sdk_url = serde_json::to_string(&sdk_url).expect("SDK URL should serialize");

    Some(format!(
        r#"(function(){{
    var config = {config};
    var sdkUrl = {sdk_url};

    window.__ultrosReportRustPanic = function(message, location) {{
        var Sentry = window.Sentry;
        if (!Sentry || !Sentry.captureException) {{
            return;
        }}

        var error = new Error(message || "Rust WASM panic");
        error.name = "RustWasmPanic";
        Sentry.withScope(function(scope) {{
            scope.setTag("runtime", "wasm");
            if (location) {{
                scope.setContext("rust_panic", {{ location: location }});
            }}
            Sentry.captureException(error);
        }});
    }};

    var init = function() {{
        if (!window.Sentry || !window.Sentry.init) {{
            return;
        }}
        window.Sentry.init(config);
    }};

    var script = document.createElement("script");
    script.src = sdkUrl;
    script.crossOrigin = "anonymous";
    script.onload = init;
    script.onerror = function() {{
        console.warn("Ultros error reporting SDK failed to load");
    }};
    document.head.appendChild(script);
}})();"#
    ))
}

pub fn shell(options: LeptosOptions) -> impl IntoView {
    let sheet_url = ["/", options.site_pkg_dir.as_ref(), "/ultros.css"].concat();
    let error_reporting_script = error_reporting_script();
    view! {
        <!DOCTYPE html>
        <html lang="en" data-theme="dark" data-palette="violet">
            <head>
                <meta charset="utf-8" />
                <link rel="apple-touch-icon" sizes="180x180" href="/static/apple-touch-icon.png" />
                <link rel="icon" type="image/png" sizes="32x32" href="/static/favicon-32x32.png" />
                <link rel="icon" type="image/png" sizes="16x16" href="/static/favicon-16x16.png" />
                <link rel="manifest" href="/static/site.webmanifest" />
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
                {error_reporting_script
                    .map(|script| {
                        view! { <script>{script}</script> }
                    })}
                <AutoReload options=options.clone() />
                <HydrationScripts options />
                <MetaTags />
                <script
                    async
                    src="https://www.googletagmanager.com/gtag/js?id=G-WYVZLM39M3"
                ></script>
                <script>
    "window.dataLayer = window.dataLayer || [];function gtag(){dataLayer.push(arguments);}gtag('js', new Date());gtag('config', 'G-WYVZLM39M3');"
                </script>
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
    let i18n = use_i18n();
    view! {
        <footer class="bg-black/20 backdrop-blur-md border-t border-[color:var(--color-outline)] mt-12">
            <div class="mx-auto max-w-7xl px-4 sm:px-6 lg:px-8 py-12 space-y-8">
                <div class="flex flex-wrap justify-center items-center gap-x-8 gap-y-4">
                    <a
                        href="https://discord.gg/pgdq9nGUP2"
                        class="btn-ghost opacity-80 hover:opacity-100"
                    >
                        <Icon icon=i::BsDiscord width="1.2em" height="1.2em" /><span>{t!(i18n, discord)}</span>
                    </a>
                    <a
                        href="https://github.com/akarras/ultros"
                        class="btn-ghost opacity-80 hover:opacity-100"
                    >
                        <Icon icon=i::IoLogoGithub width="1.2em" height="1.2em" /><span>{t!(i18n, github)}</span>
                    </a>
                    <PatreonWrapper>
                        // nobody can tell it's not real.
                        <a class="btn-ghost cursor-pointer opacity-80 hover:opacity-100">
                            <span>{t!(i18n, patreon)}</span>
                        </a>
                    </PatreonWrapper>
                    <A
                        href="/help"
                        attr:class="btn-ghost opacity-80 hover:opacity-100"
                    >
                        <Icon icon=i::BsBook width="1.2em" height="1.2em" /><span>"Help"</span>
                    </A>
                    <A
                        href="/about"
                        attr:class="btn-ghost opacity-80 hover:opacity-100"
                    >
                        <Icon icon=i::BsInfoCircle width="1.2em" height="1.2em" /><span>{t!(i18n, about)}</span>
                    </A>
                </div>
                <div class="divider opacity-50"></div>
                <div class="text-center space-y-3 muted text-sm max-w-3xl mx-auto opacity-75 hover:opacity-100 transition-opacity">
                    <p>
                        {t!(i18n, ultros_development_suggestion)}
                    </p>
                    <p>
                        "Made using "
                        <a
                            href="https://universalis.app/"
                            class="text-brand-300 hover:text-[color:var(--brand-fg)] transition-colors underline decoration-dotted underline-offset-4"
                        >
                            <Icon icon=i::FaSpaghettiMonsterFlyingSolid width="1.2em" height="1.2em" attr:class="inline mr-1" />
                            "universalis"
                        </a>
                        "' API. " {t!(i18n, made_using_universalis)}
                    </p>
                    <p>
                        {t!(i18n, version)} ": "
                        <a
                            href=format!("https://github.com/akarras/ultros/commit/{git_hash}")
                            class="text-brand-300 hover:text-[color:var(--brand-fg)] transition-colors font-mono"
                        >
                            {git_hash}
                        </a>
                    </p>
                    <p class="text-xs pt-4 opacity-50">
                        {t!(i18n, final_fantasy_copyright)}
                    </p>
                </div>
            </div>
        </footer>
    }.into_any()
}

#[component]
pub fn App() -> impl IntoView {
    info!("app run!");
    let cookies = Cookies::new();
    provide_meta_context();
    view! {
        <I18nContextProvider>
            <AppInner cookies />
        </I18nContextProvider>
    }
}

#[component]
pub fn AppInner(cookies: Cookies) -> impl IntoView {
    let i18n = use_i18n();
    let region = use_context::<GuessedRegion>();
    Effect::new(move |_| {
        if let Some(region) = region.as_ref() {
            let current_locale = i18n.get_locale();
            if current_locale == Locale::en {
                let new_locale = match region.0.as_str() {
                    "Japan" => Some(Locale::ja),
                    "中国" => Some(Locale::cn),
                    "한국" => Some(Locale::ko),
                    _ => None,
                };
                if let Some(new_locale) = new_locale {
                    i18n.set_locale(new_locale);
                }
            }
        }
    });
    provide_context(cookies);
    provide_context(CheapestPrices::new());
    provide_context(GlobalLastCopiedText(RwSignal::new(None)));
    provide_context(RecentItems::new());
    provide_theme_settings();
    provide_side_nav_settings();
    provide_toast_context();
    provide_xiv_data_revision();
    ws::realtime::provide_realtime_context();
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
                <AppShell>
                    <Routes fallback=NotFound>
                        <Route path=path!("") view=HomePage />
                        <ParentRoute path=path!("retainers") view=Retainers>
                            <Route path=path!("edit") view=EditRetainers />
                            <Route path=path!("undercuts") view=RetainerUndercuts />
                            <Route path=path!("listings") view=RetainerListings />
                            <Route path=path!("listings/:id") view=SingleRetainerListings />
                            <Route path=path!("") view=RetainersBasePath />
                        </ParentRoute>
                        <Route path=path!("alerts") view=Alerts />
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
                        <Route path=path!("vendor-resale") view=VendorResale />
                        <Route path=path!("vendor-resale/:world") view=VendorWorldView />
                        <Route path=path!("recipe-analyzer") view=RecipeAnalyzer />
                        <Route path=path!("fc-crafting-analyzer") view=FCCraftingAnalyzer />
                        <Route path=path!("fc-crafting-analyzer/:world") view=FCCraftingAnalyzer />
                        <Route path=path!("leve-analyzer") view=LeveAnalyzer />
                        <Route path=path!("scrip-sources") view=ScripSources />
                        <Route path=path!("venture-analyzer") view=VentureAnalyzer />
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
                        <Route path=path!("trends") view=Trends />
                        <Route path=path!("settings") view=Settings />
                        <Route path=path!("welcome") view=Welcome />
                        <Route path=path!("help") view=HelpIndex />
                        <Route path=path!("help/:topic") view=HelpArticle />
                        <Route path=path!("profile") view=Profile />
                        <Route path=path!("privacy") view=PrivacyPolicy />
                        <Route path=path!("cookie-policy") view=CookiePolicy />
                        <Route path=path!("about") view=About />
                        <Route path=path!("history") view=History />
                        <ParentRoute path=path!("currency-exchange") view=CurrencyExchange>
                            <Route path=path!(":id") view=ExchangeItem />
                            <Route path=path!("") view=CurrencySelection />
                        </ParentRoute>
                    </Routes>
                </AppShell>
            </Router>
        </div>
        <Footer />
    }
}
