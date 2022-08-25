#[cfg(feature = "csv_to_bincode")]
pub mod csv_to_bincode;

mod deserialize_custom;

use bincode::{Decode, Encode};
use deserialize_custom::*;
use serde::{Deserialize, Serialize};

include!(concat!(env!("OUT_DIR"), "/types.rs"));

#[cfg(test)]
mod tests {}
