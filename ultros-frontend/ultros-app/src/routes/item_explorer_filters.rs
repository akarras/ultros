//! The Item Explorer's row model, its legacy filter contract and column
//! availability.
//!
//! Kept out of [`item_explorer`](super::item_explorer) — and free of every
//! reactive and context read — so all of it can be unit-tested against the
//! shipped game-data pack. The page owns the URL plumbing and the grid; this
//! module owns "what does a row carry?", "which legacy `?` key means which
//! grid filter?" and "does this column have anything to show?".
//!
//! # Why availability is computed from the whole set
//!
//! Issue #1296: not one item in a category like Minions has an equip level, an
//! HQ variant, or a real item level, so three of the table's columns render
//! 261 dashes, blanks and identical ones. [`column_availability`] scans the
//! set once and the page leaves those columns out of the grid.
//!
//! It has to be a property of the **set**, not of the rows currently on
//! screen: a column that appears and disappears as you filter would reshuffle
//! the grid under the reader. The page therefore passes the full, *unfiltered*
//! item list.

use ultros_api_types::cheapest_listings::CheapestListingsMap;
use xiv_gen::Item;

use crate::analyzer_kit::market::MarketSubject;
use crate::components::virtual_grid::metrics::{FilterOp, GridValue};
use crate::components::virtual_grid::registry::FilterAlias;

/// Grid column and metric ids. `?sort=` tokens and `?cols=` ids are the same
/// strings (see `ItemSortOption` on the page), so a bookmark that names a
/// column names it once.
pub const COL_ITEM: &str = "item";
pub const COL_ITEM_LEVEL: &str = "ilvl";
pub const COL_EQUIP_LEVEL: &str = "lv";
pub const COL_NQ: &str = "price";
pub const COL_HQ: &str = "hq";
pub const COL_VENDOR: &str = "vendor";
pub const COL_WORLD: &str = "world";
pub const COL_KEY: &str = "key";
pub const COL_ACTIONS: &str = "actions";
/// The shared cheapest-listing column, overridden by the page so it can be
/// `Pending` before prices load (see [`ExplorerRow::listing_value`]).
pub const COL_LISTING: &str = "market-listing";

/// Legacy `?` keys, kept verbatim: every one of them is a bookmark contract
/// from #1316. They resolve into shared grid filters through
/// [`explorer_filter_aliases`], except `hq-only`, which is a route-level
/// control the page applies itself.
pub const FILTER_NAME: &str = "q";
pub const FILTER_MIN_ILVL: &str = "min-ilvl";
pub const FILTER_MAX_ILVL: &str = "max-ilvl";
pub const FILTER_MIN_LV: &str = "min-lv";
pub const FILTER_MAX_PRICE: &str = "max-price";
pub const FILTER_VENDOR: &str = "vendor-only";
pub const FILTER_HQ: &str = "hq-only";
pub const FILTER_LISTED: &str = "listed";

/// The cheapest listing across both qualities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheapestListing {
    pub price: i32,
    pub hq: bool,
    pub world_id: i32,
}

/// One row of the explorer's grid.
///
/// Prices are folded into the row rather than read from the listings
/// resource in each cell, so the shared grid's metrics, the market subject
/// and the page's own sort all see the same numbers, and so `prices_loaded`
/// can hold the hydration gate: the server and the first client render both
/// build rows with `prices_loaded == false`, and every price-backed value is
/// then [`GridValue::Pending`] — a filter keeps the row, a `grid:` sort
/// reports pending — so the two sides agree on the row set and its order.
#[derive(Clone, Debug, PartialEq)]
pub struct ExplorerRow {
    pub item_id: i32,
    pub item: &'static Item,
    pub nq: Option<i32>,
    pub hq: Option<i32>,
    pub cheapest: Option<CheapestListing>,
    pub vendor: Option<u32>,
    pub prices_loaded: bool,
}

impl ExplorerRow {
    /// `prices` is `None` until the page's hydration gate flips.
    pub fn build(
        item_id: i32,
        item: &'static Item,
        prices: Option<&CheapestListingsMap>,
        vendor: Option<u32>,
    ) -> Self {
        let summary = prices.map(|map| map.find_matching_listings(item_id));
        let cheapest = summary
            .as_ref()
            .and_then(|summary| summary.chosen(false))
            .map(|listing| {
                // Equal price/world payloads do not identify a quality.
                // Mirror chosen(false): NQ wins a price tie.
                let hq = summary.as_ref().is_some_and(|s| {
                    s.hq.is_some_and(|hq| s.lq.is_none_or(|nq| hq.price < nq.price))
                });
                CheapestListing {
                    price: listing.price,
                    hq,
                    world_id: listing.world_id,
                }
            });
        Self {
            item_id,
            item,
            nq: summary.as_ref().and_then(|s| s.lq).map(|l| l.price),
            hq: summary.as_ref().and_then(|s| s.hq).map(|l| l.price),
            cheapest,
            vendor,
            prices_loaded: prices.is_some(),
        }
    }

    /// Shared market statistics describe **the cheapest listed quality** in
    /// the pricing scope, at that listing's world. With no listing (or before
    /// prices load) the subject is NQ on world 0 with no listing price: world
    /// 0 is what `MarketGrid` already reads as "no listing", so the location
    /// columns render "—" and no history is requested for it. Statistics then
    /// describe the NQ history, the quality every item has.
    pub fn market_subject(&self) -> MarketSubject {
        let mut subject = MarketSubject::new(
            self.item_id,
            self.cheapest.is_some_and(|c| c.hq),
            self.cheapest.map_or(0, |c| c.world_id),
        );
        subject.label = self.item.name.clone();
        subject.listing_price = self.cheapest.map(|c| c.price);
        subject
    }

    /// The NQ or HQ price as a grid value: `Pending` before the gate flips,
    /// `Missing` for a loaded map with nothing listed.
    pub fn price_value(&self, quality_hq: bool) -> GridValue {
        self.loaded(if quality_hq { self.hq } else { self.nq })
    }

    /// The cheapest listing of either quality — what the legacy `max-price`
    /// and `listed` filters compared against.
    pub fn listing_value(&self) -> GridValue {
        self.loaded(self.cheapest.map(|c| c.price))
    }

    fn loaded(&self, price: Option<i32>) -> GridValue {
        if !self.prices_loaded {
            return GridValue::Pending;
        }
        price.map_or(GridValue::Missing, |p| GridValue::Number(f64::from(p)))
    }
}

/// A boolean legacy key: only the literal `true` the old chips wrote turns
/// the filter on. Anything else reads as unset, as it did before.
fn true_only(raw: &str) -> Option<String> {
    (raw.trim() == "true").then(|| "true".to_string())
}

/// The legacy filter keys as shared grid filters.
///
/// `resolve_filters` merges a `Gte` and an `Lte` alias on the same column
/// into one inclusive `Between`, which is how `min-ilvl` + `max-ilvl` stay
/// simultaneous. It keeps only the *first* alias for any other pair, so
/// `max-price` is listed before `listed`: an upper bound on the cheapest
/// listing already excludes rows with no listing, which is exactly what the
/// old chips did when both were set.
pub fn explorer_filter_aliases() -> Vec<FilterAlias> {
    vec![
        FilterAlias::new(FILTER_NAME, COL_ITEM, FilterOp::Contains),
        FilterAlias::integer(FILTER_MIN_ILVL, COL_ITEM_LEVEL, FilterOp::Gte),
        FilterAlias::integer(FILTER_MAX_ILVL, COL_ITEM_LEVEL, FilterOp::Lte),
        FilterAlias::integer(FILTER_MIN_LV, COL_EQUIP_LEVEL, FilterOp::Gte),
        FilterAlias::integer(FILTER_MAX_PRICE, COL_LISTING, FilterOp::Lte),
        FilterAlias {
            convert: true_only,
            ..FilterAlias::new(FILTER_VENDOR, COL_VENDOR, FilterOp::Present)
        },
        FilterAlias {
            convert: true_only,
            ..FilterAlias::new(FILTER_LISTED, COL_LISTING, FilterOp::Present)
        },
    ]
}

/// Which of the explorer's optional columns any item in the current set
/// actually carries data for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColumnAvailability {
    pub item_level: bool,
    pub equip_level: bool,
    pub hq: bool,
    pub vendor: bool,
}

impl ColumnAvailability {
    /// Availability for the column ids the picker and the page share.
    /// Unknown ids (the always-present columns) report `true`.
    pub fn has(&self, column: &str) -> bool {
        match column {
            COL_ITEM_LEVEL => self.item_level,
            COL_EQUIP_LEVEL => self.equip_level,
            COL_HQ => self.hq,
            COL_VENDOR => self.vendor,
            _ => true,
        }
    }
}

/// `level_equip` is 1 for everything that cannot be equipped at all, so the
/// column only means something once some item is above it. Same rule the
/// per-row cell already used to decide between the number and an em dash.
pub fn has_equip_level(item: &Item) -> bool {
    item.level_equip > 1
}

/// `level_item` reads the same way: 1 is the sheet's "no item level", not an
/// item level of one. Every minion in the pack carries `level_item == 1`, so a
/// `> 0` test would keep a column of 261 identical ones — the noise #1296 is
/// about.
pub fn has_item_level(item: &Item) -> bool {
    item.level_item > 1
}

/// Scan a whole item set for what its optional columns can show.
///
/// `vendor_price` is injected rather than called directly so the tests can run
/// without the app's `tracked_data()` context; the page passes
/// `related_items::get_vendor_price`.
pub fn column_availability<'a>(
    items: impl IntoIterator<Item = &'a Item>,
    vendor_price: impl Fn(i32) -> Option<u32>,
) -> ColumnAvailability {
    let mut availability = ColumnAvailability::default();
    for item in items {
        availability.item_level |= has_item_level(item);
        availability.equip_level |= has_equip_level(item);
        availability.hq |= item.can_be_hq;
        availability.vendor |= vendor_price(item.key_id.0).is_some();
        if availability
            == (ColumnAvailability {
                item_level: true,
                equip_level: true,
                hq: true,
                vendor: true,
            })
        {
            break;
        }
    }
    availability
}

/// The one legacy filter that is not a grid metric: "the item *can* be HQ"
/// is a property of the item sheet, not of any column, so the page applies
/// it before the rows reach the grid.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExplorerFilters {
    /// Only items with an HQ variant.
    pub hq_only: bool,
}

impl ExplorerFilters {
    pub fn matches(&self, item: &Item) -> bool {
        !self.hq_only || item.can_be_hq
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::virtual_grid::registry::resolve_filters;
    use leptos_router::params::ParamsMap;
    use ultros_api_types::cheapest_listings::{CheapestListingData, CheapestListingMapKey};
    use xiv_gen::{ItemSearchCategoryId, Language};

    /// The category the issue links to (`/items/category/75`): 261 minions,
    /// none equippable, none HQ, every one of them `level_item == 1`.
    const MINIONS: i32 = 75;
    /// A gear category, for the other side of every availability assertion.
    const GLADIATORS_ARMS: i32 = 10;

    fn data() -> &'static xiv_gen::Data {
        xiv_gen_db::data_for(Language::En)
    }

    fn category_items(category: i32) -> Vec<&'static Item> {
        data()
            .items
            .values()
            .filter(|item| item.item_search_category == category)
            .collect()
    }

    fn first(category: i32) -> &'static Item {
        category_items(category)
            .into_iter()
            .min_by_key(|item| item.key_id.0)
            .unwrap_or_else(|| panic!("category {category} is empty in the shipped English pack"))
    }

    fn listings(item_id: i32, nq: Option<i32>, hq: Option<i32>) -> CheapestListingsMap {
        let mut map = CheapestListingsMap {
            map: Default::default(),
        };
        if let Some(price) = nq {
            map.map.insert(
                CheapestListingMapKey { item_id, hq: false },
                CheapestListingData { price, world_id: 7 },
            );
        }
        if let Some(price) = hq {
            map.map.insert(
                CheapestListingMapKey { item_id, hq: true },
                CheapestListingData { price, world_id: 9 },
            );
        }
        map
    }

    /// Pins the two category ids the tests key on, so a game-data bump that
    /// renumbers them fails here rather than quietly pointing every
    /// availability assertion at some other category.
    #[test]
    fn the_fixture_categories_are_the_ones_named() {
        for (id, name) in [(MINIONS, "Minions"), (GLADIATORS_ARMS, "Gladiator's Arms")] {
            assert_eq!(
                data()
                    .item_search_categorys
                    .get(&ItemSearchCategoryId(id))
                    .map(|category| category.name.as_str()),
                Some(name),
            );
        }
    }

    /// The issue's own example: no minion has an equip level, an HQ variant,
    /// or a real item level, so all three columns are dead weight there —
    /// while a gear category fills every one of them.
    #[test]
    fn minions_fill_no_optional_column_but_weapons_fill_them_all() {
        let minions = column_availability(category_items(MINIONS), |_| None);
        assert_eq!(
            minions,
            ColumnAvailability {
                item_level: false,
                equip_level: false,
                hq: false,
                vendor: false,
            },
        );

        let weapons = column_availability(category_items(GLADIATORS_ARMS), |_| None);
        assert!(weapons.item_level);
        assert!(weapons.equip_level);
        assert!(weapons.hq);
    }

    /// Availability is a property of the *set*: one qualifying item is enough
    /// to keep a column, so filtering never changes the column layout.
    #[test]
    fn one_item_with_a_value_keeps_the_column() {
        let equippable = category_items(GLADIATORS_ARMS)
            .into_iter()
            .find(|item| has_equip_level(item) && item.can_be_hq)
            .expect("gladiator gear is equippable and HQ-able");
        let availability = column_availability([first(MINIONS), equippable], |_| None);
        assert!(availability.equip_level);
        assert!(availability.hq);
        assert!(availability.item_level);
    }

    /// The vendor column is driven by the injected lookup, not by the item
    /// sheet, since "a vendor sells this" lives in the shop tables.
    #[test]
    fn vendor_availability_follows_the_injected_lookup() {
        let minion = first(MINIONS);
        let id = minion.key_id.0;
        assert!(!column_availability([minion], |_| None).vendor);
        assert!(column_availability([minion], move |i| (i == id).then_some(100)).vendor);
    }

    /// The documented subject rule: the cheapest listed quality at its own
    /// world; NQ on world 0 with no price when nothing is listed or prices
    /// have not loaded yet.
    #[test]
    fn subject_is_the_cheapest_listed_quality_or_nq_on_world_zero() {
        let item = first(GLADIATORS_ARMS);
        let id = item.key_id.0;

        let both = ExplorerRow::build(id, item, Some(&listings(id, Some(500), Some(400))), None);
        assert_eq!(
            both.cheapest,
            Some(CheapestListing {
                price: 400,
                hq: true,
                world_id: 9
            })
        );
        assert_eq!((both.nq, both.hq), (Some(500), Some(400)));
        let subject = both.market_subject();
        assert_eq!(
            (subject.hq, subject.world_id, subject.listing_price),
            (true, 9, Some(400))
        );
        assert_eq!(subject.label, item.name);

        // A tie keeps NQ: `chosen(false)` only prefers HQ when it is cheaper.
        let tie = ExplorerRow::build(id, item, Some(&listings(id, Some(400), Some(400))), None);
        assert!(!tie.market_subject().hq);
        let mut identical = listings(id, Some(400), Some(400));
        identical
            .map
            .get_mut(&CheapestListingMapKey {
                item_id: id,
                hq: true,
            })
            .unwrap()
            .world_id = 7;
        let tie = ExplorerRow::build(id, item, Some(&identical), None);
        assert!(
            !tie.market_subject().hq,
            "identical listing payloads still choose NQ"
        );

        let nq_only = ExplorerRow::build(id, item, Some(&listings(id, Some(300), None)), None);
        assert_eq!(
            nq_only.market_subject().world_id,
            7,
            "the location is the actual listing's"
        );

        let none = ExplorerRow::build(id, item, Some(&listings(id, None, None)), None);
        let subject = none.market_subject();
        assert_eq!(
            (subject.hq, subject.world_id, subject.listing_price),
            (false, 0, None)
        );

        let unloaded = ExplorerRow::build(id, item, None, None);
        let subject = unloaded.market_subject();
        assert_eq!(
            (subject.hq, subject.world_id, subject.listing_price),
            (false, 0, None)
        );
    }

    /// The hydration contract, carried by the row: before the gate flips a
    /// price value is `Pending` (a filter keeps the row, a sort waits); once
    /// loaded, nothing listed is `Missing` and a price is a number.
    #[test]
    fn price_values_are_pending_until_prices_load() {
        let item = first(GLADIATORS_ARMS);
        let id = item.key_id.0;
        let unloaded = ExplorerRow::build(id, item, None, None);
        assert!(!unloaded.prices_loaded);
        assert_eq!(unloaded.price_value(false), GridValue::Pending);
        assert_eq!(unloaded.price_value(true), GridValue::Pending);
        assert_eq!(unloaded.listing_value(), GridValue::Pending);

        let none = ExplorerRow::build(id, item, Some(&listings(id, None, None)), None);
        assert_eq!(none.price_value(false), GridValue::Missing);
        assert_eq!(none.listing_value(), GridValue::Missing);

        let both = ExplorerRow::build(id, item, Some(&listings(id, Some(500), Some(400))), None);
        assert_eq!(both.price_value(false), GridValue::Number(500.0));
        assert_eq!(both.price_value(true), GridValue::Number(400.0));
        assert_eq!(both.listing_value(), GridValue::Number(400.0));
    }

    /// Every legacy key resolves to the shared filter with the old meaning,
    /// and a min/max pair becomes one inclusive bound.
    #[test]
    fn legacy_filter_keys_resolve_to_inclusive_shared_bounds() {
        let aliases = explorer_filter_aliases();
        let mut query = ParamsMap::new();
        query.insert(FILTER_MIN_ILVL, "600".to_string());
        query.insert(FILTER_MAX_ILVL, "700".to_string());
        query.insert(FILTER_MIN_LV, "90".to_string());
        query.insert(FILTER_NAME, " sword ".to_string());
        query.insert(FILTER_MAX_PRICE, "1000".to_string());
        query.insert(FILTER_VENDOR, "true".to_string());
        query.insert(FILTER_LISTED, "true".to_string());
        let filters = resolve_filters(&query, &aliases);

        assert_eq!(filters[COL_ITEM_LEVEL].op, FilterOp::Between);
        assert_eq!(filters[COL_ITEM_LEVEL].value, "600,700");
        assert_eq!(filters[COL_EQUIP_LEVEL].op, FilterOp::Gte);
        assert_eq!(filters[COL_ITEM].op, FilterOp::Contains);
        assert_eq!(filters[COL_ITEM].value, "sword");
        assert_eq!(filters[COL_VENDOR].op, FilterOp::Present);
        // `max-price` wins over `listed` on the shared listing column; an
        // upper bound already excludes unlisted rows.
        assert_eq!(filters[COL_LISTING].op, FilterOp::Lte);
        assert_eq!(filters[COL_LISTING].value, "1000");
        assert_eq!(filters.len(), 5);

        let mut listed = ParamsMap::new();
        listed.insert(FILTER_LISTED, "true".to_string());
        assert_eq!(
            resolve_filters(&listed, &aliases)[COL_LISTING].op,
            FilterOp::Present
        );

        // A boolean key is on only for the literal `true` the chips wrote.
        for raw in ["false", "", "yes", "1"] {
            let mut off = ParamsMap::new();
            off.insert(FILTER_VENDOR, raw.to_string());
            off.insert(FILTER_LISTED, raw.to_string());
            assert!(resolve_filters(&off, &aliases).is_empty(), "{raw:?}");
        }
    }

    /// The bounds are inclusive, as the old chips were, and a `Pending`
    /// price keeps the row: the hydration contract seen through the grid.
    #[test]
    fn level_bounds_are_inclusive_and_pending_prices_keep_rows() {
        let aliases = explorer_filter_aliases();
        let mut query = ParamsMap::new();
        query.insert(FILTER_MIN_ILVL, "600".to_string());
        query.insert(FILTER_MAX_ILVL, "700".to_string());
        query.insert(FILTER_MAX_PRICE, "1000".to_string());
        let filters = resolve_filters(&query, &aliases);
        let ilvl = &filters[COL_ITEM_LEVEL];
        for (level, pass) in [(599.0, false), (600.0, true), (700.0, true), (701.0, false)] {
            assert_eq!(ilvl.matches(&GridValue::Number(level), false), Some(pass));
        }
        let price = &filters[COL_LISTING];
        assert_eq!(price.matches(&GridValue::Pending, false), None);
        assert_eq!(price.matches(&GridValue::Missing, false), Some(false));
        assert_eq!(price.matches(&GridValue::Number(1000.0), false), Some(true));
    }

    #[test]
    fn hq_only_is_the_one_route_level_filter() {
        let minion = first(MINIONS);
        assert!(!minion.can_be_hq);
        assert!(ExplorerFilters::default().matches(minion));
        assert!(!ExplorerFilters { hq_only: true }.matches(minion));
        let hq_able = category_items(GLADIATORS_ARMS)
            .into_iter()
            .find(|item| item.can_be_hq)
            .expect("gladiator gear can be HQ");
        assert!(ExplorerFilters { hq_only: true }.matches(hq_able));
    }

    /// `has` is what the columns picker asks, so an id it does not know about
    /// (the item and action columns, which are never optional) must not read
    /// as "unavailable" and take a column away.
    #[test]
    fn unknown_column_ids_are_always_available() {
        let nothing = ColumnAvailability::default();
        assert!(nothing.has(COL_ITEM));
        assert!(!nothing.has(COL_HQ));
    }

    /// Guards the em-dash rule the Lv and iLvl cells render with: both fields
    /// are 1, not 0, for everything that has neither.
    #[test]
    fn a_level_of_one_is_no_level_at_all() {
        let minion = first(MINIONS);
        assert!(!has_equip_level(minion));
        assert!(!has_item_level(minion));
    }
}
