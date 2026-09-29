use crate::components::app_link::AppLink;
use crate::components::icon::Icon;
use crate::global_state::home_world::{GuessedRegion, use_home_world};
use crate::global_state::local_world_data::use_world_helper;
use crate::global_state::platform::use_platform_hotkeys;
use crate::global_state::search_overlay::use_search_overlay_state;
use crate::i18n::{t, t_string};
use icondata as i;
use leptos::prelude::*;
use leptos_meta::Script;

use crate::components::{
    ad::Ad,
    live_sale_ticker::LiveSaleTicker,
    market_heat::MarketHeat,
    market_movers::MarketMovers,
    market_pulse::MarketPulse,
    meta::{MetaCanonical, MetaDescription, MetaTitle},
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
fn ToolChip(
    href: &'static str,
    label: AnyView,
    description: AnyView,
    children: ChildrenFn,
) -> impl IntoView {
    // Tool grid entry: icon beside a label and a one-line description. The
    // grid wraps instead of scrolling sideways, so every tool is visible
    // without a hidden overflow; the accent wash appears on hover/focus only.
    view! {
        <AppLink
            href=href
            attr:class="group flex items-start gap-3 p-3 rounded-xl hover:bg-[color:color-mix(in_srgb,var(--accent)_8%,transparent)] focus:outline-none focus:ring-2 focus:ring-[color:var(--accent)]/40 transition-colors min-w-0"
        >
            <span class="text-[color:var(--accent)] group-hover:text-[color:var(--color-text)] transition-colors shrink-0 pt-0.5" aria-hidden="true">
                {children().into_view()}
            </span>
            <span class="min-w-0">
                <span class="block text-sm font-semibold text-[color:var(--color-text)] truncate">{label}</span>
                <span class="block text-xs text-[color:var(--color-text-muted)] leading-snug line-clamp-2">{description}</span>
            </span>
        </AppLink>
    }
    .into_any()
}

/// One "what you can do" card on the logged-out home page: an outcome, not a
/// tool name, linking to the page that delivers it.
#[component]
fn FeatureCard(
    href: &'static str,
    icon: icondata::Icon,
    title: AnyView,
    body: AnyView,
) -> impl IntoView {
    view! {
        <AppLink
            href=href
            attr:class="panel group flex flex-col gap-2 p-4 rounded-2xl hover:border-[color:var(--accent)] focus:outline-none focus:ring-2 focus:ring-[color:var(--accent)]/40 transition-colors"
        >
            <span class="text-[color:var(--accent)]" aria-hidden="true">
                <Icon icon=icon width="1.5em" height="1.5em" />
            </span>
            <span class="text-base font-semibold text-[color:var(--color-text)]">{title}</span>
            <span class="text-sm text-[color:var(--color-text-muted)] leading-relaxed">{body}</span>
        </AppLink>
    }
    .into_any()
}

/// Region names are stored with a hyphen (`North-America`); show a space.
fn region_label(name: &str) -> String {
    name.replace('-', " ")
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
    let search_overlay = use_search_overlay_state();
    let apple_hotkeys = use_platform_hotkeys().apple;
    // Market Pulse needs a world name string; track home world reactively so
    // the strip refreshes when the user changes home world.
    let pulse_world: Signal<Option<String>> =
        Signal::derive(move || homeworld.with(|w| w.as_ref().map(|w| w.name.clone())));

    // Visitors without a home world still get live data: the whole region
    // the server guessed from their connection (`GuessedRegion`, available
    // during SSR), switchable to any other region. Nothing here writes a
    // cookie — only picking a home world does.
    let guessed_region = use_context::<GuessedRegion>().map(|r| r.0);
    let regions: Vec<String> = use_world_helper()
        .ok()
        .map(|helper| {
            helper
                .regions_ordered(guessed_region.as_deref())
                .iter()
                .map(|r| r.name.clone())
                .collect()
        })
        .unwrap_or_default();
    let initial_region = guessed_region
        .filter(|g| regions.contains(g))
        .or_else(|| regions.first().cloned())
        .unwrap_or_else(|| "North-America".to_string());
    let (browse_region, set_browse_region) = signal(initial_region);
    // The scope every live widget follows: the home world when set,
    // otherwise the region being browsed.
    let feed_scope: Signal<Option<String>> =
        Signal::derive(move || pulse_world.get().or_else(|| Some(browse_region.get())));

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
        <MetaCanonical href="https://ultros.app/" />
        <Script type_="application/ld+json">{HOME_JSON_LD}</Script>
        <div class="main-content p-2 sm:p-6">
            <div class="container flex w-full min-w-0 flex-col gap-6 lg:flex-row mx-auto items-start max-w-7xl">
                // Main content
                <div class="flex w-full min-w-0 flex-col grow gap-8">
                    // Returning traders (home world set) get the command-center
                    // greeting and their world's dashboard. Everyone else gets
                    // a short pitch, a search box, and live data for their
                    // region — the product working, rather than a description
                    // of it.
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
                                        <Icon icon=i::FaMoneyBillTrendUpSolid width="3em" height="3em" attr:class="text-[color:var(--accent-decor)]" />
                                    </div>
                                </div>
                            </section>
                            <MarketPulse world=pulse_world />
                            <MarketHeat world=pulse_world />
                            // Two-column on desktop: Top Opportunity (left) + Market
                            // Movers (right). On mobile they stack vertically.
                            <div class="grid grid-cols-1 lg:grid-cols-2 gap-4">
                                <TopOpportunities world=pulse_world />
                                <MarketMovers world=pulse_world />
                            </div>
                        }.into_any()
                    } else {
                        let regions = regions.clone();
                        view! {
                            <section class="flex flex-col gap-4 pt-2">
                                <h1 class="text-3xl sm:text-4xl font-semibold tracking-tight text-[color:var(--color-text)] leading-tight">
                                    {move || t_string!(i18n, ultros_tagline)}
                                </h1>
                                <p class="text-base sm:text-lg text-[color:var(--color-text-muted)] max-w-prose">
                                    {move || t_string!(i18n, ultros_description)}
                                </p>
                                // Opens the global search overlay — the same
                                // one as the sidebar row and Cmd/Ctrl+K — so
                                // there is one search implementation, styled
                                // here as a field because looking up an item
                                // is the most common reason to land here.
                                <button
                                    type="button"
                                    class="flex w-full max-w-xl items-center gap-3 rounded-xl border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--color-text)_4%,transparent)] px-4 py-3 text-left text-[color:var(--color-text-muted)] hover:border-[color:var(--accent)] focus:outline-none focus:ring-2 focus:ring-[color:var(--accent)]/40 transition-colors"
                                    on:click=move |_| search_overlay.open.set(true)
                                >
                                    <Icon icon=i::AiSearchOutlined width="1.25em" height="1.25em" />
                                    <span class="flex-1 truncate">
                                        {move || {
                                            let hotkey = if apple_hotkeys.get() {
                                                "⌘K".to_string()
                                            } else {
                                                t_string!(i18n, hotkey_ctrl_k).to_string()
                                            };
                                            t_string!(i18n, search_box_placeholder).replace("%hotkey%", &hotkey)
                                        }}
                                    </span>
                                </button>
                            </section>

                            <section class="flex flex-col gap-3 border-t border-[color:var(--line)] pt-6">
                                <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                                    <div class="min-w-0 space-y-1">
                                        <h2 class="text-xl font-semibold text-[color:var(--color-text)]">
                                            {move || {
                                                t_string!(i18n, home_region_live_title)
                                                    .to_string()
                                                    .replace("%region%", &region_label(&browse_region.get()))
                                            }}
                                        </h2>
                                        <p class="text-sm text-[color:var(--color-text-muted)] max-w-prose">
                                            {t!(i18n, home_region_live_hint)}
                                        </p>
                                    </div>
                                    <AppLink href="/welcome" attr:class="btn-primary py-2 px-4 shrink-0 self-start">
                                        <Icon icon=i::FaMapLocationDotSolid width="1em" height="1em" />
                                        <span>{move || t_string!(i18n, set_home_world)}</span>
                                    </AppLink>
                                </div>
                                <div
                                    class="flex flex-wrap gap-2"
                                    role="group"
                                    aria-label=move || t_string!(i18n, home_region_picker_label).to_string()
                                >
                                    {regions
                                        .into_iter()
                                        .map(|region| {
                                            let label = region_label(&region);
                                            let this = region.clone();
                                            let selected = Signal::derive(move || browse_region.with(|r| *r == this));
                                            view! {
                                                <button
                                                    type="button"
                                                    aria-pressed=move || if selected.get() { "true" } else { "false" }
                                                    class=move || if selected.get() {
                                                        "px-3 py-1.5 rounded-full text-xs font-semibold border transition-colors bg-[color:color-mix(in_srgb,var(--brand-ring)_18%,transparent)] text-[color:var(--color-text)] border-[color:color-mix(in_srgb,var(--brand-ring)_40%,var(--color-outline))]"
                                                    } else {
                                                        "px-3 py-1.5 rounded-full text-xs font-semibold border transition-colors bg-transparent text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)] border-[color:var(--color-outline)]"
                                                    }
                                                    on:click=move |_| set_browse_region.set(region.clone())
                                                >
                                                    {label}
                                                </button>
                                            }
                                        })
                                        .collect_view()}
                                </div>
                            </section>
                            <MarketPulse world=feed_scope />
                            <MarketMovers world=feed_scope />

                            <section class="dashboard-section">
                                <h2 class="dashboard-section-title mb-3">{t!(i18n, home_features_title)}</h2>
                                <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
                                    <FeatureCard
                                        href="/flip-finder"
                                        icon=i::FaMoneyBillTrendUpSolid
                                        title=t!(i18n, home_feature_flips_title).into_any()
                                        body=t!(i18n, home_feature_flips_body).into_any()
                                    />
                                    <FeatureCard
                                        href="/recipe-analyzer"
                                        icon=i::FaHammerSolid
                                        title=t!(i18n, home_feature_crafts_title).into_any()
                                        body=t!(i18n, home_feature_crafts_body).into_any()
                                    />
                                    <FeatureCard
                                        href="/retainers"
                                        icon=i::FaBellSolid
                                        title=t!(i18n, home_feature_alerts_title).into_any()
                                        body=t!(i18n, home_feature_alerts_body).into_any()
                                    />
                                    <FeatureCard
                                        href="/bot"
                                        icon=i::BsDiscord
                                        title=t!(i18n, home_feature_discord_title).into_any()
                                        body=t!(i18n, home_feature_discord_body).into_any()
                                    />
                                </div>
                            </section>
                        }.into_any()
                    }}

                    <section class="dashboard-section">
                        <h2 class="dashboard-section-title mb-3">{t!(i18n, side_nav_tools)}</h2>
                        <div class="grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-3 gap-1">
                            <ToolChip href="/items" label=t!(i18n, item_explorer).into_any() description=t!(i18n, item_explorer_desc).into_any()>
                                <Icon width="1.5em" height="1.5em" icon=i::FaScrewdriverWrenchSolid />
                            </ToolChip>
                            <ToolChip href="/flip-finder" label=t!(i18n, flip_finder).into_any() description=t!(i18n, flip_finder_desc).into_any()>
                                <Icon width="1.5em" height="1.5em" icon=i::FaMoneyBillTrendUpSolid />
                            </ToolChip>
                            <ToolChip href="/vendor-resale" label=t!(i18n, vendor_resale).into_any() description=t!(i18n, vendor_resale_desc).into_any()>
                                <Icon width="1.5em" height="1.5em" icon=i::FaShopSolid />
                            </ToolChip>
                            <ToolChip href="/vendor-sell" label=t!(i18n, vendor_sell).into_any() description=t!(i18n, vendor_sell_desc).into_any()>
                                <Icon width="1.5em" height="1.5em" icon=i::FaCashRegisterSolid />
                            </ToolChip>
                            <ToolChip href="/recipe-analyzer" label=t!(i18n, recipe_analyzer).into_any() description=t!(i18n, recipe_analyzer_desc).into_any()>
                                <Icon width="1.5em" height="1.5em" icon=i::FaHammerSolid />
                            </ToolChip>
                            <ToolChip href="/leve-analyzer" label=t!(i18n, leve_analyzer).into_any() description=t!(i18n, leve_analyzer_desc).into_any()>
                                <Icon width="1.5em" height="1.5em" icon=i::FaScrollSolid />
                            </ToolChip>
                            <ToolChip href="/trends" label=t!(i18n, market_trends).into_any() description=t!(i18n, market_trends_desc).into_any()>
                                <Icon width="1.5em" height="1.5em" icon=i::FaChartLineSolid />
                            </ToolChip>
                            <ToolChip href="/retainers" label=t!(i18n, retainers).into_any() description=t!(i18n, retainers_desc).into_any()>
                                <Icon width="1.5em" height="1.5em" icon=i::BiGroupSolid />
                            </ToolChip>
                            <ToolChip href="/list" label=t!(i18n, lists).into_any() description=t!(i18n, lists_desc).into_any()>
                                <Icon width="1.5em" height="1.5em" icon=i::AiOrderedListOutlined />
                            </ToolChip>
                            <ToolChip href="/currency-exchange" label=t!(i18n, currency_exchange).into_any() description=t!(i18n, currency_exchange_desc).into_any()>
                                <Icon width="1.5em" height="1.5em" icon=i::RiExchangeFinanceLine />
                            </ToolChip>
                        </div>
                    </section>

                    <Ad class="w-full max-w-96 aspect-[21/9] rounded-2xl overflow-hidden" />
                </div>

                // Right sidebar. Sticky only from `lg`, where it is an actual
                // side column: below that the layout stacks, so pinning it
                // parks a transparent panel over the column scrolling behind
                // it and the two render on top of each other.
                <div class="flex flex-col w-full lg:w-[424px] gap-6 lg:sticky lg:top-4">
                    <LiveSaleTicker scope=feed_scope />
                    <RecentlyViewed />
                    <Ad class="w-full aspect-square rounded-2xl overflow-hidden" />
                </div>
            </div>
        </div>
    }.into_any()
}
