// use crate::components::live_sale_ticker::*;
use leptos::*;
use leptos_meta::*;

#[component]
pub fn HomePage(cx: Scope) -> impl IntoView {
    view! {cx,
    <div class="main-content">
        <h1>"Ultros Alpha"</h1>
        <Title text="Ultros The Ultra Fast Market Tool"/>
        // <LiveSaleTicker />
        <div class="flex-wrap">
            <div class="content-well">
                <span class="content-title">"Analyzer"</span>
                <br/>
                <ul>
                    <li>"Make tons of gil reselling items"</li>
                    <li>"Quickly filter by roi, profit, and estimated sale date"</li>
                </ul>
            </div>
            <div class="content-well">
                <span class="content-title">"Retainers"</span>
                <br/>
                <ul>
                    <li>"Track your sales without logging in"</li>
                    <li>"Update all listings faster by only updating listings that are actually undercut"</li>
                    <li>"Get alerted on Discord or on this site when someone undercuts you"</li>
                </ul>
            </div>
            <div class="content-well">
                <span class="content-title">"Lists"</span>
                <br/>
                <ul>
                    <li>"Make shopping lists and find the cheapest prices"</li>
                    <li>"Import entire recipes"</li>
                </ul>
            </div>
            <div class="content-well">
                <span class="content-title">"Discord Bot"</span>
                <br/>
                <ul>
                    <li>"Use many features of the site through a Discord bot"</li>
                    <li>"Get alerts via notifications through the bot"</li>
                </ul>
            </div>
        </div>
    </div>}
}
