use serde::{Deserialize, Deserializer};

pub fn deserialize_i64_from_u8_array<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    // Try to deserialize as a String first to see what we get
    let s = String::deserialize(deserializer)?;

    if s.is_empty() {
        return Ok(0);
    }

    // Attempt to parse as standard i64
    match s.parse::<i64>() {
        Ok(val) => Ok(val),
        Err(_) => Err(serde::de::Error::custom(format!(
            "Could not parse i64 from string: '{}'",
            s
        ))),
    }
}

pub fn deserialize_bool_from_anything_custom<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
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
            if (f - 1.0f64).abs() < f64::EPSILON {
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

#[cfg(all(test, feature = "csv"))]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[test]
    fn test_deserialize_i64() {
        #[derive(Deserialize, Debug, PartialEq)]
        struct Test {
            #[serde(deserialize_with = "deserialize_i64_from_u8_array")]
            val: i64,
        }

        let csv_data = "val\n12345";
        let mut reader = csv::Reader::from_reader(csv_data.as_bytes());
        for result in reader.deserialize() {
            let record: Test = result.unwrap();
            assert_eq!(record.val, 12345);
        }

        let csv_data = "val\n-999";
        let mut reader = csv::Reader::from_reader(csv_data.as_bytes());
        for result in reader.deserialize() {
            let record: Test = result.unwrap();
            assert_eq!(record.val, -999);
        }

        let csv_data = "val\n";
        let mut reader = csv::Reader::from_reader(csv_data.as_bytes());
        for result in reader.deserialize() {
            let record: Test = result.unwrap();
            assert_eq!(record.val, 0);
        }
    }
}
