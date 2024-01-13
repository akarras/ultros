use crate::{api::get_login, components::loading::Loading};
use leptos::*;
use leptos_icons::*;
use leptos_router::*;

#[component]
pub fn ProfileDisplay() -> impl IntoView {
    let user = create_resource(move || {}, move |_| async move { get_login().await.ok() });
    view! {
        <Suspense fallback=Loading>
        {move || user.get().map(|user| match user {
            Some(auth) => view! {
            <A href="/profile">
                <img alt="User profile picture" class="avatar" src=&auth.avatar alt=&auth.username/>
            </A>
            <a rel="external" class="btn" href="/logout">
                "Logout"
            </a>}.into_view(),
            _ => view! {<a rel="external" class="btn" href="/login">
                <Icon height="2rem" width="2em" icon=Icon::from(BsIcon::BsDiscord) />"Login"
            </a>
            <A href="/settings">
                <Icon height="2em" width="2em" icon=Icon::from(IoIcon::IoSettingsSharp)/>
                <span class="sr-only">Settings</span>
            </A>
            }.into_view(),
        })}
        </Suspense>
    }
}
