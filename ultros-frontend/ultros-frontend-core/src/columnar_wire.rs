//! Struct-of-arrays serialization for the analyzer's SSR-embedded resources.
//!
//! Leptos serializes every resolved `ArcResource` into the page with
//! `JsonSerdeCodec`, which for the bulk market DTOs means megabytes of
//! repeated field names (spec §8). [`ColumnarJson`] is a drop-in codec that
//! encodes any [`ColumnarWire`] value through its columnar twin instead — the
//! same shape the `?format=columnar` API endpoints serve — and decodes it
//! back on the client. Consumers never see the twin.

use codee::{Decoder, Encoder};
use leptos::prelude::ArcResource;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};
use std::future::Future;
use ultros_api_types::{
    cheapest_listings::{CheapestListings, CheapestListingsColumnar},
    listing_stats::{BulkListingStats, BulkListingStatsColumnar},
    recent_sales::{RecentSales, RecentSalesColumnar},
    sale_stats::{BulkSaleStats, BulkSaleStatsColumnar},
};

/// A value with a struct-of-arrays twin. Containers (`Option`, `Vec`,
/// `Result`) forward to their contents so a whole resource value converts.
pub trait ColumnarWire: Sized {
    type Columnar;
    fn to_columnar(&self) -> Self::Columnar;
    fn from_columnar(columnar: Self::Columnar) -> Self;
}

macro_rules! columnar_wire {
    ($rows:ty => $cols:ty) => {
        impl ColumnarWire for $rows {
            type Columnar = $cols;
            fn to_columnar(&self) -> $cols {
                <$cols>::from(self)
            }
            fn from_columnar(columnar: $cols) -> Self {
                Self::from(columnar)
            }
        }
    };
}
columnar_wire!(RecentSales => RecentSalesColumnar);
columnar_wire!(BulkSaleStats => BulkSaleStatsColumnar);
columnar_wire!(BulkListingStats => BulkListingStatsColumnar);
columnar_wire!(CheapestListings => CheapestListingsColumnar);

impl<T: ColumnarWire> ColumnarWire for Option<T> {
    type Columnar = Option<T::Columnar>;
    fn to_columnar(&self) -> Self::Columnar {
        self.as_ref().map(T::to_columnar)
    }
    fn from_columnar(columnar: Self::Columnar) -> Self {
        columnar.map(T::from_columnar)
    }
}

impl<T: ColumnarWire> ColumnarWire for Vec<T> {
    type Columnar = Vec<T::Columnar>;
    fn to_columnar(&self) -> Self::Columnar {
        self.iter().map(T::to_columnar).collect()
    }
    fn from_columnar(columnar: Self::Columnar) -> Self {
        columnar.into_iter().map(T::from_columnar).collect()
    }
}

/// The error side is passed through untouched (it is small).
impl<T: ColumnarWire, E: Clone> ColumnarWire for Result<T, E> {
    type Columnar = Result<T::Columnar, E>;
    fn to_columnar(&self) -> Self::Columnar {
        match self {
            Ok(v) => Ok(v.to_columnar()),
            Err(e) => Err(e.clone()),
        }
    }
    fn from_columnar(columnar: Self::Columnar) -> Self {
        columnar.map(T::from_columnar)
    }
}

/// `JsonSerdeCodec` for [`ColumnarWire`] values: the wire is the columnar
/// twin, the in-memory value is the row type.
pub struct ColumnarJson;

impl<T> Encoder<T> for ColumnarJson
where
    T: ColumnarWire,
    T::Columnar: Serialize,
{
    type Error = serde_json::Error;
    type Encoded = String;
    fn encode(val: &T) -> Result<String, serde_json::Error> {
        serde_json::to_string(&val.to_columnar())
    }
}

impl<T> Decoder<T> for ColumnarJson
where
    T: ColumnarWire,
    T::Columnar: DeserializeOwned,
{
    type Error = serde_json::Error;
    type Encoded = str;
    fn decode(val: &str) -> Result<T, serde_json::Error> {
        serde_json::from_str::<T::Columnar>(val).map(T::from_columnar)
    }
}

/// `#[serde(with = "ultros_frontend_core::columnar_wire::serde_with")]` for a
/// DTO field inside a composite resource value that otherwise keeps the
/// default codec.
pub mod serde_with {
    use super::*;

    pub fn serialize<T, S>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
    where
        T: ColumnarWire,
        T::Columnar: Serialize,
        S: Serializer,
    {
        value.to_columnar().serialize(serializer)
    }

    pub fn deserialize<'de, T, D>(deserializer: D) -> Result<T, D::Error>
    where
        T: ColumnarWire,
        T::Columnar: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        T::Columnar::deserialize(deserializer).map(T::from_columnar)
    }
}

/// `ArcResource::new` with the columnar codec. Same source/fetcher contract,
/// non-blocking, SSR-serialized like every other resource.
pub fn columnar_resource<S, T, Fut>(
    source: impl Fn() -> S + Send + Sync + 'static,
    fetcher: impl Fn(S) -> Fut + Send + Sync + 'static,
) -> ArcResource<T, ColumnarJson>
where
    S: PartialEq + Clone + Send + Sync + 'static,
    T: ColumnarWire + Send + Sync + 'static,
    T::Columnar: Serialize + DeserializeOwned,
    Fut: Future<Output = T> + Send + 'static,
{
    ArcResource::<T, ColumnarJson>::new_with_options(source, fetcher, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use chrono::DateTime;
    use ultros_api_types::recent_sales::{RecentSales, SaleData, Sales};

    fn sales() -> RecentSales {
        RecentSales {
            sales: vec![SaleData {
                item_id: 5,
                hq: false,
                sales: vec![Sales {
                    price_per_unit: 7,
                    sale_date: DateTime::from_timestamp(1_699_999_000, 0)
                        .unwrap()
                        .naive_utc(),
                }],
            }],
        }
    }

    #[test]
    fn ok_result_encodes_as_columnar_json() {
        let v: Result<RecentSales, AppError> = Ok(sales());
        let json = <ColumnarJson as Encoder<Result<RecentSales, AppError>>>::encode(&v).unwrap();
        assert_eq!(
            json,
            r#"{"Ok":{"item_id":[5],"hq":[false],"count":[1],"price":[7],"sold_unix":[1699999000]}}"#
        );
        let back = <ColumnarJson as Decoder<Result<RecentSales, AppError>>>::decode(&json).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn err_result_passes_through() {
        let v: Result<RecentSales, AppError> = Err(AppError::ParamMissing);
        let json = <ColumnarJson as Encoder<Result<RecentSales, AppError>>>::encode(&v).unwrap();
        assert_eq!(
            json,
            serde_json::to_string(&Err::<(), _>(AppError::ParamMissing)).unwrap()
        );
        assert_eq!(
            <ColumnarJson as Decoder<Result<RecentSales, AppError>>>::decode(&json).unwrap(),
            v
        );
    }

    #[test]
    fn option_and_vec_nest() {
        let v: Option<Vec<RecentSales>> = Some(vec![sales(), RecentSales { sales: vec![] }]);
        let json = <ColumnarJson as Encoder<Option<Vec<RecentSales>>>>::encode(&v).unwrap();
        assert!(json.starts_with(r#"[{"item_id":[5]"#), "{json}");
        assert_eq!(
            <ColumnarJson as Decoder<Option<Vec<RecentSales>>>>::decode(&json).unwrap(),
            v
        );
        let none: Option<Vec<RecentSales>> = None;
        assert_eq!(
            <ColumnarJson as Encoder<Option<Vec<RecentSales>>>>::encode(&none).unwrap(),
            "null"
        );
    }

    #[test]
    fn serde_with_annotates_struct_fields() {
        #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
        struct Composite {
            #[serde(with = "super::serde_with")]
            raw: Option<RecentSales>,
            flag: bool,
        }
        let c = Composite {
            raw: Some(sales()),
            flag: true,
        };
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(
            json,
            r#"{"raw":{"item_id":[5],"hq":[false],"count":[1],"price":[7],"sold_unix":[1699999000]},"flag":true}"#
        );
        assert_eq!(serde_json::from_str::<Composite>(&json).unwrap(), c);
    }
}
