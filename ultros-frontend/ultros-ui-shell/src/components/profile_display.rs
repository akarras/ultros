use crate::components::app_link::AppLink;
use crate::components::icon::Icon;
use crate::i18n::{t, use_i18n};
use crate::{api::get_login, components::loading::Loading};
use icondata as i;
use leptos::{either::Either, prelude::*};

#[component]
pub fn ProfileDisplay() -> impl IntoView {
    let i18n = use_i18n();
    let user = Resource::new(move || {}, move |_| async move { get_login().await.ok() });
    view! {
        <Suspense fallback=Loading>
            {move || {
                user.get()
                    .map(|user| match user {
                        Some(auth) => {
                            Either::Left(
                                view! {
                                    <div class="flex items-center gap-2">
                                        <AppLink href="/profile">
                                            <img class="avatar" src=auth.avatar alt=auth.username />
                                        </AppLink>

                                    </div>
                                },
                            )
                        }
                        _ => {
                            Either::Right(
                                view! {
                                    <div class="flex items-center gap-2">
                                        <a
                                            rel="external"
                                            class="nav-link"
                                            href="/login"
                                        >
                                            <Icon height="1.2em" width="1.2em" icon=i::BsDiscord aria_hidden=true />
                                            <span>{t!(i18n, profile_login_button)}</span>
                                        </a>
                                        <AppLink href="/settings" attr:class="nav-link">
                                            <Icon height="2em" width="2em" icon=i::IoSettingsSharp aria_hidden=true />
                                            <span class="sr-only">Settings</span>
                                        </AppLink>
                                    </div>
                                },
                            )
                        }
                    })
            }}

        </Suspense>
    }
    .into_any()
}
