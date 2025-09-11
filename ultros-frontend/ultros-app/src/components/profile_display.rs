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
                                    <a rel="external" class="btn" href="/logout">
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
                                        class="px-4 py-2 rounded-lg bg-brand-600/20 hover:bg-brand-600/30
                                        border border-brand-400/10 hover:border-brand-400/20
                                        transition-all duration-300 text-gray-200 hover:text-brand-300 gap-2
                                        flex flex-row"
                                        href="/login"
                                    >
                                        <div>
                                            <Icon height="1.2rem" width="1.2em" icon=i::BsDiscord />
                                        </div>
                                        <span>"Login"</span>
                                    </a>
                                    <A href="/settings">
                                        <Icon height="2em" width="2em" icon=i::IoSettingsSharp />
                                        <span class="sr-only">Settings</span>
                                    </A>
                                },
                            )
                        }
                    })
            }}

        </Suspense>
    }.into_any()
}
