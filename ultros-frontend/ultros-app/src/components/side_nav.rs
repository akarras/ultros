use crate::components::account_menu::AccountMenu;
use crate::components::app_link::AppLink;
use crate::components::home_world_menu::HomeWorldMenu;
use crate::components::icon::Icon;
use crate::components::notification_inbox::NotificationInbox;
use crate::components::region_menu::RegionMenu;
use crate::global_state::changelog::use_whats_new_indicator;
use crate::global_state::home_world::use_home_world;
use crate::global_state::platform::use_platform_hotkeys;
use crate::global_state::search_overlay::use_search_overlay_state;
use crate::global_state::side_nav::use_side_nav_settings;
use crate::i18n::{t, t_string, use_i18n};
use crate::routes::lazy;
use icondata as i;
use leptos::prelude::*;
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
/// Deliberately a plain `<a>` rather than `<AppLink>`: the router link it wraps sets
/// `aria-current` by comparing the whole resolved href against the URL, and
/// most of these hrefs carry a world (`/flip-finder/{homeworld}`,
/// `/scrip-sources/{homeworld}`). Viewing a world other than your homeworld — or
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
    /// When true, a small unread dot appears at the trailing edge of the row.
    ///
    /// The signal must be false during SSR and the first client render — the
    /// dot is added to the DOM by a client `Effect`, not rendered by the
    /// server — or hydration will find a node the server never wrote. See
    /// `use_whats_new_indicator`.
    #[prop(optional, into)]
    badge: Option<Signal<bool>>,
    /// Accessible name for `badge`, read out after the row's label.
    #[prop(optional, into)]
    badge_label: Option<Signal<String>>,
    /// Starts fetching the destination's lazily-loaded wasm chunk on hover /
    /// focus, so it is usually resident by the time the user clicks. See
    /// `routes::lazy::preload`. Repeated calls are free (memoised loader).
    #[prop(optional)]
    preload: Option<fn()>,
    children: Children,
) -> impl IntoView {
    let location = use_location();
    let preload = move || {
        if let Some(preload) = preload {
            preload();
        }
    };
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

    let badge_view = move || {
        badge.is_some_and(|badge| badge.get()).then(|| {
            let label = badge_label.map(|label| label.get()).unwrap_or_default();
            view! {
                <span class="side-nav-badge" aria-hidden="true"></span>
                <span class="sr-only">{label}</span>
            }
        })
    };

    view! {
        <a
            href=move || href.get()
            class=class
            aria-current=current
            on:mouseenter=move |_| preload()
            on:focus=move |_| preload()
        >
            <Icon icon=icon />
            <span class="side-nav-label">{children()}</span>
            {badge_view}
        </a>
    }
}

/// `/list` renders the Lists layout *and* its index child, so warm both chunks.
fn preload_lists() {
    lazy::preload::<lazy::ListsRoute>();
    lazy::preload::<lazy::EditListsRoute>();
}

/// `/currency-exchange` renders its layout *and* the selection index.
fn preload_currency_exchange() {
    lazy::preload::<lazy::CurrencyExchangeRoute>();
    lazy::preload::<lazy::CurrencySelectionRoute>();
}

/// `/retainers/listings` renders the Retainers layout *and* the listings tab.
fn preload_retainer_listings() {
    lazy::preload::<lazy::RetainersRoute>();
    lazy::preload::<lazy::RetainerListingsRoute>();
}

/// `/items` renders the Item Explorer layout *and* its default listing.
fn preload_item_explorer() {
    lazy::preload::<lazy::ItemExplorerRoute>();
    lazy::preload::<lazy::DefaultItemsRoute>();
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
    let apple_hotkeys = use_platform_hotkeys().apple;
    let (homeworld, _set_homeworld) = use_home_world();
    let whats_new = use_whats_new_indicator();

    // Build world-aware URLs from the current home world, falling back to
    // the world-less route when none is set.
    let with_world = move |path_with_world: &str, path_no_world: &str| {
        let path_with_world = path_with_world.to_string();
        let path_no_world = path_no_world.to_string();
        Signal::derive(move || match homeworld.get() {
            Some(w) => {
                path_with_world.replace("{world}", &leptos_router::location::Url::escape(&w.name))
            }
            None => path_no_world.clone(),
        })
    };

    let git_hash = env!("GIT_HASH");

    view! {
        <aside class="side-nav" aria-label=t_string!(i18n, side_nav_aria_primary)>
            <div class="side-nav-brand">
                <AppLink href="/" attr:class="side-nav-brand-link">
                    <Icon icon=i::MdiJellyfish width="1.6em" height="1.6em" />
                    <span class="side-nav-brand-text">"ULTROS"</span>
                </AppLink>
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
                    <span class="side-nav-kbd">
                        {move || {
                            if apple_hotkeys.get() {
                                "⌘K".to_string()
                            } else {
                                t_string!(i18n, hotkey_ctrl_k).to_string()
                            }
                        }}
                    </span>
                </button>
                <SideNavItem
                    href="/items".to_string()
                    section="items"
                    icon=i::MdiJellyfish
                    hero=true
                    preload=preload_item_explorer
                >
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
                    preload={lazy::preload::<lazy::AnalyzerWorldRoute>}
                    icon=i::FaMoneyBillTrendUpSolid
                >
                    {t!(i18n, flip_finder)}
                </SideNavItem>
                <SideNavItem
                    href=with_world("/vendor-resale/{world}", "/vendor-resale")
                    section="vendor-resale"
                    preload={lazy::preload::<lazy::VendorWorldRoute>}
                    icon=i::FaShopSolid
                >
                    {t!(i18n, vendor_resale)}
                </SideNavItem>
                <SideNavItem
                    href=with_world("/vendor-sell/{world}", "/vendor-sell")
                    section="vendor-sell"
                    preload={lazy::preload::<lazy::VendorSellRoute>}
                    icon=i::FaCashRegisterSolid
                >
                    {t!(i18n, vendor_sell)}
                </SideNavItem>
                <SideNavItem
                    href=with_world("/recipe-analyzer/{world}", "/recipe-analyzer")
                    section="recipe-analyzer"
                    preload={lazy::preload::<lazy::RecipeAnalyzerRoute>}
                    icon=i::FaHammerSolid
                >
                    {t!(i18n, recipe_analyzer)}
                </SideNavItem>
                <SideNavItem
                    href=with_world("/fc-crafting-analyzer/{world}", "/fc-crafting-analyzer")
                    section="fc-crafting-analyzer"
                    preload={lazy::preload::<lazy::FcCraftingRoute>}
                    icon=i::MdiSubmarine
                >
                    {t!(i18n, fc_crafting)}
                </SideNavItem>
                <SideNavItem
                    href=with_world("/leve-analyzer/{world}", "/leve-analyzer")
                    section="leve-analyzer"
                    preload={lazy::preload::<lazy::LeveRoute>}
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
                    href=with_world("/scrip-sources/{world}", "/scrip-sources")
                    section="scrip-sources"
                    preload={lazy::preload::<lazy::ScripRoute>}
                    icon=i::FaCoinsSolid
                >
                    {t!(i18n, scrip_sources)}
                </SideNavItem>
                <SideNavItem
                    href=with_world("/venture-analyzer/{world}", "/venture-analyzer")
                    section="venture-analyzer"
                    preload={lazy::preload::<lazy::VentureRoute>}
                    icon=i::FaBriefcaseSolid
                >
                    {t!(i18n, venture_analyzer)}
                </SideNavItem>
                <SideNavItem
                    href="/currency-exchange".to_string()
                    section="currency-exchange"
                    preload=preload_currency_exchange
                    icon=i::BsArrowLeftRight
                >
                    {t!(i18n, currency_exchange)}
                </SideNavItem>

                <div class="side-nav-section-header">{t!(i18n, side_nav_saved)}</div>

                <SideNavItem
                    href="/list".to_string()
                    section="list"
                    icon=i::AiOrderedListOutlined
                    preload=preload_lists
                >
                    {t!(i18n, lists)}
                </SideNavItem>
                <SideNavItem
                    href="/groups".to_string()
                    section="groups"
                    icon=i::BiGroupSolid
                    preload={lazy::preload::<lazy::GroupsRoute>}
                >
                    {t!(i18n, groups)}
                </SideNavItem>
                <SideNavItem
                    href="/retainers/listings".to_string()
                    preload=preload_retainer_listings
                    section="retainers"
                    icon=i::FaUserTieSolid
                >
                    {t!(i18n, retainers)}
                </SideNavItem>
            </nav>

            // Informational links (Discord bot, help, changelog) live in the
            // bottom cluster rather than the tool list — they're reference
            // material, not somewhere you work. Hidden in the collapsed rail
            // along with the footer (see `.app-shell-collapsed .side-nav-info`);
            // expand the sidebar to reach them.
            <nav class="side-nav-info" aria-label=t_string!(i18n, help_label)>
                <SideNavItem
                    href="/bot".to_string()
                    section="bot"
                    icon=i::BsDiscord
                    preload={lazy::preload::<lazy::BotGuideRoute>}
                >
                    {t!(i18n, discord_bot)}
                </SideNavItem>
                <SideNavItem
                    href="/help".to_string()
                    section="help"
                    icon=i::BsBook
                    preload={lazy::preload::<lazy::HelpIndexRoute>}
                >
                    {t!(i18n, help_label)}
                </SideNavItem>
                <SideNavItem
                    href="/changelog".to_string()
                    preload={lazy::preload::<lazy::ChangelogRoute>}
                    section="changelog"
                    icon=i::BsMegaphone
                    badge=whats_new
                    badge_label=Signal::derive(move || t_string!(i18n, changelog_whats_new).to_string())
                >
                    {t!(i18n, changelog_label)}
                </SideNavItem>
            </nav>

            <NotificationInbox />
            <HomeWorldMenu />
            <RegionMenu />
            <AccountMenu />

            // Footer sits BELOW the inbox + home-world + region + account
            // rows — the social links and commit hash are ambient
            // reference, not controls, so they anchor the very bottom
            // (#1235). Collapsed, the 56px sidebar has no room for the
            // Discord + GitHub pair (GitHub was clipped by the right edge)
            // and the version hash is hidden anyway, so the whole footer is
            // hidden at that width — see `.app-shell-collapsed
            // .side-nav-footer`. The inbox, home-world, region and account
            // rows survive collapse as icon-only triggers, and the
            // collapsed account panel's fixed `bottom` still holds because
            // the footer contributes no height there.
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
