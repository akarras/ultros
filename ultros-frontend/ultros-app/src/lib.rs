pub(crate) mod api;
pub(crate) mod components;
pub(crate) mod error;
pub(crate) mod global_state;
pub(crate) mod routes;

pub use global_state::user::User;

use std::rc::Rc;

use crate::api::get_worlds;
use crate::global_state::cheapest_prices::CheapestPrices;
use crate::global_state::LocalWorldData;
use crate::{
    components::{profile_display::*, search_box::*, tooltip::*},
    routes::{
        analyzer::*, edit_lists::*, edit_retainers::*, item_explorer::*, item_view::*,
        list_view::*, lists::*, profile::*, retainers::*,
    },
};
use leptos::*;
use leptos_meta::*;
use leptos_router::*;
use ultros_api_types::cheapest_listings::CheapestListings;
use ultros_api_types::world_helper::WorldHelper;

#[cfg(feature = "ssr")]
pub fn register_server_functions() -> Result<(), Box<dyn std::error::Error>> {
    // EditRetainerOrder::register()?;
    Ok(())
}

#[component]
pub fn App(cx: Scope) -> impl IntoView {
    provide_meta_context(cx);
    let worlds = create_resource(
        cx,
        move || "worlds",
        move |_| async move {
            let world_data = get_worlds(cx).await;
            world_data.map(|data| Rc::new(WorldHelper::new(data)))
        },
    );
    provide_context(cx, LocalWorldData(worlds));
    let (read_cheapest, write_cheapest) = create_signal(cx, CheapestListings::default());
    provide_context(cx, CheapestPrices::new(cx, read_cheapest, write_cheapest));
    view! {
        cx,
        <Stylesheet id="app" href="/target/site/pkg/ultros.css"/>
        <Stylesheet id="font-awesome" href="/static/fa/css/all.min.css"/>
        <Stylesheet id="xiv-icons" href="/static/classjob-icons/src/xivicon.css"/>
        <Title text="Ultros" />
        <div class="gradient-outer">
            <div class="gradient"></div>
        </div>
        <Router>
            <nav class="header">
                // <i><b>"ULTROS IS STILL UNDER ACTIVE DEVELOPMENT"</b></i>
                <A href="/alerts">
                    <i class="fa-solid fa-bell"></i>
                    "Alerts"
                </A>
                <A href="/analyzer">
                    <i class="fa-solid fa-money-bill-trend-up"></i>
                    "Analyzer"
                </A>
                <A href="/list">
                    <i class="fa-solid fa-list"></i>
                    "Lists"
                </A>
                <A href="/retainers/listings">
                    <i class="fa-solid fa-user-group"></i>
                    "Retainers"
                </A>
                <div>
                    <SearchBox/>
                </div>
                <A href="/items">
                    <Tooltip tooltip_text="All Items".to_string()>
                        <i class="fa-solid fa-screwdriver-wrench"></i>
                    </Tooltip>
                </A>
                <a rel="external" class="btn nav-item" href="/invitebot">
                    "Invite Bot"
                </a>
                <ProfileDisplay/>
            </nav>
            <Routes>
                <Route path="" view=move |cx| view! {cx, <div class="container"><div class="hero-title">"Dominate the marketboard"</div></div>}/>
                <Route path="retainers/edit" view=move |cx| view! { cx, <EditRetainers />}/>
                <Route path="retainers/undercuts" view=move |cx| view! { cx, <RetainerUndercuts />}/>
                <Route path="retainers/listings" view=move |cx| view! { cx, <Retainers/>} />
                    // <Route path="listings" view=move |cx| view! {cx, <h1>"Retainer Listings"</h1>}/>
                // </Route>
                <Route path="list/edit" view=move |cx| view! {cx, <EditLists/>}/>
                <Route path="list/:id" view=move |cx| view!{ cx, <ListView/>}/>
                <Route path="list" view=move |cx| view!{cx, <Lists/>}/>
                <Route path="items/jobset/:jobset" view=move |cx| view! { cx, <ItemExplorer/>}/>
                <Route path="items/category/:category" view=move |cx| view! { cx, <ItemExplorer/>}/>
                <Route path="items" view=move |cx| view! { cx, <ItemExplorer/>}/>
                <Route path="item/:world/:id" view=move |cx| view! { cx, <ItemView />} />
                // <Route path="*listings" view=move |cx| view! { cx, <h1>"Listings"</h1>}/>

                <Route path="analyzer" view=move |cx| view! { cx, <Analyzer/>}/>
                <Route path="analyzer/:world" view=move |cx| view! { cx, <AnalyzerWorldView/>}/>
                <Route path="profile" view=move |cx| view! { cx, <Profile/>}/>
            </Routes>
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
