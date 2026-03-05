use crate::components::icon::Icon;
use crate::i18n::{Locale, t_string, use_i18n};
use icondata as i;
use leptos::prelude::*;
use leptos_i18n::Locale as _;

#[component]
pub fn LanguagePicker() -> impl IntoView {
    let i18n = use_i18n();

    let on_switch = move |_| {
        let new_locale = match i18n.get_locale() {
            Locale::en => Locale::fr,
            Locale::fr => Locale::de,
            Locale::de => Locale::ja,
            Locale::ja => Locale::cn,
            Locale::cn => Locale::ko,
            Locale::ko => Locale::tc,
            Locale::tc => Locale::en,
        };
        i18n.set_locale(new_locale);
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(window) = web_sys::window() {
                let _ = window.location().reload();
            }
        }
    };

    view! {
        <button
            on:click=on_switch
            class="btn-ghost p-2 rounded-full hover:bg-black/20 transition-colors"
            title=move || t_string!(i18n, switch_language).to_string()
        >
            <Icon icon=i::IoLanguage width="1.2em" height="1.2em" />
            <span class="ml-1 uppercase text-xs font-bold">{move || i18n.get_locale().as_str()}</span>
        </button>
    }
}
