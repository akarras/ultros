use std::iter;

use async_trait::async_trait;
use axum::{
    extract::FromRequestParts,
    http::{request::Parts, HeaderName, HeaderValue},
    response::IntoResponse,
};
use axum_extra::{headers::Header, typed_header::TypedHeaderRejection, TypedHeader};
use isocountry::CountryCode;
use thiserror::Error;

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

#[derive(Error, Debug)]
enum CountryCodeError {
    #[error("Country code was not found")]
    NotFound,
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

#[async_trait]
impl<S: Sized + Send + Sync> FromRequestParts<S> for Region {
    type Rejection = TypedHeaderRejection;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let TypedHeader(country) =
            TypedHeader::<CloudflareCountryCode>::from_request_parts(parts, state).await?;
        Ok(country.0.into())
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

impl ToString for Region {
    fn to_string(&self) -> String {
        self.as_str().to_string()
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
