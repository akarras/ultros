use super::cookies::Cookies;
use leptos::prelude::*;
use leptos::reactive::wrappers::write::{IntoSignalSetter, SignalSetter};

pub const DALAMUD_COOKIE: &str = "LABS_DALAMUD";

/// Per-browser opt-in, shared by settings, navigation and plugin pages.
pub fn use_dalamud_labs() -> (Signal<bool>, SignalSetter<bool>) {
    let cookies = expect_context::<Cookies>();
    let (value, set_value) = cookies.use_cookie_typed::<_, bool>(DALAMUD_COOKIE);
    (
        Signal::derive(move || value.get().unwrap_or(false)),
        (move |enabled| set_value(Some(enabled))).into_signal_setter(),
    )
}
