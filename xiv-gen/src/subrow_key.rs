use bincode::{Decode, Encode};
use core::str::FromStr;
use serde::Deserialize;
use serde::Serialize;
use serde::de::Error;
use std::fmt::Debug;

#[derive(Serialize, Hash, Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Decode, Encode)]
pub struct SubrowKey<T>(pub T, pub i32);

impl<T> Default for SubrowKey<T>
where
    T: Default,
{
    fn default() -> Self {
        Self(T::default(), 0)
    }
}

impl<T> FromStr for SubrowKey<T>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (primary, secondary) = s.split_once('.').ok_or_else(|| "Invalid format: missing dot".to_string())?;
        let p = T::from_str(primary).map_err(|e| e.to_string())?;
        let sec = i32::from_str(secondary).map_err(|e| e.to_string())?;
        Ok(Self(p, sec))
    }
}

impl<'de, T> Deserialize<'de> for SubrowKey<T>
where
    T: FromStr + Debug,
    T::Err: std::fmt::Display,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let str = <&str>::deserialize(deserializer)?;
        Self::from_str(str).map_err(Error::custom)
    }
}
