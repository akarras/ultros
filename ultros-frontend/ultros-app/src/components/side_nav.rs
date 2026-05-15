use crate::components::icon::Icon;
use crate::global_state::home_world::use_home_world;
use crate::global_state::side_nav::use_side_nav_settings;
use crate::i18n::{t, t_string, use_i18n};
use git_const::git_short_hash;
use icondata as i;
use leptos::prelude::*;
use leptos_router::components::A;

/// Persistent left sidebar. Brand at top, sections in the middle,
/// utility links + version hash at the bottom.
///
/// Renders at 240px desktop width via the `side-nav` CSS utility
/// (see `style/tailwind.css`). Collapse + mobile drawer behavior
/// is added in later tasks.
#[component]
pub fn SideNav() -> impl IntoView {
    let i18n = use_i18n();
    let nav = use_side_nav_settings();
    let (homeworld, _set_homeworld) = use_home_world();

    // Build world-aware URLs the same way `AppsMenu` does.
    let with_world = move |path_with_world: &str, path_no_world: &str| {
        let path_with_world = path_with_world.to_string();
        let path_no_world = path_no_world.to_string();
        Signal::derive(move || match homeworld.get() {
            Some(w) => path_with_world.replace("{world}", &w.name),
            None => path_no_world.clone(),
        })
    };

    let git_hash = git_short_hash!();

    view! {
        <aside class="side-nav" aria-label=t_string!(i18n, side_nav_aria_primary)>
            <div class="side-nav-brand">
                <A href="/" attr:class="side-nav-brand-link">
                    <Icon icon=i::MdiJellyfish width="1.6em" height="1.6em" />
                    <span class="side-nav-brand-text">"ULTROS"</span>
                </A>
                <button
                    class="side-nav-collapse hidden lg:inline-flex"
                    aria-label=t_string!(i18n, side_nav_toggle_sidebar).to_string()
                    aria-pressed=move || if nav.collapsed.get() { "true" } else { "false" }
                    on:click=move |_| nav.collapsed.update(|v| *v = !*v)
                >
                    <Icon icon=i::BiChevronLeftSolid />
                </button>
            </div>

            <nav class="side-nav-sections">
                <A href="/" exact=true attr:class="side-nav-item">
                    <Icon icon=i::AiHomeFilled />
                    <span class="side-nav-label">{t!(i18n, home)}</span>
                </A>

                <div class="side-nav-section-header">{t!(i18n, side_nav_tools)}</div>

                <A href=with_world("/flip-finder/{world}", "/flip-finder") attr:class="side-nav-item">
                    <Icon icon=i::FaMoneyBillTrendUpSolid />
                    <span class="side-nav-label">{t!(i18n, flip_finder)}</span>
                </A>
                <A href=with_world("/vendor-resale/{world}", "/vendor-resale") attr:class="side-nav-item">
                    <Icon icon=i::FaShopSolid />
                    <span class="side-nav-label">{t!(i18n, vendor_resale)}</span>
                </A>
                <A href=with_world("/recipe-analyzer?world={world}", "/recipe-analyzer") attr:class="side-nav-item">
                    <Icon icon=i::FaHammerSolid />
                    <span class="side-nav-label">{t!(i18n, recipe_analyzer)}</span>
                </A>
                <A href=with_world("/fc-crafting-analyzer/{world}", "/fc-crafting-analyzer") attr:class="side-nav-item">
                    <Icon icon=i::MdiSubmarine />
                    <span class="side-nav-label">{t!(i18n, fc_crafting)}</span>
                </A>
                <A href=with_world("/leve-analyzer?world={world}", "/leve-analyzer") attr:class="side-nav-item">
                    <Icon icon=i::FaScrollSolid />
                    <span class="side-nav-label">{t!(i18n, leve_analyzer)}</span>
                </A>
                <A href=with_world("/trends/{world}", "/trends") attr:class="side-nav-item">
                    <Icon icon=i::FaChartLineSolid />
                    <span class="side-nav-label">{t!(i18n, market_trends)}</span>
                </A>
                <A href=with_world("/scrip-sources?world={world}", "/scrip-sources") attr:class="side-nav-item">
                    <Icon icon=i::FaCoinsSolid />
                    <span class="side-nav-label">{t!(i18n, scrip_sources)}</span>
                </A>
                <A href=with_world("/venture-analyzer?world={world}", "/venture-analyzer") attr:class="side-nav-item">
                    <Icon icon=i::FaBriefcaseSolid />
                    <span class="side-nav-label">{t!(i18n, venture_analyzer)}</span>
                </A>
                <A href="/items" attr:class="side-nav-item">
                    <Icon icon=i::MdiJellyfish />
                    <span class="side-nav-label">{t!(i18n, item_explorer)}</span>
                </A>
                <A href="/currency-exchange" attr:class="side-nav-item">
                    <Icon icon=i::BsArrowLeftRight />
                    <span class="side-nav-label">{t!(i18n, currency_exchange)}</span>
                </A>

                <div class="side-nav-section-header">{t!(i18n, side_nav_saved)}</div>

                <A href="/list" attr:class="side-nav-item">
                    <Icon icon=i::AiOrderedListOutlined />
                    <span class="side-nav-label">{t!(i18n, lists)}</span>
                </A>
                <A href="/retainers/listings" attr:class="side-nav-item">
                    <Icon icon=i::BiGroupSolid />
                    <span class="side-nav-label">{t!(i18n, retainers)}</span>
                </A>
                <A href="/alerts" attr:class="side-nav-item">
                    <Icon icon=i::BsBell />
                    <span class="side-nav-label">{t!(i18n, alerts)}</span>
                </A>
            </nav>

            <div class="side-nav-footer">
                <a href="https://discord.gg/pgdq9nGUP2" class="side-nav-icon-link" aria-label="Discord">
                    <Icon icon=i::BsDiscord />
                </a>
                <a href="https://github.com/akarras/ultros" class="side-nav-icon-link" aria-label="GitHub">
                    <Icon icon=i::IoLogoGithub />
                </a>
                <a
                    href=format!("https://github.com/akarras/ultros/commit/{git_hash}")
                    class="side-nav-version"
                    title=t_string!(i18n, version).to_string()
                >
                    {git_hash}
                </a>
            </div>
        </aside>
    }
    .into_any()
}
