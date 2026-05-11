use std::{fmt::Display, iter};

use axum::{
    extract::OptionalFromRequestParts,
    http::{HeaderName, HeaderValue, request::Parts},
    response::IntoResponse,
};
use axum_extra::{TypedHeader, headers::Header, typed_header::TypedHeaderRejection};
use isocountry::CountryCode;

#[derive(Debug, Copy, Clone)]
pub(crate) enum Region {
    Japan,
    NorthAmerica,
    Europe,
    Oceania,
    China,
    Korea,
}

impl IntoResponse for Region {
    fn into_response(self) -> axum::response::Response {
        self.as_str().into_response()
    }
}

struct CloudflareCountryCode(CountryCode);

static CFCOUNTRY_CODE: HeaderName = HeaderName::from_static("cf-ipcountry");

impl Header for CloudflareCountryCode {
    fn name() -> &'static axum::http::HeaderName {
        // "CF-IPCountry"
        &CFCOUNTRY_CODE
    }

    fn decode<'i, I>(values: &mut I) -> Result<Self, axum_extra::headers::Error>
    where
        Self: Sized,
        I: Iterator<Item = &'i axum::http::HeaderValue>,
    {
        let value = values
            .next()
            .and_then(|value| {
                value
                    .to_str()
                    .ok()
                    .and_then(|value| CountryCode::for_alpha2_caseless(value).ok())
            })
            .map(CloudflareCountryCode);
        value.ok_or(axum_extra::headers::Error::invalid())
    }

    fn encode<E: Extend<axum::http::HeaderValue>>(&self, values: &mut E) {
        values.extend(iter::once(HeaderValue::from_static(self.0.alpha2())));
    }
}

// #[async_trait]
impl<S: Sized + Send + Sync> OptionalFromRequestParts<S> for Region {
    type Rejection = TypedHeaderRejection;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> Result<Option<Self>, Self::Rejection> {
        let result = TypedHeader::<CloudflareCountryCode>::from_request_parts(parts, state)
            .await?
            .map(|typed_header| typed_header.0.0.into());
        Ok(result)
    }
}

impl Region {
    fn as_str(&self) -> &'static str {
        match self {
            Region::Japan => "Japan",
            Region::NorthAmerica => "North-America",
            Region::Europe => "Europe",
            Region::Oceania => "Oceania",
            Region::China => "中国",
            Region::Korea => "한국",
        }
    }
}

impl AsRef<str> for Region {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Display for Region {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<CountryCode> for Region {
    fn from(value: CountryCode) -> Self {
        match value {
            CountryCode::USA | CountryCode::MEX | CountryCode::CAN => Region::NorthAmerica,
            CountryCode::JPN => Region::Japan,
            CountryCode::KOR => Region::Korea,
            CountryCode::CHN => Region::China,
            CountryCode::AUS
            | CountryCode::NZL
            | CountryCode::FJI
            | CountryCode::GUM
            | CountryCode::WSM
            | CountryCode::PNG
            | CountryCode::TON
            | CountryCode::PLW
            | CountryCode::NCL
            | CountryCode::TUV => Region::Oceania,
            // I'm sort of assuming there's more
            _ => Region::Europe,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_region(country: CountryCode, expected: &str) {
        let r: Region = country.into();
        assert_eq!(r.as_str(), expected, "country {country:?}");
    }

    #[test]
    fn north_america_countries_map_to_north_america() {
        for c in [CountryCode::USA, CountryCode::CAN, CountryCode::MEX] {
            assert_region(c, "North-America");
        }
    }

    #[test]
    fn japan_maps_to_japan() {
        assert_region(CountryCode::JPN, "Japan");
    }

    #[test]
    fn korea_maps_to_korea() {
        let r: Region = CountryCode::KOR.into();
        assert!(matches!(r, Region::Korea));
    }

    #[test]
    fn china_maps_to_china() {
        let r: Region = CountryCode::CHN.into();
        assert!(matches!(r, Region::China));
    }

    #[test]
    fn oceania_countries_map_to_oceania() {
        for c in [
            CountryCode::AUS,
            CountryCode::NZL,
            CountryCode::FJI,
            CountryCode::GUM,
            CountryCode::WSM,
            CountryCode::PNG,
            CountryCode::TON,
            CountryCode::PLW,
            CountryCode::NCL,
            CountryCode::TUV,
        ] {
            assert_region(c, "Oceania");
        }
    }

    #[test]
    fn unknown_country_defaults_to_europe() {
        // Pick a random European country and a few not handled above.
        for c in [
            CountryCode::DEU,
            CountryCode::FRA,
            CountryCode::GBR,
            CountryCode::IND, // India — not in any explicit branch
            CountryCode::BRA, // Brazil — falls into Europe via the catch-all (documented quirk)
        ] {
            assert_region(c, "Europe");
        }
    }

    #[test]
    fn display_renders_region_name() {
        assert_eq!(Region::Japan.to_string(), "Japan");
        assert_eq!(Region::NorthAmerica.to_string(), "North-America");
        assert_eq!(Region::Europe.to_string(), "Europe");
        assert_eq!(Region::Oceania.to_string(), "Oceania");
        // The Chinese and Korean display strings are localized.
        assert_eq!(Region::China.to_string(), "中国");
        assert_eq!(Region::Korea.to_string(), "한국");
    }

    #[test]
    fn as_ref_matches_as_str() {
        assert_eq!(
            AsRef::<str>::as_ref(&Region::NorthAmerica),
            "North-America"
        );
    }
}
