pub(crate) mod api;
pub(crate) mod components;
pub(crate) mod error;
pub(crate) mod global_state;
pub(crate) mod routes;
pub(crate) mod ws;

use std::sync::Arc;

use crate::api::get_login;
use crate::error::AppResult;
use crate::global_state::cheapest_prices::CheapestPrices;
use crate::global_state::cookies::Cookies;
use crate::global_state::home_world::get_homeworld;
use crate::global_state::LocalWorldData;
use crate::{
    components::{meta::*, profile_display::*, search_box::*, tooltip::*},
    routes::{
        analyzer::*, edit_retainers::*, home_page::*, item_explorer::*, item_view::*, list_view::*,
        lists::*, profile::*, retainers::*,
    },
};
use leptos::*;
use leptos_meta::*;
use leptos_router::*;
use ultros_api_types::world_helper::WorldHelper;

#[cfg(feature = "ssr")]
pub fn register_server_functions() -> Result<(), Box<dyn std::error::Error>> {
    // EditRetainerOrder::register()?;
    Ok(())
}

#[component]
pub fn App(worlds: AppResult<Arc<WorldHelper>>) -> impl IntoView {
    provide_meta_context();
    provide_context(Cookies::new());
    provide_context(LocalWorldData(worlds));
    provide_context(CheapestPrices::new());
    let login = create_resource(move || {}, move |_| async move { get_login().await.ok() });
    // provide_context(LoggedInUser(login));
    let (homeworld, _set_homeworld) = get_homeworld();
    view! {

        <Stylesheet id="leptos" href="/target/site/pkg/ultros.css"/>
        <Stylesheet id="font-awesome" href="/static/fa/css/all.min.css"/>
        <Stylesheet id="xiv-icons" href="/static/classjob-icons/src/xivicon.css"/>
        <MetaTitle title="Ultros" />
        <MetaDescription text="Ultros is a FAST FFXIV marketboard analysis tool, keep up to date with all of your retainers and ensure you've got the best prices!" />
        <Meta name="twitter:card" content="summary_large_image"/>
        <Meta name="viewport" content="initial-scale=1.0,width=device-width"/>
        <Meta name="theme-color" content="#0f0710"/>
        <Meta property="og:type" content="website"/>
        <Meta property="og:locale" content="en_US" />
        <Meta property="og:site_name" content="Ultros" />

        <div class="gradient-outer">
            <div class="gradient"></div>
        </div>
        <div class="">
            <Router>
                <nav class="header">
                    <A href="/" exact=true>
                    // <Icon icon=Icon::from(BiIcon::BiHomeSolid) height="1.7em" width="1.7em"/>
                        "Home"
                    </A>
                    // <Suspense fallback=move || {}>
                    // {move || login.read().flatten().map(|_| view!{<A href="/alerts">
                    //     <i class="fa-solid fa-bell"></i>
                    //     "Alerts"
                    // </A>})}
                    // </Suspense>
                    {move ||
                        {
                            view!{
                            <A href=homeworld().map(|w| format!("/analyzer/{}", w.name)).unwrap_or("/analyzer".to_string())>
                                <i class="fa-solid fa-money-bill-trend-up"></i>
                                "Analyzer"
                            </A>}
                        }
                    }
                    <Suspense fallback=move || {}>
                    {move || login.get().flatten().map(|_| view!{<A href="/list">
                        <i class="fa-solid fa-list"></i>
                        "Lists"
                    </A>
                    <A href="/retainers/listings">
                        <i class="fa-solid fa-user-group"></i>
                        "Retainers"
                    </A>})}
                    </Suspense>
                    <div>
                        <SearchBox/>
                    </div>
                    <A href="/items">
                        <Tooltip tooltip_text="All Items".to_string()>
                            <i class="fa-solid fa-screwdriver-wrench"></i>
                        </Tooltip>
                    </A>
                    <div class="flex-row">
                        <a rel="external" class="btn nav-item" href="/invitebot">
                            "Invite Bot"
                        </a>
                        <ProfileDisplay />
                    </div>
                </nav>
                <AnimatedRoutes outro="route-out" intro="route-in" outro_back="route-out-back" intro_back="route-in-back">
                    <Route path="" view=HomePage/>
                    <Route path="retainers" view=Retainers>
                        <Route path="edit" view=EditRetainers/>
                        <Route path="undercuts" view=RetainerUndercuts/>
                        <Route path="listings" view=RetainerListings/>
                    </Route>
                    <Route path="list" view=Lists>
                        <Route path=":id" view=ListView/>
                        <Route path="" view=EditLists/>
                    </Route>
                    <Route path="items" view=ItemExplorer>
                        <Route path="jobset/:jobset" view=JobItems />
                        <Route path="category/:category" view=CategoryItems />
                        <Route path="" view=move || view!{"Choose a category to search!"}/>
                    </Route>
                    <Route path="item/:world/:id" view=ItemView/>
                    // <Route path="*listings" view=move || view! { <h1>"Listings"</h1>}/>

                    <Route path="analyzer" view=Analyzer/>
                    <Route path="analyzer/:world" view=AnalyzerWorldView />
                    <Route path="profile" view=Profile/>
                </AnimatedRoutes>
            </Router>
        </div>
        <footer class="flex-column flex-space flex-center">
            <div class="flex-row column-pad flex-center">
                <a href="https://discord.gg/pgdq9nGUP2">"Discord"</a>"|"
                <a href="https://github.com/akarras/ultros">"GitHub"</a>"|"
                <a href="https://leekspin.com">"Patreon"</a>
            </div>
            <span>"Made using "<a href="https://universalis.app/">"universalis"</a>"' API.Please contribute to Universalis to help this site stay up to date."</span>
            <span></span>
            <span>"FINAL FANTASY XIV © 2010 - 2020 SQUARE ENIX CO., LTD. All Rights Reserved."</span>
        </footer>
    }
}
