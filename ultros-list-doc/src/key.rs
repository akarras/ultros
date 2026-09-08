//! Row identity: the natural key the server already dedupes on.

use std::fmt;
use std::str::FromStr;

/// Mirrors `ListItem.hq`: `None` is "any", `Some(true)` HQ, `Some(false)` NQ.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Quality {
    Any,
    Hq,
    Nq,
}

impl Quality {
    pub fn as_str(self) -> &'static str {
        match self {
            Quality::Any => "any",
            Quality::Hq => "hq",
            Quality::Nq => "nq",
        }
    }
}

impl From<Option<bool>> for Quality {
    fn from(hq: Option<bool>) -> Self {
        match hq {
            None => Quality::Any,
            Some(true) => Quality::Hq,
            Some(false) => Quality::Nq,
        }
    }
}

impl From<Quality> for Option<bool> {
    fn from(quality: Quality) -> Self {
        match quality {
            Quality::Any => None,
            Quality::Hq => Some(true),
            Quality::Nq => Some(false),
        }
    }
}

impl FromStr for Quality {
    type Err = KeyError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "any" => Ok(Quality::Any),
            "hq" => Ok(Quality::Hq),
            "nq" => Ok(Quality::Nq),
            other => Err(KeyError::Quality(other.to_string())),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RowKey {
    pub item_id: i32,
    pub quality: Quality,
}

impl RowKey {
    pub fn new(item_id: i32, hq: Option<bool>) -> Self {
        Self {
            item_id,
            quality: Quality::from(hq),
        }
    }

    pub fn hq(&self) -> Option<bool> {
        self.quality.into()
    }
}

impl fmt::Display for RowKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.item_id, self.quality.as_str())
    }
}

impl FromStr for RowKey {
    type Err = KeyError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (item, quality) = s
            .split_once(':')
            .ok_or_else(|| KeyError::Shape(s.to_string()))?;
        let item_id = item
            .parse::<i32>()
            .map_err(|_| KeyError::ItemId(item.to_string()))?;
        Ok(Self {
            item_id,
            quality: quality.parse()?,
        })
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum KeyError {
    #[error("row key `{0}` is not `item:quality`")]
    Shape(String),
    #[error("row key item id `{0}` is not a number")]
    ItemId(String),
    #[error("row key quality `{0}` is not any, hq or nq")]
    Quality(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_round_trips_through_display_for_every_quality() {
        for hq in [None, Some(true), Some(false)] {
            let key = RowKey::new(4567, hq);
            let text = key.to_string();
            let parsed: RowKey = text.parse().unwrap();
            assert_eq!(parsed, key, "{text}");
            assert_eq!(parsed.hq(), hq);
        }
        assert_eq!(RowKey::new(1, None).to_string(), "1:any");
        assert_eq!(RowKey::new(1, Some(true)).to_string(), "1:hq");
        assert_eq!(RowKey::new(1, Some(false)).to_string(), "1:nq");
    }

    #[test]
    fn keys_order_by_item_then_quality() {
        let mut keys = vec![
            RowKey::new(2, None),
            RowKey::new(1, Some(false)),
            RowKey::new(1, Some(true)),
            RowKey::new(1, None),
        ];
        keys.sort();
        assert_eq!(
            keys,
            vec![
                RowKey::new(1, None),
                RowKey::new(1, Some(true)),
                RowKey::new(1, Some(false)),
                RowKey::new(2, None),
            ]
        );
    }

    #[test]
    fn junk_keys_are_rejected_with_the_right_error() {
        assert_eq!(
            "nocolon".parse::<RowKey>(),
            Err(KeyError::Shape("nocolon".into()))
        );
        assert_eq!("x:hq".parse::<RowKey>(), Err(KeyError::ItemId("x".into())));
        assert_eq!(
            "5:shiny".parse::<RowKey>(),
            Err(KeyError::Quality("shiny".into()))
        );
    }
}
