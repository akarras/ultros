//! List sort order shared by the account and device carts.
use std::{fmt, str::FromStr};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Price,
    Acquired,
}

/// Sort order for the list item table, encoded in the `sort` query param as
/// `name`, `name-desc`, `price`, `price-desc`, `acquired`, or `acquired-desc`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortSpec {
    pub key: SortKey,
    pub descending: bool,
}

impl FromStr for SortSpec {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (base, descending) = match s.strip_suffix("-desc") {
            Some(base) => (base, true),
            None => (s, false),
        };
        let key = match base {
            "name" => SortKey::Name,
            "price" => SortKey::Price,
            "acquired" => SortKey::Acquired,
            _ => return Err(()),
        };
        Ok(SortSpec { key, descending })
    }
}

impl fmt::Display for SortSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let base = match self.key {
            SortKey::Name => "name",
            SortKey::Price => "price",
            SortKey::Acquired => "acquired",
        };
        write!(f, "{base}")?;
        if self.descending {
            write!(f, "-desc")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_spec_round_trips_through_query_param_encoding() {
        for encoded in [
            "name",
            "name-desc",
            "price",
            "price-desc",
            "acquired",
            "acquired-desc",
        ] {
            let spec: SortSpec = encoded.parse().unwrap();
            assert_eq!(spec.to_string(), encoded);
        }
        assert!("bogus".parse::<SortSpec>().is_err());
        assert!("".parse::<SortSpec>().is_err());
    }
}
