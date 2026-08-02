use crate::components::icon::Icon;
use crate::i18n::{Locale, t, t_string, use_i18n};
use icondata as i;
use leptos::prelude::*;
use leptos_i18n::Locale as _;

#[derive(Clone, Copy, PartialEq, Eq)]
struct LanguageOption {
    locale: Locale,
    name: &'static str,
    native_name: &'static str,
}

const LANGUAGE_OPTIONS: [LanguageOption; 7] = [
    LanguageOption {
        locale: Locale::en,
        name: "English",
        native_name: "English",
    },
    LanguageOption {
        locale: Locale::fr,
        name: "French",
        native_name: "Français",
    },
    LanguageOption {
        locale: Locale::de,
        name: "German",
        native_name: "Deutsch",
    },
    LanguageOption {
        locale: Locale::ja,
        name: "Japanese",
        native_name: "日本語",
    },
    LanguageOption {
        locale: Locale::cn,
        name: "Chinese (Simplified)",
        native_name: "简体中文",
    },
    LanguageOption {
        locale: Locale::ko,
        name: "Korean",
        native_name: "한국어",
    },
    LanguageOption {
        locale: Locale::tc,
        name: "Chinese (Traditional)",
        native_name: "繁體中文",
    },
];

fn reload_locale_data(new_locale: Locale) {
    #[cfg(feature = "ssr")]
    let _ = new_locale;

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
}

#[component]
pub fn LanguagePicker() -> impl IntoView {
    let i18n = use_i18n();
    let selected = Selector::new(move || i18n.get_locale());

    let set_language = move |new_locale: Locale| {
        i18n.set_locale(new_locale);
        reload_locale_data(new_locale);
    };

    view! {
        <div class="grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-3 gap-3" role="radiogroup" aria-label=move || t_string!(i18n, language).to_string()>
            {LANGUAGE_OPTIONS
                .into_iter()
                .map(|option| {
                    let selected_for_aria = selected.clone();
                    let selected_for_class = selected.clone();
                    let selected_for_show = selected.clone();
                    view! {
                        <button
                            type="button"
                            role="radio"
                            aria-checked=move || selected_for_aria.selected(&option.locale).to_string()
                            class=move || {
                                if selected_for_class.selected(&option.locale) {
                                    "min-h-20 rounded-lg border border-[color:var(--brand-ring)] bg-[color:color-mix(in_srgb,var(--brand-ring)_20%,transparent)] p-4 text-left transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-[color:var(--brand-ring)]"
                                } else {
                                    "min-h-20 rounded-lg border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--color-text)_4%,transparent)] p-4 text-left transition-colors hover:border-[color:var(--brand-ring)] hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[color:var(--brand-ring)]"
                                }
                            }
                            on:click=move |_| set_language(option.locale)
                        >
                            <div class="flex items-start justify-between gap-3">
                                <div class="min-w-0">
                                    <div class="font-semibold text-[color:var(--color-text)]">{option.native_name}</div>
                                    <div class="text-sm text-[color:var(--color-text-muted)]">{option.name}</div>
                                </div>
                                <span class="shrink-0 rounded-md border border-[color:var(--color-outline)] px-2 py-1 text-xs font-bold uppercase text-[color:var(--color-text-muted)]">
                                    {option.locale.as_str()}
                                </span>
                            </div>
                            <Show when=move || selected_for_show.selected(&option.locale)>
                                <div class="mt-3 flex items-center gap-2 text-sm font-medium text-[color:var(--brand-fg)]">
                                    <Icon icon=i::BsCheckCircleFill width="1em" height="1em" />
                                    <span class="sr-only">{t!(i18n, language_picker_selected_sr)}</span>
                                </div>
                            </Show>
                        </button>
                    }
                })
                .collect::<Vec<_>>()}
        </div>
    }
}

/// Language switcher as an inline accordion, for use inside the account
/// drop-up.
///
/// Deliberately not a flyout submenu: the sidebar doubles as the mobile
/// drawer below 1024px, and a hover-opened flyout has no touch equivalent.
#[component]
pub fn LanguageAccordion() -> impl IntoView {
    let i18n = use_i18n();
    let (expanded, set_expanded) = signal(false);
    let selected = Selector::new(move || i18n.get_locale());

    let set_language = move |new_locale: Locale| {
        i18n.set_locale(new_locale);
        reload_locale_data(new_locale);
        set_expanded(false);
    };

    view! {
        <button
            type="button"
            class="menu-item"
            aria-expanded=move || if expanded.get() { "true" } else { "false" }
            on:click=move |_| set_expanded.update(|v| *v = !*v)
        >
            <Icon icon=i::IoLanguage width="1.1em" height="1.1em" />
            <span class="ml-2">{t!(i18n, language)}</span>
            <span class="menu-item-trailing">
                {move || i18n.get_locale().as_str().to_uppercase()}
            </span>
        </button>

        <Show when=move || expanded.get()>
            <div class="menu-accordion" role="radiogroup" aria-label=t_string!(i18n, language).to_string()>
                {LANGUAGE_OPTIONS
                    .into_iter()
                    .map(|option| {
                        let selected_for_class = selected.clone();
                        let selected_for_aria = selected.clone();
                        let selected_for_show = selected.clone();
                        view! {
                            <button
                                type="button"
                                role="radio"
                                class=move || {
                                    if selected_for_class.selected(&option.locale) {
                                        "menu-item menu-item-selected"
                                    } else {
                                        "menu-item"
                                    }
                                }
                                aria-checked=move || selected_for_aria.selected(&option.locale).to_string()
                                on:click=move |_| set_language(option.locale)
                            >
                                <span class="menu-item-code">{option.locale.as_str()}</span>
                                <span class="ml-2 truncate">{option.native_name}</span>
                                <Show when=move || selected_for_show.selected(&option.locale)>
                                    <span class="menu-item-trailing">
                                        <Icon icon=i::BsCheckCircleFill width="0.9em" height="0.9em" />
                                    </span>
                                </Show>
                            </button>
                        }
                    })
                    .collect::<Vec<_>>()}
            </div>
        </Show>
    }
    .into_any()
}
