use crate::components::icon::Icon;
use icondata as i;
use leptos::prelude::*;
use leptos_meta::*;
use leptos_router::components::A;

use crate::components::{
    ad::Ad, live_sale_ticker::LiveSaleTicker, meta::MetaDescription,
    recently_viewed::RecentlyViewed, top_deals::TopDeals,
};

#[component]
fn FeatureCard(
    href: &'static str,
    title: &'static str,
    description: &'static str,
    #[prop(optional)] external: bool,
    #[prop(optional)] badge: Option<&'static str>,
    children: ChildrenFn,
) -> impl IntoView {
    let aria = format!("{title} — {description}");
    let rel = if external { Some("external") } else { None };
    view! {
        <A href=href attr:rel=rel attr:aria-label=aria attr:class="group focus:outline-none h-full block">
            <div class="h-full p-6 rounded-2xl bg-[color:var(--surface-color)] border border-[color:var(--separator-color)] group-hover:border-[color:var(--brand-ring)] group-hover:shadow-lg transition-all duration-200 relative overflow-hidden flex flex-col items-start gap-4">
                {badge.map(|b| view! {
                    <span class="absolute top-4 right-4 px-2 py-1 text-xs font-bold uppercase tracking-wider text-[color:var(--brand-fg)] bg-[color:var(--brand-bg)] rounded-full">
                        {b}
                    </span>
                })}
                <div class="text-[color:var(--brand-fg)] group-hover:scale-110 transform transition-transform duration-200 origin-left">
                    {children().into_view()}
                </div>
                <div class="flex flex-col gap-1 z-10">
                    <h3 class="text-xl font-bold text-[color:var(--color-text)] group-hover:text-[color:var(--brand-fg)] transition-colors">
                        {title}
                    </h3>
                    <p class="text-[color:var(--color-text-muted)] text-sm leading-relaxed">
                        {description}
                    </p>
                </div>
                <div class="absolute -bottom-10 -right-10 w-32 h-32 bg-[color:var(--brand-bg)] rounded-full blur-3xl opacity-0 group-hover:opacity-10 transition-opacity pointer-events-none"></div>
            </div>
        </A>
    }
    .into_any()
}

#[component]
pub fn HomePage() -> impl IntoView {
    view! {
        <MetaDescription text="Ultros is a fast market board analysis tool, keep up to date with all of your retainers and ensure you've got the best prices!" />
        <div class="main-content p-2 sm:p-6">
            <Title text="Ultros - Home" />
            <div class="container flex flex-col gap-6 lg:flex-row-reverse mx-auto items-start max-w-7xl">
                // Right sidebar
                <div class="flex flex-col w-full lg:w-[424px] gap-6 sticky top-4">
                    <LiveSaleTicker />
                    <RecentlyViewed />
                    <Ad class="w-full aspect-square rounded-2xl overflow-hidden" />
                </div>

                // Main content
                <div class="flex flex-col grow gap-8">
                    <div class="panel p-6 sm:p-10 overflow-hidden relative rounded-3xl">
                        <div class="flex flex-col md:flex-row items-center gap-8 md:gap-12">
                            <div class="flex-1 space-y-6 z-10">
                                <h1 class="text-6xl sm:text-7xl lg:text-8xl font-black leading-none tracking-tighter drop-shadow-xl">
                                    <span class="bg-clip-text text-transparent bg-gradient-to-br from-brand-300 via-purple-400 to-pink-500 filter drop-shadow-sm">"Ultros"</span>
                                    <span class="block text-brand-100 text-xl sm:text-2xl mt-4 font-bold tracking-wide opacity-90 uppercase">"Market Analytics for FFXIV"</span>
                                </h1>
                                <p class="text-lg text-[color:var(--color-text-muted)] max-w-prose leading-relaxed">
                                    "Buy low, sell high, and track your retainers with the fastest, modern market board tool."
                                </p>
                                <div class="flex flex-wrap items-center gap-3 pt-2">
                                    <a
                                        rel="external"
                                        href="https://book.ultros.app"
                                        class="btn-primary py-3 px-6 text-lg shadow-lg hover:shadow-brand-500/20"
                                    >
                                        "Get Started"
                                    </a>
                                    <A href="/flip-finder" attr:class="btn-primary py-3 px-6 text-lg shadow-lg hover:shadow-brand-500/20">
                                        <Icon icon=i::FaMoneyBillTrendUpSolid width="1.25em" height="1.25em" />
                                        <span>"Open Flip Finder"</span>
                                    </A>
                                    <a
                                        rel="external"
                                        href="/invitebot"
                                        class="btn-secondary py-3 px-6 text-lg"
                                    >
                                        <Icon icon=i::BsDiscord width="1.25em" height="1.25em" />
                                        <span>"Invite Bot"</span>
                                    </a>
                                </div>
                            </div>
                            <div class="w-full md:w-64 lg:w-80 aspect-square rounded-3xl elevated surface-blur flex items-center justify-center animate-float ring-1 ring-white/10">
                                <Icon icon=i::FaMoneyBillTrendUpSolid width="5em" height="5em" attr:class="text-brand-300 drop-shadow-lg" />
                            </div>
                        </div>
                    </div>

                    <TopDeals />

                    // Feature cards grid
                    <div class="grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-3 gap-4">
                        <FeatureCard href="/items?menu-open=true" title="Item Explorer" description="Explore all items on the market board" badge="New">
                            <Icon
                                width="3em"
                                height="3em"
                                icon=i::FaScrewdriverWrenchSolid
                            />
                        </FeatureCard>
                        <FeatureCard href="/flip-finder" title="Flip Finder" description="Earn gil by buying low and selling high">
                            <Icon
                                width="3em"
                                height="3em"
                                icon=i::FaMoneyBillTrendUpSolid
                            />
                        </FeatureCard>
                        <FeatureCard href="/recipe-analyzer" title="Recipe Analyzer" description="Calculate profits for crafting recipes" badge="New">
                            <Icon
                                width="3em"
                                height="3em"
                                icon=i::FaHammerSolid
                            />
                        </FeatureCard>
                        <FeatureCard href="/leve-analyzer" title="Leve Analyzer" description="Find the most profitable Levequests" badge="New">
                            <Icon
                                width="3em"
                                height="3em"
                                icon=i::FaScrollSolid
                            />
                        </FeatureCard>
                        <FeatureCard href="/trends" title="Market Trends" description="View top market movers and trends" badge="New">
                            <Icon
                                width="3em"
                                height="3em"
                                icon=i::FaChartLineSolid
                            />
                        </FeatureCard>
                        <FeatureCard href="/retainers" title="Retainers" description="Track your retainers and undercuts online">
                            <Icon
                                width="3em"
                                height="3em"
                                icon=i::BiGroupSolid
                            />
                        </FeatureCard>
                        <FeatureCard href="/list" title="Shopping Lists" description="Create lists & buy the cheapest items">
                            <Icon
                                width="3em"
                                height="3em"
                                icon=i::AiOrderedListOutlined
                            />
                        </FeatureCard>
                        <FeatureCard href="/invitebot" external=true title="Discord Bot" description="Get alerts when your retainer is undercut">
                            <Icon
                                width="3em"
                                height="3em"
                                icon=i::BsDiscord
                            />
                        </FeatureCard>
                        <FeatureCard href="/currency-exchange" title="Currency Exchange" description="Convert tomestones and scrips to gil">
                            <Icon
                                width="3em"
                                height="3em"
                                icon=i::RiExchangeFinanceLine
                            />
                        </FeatureCard>
                    </div>

                    <Ad class="w-96 aspect-[21/9] rounded-2xl overflow-hidden self-center" />
                </div>
            </div>
        </div>
    }.into_any()
}
