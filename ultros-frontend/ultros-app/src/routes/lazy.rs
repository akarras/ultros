//! Lazily-loaded route wrappers.
//!
//! Each unit struct here is a `LazyRoute` whose `view` body becomes a
//! `#[lazy]` (wasm-split) function: when the frontend is built with
//! `cargo leptos build --split`, everything reachable *only* from that body
//! is emitted as a separate `.wasm` chunk that the browser fetches on first
//! navigation instead of as part of the main bundle. Without `--split` the
//! wrappers are inert — `view` is just an async fn that resolves at once.
//!
//! Only the two clusters that dominate the bundle are wrapped for now: the
//! analyzer family and Lists (which drags in `loro`). `data()` is left empty
//! on purpose; the components keep owning their resources, so the wrappers
//! never change what a route renders, only *when its code arrives*.
//!
//! The wrapped argument must be a named pattern (`this`), not `_`: the
//! `#[lazy_route]` macro re-emits it as a call argument.

use leptos::prelude::*;
use leptos_router::{LazyRoute, lazy_route};

use super::{
    analyzer::{Analyzer, AnalyzerWorldView},
    fc_crafting_analyzer::FCCraftingAnalyzer,
    guest_lists::GuestListRoute,
    leve_analyzer::LeveAnalyzer,
    list_view_sync::ListRoute,
    lists::{EditLists, ListInviteAccept, Lists},
    recipe_analyzer::RecipeAnalyzer,
    scrip_sources::ScripSources,
    vendor_resale::{VendorResale, VendorWorldView},
    vendor_sell::VendorSell,
    venture_analyzer::VentureAnalyzer,
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
