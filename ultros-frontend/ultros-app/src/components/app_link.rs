//! A non-panicking `<A/>`.
//!
//! `leptos_router`'s `<A/>` reads router context twice, and both reads are
//! `expect`s: `use_resolved_path` (`link.rs:161`,
//! "called use_resolved_path outside a <Router>") and the `RouterContext`
//! lookup that follows it (`link.rs:132`, "tried to use <A/> outside a
//! <Router/>"). Both hold for every link reached through a normal render —
//! `AppShell` sits inside `<Router>` and everything else hangs off it — but
//! neither holds for a link that ends up rendering under an owner which never
//! saw the router's `provide_context`.
//!
//! Prod hits exactly that, and it is the same failure mode
//! [`crate::i18n_fallback`] documents: when a suspended SSR fragment's owner is
//! disposed before the fragment resolves, `ScopedFuture::new` falls back to
//! `Owner::current().unwrap_or_default()` and hands the children a *fresh,
//! empty* owner instead of failing loudly. The first context read in that
//! subtree panics, and a panic mid-response aborts the SSR stream, so one link
//! costs the whole page. GlitchTip #7171/#6895 and #7172/#7119 are those two
//! panics, still firing daily.
//!
//! [`AppLink`] degrades instead: with router context present it *is* `<A/>`,
//! and without it renders a plain `<a>`. That fallback is not a downgrade in
//! behaviour — `Router` intercepts clicks on every same-origin anchor through
//! one global handler (`location::handle_anchor_click`), not per-`<A/>`, so a
//! plain anchor still navigates client-side. What is lost is `aria-current`
//! and relative-href resolution, and neither means anything under a dead owner:
//! the app's hrefs are all absolute, which `use_resolved_path` returns
//! unchanged anyway.
//!
//! Router context itself is `pub(crate)` in `leptos_router`, so the presence
//! check goes through [`RouterAvailable`], a marker this crate provides from
//! `AppShell` — which renders inside `<Router>` and already calls
//! `use_location()`, so it cannot be reached without router context either.
//!
//! [`use_location_or_default`] is the same idea for the other panicking router
//! read this app makes. `use_location()` is an `expect` too
//! (`hooks.rs:173`, "Tried to access Location outside a <Router>."), and
//! GlitchTip #7278 caught it firing from `QueryButton` — the sort/filter chips
//! build their href out of the live pathname and query. Under a dead owner it
//! yields an *empty* location instead, so the href degrades to a query-only
//! relative URL (`?sort=price`). A browser resolves that against the document's
//! own path, which is the path the link wanted in the first place; what is lost
//! is the other query params riding along, not the link.

use leptos::either::Either;
use leptos::prelude::*;
use leptos_router::components::{A, ToHref};
use leptos_router::hooks::use_location;
use leptos_router::location::{Location, State};
use leptos_router::params::ParamsMap;

/// Marker for "router context is reachable from this owner".
///
/// Provided by `AppShell`; see the module docs for why the router's own
/// context can't be probed directly.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RouterAvailable;

/// Announce that everything rendered below this owner sits inside `<Router>`.
pub fn provide_router_available() {
    provide_context(RouterAvailable);
}

/// Whether [`provide_router_available`] is visible from the current owner.
pub fn has_router_context() -> bool {
    use_context::<RouterAvailable>().is_some()
}

/// [`use_location`] that yields an empty location instead of panicking when
/// the router context is missing.
///
/// The empty pathname is deliberate: callers format `{pathname}{query}`, so
/// the href they build stays a valid relative URL that the browser resolves
/// against the current document. See the module docs.
pub fn use_location_or_default() -> Location {
    if has_router_context() {
        use_location()
    } else {
        Location {
            pathname: Memo::new(|_| String::new()),
            search: Memo::new(|_| String::new()),
            query: Memo::new(|_| ParamsMap::new()),
            hash: Memo::new(|_| String::new()),
            state: RwSignal::new(State::default()).read_only(),
        }
    }
}

/// `<A/>` that renders a plain `<a>` instead of panicking when the router
/// context is missing.
#[component]
pub fn AppLink<H>(
    /// Used to calculate the link's `href` attribute, exactly as `<A/>` does.
    href: H,
    /// If `true`, the link is marked active only when the location matches
    /// exactly. Ignored by the plain-anchor fallback, which has no active
    /// state to mark.
    #[prop(optional)]
    exact: bool,
    /// The nodes or elements to be shown inside the link.
    children: Children,
) -> impl IntoView
where
    H: ToHref + Send + Sync + 'static,
{
    if has_router_context() {
        Either::Left(view! {
            <A href exact>
                {children()}
            </A>
        })
    } else {
        // Resolved once, eagerly: a dead owner has no reactivity left to drive
        // an updating href anyway.
        let href = href.to_href()();
        Either::Right(view! { <a href=href>{children()}</a> })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reproduces GlitchTip #7171/#6895: a link rendered under an owner that
    /// never saw `<Router>`. `<A/>` panics on construction here, which on the
    /// server kills the SSR response mid-stream.
    #[test]
    #[should_panic(expected = "called use_resolved_path outside a <Router>")]
    fn plain_a_panics_when_the_router_context_is_missing() {
        let owner = Owner::new();
        owner.with(|| {
            let _ = view! {
                <A href="/help">"Help"</A>
            };
        });
    }

    #[test]
    fn falls_back_to_a_plain_anchor_when_the_router_context_is_missing() {
        let owner = Owner::new();
        owner.with(|| {
            let html = view! {
                <AppLink href="/help">"Help"</AppLink>
            }
            .to_html();
            assert!(html.contains("href=\"/help\""), "{html}");
            assert!(html.contains("Help"), "{html}");
        });
    }

    /// Reproduces GlitchTip #7278 at its source: the `expect` inside
    /// `use_location()`.
    #[test]
    #[should_panic(expected = "Tried to access Location outside a <Router>")]
    fn use_location_panics_when_the_router_context_is_missing() {
        let owner = Owner::new();
        owner.with(|| {
            let _ = use_location();
        });
    }

    #[test]
    fn use_location_or_default_yields_an_empty_location_instead() {
        let owner = Owner::new();
        owner.with(|| {
            let location = use_location_or_default();
            assert_eq!(location.pathname.get(), "");
            assert_eq!(location.search.get(), "");
            assert_eq!(location.hash.get(), "");
            assert!(location.query.with(|q| q.to_query_string().is_empty()));
        });
    }

    /// With the marker in scope the component is `<A/>`, so the same missing
    /// router context must still panic rather than silently degrade — the
    /// fallback is for dead owners, not for a mis-mounted app.
    #[test]
    #[should_panic(expected = "called use_resolved_path outside a <Router>")]
    fn uses_the_router_link_when_the_marker_is_provided() {
        let owner = Owner::new();
        owner.with(|| {
            provide_router_available();
            let _ = view! {
                <AppLink href="/help">"Help"</AppLink>
            };
        });
    }
}
