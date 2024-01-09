use chrono::Utc;
// use crate::components::live_sale_ticker::*;
use leptos::*;
use leptos_icons::*;
use leptos_meta::*;
use leptos_router::*;
use ultros_api_types::{ActiveListing, Retainer};

use crate::{
    components::{
        ad::Ad, gil::Gil, live_sale_ticker::LiveSaleTicker, meta::MetaDescription,
        recently_viewed::RecentlyViewed,
    },
    routes::retainers::CharacterRetainerList,
};

#[component]
pub fn HomePage() -> impl IntoView {
    view! {
        <MetaDescription text="Ultros is a FAST FFXIV marketboard analysis tool, keep up to date with all of your retainers and ensure you've got the best prices!" />
    <div class="main-content p-4">
        <div class="flex flex-col lg:flex-row-reverse mx-auto container items-start">
            <div class="flex flex-col md:w-[424px]">
                <LiveSaleTicker />
                <RecentlyViewed />
                <Ad />
            </div>
            <div class="flex flex-col">
                <h1 class="text-3xl">"Ultros Alpha"</h1>
                <Title text="Ultros: The Ultra Fast Market Tool"/>
                <div class="p-8 flex flex-col md:flex-row">
                    <div class="flex flex-col">
                        <h2 class="text-3xl p-1">"Fast Prices"</h2>
                        <ul class="list-disc text-xl p-2">
                            <li>"Fastest search in the west"</li>
                            <li>"View associated recipes and crafting costs"</li>
                            <li>"Explore prices for job gear, e.g., "<A href="/items/jobset/SAM">"all Samurai gear"</A></li>
                        </ul>
                    </div>
                </div>
                <div class="p-8 grow flex-auto flex flex-col md:flex-row">
                    <div class="flex flex-col">
                        <span class="text-3xl p-1">"Analyzer"</span>
                        <ul class="list-disc text-xl p-2">
                            <li>"Profit from market arbitrage"</li>
                            <li>"Filter by ROI, profit, and estimated sale date"</li>
                        </ul>
                    </div>
                    <div class="flex md:ml-20 flex-col text-right align-top">
                        <div class="flex flex-row text-red-700 gap-1">"BUY:" <Gil amount=30000/>"on Balmung"</div>
                        <div class="flex flex-row text-green-700 gap-1">"SELL:" <Gil amount=100000/>"on Gilgamesh"</div>
                        <div class="flex flex-row text-green-400 gap-1">"PROFIT:"<Gil amount={100000 - 30000}/></div>
                    </div>
                </div>
                <div class="grow relative">
                    <div class="flex flex-col absolute -z-40 right-0">
                        <CharacterRetainerList character=None retainers=vec![(Retainer {
                            id: 0,
                            world_id: 9,
                            name: "your-best-retainer".to_string(),
                            retainer_city_id: 1
                        }, vec![ActiveListing {
                            id: 0,
                            world_id: 3,
                            item_id: 4,
                            retainer_id: 0,
                            price_per_unit: 13000,
                            quantity: 3,
                            hq: true,
                            timestamp: Utc::now().naive_utc(),
                        }, ActiveListing {
                            id: 0,
                            world_id: 3,
                            item_id: 35578,
                            retainer_id: 0,
                            price_per_unit: 13000,
                            quantity: 1,
                            hq: true,
                            timestamp: Utc::now().naive_utc(),
                        }])] />
                    </div>
                    <div class="flex flex-col p-8 h-full w-full bg-gradient-to-br from-[#100a13] to-transparent">
                        <span class="text-3xl p-1">"Retainers"</span>
                        <ul class="list-disc text-xl p-2">
                            <li>"Track your retainer's listings online"</li>
                            <li>"View undercut items in one place"</li>
                            <li>"Get undercut alerts on Discord"</li>
                        </ul>
                    </div>
                </div>
                <div class="p-8 grow">
                    <span class="content-title">"Lists"</span>
                    <ul class="list-disc text-xl p-2">
                        <li>"Create shopping lists with the best prices"</li>
                        <li>"Import entire recipes"</li>
                        <li>"Ongoing improvements for a better experience"</li>
                    </ul>
                </div>
                <Ad />
                <div class="p-8 flex flex-col md:flex-row">
                    <div class="flex flex-col p-2">
                        <span class="content-title">"Discord Bot"</span>
                        <ul>
                            <li>"Use site features through a Discord bot"</li>
                            <li>"Receive alerts via bot notifications"</li>
                        </ul>
                    </div>
                    <a class="flex flex-col p-4 bg-slate-950 text-lg rounded-md b-solid border-2 border-violet-950 text-white items-center text-center ml-10" href="/invitebot">
                        <Icon icon=Icon::from(BsIcon::BsDiscord) />
                        <span>"Invite Bot"</span>
                    </a>
                </div>
            </div>
        </div>
    </div>}
}
