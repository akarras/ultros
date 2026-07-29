//! Query-string helpers shared by the JSON API handlers.

use serde::{Deserialize, Deserializer, de};

/// Deserialize an optional boolean flag that accepts both the `1`/`0` and
/// `true`/`false` spellings.
///
/// `axum::extract::Query` decodes with `serde_urlencoded`, whose `bool`
/// deserializer is `str::parse::<bool>()` — it accepts *only* the literals
/// `true` and `false`. A plain `Option<bool>` field therefore fails on
/// `?show_suspicious=0`, and a `Query` rejection rejects the **whole
/// request** with a `400`; it does not fall back to the field default. Both
/// the API design doc (`?window=7|30|90&show_suspicious=0|1`) and the
/// frontend spell these flags numerically, so `0`/`1` has to parse.
///
/// A valueless `?flag=` carries no opinion and yields `None`, which leaves
/// the caller's default in place. Anything else is a genuine client mistake
/// and is reported as such rather than silently defaulted.
pub(crate) fn optional_flag<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    // `#[serde(default, ...)]` covers the absent-key case without calling this
    // function at all, so a present key always carries a string value here.
    let raw = String::deserialize(deserializer)?;
    let value = raw.trim();
    if value.is_empty() {
        return Ok(None);
    }
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" => Ok(Some(true)),
        "0" | "false" => Ok(Some(false)),
        other => Err(de::Error::custom(format!(
            "invalid boolean flag {other:?}: expected one of 1, 0, true, false"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::IntoDeserializer;
    use serde::de::value::{Error as ValueError, StringDeserializer};

    fn parse(raw: &str) -> Result<Option<bool>, ValueError> {
        let deserializer: StringDeserializer<ValueError> = raw.to_string().into_deserializer();
        optional_flag(deserializer)
    }

    #[test]
    fn accepts_numeric_spelling() {
        assert_eq!(parse("1").unwrap(), Some(true));
        assert_eq!(parse("0").unwrap(), Some(false));
    }

    #[test]
    fn accepts_literal_spelling() {
        assert_eq!(parse("true").unwrap(), Some(true));
        assert_eq!(parse("false").unwrap(), Some(false));
    }

    #[test]
    fn accepts_mixed_case_and_surrounding_space() {
        assert_eq!(parse("True").unwrap(), Some(true));
        assert_eq!(parse("FALSE").unwrap(), Some(false));
        assert_eq!(parse(" 1 ").unwrap(), Some(true));
    }

    #[test]
    fn valueless_flag_defers_to_the_default() {
        assert_eq!(parse("").unwrap(), None);
    }

    #[test]
    fn rejects_nonsense() {
        let err = parse("banana").unwrap_err().to_string();
        assert!(
            err.contains("expected one of 1, 0, true, false"),
            "error should name the accepted spellings, got: {err}"
        );
    }
}
