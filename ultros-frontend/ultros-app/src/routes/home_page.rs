// use crate::components::live_sale_ticker::*;
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
        <MetaDescription text="Ultros is a FAST FFXIV marketboard analysis tool, keep up to date with all of your retainers and ensure you've got the best prices!" />
    <div class="main-content p-4">
        <Title text="Ultros - Home"/>
        <div class="container flex flex-col gap-2 lg:flex-row-reverse mx-auto items-start">
            <div class="flex flex-col md:min-w-[424px]">
                <LiveSaleTicker />
                <RecentlyViewed />
                <Ad />
            </div>
            <div class="flex flex-col grow">
                <div class="text-xl">
                    "Ultros is a marketboard tool for Final Fantasy 14."<br/>
                    "Get started by reading the "<b><a href="https://book.ultros.app">"book"</a></b>" and inviting the "
                    <a href="/invitebot">"discord bot to your server"</a>"!"
                </div>
                <div class="flex flex-1 flex-col md:flex-row md:flex-wrap gap-4">
                    <a href="/items?menu-open=true">
                        <FeatureCard>
                            <Icon width="1.75em" height="1.75em" icon=Icon::from(FaIcon::FaScrewdriverWrenchSolid) />
                            <h3 class="text-xl p-1 text-yellow-100">"Item Finder"</h3>
                            <span>"Explore all the items on the marketboard"</span>
                        </FeatureCard>
                    </a>
                    <a href="/analyzer">
                        <FeatureCard>
                            <Icon width="1.75em" height="1.75em" icon=Icon::from(FaIcon::FaMoneyBillTrendUpSolid)/>
                            <h3 class="text-xl p-1 text-yellow-100">"Analyzer"</h3>
                            <span>"Earn gil by buying low, selling high"</span>
                        </FeatureCard>
                    </a>
                    <a href="/retainers">
                        <FeatureCard>
                            <Icon width="1.75em" height="1.75em" icon=Icon::from(BiIcon::BiGroupSolid) />
                            <h3 class="text-xl p-1 text-yellow-100">"Retainers"</h3>
                            <span>"Track your retainers online"</span>
                        </FeatureCard>
                    </a>
                    <a href="/list">
                        <FeatureCard>
                            <Icon width="1.75em" height="1.75em" icon=Icon::from(AiIcon::AiOrderedListOutlined) />
                            <h3 class="text-xl text-yellow-100">"Lists"</h3>
                            <span>"Create lists & buy the cheapest items"</span>
                        </FeatureCard>
                    </a>
                    <a href="/invitebot">
                        <FeatureCard>
                            <Icon width="1.75em" height="1.75em" icon=Icon::from(BsIcon::BsDiscord) />
                            <span class="text-xl text-yellow-100">"Discord Bot"</span>
                            <span>
                                "Get alerts when your retainer is undercut"
                            </span>
                        </FeatureCard>
                    </a>
                </div>
                <Ad class="h-[50vh]" />
            </div>
        </div>
    </div>}
}
