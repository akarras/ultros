use serde::{Deserialize, Deserializer};

#[allow(dead_code)]
pub fn deserialize_i64_from_u8_array<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    let s: [u8; 8] = Deserialize::deserialize(deserializer)?;
    Ok(i64::from_be_bytes(s))
}

#[allow(dead_code)]
pub fn deserialize_bool_from_anything_custom<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    Ok(s == "True" || s == "true" || s == "1")
}
