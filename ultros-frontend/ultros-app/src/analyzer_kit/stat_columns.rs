//! The window × statistic matrix behind the shared `market-*` sale-history
//! columns: the windows the server serves, the statistics each carries, the
//! literal column ids (a bookmark and saved-view contract), the labels, and
//! the picker descriptions built from them. Every analyzer that renders
//! `MarketGrid` exposes this metadata through the shared column registry.

use std::collections::HashSet;

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

/// One statistic read from `ItemListingStats::window`: listing history over
/// the page window, so labels carry the window suffix like the
/// follow-window sale columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ListingWindowKind {
    /// Lowest observed scope floor (cheapest alive listing) in the window.
    FloorMin,
    /// Highest observed scope floor in the window.
    FloorMax,
    /// Listings observed arriving; a remove-and-relist counts here too.
    Additions,
    /// Listings observed leaving, for any reason.
    Removals,
    /// Median seconds from a listing's last review to its matched sale.
    TimeToSell,
    /// Alive units over the window's average units sold per day.
    DaysOfStock,
    /// Same-listing price drops per day of scope-wide listing coverage.
    UndercutsPerDay,
    /// Median relative drop across those undercuts, shown as a percentage.
    UndercutMedian,
}

impl ListingWindowKind {
    /// Time to sell counts from sale receipts; every other statistic from
    /// listing events. Their coverage starts at different instants.
    pub const fn counts_receipts(self) -> bool {
        matches!(self, ListingWindowKind::TimeToSell)
    }
}

/// Follow-window ids only: one body per scope and selected window, never a
/// pinned `-N` variant, because every window body is a separate fetch.
pub static LISTING_WINDOW_COLUMNS: [(ListingWindowKind, &str); 8] = [
    (ListingWindowKind::FloorMin, "market-floor-min"),
    (ListingWindowKind::FloorMax, "market-floor-max"),
    (ListingWindowKind::Additions, "market-listings-added"),
    (ListingWindowKind::Removals, "market-listings-removed"),
    (ListingWindowKind::TimeToSell, "market-time-to-sell"),
    (ListingWindowKind::DaysOfStock, "market-days-of-stock"),
    (ListingWindowKind::UndercutsPerDay, "market-undercuts"),
    (ListingWindowKind::UndercutMedian, "market-undercut-pct"),
];

/// The pinned 30-day floor sparkline. It reads its own per-row feed
/// (`POST /api/v1/floor_history/{scope}`), not a listing-stats body, so it
/// carries its window in the id like the pinned sale-history columns.
pub const FLOOR_TREND_ID: &str = "market-floor-30";
pub const FLOOR_TREND_WINDOW: Window = Window::D30;

pub fn listing_window_id(kind: ListingWindowKind) -> &'static str {
    LISTING_WINDOW_COLUMNS
        .iter()
        .find(|(k, _)| *k == kind)
        .unwrap()
        .1
}

/// Whether any windowed listing column is in the grid's wanted set.
pub fn listing_window_wanted(needs: &HashSet<String>) -> bool {
    LISTING_WINDOW_COLUMNS
        .iter()
        .any(|(_, id)| needs.contains(*id))
}

pub fn listing_window_label(kind: ListingWindowKind, window: Window) -> String {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let name = match kind {
        ListingWindowKind::FloorMin => t_string!(i18n, market_floor_min),
        ListingWindowKind::FloorMax => t_string!(i18n, market_floor_max),
        ListingWindowKind::Additions => t_string!(i18n, market_listings_added),
        ListingWindowKind::Removals => t_string!(i18n, market_listings_removed),
        ListingWindowKind::TimeToSell => t_string!(i18n, market_time_to_sell),
        ListingWindowKind::DaysOfStock => t_string!(i18n, market_days_of_stock),
        ListingWindowKind::UndercutsPerDay => t_string!(i18n, market_undercuts),
        ListingWindowKind::UndercutMedian => t_string!(i18n, market_undercut_pct),
    }
    .to_string();
    with_window(name, window)
}

/// Hover text: what each statistic counts, and what it leaves out.
pub fn listing_window_title(kind: ListingWindowKind) -> String {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    match kind {
        ListingWindowKind::FloorMin => t_string!(i18n, market_floor_min_title),
        ListingWindowKind::FloorMax => t_string!(i18n, market_floor_max_title),
        ListingWindowKind::Additions => t_string!(i18n, market_listings_added_title),
        ListingWindowKind::Removals => t_string!(i18n, market_listings_removed_title),
        ListingWindowKind::TimeToSell => t_string!(i18n, market_time_to_sell_title),
        ListingWindowKind::DaysOfStock => t_string!(i18n, market_days_of_stock_title),
        ListingWindowKind::UndercutsPerDay => t_string!(i18n, market_undercuts_title),
        ListingWindowKind::UndercutMedian => t_string!(i18n, market_undercut_pct_title),
    }
    .to_string()
}

/// "Ultros has observed about 12 of these 30 days so far": appended to a
/// windowed column's hover text while the history is younger than the
/// window, so a 30-day label never overstates what the numbers cover.
pub fn history_observed_note(observed_days: u16, window: Window) -> String {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    t_string!(
        i18n,
        market_history_observed,
        days = observed_days.to_string(),
        window = window.days().to_string()
    )
    .to_string()
}

pub fn floor_trend_label() -> String {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    with_window(
        t_string!(i18n, market_floor_trend).to_string(),
        FLOOR_TREND_WINDOW,
    )
}

pub fn floor_trend_title() -> String {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    t_string!(i18n, market_floor_trend_title).to_string()
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

/// Distinguish selected-window and pinned columns even when both show 7d today.
/// This is presentation only: saved-view and URL column ids stay unchanged.
pub fn stat_picker_label(kind: StatKind, window: Window, follows_window: bool) -> String {
    let label = stat_label(kind, window);
    if follows_window {
        let i18n = crate::i18n_fallback::use_i18n_or_default();
        t_string!(i18n, analyzer_columns_follows_window, stat = label).to_string()
    } else {
        label
    }
}

pub fn stat_picker_hint(window: Window, follows_window: bool) -> String {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let days = window.days().to_string();
    if follows_window {
        t_string!(i18n, analyzer_columns_follows_window_hint, days = days).to_string()
    } else {
        t_string!(i18n, analyzer_columns_fixed_window_hint, days = days).to_string()
    }
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

/// The columns picker folds each statistic's windows into one row under
/// this heading ("Sale history"), named by the bare statistic.
pub fn market_picker_family_group() -> String {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    t_string!(i18n, market_picker_group_history).to_string()
}

pub fn stat_family(kind: StatKind) -> String {
    stat_name(kind)
}

/// A statistic's window as a picker pill: "7d", or "Selected" for the
/// column that follows the page's window.
pub fn stat_variant_label(window: Option<Window>) -> String {
    match window {
        Some(window) => window_label(window),
        None => {
            let i18n = crate::i18n_fallback::use_i18n_or_default();
            t_string!(i18n, analyzer_columns_window_selected).to_string()
        }
    }
}

/// The picker and filter-menu heading for the current-listing family.
pub fn market_picker_group_listings() -> String {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    t_string!(i18n, market_picker_group_listings).to_string()
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
        assert!(ListingKind::MedianAge.is_age() && ListingKind::OldestAge.is_age());
        assert!(!ListingKind::Alive.is_age());
    }

    #[test]
    fn listing_window_columns_are_follow_window_ids_wanted_together() {
        let ids: HashSet<_> = LISTING_WINDOW_COLUMNS.iter().map(|(_, id)| *id).collect();
        assert_eq!(ids.len(), LISTING_WINDOW_COLUMNS.len());
        assert!(
            !ids.contains(FLOOR_TREND_ID),
            "the sparkline has its own feed"
        );
        assert!(FLOOR_TREND_ID.ends_with(&format!("-{}", FLOOR_TREND_WINDOW.days())));
        assert!(!STAT_COLUMNS.iter().any(|c| c.id == FLOOR_TREND_ID));
        assert!(ListingWindowKind::TimeToSell.counts_receipts());
        assert!(!ListingWindowKind::FloorMin.counts_receipts());
        for (kind, id) in &LISTING_WINDOW_COLUMNS {
            assert!(!STAT_COLUMNS.iter().any(|c| c.id == *id), "{id} collides");
            assert!(
                !FOLLOW_COLUMNS.iter().any(|(_, f)| f == id),
                "{id} collides"
            );
            assert!(
                !LISTING_COLUMNS.iter().any(|(_, f)| f == id),
                "{id} collides"
            );
            for window in Window::ALL {
                assert!(!id.ends_with(&format!("-{}", window.days())), "{id}");
            }
            assert_eq!(listing_window_id(*kind), *id);
        }
        let needs: HashSet<String> = ["market-undercut-pct", "roi"].map(str::to_owned).into();
        assert!(listing_window_wanted(&needs));
        assert!(
            !listings_wanted(&needs),
            "history columns never want the alive set"
        );
        assert!(required_windows(&needs, Window::D7, false).is_empty());
        let none: HashSet<String> = ["market-alive"].map(str::to_owned).into();
        assert!(!listing_window_wanted(&none));
    }

    #[test]
    fn a_window_is_wanted_only_when_one_of_its_columns_is() {
        let needs: HashSet<String> = ["market-sale-median-30", "roi"].map(str::to_owned).into();
        assert!(window_wanted(&needs, Window::D30));
        assert!(!window_wanted(&needs, Window::D90));
        assert!(!window_wanted(&needs, Window::D1));
    }

    #[test]
    fn labels_preserve_legacy_text_and_explain_following_and_fixed_windows() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            assert_eq!(stat_label(StatKind::Median, Window::D7), "Sale median (7d)");
            assert_eq!(
                stat_label(StatKind::GilVolume, Window::D90),
                "Gil traded (90d)"
            );
            assert_eq!(listing_label(ListingKind::Alive), "Active listings");
            assert_eq!(market_picker_group_listings(), "Listings");
            assert_eq!(listing_title(ListingKind::Alive), None);
            assert_eq!(listing_label(ListingKind::OldestAge), "Oldest listing age");
            assert_eq!(
                listing_title(ListingKind::OldestAge).as_deref(),
                Some("Time since the retainer last touched the listing, not how long it has been for sale. An old listing may be an unrelated expensive one; it says nothing about how fresh the price data is.")
            );
            assert_eq!(
                listing_window_label(ListingWindowKind::UndercutsPerDay, Window::D7),
                "Undercuts/day (7d)"
            );
            assert_eq!(
                listing_window_title(ListingWindowKind::UndercutsPerDay),
                "Same-listing price drops per day across the scope, both edit-in-place and remove-and-relist. Raises and new listings are not counted."
            );
            assert_eq!(
                listing_window_label(ListingWindowKind::UndercutMedian, Window::D30),
                "Undercut % (30d)"
            );
            assert_eq!(
                listing_window_title(ListingWindowKind::UndercutMedian),
                "Median drop as a share of the previous price across those undercuts."
            );
            assert_eq!(
                listing_window_label(ListingWindowKind::FloorMin, Window::D30),
                "Lowest floor (30d)"
            );
            assert_eq!(
                listing_window_label(ListingWindowKind::TimeToSell, Window::D7),
                "Time to sell (7d)"
            );
            assert!(listing_window_title(ListingWindowKind::Additions).contains("relisting"));
            assert_eq!(floor_trend_label(), "Floor trend (30d)");
            assert!(floor_trend_title().contains("shorter line"));
            assert_eq!(
                history_observed_note(12, Window::D30),
                "Ultros has observed about 12 of these 30 days so far."
            );
            assert_eq!(
                market_picker_group(Some(Window::D7)),
                "Sale history (7d)"
            );
            assert_eq!(
                stat_picker_label(StatKind::Min, Window::D7, true),
                "Sale minimum (7d) · follows window"
            );
            assert_eq!(
                stat_picker_label(StatKind::Min, Window::D7, false),
                "Sale minimum (7d)"
            );
            assert_eq!(
                stat_picker_hint(Window::D7, true),
                "Sale history for this quality and price scope. Follows the selected history window (currently 7 days)."
            );
            assert_eq!(
                stat_picker_hint(Window::D7, false),
                "Sale history for this quality and price scope. Always uses the last 7 days, regardless of the selected history window."
            );
            assert_eq!(
                market_picker_group(None),
                "Sale history (selected window)"
            );
            assert_eq!(market_picker_family_group(), "Sale history");
            assert_eq!(stat_family(StatKind::SalesPerDay), "Sales/day");
            assert_eq!(stat_variant_label(Some(Window::D30)), "30d");
            assert_eq!(stat_variant_label(None), "Selected");
        });
    }
}
