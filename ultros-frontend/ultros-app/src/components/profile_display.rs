use crate::{components::loading::Loading, api::get_login};
use leptos::*;

#[component]
pub fn ProfileDisplay() -> impl IntoView {
    
    let user = create_resource(
        move || {},
        move |_| async move { get_login().await.ok() },
    );
    view! {
        <Suspense fallback=Loading>
        {move || user.read().map(|user| match user {
            Some(auth) => view! {
            <a href="/profile">
                <img class="avatar" src=&auth.avatar alt=&auth.username/>
            </a>
            <a rel="external" class="btn" href="/logout">
                "Logout"
            </a>}.into_view(),
            _ => view! {<a rel="external" class="btn" href="/login">
                <i class="fa-brands fa-discord"></i>"Login"
            </a>
            }.into_view(),
        })}
        </Suspense>
    }
}
