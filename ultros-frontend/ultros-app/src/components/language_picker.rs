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
        #[cfg(not(feature = "ssr"))]
        if let Some(rev) = use_context::<crate::global_state::xiv_data::DataRevision>() {
            let locale_str = new_locale.as_str().to_string();
            leptos::task::spawn_local(async move {
                match crate::global_state::xiv_data::reload_xiv_data(&locale_str).await {
                    Ok(()) => rev.0.update(|v| *v = v.wrapping_add(1)),
                    Err(e) => log::error!("failed to reload xiv data for {locale_str}: {e}"),
                }
            });
        }
    };

    view! {
        <button
            on:click=on_switch
            class="btn-ghost p-2 rounded-full hover:bg-black/20 transition-colors focus-visible:ring-2 focus-visible:ring-[color:var(--brand-ring)] focus:outline-none"
            title=move || t_string!(i18n, switch_language).to_string()
            aria-label=move || t_string!(i18n, switch_language).to_string()
        >
            <Icon icon=i::IoLanguage width="1.2em" height="1.2em" />
            <span class="ml-1 uppercase text-xs font-bold">{move || i18n.get_locale().as_str()}</span>
        </button>
    }
}
