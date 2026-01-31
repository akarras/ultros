#[cfg(feature = "csv_to_bincode")]
pub mod csv_to_bincode;

mod deserialize_custom;
pub mod subrow_key;

use bincode::{Decode, Encode, config::Config};
use serde::{Deserialize, Serialize};

include!(concat!(env!("OUT_DIR"), "/types.rs"));

pub fn bincode_config() -> impl Config {
    bincode::config::standard()
}

pub fn data_version() -> &'static str {
    // TODO somehow get a macro to get the HASH of ffxiv-datamining?
    env!("GIT_HASH")
}

#[cfg(test)]
mod tests {}
