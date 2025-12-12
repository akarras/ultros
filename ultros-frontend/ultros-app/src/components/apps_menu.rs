use crate::api::get_login;
use crate::components::theme_picker::QuickThemeToggle;
use crate::global_state::home_world::use_home_world;
use cfg_if::cfg_if;
use icondata as i;
use leptos::html;
use leptos::prelude::*;
use leptos_icons::*;
use leptos_router::components::A;
<<<<<<< HEAD
=======
#[cfg(feature = "hydrate")]
use leptos_use::use_element_hover;
>>>>>>> main

/// An overflow menu for primary app destinations (Flip Finder, Explorer, Exchange).
#[component]
pub fn AppsMenu() -> impl IntoView {
    // Focus/hover-driven open state (mirrors Select component behavior)
    let (has_focus, set_has_focus) = signal(false);
    let (homeworld, _set_homeworld) = use_home_world();
    let panel_ref = NodeRef::<html::Div>::new();
    cfg_if! {
        if #[cfg(feature = "hydrate")] {
            let hovered = use_element_hover(panel_ref);
        } else {
            let (hovered, _set_hovered) = signal(false);
        }
    }
    let is_open = Signal::derive(move || has_focus() || hovered());

    view! {
        <div class="relative" on:focusin=move |_| set_has_focus(true) on:focusout=move |_| set_has_focus(false)>
            <button
                class="nav-link"
                aria-haspopup="menu"
                aria-expanded=move || if is_open() { "true" } else { "false" }
                // click naturally focuses the button; no explicit toggle required
            >
                <Icon height="1.4em" width="1.4em" icon=i::BsGrid />
                <span class="hidden lg:inline ml-2">"Apps"</span>
            </button>

            <Show when=move || is_open()>
                <div
                    node_ref=panel_ref
                    class="absolute left-0 mt-2 min-w-[14rem]
                           panel rounded-xl shadow-xl border border-[color:var(--color-outline)]
                           bg-[color:var(--color-background-elevated)]
                           content-visible contain-content z-50"
                    role="menu"
                    tabindex="-1"
                >
                    <div class="p-2 flex flex-col gap-1">
                        <A
                            href=homeworld()
                                .map(|w| format!("/flip-finder/{}", w.name))
                                .unwrap_or("/flip-finder".to_string())
                            attr:class="nav-link w-full justify-start"
                            on:click=move |_| set_has_focus(false)
                        >
                            <Icon height="1.1em" width="1.1em" icon=i::FaMoneyBillTrendUpSolid />
                            <span class="ml-2">"Flip Finder"</span>
                        </A>

                        <A
                            href=homeworld()
                                .map(|w| format!("/recipe-analyzer?world={}", w.name))
                                .unwrap_or("/recipe-analyzer".to_string())
                            attr:class="nav-link w-full justify-start"
                            on:click=move |_| set_has_focus(false)
                        >
                            <Icon height="1.1em" width="1.1em" icon=i::FaHammerSolid />
                            <span class="ml-2">"Recipe Analyzer"</span>
                        </A>

                        <A
                            href="/items?menu-open=true"
                            attr:class="nav-link w-full justify-start"
                            on:click=move |_| set_has_focus(false)
                        >
                            <Icon height="1.1em" width="1.1em" icon=i::FaScrewdriverWrenchSolid />
                            <span class="ml-2">"Explorer"</span>
                        </A>

                        <A
                            href="/currency-exchange"
                            attr:class="nav-link w-full justify-start"
                            on:click=move |_| set_has_focus(false)
                        >
                            <Icon height="1.1em" width="1.1em" icon=i::BsArrowLeftRight />
                            <span class="ml-2">"Exchange"</span>
                        </A>
                    </div>
                </div>
            </Show>
        </div>
    }
    .into_any()
}

/// A user/utility menu:
/// - When logged in: Profile, Settings, Lists, Retainers, Invite Bot, Theme (mobile), Logout
/// - When logged out: Login (Discord), Settings
#[component]
pub fn UserMenu() -> impl IntoView {
    // Focus/hover-driven open state (mirrors Select component behavior)
    let (has_focus, set_has_focus) = signal(false);
    let user = Resource::new(move || {}, move |_| async move { get_login().await.ok() });
    let panel_ref = NodeRef::<html::Div>::new();
    cfg_if! {
        if #[cfg(feature = "hydrate")] {
            let hovered = use_element_hover(panel_ref);
        } else {
            let (hovered, _set_hovered) = signal(false);
        }
    }
    let is_open = Signal::derive(move || has_focus() || hovered());

    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Escape" {
            set_has_focus(false);
        }
    };

    view! {
        <div class="relative" on:keydown=on_keydown on:focusin=move |_| set_has_focus(true) on:focusout=move |_| set_has_focus(false)>
            <Suspense fallback=move || view! { <button class="nav-link opacity-70 cursor-wait"><Icon icon=i::BsPersonCircle /><span class="hidden lg:inline ml-2">"Account"</span></button> }>
                {move || {
                    let u = user.get().flatten();
                    match u {
                        Some(auth) => {
                            // logged in trigger: avatar circle + caret
                            view! {
                                <button class="nav-link flex items-center gap-2" aria-haspopup="menu" aria-expanded=move || if is_open() { "true" } else { "false" }>
                                    <img class="avatar" src=auth.avatar alt=auth.username />
                                    <Icon height="1em" width="1em" icon=i::BiChevronDownSolid />
                                </button>
                            }.into_any()
                        }
                        None => {
                            // logged out trigger: user icon + caret
                            view! {
                                <button class="nav-link flex items-center gap-2" aria-haspopup="menu" aria-expanded=move || if is_open() { "true" } else { "false" }>
                                    <Icon height="1.5em" width="1.5em" icon=i::BsPersonCircle />
                                    <Icon height="1em" width="1em" icon=i::BiChevronDownSolid />
                                </button>
                            }.into_any()
                        }
                    }
                }}
            </Suspense>

            <Show when=move || is_open()>
                <div
                    node_ref=panel_ref
                    class="absolute right-0 mt-2 min-w-[16rem]
                           panel rounded-xl shadow-xl border border-[color:var(--color-outline)]
                           bg-[color:var(--color-background-elevated)]
                           content-visible contain-content z-50"
                    role="menu"
                    tabindex="-1"
                >
                    <div class="p-2 flex flex-col gap-1">
                        <Suspense fallback=move || view! { <div class="px-3 py-2 text-sm muted">"Loading…"</div> }>
                            {move || {
                                let u = user.get().flatten();
                                match u {
                                    Some(_auth) => {
                                        view! {
                                            <A href="/profile" attr:class="nav-link w-full justify-start" on:click=move |_| set_has_focus(false)>
                                                <Icon height="1.1em" width="1.1em" icon=i::BsPersonCircle />
                                                <span class="ml-2">"Profile"</span>
                                            </A>
                                            <A href="/settings" attr:class="nav-link w-full justify-start" on:click=move |_| set_has_focus(false)>
                                                <Icon height="1.1em" width="1.1em" icon=i::IoSettingsSharp />
                                                <span class="ml-2">"Settings"</span>
                                            </A>

                        <div class="divider my-1"></div>

                                            <A href="/list" attr:class="nav-link w-full justify-start" on:click=move |_| set_has_focus(false)>
                                                <Icon height="1.1em" width="1.1em" icon=i::AiOrderedListOutlined />
                                                <span class="ml-2">"Lists"</span>
                                            </A>
                                            <A href="/retainers/listings" attr:class="nav-link w-full justify-start" on:click=move |_| set_has_focus(false)>
                                                <Icon height="1.1em" width="1.1em" icon=i::BiGroupSolid />
                                                <span class="ml-2">"Retainers"</span>
                                            </A>

                                            <div class="divider my-1"></div>

                                            <a rel="external" href="/invitebot" class="nav-link w-full justify-start" on:click=move |_| set_has_focus(false)>
                                                <Icon height="1.1em" width="1.1em" icon=i::BsDiscord />
                                                <span class="ml-2">"Invite Bot"</span>
                                            </a>

                                            <div class="lg:hidden">
                                                <QuickThemeToggle />
                                            </div>

                                            <div class="divider my-1"></div>

                                            <a rel="external" href="/logout" class="nav-link w-full justify-start" on:click=move |_| set_has_focus(false)>
                                                <span class="ml-2">"Logout"</span>
                                            </a>
                                        }.into_any()
                                    }
                                    None => {
                                        view! {
                                            <a rel="external" href="/login" class="nav-link w-full justify-start" on:click=move |_| set_has_focus(false)>
                                                <Icon height="1.1em" width="1.1em" icon=i::BsDiscord />
                                                <span class="ml-2">"Login with Discord"</span>
                                            </a>
                                            <A href="/settings" attr:class="nav-link w-full justify-start" on:click=move |_| set_has_focus(false)>
                                                <Icon height="1.1em" width="1.1em" icon=i::IoSettingsSharp />
                                                <span class="ml-2">"Settings"</span>
                                            </A>
                                            <div class="divider my-1"></div>
                                            <div class="lg:hidden">
                                                <QuickThemeToggle />
                                            </div>
                                        }.into_any()
                                    }
                                }
                            }}
                        </Suspense>
                    </div>
                </div>
            </Show>
        </div>
    }
    .into_any()
}
