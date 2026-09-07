//! Filter predicates and column availability for the Item Explorer.
//!
//! Kept out of [`item_explorer`](super::item_explorer) — and free of every
//! reactive and context read — so both halves can be unit-tested against the
//! shipped game-data pack. The page owns the URL plumbing and the chips; this
//! module owns "does this item match?" and "does this column have anything to
//! show?".
//!
//! # Why availability is computed from the whole set
//!
//! Issue #1296: not one item in a category like Minions has an equip level, an
//! HQ variant, or a real item level, so three of the table's columns render
//! 261 dashes, blanks and identical ones. [`column_availability`] scans the
//! set once and the page hides the columns nothing filled in.
//!
//! It has to be a property of the **set**, not of the page of rows currently
//! on screen: a column that appears and disappears as you page through would
//! reshuffle the grid template under the reader. Both callers therefore pass
//! the full, unpaginated, *unfiltered* item list.

use xiv_gen::Item;

/// Cheapest market price for one item, as far as the filters need to know it.
///
/// The three-way split is what keeps the market-price filters out of
/// hydration's way. `NotLoaded` is what the server and the first client render
/// both see — the price map is deliberately withheld until after hydration
/// (see the `hydrated` flag in `item_explorer.rs`), and a price filter that
/// dropped rows on one side only is exactly the SSR/CSR row-count mismatch
/// that tachys panics on. `Missing` is a *loaded* price map that has no
/// listing for this item, which the filters treat as "does not match a price
/// bound" rather than "unknown".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheapestPrice {
    /// Prices are not available to this render yet. Price filters no-op.
    NotLoaded,
    /// Prices are loaded and nothing is listed for this item.
    Missing,
    Some(i32),
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
            super::item_explorer::COL_ID_ITEM_LEVEL => self.item_level,
            super::item_explorer::COL_ID_EQUIP_LEVEL => self.equip_level,
            super::item_explorer::COL_ID_HQ => self.hq,
            super::item_explorer::COL_ID_VENDOR => self.vendor,
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

/// The explorer's filter chips, parsed out of the URL.
///
/// Every field is "no filter" when unset, so an explorer with no `?`-params
/// keeps rendering the full category exactly as it did before #1296.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExplorerFilters {
    /// Case-insensitive substring of the item name.
    pub name: Option<String>,
    pub min_ilvl: Option<i32>,
    pub max_ilvl: Option<i32>,
    pub min_lv: Option<i32>,
    /// Cheapest market listing at or below this many gil.
    pub max_price: Option<i32>,
    /// Only items a vendor sells — the collection-completion case the issue
    /// opens with ("which minions can be bought from a vendor").
    pub vendor_only: bool,
    /// Only items with an HQ variant.
    pub hq_only: bool,
    /// Only items with at least one market listing in the current scope.
    pub listed_only: bool,
}

impl ExplorerFilters {
    /// Is the filter behind this `?` key in use? Drives which chips the bar
    /// draws and which entries `+ Filter` still offers.
    pub fn is_set(&self, key: &str) -> bool {
        use super::item_explorer as page;
        match key {
            page::FILTER_NAME => self.name.is_some(),
            page::FILTER_MIN_ILVL => self.min_ilvl.is_some(),
            page::FILTER_MAX_ILVL => self.max_ilvl.is_some(),
            page::FILTER_MIN_LV => self.min_lv.is_some(),
            page::FILTER_MAX_PRICE => self.max_price.is_some(),
            page::FILTER_VENDOR => self.vendor_only,
            page::FILTER_HQ => self.hq_only,
            page::FILTER_LISTED => self.listed_only,
            _ => false,
        }
    }

    /// Does this item survive the filters?
    ///
    /// `vendor` is the item's vendor price (`None` when no vendor sells it)
    /// and `price` its cheapest listing. Both are passed in because both come
    /// from sources this module deliberately does not reach into: static game
    /// data behind a context read, and a resource that is not resolved yet
    /// during hydration.
    pub fn matches(&self, item: &Item, vendor: Option<u32>, price: CheapestPrice) -> bool {
        if let Some(needle) = &self.name
            && !item.name.to_lowercase().contains(&needle.to_lowercase())
        {
            return false;
        }
        if let Some(min) = self.min_ilvl
            && item.level_item < min
        {
            return false;
        }
        if let Some(max) = self.max_ilvl
            && item.level_item > max
        {
            return false;
        }
        if let Some(min) = self.min_lv
            && item.level_equip < min
        {
            return false;
        }
        if self.vendor_only && vendor.is_none() {
            return false;
        }
        if self.hq_only && !item.can_be_hq {
            return false;
        }
        // Both market-price filters are inert while `price` is `NotLoaded`, so
        // the server and the first client render agree on the row set. See
        // [`CheapestPrice`].
        if self.listed_only && price == CheapestPrice::Missing {
            return false;
        }
        if let Some(max) = self.max_price {
            match price {
                CheapestPrice::Some(gil) if gil > max => return false,
                CheapestPrice::Missing => return false,
                _ => {}
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::item_explorer::ADDABLE_FILTERS;
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
    /// to keep a column, so paging never changes the column layout.
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

    #[test]
    fn no_filters_match_everything() {
        let filters = ExplorerFilters::default();
        assert!(ADDABLE_FILTERS.iter().all(|key| !filters.is_set(key)));
        for item in category_items(MINIONS) {
            assert!(filters.matches(item, None, CheapestPrice::Missing));
        }
    }

    #[test]
    fn name_filter_is_a_case_insensitive_substring() {
        let minion = first(MINIONS);
        let shouty = minion.name.to_uppercase();
        let filters = ExplorerFilters {
            name: Some(shouty),
            ..Default::default()
        };
        assert!(filters.matches(minion, None, CheapestPrice::NotLoaded));

        let elsewhere = ExplorerFilters {
            name: Some("definitely not an item name".to_string()),
            ..Default::default()
        };
        assert!(!elsewhere.matches(minion, None, CheapestPrice::NotLoaded));
    }

    /// The issue's motivating question — "which minions can be bought from a
    /// vendor" — is this filter.
    #[test]
    fn vendor_only_keeps_exactly_the_vendor_items() {
        let filters = ExplorerFilters {
            vendor_only: true,
            ..Default::default()
        };
        let minion = first(MINIONS);
        assert!(!filters.matches(minion, None, CheapestPrice::NotLoaded));
        assert!(filters.matches(minion, Some(1000), CheapestPrice::NotLoaded));
    }

    #[test]
    fn level_bounds_are_inclusive() {
        let sword = category_items(GLADIATORS_ARMS)
            .into_iter()
            .find(|item| item.level_item > 1)
            .expect("gladiator gear carries an item level");
        let ilvl = sword.level_item;
        for (min, max, expected) in [
            (Some(ilvl), Some(ilvl), true),
            (Some(ilvl + 1), None, false),
            (None, Some(ilvl - 1), false),
        ] {
            let filters = ExplorerFilters {
                min_ilvl: min,
                max_ilvl: max,
                ..Default::default()
            };
            assert_eq!(
                filters.matches(sword, None, CheapestPrice::NotLoaded),
                expected,
                "ilvl {ilvl} against min {min:?} / max {max:?}",
            );
        }
    }

    /// The hydration contract: while prices are `NotLoaded`, a price filter
    /// must not remove a single row, or the server's row set and the client's
    /// first render disagree and tachys' walker panics.
    #[test]
    fn price_filters_are_inert_until_prices_load() {
        let filters = ExplorerFilters {
            max_price: Some(1),
            listed_only: true,
            ..Default::default()
        };
        let minion = first(MINIONS);
        assert!(filters.matches(minion, None, CheapestPrice::NotLoaded));
        // Once loaded, the same filters bite.
        assert!(!filters.matches(minion, None, CheapestPrice::Missing));
        assert!(!filters.matches(minion, None, CheapestPrice::Some(500)));
        assert!(filters.matches(minion, None, CheapestPrice::Some(1)));
    }

    /// `has` is what the columns picker asks, so an id it does not know about
    /// (the icon, name and action columns, which are never optional) must not
    /// read as "unavailable" and take a column away.
    #[test]
    fn unknown_column_ids_are_always_available() {
        let nothing = ColumnAvailability::default();
        assert!(nothing.has("name"));
        assert!(!nothing.has(crate::routes::item_explorer::COL_ID_HQ));
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
