use crate::api::get_login;
use crate::components::icon::Icon;
use crate::global_state::home_world::use_home_world;
use crate::i18n::{t, t_string};
use cfg_if::cfg_if;
use icondata as i;
use leptos::html;
use leptos::prelude::*;
use leptos_router::components::A;
#[cfg(feature = "hydrate")]
use leptos_use::use_element_hover;

/// An overflow menu for primary app destinations (Flip Finder, Explorer, Exchange).
#[component]
pub fn AppsMenu() -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    // Focus/hover-driven open state (mirrors Select component behavior)
    let (has_focus, set_has_focus) = signal(false);
    let (force_close, set_force_close) = signal(false);
    let (homeworld, _set_homeworld) = use_home_world();
    let panel_ref = NodeRef::<html::Div>::new();
    cfg_if! {
        if #[cfg(feature = "hydrate")] {
            let hovered = use_element_hover(panel_ref);
        } else {
            let (hovered, _set_hovered) = signal(false);
        }
    }
    let is_open = Signal::derive(move || (has_focus() || hovered()) && !force_close());

    let close_menu = move |_| {
        set_has_focus(false);
        set_force_close(true);
    };

    view! {
        <div
            class="relative"
            on:focusin=move |_| {
                set_has_focus(true);
                set_force_close(false);
            }
            on:focusout=move |_| set_has_focus(false)
            on:mouseleave=move |_| set_force_close(false)
        >
            <button
                class="nav-link"
                aria-haspopup="menu"
                aria-expanded=move || if is_open() { "true" } else { "false" }
                aria_label=move || t_string!(i18n, apps)
                // click naturally focuses the button; no explicit toggle required
            >
                <Icon height="1.4em" width="1.4em" icon=i::MdiJellyfish />
                <span class="hidden lg:inline ml-2">{t!(i18n, apps)}</span>
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
                            on:click=close_menu
                        >
                            <Icon height="1.1em" width="1.1em" icon=i::FaMoneyBillTrendUpSolid />
                            <span class="ml-2">{t!(i18n, flip_finder)}</span>
                        </A>

                        <A
                            href=homeworld()
                                .map(|w| format!("/vendor-resale/{}", w.name))
                                .unwrap_or("/vendor-resale".to_string())
                            attr:class="nav-link w-full justify-start"
                            on:click=close_menu
                        >
                            <Icon height="1.1em" width="1.1em" icon=i::FaShopSolid />
                            <span class="ml-2">{t!(i18n, vendor_resale)}</span>
                        </A>

                        <A
                            href=homeworld()
                                .map(|w| format!("/recipe-analyzer?world={}", w.name))
                                .unwrap_or("/recipe-analyzer".to_string())
                            attr:class="nav-link w-full justify-start"
                            on:click=close_menu
                        >
                            <Icon height="1.1em" width="1.1em" icon=i::FaHammerSolid />
                            <span class="ml-2">{t!(i18n, recipe_analyzer)}</span>
                        </A>

                        <A
                            href=homeworld()
                                .map(|w| format!("/fc-crafting-analyzer/{}", w.name))
                                .unwrap_or("/fc-crafting-analyzer".to_string())
                            attr:class="nav-link w-full justify-start"
                            on:click=close_menu
                        >
                            <Icon height="1.1em" width="1.1em" icon=i::MdiSubmarine />
                            <span class="ml-2">{t!(i18n, fc_crafting)}</span>
                        </A>

                        <A
                            href=homeworld()
                                .map(|w| format!("/leve-analyzer?world={}", w.name))
                                .unwrap_or("/leve-analyzer".to_string())
                            attr:class="nav-link w-full justify-start"
                            on:click=close_menu
                        >
                            <Icon height="1.1em" width="1.1em" icon=i::FaScrollSolid />
                            <span class="ml-2">{t!(i18n, leve_analyzer)}</span>
                        </A>

                        <A
                            href=homeworld()
                                .map(|w| format!("/trends/{}", w.name))
                                .unwrap_or("/trends".to_string())
                            attr:class="nav-link w-full justify-start"
                            on:click=close_menu
                        >
                            <Icon height="1.1em" width="1.1em" icon=i::FaChartLineSolid />
                            <span class="ml-2">{t!(i18n, market_trends)}</span>
                        </A>
                        <A
                            href=homeworld()
                                .map(|w| format!("/scrip-sources?world={}", w.name))
                                .unwrap_or("/scrip-sources".to_string())
                            attr:class="nav-link w-full justify-start"
                            on:click=close_menu
                        >
                            <Icon height="1.1em" width="1.1em" icon=i::FaCoinsSolid />
                            <span class="ml-2">{t!(i18n, scrip_sources)}</span>
                        </A>

                        <A
                            href=homeworld()
                                .map(|w| format!("/venture-analyzer?world={}", w.name))
                                .unwrap_or("/venture-analyzer".to_string())
                            attr:class="nav-link w-full justify-start"
                            on:click=move |_| set_has_focus(false)
                        >
                             <Icon height="1.1em" width="1.1em" icon=i::FaBriefcaseSolid />
                            <span class="ml-2">{t!(i18n, venture_analyzer)}</span>
                        </A>

                        <A
                            href="/items?menu-open=true"
                            attr:class="nav-link w-full justify-start"
                            on:click=close_menu
                        >
                            <Icon height="1.1em" width="1.1em" icon=i::MdiJellyfish />
                            <span class="ml-2">{t!(i18n, explorer)}</span>
                        </A>

                        <A
                            href="/currency-exchange"
                            attr:class="nav-link w-full justify-start"
                            on:click=close_menu
                        >
                            <Icon height="1.1em" width="1.1em" icon=i::BsArrowLeftRight />
                            <span class="ml-2">{t!(i18n, exchange)}</span>
                        </A>

                        <div class="divider my-1 2xl:hidden"></div>

                        <A
                            href="/settings"
                            attr:class="nav-link w-full justify-start 2xl:hidden"
                            on:click=close_menu
                        >
                            <Icon height="1.1em" width="1.1em" icon=i::IoSettingsSharp />
                            <span class="ml-2">{t!(i18n, settings)}</span>
                        </A>
                    </div>
                </div>
            </Show>
        </div>
    }
    .into_any()
}

/// A trigger that goes straight to the profile page when logged in,
/// or to the Discord login when logged out.
#[component]
pub fn UserMenu() -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    let user = Resource::new(move || {}, move |_| async move { get_login().await.ok() });

    let fallback = move || {
        view! {
            <span class="nav-link opacity-70" aria-busy="true">
                <Icon height="1.5em" width="1.5em" icon=i::BsPersonCircle />
            </span>
        }
    };

    view! {
        <Suspense fallback=fallback>
            {move || {
                let u = user.get().flatten();
                match u {
                    Some(auth) => {
                        view! {
                            <A
                                href="/profile"
                                attr:class="nav-link flex items-center"
                                attr:aria_label=move || t_string!(i18n, profile)
                            >
                                <img class="avatar" src=auth.avatar alt=auth.username />
                            </A>
                        }.into_any()
                    }
                    None => {
                        view! {
                            <a
                                rel="external"
                                href="/login"
                                class="nav-link flex items-center"
                                aria-label=move || t_string!(i18n, login_with_discord)
                            >
                                <Icon height="1.5em" width="1.5em" icon=i::BsPersonCircle />
                            </a>
                        }.into_any()
                    }
                }
            }}
        </Suspense>
    }
    .into_any()
}
