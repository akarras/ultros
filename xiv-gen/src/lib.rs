#[cfg(feature = "csv_to_bincode")]
pub mod csv_to_bincode;

mod deserialize_custom;
pub mod subrow_key;

use bincode::config::Config;
use serde::{Deserialize, Deserializer};

pub mod types {
    #![allow(clippy::all)]
    #![allow(unused_imports)]
    use serde::{Deserialize, Serialize};
    use bincode::{Decode, Encode};

    include!(concat!(env!("OUT_DIR"), "/types.rs"));
}
pub use types::*;

pub fn bincode_config() -> impl Config {
    bincode::config::standard()
}

pub fn data_version() -> &'static str {
    // TODO somehow get a macro to get the HASH of ffxiv-datamining?
    env!("GIT_HASH")
}

#[allow(dead_code)]
fn ok_or_default<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: Deserialize<'de> + Default,
    D: Deserializer<'de>,
{
    Ok(T::deserialize(deserializer).unwrap_or_default())
}

#[cfg(test)]
mod tests {}
