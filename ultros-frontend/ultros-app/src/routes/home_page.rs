use crate::components::live_sale_ticker::*;
use leptos::*;

#[component]
pub fn HomePage(cx: Scope) -> impl IntoView {
    view! {cx,
    <div class="container">
        <div class="main-content">
            <h1>"Ultros Alpha"</h1>
            <LiveSaleTicker />
            <div class="flex-wrap">
                <div class="content-well">
                    <span class="content-title">"Analyzer"</span>
                    <br/>
                    <ul>
                        <li>"Find items to resale on your own world"</li>
                        <li>"Quickly filter by roi, profit, and estimated sale date"</li>
                    </ul>
                </div>
                <div class="content-well">
                    <span class="content-title">"Retainers"</span>
                    <br/>
                    <ul>
                        <li>"Quickly check which of your retainer's listings have been undercut"</li>
                        <li>"Get alerted on Discord or on this site"</li>
                    </ul>
                </div>
                <div class="content-well">
                    <span class="content-title">"Lists"</span>
                    <br/>
                    <ul>
                        <li>"Make shopping lists and find the cheapest prices"</li>
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
        </div>
    </div>}
}
