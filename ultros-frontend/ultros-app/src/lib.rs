pub(crate) mod api;
pub(crate) mod components;
pub(crate) mod global_state;
pub(crate) mod item_icon;
pub(crate) mod routes;
pub(crate) mod search_box;

use crate::global_state::AuthenticationState;
use crate::global_state::LocalWorldData;
use crate::routes::analyzer::*;
use crate::routes::listings::*;
use crate::search_box::*;
use leptos::*;
use leptos_meta::*;
use leptos_router::*;

#[component]
pub fn App(cx: Scope) -> impl IntoView {
    let world_data = LocalWorldData::new(cx);
    let auth_state = AuthenticationState::new(cx);
    provide_context(cx, world_data);
    provide_context(cx, auth_state);
    let auth_state = use_context::<AuthenticationState>(cx).expect("Auth state should always be present");
    view! {
        cx,
        <Stylesheet id="leptos" href="/target/site/pkg/ultros.css"/>
        <Stylesheet id="font-awesome" href="/static/fa/css/all.min.css"/>
        <Title text="Ultros" />
        <div class="gradient-outer">
            <div class="gradient"></div>
        </div>
        <Router>
            <nav class="header">
                <i><b>"ULTROS IS STILL UNDER ACTIVE DEVELOPMENT"</b></i>
                <A href="alerts">
                    <i class="fa-solid fa-bell"></i>
                    "Alerts"
                </A>
                <A href="analyzer">
                    <i class="fa-solid fa-money-bill-trend-up"></i>
                    "Analyzer"
                </A>
                <A href="list">
                    <i class="fa-solid fa-list"></i>
                    "Lists"
                </A>
                <A href="retainers">
                    <i class="fa-solid fa-user-group"></i>
                    "Retainers"
                </A>
                <SearchBox/>
                <a class="btn nav-item" href="invitebot">
                    "Invite Bot"
                </a>
                // {move || {
                //     match auth_state.0.read() {
                //         Some(Some(auth)) => {
                //             view!{cx,
                //                 <a class="btn nav-item" href="profile">
                //                     <img class="avatar" src=&auth.avatar alt=&auth.username/>
                //                 </a>
                //                 <a class="btn nav-item" href="/logout">
                //                     "Logout"
                //                 </a>}
                //         }
                //         _ => {
                //             view!{cx, <a class="btn nav-item" href="/login">
                //                 "Login"
                //             </a>
                //             }
                //         }
                //     }
                // }}
            </nav>
            <Routes>
                <Route path="retainers/undercuts" view=move |cx| view! { cx, <h1>"Undercuts"</h1>}/>
                <Route path="*retainers" view=move |cx| view! { cx, <h1>"Retainers root"</h1>}>
                    <Route path="listings" view=move |cx| view! {cx, <h1>"Retainer Listings"</h1>}/>
                </Route>
                <Route path="list" view=move |cx| view!{cx, <h1>"List"</h1>}/>
                <Route path="listings/:world/:id" view=move |cx| view! { cx, <Listings />}/>
                // <Route path="*listings" view=move |cx| view! { cx, <h1>"Listings"</h1>}/>
                <Route path="analyzer" view=move |cx| view! { cx, <Analyzer/>}/>
                <Route path="" view=move |cx| view! {cx, <div class="container"><div class="hero-title">"Dominate the marketboard"</div></div>}/>
            </Routes>
        </Router>
        <footer class="flex-column flex-space flex-center">
            <div class="flex-row column-pad">
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
