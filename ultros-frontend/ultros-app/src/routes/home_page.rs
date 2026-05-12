use crate::components::icon::Icon;
use crate::global_state::home_world::use_home_world;
use crate::i18n::{t, t_string};
use icondata as i;
use leptos::prelude::*;
use leptos_router::components::A;

use crate::components::{
    ad::Ad,
    live_sale_ticker::LiveSaleTicker,
    meta::{MetaDescription, MetaTitle},
    recently_viewed::RecentlyViewed,
    top_deals::TopDeals,
};

#[component]
fn ToolChip(href: &'static str, label: AnyView, children: ChildrenFn) -> impl IntoView {
    view! {
        <A
            href=href
            attr:class="group flex flex-col items-center justify-center gap-2 px-4 py-3 rounded-lg border border-[color:var(--color-outline)] hover:border-[color:color-mix(in_srgb,var(--brand-ring)_40%,var(--color-outline))] hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_8%,transparent)] transition-colors min-w-[120px] text-center"
        >
            <span class="text-[color:var(--brand-ring)] group-hover:text-[color:var(--color-text)] transition-colors" aria-hidden="true">
                {children().into_view()}
            </span>
            <span class="text-sm font-medium text-[color:var(--color-text)] whitespace-nowrap">{label}</span>
        </A>
    }
    .into_any()
}

#[component]
pub fn HomePage() -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    let (homeworld, _) = use_home_world();
    let needs_onboarding = Memo::new(move |_| homeworld.with(|w| w.is_none()));
    view! {
        <MetaTitle title=move || t_string!(i18n, meta_title).to_string() />
        <MetaDescription text=move || t_string!(i18n, meta_description).to_string() />
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

                    <TopDeals />

                    // Tool rail — flat chips instead of card grid
                    <div>
                        <h2 class="text-sm uppercase tracking-wider text-[color:var(--color-text-muted)] mb-3 px-1">{t!(i18n, side_nav_tools)}</h2>
                        <div class="flex max-w-full gap-3 overflow-x-auto pb-2 -mx-2 px-2 scroll-snap-x snap-x">
                            <ToolChip href="/items" label=t!(i18n, item_explorer).into_any()>
                                <Icon width="1.75em" height="1.75em" icon=i::FaScrewdriverWrenchSolid />
                            </ToolChip>
                            <ToolChip href="/flip-finder" label=t!(i18n, flip_finder).into_any()>
                                <Icon width="1.75em" height="1.75em" icon=i::FaMoneyBillTrendUpSolid />
                            </ToolChip>
                            <ToolChip href="/vendor-resale" label=t!(i18n, vendor_resale).into_any()>
                                <Icon width="1.75em" height="1.75em" icon=i::FaShopSolid />
                            </ToolChip>
                            <ToolChip href="/recipe-analyzer" label=t!(i18n, recipe_analyzer).into_any()>
                                <Icon width="1.75em" height="1.75em" icon=i::FaHammerSolid />
                            </ToolChip>
                            <ToolChip href="/leve-analyzer" label=t!(i18n, leve_analyzer).into_any()>
                                <Icon width="1.75em" height="1.75em" icon=i::FaScrollSolid />
                            </ToolChip>
                            <ToolChip href="/trends" label=t!(i18n, market_trends).into_any()>
                                <Icon width="1.75em" height="1.75em" icon=i::FaChartLineSolid />
                            </ToolChip>
                            <ToolChip href="/retainers" label=t!(i18n, retainers).into_any()>
                                <Icon width="1.75em" height="1.75em" icon=i::BiGroupSolid />
                            </ToolChip>
                            <ToolChip href="/list" label=t!(i18n, lists).into_any()>
                                <Icon width="1.75em" height="1.75em" icon=i::AiOrderedListOutlined />
                            </ToolChip>
                            <ToolChip href="/currency-exchange" label=t!(i18n, currency_exchange).into_any()>
                                <Icon width="1.75em" height="1.75em" icon=i::RiExchangeFinanceLine />
                            </ToolChip>
                        </div>
                    </div>

                    <Ad class="w-full max-w-96 aspect-[21/9] rounded-2xl overflow-hidden" />
                </div>
            </div>
        </div>
    }.into_any()
}
