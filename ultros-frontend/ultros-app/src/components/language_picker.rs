use crate::i18n::*;
use leptos::prelude::*;
use crate::components::icon::Icon;
use icondata as i;

#[component]
pub fn LanguagePicker() -> impl IntoView {
    let i18n = use_i18n();

    let on_switch = move |_| {
        let new_locale = match i18n.get_locale() {
            Locale::en => Locale::fr,
            Locale::fr => Locale::en,
        };
        i18n.set_locale(new_locale);
    };

    view! {
        <button
            on:click=on_switch
            class="btn-ghost p-2 rounded-full hover:bg-black/20 transition-colors"
            title="Switch Language"
        >
            <Icon icon=i::IoLanguage width="1.2em" height="1.2em" />
            <span class="ml-1 uppercase text-xs font-bold">{move || i18n.get_locale().as_str()}</span>
        </button>
    }
}
