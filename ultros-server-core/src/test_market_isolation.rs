//! Opt-in upstream writer isolation for deterministic, real-database QA.
//! The whole module and every call site are absent without `test-auth`.
use std::sync::OnceLock;

pub const ENV: &str = "ULTROS_TEST_MARKET_ISOLATION";
pub const BLOCKED: &str = "Upstream writes blocked by test-auth market fixture isolation";

// Capture once before spawning any market worker. A process cannot seed an
// isolated fixture and then enable writers by changing its environment.
pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    initialize(&ENABLED, || std::env::var(ENV).ok())
}

fn initialize(value: &OnceLock<bool>, read: impl FnOnce() -> Option<String>) -> bool {
    *value.get_or_init(|| from_value(read().as_deref()))
}

fn from_value(raw: Option<&str>) -> bool {
    crate::env_flag::env_flag_enabled(ENV, raw)
}

#[cfg(test)]
mod tests {
    use super::{from_value, initialize};
    use std::sync::OnceLock;

    #[test]
    fn startup_choice_is_frozen_without_reading_environment_again() {
        for initial in [None, Some("true".to_owned())] {
            let expected = initial.is_some();
            let value = OnceLock::new();
            assert_eq!(initialize(&value, || initial), expected);
            assert_eq!(
                initialize(&value, || panic!("must not re-read a startup choice")),
                expected
            );
        }
    }

    #[test]
    fn isolation_is_opt_in_and_false_values_leave_normal_test_auth_behavior() {
        for value in [
            None,
            Some(""),
            Some(" "),
            Some("0"),
            Some(" false "),
            Some("off"),
            Some("NO"),
        ] {
            assert!(!from_value(value));
        }
    }

    #[test]
    fn explicit_or_misspelled_opt_in_never_silently_starts_market_writers() {
        for value in ["1", "true", " YES ", "On", "please-isolate"] {
            assert!(from_value(Some(value)));
        }
    }
}
