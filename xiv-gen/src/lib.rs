#[cfg(feature = "csv_to_bincode")]
pub mod csv_to_bincode;

mod deserialize_custom;

use bincode::{config::Config, Decode, Encode};
use deserialize_custom::*;
use serde::{Deserialize, Serialize};

include!(concat!(env!("OUT_DIR"), "/types.rs"));

pub fn bincode_config() -> impl Config {
    bincode::config::standard()
}

pub fn data_version() -> &'static str {
    // TODO somehow get a macro to get the HASH of ffxiv-datamining?
    "0.0.1"
}

#[cfg(test)]
mod tests {}
