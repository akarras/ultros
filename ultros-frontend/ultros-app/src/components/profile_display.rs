use crate::{api::get_login, components::loading::Loading};
use icondata as i;
use leptos::{either::Either, prelude::*};
use leptos_icons::*;
use leptos_router::components::*;

#[component]
pub fn ProfileDisplay() -> impl IntoView {
    let user = Resource::new(move || {}, move |_| async move { get_login().await.ok() });
    view! {
        <Suspense fallback=Loading>
            {move || {
                user.get()
                    .map(|user| match user {
                        Some(auth) => {
                            Either::Left(
                                view! {
                                    <A href="/profile">
                                        <img class="avatar" src=auth.avatar alt=auth.username />
                                    </A>
                                    <a rel="external" class="nav-link" href="/logout">
                                        "Logout"
                                    </a>
                                },
                            )
                        }
                        _ => {
                            Either::Right(
                                view! {
                                    <a
                                        rel="external"
                                        class="nav-link"
                                        href="/login"
                                    >
                                        <Icon height="1.2em" width="1.2em" icon=i::BsDiscord />
                                        <span>"Login"</span>
                                    </a>
                                    <A href="/settings" attr:class="nav-link">
                                        <Icon height="2em" width="2em" icon=i::IoSettingsSharp />
                                        <span class="sr-only">Settings</span>
                                    </A>
                                },
                            )
                        }
                    })
            }}

        </Suspense>
    }
    .into_any()
}
