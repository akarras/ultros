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
    type Err = ();

    fn from_str(_s: &str) -> Result<Self, Self::Err> {
        panic!("Unused");
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

        let (primary_key, secondary) = str.split_once(".").ok_or(Error::custom("Invalid str"))?;
        let primary_key = match primary_key.parse() {
            Ok(v) => v,
            Err(_e) => panic!("Primary key failed to parse?"),
        };
        Ok(Self(primary_key, secondary.parse().unwrap()))
    }
}
