// use crate::components::live_sale_ticker::*;
use icondata as i;
use leptos::*;
use leptos_icons::*;
use leptos_meta::*;

use crate::components::{
    ad::Ad, live_sale_ticker::LiveSaleTicker, meta::MetaDescription,
    recently_viewed::RecentlyViewed,
};

#[component]
fn FeatureCard(children: ChildrenFn) -> impl IntoView {
    view! {
        <div class="p-2 flex flex-col text-center border rounded-xl hover:border-yellow-100 border-violet-950 items-center
                    transition-all duration-500 bg-gradient-to-br to-yellow-300 via-black from-violet-950 bg-size-200 bg-pos-0
                    hover:bg-pos-100 w-48 h-36">
            {children}
        </div>
    }
}

#[component]
pub fn HomePage() -> impl IntoView {
    view! {
        <MetaDescription text="Ultros is a fast market board analysis tool, keep up to date with all of your retainers and ensure you've got the best prices!" />
    <div class="main-content p-4">
        <Title text="Ultros - Home"/>
        <div class="container flex flex-col gap-2 lg:flex-row-reverse mx-auto items-start">
            <div class="flex flex-col md:min-w-[424px]">
                <LiveSaleTicker />
                <RecentlyViewed />
                <Ad class="w-96 h-96"/>
            </div>
            <div class="flex flex-col grow">
                <div class="text-xl">
                    "Ultros is a market board tool for Final Fantasy 14."<br/>
                    "Get started by reading the "<b><a href="https://book.ultros.app">"book"</a></b>" and inviting the "
                    <a rel="external" href="/invitebot">"discord bot to your server"</a>"!"
                </div>
                <div class="flex flex-1 flex-col md:flex-row md:flex-wrap gap-4">
                    <a href="/items?menu-open=true">
                        <FeatureCard>
                            <Icon width="3em" height="3em" icon=i::FaScrewdriverWrenchSolid />
                            <h3 class="font-bold p-1 text-yellow-100">"Item Explorer"</h3>
                            <span>"Explore all the items on the market board"</span>
                        </FeatureCard>
                    </a>
                    <a href="/analyzer">
                        <FeatureCard>
                            <Icon width="3em" height="3em" icon=i::FaMoneyBillTrendUpSolid/>
                            <h3 class="font-bold p-1 text-yellow-100">"Analyzer"</h3>
                            <span>"Earn gil by buying low, selling high"</span>
                        </FeatureCard>
                    </a>
                    <a href="/retainers">
                        <FeatureCard>
                            <Icon width="3em" height="3em" icon=i::BiGroupSolid />
                            <h3 class="font-bold p-1 text-yellow-100">"Retainers"</h3>
                            <span>"Track your retainers online"</span>
                        </FeatureCard>
                    </a>
                    <a href="/list">
                        <FeatureCard>
                            <Icon width="3em" height="3em" icon=i::AiOrderedListOutlined />
                            <h3 class="font-bold text-yellow-100">"Lists"</h3>
                            <span>"Create lists & buy the cheapest items"</span>
                        </FeatureCard>
                    </a>
                    <a rel="external" href="/invitebot">
                        <FeatureCard>
                            <Icon width="3em" height="3em" icon=i::BsDiscord />
                            <span class="font-bold text-yellow-100">"Discord Bot"</span>
                            <span>
                                "Get alerts when your retainer is undercut"
                            </span>
                        </FeatureCard>
                    </a>
                    <a href="/currency-exchange">
                        <FeatureCard>
                            <Icon width="3em" height="3em" icon=i::RiExchangeFinanceLine />
                            <h3 class="p-1 font-bold text-yellow-100">"Currency Exchange"</h3>
                            <span>"Spend tomestones, get gil"</span>
                        </FeatureCard>
                    </a>
                </div>
                <Ad class="min-h-40 max-h-[70vh] w-full" />
            </div>
        </div>
    </div>}
}
