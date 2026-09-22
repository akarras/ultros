//! Lazily-loaded route wrappers.
//!
//! Each unit struct here is a `LazyRoute` whose `view` body becomes a
//! `#[lazy]` (wasm-split) function: when the frontend is built with
//! `cargo leptos build --split`, everything reachable *only* from that body
//! is emitted as a separate `.wasm` chunk that the browser fetches on first
//! navigation instead of as part of the main bundle. Without `--split` the
//! wrappers are inert — `view` is just an async fn that resolves at once.
//!
//! Wrapped: the analyzer family, Lists (which drags in `loro`), the
//! logged-in features (groups, retainers, alerts), currency exchange, the
//! item explorer and the text pages. Home, the item page and settings stay
//! eager — they are the entry routes, and a chunk on that path would only add
//! a request. `data()` is left empty on purpose; the components keep owning
//! their resources, so the wrappers never change what a route renders, only
//! *when its code arrives*.
//!
//! The wrapped argument must be a named pattern (`this`), not `_`: the
//! `#[lazy_route]` macro re-emits it as a call argument.

use leptos::prelude::*;
use leptos_router::{LazyRoute, lazy_route};

use super::{
    about::About,
    alerts::Alerts,
    analyzer::{Analyzer, AnalyzerWorldView},
    bot::BotGuide,
    changelog::Changelog,
    currency_exchange::{CurrencyExchange, CurrencySelection, ExchangeItem},
    edit_retainers::EditRetainers,
    fc_crafting_analyzer::FCCraftingAnalyzer,
    group_detail::GroupDetail,
    groups::{GroupInviteAccept, Groups},
    guest_lists::GuestListRoute,
    help::{HelpArticle, HelpIndex},
    history::History,
    item_explorer::{CategoryItems, DefaultItems, ItemExplorer, JobItems},
    job_set_detail::JobSetDetail,
    legal::{cookie_policy::CookiePolicy, privacy_policy::PrivacyPolicy},
    leve_analyzer::LeveAnalyzer,
    list_view_sync::ListRoute,
    lists::{EditLists, ListInviteAccept, Lists},
    recipe_analyzer::RecipeAnalyzer,
    retainers::{
        RetainerListings, RetainerUndercuts, Retainers, RetainersBasePath, SingleRetainerListings,
    },
    scrip_sources::ScripSources,
    vendor_resale::{VendorResale, VendorWorldView},
    vendor_sell::VendorSell,
    venture_analyzer::VentureAnalyzer,
    welcome::Welcome,
};

/// Declares a unit-struct `LazyRoute` that renders one existing component.
macro_rules! lazy_component_route {
    ($(#[$meta:meta])* $name:ident => $component:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy)]
        pub struct $name;

        #[lazy_route]
        impl LazyRoute for $name {
            fn data() -> Self {
                Self
            }

            fn view(this: Self) -> AnyView {
                let _: $name = this;
                view! { <$component /> }.into_any()
            }
        }
    };
}

// Analyzer family.
lazy_component_route!(AnalyzerRoute => Analyzer);
lazy_component_route!(AnalyzerWorldRoute => AnalyzerWorldView);
lazy_component_route!(VendorResaleRoute => VendorResale);
lazy_component_route!(VendorWorldRoute => VendorWorldView);
lazy_component_route!(VendorSellRoute => VendorSell);
lazy_component_route!(RecipeAnalyzerRoute => RecipeAnalyzer);
lazy_component_route!(FcCraftingRoute => FCCraftingAnalyzer);
lazy_component_route!(LeveRoute => LeveAnalyzer);
lazy_component_route!(ScripRoute => ScripSources);
lazy_component_route!(VentureRoute => VentureAnalyzer);

// Lists cluster. `ListsRoute` is the `ParentRoute` layout; the rest are its
// children.
lazy_component_route!(ListsRoute => Lists);
lazy_component_route!(ListViewRoute => ListRoute);
lazy_component_route!(EditListsRoute => EditLists);
lazy_component_route!(GuestListLazyRoute => GuestListRoute);
lazy_component_route!(ListInviteRoute => ListInviteAccept);

// Logged-in features: most visitors never open these.
lazy_component_route!(GroupsRoute => Groups);
lazy_component_route!(GroupDetailRoute => GroupDetail);
lazy_component_route!(GroupInviteRoute => GroupInviteAccept);
lazy_component_route!(RetainersRoute => Retainers);
lazy_component_route!(RetainersIndexRoute => RetainersBasePath);
lazy_component_route!(EditRetainersRoute => EditRetainers);
lazy_component_route!(RetainerUndercutsRoute => RetainerUndercuts);
lazy_component_route!(RetainerListingsRoute => RetainerListings);
lazy_component_route!(SingleRetainerListingsRoute => SingleRetainerListings);
lazy_component_route!(AlertsRoute => Alerts);

// Currency exchange: a self-contained tool with its own data model.
lazy_component_route!(CurrencyExchangeRoute => CurrencyExchange);
lazy_component_route!(CurrencySelectionRoute => CurrencySelection);
lazy_component_route!(ExchangeItemRoute => ExchangeItem);

// Text pages: no resources, no interactivity, surprisingly large as code.
lazy_component_route!(PrivacyPolicyRoute => PrivacyPolicy);
lazy_component_route!(CookiePolicyRoute => CookiePolicy);
lazy_component_route!(WelcomeRoute => Welcome);
lazy_component_route!(HelpIndexRoute => HelpIndex);
lazy_component_route!(HelpArticleRoute => HelpArticle);
lazy_component_route!(ChangelogRoute => Changelog);
lazy_component_route!(AboutRoute => About);
lazy_component_route!(BotGuideRoute => BotGuide);
lazy_component_route!(HistoryRoute => History);

// Item explorer: the browse-by-category/job layout and its children.
lazy_component_route!(ItemExplorerRoute => ItemExplorer);
lazy_component_route!(DefaultItemsRoute => DefaultItems);
lazy_component_route!(CategoryItemsRoute => CategoryItems);
lazy_component_route!(JobItemsRoute => JobItems);
lazy_component_route!(JobSetDetailRoute => JobSetDetail);

/// Starts fetching a lazy route's wasm chunk without navigating to it.
///
/// Used from nav-link hover/focus so the chunk is usually resident by the
/// time the user clicks. Loads are memoised by the split loader, so calling
/// this repeatedly is free; a failed load is retried on the next call. On the
/// server (and in non-split builds) it is a no-op.
pub fn preload<T: LazyRoute>() {
    #[cfg(feature = "hydrate")]
    leptos::task::spawn_local(async move {
        T::preload().await;
    });
}

/// Fetches the chunk(s) a direct load of `path` will render, and resolves
/// once they are resident.
///
/// The client awaits this *before* hydrating. Async hydration otherwise
/// pauses inside the router while the route's chunk downloads, and during
/// that pause the executor runs the effects the already-hydrated shell
/// queued — any of which can move state the not-yet-hydrated route body
/// reads (URL normalisation, restored views, cookies), so the client render
/// no longer matches the server's and tachys panics mid-walk. With the chunk
/// already loaded the router's await resolves without yielding, and
/// hydration is as atomic as the synchronous entry point used to be.
///
/// Mirrors the lazy entries of the route table in `lib.rs`; an unmatched path
/// (an eager route, or a wrapper added without a line here) just falls back to
/// the router's own fetch, which still works but re-opens the window above.
pub async fn preload_for_path(path: &str) {
    let mut segments = path.trim_matches('/').splitn(3, '/');
    let (first, second) = (segments.next().unwrap_or(""), segments.next());
    match (first, second) {
        ("flip-finder", None) => AnalyzerRoute::preload().await,
        ("flip-finder", Some(_)) => AnalyzerWorldRoute::preload().await,
        ("vendor-resale", None) => VendorResaleRoute::preload().await,
        ("vendor-resale", Some(_)) => VendorWorldRoute::preload().await,
        ("vendor-sell", _) => VendorSellRoute::preload().await,
        ("recipe-analyzer", _) => RecipeAnalyzerRoute::preload().await,
        ("fc-crafting-analyzer", _) => FcCraftingRoute::preload().await,
        ("leve-analyzer", _) => LeveRoute::preload().await,
        ("scrip-sources", _) => ScripRoute::preload().await,
        ("venture-analyzer", _) => VentureRoute::preload().await,
        // The Lists layout renders alongside whichever child matched.
        ("list", None) => {
            futures::join!(ListsRoute::preload(), EditListsRoute::preload());
        }
        ("list", Some("invite")) => {
            futures::join!(ListsRoute::preload(), ListInviteRoute::preload());
        }
        ("list", Some("device")) => {
            futures::join!(ListsRoute::preload(), GuestListLazyRoute::preload());
        }
        ("list", Some(_)) => {
            futures::join!(ListsRoute::preload(), ListViewRoute::preload());
        }
        ("groups", None) => GroupsRoute::preload().await,
        ("groups", Some(_)) => GroupDetailRoute::preload().await,
        ("group", Some("invite")) => GroupInviteRoute::preload().await,
        // The Retainers layout renders alongside whichever child matched.
        ("retainers", None) => {
            futures::join!(RetainersRoute::preload(), RetainersIndexRoute::preload());
        }
        ("retainers", Some("edit")) => {
            futures::join!(RetainersRoute::preload(), EditRetainersRoute::preload());
        }
        ("retainers", Some("undercuts")) => {
            futures::join!(RetainersRoute::preload(), RetainerUndercutsRoute::preload());
        }
        ("retainers", Some("listings")) => {
            // `listings` and `listings/:id` are different routes; fetching both
            // costs one small extra module and keeps the arm simple.
            futures::join!(
                RetainersRoute::preload(),
                RetainerListingsRoute::preload(),
                SingleRetainerListingsRoute::preload()
            );
        }
        ("alerts", _) => AlertsRoute::preload().await,
        ("currency-exchange", None) => {
            futures::join!(
                CurrencyExchangeRoute::preload(),
                CurrencySelectionRoute::preload()
            );
        }
        ("currency-exchange", Some(_)) => {
            futures::join!(
                CurrencyExchangeRoute::preload(),
                ExchangeItemRoute::preload()
            );
        }
        ("privacy", _) => PrivacyPolicyRoute::preload().await,
        ("cookie-policy", _) => CookiePolicyRoute::preload().await,
        ("welcome", _) => WelcomeRoute::preload().await,
        ("help", None) => HelpIndexRoute::preload().await,
        ("help", Some(_)) => HelpArticleRoute::preload().await,
        ("changelog", _) => ChangelogRoute::preload().await,
        ("about", _) => AboutRoute::preload().await,
        ("bot", _) => BotGuideRoute::preload().await,
        ("history", _) => HistoryRoute::preload().await,
        // The Item Explorer layout renders alongside whichever child matched.
        ("items", None) => {
            futures::join!(ItemExplorerRoute::preload(), DefaultItemsRoute::preload());
        }
        ("items", Some("category")) => {
            futures::join!(ItemExplorerRoute::preload(), CategoryItemsRoute::preload());
        }
        ("items", Some("jobset")) => {
            // `jobset/:jobset` and `jobset/:jobset/set/:ilvl` are different
            // routes; fetch both rather than parse the third segment.
            futures::join!(
                ItemExplorerRoute::preload(),
                JobItemsRoute::preload(),
                JobSetDetailRoute::preload()
            );
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    // The table above is matched by hand, so pin the shapes it must
    // recognise. `preload` is a no-op off wasm, which keeps this a pure
    // routing check.
    use super::preload_for_path;
    use futures::FutureExt;

    #[test]
    fn every_lazy_path_shape_resolves_synchronously_off_wasm() {
        for path in [
            "/flip-finder",
            "/flip-finder/Cerberus",
            "/vendor-resale/Cerberus/",
            "/vendor-sell",
            "/recipe-analyzer/Cerberus",
            "/list",
            "/list/",
            "/list/123",
            "/list/invite/abc",
            "/list/device/xyz",
            "/groups",
            "/groups/7",
            "/group/invite/abc",
            "/retainers",
            "/retainers/edit",
            "/retainers/listings",
            "/retainers/listings/42",
            "/alerts",
            "/currency-exchange",
            "/currency-exchange/3",
            "/privacy",
            "/help",
            "/help/getting-started",
            "/items",
            "/items/category/5",
            "/items/jobset/BLM",
            "/items/jobset/BLM/set/700",
            "/item/1",
            "",
        ] {
            assert!(
                preload_for_path(path).now_or_never().is_some(),
                "{path} must not pend on the server"
            );
        }
    }
}
