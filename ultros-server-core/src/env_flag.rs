use tracing::warn;

/// Interpret a boolean-ish environment variable.
///
/// Unset, empty, or any recognised falsy spelling means "off"; the usual truthy
/// spellings mean "on". An unrecognised value is treated as "on" *and* warned
/// about: someone who set the variable at all meant to flip it, so honouring
/// the intent beats silently ignoring `ULTROS_DISABLE_UNIVERSALIS_WEBSOCKET=please`
/// and letting a QA deploy keep writing to the shared database.
pub fn env_flag_enabled(name: &str, raw: Option<&str>) -> bool {
    let Some(value) = raw.map(str::trim).filter(|v| !v.is_empty()) else {
        return false;
    };
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        other => {
            warn!(
                variable = name,
                value = other,
                "unrecognised boolean value; treating it as enabled"
            );
            true
        }
    }
}
