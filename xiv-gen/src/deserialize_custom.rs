use serde::{Deserialize, Deserializer};

pub fn deserialize_i64_from_u8_array<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    struct I64Slice([u16; 4]);
    let _ = String::deserialize(deserializer);
    // let slice = I64Slice::deserialize(deserializer)?;
    // let bytes : Vec<_> = slice.0.into_iter().map(|m| m.to_le_bytes()).flatten().collect();
    //let bytes : [i8; 8] = bytes.as_slice();
    // TODO, no idea if this is little-endian or big-endian. Might need better conversions
    //let i64 = i64::from_be_bytes(slice);
    // TODO properly implement
    Ok(0)
}

pub fn deserialize_bool_from_anything_custom<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    use std::f64::EPSILON;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum AnythingOrBool {
        String(String),
        Int(i64),
        Float(f64),
        Boolean(bool),
    }

    match AnythingOrBool::deserialize(deserializer)? {
        AnythingOrBool::Boolean(b) => Ok(b),
        AnythingOrBool::Int(i) => match i {
            1 => Ok(true),
            0 => Ok(false),
            _ => Err(serde::de::Error::custom(format!(
                "The number is neither 1 nor 0, was {i}"
            ))),
        },
        AnythingOrBool::Float(f) => {
            if (f - 1.0f64).abs() < EPSILON {
                Ok(true)
            } else if f == 0.0f64 {
                Ok(false)
            } else {
                Err(serde::de::Error::custom(
                    "The number is neither 1.0 nor 0.0",
                ))
            }
        }
        AnythingOrBool::String(string) => {
            if let Ok(b) = string.parse::<bool>() {
                Ok(b)
            } else if let Ok(i) = string.parse::<i64>() {
                match i {
                    1 => Ok(true),
                    0 => Ok(false),
                    _ => Err(serde::de::Error::custom("The number is neither 1 nor 0")),
                }
            } else if let Ok(f) = string.parse::<f64>() {
                if (f - 1.0f64).abs() < f64::EPSILON {
                    Ok(true)
                } else if f == 0.0f64 {
                    Ok(false)
                } else {
                    Err(serde::de::Error::custom(
                        "The number is neither 1.0 nor 0.0",
                    ))
                }
            } else if string.eq_ignore_ascii_case("true") {
                Ok(true)
            } else if string.eq_ignore_ascii_case("false") {
                Ok(false)
            } else {
                Err(serde::de::Error::custom(format!(
                    "Could not parse boolean from a string: {}",
                    string
                )))
            }
        }
    }
}
