use serde::de::Error;
use serde::Deserialize;

#[allow(dead_code)]
pub fn deserialize_i64_from_u8_array<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: &[u8] = Deserialize::deserialize(deserializer)?;
    // do better hex decoding than this
    let s = std::str::from_utf8(s).map_err(D::Error::custom)?;
    let mut value = 0i64;
    for &byte in s.as_bytes() {
        value <<= 8;
        value |= byte as i64;
    }
    Ok(value)
}

#[allow(dead_code)]
pub fn deserialize_bool_from_anything_custom<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    match s.as_str() {
        "True" | "TRUE" | "true" | "t" | "1" => Ok(true),
        "False" | "FALSE" | "false" | "f" | "0" | "" => Ok(false),
        _ => Err(D::Error::custom(format!("invalid bool: {s}"))),
    }
}
