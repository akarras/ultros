//! The window × statistic matrix behind the shared `market-*` sale-history
//! columns: the windows the server serves, the statistics each carries, the
//! literal column ids (a bookmark and saved-view contract), the labels, and
//! the Columns-picker options built from them. Every analyzer that renders
//! `MarketGrid` gets all of these; the Flip Finder also lists them in its
//! toolbar picker.

use std::collections::HashSet;

use crate::components::control_bar::{ColumnOption, PickerHeading};
use crate::i18n::*;

use super::needed::is_supported_window;

/// A trailing sale-history window `/api/v1/sale_stats` serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Window {
    D1,
    D7,
    D30,
    D90,
}

impl Window {
    pub const ALL: [Window; 4] = [Window::D1, Window::D7, Window::D30, Window::D90];

    pub const fn days(self) -> u16 {
        match self {
            Window::D1 => 1,
            Window::D7 => 7,
            Window::D30 => 30,
            Window::D90 => 90,
        }
    }

    /// Position in [`Window::ALL`]; indexes `MarketData`'s per-window slots.
    pub const fn index(self) -> usize {
        match self {
            Window::D1 => 0,
            Window::D7 => 1,
            Window::D30 => 2,
            Window::D90 => 3,
        }
    }
}

const _: () = {
    let mut i = 0;
    while i < Window::ALL.len() {
        assert!(
            is_supported_window(Window::ALL[i].days()),
            "a Window the server does not serve"
        );
        i += 1;
    }
};

/// One statistic read from an `ItemSaleStats` row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StatKind {
    Min,
    Median,
    Average,
    SalesPerDay,
    Cadence,
    Units,
    Sales,
    Vwap,
    GilVolume,
}

#[derive(Debug)]
pub struct StatColumn {
    pub kind: StatKind,
    pub window: Window,
    pub id: &'static str,
}

/// Ids are literals so `grep market-sale-median-7` finds the contract.
macro_rules! window_columns {
    ($($days:literal => $w:ident),* $(,)?) => {
        [$(
            StatColumn { kind: StatKind::Min,         window: Window::$w, id: concat!("market-sale-min-", $days) },
            StatColumn { kind: StatKind::Median,      window: Window::$w, id: concat!("market-sale-median-", $days) },
            StatColumn { kind: StatKind::Average,     window: Window::$w, id: concat!("market-sale-avg-", $days) },
            StatColumn { kind: StatKind::SalesPerDay, window: Window::$w, id: concat!("market-sales-per-day-", $days) },
            StatColumn { kind: StatKind::Cadence,     window: Window::$w, id: concat!("market-cadence-", $days) },
            StatColumn { kind: StatKind::Units,       window: Window::$w, id: concat!("market-units-", $days) },
            StatColumn { kind: StatKind::Sales,       window: Window::$w, id: concat!("market-sales-", $days) },
            StatColumn { kind: StatKind::Vwap,        window: Window::$w, id: concat!("market-vwap-", $days) },
            StatColumn { kind: StatKind::GilVolume,   window: Window::$w, id: concat!("market-gil-", $days) },
        )*]
    };
}

/// Window-major, kind order as declared: this is also the picker order.
pub static STAT_COLUMNS: [StatColumn; 36] = window_columns!(1 => D1, 7 => D7, 30 => D30, 90 => D90);

/// Follow-window IDs never acquire a numeric suffix; saved explicit IDs stay fixed.
pub static FOLLOW_COLUMNS: [(StatKind, &str); 9] = [
    (StatKind::Min, "market-sale-min"),
    (StatKind::Median, "market-sale-median"),
    (StatKind::Average, "market-sale-avg"),
    (StatKind::SalesPerDay, "market-sales-per-day"),
    (StatKind::Cadence, "market-cadence"),
    (StatKind::Units, "market-units"),
    (StatKind::Sales, "market-sales"),
    (StatKind::Vwap, "market-vwap"),
    (StatKind::GilVolume, "market-gil"),
];

pub fn follow_id(kind: StatKind) -> &'static str {
    FOLLOW_COLUMNS.iter().find(|(k, _)| *k == kind).unwrap().1
}

/// One statistic read from an `ItemListingStats` row: the board as it
/// stands now, independent of the page window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ListingKind {
    /// Listings whose last observed event is not a removal.
    Alive,
    /// Units across those listings.
    AliveUnits,
    /// Distinct retainers holding them.
    Sellers,
    /// Median seconds since an alive listing was last touched by its retainer.
    MedianAge,
    /// Seconds since the least recently touched alive listing was reviewed.
    OldestAge,
}

impl ListingKind {
    /// Age columns hold seconds since the retainer's last review, never
    /// since the listing was first posted.
    pub const fn is_age(self) -> bool {
        matches!(self, ListingKind::MedianAge | ListingKind::OldestAge)
    }
}

/// Window-independent ids: the alive set is one body per scope, so these
/// never acquire a numeric suffix.
pub static LISTING_COLUMNS: [(ListingKind, &str); 5] = [
    (ListingKind::Alive, "market-alive"),
    (ListingKind::AliveUnits, "market-alive-units"),
    (ListingKind::Sellers, "market-sellers"),
    (ListingKind::MedianAge, "market-listing-age"),
    (ListingKind::OldestAge, "market-oldest-listing"),
];

pub fn listing_id(kind: ListingKind) -> &'static str {
    LISTING_COLUMNS.iter().find(|(k, _)| *k == kind).unwrap().1
}

/// Whether any current-listing column is in the grid's wanted set.
pub fn listings_wanted(needs: &HashSet<String>) -> bool {
    LISTING_COLUMNS.iter().any(|(_, id)| needs.contains(*id))
}

pub fn listing_label(kind: ListingKind) -> String {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    match kind {
        ListingKind::Alive => t_string!(i18n, market_alive),
        ListingKind::AliveUnits => t_string!(i18n, market_alive_units),
        ListingKind::Sellers => t_string!(i18n, market_sellers),
        ListingKind::MedianAge => t_string!(i18n, market_listing_age),
        ListingKind::OldestAge => t_string!(i18n, market_oldest_listing),
    }
    .to_string()
}

/// Hover text for the age columns: review age is not ingestion freshness,
/// and an old listing may be an unrelated expensive one.
pub fn listing_title(kind: ListingKind) -> Option<String> {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    kind.is_age()
        .then(|| t_string!(i18n, market_listing_age_title).to_string())
}

pub fn shared_cols_in(raw: Option<&str>) -> HashSet<&'static str> {
    let selected: HashSet<_> = raw.unwrap_or("").split(',').collect();
    FOLLOW_COLUMNS
        .iter()
        .map(|(_, id)| *id)
        .chain(STAT_COLUMNS.iter().map(|c| c.id))
        .chain(LISTING_COLUMNS.iter().map(|(_, id)| *id))
        .filter(|id| selected.contains(id))
        .collect()
}

/// Toggle one picker entry while preserving columns owned by other providers.
pub fn toggle_shared_col(previous: Option<&str>, defaults: &str, id: &str) -> String {
    let mut ids: Vec<_> = previous
        .unwrap_or(defaults)
        .split(',')
        .filter(|t| !t.is_empty())
        .collect();
    if ids.contains(&id) {
        ids.retain(|t| *t != id);
    } else {
        ids.push(id);
    }
    ids.join(",")
}

pub fn window_label(window: Window) -> String {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    t_string!(i18n, market_window_days, days = window.days().to_string()).to_string()
}

pub fn stat_column(kind: StatKind, window: Window) -> &'static StatColumn {
    STAT_COLUMNS
        .iter()
        .find(|c| c.kind == kind && c.window == window)
        .expect("every (kind, window) pair is in STAT_COLUMNS")
}

fn stat_name(kind: StatKind) -> String {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    match kind {
        StatKind::Min => t_string!(i18n, market_stat_sale_min),
        StatKind::Median => t_string!(i18n, market_stat_sale_median),
        StatKind::Average => t_string!(i18n, market_stat_sale_avg),
        StatKind::SalesPerDay => t_string!(i18n, market_stat_sales_per_day),
        StatKind::Cadence => t_string!(i18n, market_stat_cadence),
        StatKind::Units => t_string!(i18n, market_stat_units),
        StatKind::Sales => t_string!(i18n, market_stat_sales),
        StatKind::Vwap => t_string!(i18n, market_stat_vwap),
        StatKind::GilVolume => t_string!(i18n, market_stat_gil),
    }
    .to_string()
}

/// "`{name}` (7d)" in the locale's own suffix form.
fn with_window(name: String, window: Window) -> String {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    t_string!(
        i18n,
        market_stat_window,
        stat = name,
        days = window.days().to_string()
    )
    .to_string()
}

pub fn stat_label(kind: StatKind, window: Window) -> String {
    with_window(stat_name(kind), window)
}

/// Whether any column of `window` is in the grid's wanted set (`?cols=`,
/// `?gf=` filters, the `?sort=grid:` target, visible defs).
pub fn window_wanted(needs: &HashSet<String>, window: Window) -> bool {
    STAT_COLUMNS
        .iter()
        .any(|c| c.window == window && needs.contains(c.id))
}

/// Deduplicated bulk bodies for visible columns, hidden query columns and prices.
pub fn required_windows(
    needs: &HashSet<String>,
    selected: Window,
    sale_basis: bool,
) -> Vec<Window> {
    Window::ALL
        .into_iter()
        .filter(|&window| {
            window_wanted(needs, window)
                || (window == selected
                    && (sale_basis || FOLLOW_COLUMNS.iter().any(|(_, id)| needs.contains(*id))))
                || (window == Window::D7
                    && (needs.contains("market-last-sold") || needs.contains("market-confidence")))
        })
        .collect()
}

/// Shared grouping for the toolbar and the grid's insert-column picker.
pub fn market_picker_group(window: Option<Window>) -> String {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    match window {
        Some(window) => with_window(
            t_string!(i18n, market_picker_group_history).to_string(),
            window,
        ),
        None => t_string!(i18n, market_picker_group_selected).to_string(),
    }
}

/// The picker and filter-menu heading for the current-listing family.
pub fn market_picker_group_listings() -> String {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    t_string!(i18n, market_picker_group_listings).to_string()
}

/// Every stat column as a toolbar-picker option, grouped under one
/// "Sale history (Nd)" heading per window, then the current-listing
/// columns under "Listings".
pub fn market_picker_options(window: Window) -> Vec<ColumnOption> {
    FOLLOW_COLUMNS
        .iter()
        .map(|(kind, id)| ColumnOption {
            id,
            label: stat_label(*kind, window),
            group: Some(PickerHeading {
                label: market_picker_group(None),
                title: None,
            }),
            disabled: false,
            hint: None,
        })
        .chain(STAT_COLUMNS.iter().map(|c| ColumnOption {
            id: c.id,
            label: stat_label(c.kind, c.window),
            group: Some(PickerHeading {
                label: market_picker_group(Some(c.window)),
                title: None,
            }),
            disabled: false,
            hint: None,
        }))
        .chain(LISTING_COLUMNS.iter().map(|(kind, id)| ColumnOption {
            id,
            label: listing_label(*kind),
            group: Some(PickerHeading {
                label: market_picker_group_listings(),
                title: None,
            }),
            disabled: false,
            hint: listing_title(*kind),
        }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use leptos::prelude::*;
    use leptos_i18n::context::init_i18n_context;

    #[test]
    fn follow_window_and_hidden_fixed_requirements_are_deduplicated() {
        // The same set comes from visible defs, hidden sort and hidden filters.
        let needs = [
            "market-sale-median",
            "market-units",
            "market-sale-min-7",
            "market-sale-median-30",
        ]
        .map(str::to_owned)
        .into();
        assert_eq!(
            required_windows(&needs, Window::D30, true),
            vec![Window::D7, Window::D30]
        );
        assert_eq!(
            required_windows(&HashSet::new(), Window::D30, true),
            vec![Window::D30]
        );
        assert!(required_windows(&HashSet::new(), Window::D30, false).is_empty());
        let hidden = ["market-sale-median-90"].map(str::to_owned).into();
        assert_eq!(
            required_windows(&hidden, Window::D30, false),
            vec![Window::D90]
        );
    }

    #[test]
    fn shared_picker_keeps_follow_fixed_and_foreign_columns() {
        let original = "profit,market-sale-median,market-sale-median-7,market-world";
        let selected = shared_cols_in(Some(original));
        assert_eq!(
            selected,
            HashSet::from(["market-sale-median", "market-sale-median-7"])
        );
        let toggled = toggle_shared_col(Some(original), "", "market-sale-median");
        assert_eq!(toggled, "profit,market-sale-median-7,market-world");
        assert_eq!(
            toggle_shared_col(None, "sale_estimate", "market-sale-median"),
            "sale_estimate,market-sale-median"
        );
    }

    #[test]
    fn legacy_ids_are_preserved_and_all_ids_are_unique() {
        for id in [
            "market-sale-min-7",
            "market-sale-median-7",
            "market-sale-avg-7",
            "market-sales-per-day-7",
            "market-cadence-7",
            "market-units-7",
            "market-sales-7",
            "market-vwap-7",
            "market-units-30",
            "market-sales-30",
            "market-vwap-30",
        ] {
            assert!(STAT_COLUMNS.iter().any(|c| c.id == id), "lost {id}");
        }
        let ids: HashSet<_> = STAT_COLUMNS.iter().map(|c| c.id).collect();
        assert_eq!(ids.len(), STAT_COLUMNS.len());
        assert_eq!(STAT_COLUMNS.len(), 9 * Window::ALL.len());
    }

    #[test]
    fn every_id_ends_with_its_window() {
        for c in &STAT_COLUMNS {
            assert!(c.id.ends_with(&format!("-{}", c.window.days())), "{}", c.id);
            assert!(std::ptr::eq(stat_column(c.kind, c.window), c));
        }
    }

    #[test]
    fn listing_columns_are_window_free_unique_and_wanted_together() {
        let ids: HashSet<_> = LISTING_COLUMNS.iter().map(|(_, id)| *id).collect();
        assert_eq!(ids.len(), LISTING_COLUMNS.len());
        assert_eq!(LISTING_COLUMNS.len(), 5);
        for (kind, id) in &LISTING_COLUMNS {
            assert_eq!(listing_id(*kind), *id);
            assert!(!STAT_COLUMNS.iter().any(|c| c.id == *id), "{id} collides");
            assert!(
                !FOLLOW_COLUMNS.iter().any(|(_, f)| f == id),
                "{id} collides"
            );
            for window in Window::ALL {
                assert!(!id.ends_with(&format!("-{}", window.days())), "{id}");
            }
        }
        let needs: HashSet<String> = ["market-sellers", "roi"].map(str::to_owned).into();
        assert!(listings_wanted(&needs));
        assert!(required_windows(&needs, Window::D7, false).is_empty());
        let none: HashSet<String> = ["market-sale-median-7"].map(str::to_owned).into();
        assert!(!listings_wanted(&none));
        assert_eq!(
            shared_cols_in(Some("profit,market-alive,market-oldest-listing")),
            HashSet::from(["market-alive", "market-oldest-listing"])
        );
        assert!(ListingKind::MedianAge.is_age() && ListingKind::OldestAge.is_age());
        assert!(!ListingKind::Alive.is_age());
    }

    #[test]
    fn a_window_is_wanted_only_when_one_of_its_columns_is() {
        let needs: HashSet<String> = ["market-sale-median-30", "roi"].map(str::to_owned).into();
        assert!(window_wanted(&needs, Window::D30));
        assert!(!window_wanted(&needs, Window::D90));
        assert!(!window_wanted(&needs, Window::D1));
    }

    #[test]
    fn labels_and_picker_match_the_legacy_seven_day_text() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            assert_eq!(stat_label(StatKind::Median, Window::D7), "Sale median (7d)");
            assert_eq!(
                stat_label(StatKind::GilVolume, Window::D90),
                "Gil traded (90d)"
            );
            let options = market_picker_options(Window::D7);
            assert_eq!(
                options.len(),
                STAT_COLUMNS.len() + FOLLOW_COLUMNS.len() + LISTING_COLUMNS.len()
            );
            let alive = options.iter().find(|o| o.id == "market-alive").unwrap();
            assert_eq!(alive.label, "Active listings");
            assert_eq!(
                alive.group.as_ref().map(|g| g.label.as_str()),
                Some("Listings")
            );
            assert_eq!(alive.hint, None);
            let oldest = options
                .iter()
                .find(|o| o.id == "market-oldest-listing")
                .unwrap();
            assert_eq!(oldest.label, "Oldest listing age");
            assert_eq!(
                oldest.hint.as_deref(),
                Some("Time since the retainer last touched the listing, not how long it has been for sale. An old listing may be an unrelated expensive one; it says nothing about how fresh the price data is.")
            );
            assert_eq!(options.last().unwrap().id, "market-oldest-listing");
            let median = options
                .iter()
                .find(|o| o.id == "market-sale-median-7")
                .unwrap();
            assert_eq!(median.label, "Sale median (7d)");
            assert_eq!(
                median.group.as_ref().map(|g| g.label.as_str()),
                Some("Sale history (7d)")
            );
            assert_eq!(options[0].id, "market-sale-min");
            assert_eq!(
                options[0].group.as_ref().unwrap().label,
                "Sale history (selected window)"
            );
        });
    }
}
