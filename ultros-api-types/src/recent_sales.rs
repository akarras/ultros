use chrono::{DateTime, NaiveDateTime};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sales {
    pub price_per_unit: i32,
    pub sale_date: NaiveDateTime,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaleData {
    pub item_id: i32,
    pub hq: bool,
    pub sales: Vec<Sales>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentSales {
    pub sales: Vec<SaleData>,
}

/// Struct-of-arrays wire form of [`RecentSales`] — what
/// `/api/v1/recentSales/{world}?format=columnar` returns. Row `i` is
/// `(item_id[i], hq[i])` and owns the next `count[i]` entries of the
/// flattened `price` / `sold_unix` columns, newest first, exactly as the
/// row shape orders them. Timestamps are unix seconds; the row shape's
/// `NaiveDateTime` is second-resolution so the conversion is lossless.
/// Rows are emitted in `(item_id, hq)` order, which is what makes the
/// columns compress well; decoding does not depend on the order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RecentSalesColumnar {
    pub item_id: Vec<i32>,
    pub hq: Vec<bool>,
    pub count: Vec<u32>,
    pub price: Vec<i32>,
    pub sold_unix: Vec<i64>,
}

impl From<&RecentSales> for RecentSalesColumnar {
    fn from(value: &RecentSales) -> Self {
        let n = value.sales.len();
        let total: usize = value.sales.iter().map(|s| s.sales.len()).sum();
        let mut out = Self {
            item_id: Vec::with_capacity(n),
            hq: Vec::with_capacity(n),
            count: Vec::with_capacity(n),
            price: Vec::with_capacity(total),
            sold_unix: Vec::with_capacity(total),
        };
        for row in &value.sales {
            out.item_id.push(row.item_id);
            out.hq.push(row.hq);
            out.count.push(row.sales.len() as u32);
            for sale in &row.sales {
                out.price.push(sale.price_per_unit);
                out.sold_unix.push(sale.sale_date.and_utc().timestamp());
            }
        }
        out
    }
}

impl From<RecentSales> for RecentSalesColumnar {
    fn from(value: RecentSales) -> Self {
        Self::from(&value)
    }
}

impl From<RecentSalesColumnar> for RecentSales {
    /// Zips the key columns and walks the flattened sale columns with a
    /// cursor. A malformed payload degrades rather than panicking: key
    /// columns of unequal length truncate to the shortest, and a `count`
    /// that outruns the flattened columns yields the sales that exist.
    fn from(value: RecentSalesColumnar) -> Self {
        let mut prices = value.price.into_iter();
        let mut dates = value.sold_unix.into_iter();
        let sales = value
            .item_id
            .into_iter()
            .zip(value.hq)
            .zip(value.count)
            .map(|((item_id, hq), count)| {
                let sales = (0..count)
                    .map_while(|_| {
                        Some(Sales {
                            price_per_unit: prices.next()?,
                            sale_date: unix_to_naive(dates.next()?),
                        })
                    })
                    .collect();
                SaleData { item_id, hq, sales }
            })
            .collect();
        Self { sales }
    }
}

/// Out-of-range seconds fall back to the epoch instead of failing the
/// whole payload.
fn unix_to_naive(secs: i64) -> NaiveDateTime {
    DateTime::from_timestamp(secs, 0)
        .unwrap_or_default()
        .naive_utc()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;

    fn at(secs: i64) -> NaiveDateTime {
        DateTime::from_timestamp(secs, 0).unwrap().naive_utc()
    }

    fn sale(price: i32, secs: i64) -> Sales {
        Sales {
            price_per_unit: price,
            sale_date: at(secs),
        }
    }

    fn rows() -> RecentSales {
        RecentSales {
            sales: vec![
                SaleData {
                    item_id: 2,
                    hq: false,
                    sales: vec![],
                },
                SaleData {
                    item_id: 2,
                    hq: true,
                    sales: vec![
                        sale(90, 1_700_000_300),
                        sale(88, 1_700_000_200),
                        sale(87, 1_700_000_100),
                    ],
                },
                SaleData {
                    item_id: 5,
                    hq: false,
                    sales: vec![sale(7, 1_699_999_000)],
                },
            ],
        }
    }

    #[test]
    fn columnar_round_trips_ragged_rows_in_order() {
        let columnar = RecentSalesColumnar::from(rows());
        assert_eq!(columnar.item_id, vec![2, 2, 5]);
        assert_eq!(columnar.hq, vec![false, true, false]);
        assert_eq!(columnar.count, vec![0, 3, 1]);
        assert_eq!(columnar.price, vec![90, 88, 87, 7]);
        assert_eq!(
            columnar.sold_unix,
            vec![1_700_000_300, 1_700_000_200, 1_700_000_100, 1_699_999_000]
        );
        assert_eq!(RecentSales::from(columnar), rows());
    }

    #[test]
    fn columnar_serializes_as_five_arrays() {
        let columnar = RecentSalesColumnar::from(RecentSales {
            sales: vec![SaleData {
                item_id: 5,
                hq: false,
                sales: vec![sale(7, 1_699_999_000)],
            }],
        });
        let json = serde_json::to_string(&columnar).unwrap();
        assert_eq!(
            json,
            r#"{"item_id":[5],"hq":[false],"count":[1],"price":[7],"sold_unix":[1699999000]}"#
        );
        let back: RecentSalesColumnar = serde_json::from_str(&json).unwrap();
        assert_eq!(back, columnar);
    }

    #[test]
    fn columnar_empty_round_trips() {
        assert_eq!(
            RecentSales::from(RecentSalesColumnar::default()).sales,
            vec![]
        );
        assert_eq!(
            RecentSalesColumnar::from(RecentSales { sales: vec![] }),
            RecentSalesColumnar::default()
        );
    }

    #[test]
    fn columnar_mismatched_key_columns_truncate_to_shortest() {
        let columnar = RecentSalesColumnar {
            item_id: vec![1, 2, 3],
            hq: vec![false, true],
            count: vec![1, 1, 1],
            price: vec![10, 20, 30],
            sold_unix: vec![100, 200, 300],
        };
        let rows = RecentSales::from(columnar);
        assert_eq!(rows.sales.len(), 2);
        assert_eq!(rows.sales[0].sales, vec![sale(10, 100)]);
        assert_eq!(rows.sales[1].sales, vec![sale(20, 200)]);
    }

    #[test]
    fn columnar_count_beyond_flattened_columns_yields_what_exists() {
        // `count` promises 3 + 2 sales but only 4 (price) / 3 (sold_unix) exist.
        let columnar = RecentSalesColumnar {
            item_id: vec![1, 2],
            hq: vec![false, false],
            count: vec![3, 2],
            price: vec![10, 11, 12, 20],
            sold_unix: vec![100, 101, 102],
        };
        let rows = RecentSales::from(columnar);
        assert_eq!(rows.sales.len(), 2);
        assert_eq!(
            rows.sales[0].sales,
            vec![sale(10, 100), sale(11, 101), sale(12, 102)]
        );
        assert_eq!(rows.sales[1].sales, vec![]);
    }

    #[test]
    fn columnar_from_ref_matches_from_value() {
        let rows = rows();
        assert_eq!(
            RecentSalesColumnar::from(&rows),
            RecentSalesColumnar::from(rows.clone())
        );
    }

    #[test]
    fn out_of_range_timestamp_falls_back_to_epoch() {
        let columnar = RecentSalesColumnar {
            item_id: vec![1],
            hq: vec![false],
            count: vec![1],
            price: vec![10],
            sold_unix: vec![i64::MAX],
        };
        let rows = RecentSales::from(columnar);
        assert_eq!(rows.sales[0].sales, vec![sale(10, 0)]);
    }
}
