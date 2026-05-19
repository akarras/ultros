use crate::components::icon::Icon;
use crate::global_state::home_world::use_home_world;
use crate::i18n::{t, t_string};
use icondata as i;
use leptos::prelude::*;
use leptos_meta::Script;
use leptos_router::components::A;

use crate::components::{
    ad::Ad,
    live_sale_ticker::LiveSaleTicker,
    market_heat::MarketHeat,
    market_movers::MarketMovers,
    market_pulse::MarketPulse,
    meta::{MetaCanonical, MetaDescription, MetaImage, MetaTitle},
    recently_viewed::RecentlyViewed,
    top_opportunity::TopOpportunities,
};

/// JSON-LD structured data for Google. Two graphs:
///
/// 1. `WebSite` — declares the canonical URL, name, and a `SearchAction`
///    pointing at the item explorer. Eligible to render a sitelinks
///    search box for the brand "ultros" in Google results.
/// 2. `SoftwareApplication` — positions Ultros as a free FFXIV market-board
///    tool. Helps Google understand the app's category and surface it for
///    "ffxiv market board tool" style queries.
///
/// Static at build time so we can inline it as a constant — no per-render
/// allocation. Pre-escaped (no `<`, no `</script>` substrings) and JSON
/// strings only contain ASCII so we can embed safely without
/// `escape_for_script_tag`.
const HOME_JSON_LD: &str = r#"{
  "@context": "https://schema.org",
  "@graph": [
    {
      "@type": "WebSite",
      "@id": "https://ultros.app/#website",
      "url": "https://ultros.app/",
      "name": "Ultros",
      "description": "FFXIV market board analytics, retainer tracking, crafting profit calculators, and Discord alerts for Final Fantasy XIV.",
      "inLanguage": "en",
      "publisher": {"@type": "Organization", "name": "Ultros", "url": "https://ultros.app/"},
      "potentialAction": {
        "@type": "SearchAction",
        "target": {"@type": "EntryPoint", "urlTemplate": "https://ultros.app/items?search={search_term_string}"},
        "query-input": "required name=search_term_string"
      }
    },
    {
      "@type": "SoftwareApplication",
      "name": "Ultros",
      "url": "https://ultros.app/",
      "description": "Final Fantasy XIV market board analytics — flip finder, recipe profit calculator, retainer undercut alerts, and Discord bot integration.",
      "applicationCategory": "WebApplication",
      "operatingSystem": "Web",
      "browserRequirements": "Requires JavaScript and WebAssembly. Modern browser recommended.",
      "offers": {"@type": "Offer", "price": "0", "priceCurrency": "USD"},
      "featureList": [
        "Real-time FFXIV market board listings",
        "Flip finder for cross-world arbitrage",
        "Recipe and Free Company crafting profit analyzer",
        "Levequest and Venture profitability calculators",
        "Retainer undercut alerts via Discord"
      ]
    }
  ]
}"#;

#[component]
fn ToolChip(href: &'static str, label: AnyView, children: ChildrenFn) -> impl IntoView {
    // Icon rail entry: large icon on top, label beneath, no border or
    // background by default. Accent glow appears on hover so the rail
    // stays quiet at rest and signals intent on focus.
    view! {
        <A
            href=href
            attr:class="group flex flex-col items-center justify-center gap-2 px-3 py-3 rounded-lg hover:bg-[color:color-mix(in_srgb,var(--accent)_8%,transparent)] focus:outline-none focus:ring-2 focus:ring-[color:var(--accent)]/40 transition-colors min-w-[88px] text-center"
        >
            <span class="text-[color:var(--accent)] group-hover:text-[color:var(--color-text)] group-hover:drop-shadow-[0_0_6px_var(--accent-glow)] transition-all" aria-hidden="true">
                {children().into_view()}
            </span>
            <span class="text-xs font-medium text-[color:var(--color-text-muted)] group-hover:text-[color:var(--color-text)] whitespace-nowrap transition-colors">{label}</span>
        </A>
    }
    .into_any()
}

/// Time-of-day greeting bucket. Computed once on hydration from
/// the browser's local clock; the server renders `Evening` as a stable
/// default so SSR/CSR don't diverge during hydration.
///
/// `Morning` / `Afternoon` look "never constructed" to the SSR compiler
/// because they're only produced inside a `cfg(not(feature = "ssr"))`
/// branch — keep the allow until the cfg can be removed.
#[derive(Clone, Copy, PartialEq)]
#[allow(dead_code)]
enum Greeting {
    Morning,
    Afternoon,
    Evening,
}

impl Greeting {
    #[allow(dead_code)]
    fn from_hour(hour: u32) -> Self {
        match hour {
            5..=11 => Greeting::Morning,
            12..=17 => Greeting::Afternoon,
            _ => Greeting::Evening,
        }
    }
}

#[component]
pub fn HomePage() -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    let (homeworld, _) = use_home_world();
    let needs_onboarding = Memo::new(move |_| homeworld.with(|w| w.is_none()));
    // Market Pulse needs a world name string; track home world reactively so
    // the strip refreshes when the user changes home world.
    let pulse_world: Signal<Option<String>> =
        Signal::derive(move || homeworld.with(|w| w.as_ref().map(|w| w.name.clone())));

    // Time-of-day greeting. Default to Evening so SSR matches the first
    // client render; an Effect updates it to the real local hour on
    // hydration so reactive view updates pick the right bucket without
    // a hydration mismatch.
    let (greeting, set_greeting) = signal(Greeting::Evening);
    Effect::new(move |_| {
        #[cfg(not(feature = "ssr"))]
        {
            let hour = js_sys::Date::new_0().get_hours();
            set_greeting.set(Greeting::from_hour(hour));
        }
        #[cfg(feature = "ssr")]
        {
            let _ = set_greeting;
        }
    });
    view! {
        <MetaTitle title=move || t_string!(i18n, meta_title).to_string() />
        <MetaDescription text=move || t_string!(i18n, meta_description).to_string() />
        <MetaImage url="https://ultros.app/static/fallback-image.png" />
        <MetaCanonical href="https://ultros.app/" />
        <Script type_="application/ld+json">{HOME_JSON_LD}</Script>
        <div class="main-content p-2 sm:p-6">
            <div class="container flex w-full min-w-0 flex-col gap-6 lg:flex-row-reverse mx-auto items-start max-w-7xl">
                // Right sidebar
                <div class="flex flex-col w-full lg:w-[424px] gap-6 sticky top-4">
                    <LiveSaleTicker />
                    <RecentlyViewed />
                    <Ad class="w-full aspect-square rounded-2xl overflow-hidden" />
                </div>

                // Main content
                <div class="flex w-full min-w-0 flex-col grow gap-8">
                    {move || needs_onboarding.get().then(|| view! {
                        <A
                            href="/welcome"
                            attr:class="group focus:outline-none rounded-2xl"
                            attr:aria-label=move || t_string!(i18n, home_onboarding_banner_cta).to_string()
                        >
                            <div class="panel p-5 sm:p-6 rounded-2xl border-l-4 border-brand-300/70 flex flex-col items-start gap-4 hover:border-brand-300 transition-colors sm:flex-row sm:items-center">
                                <div class="p-3 rounded-xl bg-[color:var(--brand-bg)] text-[color:var(--brand-fg)] shrink-0">
                                    <Icon icon=i::FaMapLocationDotSolid width="1.75em" height="1.75em" />
                                </div>
                                <div class="min-w-0 flex-1">
                                    <h2 class="text-xl font-bold text-[color:var(--brand-fg)]">
                                        {t!(i18n, home_onboarding_banner_title)}
                                    </h2>
                                    <p class="text-sm text-[color:var(--color-text-muted)]">
                                        {t!(i18n, home_onboarding_banner_body)}
                                    </p>
                                </div>
                                <span class="btn-primary w-full justify-center py-2 px-4 group-hover:translate-x-0.5 transition-transform sm:w-auto">
                                    <span>{t!(i18n, home_onboarding_banner_cta)}</span>
                                    <Icon icon=i::FaArrowRightSolid width="0.9em" height="0.9em" />
                                </span>
                            </div>
                        </A>
                    })}
                    // Hero: command-center greeting when a home world is set,
                    // or the marketing pitch for new/anonymous visitors. We
                    // only show one or the other to keep the dashboard focused
                    // for returning traders.
                    {move || if pulse_world.with(|w| w.is_some()) {
                        view! {
                            <section class="command-greeting relative overflow-hidden pt-2 pb-6">
                                <div class="flex flex-col md:flex-row md:items-center md:justify-between gap-4 relative z-10">
                                    <div class="min-w-0 flex-1 space-y-1">
                                        <h1 class="text-4xl sm:text-5xl font-semibold tracking-tight text-[color:var(--color-text)] leading-tight">
                                            {move || match greeting.get() {
                                                Greeting::Morning => t_string!(i18n, home_greeting_morning).to_string(),
                                                Greeting::Afternoon => t_string!(i18n, home_greeting_afternoon).to_string(),
                                                Greeting::Evening => t_string!(i18n, home_greeting_evening).to_string(),
                                            }}
                                        </h1>
                                        <p class="text-base sm:text-lg text-[color:var(--color-text-muted)]">
                                            {move || {
                                                let world = pulse_world.with(|w| w.clone().unwrap_or_default());
                                                t_string!(i18n, home_greeting_subtitle)
                                                    .to_string()
                                                    .replace("%world%", &world)
                                            }}
                                        </p>
                                    </div>
                                    <div class="hidden md:flex md:w-32 lg:w-40 aspect-square items-center justify-center opacity-30 shrink-0">
                                        <Icon icon=i::FaMoneyBillTrendUpSolid width="3em" height="3em" attr:class="text-[color:var(--accent)]" />
                                    </div>
                                </div>
                            </section>
                        }.into_any()
                    } else {
                        view! {
                            <div class="p-4 sm:p-6 overflow-hidden relative">
                                <div class="flex flex-col md:flex-row items-center gap-6 md:gap-10">
                                    <div class="flex-1 space-y-4 z-10">
                                        <h1 class="text-6xl sm:text-8xl font-extrabold leading-none tracking-tighter drop-shadow-2xl">
                                            <span class="bg-clip-text text-transparent bg-gradient-to-br from-brand-300 via-purple-400 to-pink-500 filter drop-shadow-sm animate-pulse">"Ultros"</span>
                                            <span class="block text-brand-100 text-xl sm:text-2xl mt-4 font-medium tracking-normal opacity-90">{move || t_string!(i18n, ultros_tagline)}</span>
                                        </h1>
                                        <p class="text-lg text-[color:var(--color-text-muted)] max-w-prose leading-relaxed">
                                            {move || t_string!(i18n, ultros_description)}
                                        </p>
                                        <div class="flex flex-wrap items-center gap-3 pt-4">
                                            <A
                                                href="/help/getting-started"
                                                attr:class="btn-primary py-3 px-6 text-lg"
                                            >
                                                {move || t_string!(i18n, get_started)}
                                            </A>
                                            <A href="/flip-finder" attr:class="btn-primary py-3 px-6 text-lg">
                                                <Icon icon=i::FaMoneyBillTrendUpSolid width="1.25em" height="1.25em" />
                                                <span>{move || t_string!(i18n, open_flip_finder)}</span>
                                            </A>
                                            <a
                                                rel="external"
                                                href="/invitebot"
                                                class="btn-secondary py-3 px-6 text-lg"
                                            >
                                                <Icon icon=i::BsDiscord width="1.25em" height="1.25em" />
                                                <span>{move || t_string!(i18n, invite_bot)}</span>
                                            </a>
                                        </div>
                                    </div>
                                    <div class="hidden md:flex md:w-56 lg:w-64 aspect-square items-center justify-center animate-float opacity-60">
                                        <Icon icon=i::FaMoneyBillTrendUpSolid width="4.5em" height="4.5em" attr:class="text-brand-300" />
                                    </div>
                                </div>
                            </div>
                        }.into_any()
                    }}

                    // Market Pulse + Market Movers — only render when we have a
                    // home world; otherwise the onboarding banner above is the
                    // right call to action.
                    {move || pulse_world.with(|w| w.is_some()).then(|| view! {
                        <MarketPulse world=pulse_world />
                        <MarketHeat world=pulse_world />
                        // Two-column on desktop: Top Opportunity (left) + Market
                        // Movers (right). On mobile they stack vertically.
                        <div class="grid grid-cols-1 lg:grid-cols-2 gap-4">
                            <TopOpportunities world=pulse_world />
                            <MarketMovers world=pulse_world />
                        </div>
                    })}

                    // Tool rail — icon-prominent quick access. The rail
                    // scrolls horizontally on narrow viewports so the
                    // dashboard doesn't reflow into a chunky grid.
                    <section class="dashboard-section">
                        <h2 class="dashboard-section-title mb-3">{t!(i18n, side_nav_tools)}</h2>
                        <div class="flex max-w-full gap-1 overflow-x-auto pb-2 -mx-2 px-2 scroll-snap-x snap-x">
                            <ToolChip href="/items" label=t!(i18n, item_explorer).into_any()>
                                <Icon width="2em" height="2em" icon=i::FaScrewdriverWrenchSolid />
                            </ToolChip>
                            <ToolChip href="/flip-finder" label=t!(i18n, flip_finder).into_any()>
                                <Icon width="2em" height="2em" icon=i::FaMoneyBillTrendUpSolid />
                            </ToolChip>
                            <ToolChip href="/vendor-resale" label=t!(i18n, vendor_resale).into_any()>
                                <Icon width="2em" height="2em" icon=i::FaShopSolid />
                            </ToolChip>
                            <ToolChip href="/recipe-analyzer" label=t!(i18n, recipe_analyzer).into_any()>
                                <Icon width="2em" height="2em" icon=i::FaHammerSolid />
                            </ToolChip>
                            <ToolChip href="/leve-analyzer" label=t!(i18n, leve_analyzer).into_any()>
                                <Icon width="2em" height="2em" icon=i::FaScrollSolid />
                            </ToolChip>
                            <ToolChip href="/trends" label=t!(i18n, market_trends).into_any()>
                                <Icon width="2em" height="2em" icon=i::FaChartLineSolid />
                            </ToolChip>
                            <ToolChip href="/retainers" label=t!(i18n, retainers).into_any()>
                                <Icon width="2em" height="2em" icon=i::BiGroupSolid />
                            </ToolChip>
                            <ToolChip href="/list" label=t!(i18n, lists).into_any()>
                                <Icon width="2em" height="2em" icon=i::AiOrderedListOutlined />
                            </ToolChip>
                            <ToolChip href="/currency-exchange" label=t!(i18n, currency_exchange).into_any()>
                                <Icon width="2em" height="2em" icon=i::RiExchangeFinanceLine />
                            </ToolChip>
                        </div>
                    </section>

                    <Ad class="w-full max-w-96 aspect-[21/9] rounded-2xl overflow-hidden" />
                </div>
            </div>
        </div>
    }.into_any()
}
