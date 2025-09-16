use icondata as i;
use leptos::prelude::*;
use leptos_icons::*;
use leptos_meta::*;
use leptos_router::components::A;

use crate::components::{
    ad::Ad, live_sale_ticker::LiveSaleTicker, meta::MetaDescription,
    recently_viewed::RecentlyViewed,
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
        <A href=href attr:rel=rel attr:aria-label=aria attr:class="group focus:outline-none rounded-2xl">
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
    view! {
        <MetaDescription text="Ultros is a fast market board analysis tool, keep up to date with all of your retainers and ensure you've got the best prices!" />
        <div class="main-content p-6">
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
                    <div class="panel p-6 sm:p-8 overflow-hidden relative">
                        <div class="flex flex-col md:flex-row items-center gap-6 md:gap-10">
                            <div class="flex-1 space-y-4">
                                <h1 class="text-4xl sm:text-5xl font-extrabold leading-tight">
                                    <span class="bg-clip-text text-transparent bg-gradient-to-r from-brand-200 to-brand-100">"Ultros"</span>
                                    <span class="block text-[color:var(--color-text)] text-2xl sm:text-3xl mt-2 font-semibold">"market board analytics for final fantasy xiv"</span>
                                </h1>
                                <p class="text-lg text-[color:var(--color-text)] max-w-prose">
                                    "buy low, sell high, and track your retainers with a fast, modern ui."
                                </p>
                                <div class="flex flex-wrap items-center gap-3 pt-2">
                                    <a
                                        rel="external"
                                        href="https://book.ultros.app"
                                        class="btn-primary"
                                    >
                                        "get started"
                                    </a>
                                    <a
                                        rel="external"
                                        href="/invitebot"
                                        class="btn-secondary"
                                    >
                                        <Icon icon=i::BsDiscord width="1.25em" height="1.25em" />
                                        <span>"invite bot"</span>
                                    </a>
                                    <A href="/analyzer" attr:class="btn-primary">
                                        <Icon icon=i::FaMoneyBillTrendUpSolid width="1.25em" height="1.25em" />
                                        <span>"open analyzer"</span>
                                    </A>
                                </div>
                            </div>
                            <div class="w-full md:w-72 lg:w-80 aspect-square rounded-2xl elevated surface-blur flex items-center justify-center">
                                <Icon icon=i::FaMoneyBillTrendUpSolid width="4.5em" height="4.5em" attr:class="text-brand-300" />
                            </div>
                        </div>
                    </div>

                    // Feature cards grid
                                        <div class="feature-grid">
                        <FeatureCard href="/items?menu-open=true" title="Item Explorer" description="Explore all the items on the market board" badge="New">
                            <Icon
                                attr:class="feature-card-icon"
                                width="3.5em"
                                height="3.5em"
                                icon=i::FaScrewdriverWrenchSolid
                            />
                        </FeatureCard>
                        <FeatureCard href="/analyzer" title="Analyzer" description="Earn gil by buying low, selling high">
                            <Icon
                                attr:class="feature-card-icon"
                                width="3.5em"
                                height="3.5em"
                                icon=i::FaMoneyBillTrendUpSolid
                            />
                        </FeatureCard>
                        <FeatureCard href="/retainers" title="Retainers" description="Track your retainers online">
                            <Icon
                                attr:class="feature-card-icon"
                                width="3.5em"
                                height="3.5em"
                                icon=i::BiGroupSolid
                            />
                        </FeatureCard>
                        <FeatureCard href="/list" title="Lists" description="Create lists & buy the cheapest items">
                            <Icon
                                attr:class="feature-card-icon"
                                width="3.5em"
                                height="3.5em"
                                icon=i::AiOrderedListOutlined
                            />
                        </FeatureCard>
                        <FeatureCard href="/invitebot" external=true title="Discord Bot" description="Get alerts when your retainer is undercut">
                            <Icon
                                attr:class="feature-card-icon"
                                width="3.5em"
                                height="3.5em"
                                icon=i::BsDiscord
                            />
                        </FeatureCard>
                        <FeatureCard href="/currency-exchange" title="Currency Exchange" description="Spend tomestones, get gil">
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
