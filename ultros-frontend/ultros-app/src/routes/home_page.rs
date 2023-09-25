use chrono::Utc;
// use crate::components::live_sale_ticker::*;
use leptos::*;
use leptos_icons::*;
use leptos_meta::*;
use ultros_api_types::{ActiveListing, Retainer};

use crate::{
    components::{ad::Ad, gil::Gil, live_sale_ticker::LiveSaleTicker, related_items::RelatedItems},
    routes::retainers::CharacterRetainerList,
};

#[component]
pub fn HomePage() -> impl IntoView {
    view! {
    <div class="main-content p-4">
        <div class="flex flex-col container mx-auto">
            <h1 class="text-3xl">"Ultros Alpha"</h1>
            <Title text="Ultros The Ultra Fast Market Tool"/>
            <span class="text-2xl">"Live sales"</span>
            <LiveSaleTicker />
            <div>
                <div class="flex p-8 flex-col md:flex-row">
                    <div class="overflow-hidden md:w-[40vw] h-96">
                        <RelatedItems item_id=2 />
                    </div>
                    <ul>
                        <h2 class="text-3xl p-1">"Fast Prices"</h2>
                        <ul class="list-disc text-xl p-2">
                            <li>"quickly search prices"</li>
                            <li>"discover items with a similar item level"</li>
                            <li>"view associated recipes with an item and the price to craft it"</li>
                        </ul>
                    </ul>
                </div>
            </div>
            <Ad />
            <div class="p-8 grow flex-auto flex flex-col md:flex-row">
                <div class="flex flex-col">
                    <span class="text-3xl p-1">"Analyzer"</span>
                    <ul class="list-disc text-xl p-2">
                        <li>"Make tons of gil with market arbitrage, find items cheap on other worlds and resell on yours"</li>
                        <li>"Quickly filter by roi, profit, and estimated sale date"</li>
                    </ul>
                </div>
                <div class="flex md:ml-20 flex-col text-right align-top">
                    <div class="flex flex-row text-red-700 gap-1">"BUY:" <Gil amount=30000/>"on Balmung"</div>
                    <div class="flex flex-row text-green-700 gap-1">"SELL:" <Gil amount=100000/>"on Gilgamesh"</div>
                    <div class="flex flex-row text-green-400 gap-1">"PROFIT:"<Gil amount={100000 - 30000}/></div>
                </div>
            </div>
            <div class="p-8 grow flex-auto flex flex-col md:flex-row items-start">
                <div class="flex flex-col">
                    <CharacterRetainerList character=None retainers=vec![(Retainer {
                        id: 0,
                        world_id: 9,
                        name: "Retainer 1".to_string(),
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
                    <CharacterRetainerList character=None retainers=vec![(Retainer {
                        id: 0,
                        world_id: 9,
                        name: "Retainer 2".to_string(),
                        retainer_city_id: 1
                    }, vec![ActiveListing {
                        id: 0,
                        world_id: 3,
                        item_id: 39643,
                        retainer_id: 0,
                        price_per_unit: 13000,
                        quantity: 3,
                        hq: true,
                        timestamp: Utc::now().naive_utc(),
                    }, ActiveListing {
                        id: 0,
                        world_id: 3,
                        item_id: 34693,
                        retainer_id: 0,
                        price_per_unit: 13000,
                        quantity: 1,
                        hq: true,
                        timestamp: Utc::now().naive_utc(),
                    }])] />
                </div>
                <div class="flex flex-col p-3">
                    <span class="text-3xl p-1">"Retainers"</span>
                    <br/>
                    <ul class="list-disc text-xl p-2">
                        <li>"Track your retainer's listings online"</li>
                        <li>"View undercut items in one place"</li>
                        <li>"Get alerted on Discord or on this site when someone undercuts you, try /ffxiv retainer add_undercut_alert"</li>
                    </ul>
                </div>
            </div>
            <div class="p-8 grow">
                <span class="content-title">"Lists"</span>
                <br/>
                <ul>
                    <li>"Make shopping lists and find the cheapest prices"</li>
                    <li>"Import entire recipes"</li>
                    <li>"Honestly not as good as Teamcraft, but maybe it'll be better one day"</li>
                </ul>
            </div>
            <div class="p-8 flex flex-col md:flex-row">
                <div class="flex flex-col p-2">
                    <span class="content-title">"Discord Bot"</span>
                    <br/>
                    <ul>
                        <li>"Use many features of the site through a Discord bot"</li>
                        <li>"Get alerts via notifications through the bot"</li>
                    </ul>
                </div>
                <a class="flex flex-col p-4 bg-violet-900 text-6xl rounded-md b-solid border-2 border-violet-950 text-white items-center text-center ml-10" href="/invitebot">
                    <Icon icon=Icon::from(BsIcon::BsDiscord) />
                    <span>"Invite Bot"</span>
                </a>
            </div>
        </div>
    </div>}
}
