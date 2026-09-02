//! The profit ledger: which signal feeds each side of
//! `profit = revenue − tax − cost`, and the per-tool policies that make
//! every analyzer's numbers reproducible from one function.

use std::fmt::{self, Display};
use std::str::FromStr;

/// Which sale-history statistic a signal reads from an
/// [`ultros_api_types::sale_stats::ItemSaleStats`] row.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum SaleStat {
    Min,
    Median,
    Avg,
}

/// A price signal: the cheapest current listing, or a trailing-window sale
/// statistic. The same four signals price ingredients (over the buy scope)
/// and revenue (on the sell world); the URL tokens are the `cost-basis` /
/// `revenue` bookmark contract and never change.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum PriceSignal {
    #[default]
    ListingMin,
    SaleMin,
    SaleMedian,
    SaleAvg,
}

/// Ingredient-side name for [`PriceSignal`], kept so the route reads as
/// it did when the two sides were separate enums.
pub type CostBasis = PriceSignal;
/// Revenue-side name for [`PriceSignal`].
pub type RevenueMetric = PriceSignal;

impl PriceSignal {
    /// The sale statistic this signal reads, or `None` for the listing.
    pub fn sale_stat(self) -> Option<SaleStat> {
        match self {
            PriceSignal::ListingMin => None,
            PriceSignal::SaleMin => Some(SaleStat::Min),
            PriceSignal::SaleMedian => Some(SaleStat::Median),
            PriceSignal::SaleAvg => Some(SaleStat::Avg),
        }
    }
}

impl FromStr for PriceSignal {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "listing-min" => Ok(PriceSignal::ListingMin),
            "sale-median" => Ok(PriceSignal::SaleMedian),
            "sale-min" => Ok(PriceSignal::SaleMin),
            "sale-avg" => Ok(PriceSignal::SaleAvg),
            _ => Err(()),
        }
    }
}

impl Display for PriceSignal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PriceSignal::ListingMin => "listing-min",
            PriceSignal::SaleMedian => "sale-median",
            PriceSignal::SaleMin => "sale-min",
            PriceSignal::SaleAvg => "sale-avg",
        })
    }
}

/// Where ingredient prices are searched: the sell world only, its
/// datacenter, or the whole region. Defaults to the datacenter.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum BuyScope {
    World,
    #[default]
    Datacenter,
    Region,
}

impl FromStr for BuyScope {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "world" => Ok(BuyScope::World),
            "datacenter" => Ok(BuyScope::Datacenter),
            "region" => Ok(BuyScope::Region),
            _ => Err(()),
        }
    }
}

impl Display for BuyScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            BuyScope::World => "world",
            BuyScope::Datacenter => "datacenter",
            BuyScope::Region => "region",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_signal_tokens_are_unchanged() {
        // These four tokens are the `cost-basis` / `revenue` URL contract.
        assert_eq!(PriceSignal::ListingMin.to_string(), "listing-min");
        assert_eq!(PriceSignal::SaleMedian.to_string(), "sale-median");
        assert_eq!(PriceSignal::SaleMin.to_string(), "sale-min");
        assert_eq!(PriceSignal::SaleAvg.to_string(), "sale-avg");
        for s in ["listing-min", "sale-median", "sale-min", "sale-avg"] {
            assert_eq!(s.parse::<PriceSignal>().unwrap().to_string(), s);
        }
        assert!("world-min".parse::<PriceSignal>().is_err());
        assert_eq!(PriceSignal::default(), PriceSignal::ListingMin);
        assert_eq!(PriceSignal::ListingMin.sale_stat(), None);
        assert_eq!(PriceSignal::SaleMedian.sale_stat(), Some(SaleStat::Median));
    }

    #[test]
    fn aliases_are_the_same_type() {
        let cost: CostBasis = PriceSignal::SaleMin;
        let revenue: RevenueMetric = cost;
        assert_eq!(revenue, PriceSignal::SaleMin);
    }
}
