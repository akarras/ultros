use bincode::{Decode, Encode};
use core::str::FromStr;
use serde::Serialize;
use serde::{
    de::{DeserializeOwned, IntoDeserializer},
    Deserialize,
};
use std::fmt::Debug;

#[derive(Serialize, Hash, Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Decode, Encode)]
pub struct SubrowKey<T>(T, i32);

impl<T> FromStr for SubrowKey<T>
where
    T: FromStr,
{
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
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
        // this only happens at compile time so being lazy with errors
        let (primary_key, secondary) = str.split_once(".").unwrap();
        let primary_key = match primary_key.parse() {
            Ok(v) => v,
            Err(e) => panic!("Primary key failed to parse?")
        };
        Ok(Self(
            primary_key,
            secondary.parse().unwrap(),
        ))
    }
}
