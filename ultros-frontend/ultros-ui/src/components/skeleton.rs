//! Loading placeholders.
//!
//! Every skeleton here is built from two CSS classes defined in
//! `style/tailwind.css`: `.skeleton-block` paints a placeholder surface and
//! `.skeleton-shimmer` sweeps a highlight across whatever it is put on, left
//! to right. The multi-row skeletons put the shimmer on their *container*, so
//! one highlight travels across the whole block instead of every bar
//! animating on its own.
//!
//! These render inside `<Suspense>`/`<Transition>` fallbacks, which means they
//! are part of the SSR response and are hydrated. Their markup must therefore
//! be a pure function of their props — no randomness, no clock. Where a
//! skeleton wants bars of varying width (a column of identical bars reads as a
//! bar chart, not as text) the width is picked from a fixed table by row and
//! column index, so the server and the client always agree.

use leptos::prelude::*;

use crate::i18n::*;
use crate::i18n_fallback::use_i18n_or_default;

/// Bar widths cycled through to keep placeholder text from looking like a
/// perfectly ragged-right block. Chosen by position, never at random — see
/// the module docs.
///
/// These are fractions of the *column*, so they suit the narrow, mostly-full
/// columns that hold a number or a short label.
const BAR_WIDTHS: &[&str] = &["w-3/5", "w-4/5", "w-1/2", "w-11/12", "w-2/3", "w-3/4"];

/// Widths for the name bar in an [`SkeletonCell::IconText`] cell.
///
/// A separate, much narrower set: the item column is the table's flexible one,
/// so it is several times wider than the name that sits in it. Reusing
/// [`BAR_WIDTHS`] there drew a bar three times the length of a real item name
/// and made the loading state read as a much denser table than the one that
/// replaced it.
const NAME_WIDTHS: &[&str] = &["w-1/3", "w-5/12", "w-1/4", "w-2/5", "w-1/2", "w-1/6"];

/// The width for the bar at `(row, column)`, picked out of `widths`.
/// Multiplying the two indices by strides coprime with the table length keeps
/// neighbouring rows and columns from landing on the same width.
fn width_at(widths: &[&'static str], row: usize, column: usize) -> &'static str {
    widths[(row * 5 + column * 3) % widths.len()]
}

fn bar_width(row: usize, column: usize) -> &'static str {
    width_at(BAR_WIDTHS, row, column)
}

fn name_width(row: usize, column: usize) -> &'static str {
    width_at(NAME_WIDTHS, row, column)
}

/// The silhouette drawn inside one skeleton cell. These are rough shapes, not
/// pixel copies — enough that the loading state has the same visual rhythm as
/// the table that replaces it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkeletonCell {
    /// A square icon followed by a text bar. The item column.
    IconText,
    /// A left-aligned text bar.
    Text,
    /// A shorter bar, laid out by the column's own alignment classes. Gil
    /// amounts, counts, percentages.
    Number,
    /// A small pill. Confidence bands, sales-cadence badges.
    Badge,
    /// A wide, short bar standing in for a sparkline.
    Spark,
    /// Nothing at all. Columns that are blank on most rows (an HQ flag) look
    /// wrong when the skeleton fills every one of them in.
    Blank,
}

/// One column of a [`TableSkeleton`].
///
/// `class` should be copied from the real table's cell so the skeleton's
/// columns land on the same x positions as the columns they stand in for —
/// including the responsive `hidden md:flex` visibility, or the skeleton will
/// show columns the table itself hides at that width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkeletonColumn {
    pub class: &'static str,
    pub cell: SkeletonCell,
}

impl SkeletonColumn {
    pub const fn new(class: &'static str, cell: SkeletonCell) -> Self {
        Self { class, cell }
    }
}

fn cell_view(cell: SkeletonCell, row: usize, column: usize) -> AnyView {
    match cell {
        SkeletonCell::IconText => view! {
            <div class="flex flex-row items-center gap-2 min-w-0 w-full">
                <div class="skeleton-block size-6 rounded shrink-0"></div>
                <div class=format!("skeleton-block h-2.5 rounded {}", name_width(row, column))></div>
            </div>
        }
        .into_any(),
        SkeletonCell::Text => view! {
            <div class=format!("skeleton-block h-2.5 rounded {}", bar_width(row, column))></div>
        }
        .into_any(),
        SkeletonCell::Number => view! {
            <div class=format!("skeleton-block h-2.5 rounded {}", bar_width(row, column + 1))></div>
        }
        .into_any(),
        SkeletonCell::Badge => {
            view! { <div class="skeleton-block h-4 w-12 rounded-full"></div> }.into_any()
        }
        SkeletonCell::Spark => {
            view! { <div class="skeleton-block h-4 w-full rounded"></div> }.into_any()
        }
        SkeletonCell::Blank => ().into_any(),
    }
}

/// A skeleton shaped like a data table: a header strip over a run of striped
/// rows, with one placeholder per column.
///
/// The caller supplies the columns, so the skeleton can be made to match a
/// specific table rather than approximating all of them. `class`, `row_class`
/// and `style` exist for the same reason — a table whose column widths come
/// from CSS variables on its container (the Flip Finder grid) can hand those
/// straight through and get a skeleton whose columns line up exactly with the
/// real ones.
#[component]
pub fn TableSkeleton(
    /// Columns in DOM order.
    columns: Vec<SkeletonColumn>,
    /// How many placeholder rows to draw.
    #[prop(default = 12)]
    rows: usize,
    /// Extra classes for the container.
    #[prop(optional, into)]
    class: String,
    /// Inline style for the container, for tables driven by CSS variables.
    #[prop(optional, into)]
    style: String,
    /// Extra classes for every row, header included. Carries whatever layout
    /// the real rows depend on — a min-width, a bottom border.
    #[prop(optional, into)]
    row_class: String,
    /// Height class for body rows. Matching the real row height is what keeps
    /// the page from jumping when the data arrives.
    #[prop(default = "h-10")]
    row_height: &'static str,
    /// Height class for the header strip.
    #[prop(default = "h-14")]
    header_height: &'static str,
    /// Draw the header strip. Off for tables whose header lives outside the
    /// area the fallback replaces.
    #[prop(default = true)]
    header: bool,
    /// Alternate the row tint. Off for tables that separate rows with a
    /// border instead.
    #[prop(default = true)]
    striped: bool,
) -> impl IntoView {
    let i18n = use_i18n_or_default();
    let header_row = header.then(|| {
        let cells = columns
            .iter()
            .map(|column| {
                view! {
                    <div class=column.class>
                        <div class="skeleton-block h-2 w-2/3 max-w-[4rem] rounded"></div>
                    </div>
                }
            })
            .collect::<Vec<_>>();
        view! {
            <div class=format!(
                "flex flex-row items-center flex-nowrap {header_height} border-b border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_8%,transparent)] {row_class}",
            )>{cells}</div>
        }
    });
    let body = (0..rows)
        .map(|row| {
            // Two-tone striping matching the tables this stands in for, so the
            // swap to real rows doesn't shift the page's texture.
            let stripe = match (striped, row % 2 == 0) {
                (false, _) => "",
                (true, true) => "bg-[color:color-mix(in_srgb,var(--color-text)_6%,transparent)]",
                (true, false) => "bg-[color:color-mix(in_srgb,var(--color-text)_8%,transparent)]",
            };
            let cells = columns
                .iter()
                .enumerate()
                .map(|(index, column)| {
                    view! { <div class=column.class>{cell_view(column.cell, row, index)}</div> }
                })
                .collect::<Vec<_>>();
            view! {
                <div class=format!(
                    "flex flex-row items-center flex-nowrap {row_height} {stripe} {row_class}",
                )>{cells}</div>
            }
        })
        .collect::<Vec<_>>();
    view! {
        <div class="w-full" role="status">
            <div class=format!("skeleton-shimmer {class}") style=style aria-hidden="true">
                {header_row}
                {body}
            </div>
            <div class="sr-only">{t!(i18n, loading)}</div>
        </div>
    }
    .into_any()
}

/// A single placeholder bar, for standing in for one value inside an
/// otherwise-rendered row.
///
/// Deliberately *not* a `role="status"` live region, unlike the block-level
/// skeletons. This one renders per table cell — the Flip Finder puts up to
/// three of them in every row of a list that is hundreds long — and a live
/// region each would give a screen reader hundreds of things to announce.
/// The `sr-only` label still describes the cell to anyone reading through it.
#[component]
pub fn SingleLineSkeleton() -> impl IntoView {
    let i18n = use_i18n_or_default();
    view! {
        <div class="w-full">
            <div class="skeleton-block skeleton-shimmer w-full h-3 rounded-md" aria-hidden="true"></div>
            <div class="sr-only">{t!(i18n, loading)}</div>
        </div>
    }
    .into_any()
}

/// A run of avatar-and-two-lines rows. The generic fallback for list-shaped
/// content that isn't a table.
#[component]
pub fn BoxSkeleton(
    /// How many placeholder rows to draw.
    #[prop(default = 6)]
    rows: usize,
) -> impl IntoView {
    let i18n = use_i18n_or_default();
    let rows = (0..rows)
        .map(|row| {
            view! {
                <div class="flex items-center gap-4 p-3 rounded-lg panel">
                    <div class="skeleton-block size-10 rounded-md"></div>
                    <div class="flex-1 space-y-2">
                        <div class=format!(
                            "skeleton-block h-3 rounded-md {}",
                            bar_width(row, 0),
                        )></div>
                        <div class=format!(
                            "skeleton-block h-3 rounded-md {}",
                            bar_width(row, 3),
                        )></div>
                    </div>
                </div>
            }
        })
        .collect::<Vec<_>>();
    view! {
        <div class="w-full h-full" role="status">
            <div class="skeleton-shimmer space-y-2 rounded-lg" aria-hidden="true">
                {rows}
            </div>
            <div class="sr-only">{t!(i18n, loading)}</div>
        </div>
    }
    .into_any()
}

/// A short inline placeholder standing in for a status line next to a
/// control — e.g. "Loading recent sales…" beside a world picker.
///
/// Unlike [`SingleLineSkeleton`] this does not stretch to fill its
/// container: the analyzer pages that use it sit inside a `justify-end` row,
/// where a full-width bar would blow out the layout the real text sits in.
#[component]
pub fn InlineStatusSkeleton() -> impl IntoView {
    let i18n = use_i18n_or_default();
    view! {
        <div class="inline-flex items-center" role="status">
            <div class="skeleton-block skeleton-shimmer h-3 w-28 rounded" aria-hidden="true"></div>
            <span class="sr-only">{t!(i18n, loading)}</span>
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod test {
    use super::*;

    /// Skeletons are rendered on the server and then hydrated, so the same
    /// `(row, column)` has to produce the same width in both passes. Anything
    /// derived from a clock or an RNG would diverge and take the whole page
    /// down with a hydration panic.
    #[test]
    fn bar_width_is_a_pure_function_of_position() {
        for row in 0..32 {
            for column in 0..12 {
                assert_eq!(bar_width(row, column), bar_width(row, column));
            }
        }
    }

    /// The widths are indexed modulo the table's length; every index must land
    /// inside it for any row/column a caller can reach.
    #[test]
    fn width_never_panics_on_large_indices() {
        for row in [0usize, 1, 7, 100, 10_000] {
            for column in [0usize, 1, 5, 64, 1_000] {
                assert!(BAR_WIDTHS.contains(&bar_width(row, column)));
                assert!(NAME_WIDTHS.contains(&name_width(row, column)));
            }
        }
    }

    /// Neighbouring cells in a row shouldn't share a width, or the "ragged
    /// text" effect collapses back into a block. Both tables are the same
    /// length, so the stride argument holds for either.
    #[test]
    fn adjacent_columns_differ() {
        for widths in [BAR_WIDTHS, NAME_WIDTHS] {
            for row in 0..12 {
                for column in 0..8 {
                    assert_ne!(
                        width_at(widths, row, column),
                        width_at(widths, row, column + 1),
                        "row {row}, columns {column} and {}",
                        column + 1
                    );
                }
            }
        }
    }

    /// The item column is the table's flexible one, several times wider than
    /// the name inside it, so its bars have to stay well under a full column.
    /// A regression here is what made the first cut of this skeleton read as a
    /// denser table than the real one.
    #[test]
    fn name_widths_are_narrower_than_bar_widths() {
        fn fraction(class: &str) -> f64 {
            let (num, den) = class
                .trim_start_matches("w-")
                .split_once('/')
                .expect("width classes are fractions");
            num.parse::<f64>().unwrap() / den.parse::<f64>().unwrap()
        }
        let widest = NAME_WIDTHS
            .iter()
            .copied()
            .map(fraction)
            .fold(0.0, f64::max);
        assert!(
            widest <= 0.5,
            "widest name bar is {widest} of the column, which is too close to full"
        );
        let widest_bar = BAR_WIDTHS.iter().copied().map(fraction).fold(0.0, f64::max);
        assert!(widest < widest_bar);
    }
}
