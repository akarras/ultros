#![allow(unused)]
#[cfg(feature = "csv_to_bincode")]
pub mod csv_to_bincode;

mod deserialize_custom;
pub mod subrow_key;

use bincode::{Decode, Encode, config::Config};
use deserialize_custom::*;
use dumb_csv::ParseBool;
use serde::{Deserialize, Deserializer, Serialize};

include!(concat!(env!("OUT_DIR"), "/types.rs"));

pub fn bincode_config() -> impl Config {
    bincode::config::standard()
}

pub fn data_version() -> &'static str {
    // TODO somehow get a macro to get the HASH of ffxiv-datamining?
    env!("GIT_HASH")
}

fn ok_or_default<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: Deserialize<'de> + Default,
    D: Deserializer<'de>,
{
    Ok(T::deserialize(deserializer).unwrap_or_default())
}

#[cfg(test)]
mod tests {}
