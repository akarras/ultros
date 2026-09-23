//! Item-page verdicts: what to list an item at, and whether to craft or buy it.
//!
//! Pure functions over the item page's listings and recent sales, so they can
//! be tested without Leptos. The thresholds are first guesses and live here so
//! tuning one is a one-line change.

use crate::analysis::{RealPriceBreakdown, RealPriceEstimate, real_price};

/// A wall keeps growing while each next listing is at most this multiple of
/// the one before it.
pub const WALL_STEP: f64 = 1.05;
/// Longest stretch of recent sales the sale rate looks back over, in days.
pub const RATE_WINDOW_DAYS: f64 = 7.0;
/// With fewer matching sales than this the rate is "too few sales".
pub const MIN_RATE_SALES: usize = 3;
/// A floor under this fraction of real price earns a warning.
pub const FLOOR_LOW: f64 = 0.7;
/// A floor over this multiple of real price earns a warning.
pub const FLOOR_HIGH: f64 = 1.5;
/// Craft and buy within this fraction of each other are "about even".
pub const EVEN_BAND: f64 = 0.03;

const SECONDS_PER_DAY: f64 = 86_400.0;
/// Shortest rate window, so a burst of same-second sales (or a client clock
/// behind the server) cannot divide by zero.
const MIN_WINDOW_DAYS: f64 = 1.0 / 24.0;

/// One listing on the board.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListingSample {
    pub world_id: i32,
    pub price_per_unit: i32,
    pub quantity: i32,
    pub hq: bool,
}

/// One recent sale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SaleSample {
    pub world_id: i32,
    pub price_per_item: i32,
    pub quantity: i32,
    pub hq: bool,
    /// Unix seconds.
    pub sold_at: i64,
}

fn breakdown<'a>(
    sales: impl Iterator<Item = &'a SaleSample>,
    vendor_price: Option<i32>,
) -> RealPriceBreakdown {
    let samples: Vec<(i32, i32, bool)> = sales
        .map(|sale| (sale.price_per_item, sale.quantity, sale.hq))
        .collect();
    real_price(&samples, vendor_price)
}

fn estimate_for(breakdown: &RealPriceBreakdown, hq: bool) -> Option<RealPriceEstimate> {
    if hq { breakdown.hq } else { breakdown.nq }
}

/// The quality both verdict cards describe: `real_price`'s headline quality
/// over the page scope's sales (more sales wins, NQ on a tie), NQ when there
/// are no sales at all.
pub fn headline_hq(sales: &[SaleSample], vendor_price: Option<i32>) -> bool {
    breakdown(sales.iter(), vendor_price)
        .primary()
        .is_some_and(|(hq, _)| hq)
}

/// The cheapest run of listings: from the floor, while each next price is at
/// most [`WALL_STEP`] times the previous one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wall {
    pub listings: u32,
    pub units: u32,
    pub low: i32,
    pub high: i32,
}

/// The first listing above the wall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gap {
    pub price: i32,
    /// `(price - wall.high) / wall.high`, in percent.
    pub percent: f64,
}

fn units(quantity: i32) -> u32 {
    quantity.max(0) as u32
}

/// The wall and the gap above it, from `(price, quantity)` pairs sorted by
/// price ascending. `None` for an empty board.
pub fn wall_and_gap(sorted: &[(i32, i32)]) -> Option<(Wall, Option<Gap>)> {
    let (&(low, quantity), rest) = sorted.split_first()?;
    let mut wall = Wall {
        listings: 1,
        units: units(quantity),
        low,
        high: low,
    };
    for &(price, quantity) in rest {
        if f64::from(price) <= f64::from(wall.high) * WALL_STEP {
            wall.listings += 1;
            wall.units += units(quantity);
            wall.high = price;
        } else {
            let percent = f64::from(price - wall.high) / f64::from(wall.high.max(1)) * 100.0;
            return Some((wall, Some(Gap { price, percent })));
        }
    }
    Some((wall, None))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SaleRate {
    UnitsPerDay(f64),
    TooFewSales,
}

/// Units of one quality sold per day on one world.
///
/// `sales` is the page scope's recent-sale buffer. The window runs from its
/// oldest sale to `now`, capped at [`RATE_WINDOW_DAYS`]: the buffer is shared
/// by every world in scope, so when a busy datacenter's buffer only reaches
/// back two days, two days is all that is known on any of its worlds.
pub fn sale_rate(sales: &[SaleSample], world_id: i32, hq: bool, now: i64) -> SaleRate {
    let Some(oldest) = sales.iter().map(|sale| sale.sold_at).min() else {
        return SaleRate::TooFewSales;
    };
    let window_days =
        ((now - oldest) as f64 / SECONDS_PER_DAY).clamp(MIN_WINDOW_DAYS, RATE_WINDOW_DAYS);
    let cutoff = now - (window_days * SECONDS_PER_DAY) as i64;
    let (count, sold) = sales
        .iter()
        .filter(|sale| sale.world_id == world_id && sale.hq == hq && sale.sold_at >= cutoff)
        .fold((0usize, 0u64), |(count, sold), sale| {
            (count + 1, sold + u64::from(units(sale.quantity)))
        });
    if count < MIN_RATE_SALES {
        return SaleRate::TooFewSales;
    }
    SaleRate::UnitsPerDay(sold as f64 / window_days)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FloorWarning {
    /// The floor sits this many percent under real price.
    UnderRealPrice {
        percent: f64,
    },
    AboveRecentSales,
}

fn floor_warning(floor: i32, real: i32) -> Option<FloorWarning> {
    if real <= 0 {
        return None;
    }
    let (floor, real) = (f64::from(floor), f64::from(real));
    if floor < real * FLOOR_LOW {
        Some(FloorWarning::UnderRealPrice {
            percent: (real - floor) / real * 100.0,
        })
    } else if floor > real * FLOOR_HIGH {
        Some(FloorWarning::AboveRecentSales)
    } else {
        None
    }
}

/// What the board says about listing now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoardVerdict {
    pub floor: i32,
    /// One gil under the floor (never below 1).
    pub fast: i32,
    pub wall: Wall,
    pub gap: Option<Gap>,
    /// One gil under the gap listing; `None` without a gap.
    pub patient: Option<i32>,
    /// Units listed at the headline quality on the world.
    pub listed_units: u32,
    pub days_of_stock: Option<f64>,
    /// Days until the wall ahead of a patient listing sells through.
    pub patient_eta_days: Option<f64>,
    pub warning: Option<FloorWarning>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SellVerdict {
    pub hq: bool,
    pub real_price: Option<i32>,
    /// The world had no sales at `hq`, so `real_price` is the page scope's.
    pub real_price_scope_wide: bool,
    pub rate: SaleRate,
    /// `None` when nothing is listed at `hq` on the world.
    pub board: Option<BoardVerdict>,
}

/// The sell verdict for one world.
///
/// `listings` may span the page scope; only `world_id`'s rows are used.
/// `sales` is the page scope's recent-sale buffer.
pub fn sell_verdict(
    listings: &[ListingSample],
    sales: &[SaleSample],
    world_id: i32,
    vendor_price: Option<i32>,
    now: i64,
) -> SellVerdict {
    let hq = headline_hq(sales, vendor_price);
    let world_real = estimate_for(
        &breakdown(
            sales.iter().filter(|sale| sale.world_id == world_id),
            vendor_price,
        ),
        hq,
    );
    let (real_price, real_price_scope_wide) = match world_real {
        Some(estimate) => (Some(estimate.value), false),
        None => {
            let scope = estimate_for(&breakdown(sales.iter(), vendor_price), hq)
                .map(|estimate| estimate.value);
            (scope, scope.is_some())
        }
    };
    let rate = sale_rate(sales, world_id, hq, now);
    let per_day = match rate {
        SaleRate::UnitsPerDay(rate) if rate > 0.0 => Some(rate),
        _ => None,
    };

    let mut board: Vec<(i32, i32)> = listings
        .iter()
        .filter(|listing| listing.world_id == world_id && listing.hq == hq)
        .map(|listing| (listing.price_per_unit, listing.quantity))
        .collect();
    board.sort_unstable();
    let listed_units: u32 = board.iter().map(|&(_, quantity)| units(quantity)).sum();

    let board = wall_and_gap(&board).map(|(wall, gap)| BoardVerdict {
        floor: wall.low,
        fast: (wall.low - 1).max(1),
        wall,
        gap,
        patient: gap.map(|gap| (gap.price - 1).max(1)),
        listed_units,
        days_of_stock: per_day.map(|rate| f64::from(listed_units) / rate),
        patient_eta_days: gap.and(per_day).map(|rate| f64::from(wall.units) / rate),
        warning: real_price.and_then(|real| floor_warning(wall.low, real)),
    });

    SellVerdict {
        hq,
        real_price,
        real_price_scope_wide,
        rate,
        board,
    }
}

/// Where the "buy" side of the craft verdict comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuySource {
    /// Cheapest listing in the viewer's price zone. `nq_fallback`: an HQ
    /// verdict with no HQ listing, priced from the NQ one.
    Market { nq_fallback: bool },
    /// An NPC gil shop.
    Vendor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuyPrice {
    pub price: i32,
    pub source: BuySource,
}

/// What buying one unit at the headline quality costs. Vendors only sell NQ,
/// so `vendor` is considered for an NQ verdict only.
pub fn buy_price(
    hq: bool,
    market_hq: Option<i32>,
    market_nq: Option<i32>,
    vendor: Option<i32>,
) -> Option<BuyPrice> {
    let market = |price, nq_fallback| BuyPrice {
        price,
        source: BuySource::Market { nq_fallback },
    };
    let market = if hq {
        market_hq
            .map(|price| market(price, false))
            .or_else(|| market_nq.map(|price| market(price, true)))
    } else {
        market_nq.map(|price| market(price, false))
    };
    let vendor = vendor
        .filter(|price| !hq && *price > 0)
        .map(|price| BuyPrice {
            price,
            source: BuySource::Vendor,
        });
    match (market, vendor) {
        (Some(market), Some(vendor)) => Some(if vendor.price < market.price {
            vendor
        } else {
            market
        }),
        (market, vendor) => market.or(vendor),
    }
}

/// `compute_cost` prices one execution of a recipe; this is the cost of one
/// of the `amount_result` units it yields.
pub fn craft_unit_cost(cost: i32, amount_result: i32) -> i32 {
    cost / amount_result.max(1)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CraftVerdict {
    CraftSaves {
        gil: i32,
        percent: f64,
    },
    BuyCheaper {
        gil: i32,
        percent: f64,
    },
    AboutEven,
    /// An ingredient had no listing, or there is nothing to buy to compare with.
    Incomplete,
}

/// Compare one crafted unit with one bought unit. `unpriced_lines` is
/// `CostBreakdown::unpriced_market_lines`.
pub fn craft_verdict(craft_unit: i32, buy: Option<i32>, unpriced_lines: u16) -> CraftVerdict {
    let Some(buy) = buy.filter(|price| *price > 0) else {
        return CraftVerdict::Incomplete;
    };
    if unpriced_lines > 0 {
        return CraftVerdict::Incomplete;
    }
    let (craft, bought) = (f64::from(craft_unit), f64::from(buy));
    if craft < bought * (1.0 - EVEN_BAND) {
        CraftVerdict::CraftSaves {
            gil: buy - craft_unit,
            percent: (bought - craft) / bought * 100.0,
        }
    } else if craft > bought * (1.0 + EVEN_BAND) {
        CraftVerdict::BuyCheaper {
            gil: craft_unit - buy,
            percent: (craft - bought) / craft * 100.0,
        }
    } else {
        CraftVerdict::AboutEven
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;
    const NOW: i64 = 1_800_000_000;
    const WORLD: i32 = 40;
    const OTHER: i32 = 41;

    fn listing(world_id: i32, price: i32, quantity: i32, hq: bool) -> ListingSample {
        ListingSample {
            world_id,
            price_per_unit: price,
            quantity,
            hq,
        }
    }

    fn sale(world_id: i32, price: i32, quantity: i32, hq: bool, sold_at: i64) -> SaleSample {
        SaleSample {
            world_id,
            price_per_item: price,
            quantity,
            hq,
            sold_at,
        }
    }

    #[test]
    fn headline_hq_prefers_the_quality_with_more_sales() {
        let sales = [
            sale(WORLD, 100, 1, true, NOW - 10),
            sale(WORLD, 100, 1, true, NOW - 20),
            sale(WORLD, 50, 1, false, NOW - 30),
        ];
        assert!(headline_hq(&sales, None));
    }

    #[test]
    fn headline_hq_breaks_ties_and_empties_to_nq() {
        let tie = [
            sale(WORLD, 100, 1, true, NOW),
            sale(WORLD, 50, 1, false, NOW),
        ];
        assert!(!headline_hq(&tie, None));
        assert!(!headline_hq(&[], None));
    }

    #[test]
    fn wall_of_a_single_listing_has_no_gap() {
        let (wall, gap) = wall_and_gap(&[(500, 3)]).unwrap();
        assert_eq!(
            wall,
            Wall {
                listings: 1,
                units: 3,
                low: 500,
                high: 500
            }
        );
        assert_eq!(gap, None);
    }

    #[test]
    fn wall_spanning_the_whole_board_has_no_gap() {
        let (wall, gap) = wall_and_gap(&[(100, 1), (104, 2), (108, 5)]).unwrap();
        assert_eq!(
            wall,
            Wall {
                listings: 3,
                units: 8,
                low: 100,
                high: 108
            }
        );
        assert_eq!(gap, None);
    }

    #[test]
    fn a_step_of_exactly_five_percent_stays_in_the_wall() {
        let (wall, gap) = wall_and_gap(&[(100, 1), (105, 1), (111, 1)]).unwrap();
        assert_eq!(wall.high, 105);
        let gap = gap.unwrap();
        assert_eq!(gap.price, 111);
        assert!((gap.percent - 100.0 * 6.0 / 105.0).abs() < 1e-9);
    }

    #[test]
    fn empty_board_has_no_wall() {
        assert_eq!(wall_and_gap(&[]), None);
    }

    #[test]
    fn sale_rate_uses_only_the_world_and_quality() {
        let sales = [
            sale(WORLD, 100, 2, false, NOW - DAY),
            sale(WORLD, 100, 3, false, NOW - DAY),
            sale(WORLD, 100, 5, false, NOW - 2 * DAY),
            sale(WORLD, 100, 50, true, NOW - DAY),
            sale(OTHER, 100, 50, false, NOW - 2 * DAY),
        ];
        // Window = oldest sale (2 days) → 10 NQ units on WORLD over 2 days.
        match sale_rate(&sales, WORLD, false, NOW) {
            SaleRate::UnitsPerDay(rate) => assert!((rate - 5.0).abs() < 1e-9),
            other => panic!("expected a rate, got {other:?}"),
        }
    }

    #[test]
    fn sale_rate_window_is_capped_at_seven_days() {
        let sales = [
            sale(WORLD, 100, 7, false, NOW - DAY),
            sale(WORLD, 100, 7, false, NOW - 2 * DAY),
            sale(WORLD, 100, 7, false, NOW - 3 * DAY),
            sale(OTHER, 100, 1, false, NOW - 30 * DAY),
        ];
        match sale_rate(&sales, WORLD, false, NOW) {
            SaleRate::UnitsPerDay(rate) => assert!((rate - 3.0).abs() < 1e-9),
            other => panic!("expected a rate, got {other:?}"),
        }
    }

    #[test]
    fn sale_rate_needs_three_sales() {
        let sales = [
            sale(WORLD, 100, 9, false, NOW - DAY),
            sale(WORLD, 100, 9, false, NOW - DAY),
        ];
        assert_eq!(sale_rate(&sales, WORLD, false, NOW), SaleRate::TooFewSales);
        assert_eq!(sale_rate(&[], WORLD, false, NOW), SaleRate::TooFewSales);
    }

    #[test]
    fn sale_rate_ignores_sales_older_than_the_window() {
        let sales = [
            sale(WORLD, 100, 1, false, NOW - 20 * DAY),
            sale(WORLD, 100, 1, false, NOW - 21 * DAY),
            sale(WORLD, 100, 1, false, NOW - 22 * DAY),
        ];
        assert_eq!(sale_rate(&sales, WORLD, false, NOW), SaleRate::TooFewSales);
    }

    #[test]
    fn sale_rate_survives_sales_in_the_future() {
        let sales = [
            sale(WORLD, 100, 1, false, NOW + 60),
            sale(WORLD, 100, 1, false, NOW + 60),
            sale(WORLD, 100, 1, false, NOW + 60),
        ];
        match sale_rate(&sales, WORLD, false, NOW) {
            SaleRate::UnitsPerDay(rate) => assert!(rate.is_finite() && rate > 0.0),
            other => panic!("expected a rate, got {other:?}"),
        }
    }

    fn steady_sales(world_id: i32, hq: bool) -> Vec<SaleSample> {
        // 4 sales of 1 unit at 1000 over the last 2 days: rate 2 units/day.
        (0..4)
            .map(|i| sale(world_id, 1000, 1, hq, NOW - DAY / 2 - i * (DAY / 2)))
            .collect()
    }

    #[test]
    fn sell_verdict_builds_fast_and_patient_prices() {
        let listings = [
            listing(WORLD, 900, 2, false),
            listing(WORLD, 920, 3, false),
            listing(WORLD, 1100, 1, false),
            listing(WORLD, 10, 99, true),
            listing(OTHER, 1, 99, false),
        ];
        let v = sell_verdict(&listings, &steady_sales(WORLD, false), WORLD, None, NOW);
        assert!(!v.hq);
        assert_eq!(v.real_price, Some(1000));
        assert!(!v.real_price_scope_wide);
        let board = v.board.unwrap();
        assert_eq!(board.floor, 900);
        assert_eq!(board.fast, 899);
        assert_eq!(
            board.wall,
            Wall {
                listings: 2,
                units: 5,
                low: 900,
                high: 920
            }
        );
        assert_eq!(board.patient, Some(1099));
        assert_eq!(board.listed_units, 6);
        let rate = match v.rate {
            SaleRate::UnitsPerDay(rate) => rate,
            other => panic!("expected a rate, got {other:?}"),
        };
        assert!((board.days_of_stock.unwrap() - 6.0 / rate).abs() < 1e-9);
        assert!((board.patient_eta_days.unwrap() - 5.0 / rate).abs() < 1e-9);
        assert_eq!(board.warning, None);
    }

    #[test]
    fn fast_price_never_drops_below_one() {
        let v = sell_verdict(&[listing(WORLD, 1, 1, false)], &[], WORLD, None, NOW);
        assert_eq!(v.board.unwrap().fast, 1);
    }

    #[test]
    fn no_rate_means_no_day_estimates() {
        let v = sell_verdict(
            &[
                listing(WORLD, 900, 2, false),
                listing(WORLD, 2000, 1, false),
            ],
            &[],
            WORLD,
            None,
            NOW,
        );
        let board = v.board.unwrap();
        assert_eq!(v.rate, SaleRate::TooFewSales);
        assert_eq!(board.days_of_stock, None);
        assert_eq!(board.patient_eta_days, None);
    }

    #[test]
    fn real_price_falls_back_to_the_scope() {
        let v = sell_verdict(
            &[listing(WORLD, 900, 1, false)],
            &steady_sales(OTHER, false),
            WORLD,
            None,
            NOW,
        );
        assert_eq!(v.real_price, Some(1000));
        assert!(v.real_price_scope_wide);
    }

    #[test]
    fn no_sales_anywhere_means_no_real_price() {
        let v = sell_verdict(&[listing(WORLD, 900, 1, false)], &[], WORLD, None, NOW);
        assert_eq!(v.real_price, None);
        assert!(!v.real_price_scope_wide);
        assert_eq!(v.board.unwrap().warning, None);
    }

    #[test]
    fn empty_board_for_the_headline_quality() {
        let v = sell_verdict(
            &[listing(WORLD, 900, 1, true)],
            &steady_sales(WORLD, false),
            WORLD,
            None,
            NOW,
        );
        assert!(!v.hq);
        assert_eq!(v.board, None);
        assert_eq!(v.real_price, Some(1000));
    }

    fn warning_for(floor: i32) -> Option<FloorWarning> {
        sell_verdict(
            &[listing(WORLD, floor, 1, false)],
            &steady_sales(WORLD, false),
            WORLD,
            None,
            NOW,
        )
        .board
        .unwrap()
        .warning
    }

    #[test]
    fn floor_warnings_trip_strictly_outside_the_band() {
        // Real price is 1000.
        assert_eq!(warning_for(700), None);
        match warning_for(699) {
            Some(FloorWarning::UnderRealPrice { percent }) => {
                assert!((percent - 30.1).abs() < 1e-9)
            }
            other => panic!("expected an under-real warning, got {other:?}"),
        }
        assert_eq!(warning_for(1500), None);
        assert_eq!(warning_for(1501), Some(FloorWarning::AboveRecentSales));
    }

    #[test]
    fn craft_unit_cost_divides_by_yield() {
        assert_eq!(craft_unit_cost(300, 3), 100);
        assert_eq!(craft_unit_cost(300, 1), 300);
        assert_eq!(craft_unit_cost(300, 0), 300);
    }

    #[test]
    fn craft_verdict_band_edges() {
        match craft_verdict(969, Some(1000), 0) {
            CraftVerdict::CraftSaves { gil, percent } => {
                assert_eq!(gil, 31);
                assert!((percent - 3.1).abs() < 1e-9);
            }
            other => panic!("expected craft saves, got {other:?}"),
        }
        assert_eq!(craft_verdict(971, Some(1000), 0), CraftVerdict::AboutEven);
        assert_eq!(craft_verdict(1029, Some(1000), 0), CraftVerdict::AboutEven);
        match craft_verdict(1031, Some(1000), 0) {
            CraftVerdict::BuyCheaper { gil, percent } => {
                assert_eq!(gil, 31);
                assert!((percent - 100.0 * 31.0 / 1031.0).abs() < 1e-9);
            }
            other => panic!("expected buy cheaper, got {other:?}"),
        }
    }

    #[test]
    fn craft_verdict_is_incomplete_without_prices() {
        assert_eq!(craft_verdict(500, Some(1000), 1), CraftVerdict::Incomplete);
        assert_eq!(craft_verdict(500, None, 0), CraftVerdict::Incomplete);
        assert_eq!(craft_verdict(500, Some(0), 0), CraftVerdict::Incomplete);
    }

    #[test]
    fn craft_verdict_zero_cost_from_on_hand_saves_everything() {
        match craft_verdict(0, Some(100), 0) {
            CraftVerdict::CraftSaves { gil, percent } => {
                assert_eq!(gil, 100);
                assert!((percent - 100.0).abs() < 1e-9);
            }
            other => panic!("expected craft saves, got {other:?}"),
        }
    }

    #[test]
    fn buy_price_takes_the_cheaper_vendor_for_nq() {
        assert_eq!(
            buy_price(false, None, Some(200), Some(68)),
            Some(BuyPrice {
                price: 68,
                source: BuySource::Vendor
            })
        );
        assert_eq!(
            buy_price(false, None, Some(50), Some(68)),
            Some(BuyPrice {
                price: 50,
                source: BuySource::Market { nq_fallback: false }
            })
        );
        assert_eq!(
            buy_price(false, None, None, Some(68)),
            Some(BuyPrice {
                price: 68,
                source: BuySource::Vendor
            })
        );
    }

    #[test]
    fn buy_price_ignores_the_vendor_for_hq() {
        assert_eq!(
            buy_price(true, Some(300), Some(200), Some(68)),
            Some(BuyPrice {
                price: 300,
                source: BuySource::Market { nq_fallback: false }
            })
        );
        assert_eq!(buy_price(true, None, None, Some(68)), None);
    }

    #[test]
    fn buy_price_falls_back_to_nq_for_hq() {
        assert_eq!(
            buy_price(true, None, Some(200), None),
            Some(BuyPrice {
                price: 200,
                source: BuySource::Market { nq_fallback: true }
            })
        );
    }
}
