//! Opinionated defaults for URL-backed filters.
//!
//! The analyzer tools land first-time visitors on a sale-velocity-filtered view
//! instead of a list topped by items that sell once a month. The default lives
//! in the URL rather than in the filter logic, so chips, Clear All, and shared
//! links all keep behaving exactly as they do for a hand-typed filter.

use std::str::FromStr;

use leptos::oco::Oco;
use leptos::prelude::*;
use leptos_router::NavigateOptions;
use leptos_router::hooks::query_signal_with_options;
use leptos_router::location::Url;

use crate::components::app_link::use_query_map_or_default;
use crate::components::saved_views::default_view_query;

/// Default ceiling on predicted time to next sale: items that sell at least
/// once a day. Parsed with `humantime`, same as anything typed into the box.
pub const DEFAULT_MAX_SALE_TIME: &str = "1d";

/// The same velocity floor, expressed as the crafting analyzers' daily-sales
/// metric rather than as a duration.
pub const DEFAULT_MIN_DAILY_SALES: f32 = 1.0;

/// Navigation options for filter query params.
///
/// `query_signal`'s defaults (`replace: false`, `scroll: true`) mean every
/// keystroke in a filter box pushes a history entry and yanks the window back
/// to the top. Filters are not navigation.
fn filter_nav_options() -> NavigateOptions {
    NavigateOptions {
        replace: true,
        scroll: false,
        ..Default::default()
    }
}

/// [`query_signal_with_options`] that degrades to an inert signal instead of
/// panicking when the router context is missing.
///
/// Every `query_signal` opens with `use_query_map()`, which is an `expect` on
/// router context ("Tried to access reactive URL outside a <Router>
/// component."). A suspended SSR fragment can rebuild its children under an
/// owner that never saw `<Router>` — `ScopedFuture` falls back to a *fresh,
/// empty* `Owner` — and a panic there aborts the whole SSR response, so one
/// URL-backed filter costs the entire page (GlitchTip #7305, same mechanism as
/// #7171/#7172 and #7278).
///
/// Presence is decided on [`RouterAvailable`](crate::components::app_link),
/// the `Location` `AppShell` captured *inside* `<Router>`, which is also where
/// the fallback's empty query map comes from — so the check and the value it
/// guards are one lookup that cannot disagree with itself.
///
/// The degraded signal reads as "param absent" and writes nowhere. That is the
/// right shape for the case it exists for: a fragment rendering under a dead
/// owner is server-side markup nobody navigates from, and setters never run on
/// the server anyway.
pub fn query_signal_or_default<T>(
    key: impl Into<Oco<'static, str>>,
    nav_options: NavigateOptions,
) -> (Memo<Option<T>>, SignalSetter<Option<T>>)
where
    T: FromStr + ToString + PartialEq + Send + Sync + 'static,
{
    if use_context::<crate::components::app_link::RouterAvailable>().is_none() {
        return (Memo::new(|_| None), SignalSetter::map(|_: Option<T>| ()));
    }
    query_signal_with_options::<T>(key, nav_options)
}

/// [`leptos_router::hooks::query_signal`], router-safe per
/// [`query_signal_or_default`].
///
/// Imported in place of the router's own so that no app code reaches the
/// panicking hook — the same trade `AppLink` made for `<A/>`. Behaviour is
/// identical wherever `<Router>` is reachable, which is everywhere a user ever
/// sees.
pub fn query_signal<T>(
    key: impl Into<Oco<'static, str>>,
) -> (Memo<Option<T>>, SignalSetter<Option<T>>)
where
    T: FromStr + ToString + PartialEq + Send + Sync + 'static,
{
    query_signal_or_default::<T>(key, NavigateOptions::default())
}

/// A [`query_signal`](leptos_router::hooks::query_signal) for a filter param,
/// using [`filter_nav_options`], and router-safe per [`query_signal_or_default`].
pub fn filter_query_signal<T>(key: &'static str) -> (Memo<Option<T>>, SignalSetter<Option<T>>)
where
    T: FromStr + ToString + PartialEq + Send + Sync + 'static,
{
    query_signal_or_default::<T>(key, filter_nav_options())
}

/// Write `default` into the URL if `key` is absent when this mounts.
///
/// Seeding fires only when the param is *absent*, so a link that carries the
/// param is honored verbatim — `?next-sale=` (unparseable) and `?min-sales=0`
/// both mean "no limit", and both are what the input box produces when a user
/// empties it.
///
/// Call this from the **route** component. Anything rendered inside a
/// `Suspense`/resource closure remounts whenever its resource changes — a live
/// market refetch, a world switch — and seeding there would silently reinstate
/// a filter the user had just cleared. The route component mounts once per
/// navigation, which is the granularity a default wants.
pub fn seed_query_default<T>(key: &'static str, default: T)
where
    T: FromStr + ToString + PartialEq + Clone + Send + Sync + 'static,
{
    let query = use_query_map_or_default();
    if query.with_untracked(|q| q.get(key).is_some() || q.get("v").is_some())
        || crate::last_view::has_restorable_view()
    {
        return;
    }
    let (_, set_value) = filter_query_signal::<T>(key);
    Effect::new(move |_| {
        if query.with_untracked(|q| q.get(key).is_none() && q.get("v").is_none()) {
            set_value.set(Some(default.clone()));
        }
    });
}

/// Split a stored query string (`?a=1&b=2`) into decoded key/value pairs.
///
/// Decoded with the router's own [`Url::unescape`], the exact inverse of the
/// escaping `ParamsMap::to_query_string` applies on the way out. Writing a
/// still-encoded value back through a `query_signal` setter would encode it
/// a second time, turning a saved `?name=Grade%208` into `Grade%25208`.
fn parse_query_pairs(query: &str) -> Vec<(String, String)> {
    query
        .trim_start_matches('?')
        .split('&')
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let key = Url::unescape(key);
            // A lone `?`, a trailing `&`, or a doubled `&&` each split into an
            // empty segment. Dropping those is what keeps them from seeding a
            // param named "".
            (!key.is_empty()).then(|| (key, Url::unescape(value)))
        })
        .collect()
}

/// Language selects presentation, not a saved market view. A freshly shared
/// localized URL should still receive the same landing defaults as a bare URL.
fn has_view_query(query: &str) -> bool {
    parse_query_pairs(query)
        .iter()
        .any(|(key, _)| key != "lang")
}

/// Seed a whole default *view* onto a bare Flip Finder URL.
///
/// Returns whether the URL was bare, so the caller can skip the per-param
/// seeds it would otherwise run — a view already carries its own recency
/// filter, and layering [`seed_query_default`] on top would add a param the
/// view never asked for.
///
/// Unlike [`seed_query_default`], which fires on a single *absent* param,
/// this fires only when the query map has **no view parameters**, and the
/// emptiness is decided synchronously at setup, before any seeding effect
/// has had a chance to write. That is what keeps Clear All cleared: it
/// empties the query long after this component mounted, and nothing here
/// re-reads the query afterwards.
///
/// Params are written through `query_signal` setters rather than a single
/// `use_navigate`, even though a whole-view seed knows the entire query it
/// wants. `AnalyzerWorldNavigator` rebuilds the URL from an *untracked* query
/// snapshot in its own mount-time effect, so a competing `navigate` is simply
/// overwritten — the params never land. Setters push into the router's
/// mutation queue and are replayed onto whatever URL the navigator produces,
/// and they coalesce into one navigation anyway.
///
/// Same route-component rule as [`seed_query_default`] — call this from the
/// route, never from inside a `Suspense`/resource closure.
pub fn seed_flip_finder_default_view() -> bool {
    if crate::last_view::has_restorable_view() {
        return true;
    }
    let query = use_query_map_or_default();
    let was_bare = query.with_untracked(|q| !has_view_query(&q.to_query_string()));
    if was_bare {
        Effect::new(move |_| {
            // Reads localStorage, so it must happen post-hydration — which is
            // exactly when an Effect runs, and never on the server.
            //
            // An empty default is a real choice ("land me on the whole
            // list"), and parses to no pairs, so it seeds nothing.
            for (key, value) in parse_query_pairs(&default_view_query()) {
                if key == "lang" {
                    continue;
                }
                let (_, set) = query_signal_or_default::<String>(key, filter_nav_options());
                set.set(Some(value));
            }
        });
    }
    was_bare
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::components::saved_views::{
        FALLBACK_DEFAULT_VIEW, built_in_views, fallback_default_query,
    };

    /// Reproduces GlitchTip #7305 at its source: every `query_signal` starts
    /// with `use_query_map()`, which is an `expect` on router context. On the
    /// server that panic aborts the whole response, so it is not a local
    /// failure — it is the page.
    #[test]
    #[should_panic(expected = "Tried to access reactive URL outside a <Router> component.")]
    fn the_router_query_signal_panics_when_the_router_context_is_missing() {
        let owner = Owner::new();
        owner.with(|| {
            let _ = query_signal_with_options::<String>("roi", filter_nav_options());
        });
    }

    /// The regression guard for #7305: the app's own wrapper must render an
    /// absent param instead, under an owner that never saw `<Router>`.
    #[test]
    fn filter_query_signal_is_inert_without_the_router() {
        let owner = Owner::new();
        owner.with(|| {
            let (value, set) = filter_query_signal::<String>("roi");
            assert_eq!(value.get(), None);
            // A write must be a no-op, not a panic: the setter cannot navigate
            // with no router to navigate.
            set.set(Some("30".to_string()));
            assert_eq!(value.get(), None);
        });
    }

    /// The invariant the fix rests on: no app code reaches the router's
    /// panicking URL hooks, so a new filter cannot quietly reintroduce #7305.
    /// The same trade `AppLink` made for `<A/>` — the wrapper is only worth
    /// anything while it is the *only* door.
    ///
    /// `query_defaults.rs` itself is the one file allowed to name them; the
    /// virtual-grid fixture is a dev harness mounted under the real shell.
    #[test]
    fn no_app_code_calls_the_panicking_router_url_hooks() {
        fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
            let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
                .expect("the crate's src tree is readable")
                .map(|e| e.expect("a readable directory entry").path())
                .collect();
            entries.sort();
            for path in entries {
                if path.is_dir() {
                    walk(&path, out);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") {
                    continue;
                }
                let src = std::fs::read_to_string(&path).expect("a readable source file");
                let full = path.to_string_lossy().replace('\\', "/");
                let name = match full.rsplit_once("/src/") {
                    Some((_, rel)) => rel.to_string(),
                    None => full,
                };
                // Only the production half: a test is allowed — required,
                // even — to call the panicking hook and prove it panics.
                let production = match src.split_once(&format!("#[cfg({})]", "test")) {
                    Some((head, rest))
                        if rest.trim_start().starts_with(&format!("mod {}", "test")) =>
                    {
                        head.to_string()
                    }
                    _ => src,
                };
                out.push((name, production));
            }
        }
        let mut files = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut files,
        );
        assert!(
            files.len() > 100,
            "the walk must reach the whole crate, not one directory"
        );

        const ALLOWED: [&str; 2] = ["query_defaults.rs", "components/virtual_grid/fixture.rs"];
        let mut offenders = Vec::new();
        for (name, src) in &files {
            if ALLOWED.contains(&name.as_str()) {
                continue;
            }
            for hook in ["use_query_map", "query_signal_with_options"] {
                // Only real uses: a doc comment naming the hook is how the
                // fallbacks explain themselves.
                let used = src.lines().any(|line| {
                    !line.trim_start().starts_with("//")
                        && line
                            .match_indices(hook)
                            .any(|(at, _)| !line[at + hook.len()..].starts_with("_or_default"))
                });
                if used {
                    offenders.push(format!("{name} uses {hook}"));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "route these through query_defaults instead: {offenders:?}"
        );
    }

    /// ...and `seed_query_default`, which reads the query map directly, has to
    /// survive the same owner. It seeds through the inert setter, so nothing
    /// lands — the point is that the response is not aborted.
    #[test]
    fn seeding_a_default_survives_a_missing_router() {
        let owner = Owner::new();
        owner.with(|| {
            seed_query_default("next-sale", DEFAULT_MAX_SALE_TIME.to_string());
        });
    }

    #[test]
    fn language_only_links_still_receive_the_saved_default_view() {
        assert!(!has_view_query("?lang=ja"));
        assert!(!has_view_query("?%6cang=de"));
        assert!(!has_view_query(""));
        assert!(has_view_query("?lang=ja&roi=30"));
        assert!(has_view_query("?lang=ja&next-sale="));
    }

    /// The seeded value goes through the same `humantime` parse as anything
    /// typed into the box, and an unparseable duration doesn't error — it just
    /// leaves `predicted_time` as `None`, i.e. no filter at all. A typo in the
    /// constant would silently undo the default, so pin it.
    #[test]
    fn default_max_sale_time_parses_to_one_day() {
        assert_eq!(
            humantime::parse_duration(DEFAULT_MAX_SALE_TIME).expect("default must parse"),
            std::time::Duration::from_secs(60 * 60 * 24),
        );
    }

    /// The landing view is *derived* from the built-in menu entry rather
    /// than written out again, so editing the "Realistic flips" preset moves
    /// the default with it. Pin the derivation: a hardcoded copy would pass
    /// the day it was written and silently rot on the first preset tweak.
    #[test]
    fn fallback_default_is_the_realistic_built_in() {
        let realistic = built_in_views()
            .into_iter()
            .find(|v| v.name == FALLBACK_DEFAULT_VIEW)
            .expect("the fallback default must name a view that exists in the menu");
        assert_eq!(fallback_default_query(), realistic.query);
    }

    /// What makes the landing view "realistic" is that it filters at all: a
    /// bare or trivially-filtered query is the unfiltered list this default
    /// exists to replace.
    #[test]
    fn fallback_default_actually_filters() {
        let q = fallback_default_query();
        assert!(q.starts_with('?'), "expected a query string, got {q:?}");
        for param in ["min-buy=", "last-sold=", "roi=", "sort="] {
            assert!(q.contains(param), "landing view {q:?} must set {param}");
        }
    }

    /// Seeding a view is an alternative to the per-param seeds, not an
    /// addition to them — the view carries its own recency filter, and
    /// layering `next-sale` on top would apply a filter the view never asked
    /// for. `AnalyzerWorldView` relies on that being true of the built-in.
    #[test]
    fn fallback_default_carries_its_own_recency_filter() {
        assert!(fallback_default_query().contains("last-sold=1d"));
    }

    /// The landing view is applied param-by-param, so every param the preset
    /// declares has to survive the split. This is the seeded set the user
    /// actually gets — derived from the preset, never written out here.
    #[test]
    fn the_realistic_preset_splits_into_its_filters() {
        let pairs = parse_query_pairs(&fallback_default_query());
        assert_eq!(
            pairs,
            vec![
                ("min-buy".to_string(), "5000".to_string()),
                ("last-sold".to_string(), "1d".to_string()),
                ("roi".to_string(), "30".to_string()),
                ("sort".to_string(), "profit-per-day".to_string()),
            ],
            "the seeded set drifted from the Realistic flips preset",
        );
    }

    /// "No filters at all" is a legitimate saved default, and it must seed
    /// nothing rather than a param named "".
    #[test]
    fn an_empty_default_seeds_nothing() {
        assert!(parse_query_pairs("").is_empty());
        assert!(parse_query_pairs("?").is_empty());
        assert!(parse_query_pairs("?&&").is_empty());
    }

    /// Values are stored escaped and re-escaped on the way back out, so the
    /// seed has to decode or a saved name filter gains a `%25` per visit.
    #[test]
    fn values_are_decoded_once() {
        assert_eq!(
            parse_query_pairs("?name=Grade%208&roi=30"),
            vec![
                ("name".to_string(), "Grade 8".to_string()),
                ("roi".to_string(), "30".to_string()),
            ]
        );
    }

    /// An empty value is what a filter box produces when the user clears it,
    /// and `?next-sale=` explicitly means "no limit" (see `seed_query_default`).
    /// It must round-trip as a present-but-empty param, not vanish.
    #[test]
    fn an_empty_value_is_kept() {
        assert_eq!(
            parse_query_pairs("?next-sale=&roi=30"),
            vec![
                ("next-sale".to_string(), String::new()),
                ("roi".to_string(), "30".to_string()),
            ]
        );
    }
}
