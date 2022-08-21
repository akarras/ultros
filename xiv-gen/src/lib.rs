mod custom_bool_deserialize;

use crate::custom_bool_deserialize::{
    deserialize_bool_from_anything_custom, deserialize_i64_from_u8_array,
};
use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};

include!(concat!(env!("OUT_DIR"), "/types.rs"));

#[cfg(test)]
mod tests {
    

}
