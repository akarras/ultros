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
use crate::global_state::{
    cheapest_prices::CheapestPrices,
    clipboard_text::GlobalLastCopiedText,
    cookies::Cookies,
    home_world::{use_home_world, GuessedRegion},
};
use crate::{
    components::{ad::Ad, profile_display::*, search_box::*, tooltip::*},
    global_state::LocalWorldData,
    routes::{
        analyzer::*,
        currency_exchange::{CurrencyExchange, CurrencySelection, ExchangeItem},
        edit_retainers::*,
        history::*,
        home_page::*,
        item_explorer::*,
        item_view::*,
        legal::{cookie_policy::CookiePolicy, privacy_policy::PrivacyPolicy},
        list_view::*,
        lists::*,
        retainers::*,
        settings::*,
    },
};
use git_const::git_short_hash;
use icondata as i;
use leptos::*;
use leptos_animation::AnimationContext;
use leptos_hotkeys::{provide_hotkeys_context, scopes};
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
    AnimationContext::provide();
    let root_node_ref = create_node_ref::<html::Div>();
    provide_hotkeys_context(root_node_ref, false, scopes!());
    let login = create_resource(move || {}, move |_| async move { get_login().await.ok() });
    let (homeworld, _set_homeworld) = use_home_world();
    let git_hash = git_short_hash!();
    let sheet_url = ["/pkg/", git_hash, "/ultros.css"].concat();
    view! {
        <Stylesheet id="xiv-icons" href="/static/classjob-icons/src/xivicon.css"/>
        <Link id="leptos" rel="stylesheet" href=sheet_url/>
        <Title text="Ultros"/>
        // <Meta name="twitter:card" content="summary_large_image"/>
        <Meta name="viewport" content="initial-scale=1.0,width=device-width"/>
        <Meta name="theme-color" content="#0f0710"/>
        <Meta property="og:type" content="website"/>
        <Meta property="og:locale" content="en-US"/>
        <Meta property="og:site_name" content="Ultros"/>
        <Html class="m-0 p-0" lang="en-US"/>

        // Background gradient
        <div class="fixed inset-0 -z-10 bg-black">
            <div class="absolute inset-0 bg-gradient-to-br from-violet-950/90 via-black/95 to-violet-950/90"/>
            <div class="absolute inset-0 bg-[radial-gradient(circle_at_center,rgba(139,92,246,0.05),transparent_50%)]"/>
        </div>

        <div _ref=root_node_ref class="min-h-screen flex flex-col m-0">
            <Router>
                // Navigation
                <nav class="sticky top-0 z-50 backdrop-blur-sm border-b border-white/5 bg-black/40">
                    <div class="mx-auto max-w-7xl px-2 sm:px-4 lg:px-6 py-2 flex flex-col md:flex-row items-center">
                        // Left section
                        <div class="flex items-center space-x-2">
                            <A href="/" exact=true
                                class="flex items-center gap-2 px-3 py-2 rounded-lg
                                        hover:bg-white/5 transition-colors
                                        text-gray-200 hover:text-amber-200">
                                <Icon icon=i::BiHomeSolid height="1.75em" width="1.75em"/>
                                <span class="hidden sm:inline">"Home"</span>
                            </A>

                            {move || {
                                view! {
                                    <A href=homeworld()
                                        .map(|w| format!("/analyzer/{}", w.name))
                                        .unwrap_or("/analyzer".to_string())
                                       class="flex items-center gap-2 px-3 py-2 rounded-lg
                                               hover:bg-white/5 transition-colors
                                               text-gray-200 hover:text-amber-200">
                                        <Icon
                                            width="1.75em"
                                            height="1.75em"
                                            icon=i::FaMoneyBillTrendUpSolid
                                        />
                                        <span class="hidden sm:inline">"Analyzer"</span>
                                    </A>
                                }
                            }}

                            <Suspense fallback=move || {}>
                                {move || {
                                    login
                                        .get()
                                        .flatten()
                                        .map(|_| {
                                            view! {
                                                <A href="/list"
                                                   class="flex items-center gap-2 px-3 py-2 rounded-lg
                                                           hover:bg-white/5 transition-colors
                                                           text-gray-200 hover:text-amber-200">
                                                    <Icon
                                                        width="1.75em"
                                                        height="1.75em"
                                                        icon=i::AiOrderedListOutlined
                                                    />
                                                    <span class="hidden sm:inline">"Lists"</span>
                                                </A>
                                                <A href="/retainers/listings"
                                                   class="flex items-center gap-2 px-3 py-2 rounded-lg
                                                           hover:bg-white/5 transition-colors
                                                           text-gray-200 hover:text-amber-200">
                                                    <Icon width="1.75em" height="1.75em" icon=i::BiGroupSolid/>
                                                    <span class="hidden sm:inline">"Retainers"</span>
                                                </A>
                                            }
                                        })
                                }}
                            </Suspense>
                        </div>

                        // Center section
                        <div class="flex-1 max-w-xl">
                            <SearchBox/>
                        </div>

                        // Right section
                        <div class="flex items-center gap-4">
                            <A href="/items?menu-open=true"
                               class="flex items-center gap-2 px-3 py-2 rounded-lg
                                        hover:bg-white/5 transition-colors
                                        text-gray-200 hover:text-amber-200">
                                <Tooltip tooltip_text=Oco::from("Item Explorer")>
                                    <Icon width="1.75em" height="1.75em" icon=i::FaScrewdriverWrenchSolid/>
                                </Tooltip>
                                <span class="sr-only">"Item Explorer"</span>
                            </A>

                            <a rel="external" href="/invitebot"
                               class="px-4 py-2 rounded-lg bg-violet-600/20 hover:bg-violet-600/30
                                            border border-violet-400/10 hover:border-violet-400/20
                                            transition-all duration-300 text-gray-200 hover:text-amber-200">
                                "Invite Bot"
                            </a>

                            <ProfileDisplay/>
                        </div>
                    </div>
                </nav>
                // <AnimatedRoutes outro="route-out" intro="route-in" outro_back="route-out-back" intro_back="route-in-back">
                // https://github.com/leptos-rs/leptos/issues/1754
                <main class="flex-1">
                    <div class="mx-auto max-w-7xl px-2 sm:px-4 lg:px-6 py-4 sm:py-6">
                        <Routes>
                            <Route path="" view=HomePage/>
                            <Route path="retainers" view=Retainers>
                                <Route path="edit" view=EditRetainers/>
                                <Route path="undercuts" view=RetainerUndercuts/>
                                <Route path="listings" view=RetainerListings/>
                                <Route
                                    path="listings/:id"
                                    view=SingleRetainerListings
                                    ssr=SsrMode::PartiallyBlocked
                                />
                                <Route path="" view=RetainersBasePath/>
                            </Route>
                            <Route path="list" view=Lists>
                                <Route path=":id" view=ListView/>
                                <Route path="" view=EditLists/>
                            </Route>
                            <Route path="items" view=ItemExplorer>
                                <Route path="jobset/:jobset" view=JobItems/>
                                <Route path="category/:category" view=CategoryItems/>
                                <Route path="" view=move || view! { "Choose a category to search!" }/>
                            </Route>
                            <Route path="item/:world/:id" view=ItemView/>
                            <Route path="item/:id" view=ItemView/>
                            <Route path="analyzer" view=Analyzer/>
                            <Route path="analyzer/:world" view=AnalyzerWorldView/>
                            <Route path="settings" view=Settings/>
                            <Route path="profile" view=Profile/>
                            <Route path="privacy" view=PrivacyPolicy/>
                            <Route path="cookie-policy" view=CookiePolicy/>
                            <Route path="history" view=History/>
                            <Route path="currency-exchange" view=CurrencyExchange>
                                <Route path=":id" view=ExchangeItem/>
                                <Route path="" view=CurrencySelection/>
                            </Route>
                        </Routes>
                    </div>
                </main>
            </Router>
        </div>
        <footer class="bg-black/40 backdrop-blur-sm border-t border-white/5">
            <div class="container mx-auto px-6 py-8 space-y-6">
                                <Ad/>

                                <div class="flex flex-wrap justify-center gap-x-6 gap-y-2">
                                    <a href="https://discord.gg/pgdq9nGUP2" class="text-gray-400 hover:text-amber-200 transition-colors">
                                        "Discord"
                                    </a>
                                    <a href="https://github.com/akarras/ultros" class="text-gray-400 hover:text-amber-200 transition-colors">
                                        "GitHub"
                                    </a>
                                    <a href="https://leekspin.com" class="text-gray-400 hover:text-amber-200 transition-colors">
                                        "Patreon"
                                    </a>
                                    <a href="https://book.ultros.app" class="text-gray-400 hover:text-amber-200 transition-colors">
                                        "Book"
                                    </a>
                                </div>

                                <div class="text-center space-y-2 text-gray-500 text-sm">
                                    <p>
                                        "Ultros is still under constant development. If you have suggestions or feedback,
                                         feel free to leave suggestions in the discord."
                                    </p>
                                    <p>
                                        "Made using "
                                        <a href="https://universalis.app/" class="text-amber-200 hover:text-amber-100 transition-colors">
                                            "universalis"
                                        </a>
                                        "' API. Please contribute to Universalis to help this site stay up to date."
                                    </p>
                                    <p>
                                        "Version: "
                                        <a href=format!("https://github.com/akarras/ultros/commit/{git_hash}")
                                           class="text-amber-200 hover:text-amber-100 transition-colors">
                                            {git_hash}
                                        </a>
                                    </p>
                                    <p class="text-xs">
                                        "FINAL FANTASY XIV © 2010 - 2020 SQUARE ENIX CO., LTD. All Rights Reserved."
                                    </p>
                                </div>
                            </div>
                        </footer>
    }
}
