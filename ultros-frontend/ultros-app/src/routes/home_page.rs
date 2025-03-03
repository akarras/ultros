use icondata as i;
use leptos::prelude::*;
use leptos_icons::*;
use leptos_meta::*;

use crate::components::{
    ad::Ad, live_sale_ticker::LiveSaleTicker, meta::MetaDescription,
    recently_viewed::RecentlyViewed,
};

#[component]
fn FeatureCard(children: ChildrenFn) -> impl IntoView {
    view! {
        <div class="p-6 flex flex-col text-center rounded-2xl
        backdrop-blur-sm backdrop-brightness-110
        border border-white/10 hover:border-yellow-200/30
        transition-all duration-300 ease-in-out
        bg-gradient-to-br from-violet-900/20 via-black/10 to-amber-500/10
        hover:from-violet-800/30 hover:to-amber-400/20
        hover:transform hover:scale-[1.02] hover:shadow-lg hover:shadow-violet-500/10
        w-full aspect-[4/3] justify-center gap-3">{children().into_view()}</div>
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
                    <div class="text-2xl font-light bg-gradient-to-r from-violet-200 to-amber-200
                    bg-clip-text text-transparent p-4 rounded-xl
                    backdrop-blur-sm backdrop-brightness-110 border border-white/10">
                        <h1 class="font-bold mb-4 text-3xl">"Welcome to Ultros"</h1>
                        "Ultros is a modern market board tool for Final Fantasy 14."
                        <br />
                        "Get started by reading the "
                        <b>
                            <a
                                href="https://book.ultros.app"
                                class="text-amber-300 hover:text-amber-200 transition-colors"
                            >
                                "book"
                            </a>
                        </b>
                        " and inviting the "
                        <a
                            rel="external"
                            href="/invitebot"
                            class="text-amber-300 hover:text-amber-200 transition-colors"
                        >
                            "discord bot to your server"
                        </a>
                        "!"
                    </div>

                    // Feature cards grid
                    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
                        <a href="/items?menu-open=true">
                            <FeatureCard>
                                <Icon
                                    attr:class="text-amber-300"
                                    width="3.5em"
                                    height="3.5em"
                                    icon=i::FaScrewdriverWrenchSolid
                                />
                                <h3 class="font-bold text-xl text-amber-200">"Item Explorer"</h3>
                                <span class="text-gray-300">
                                    "Explore all the items on the market board"
                                </span>
                            </FeatureCard>
                        </a>
                        <a href="/analyzer">
                            <FeatureCard>
                                <Icon
                                    attr:class="text-amber-300"
                                    width="3.5em"
                                    height="3.5em"
                                    icon=i::FaMoneyBillTrendUpSolid
                                />
                                <h3 class="font-bold text-xl text-amber-200">"Analyzer"</h3>
                                <span class="text-gray-300">
                                    "Earn gil by buying low, selling high"
                                </span>
                            </FeatureCard>
                        </a>
                        <a href="/retainers">
                            <FeatureCard>
                                <Icon
                                    attr:class="text-amber-300"
                                    width="3.5em"
                                    height="3.5em"
                                    icon=i::BiGroupSolid
                                />
                                <h3 class="font-bold text-xl text-amber-200">"Retainers"</h3>
                                <span class="text-gray-300">"Track your retainers online"</span>
                            </FeatureCard>
                        </a>
                        <a href="/list">
                            <FeatureCard>
                                <Icon
                                    attr:class="text-amber-300"
                                    width="3.5em"
                                    height="3.5em"
                                    icon=i::AiOrderedListOutlined
                                />
                                <h3 class="font-bold text-xl text-amber-200">"Lists"</h3>
                                <span class="text-gray-300">
                                    "Create lists & buy the cheapest items"
                                </span>
                            </FeatureCard>
                        </a>
                        <a rel="external" href="/invitebot">
                            <FeatureCard>
                                <Icon
                                    attr:class="text-amber-300"
                                    width="3.5em"
                                    height="3.5em"
                                    icon=i::BsDiscord
                                />
                                <h3 class="font-bold text-xl text-amber-200">"Discord Bot"</h3>
                                <span class="text-gray-300">
                                    "Get alerts when your retainer is undercut"
                                </span>
                            </FeatureCard>
                        </a>
                        <a href="/currency-exchange">
                            <FeatureCard>
                                <Icon
                                    attr:class="text-amber-300"
                                    width="3.5em"
                                    height="3.5em"
                                    icon=i::RiExchangeFinanceLine
                                />
                                <h3 class="font-bold text-xl text-amber-200">
                                    "Currency Exchange"
                                </h3>
                                <span class="text-gray-300">"Spend tomestones, get gil"</span>
                            </FeatureCard>
                        </a>
                    </div>

                    <Ad class="w-96 aspect-[21/9] rounded-2xl overflow-hidden" />
                </div>
            </div>
        </div>
    }.into_any()
}
