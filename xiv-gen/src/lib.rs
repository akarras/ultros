#![allow(unused_imports)]
#[cfg(feature = "csv_to_bincode")]
pub mod csv_to_bincode;
pub mod deserialize_custom;
#[cfg(feature = "csv_to_bincode")]
pub mod dumb_csv_reader;
pub mod subrow_key;

use deserialize_custom::*;
use dumb_csv::ParseBool;

use bincode::{Decode, Encode};
use serde::Deserialize;
use serde::Serialize;
use std::fmt::Display;

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Hash, Deserialize)]
pub struct ItemId(pub i32);

impl Display for ItemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub fn bincode_config() -> bincode::config::Configuration {
    bincode::config::standard()
}

#[allow(dead_code)]
fn ok_or_default<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: Deserialize<'de> + Default,
    D: serde::Deserializer<'de>,
{
    let v: Result<T, D::Error> = T::deserialize(deserializer);
    Ok(v.unwrap_or_default())
}

include!(concat!(env!("OUT_DIR"), "/types.rs"));
