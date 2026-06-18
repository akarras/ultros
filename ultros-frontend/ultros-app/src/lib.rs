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
pub use crate::global_state::{BootstrapUser, LocalWorldData, home_world::GuessedRegion};
use crate::global_state::{
    cheapest_prices::CheapestPrices, clipboard_text::GlobalLastCopiedText, cookies::Cookies,
    side_nav::provide_side_nav_settings, theme::provide_theme_settings,
    toasts::provide_toast_context, xiv_data::provide_xiv_data_revision,
};
use crate::{
    components::{
        app_shell::AppShell, on_hand_input::provide_on_hand_context, patreon::*, toast::*,
        tooltip::*,
    },
    routes::{
        about::*,
        alerts::Alerts,
        analyzer::*,
        bot::BotGuide,
        currency_exchange::{CurrencyExchange, CurrencySelection, ExchangeItem},
        edit_retainers::*,
        fc_crafting_analyzer::*,
        help::*,
        history::*,
        home_page::*,
        item_explorer::*,
        item_view::*,
        job_set_detail::JobSetDetail,
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
use icondata as i;
use leptos::html::Div;
use leptos::prelude::*;
#[cfg(feature = "hydrate")]
use leptos_hotkeys::{provide_hotkeys_context, scopes};
use leptos_meta::*;
use leptos_router::components::{A, ParentRoute, Route, Router, Routes};
use leptos_router::path;
use log::info;

#[cfg(feature = "hydrate")]
mod sentry_tags {
    use wasm_bindgen::{JsCast, JsValue};

    /// Set a Sentry tag on every subsequent event. Backed by a queue in
    /// `error_reporting_script` so tags survive even if WASM beats the
    /// Sentry SDK <script> to the punch. Best-effort — no-op when
    /// reporting isn't enabled (no DSN → no inline script → no setter
    /// on window).
    ///
    /// Resolved via `js_sys::Reflect` for the same reason
    /// `__ultrosReportRustPanic` is: a `#[wasm_bindgen]` extern would
    /// throw on environments where the function isn't defined.
    pub fn set_sentry_tag(key: &str, value: &str) {
        let global = js_sys::global();
        let Ok(setter) = js_sys::Reflect::get(&global, &JsValue::from_str("__ultrosSentrySetTag"))
        else {
            return;
        };
        let Some(setter) = setter.dyn_ref::<js_sys::Function>() else {
            return;
        };
        let _ = setter.call2(
            &JsValue::NULL,
            &JsValue::from_str(key),
            &JsValue::from_str(value),
        );
    }
}

#[cfg(not(feature = "hydrate"))]
mod sentry_tags {
    /// SSR no-op: no Sentry on the server side. Allowed-unused because
    /// the call sites that would reference it are themselves gated to
    /// `feature = "hydrate"`.
    #[allow(dead_code)]
    pub fn set_sentry_tag(_key: &str, _value: &str) {}
}

#[cfg(feature = "hydrate")]
use sentry_tags::set_sentry_tag;

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
    let release = format!("ultros@{}", env!("GIT_HASH"));

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
    // The beforeSend noise filter, injected verbatim via the {filter_js}
    // placeholder below. Lives in its own file so Node can unit-test it.
    let filter_js = include_str!("error_filter.js");

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

    // Unactionable-noise filter. Defines window.__ultrosShouldDropEvent,
    // which classifies WASM/bundle fetch aborts, injected-WebView document
    // TypeErrors, injected-translation tachys hydration panics, and empty
    // promise rejections. Extracted to error_filter.js so it can be unit
    // tested with Node (see integration/error-filter.test.cjs); injected
    // here verbatim. Its single braces are safe because it arrives as a
    // format! argument value, not part of the format string literal.
{filter_js}

    var existingBeforeSend = config && config.beforeSend;
    config = config || {{}};
    config.beforeSend = function(event, hint) {{
        if (window.__ultrosShouldDropEvent && window.__ultrosShouldDropEvent(event)) {{
            return null;
        }}
        if (typeof existingBeforeSend === "function") {{
            return existingBeforeSend(event, hint);
        }}
        return event;
    }};

    // Tags queued from WASM before the Sentry SDK script finishes
    // loading. The setter writes through to Sentry.setTag if it's
    // ready, otherwise it enqueues; init() flushes the queue. This
    // matters because hydrate() runs almost immediately and may try
    // to tag the session before the async <script> has executed.
    window.__ultrosSentryTagQueue = window.__ultrosSentryTagQueue || [];
    window.__ultrosSentrySetTag = function(key, value) {{
        try {{
            var S = window.Sentry;
            if (S && typeof S.setTag === "function") {{
                S.setTag(key, value);
            }} else {{
                window.__ultrosSentryTagQueue.push([key, value]);
            }}
        }} catch (_) {{}}
    }};

    var init = function() {{
        if (!window.Sentry || !window.Sentry.init) {{
            return;
        }}
        window.Sentry.init(config);
        try {{
            var q = window.__ultrosSentryTagQueue || [];
            for (var i = 0; i < q.length; i++) {{
                try {{ window.Sentry.setTag(q[i][0], q[i][1]); }} catch(_) {{}}
            }}
            window.__ultrosSentryTagQueue = [];
        }} catch (_) {{}}
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

pub fn shell(options: LeptosOptions, bootstrap_script: String) -> impl IntoView {
    let sheet_url = ["/", options.site_pkg_dir.as_ref(), "/ultros.css"].concat();
    let error_reporting_script = error_reporting_script();
    view! {
        <!DOCTYPE html>
        // Chrome's Google Translate rewrites text nodes into `<font>` wrappers
        // before hydration finishes, which shifts tachys' hydration cursor and
        // trips `failed_to_cast_text_node` (panic at `hydration.rs:227`), then
        // cascades into `RefCell already borrowed` in the wasm-bindgen-futures
        // executor and kills the page. We opt out of translation with the full
        // Google trifecta: `translate="no"` (W3C standard), `class="notranslate"`
        // (Google's legacy signal — honored more reliably by older Chrome and the
        // Translate extension, which under-honor the `translate` attribute), and
        // `<meta name="google" content="notranslate">` below (suppresses the
        // page-level translate prompt). The class is repeated on `<body>` because
        // Translate walks the ancestor chain per text node. App has its own
        // locale switcher, so we never want the browser translating our markup.
        <html lang="en" translate="no" class="notranslate" data-theme="dark" data-palette="violet">
            <head>
                <meta charset="utf-8" />
                <meta name="google" content="notranslate" />
                <link rel="apple-touch-icon" sizes="180x180" href="/static/apple-touch-icon.png" />
                <link rel="icon" type="image/png" sizes="32x32" href="/static/favicon-32x32.png" />
                <link rel="icon" type="image/png" sizes="16x16" href="/static/favicon-16x16.png" />
                <link rel="manifest" href="/static/site.webmanifest" />
                // Bootstrap data injected by the SSR handler. Reading this in
                // hydrate() lets us skip the /world_data, /detectregion, and
                // /current_user round-trips on every cold load.
                <script inner_html=bootstrap_script />
                <script>
    "(function(){try{var d=document.documentElement;var ls=localStorage;var g=function(k){try{return ls.getItem(k)}catch(_){return null}};var gc=function(n){var m=document.cookie.match(new RegExp('(?:^|; )'+n+'=([^;]+)'));return m?decodeURIComponent(m[1]):null};var mode=g('theme.mode')||gc('theme_mode')||'system';if(mode==='system'){mode=(window.matchMedia&&window.matchMedia('(prefers-color-scheme: dark)').matches)?'dark':'light'};d.setAttribute('data-theme',mode==='light'?'light':'dark');var palette=g('theme.palette')||gc('theme_palette')||'violet';d.setAttribute('data-palette',palette)}catch(_){}})();"
                </script>
                <style>
    "#boot-progress{position:fixed;top:0;left:0;right:0;height:2px;z-index:99999;pointer-events:none;transition:opacity .4s ease}#boot-progress-bar{height:100%;width:0%;background:linear-gradient(90deg,#a78bfa,#f0abfc);box-shadow:0 0 8px rgba(167,139,250,.55);animation:boot-progress-grow 12s cubic-bezier(.05,.7,.1,1) forwards}#boot-progress.mid #boot-progress-bar{animation:boot-progress-mid 3s cubic-bezier(.2,.6,.2,1) forwards}#boot-progress.done{opacity:0}#boot-progress.done #boot-progress-bar{width:100%!important;transition:width .25s ease;animation:none}#boot-progress.error #boot-progress-bar{background:#ef4444;width:100%;animation:none;box-shadow:0 0 8px rgba(239,68,68,.55)}#boot-progress-status{position:fixed;top:8px;right:12px;z-index:99999;font:12px/1.2 system-ui,-apple-system,sans-serif;color:rgba(255,255,255,.55);pointer-events:none;letter-spacing:.02em}#boot-progress.error~#boot-progress-status,#boot-progress.error+#boot-progress-status{color:#fca5a5;pointer-events:auto}@keyframes boot-progress-grow{0%{width:0%}30%{width:25%}60%{width:50%}100%{width:75%}}@keyframes boot-progress-mid{0%{width:75%}100%{width:92%}}@media (prefers-reduced-motion:reduce){#boot-progress-bar{animation-duration:1s!important}#boot-progress{transition:none}}"
                </style>
                <script>
    "(function(){try{var root=document.documentElement;var bar=document.createElement('div');bar.id='boot-progress';var inner=document.createElement('div');inner.id='boot-progress-bar';bar.appendChild(inner);var status=document.createElement('span');status.id='boot-progress-status';status.textContent='Loading\\u2026';root.appendChild(bar);root.appendChild(status);var done=false;var finish=function(){if(done)return;done=true;clearTimeout(wd);bar.classList.add('done');setTimeout(function(){if(bar.parentNode)bar.parentNode.removeChild(bar);if(status.parentNode)status.parentNode.removeChild(status);},450)};var fail=function(msg){if(done)return;done=true;clearTimeout(wd);bar.classList.add('error');status.innerHTML=msg+' \\u2014 <a href=\"\" onclick=\"location.reload();return false\" style=\"color:inherit;text-decoration:underline\">reload</a>'};window.addEventListener('ultros:wasm-loaded',function(){bar.classList.add('mid')});window.addEventListener('ultros:hydrated',finish);window.addEventListener('error',function(e){var f=(e&&e.filename)||'';if(f.indexOf('.wasm')!==-1||f.indexOf('/pkg/')!==-1)fail('Failed to load app')});window.addEventListener('unhandledrejection',function(e){var r=e&&e.reason;var msg=(r&&(r.message||(''+r)))||'';if(msg.indexOf('wasm')!==-1||msg.indexOf('WebAssembly')!==-1)fail('App crashed during load')});var wd=setTimeout(function(){fail('Loading is taking longer than expected')},30000)}catch(_){}})();"
                </script>
                <link
                    id="xiv-icons"
                    rel="stylesheet"
                    href="/static/classjob-icons/src/xivicon.css"
                />
                <link id="leptos" rel="stylesheet" href=sheet_url />
                <meta name="twitter:card" content="summary_large_image" />
                <meta name="twitter:site" content="@ultros_app" />
                <meta name="viewport" content="initial-scale=1.0,width=device-width" />
                <meta name="theme-color" content="#0f0710" />
                <meta name="application-name" content="Ultros" />
                <meta property="og:type" content="website" />
                <meta property="og:locale" content="en_US" />
                <meta property="og:locale:alternate" content="ja_JP" />
                <meta property="og:locale:alternate" content="fr_FR" />
                <meta property="og:locale:alternate" content="de_DE" />
                <meta property="og:locale:alternate" content="ko_KR" />
                <meta property="og:locale:alternate" content="zh_CN" />
                <meta property="og:locale:alternate" content="zh_TW" />
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
            <body translate="no" class="notranslate">
                <App />
            </body>
        </html>
    }
}

#[component]
pub fn Footer() -> impl IntoView {
    let git_hash = env!("GIT_HASH");
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
                        <Icon icon=i::BsBook width="1.2em" height="1.2em" /><span>{t!(i18n, help_label)}</span>
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
    #[cfg(feature = "hydrate")]
    let region_for_tags = region.clone();
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
    provide_on_hand_context();
    ws::realtime::provide_realtime_context();
    // AnimationContext::provide();
    let root_node_ref = NodeRef::<Div>::new();
    #[cfg(feature = "hydrate")]
    {
        provide_hotkeys_context(root_node_ref, false, scopes!());
    }
    // Sentry context tags — locale + guessed region track the user's
    // i18n state. The route tag lives in <SentryRouteTag/> below since
    // use_location() requires Router context.
    #[cfg(feature = "hydrate")]
    {
        Effect::new(move |_| {
            let locale = i18n.get_locale();
            set_sentry_tag("locale", locale.as_str());
            if let Some(region) = region_for_tags.as_ref() {
                set_sentry_tag("region.guessed", &region.0);
            }
        });
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
                <SentryRouteTag />
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
                            <Route path=path!("invite/:invite_id") view=ListInviteAccept />
                            <Route path=path!(":id") view=ListView />
                            <Route path=path!("") view=EditLists />
                        </ParentRoute>
                        <ParentRoute path=path!("items") view=ItemExplorer>
                            <Route path=path!("jobset/:jobset/set/:ilvl") view=JobSetDetail />
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
                        <Route path=path!("bot") view=BotGuide />
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

/// Sets a Sentry `route` tag whenever the URL path changes. Lives inside
/// `<Router>` because `use_location()` requires Router context. SSR
/// renders nothing and never hits the effect.
#[component]
fn SentryRouteTag() -> impl IntoView {
    #[cfg(feature = "hydrate")]
    {
        let location = leptos_router::hooks::use_location();
        Effect::new(move |_| {
            let path = location.pathname.get();
            set_sentry_tag("route", &path);
        });
    }
}

#[cfg(test)]
mod error_filter_wiring {
    /// The beforeSend noise filter is maintained in error_filter.js and
    /// pulled into the reporting script via `include_str!`. The JS logic is
    /// exercised by `integration/error-filter.test.cjs`; this test guards the
    /// Rust↔JS contract — that the file is still compiled in and still carries
    /// each predicate's load-bearing token. If someone deletes one of these,
    /// the corresponding GlitchTip flood silently returns.
    const FILTER_JS: &str = include_str!("error_filter.js");

    #[test]
    fn filter_defines_the_drop_predicate_called_by_before_send() {
        assert!(
            FILTER_JS.contains("window.__ultrosShouldDropEvent ="),
            "error_filter.js must define the predicate beforeSend invokes",
        );
    }

    #[test]
    fn filter_keeps_each_noise_category_signal() {
        // Category 1: bundle fetch aborts, incl. the dynamic-import variant.
        assert!(FILTER_JS.contains("Failed to fetch dynamically imported module"));
        assert!(FILTER_JS.contains("WebAssembly compilation aborted"));
        // Category 2: injected-WebView document TypeError.
        assert!(FILTER_JS.contains("HTMLDocument.c"));
        // Category 3: hydration-panic flood, gated behind an injecting-
        // population fingerprint. The stale-Chrome check must read the live UA
        // (the browser tag is server-derived and absent in beforeSend), so the
        // navigator read is the load-bearing part.
        assert!(FILTER_JS.contains("isStaleChromeMajor"));
        assert!(FILTER_JS.contains("navigator.userAgent"));
        assert!(FILTER_JS.contains("hydration.rs"));
        // The cascade shapes are recognized WITHOUT the tachys breadcrumb (it
        // is not in the array the SDK hands beforeSend): the `RefCell already
        // borrowed` cascade via the js-sys futures-executor rust_panic location,
        // and the unhandled wasm trap via the exact RuntimeError "unreachable"
        // value. Deleting either silently re-opens the #6661/#4908/#6570 flood.
        assert!(FILTER_JS.contains("ULTROS_JSSYS_EXECUTOR_RE"));
        assert!(FILTER_JS.contains("\"unreachable\""));
        // Category 3 (modern-Chrome translation population): the injected
        // <font> DOM fingerprint that catches the flood the stale-UA check
        // misses. Removing it silently re-opens the #3005/#4911/#6406 flood.
        assert!(FILTER_JS.contains("hasInjectedTranslationFont"));
        assert!(FILTER_JS.contains("getElementsByTagName"));
        // Category 4: empty promise rejections.
        assert!(FILTER_JS.contains("Non-Error promise rejection captured with value: undefined"));
    }
}
