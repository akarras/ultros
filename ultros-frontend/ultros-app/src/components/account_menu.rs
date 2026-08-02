//! Account row + drop-up for the sidebar footer.
//!
//! Replaces `UserMenu`. Two deliberate changes from that component:
//!
//! 1. Opens on **click**, not hover. The old hover trigger
//!    (`use_element_hover`) has no touch equivalent, and this sidebar is
//!    the mobile drawer below 1024px.
//! 2. The signed-out panel includes Language. Previously locale lived in a
//!    separate top-bar control, so the signed-out menu omitted it —
//!    carrying that omission over would leave non-English visitors with no
//!    switcher at all.

use crate::api::get_login;
use crate::components::character_switcher::CharacterSwitcher;
use crate::components::dismissable::use_dismissable;
use crate::components::icon::Icon;
use crate::components::language_picker::LanguageAccordion;
use crate::components::theme_picker::QuickThemeToggle;
use crate::i18n::{t, t_string, use_i18n};
use icondata as i;
use leptos::html;
use leptos::prelude::*;
use leptos_router::components::A;

#[component]
pub fn AccountMenu() -> impl IntoView {
    let i18n = use_i18n();
    let (open, set_open) = signal(false);
    let user = Resource::new(move || {}, move |_| async move { get_login().await.ok() });
    let root_ref = NodeRef::<html::Div>::new();

    // Route change, click outside, Escape — the shared idiom.
    use_dismissable(root_ref, move || set_open(false));

    view! {
        <div class="side-nav-account" node_ref=root_ref>
            <button
                class="side-nav-account-trigger"
                aria-haspopup="true"
                aria-expanded=move || if open.get() { "true" } else { "false" }
                aria-label=t_string!(i18n, account).to_string()
                on:click=move |_| set_open.update(|v| *v = !*v)
            >
                <Suspense fallback=move || {
                    view! { <Icon icon=i::BsPersonCircle width="1.25em" height="1.25em" /> }
                }>
                    {move || {
                        match user.get().flatten() {
                            Some(auth) => {
                                view! {
                                    <img class="avatar" src=auth.avatar alt=auth.username.clone() />
                                    <span class="side-nav-label ml-2">{auth.username}</span>
                                }
                                    .into_any()
                            }
                            None => {
                                view! {
                                    <Icon icon=i::BsPersonCircle width="1.25em" height="1.25em" />
                                    <span class="side-nav-label ml-2">{t!(i18n, sign_in)}</span>
                                }
                                    .into_any()
                            }
                        }
                    }}
                </Suspense>
                <Icon
                    icon=i::BiChevronUpSolid
                    width="1em"
                    height="1em"
                    attr:class="side-nav-account-caret"
                />
            </button>

            <Show when=move || open.get()>
                <div class="side-nav-account-panel" tabindex="-1">
                    <Suspense fallback=move || {
                        view! { <div class="menu-item muted">{t!(i18n, loading)}</div> }
                    }>
                        {move || {
                            let signed_in = user.get().flatten().is_some();
                            if signed_in {
                                view! {
                                    // Switching home world is the most frequent
                                    // thing a signed-in player does here, so it
                                    // sits above the navigation links.
                                    <CharacterSwitcher />
                                    <A href="/profile" attr:class="menu-item">
                                        <Icon icon=i::BsPersonCircle width="1.1em" height="1.1em" />
                                        <span class="ml-2">{t!(i18n, profile)}</span>
                                    </A>
                                    <A href="/settings" attr:class="menu-item">
                                        <Icon icon=i::IoSettingsSharp width="1.1em" height="1.1em" />
                                        <span class="ml-2">{t!(i18n, settings)}</span>
                                    </A>
                                    <div class="menu-divider"></div>
                                    <LanguageAccordion />
                                    <QuickThemeToggle menu_item=true />
                                    <div class="menu-divider"></div>
                                    // No icon — matches the existing logout
                                    // link, which is also icon-less. Don't
                                    // invent an icondata identifier here
                                    // without checking it resolves.
                                    <a rel="external" href="/logout" class="menu-item">
                                        <span class="ml-2">{t!(i18n, logout)}</span>
                                    </a>
                                }
                                    .into_any()
                            } else {
                                view! {
                                    <a rel="external" href="/login" class="menu-item">
                                        <Icon icon=i::BsDiscord width="1.1em" height="1.1em" />
                                        <span class="ml-2">{t!(i18n, login_with_discord)}</span>
                                    </a>
                                    <A href="/settings" attr:class="menu-item">
                                        <Icon icon=i::IoSettingsSharp width="1.1em" height="1.1em" />
                                        <span class="ml-2">{t!(i18n, settings)}</span>
                                    </A>
                                    <div class="menu-divider"></div>
                                    <LanguageAccordion />
                                    <QuickThemeToggle menu_item=true />
                                }
                                    .into_any()
                            }
                        }}
                    </Suspense>
                </div>
            </Show>
        </div>
    }
    .into_any()
}
