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
//! panics.
//!
//! # Why probing for the router was not enough
//!
//! The first fix (#1198) kept `<A/>` for the live case and fell back to a
//! plain `<a>` only when a [`RouterAvailable`] marker was missing. Both panics
//! kept firing on builds carrying it, and #7172 in particular is proof that a
//! *probe-then-use* guard cannot work here: inside `<A/>` the two
//! `use_context::<RouterContext>()` calls are back to back, with
//! `use_resolved_path` first. For the second one to panic while the first
//! succeeded, the context has to vanish *between* them — the owner is torn
//! down concurrently with the render, so any check made before a read can be
//! stale by the time the read happens.
//!
//! So [`AppLink`] does not probe. It never constructs an `<A/>` and never
//! reads `RouterContext` at all: it renders a plain `<a>` and computes
//! `aria-current` itself, porting `<A/>`'s own matching rules. There is no
//! longer a code path from a link to a panicking router hook, in a live render
//! or a dead one.
//!
//! Nothing is lost by dropping `<A/>`. `Router` intercepts clicks on every
//! same-origin anchor through one global handler
//! (`location::handle_anchor_click`), not per-`<A/>`, so a plain anchor still
//! navigates client-side; `aria-current` is reproduced below; and
//! `use_resolved_path` only ever resolved *relative* hrefs, which this app no
//! longer has — every `AppLink` href is absolute. `SideNavItem` already made
//! the same trade for the same reason.
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
//!
//! That one is race-free for the same reason `AppLink` is: the [`Location`]
//! handed back is the one `AppShell` captured *inside* `<Router>` and stored in
//! [`RouterAvailable`], so the presence check and the value it guards are a
//! single context lookup that cannot disagree with itself.

use leptos::prelude::*;
use leptos_router::components::ToHref;
use leptos_router::hooks::use_location;
use leptos_router::location::{Location, State};
use leptos_router::params::ParamsMap;
use std::sync::Arc;

/// The router's [`Location`], captured inside `<Router>` by `AppShell`.
///
/// Carrying the value rather than being a bare marker is deliberate: it makes
/// "is the router reachable?" and "give me the location" the same lookup, so
/// the two cannot disagree under a concurrent teardown. See the module docs.
#[derive(Clone)]
pub struct RouterAvailable(Location);

/// Announce that everything rendered below this owner sits inside `<Router>`.
///
/// Panics outside a `<Router>`, exactly as the `use_location()` it wraps does —
/// the fallbacks in this module are for dead owners, not for an app mounted in
/// the wrong place.
pub fn provide_router_available() {
    provide_context(RouterAvailable(use_location()));
}

/// [`use_location`] that yields an empty location instead of panicking when
/// the router context is missing.
///
/// The empty pathname is deliberate: callers format `{pathname}{query}`, so
/// the href they build stays a valid relative URL that the browser resolves
/// against the current document. See the module docs.
pub fn use_location_or_default() -> Location {
    match use_context::<RouterAvailable>() {
        Some(RouterAvailable(location)) => location,
        None => Location {
            pathname: Memo::new(|_| String::new()),
            search: Memo::new(|_| String::new()),
            query: Memo::new(|_| ParamsMap::new()),
            hash: Memo::new(|_| String::new()),
            state: RwSignal::new(State::default()).read_only(),
        },
    }
}

/// Resolve `.` / `..` segments and strip the query and hash, so an href can be
/// compared against a pathname.
///
/// Ported from `leptos_router`'s private `link.rs::normalize_path`, which is
/// what `<A/>` runs before deciding whether it is active. Kept equivalent so
/// `aria-current` does not shift under any link as it changes hands.
fn normalize_path(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let mut del = 0;
    let mut it = path
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .split('/')
        .rev()
        .peekable();

    let init = if it.peek() == Some(&"..") {
        String::from("/")
    } else {
        String::new()
    };
    let mut path = it
        .filter(|v| {
            if *v == ".." {
                del += 1;
                false
            } else if *v == "." {
                false
            } else if del > 0 {
                del -= 1;
                false
            } else {
                true
            }
        })
        // Cannot reverse before the fold: the filter would run forwards again.
        .fold(init, |mut p, v| {
            p.reserve(v.len() + 1);
            p.insert(0, '/');
            p.insert_str(0, v);
            p
        });
    path.truncate(path.len().saturating_sub(1));

    if !path.starts_with('/') {
        path.insert(0, '/');
    }
    path
}

/// Whether `location` sits at or below `href`.
///
/// Ported from `leptos_router`'s private `link.rs::is_active_for`, with
/// `strict_trailing_slash` fixed to `false` — the default `<A/>` used, and the
/// only value any call site here ever wanted.
fn is_active_for(href: &str, location: &str) -> bool {
    let mut href_f = href.split('/');
    // `location` must be consumed first so the zip doesn't drain `href_f`
    // early; the `c > 1` allowance is what keeps a bare "/" from matching
    // every page.
    std::iter::zip(location.split('/'), href_f.by_ref())
        .enumerate()
        .all(|(c, (loc_p, href_p))| loc_p == href_p || href_p.is_empty() && c > 1)
        && match href_f.next() {
            // No href segments left: the location is nested inside the href.
            None => true,
            // A trailing slash on the href, which is not strict here.
            Some("") => true,
            // href="/item/one" must not be active for location="/item".
            _ => false,
        }
}

/// `<A/>`'s active test: an exact link matches only its own path, a normal one
/// also matches everything nested below it.
fn link_is_active(href: &str, location: &str, exact: bool) -> bool {
    let href = normalize_path(href);
    if exact {
        location == href
    } else {
        is_active_for(&href, location)
    }
}

/// A same-origin app link.
///
/// A plain `<a>` that carries `<A/>`'s `aria-current`, and — unlike `<A/>` —
/// never touches router context, so it cannot abort an SSR response when it
/// renders under a disposed owner. See the module docs.
///
/// `href` must be absolute (`/items`, not `items`): without
/// `use_resolved_path` there is no matched route to resolve a relative href
/// against.
#[component]
pub fn AppLink<H>(
    /// Used to calculate the link's `href` attribute, exactly as `<A/>` does.
    href: H,
    /// If `true`, the link is marked active only when the location matches
    /// exactly, rather than also when the location is nested below it.
    #[prop(optional)]
    exact: bool,
    /// The nodes or elements to be shown inside the link.
    children: Children,
) -> impl IntoView
where
    H: ToHref + Send + Sync + 'static,
{
    let href = Arc::new(href);
    let href_attr = {
        let href = Arc::clone(&href);
        move || href.to_href()()
    };
    let pathname = use_location_or_default().pathname;
    let aria_current = move || {
        let target = href.to_href()();
        // `try_with`, not `with`: the captured location outlives nothing, but
        // its memo belongs to the router's owner, and reading a disposed one
        // panics. An unhighlighted link beats a dead SSR stream.
        pathname
            .try_with(|location| link_is_active(&target, location, exact))
            .unwrap_or(false)
            .then_some("page")
    };

    view! {
        <a href=href_attr aria-current=aria_current>
            {children()}
        </a>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The panic this component exists to avoid, at its source: `<A/>` reads
    /// router context on construction, which on the server kills the SSR
    /// response mid-stream. Reproduces GlitchTip #7171/#6895.
    #[test]
    #[should_panic(expected = "called use_resolved_path outside a <Router>")]
    fn plain_a_panics_when_the_router_context_is_missing() {
        use leptos_router::components::A;
        let owner = Owner::new();
        owner.with(|| {
            let _ = view! {
                <A href="/help">"Help"</A>
            };
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

    fn provide_location_at(path: &'static str) {
        provide_context(RouterAvailable(Location {
            pathname: Memo::new(move |_| path.to_string()),
            search: Memo::new(|_| String::new()),
            query: Memo::new(|_| ParamsMap::new()),
            hash: Memo::new(|_| String::new()),
            state: RwSignal::new(State::default()).read_only(),
        }));
    }

    /// The regression guard for #7171/#7172: a link under an owner that never
    /// saw `<Router>` must render, not panic.
    #[test]
    fn renders_without_router_context() {
        let owner = Owner::new();
        owner.with(|| {
            let html = view! {
                <AppLink href="/help">"Help"</AppLink>
            }
            .to_html();
            assert!(html.contains("href=\"/help\""), "{html}");
            assert!(html.contains("Help"), "{html}");
            assert!(!html.contains("aria-current"), "{html}");
        });
    }

    /// ...and with the router reachable it must *still* not panic, because it
    /// no longer reads router context at all. This is the case the old
    /// marker-based guard sent into `<A/>`, and the one #7172 kept firing from.
    #[test]
    fn renders_with_the_router_present() {
        let owner = Owner::new();
        owner.with(|| {
            provide_location_at("/help");
            let html = view! {
                <AppLink href="/help">"Help"</AppLink>
            }
            .to_html();
            assert!(html.contains("href=\"/help\""), "{html}");
            assert!(html.contains("aria-current=\"page\""), "{html}");
        });
    }

    #[test]
    fn marks_nothing_current_on_an_unrelated_page() {
        let owner = Owner::new();
        owner.with(|| {
            provide_location_at("/items");
            let html = view! {
                <AppLink href="/help">"Help"</AppLink>
            }
            .to_html();
            assert!(!html.contains("aria-current"), "{html}");
        });
    }

    /// `aria-current` parity with `<A/>`. Each row is
    /// (href, current path, exact, expected-active).
    #[test]
    fn active_matching_matches_the_router_link() {
        let cases = [
            ("/help", "/help", false, true),
            ("/help", "/help/pricing", false, true),
            ("/help", "/helpdesk", false, false),
            ("/help", "/help/pricing", true, false),
            ("/help", "/help", true, true),
            ("/", "/", false, true),
            // Root is special-cased by the zip/enumerate rule: the brand link
            // must not light up on every page.
            ("/", "/items", false, false),
            ("/retainers/edit", "/retainers", false, false),
            ("/retainers", "/retainers/edit", false, true),
            ("/retainers/edit", "/retainers/listings", true, false),
            // Query strings are stripped before matching, so a world-scoped
            // link still highlights on its own page.
            ("/scrip-sources?world=Siren", "/scrip-sources", false, true),
            ("/item/Siren/12412", "/item/Siren/12412", false, true),
        ];
        for (href, location, exact, expected) in cases {
            assert_eq!(
                link_is_active(href, location, exact),
                expected,
                "href={href} location={location} exact={exact}",
            );
        }
    }

    #[test]
    fn normalize_path_strips_query_and_hash() {
        assert_eq!(normalize_path("/help"), "/help");
        assert_eq!(normalize_path("/help?world=Siren"), "/help");
        assert_eq!(normalize_path("/help#frag"), "/help");
        assert_eq!(normalize_path("/one/two/../three"), "/one/three");
        assert_eq!(normalize_path("/"), "/");
    }
}
