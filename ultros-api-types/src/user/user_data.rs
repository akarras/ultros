use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, PartialOrd, Eq, Ord, Clone)]
pub struct UserData {
    // Discord snowflake. Serialized as a JSON string so the value survives a
    // round-trip through any JS engine — snowflakes routinely exceed 2^53 and
    // would silently lose precision if parsed into a JS Number. The custom
    // deserializer accepts either a string or a JSON number to remain
    // compatible with older clients / cached payloads.
    #[serde(with = "u64_string")]
    pub id: u64,
    pub username: String,
    pub avatar: String,
}

mod u64_string {
    use serde::{
        Deserializer, Serializer,
        de::{self, Visitor},
    };
    use std::fmt;

    pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(value)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = u64;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a u64 expressed as a string or JSON number")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<u64, E> {
                v.parse::<u64>().map_err(de::Error::custom)
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<u64, E> {
                Ok(v)
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<u64, E> {
                u64::try_from(v).map_err(de::Error::custom)
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<u64, E> {
                // f64 only represents integers exactly up to 2^53; anything
                // larger has already lost precision by the time we see it, so
                // refuse rather than return a silently corrupted id.
                const MAX_SAFE: f64 = 9_007_199_254_740_992.0;
                if !v.is_finite() || v.fract() != 0.0 || v.is_sign_negative() || v > MAX_SAFE {
                    return Err(de::Error::custom(format!(
                        "u64 value {v} cannot be represented exactly as f64"
                    )));
                }
                Ok(v as u64)
            }
        }

        deserializer.deserialize_any(V)
    }
}

#[cfg(test)]
mod tests {
    use super::UserData;

    #[test]
    fn snowflake_round_trips_as_string() {
        let user = UserData {
            id: 66154228472619010,
            username: "aaron".to_string(),
            avatar: "x".to_string(),
        };
        let json = serde_json::to_string(&user).unwrap();
        assert!(json.contains("\"id\":\"66154228472619010\""), "{json}");
        let back: UserData = serde_json::from_str(&json).unwrap();
        assert_eq!(back, user);
    }

    #[test]
    fn deserializes_legacy_numeric_id() {
        let json = r#"{"id":123456,"username":"a","avatar":"x"}"#;
        let user: UserData = serde_json::from_str(json).unwrap();
        assert_eq!(user.id, 123456);
    }

    #[test]
    fn rejects_unsafe_float_id() {
        // 66154228472619010 cannot be represented exactly as f64; if the
        // value reached us as a float we know precision has already been
        // lost, so we must reject rather than silently corrupt the id.
        let json = r#"{"id":66154228472619010.0,"username":"a","avatar":"x"}"#;
        let err = serde_json::from_str::<UserData>(json).unwrap_err();
        assert!(
            err.to_string().contains("cannot be represented"),
            "unexpected error: {err}"
        );
    }
}
