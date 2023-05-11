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
use crate::global_state::user::LoggedInUser;
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
pub fn App(cx: Scope, worlds: AppResult<Arc<WorldHelper>>) -> impl IntoView {
    provide_meta_context(cx);
    provide_context(cx, Cookies::new(cx));
    provide_context(cx, LocalWorldData(worlds));
    provide_context(cx, CheapestPrices::new(cx));
    let login = create_resource_with_initial_value(
        cx,
        move || {},
        move |_| async move { get_login(cx).await.ok() },
        None,
    );
    provide_context(cx, LoggedInUser(login));
    let (homeworld, _set_homeworld) = get_homeworld(cx);
    view! {
        cx,
        <Stylesheet id="app" href="/target/site/pkg/ultros.css"/>
        <Stylesheet id="font-awesome" href="/static/fa/css/all.min.css"/>
        <Stylesheet id="xiv-icons" href="/static/classjob-icons/src/xivicon.css"/>
        <MetaTitle title="Ultros" />
        <MetaDescription text="Ultros is a FAST FFXIV marketboard analysis tool, keep up to date with all of your retainers and ensure you've got the best prices!" />
        <Meta name="twitter:card" content="summary_large_image"/>
        <Meta name="viewport" content="width=device-width"/>
        <Meta name="theme-color" content="#0f0710"/>
        <Meta property="og:type" content="website"/>
        <Meta property="og:locale" content="en_US" />
        <Meta property="og:site_name" content="Ultros" />

        <div class="gradient-outer">
            <div class="gradient"></div>
        </div>
        <Router>
            <nav class="header">
            <b><i>"ULTRA ALPHA™"</i></b>
                // <Suspense fallback=move || {}>
                // {move || login.read(cx).flatten().map(|_| view!{cx, <A href="/alerts">
                //     <i class="fa-solid fa-bell"></i>
                //     "Alerts"
                // </A>})}
                // </Suspense>
                {move ||
                    {
                        view!{cx,
                        <A href=homeworld().map(|w| format!("/analyzer/{}", w.name)).unwrap_or("/analyzer".to_string())>
                            <i class="fa-solid fa-money-bill-trend-up"></i>
                            "Analyzer"
                        </A>}
                    }
                }
                <Suspense fallback=move || {}>
                {move || login.read(cx).flatten().map(|_| view!{cx, <A href="/list">
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
                <Route path="" view=move |cx| view!{cx, <HomePage/>} />
                <Route path="retainers" view=move |cx| view!{cx, <Retainers/>}>
                    <Route path="edit" view=move |cx| view!{cx, <EditRetainers />}/>
                    <Route path="undercuts" view=move |cx| view!{cx, <RetainerUndercuts />}/>
                    <Route path="listings" view=move |cx| view!{cx, <RetainerListings />}/>
                </Route>
                <Route path="list" view=move |cx| view!{cx, <Lists/>}>
                    <Route path=":id" view=move |cx| view!{ cx, <ListView/>}/>
                    <Route path="" view=move |cx| view! {cx, <EditLists/>}/>
                </Route>
                <Route path="items" view=move |cx| view! { cx, <ItemExplorer/>}>
                    <Route path="jobset/:jobset" view=move |cx| view!{cx, <JobItems />}/>
                    <Route path="category/:category" view=move |cx| view!{cx, <CategoryItems />}/>
                    <Route path="" view=move |_cx| view!{cx, "Choose a category to search!"}/>
                </Route>
                <Route path="item/:world/:id" view=move |cx| view! { cx, <ItemView />} />
                // <Route path="*listings" view=move |cx| view! { cx, <h1>"Listings"</h1>}/>

                <Route path="analyzer" view=move |cx| view! { cx, <Analyzer/>}/>
                <Route path="analyzer/:world" view=move |cx| view! { cx, <AnalyzerWorldView/>}/>
                <Route path="profile" view=move |cx| view! { cx, <Profile/>}/>
            </AnimatedRoutes>
        </Router>
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
