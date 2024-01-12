pub(crate) mod api;
pub(crate) mod components;
pub(crate) mod error;
pub(crate) mod global_state;
pub(crate) mod routes;
pub(crate) mod ws;

use std::sync::Arc;

use crate::api::get_login;
use crate::components::recently_viewed::RecentItems;
use crate::error::AppResult;
use crate::global_state::cheapest_prices::CheapestPrices;
use crate::global_state::clipboard_text::GlobalLastCopiedText;
use crate::global_state::cookies::Cookies;
use crate::global_state::home_world::{get_homeworld, GuessedRegion};
use crate::global_state::LocalWorldData;
use crate::{
    components::{ad::Ad, profile_display::*, search_box::*, tooltip::*},
    routes::{
        analyzer::*, edit_retainers::*, home_page::*, item_explorer::*, item_view::*,
        legal::cookie_policy::CookiePolicy, legal::privacy_policy::PrivacyPolicy, list_view::*,
        lists::*, retainers::*, settings::*,
    },
};
use git_const::git_short_hash;
use leptos::*;
use leptos_icons::*;
use leptos_meta::*;
use leptos_router::*;
use ultros_api_types::world_helper::WorldHelper;

#[component]
pub fn App(worlds: AppResult<Arc<WorldHelper>>, region: String) -> impl IntoView {
    provide_meta_context();
    let cookies = Cookies::new();
    provide_context(GuessedRegion(region));
    provide_context(cookies);
    provide_context(LocalWorldData(worlds));
    provide_context(CheapestPrices::new());
    provide_context(GlobalLastCopiedText(create_rw_signal(None)));
    provide_context(RecentItems::new());
    let login = create_resource(move || {}, move |_| async move { get_login().await.ok() });
    let (homeworld, _set_homeworld) = get_homeworld();
    let git_hash = git_short_hash!();
    let sheet_url = ["/pkg/", git_hash, "/ultros.css"].concat();
    view! {
        <Stylesheet id="leptos" href=sheet_url/>
        <Stylesheet id="xiv-icons" href="/static/classjob-icons/src/xivicon.css"/>
        <Title text="Ultros" />
        // <Meta name="twitter:card" content="summary_large_image"/>
        <Meta name="viewport" content="initial-scale=1.0,width=device-width"/>
        <Meta name="theme-color" content="#0f0710"/>
        <Meta property="og:type" content="website"/>
        <Meta property="og:locale" content="en_US" />
        <Meta property="og:site_name" content="Ultros" />

        <div class="gradient-outer">
            <div class="gradient"></div>
        </div>
        <div>
            <Router>
                <nav class="header">
                    <A href="/" exact=true>
                    <Icon icon=Icon::from(BiIcon::BiHomeSolid) height="1.75em" width="1.75em"/>
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
                                <Icon width="1.75em" height="1.75em" icon=Icon::from(FaIcon::FaMoneyBillTrendUpSolid)/>
                                "Analyzer"
                            </A>}
                        }
                    }
                    <Suspense fallback=move || {}>
                    {move || login.get().flatten().map(|_| view!{<A href="/list">
                        <Icon width="1.75em" height="1.75em" icon=Icon::from(AiIcon::AiOrderedListOutlined) />
                        "Lists"
                    </A>
                    <A href="/retainers/listings">
                        <Icon width="1.75em" height="1.75em" icon=Icon::from(BiIcon::BiGroupSolid) />
                        "Retainers"
                    </A>})}
                    </Suspense>
                    <div>
                        <SearchBox/>
                    </div>
                    <A href="/items">
                        <Tooltip tooltip_text=Oco::from("All Items")>
                            <Icon width="1.75em" height="1.75em" icon=Icon::from(FaIcon::FaScrewdriverWrenchSolid) />
                        </Tooltip>
                    </A>
                    <div class="flex-row">
                        <a rel="external" class="btn" href="/invitebot">
                            "Invite Bot"
                        </a>
                        <ProfileDisplay />
                    </div>
                </nav>
                // <AnimatedRoutes outro="route-out" intro="route-in" outro_back="route-out-back" intro_back="route-in-back">
                // https://github.com/leptos-rs/leptos/issues/1754
                <Routes>
                    <Route path="" view=HomePage/>
                    <Route path="retainers" view=Retainers>
                        <Route path="edit" view=EditRetainers/>
                        <Route path="undercuts" view=RetainerUndercuts/>
                        <Route path="listings" view=RetainerListings/>
                        <Route path="listings/:id" view=SingleRetainerListings />
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
                    <Route path="item/:id" view=ItemView/>
                    // <Route path="*listings" view=move || view! { <h1>"Listings"</h1>}/>

                    <Route path="analyzer" view=Analyzer/>
                    <Route path="analyzer/:world" view=AnalyzerWorldView />
                    <Route path="settings" view=Settings/>
                    <Route path="profile" view=Profile/>
                    <Route path="privacy" view=PrivacyPolicy/>
                    <Route path="cookie-policy" view=CookiePolicy/>
                </Routes>
            </Router>
        </div>
        <footer class="flex-column flex-space flex-center">
            <Ad />
            <div class="flex-row column-pad flex-center">
                <a href="https://discord.gg/pgdq9nGUP2">"Discord"</a>"|"
                <a href="https://github.com/akarras/ultros">"GitHub"</a>"|"
                <a href="https://leekspin.com">"Patreon"</a>
            </div>
            <span>"Made using "<a href="https://universalis.app/">"universalis"</a>"' API.Please contribute to Universalis to help this site stay up to date."</span>
            <span>"Version: "{git_hash}</span>
            <span>"FINAL FANTASY XIV © 2010 - 2020 SQUARE ENIX CO., LTD. All Rights Reserved."</span>
        </footer>
    }
}
