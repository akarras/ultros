pub(crate) mod item;
pub(crate) mod main_nav;
pub mod routes;
pub(crate) mod search_box;

use crate::routes::analyzer::*;
use crate::search_box::*;
use leptos::*;
use leptos_meta::*;
use leptos_router::*;

#[component]
pub fn App(cx: Scope) -> impl IntoView {
    // provide_context(cx, MetaContext::default());
    let head = use_head(cx);

    view! {
        cx,
        <div>
            <Stylesheet id="leptos" href="./static/main.css"/>
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
                <A href="invitebot">
                  "Invite Bot"
                </A>
                </nav>
                <Routes>
                    <Route path="retainers" view=move |cx| view! { cx, <h1>"Retainers root"</h1>}>
                        <Route path="undercuts" view=move |cx| view! { cx, <h1>"Undercuts"</h1>}/>
                        <Route path="" view=move |cx| view! {cx, <h1>"Retainers"</h1>}/>
                    </Route>
                    <Route path="list" view=move |cx| view!{cx, <h1>"List"</h1>}/>
                    <Route path="listings" view=move |cx| view! { cx, <h1>"Listings"</h1>}>
                        <Route path=":world/:id" view=move |cx| view! { cx, <h1>"Listings for world"</h1>}/>
                    </Route>
                    <Route path="analyzer" view=move |cx| view! { cx, <Analyzer/>}/>
                </Routes>
            </Router>
        </div>
    }
}
