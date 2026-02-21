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
{
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (primary_key, secondary) = s
            .split_once('.')
            .ok_or_else(|| format!("Invalid SubrowKey: {}", s))?;
        let primary_key = primary_key
            .parse()
            .map_err(|_| format!("Failed to parse primary key in SubrowKey"))?;
        let secondary = secondary
            .parse()
            .map_err(|_| format!("Failed to parse secondary key in SubrowKey"))?;
        Ok(Self(primary_key, secondary))
    }
}

impl<'de, T> Deserialize<'de> for SubrowKey<T>
where
    T: FromStr + Debug,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let str = <&str>::deserialize(deserializer)?;
        Self::from_str(str).map_err(D::Error::custom)
    }
}
