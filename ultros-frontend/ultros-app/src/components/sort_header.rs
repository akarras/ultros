//! Sortable column header, shared by every tool table.
//!
//! Four routes had grown their own copy of this — the Flip Finder's (the
//! reference), Scrip Sources' near-identical fork, the Item Explorer's
//! string-keyed variant, and Vendor Resale's inline `QueryButton` with a
//! hardcoded down arrow. They disagreed on the details that matter: which
//! direction a fresh click applies, whether the arrow reflects the direction
//! actually in effect, and whether `?dir=` is written when it is redundant.
//!
//! The contract here:
//!
//! * clicking an inactive column sorts by it in that column's
//!   [`SortColumn::default_dir`]; clicking the active column flips it,
//! * the arrow always reflects the direction actually applied,
//! * `dir` is omitted from the href when it matches the column's default, so
//!   the common case stays a clean `?sort=…` and bookmarks don't accumulate a
//!   redundant param,
//! * every other query param survives — that's the part each copy got subtly
//!   different.

use leptos::prelude::*;
use leptos_router::location::Location;
use leptos_router::params::ParamsMap;

use crate::components::app_link::use_location_or_default;
use crate::components::term_badge::{TermBadge, TermRole};

use crate::components::icon::Icon;
use icondata as i;

/// `?dir=` — sort direction. Absent means the active column's
/// [`SortColumn::default_dir`].
#[derive(Debug, PartialEq, Eq, Clone, Copy, Default)]
pub enum SortDir {
    Asc,
    #[default]
    Desc,
}

impl SortDir {
    /// The direction a click on the already-active column moves to.
    pub fn flipped(self) -> Self {
        match self {
            SortDir::Asc => SortDir::Desc,
            SortDir::Desc => SortDir::Asc,
        }
    }
}

impl std::str::FromStr for SortDir {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "asc" => Ok(SortDir::Asc),
            "desc" => Ok(SortDir::Desc),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for SortDir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SortDir::Asc => "asc",
            SortDir::Desc => "desc",
        })
    }
}

/// A route's sort-mode enum, as far as [`SortHeader`] needs to know it.
///
/// [`Display`](std::fmt::Display) has to produce exactly the token the route's
/// `FromStr` parses back out of `?sort=`, since that round trip is the whole
/// mechanism.
pub trait SortColumn: Copy + PartialEq + std::fmt::Display + Send + Sync + 'static {
    /// The column in effect when `?sort=` is absent or unparseable. Must match
    /// whatever the route's sort itself falls back to, or the highlighted
    /// header lies about which column the rows are ordered by.
    fn fallback() -> Self;

    /// Direction applied when this column is first clicked, and when `?dir=`
    /// is absent. Defaults to descending — override where a column reads
    /// best-first ascending (a cost, a time-to-sell).
    fn default_dir(self) -> SortDir {
        SortDir::Desc
    }
}

/// Direction a click on `mode` applies.
///
/// Clicking the column already in effect flips it; clicking any other column
/// starts from that column's own default, because arriving at a new column in
/// the wrong direction buries exactly the rows it was clicked for.
pub(crate) fn next_dir<M: SortColumn>(mode: M, is_active: bool, current: SortDir) -> SortDir {
    if is_active {
        current.flipped()
    } else {
        mode.default_dir()
    }
}

/// The href a header links to: the current query with `sort`/`dir` rewritten
/// and everything else left alone.
///
/// `dir` is written only when it differs from the column's default, so the
/// common case stays a clean `?sort=…`.
pub(crate) fn sort_href<M: SortColumn>(
    pathname: &str,
    mut q: ParamsMap,
    mode: M,
    is_active: bool,
    current: SortDir,
    reset_keys: &[&str],
) -> String {
    for key in reset_keys {
        q.remove(key);
    }
    q.remove("sort");
    q.remove("dir");
    q.insert("sort".to_string(), mode.to_string());
    let next = next_dir(mode, is_active, current);
    if next != mode.default_dir() {
        q.insert("dir".to_string(), next.to_string());
    }
    format!("{}{}", pathname, q.to_query_string())
}

/// `aria-sort` for a header cell.
fn column_aria_sort<M: SortColumn>(
    mode: M,
    sort: Signal<Option<M>>,
    dir: Signal<Option<SortDir>>,
) -> &'static str {
    if sort.get().unwrap_or_else(M::fallback) != mode {
        return "none";
    }
    match dir.get().unwrap_or_else(|| mode.default_dir()) {
        SortDir::Asc => "ascending",
        SortDir::Desc => "descending",
    }
}

/// Sort `rows` under `cmp` oriented by `dir`, truncating to the best `limit`
/// rows first when there are more than that. Both steps share the one
/// oriented comparator — truncating in a fixed direction while sorting in the
/// other would keep exactly the wrong rows.
pub fn sort_and_truncate<T>(
    rows: &mut Vec<T>,
    dir: SortDir,
    limit: usize,
    cmp: impl Fn(&T, &T) -> std::cmp::Ordering,
) {
    let oriented = |a: &T, b: &T| match dir {
        SortDir::Asc => cmp(a, b),
        SortDir::Desc => cmp(a, b).reverse(),
    };
    if rows.len() > limit {
        rows.select_nth_unstable_by(limit, oriented);
        rows.truncate(limit);
    }
    rows.sort_unstable_by(oriented);
}

/// Compare two optional sort keys so rows without a value sort last in
/// *both* directions. The direction flip applies only between two present
/// values — reversing the whole comparator instead would drag the
/// value-less rows to the top exactly when the user asks for best-first.
pub fn cmp_none_last<T>(
    a: Option<T>,
    b: Option<T>,
    dir: SortDir,
    cmp: impl Fn(&T, &T) -> std::cmp::Ordering,
) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => match dir {
            SortDir::Asc => cmp(&x, &y),
            SortDir::Desc => cmp(&x, &y).reverse(),
        },
    }
}

/// A complete sortable header cell: the `role="columnheader"` div with a live
/// `aria-sort`, wrapping a [`SortHeader`] link. Tables that own their cell
/// markup (responsive classes on the cell, grids) can keep composing
/// [`SortHeader`] directly; everything else should use this.
#[component]
pub fn SortableHeaderCell<M>(
    /// Column this header sorts by.
    mode: M,
    #[prop(into)] label: String,
    /// Classes for the cell (widths, padding, responsive visibility).
    #[prop(into, optional)]
    class: String,
    /// Current `?sort=`, as parsed by the route.
    #[prop(into)]
    sort_mode: Signal<Option<M>>,
    /// Current `?dir=`, as parsed by the route.
    #[prop(into)]
    sort_dir: Signal<Option<SortDir>>,
    /// Query keys dropped when the sort changes (e.g. `page`).
    #[prop(optional)]
    reset_keys: &'static [&'static str],
    /// Hover text for the whole header cell.
    #[prop(optional, into)]
    title: Option<String>,
    /// A muted 10px second line ("listing · Aether"). When set the cell
    /// lays out as two lines; pass `px-3 py-2 leading-tight` in `class`.
    #[prop(optional, into)]
    sub_label: Option<Signal<String>>,
    /// The formula operator this column plays, rendered before the label.
    #[prop(optional)]
    badge: Option<TermRole>,
    /// Brand tint plus a bottom hairline: this column feeds Profit.
    #[prop(optional, into)]
    emphasized: Option<Signal<bool>>,
    /// Line-2 content after the sub-label (the "use" pill). Rendered only
    /// alongside `sub_label`; DOM order is the `SortHeader` `<a>` first,
    /// then the sub-label line `<div>` holding the text `<span>` and this
    /// `<button>` — two focus stops, never nested.
    #[prop(optional)]
    trailing: Option<ViewFn>,
    /// The containing VirtualGrid cell already owns column semantics.
    #[prop(optional)]
    embedded: bool,
) -> impl IntoView
where
    M: SortColumn,
{
    let two_line = sub_label.is_some();
    let cell_class = move || {
        let mut c = class.clone();
        if two_line {
            c.push_str(" flex flex-col justify-center gap-0.5");
        }
        if emphasized.is_some_and(|e| e.get()) {
            c.push_str(" bg-[color:color-mix(in_srgb,var(--brand-ring)_18%,transparent)] shadow-[inset_0_-2px_0_var(--brand-ring)]");
        }
        c
    };
    view! {
        <div
            role=(!embedded).then_some("columnheader")
            class=cell_class
            title=title
            aria-sort=move || (!embedded).then(|| column_aria_sort(mode, sort_mode, sort_dir))
        >
            // Everything-unset must render a single concrete child with no
            // `Option`-typed slot: an `Option<T>` child that is `None` still
            // writes a `<!>` hydration-marker comment (tachys), which the 35
            // existing call sites (all of which leave `badge`/`sub_label`
            // unset) never emitted before this component grew these props.
            {match (badge, sub_label) {
                (None, None) => {
                    view! { <SortHeader mode label sort_mode sort_dir reset_keys /> }.into_any()
                }
                (badge, sub_label) => view! {
                    {match badge {
                        Some(role) => view! {
                            <div class="flex items-center gap-2 min-w-0">
                                <TermBadge role=role />
                                <SortHeader mode label sort_mode sort_dir reset_keys />
                            </div>
                        }
                        .into_any(),
                        None => {
                            view! { <SortHeader mode label sort_mode sort_dir reset_keys /> }
                                .into_any()
                        }
                    }}
                    {sub_label.map(|s| match trailing {
                        None => view! {
                            <div class="text-[10px] leading-3 font-normal normal-case text-[color:var(--color-text-muted)] truncate max-w-full">{move || s.get()}</div>
                        }
                        .into_any(),
                        Some(trailing) => view! {
                            <div class="text-[10px] leading-3 font-normal normal-case text-[color:var(--color-text-muted)] flex items-center gap-1 max-w-full">
                                <span class="truncate">{move || s.get()}</span>
                                {trailing.run()}
                            </div>
                        }
                        .into_any(),
                    })}
                }
                .into_any(),
            }}
        </div>
    }
    .into_any()
}

/// One sortable column header link.
///
/// Renders the `<a>` only; callers keep their own `role="columnheader"` cell
/// so column widths and responsive visibility stay with the table that owns
/// them.
#[component]
pub fn SortHeader<M>(
    /// Column this header sorts by.
    mode: M,
    #[prop(into)] label: String,
    /// Current `?sort=`, as parsed by the route.
    #[prop(into)]
    sort_mode: Signal<Option<M>>,
    /// Current `?dir=`, as parsed by the route.
    #[prop(into)]
    sort_dir: Signal<Option<SortDir>>,
    /// Query keys dropped when the sort changes. The Item Explorer resets
    /// `page` this way — re-sorting a paginated list otherwise lands you on
    /// page 7 of a completely different ordering.
    #[prop(optional)]
    reset_keys: &'static [&'static str],
) -> impl IntoView
where
    M: SortColumn,
{
    // See `QueryButton`: `use_location()` panics under a dead owner, and this
    // header renders inside suspended tables.
    let Location {
        pathname, query, ..
    } = use_location_or_default();
    let is_active = Signal::derive(move || sort_mode.get().unwrap_or_else(M::fallback) == mode);
    let dir = Signal::derive(move || {
        sort_dir
            .get()
            .unwrap_or_else(|| sort_mode.get().unwrap_or_else(M::fallback).default_dir())
    });
    view! {
        <a
            class=move || {
                if is_active() {
                    "!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                } else {
                    "!text-brand-300 hover:text-brand-200"
                }
            }
            aria-current=move || if is_active() { "true" } else { "false" }
            href=move || {
                sort_href(&pathname(), query(), mode, is_active(), dir(), reset_keys)
            }
        >
            <div class="flex items-center gap-2">
                {label}
                {move || {
                    is_active()
                        .then(|| match dir() {
                            SortDir::Asc => view! { <Icon icon=i::BiSortUpRegular /> },
                            SortDir::Desc => view! { <Icon icon=i::BiSortDownRegular /> },
                        })
                }}
            </div>
        </a>
    }
    .into_any()
}

#[cfg(test)]
mod test {
    use super::*;

    /// Stand-in for a route's sort enum: one column that reads best-first
    /// descending (a profit) and one that reads best-first ascending (a cost),
    /// which is the combination the four copies disagreed about.
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Col {
        Profit,
        Cost,
    }

    impl std::fmt::Display for Col {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(match self {
                Col::Profit => "profit",
                Col::Cost => "cost",
            })
        }
    }

    impl SortColumn for Col {
        fn fallback() -> Self {
            Col::Profit
        }
        fn default_dir(self) -> SortDir {
            match self {
                Col::Profit => SortDir::Desc,
                Col::Cost => SortDir::Asc,
            }
        }
    }

    fn params(pairs: &[(&str, &str)]) -> ParamsMap {
        let mut q = ParamsMap::new();
        for (k, v) in pairs {
            q.insert(k.to_string(), v.to_string());
        }
        q
    }

    #[test]
    fn clicking_the_active_column_flips_direction() {
        assert_eq!(next_dir(Col::Profit, true, SortDir::Desc), SortDir::Asc);
        assert_eq!(next_dir(Col::Profit, true, SortDir::Asc), SortDir::Desc);
    }

    #[test]
    fn clicking_a_different_column_starts_at_that_columns_default() {
        // Not "always descending": a cost column arriving descending shows the
        // most expensive rows first, which is never why it was clicked.
        assert_eq!(next_dir(Col::Profit, false, SortDir::Asc), SortDir::Desc);
        assert_eq!(next_dir(Col::Cost, false, SortDir::Desc), SortDir::Asc);
    }

    #[test]
    fn the_default_direction_stays_out_of_the_url() {
        // Clicking a fresh column writes `?sort=` alone; if `dir` leaked in
        // here every shared link would carry a redundant param, and flipping
        // the default later would silently change what old links mean.
        assert_eq!(
            sort_href("/t", params(&[]), Col::Cost, false, SortDir::Desc, &[]),
            "/t?sort=cost"
        );
        assert_eq!(
            sort_href("/t", params(&[]), Col::Profit, false, SortDir::Asc, &[]),
            "/t?sort=profit"
        );
    }

    #[test]
    fn the_non_default_direction_is_written() {
        assert_eq!(
            sort_href("/t", params(&[]), Col::Cost, true, SortDir::Asc, &[]),
            "/t?sort=cost&dir=desc"
        );
        assert_eq!(
            sort_href("/t", params(&[]), Col::Profit, true, SortDir::Desc, &[]),
            "/t?sort=profit&dir=asc"
        );
    }

    #[test]
    fn every_other_query_param_survives() {
        // The part each copy got subtly different. Losing `world` here drops
        // the visitor onto a different world's data mid-sort.
        let q = params(&[("world", "Gilgamesh"), ("dir", "asc"), ("sort", "profit")]);
        let href = sort_href("/t", q, Col::Cost, false, SortDir::Asc, &[]);
        assert!(href.contains("world=Gilgamesh"), "{href}");
        assert!(href.contains("sort=cost"), "{href}");
        assert!(!href.contains("dir="), "stale dir survived: {href}");
    }

    #[test]
    fn reset_keys_are_dropped() {
        // Re-sorting a paginated list has to return to page 1 — page 7 of a
        // different ordering is a different set of items entirely.
        let q = params(&[("page", "7"), ("per_page", "50")]);
        let href = sort_href("/t", q, Col::Cost, false, SortDir::Desc, &["page"]);
        assert!(!href.contains("page=7"), "{href}");
        // `per%5Fpage` is `ParamsMap`'s own escaping of the underscore, not a
        // mangled key — the browser decodes it back to `per_page`.
        assert!(href.contains("per%5Fpage=50"), "{href}");
    }

    #[test]
    fn sort_dir_round_trips_through_string() {
        assert_eq!("asc".parse::<SortDir>(), Ok(SortDir::Asc));
        assert_eq!("desc".parse::<SortDir>(), Ok(SortDir::Desc));
        assert_eq!(SortDir::Asc.to_string(), "asc");
        assert_eq!(SortDir::Desc.to_string(), "desc");
        assert!("sideways".parse::<SortDir>().is_err());
    }

    #[test]
    fn sort_and_truncate_orders_both_directions() {
        let mut rows = vec![3, 1, 2];
        sort_and_truncate(&mut rows, SortDir::Asc, 100, |a, b| a.cmp(b));
        assert_eq!(rows, vec![1, 2, 3]);
        sort_and_truncate(&mut rows, SortDir::Desc, 100, |a, b| a.cmp(b));
        assert_eq!(rows, vec![3, 2, 1]);
    }

    #[test]
    fn cmp_none_last_keeps_missing_values_last_in_both_directions() {
        let mut rows = vec![Some(2), None, Some(1), Some(3), None];
        rows.sort_by(|a, b| cmp_none_last(*a, *b, SortDir::Asc, Ord::cmp));
        assert_eq!(rows, vec![Some(1), Some(2), Some(3), None, None]);
        rows.sort_by(|a, b| cmp_none_last(*a, *b, SortDir::Desc, Ord::cmp));
        assert_eq!(rows, vec![Some(3), Some(2), Some(1), None, None]);
    }

    #[test]
    fn truncation_follows_the_sort_direction() {
        // The regression this helper exists to prevent: a truncation keyed to
        // a fixed descending metric would keep the LARGEST rows and then sort
        // them ascending — the cheapest rows, the ones the click asked for,
        // would never make the cut.
        let mut rows: Vec<i32> = (0..500).collect();
        sort_and_truncate(&mut rows, SortDir::Asc, 100, |a, b| a.cmp(b));
        assert_eq!(rows.len(), 100);
        assert_eq!(rows, (0..100).collect::<Vec<_>>());

        let mut rows: Vec<i32> = (0..500).collect();
        sort_and_truncate(&mut rows, SortDir::Desc, 100, |a, b| a.cmp(b));
        assert_eq!(rows.len(), 100);
        assert_eq!(rows, (400..500).rev().collect::<Vec<_>>());
    }

    #[test]
    fn header_cell_renders_badge_sub_label_and_emphasis() {
        // `TermBadge` builds an I18nContext (spawns an Effect) and `<Gil>`
        // reads it: stand up the executor and the context, as
        // components/list/filter_row.rs's tests do.
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let html = view! {
                <SortableHeaderCell
                    mode=Col::Cost
                    label="Cost"
                    class="w-40"
                    sort_mode=Signal::derive(|| None::<Col>)
                    sort_dir=Signal::derive(|| None::<SortDir>)
                    badge=crate::components::term_badge::TermRole::Cost
                    sub_label=Signal::derive(|| "listing · Aether".to_string())
                    emphasized=Signal::derive(|| true)
                />
            }
            .to_html();
            assert!(html.contains("listing · Aether"), "{html}");
            assert!(html.contains("aria-hidden=\"true\""), "{html}");
            assert!(html.contains("sr-only"), "{html}");
            assert!(
                html.contains("shadow-[inset_0_-2px_0_var(--brand-ring)]"),
                "{html}"
            );
            // Without the props the markup is what it was.
            let plain = view! {
                <SortableHeaderCell
                    mode=Col::Cost
                    label="Cost"
                    class="w-32"
                    sort_mode=Signal::derive(|| None::<Col>)
                    sort_dir=Signal::derive(|| None::<SortDir>)
                />
            }
            .to_html();
            assert!(!plain.contains("sr-only"), "{plain}");
            assert!(!plain.contains("min-w-0"), "{plain}");
            // The cell must add no hydration markers of its own beyond what
            // the bare `SortHeader` it wraps already emits — an `Option`
            // child that resolves to `None` (e.g. an unset `sub_label`)
            // still writes a `<!>` placeholder comment, which would make
            // every one of the 35 existing call sites' markup drift from
            // what it was before this component grew these props.
            let bare = view! {
                <SortHeader
                    mode=Col::Cost
                    label="Cost"
                    sort_mode=Signal::derive(|| None::<Col>)
                    sort_dir=Signal::derive(|| None::<SortDir>)
                />
            }
            .to_html();
            assert_eq!(
                plain.matches("<!>").count(),
                bare.matches("<!>").count(),
                "cell added its own hydration marker(s): {plain}"
            );
            assert_eq!(plain.matches("role=\"columnheader\"").count(), 1, "{plain}");
        });
    }

    /// The pill slot lives inside the sub-label line, after the text, as a
    /// separate focus stop; unset it adds no markup to a two-line header.
    #[test]
    fn trailing_renders_after_the_sub_label_and_adds_nothing_when_unset() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let with = view! {
                <SortableHeaderCell
                    mode=Col::Cost
                    label="Sale median (7d)"
                    class="w-40"
                    sort_mode=Signal::derive(|| None::<Col>)
                    sort_dir=Signal::derive(|| None::<SortDir>)
                    sub_label=Signal::derive(|| "7d median · Aether".to_string())
                    trailing=ViewFn::from(|| view! { <button type="button">"use"</button> }.into_any())
                />
            }
            .to_html();
            let sub_at = with.find("7d median · Aether").unwrap();
            let btn_at = with.find("<button").unwrap();
            assert!(sub_at < btn_at, "{with}");
            assert!(with.contains("flex items-center gap-1"), "{with}");
            let without = view! {
                <SortableHeaderCell
                    mode=Col::Cost
                    label="Sale median (7d)"
                    class="w-40"
                    sort_mode=Signal::derive(|| None::<Col>)
                    sort_dir=Signal::derive(|| None::<SortDir>)
                    sub_label=Signal::derive(|| "7d median · Aether".to_string())
                />
            }
            .to_html();
            assert!(!without.contains("<button"), "{without}");
            assert!(!without.contains("flex items-center gap-1"), "{without}");
            assert!(
                without.contains("text-[color:var(--color-text-muted)] truncate max-w-full\">7d median · Aether</div>"),
                "{without}"
            );
        });
    }

    /// Same defect class as GlitchTip #7278 in `QueryButton`: this header
    /// reads the location too, so it has to degrade rather than take the SSR
    /// stream down with it.
    #[test]
    fn renders_a_relative_href_without_router_context() {
        let owner = Owner::new();
        owner.with(|| {
            let html = view! {
                <SortHeader
                    mode=Col::Cost
                    label="Cost"
                    sort_mode=Signal::derive(|| None::<Col>)
                    sort_dir=Signal::derive(|| None::<SortDir>)
                />
            }
            .to_html();
            assert!(html.contains("href=\"?sort=cost\""), "{html}");
            assert!(html.contains("Cost"), "{html}");
        });
    }
}
