#[cfg(feature = "csv_to_bincode")]
pub mod csv_to_bincode;

mod deserialize_custom;
pub mod subrow_key;

use bincode::{config::Config};
#[allow(unused_imports)]
use deserialize_custom::*;
#[allow(unused_imports)]
use dumb_csv::ParseBool;
use serde::{Deserialize, Deserializer};

#[allow(unused_imports)]
mod types {
    use serde::{Serialize, Deserialize};
    use bincode::{Encode, Decode};

    // The generated code (types.rs) already contains imports for:
    // - std::collections::HashMap
    // - crate::subrow_key::SubrowKey
    // - derive_more::FromStr
    // - dumb_csv::DumbCsvDeserialize
    // So we should NOT import them here to avoid E0252 (reimport conflict)

    // However, it seems to rely on `crate::deserialize_custom::*` which isn't imported in generated code.
    use crate::deserialize_custom::*;

    // Check if `read_csv` and `read_dumb_csv` are needed.
    // They are used in the generated code for the `read_data` function (if args.read_data is generated)
    // The error says `dumb_csv::read_csv` doesn't exist. Let's check dumb_csv crate if needed,
    // but for now let's assume the generated code uses fully qualified paths or expects them in scope.
    // Actually, the build.rs generates `read_csv::<Struct>(...)`.
    // `dumb_csv` crate likely exposes `read_csv` at top level or we need to fix the path.
    // But since I can't verify dumb-csv content easily without reading it, and the error was `no read_csv in the root`,
    // it implies `dumb_csv` might need `use dumb_csv::*;` or similar if it re-exports.

    // Wait, the generated code uses `read_csv`.
    // Let's remove the explicit imports that collide and see.
    // Also `dumb_csv` might be available as a crate alias.

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
