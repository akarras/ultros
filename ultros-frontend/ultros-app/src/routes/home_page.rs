use crate::components::icon::Icon;
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
fn FeatureCard(
    href: &'static str,
    title: AnyView,
    description: AnyView,
    #[prop(optional)] external: bool,
    #[prop(optional)] badge: Option<&'static str>,
    children: ChildrenFn,
) -> impl IntoView {
    let rel = if external { Some("external") } else { None };
    view! {
        <A href=href attr:rel=rel attr:class="group focus:outline-none rounded-2xl">
            <div class="feature-card w-full aspect-square flex flex-col items-center justify-center text-center gap-3">
                <div aria-hidden="true">
                    {children().into_view()}
                </div>
                {badge.map(|b| view! { <span class="feature-badge">{b}</span> })}
                <h3 class="font-extrabold tracking-tight text-[color:var(--color-text)]">{title}</h3>
                <span class="feature-card-desc" style="color: var(--color-text)">{description}</span>
            </div>
        </A>
    }
    .into_any()
}

#[component]
pub fn HomePage() -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    view! {
        <MetaTitle title=move || t_string!(i18n, meta_title).to_string() />
        <MetaDescription text=move || t_string!(i18n, meta_description).to_string() />
        <div class="main-content p-2 sm:p-6">
            <div class="container flex flex-col gap-6 lg:flex-row-reverse mx-auto items-start max-w-7xl">
                // Right sidebar
                <div class="flex flex-col w-full lg:w-[424px] gap-6 sticky top-4">
                    <LiveSaleTicker />
                    <RecentlyViewed />
                    <Ad class="w-full aspect-square rounded-2xl overflow-hidden" />
                </div>

                // Main content
                <div class="flex flex-col grow gap-8">
                    <div class="panel p-4 sm:p-8 overflow-hidden relative">
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
                                    <a
                                        rel="external"
                                        href="https://book.ultros.app"
                                        class="btn-primary py-3 px-6 text-lg"
                                    >
                                        {move || t_string!(i18n, get_started)}
                                    </a>
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
                            <div class="w-full md:w-72 lg:w-80 aspect-square rounded-2xl elevated surface-blur flex items-center justify-center animate-float">
                                <Icon icon=i::FaMoneyBillTrendUpSolid width="4.5em" height="4.5em" attr:class="text-brand-300" />
                            </div>
                        </div>
                    </div>

                    <TopDeals />

                    // Feature cards grid
                                        <div class="feature-grid">
                        <FeatureCard href="/items?menu-open=true" title=t!(i18n, item_explorer).into_any() description=t!(i18n, item_explorer_desc).into_any() badge="New">
                            <Icon
                                attr:class="feature-card-icon"
                                width="3.5em"
                                height="3.5em"
                                icon=i::FaScrewdriverWrenchSolid
                            />
                        </FeatureCard>
                        <FeatureCard href="/flip-finder" title=t!(i18n, flip_finder).into_any() description=t!(i18n, flip_finder_desc).into_any()>
                            <Icon
                                attr:class="feature-card-icon"
                                width="3.5em"
                                height="3.5em"
                                icon=i::FaMoneyBillTrendUpSolid
                            />
                        </FeatureCard>
                        <FeatureCard href="/vendor-resale" title=t!(i18n, vendor_resale).into_any() description=t!(i18n, vendor_resale_desc).into_any()>
                            <Icon
                                attr:class="feature-card-icon"
                                width="3.5em"
                                height="3.5em"
                                icon=i::FaShopSolid
                            />
                        </FeatureCard>
                        <FeatureCard href="/recipe-analyzer" title=t!(i18n, recipe_analyzer).into_any() description=t!(i18n, recipe_analyzer_desc).into_any() badge="New">
                            <Icon
                                attr:class="feature-card-icon"
                                width="3.5em"
                                height="3.5em"
                                icon=i::FaHammerSolid
                            />
                        </FeatureCard>
                        <FeatureCard href="/leve-analyzer" title=t!(i18n, leve_analyzer).into_any() description=t!(i18n, leve_analyzer_desc).into_any() badge="New">
                            <Icon
                                attr:class="feature-card-icon"
                                width="3.5em"
                                height="3.5em"
                                icon=i::FaScrollSolid
                            />
                        </FeatureCard>
                        <FeatureCard href="/trends" title=t!(i18n, market_trends).into_any() description=t!(i18n, market_trends_desc).into_any() badge="New">
                            <Icon
                                attr:class="feature-card-icon"
                                width="3.5em"
                                height="3.5em"
                                icon=i::FaChartLineSolid
                            />
                        </FeatureCard>
                        <FeatureCard href="/retainers" title=t!(i18n, retainers).into_any() description=t!(i18n, retainers_desc).into_any()>
                            <Icon
                                attr:class="feature-card-icon"
                                width="3.5em"
                                height="3.5em"
                                icon=i::BiGroupSolid
                            />
                        </FeatureCard>
                        <FeatureCard href="/list" title=t!(i18n, lists).into_any() description=t!(i18n, lists_desc).into_any()>
                            <Icon
                                attr:class="feature-card-icon"
                                width="3.5em"
                                height="3.5em"
                                icon=i::AiOrderedListOutlined
                            />
                        </FeatureCard>
                        <FeatureCard href="/invitebot" external=true title=t!(i18n, discord_bot).into_any() description=t!(i18n, discord_bot_desc).into_any()>
                            <Icon
                                attr:class="feature-card-icon"
                                width="3.5em"
                                height="3.5em"
                                icon=i::BsDiscord
                            />
                        </FeatureCard>
                        <FeatureCard href="/currency-exchange" title=t!(i18n, currency_exchange).into_any() description=t!(i18n, currency_exchange_desc).into_any()>
                            <Icon
                                attr:class="feature-card-icon"
                                width="3.5em"
                                height="3.5em"
                                icon=i::RiExchangeFinanceLine
                            />
                        </FeatureCard>
                    </div>

                    <Ad class="w-96 aspect-[21/9] rounded-2xl overflow-hidden" />
                </div>
            </div>
        </div>
    }.into_any()
}
