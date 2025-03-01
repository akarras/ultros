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
                            Either::Left(view! {
                                <A href="/profile">
                                    <img
                                        class="avatar"
                                        src=auth.avatar
                                        alt=auth.username
                                    />
                                </A>
                                <a rel="external" class="btn" href="/logout">
                                    "Logout"
                                </a>
                            })
                        }
                        _ => {
                            Either::Right(view! {
                                <a rel="external" class="px-4 py-2 rounded-lg bg-violet-600/20 hover:bg-violet-600/30
                                                        border border-violet-400/10 hover:border-violet-400/20
                                                        transition-all duration-300 text-gray-200 hover:text-amber-200 flex flex-row" href="/login">
                                    <Icon height="2rem" width="2em" icon=i::BsDiscord/>
                                    "Login"
                                </a>
                                <A href="/settings">
                                    <Icon height="2em" width="2em" icon=i::IoSettingsSharp/>
                                    <span class="sr-only">Settings</span>
                                </A>
                            })
                        }
                    })
            }}

        </Suspense>
    }.into_any()
}
