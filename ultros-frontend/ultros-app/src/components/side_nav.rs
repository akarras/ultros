use crate::components::account_menu::AccountMenu;
use crate::components::icon::Icon;
use crate::global_state::home_world::use_home_world;
use crate::global_state::search_overlay::use_search_overlay_state;
use crate::global_state::side_nav::use_side_nav_settings;
use crate::i18n::{t, t_string, use_i18n};
use icondata as i;
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_location;

/// First path segment of `path` — the section of the app a URL belongs to.
/// `"/"` maps to `""`, `"/items/jobset/PLD"` to `"items"`.
fn section_of(path: &str) -> &str {
    path.trim_start_matches('/')
        .split('/')
        .next()
        .unwrap_or_default()
}

/// One sidebar entry, highlighted whenever the current page belongs to
/// `section`.
///
/// Deliberately a plain `<a>` rather than the router's `<A>`: `<A>` sets
/// `aria-current` by comparing the whole resolved href against the URL, and
/// most of these hrefs carry a world (`/flip-finder/{homeworld}`,
/// `/scrip-sources?world=…`). Viewing a world other than your homeworld — or
/// any sub-route like `/items/category/5` — would then leave the sidebar with
/// nothing highlighted. Matching the first path segment is what "which tool am
/// I in" actually means here. The router intercepts clicks on any same-origin
/// anchor, so navigation stays client-side either way.
#[component]
fn SideNavItem(
    #[prop(into)] href: Signal<String>,
    section: &'static str,
    #[prop(into)] icon: Signal<icondata_core::Icon>,
    /// Renders as one of the two "find an item" rows above the tool list,
    /// visually paired with the Search button that sits beside it.
    #[prop(optional)]
    hero: bool,
    children: Children,
) -> impl IntoView {
    let location = use_location();
    let current = move || {
        location
            .pathname
            .with(|path| section_of(path) == section)
            .then_some("page")
    };
    let class = if hero {
        "side-nav-item side-nav-item-hero"
    } else {
        "side-nav-item"
    };

    view! {
        <a href=move || href.get() class=class aria-current=current>
            <Icon icon=icon />
            <span class="side-nav-label">{children()}</span>
        </a>
    }
}

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
    let search_overlay = use_search_overlay_state();
    let (homeworld, _set_homeworld) = use_home_world();

    // Build world-aware URLs from the current home world, falling back to
    // the world-less route when none is set.
    let with_world = move |path_with_world: &str, path_no_world: &str| {
        let path_with_world = path_with_world.to_string();
        let path_no_world = path_no_world.to_string();
        Signal::derive(move || match homeworld.get() {
            Some(w) => path_with_world.replace("{world}", &w.name),
            None => path_no_world.clone(),
        })
    };

    let git_hash = env!("GIT_HASH");

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
                // Search and Item Explorer are the two "find me an item"
                // surfaces — Search jumps to a known item, Explorer browses
                // when you don't know what you want — so they pair above the
                // rule, leaving Tools an honest list of nine analyzers.
                <button
                    class="side-nav-item side-nav-item-hero"
                    aria-label=t_string!(i18n, search).to_string()
                    on:click=move |_| search_overlay.toggle()
                >
                    <Icon icon=i::AiSearchOutlined />
                    <span class="side-nav-label">{t!(i18n, search)}</span>
                    <span class="side-nav-kbd">"⌘K"</span>
                </button>
                <SideNavItem href="/items".to_string() section="items" icon=i::MdiJellyfish hero=true>
                    {t!(i18n, item_explorer)}
                </SideNavItem>

                <div class="side-nav-rule"></div>

                <SideNavItem href="/".to_string() section="" icon=i::AiHomeFilled>
                    {t!(i18n, home)}
                </SideNavItem>

                <div class="side-nav-section-header">{t!(i18n, side_nav_tools)}</div>

                <SideNavItem
                    href=with_world("/flip-finder/{world}", "/flip-finder")
                    section="flip-finder"
                    icon=i::FaMoneyBillTrendUpSolid
                >
                    {t!(i18n, flip_finder)}
                </SideNavItem>
                <SideNavItem
                    href=with_world("/vendor-resale/{world}", "/vendor-resale")
                    section="vendor-resale"
                    icon=i::FaShopSolid
                >
                    {t!(i18n, vendor_resale)}
                </SideNavItem>
                <SideNavItem
                    href=with_world("/recipe-analyzer?world={world}", "/recipe-analyzer")
                    section="recipe-analyzer"
                    icon=i::FaHammerSolid
                >
                    {t!(i18n, recipe_analyzer)}
                </SideNavItem>
                <SideNavItem
                    href=with_world("/fc-crafting-analyzer/{world}", "/fc-crafting-analyzer")
                    section="fc-crafting-analyzer"
                    icon=i::MdiSubmarine
                >
                    {t!(i18n, fc_crafting)}
                </SideNavItem>
                <SideNavItem
                    href=with_world("/leve-analyzer?world={world}", "/leve-analyzer")
                    section="leve-analyzer"
                    icon=i::FaScrollSolid
                >
                    {t!(i18n, leve_analyzer)}
                </SideNavItem>
                <SideNavItem
                    href=with_world("/trends/{world}", "/trends")
                    section="trends"
                    icon=i::FaChartLineSolid
                >
                    {t!(i18n, market_trends)}
                </SideNavItem>
                <SideNavItem
                    href=with_world("/scrip-sources?world={world}", "/scrip-sources")
                    section="scrip-sources"
                    icon=i::FaCoinsSolid
                >
                    {t!(i18n, scrip_sources)}
                </SideNavItem>
                <SideNavItem
                    href=with_world("/venture-analyzer?world={world}", "/venture-analyzer")
                    section="venture-analyzer"
                    icon=i::FaBriefcaseSolid
                >
                    {t!(i18n, venture_analyzer)}
                </SideNavItem>
                <SideNavItem
                    href="/currency-exchange".to_string()
                    section="currency-exchange"
                    icon=i::BsArrowLeftRight
                >
                    {t!(i18n, currency_exchange)}
                </SideNavItem>

                <div class="side-nav-section-header">{t!(i18n, side_nav_saved)}</div>

                <SideNavItem href="/list".to_string() section="list" icon=i::AiOrderedListOutlined>
                    {t!(i18n, lists)}
                </SideNavItem>
                <SideNavItem href="/groups".to_string() section="groups" icon=i::BiGroupSolid>
                    {t!(i18n, groups)}
                </SideNavItem>
                <SideNavItem
                    href="/retainers/listings".to_string()
                    section="retainers"
                    icon=i::BiGroupSolid
                >
                    {t!(i18n, retainers)}
                </SideNavItem>
                <SideNavItem href="/alerts".to_string() section="alerts" icon=i::BsBell>
                    {t!(i18n, alerts)}
                </SideNavItem>

                <div class="side-nav-section-header">{t!(i18n, help_label)}</div>

                <SideNavItem href="/bot".to_string() section="bot" icon=i::BsDiscord>
                    {t!(i18n, discord_bot)}
                </SideNavItem>
                <SideNavItem href="/help".to_string() section="help" icon=i::BsBook>
                    {t!(i18n, help_label)}
                </SideNavItem>
            </nav>

            <AccountMenu />

            <div class="side-nav-footer">
                <a href=crate::DISCORD_INVITE class="side-nav-icon-link" aria-label="Discord">
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
