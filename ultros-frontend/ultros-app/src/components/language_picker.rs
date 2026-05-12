use crate::components::icon::Icon;
use crate::i18n::{Locale, t_string, use_i18n};
use cfg_if::cfg_if;
use icondata as i;
use leptos::html;
use leptos::prelude::*;
use leptos_i18n::Locale as _;
#[cfg(feature = "hydrate")]
use leptos_use::use_element_hover;

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
                                    <span class="sr-only">"Selected"</span>
                                </div>
                            </Show>
                        </button>
                    }
                })
                .collect::<Vec<_>>()}
        </div>
    }
}

#[component]
pub fn LanguageNavMenu() -> impl IntoView {
    let i18n = use_i18n();
    let (has_focus, set_has_focus) = signal(false);
    let (force_close, set_force_close) = signal(false);
    let panel_ref = NodeRef::<html::Div>::new();
    cfg_if! {
        if #[cfg(feature = "hydrate")] {
            let hovered = use_element_hover(panel_ref);
        } else {
            let (hovered, _set_hovered) = signal(false);
        }
    }
    let is_open = Signal::derive(move || (has_focus() || hovered()) && !force_close());
    let selected = Selector::new(move || i18n.get_locale());

    let set_language = move |new_locale: Locale| {
        i18n.set_locale(new_locale);
        reload_locale_data(new_locale);
        set_has_focus(false);
        set_force_close(true);
    };

    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Escape" {
            set_has_focus(false);
        }
    };

    view! {
        <div
            class="relative"
            on:keydown=on_keydown
            on:focusin=move |_| {
                set_has_focus(true);
                set_force_close(false);
            }
            on:focusout=move |_| set_has_focus(false)
            on:mouseleave=move |_| set_force_close(false)
        >
            <button
                class="nav-link"
                aria-haspopup="menu"
                aria-expanded=move || if is_open() { "true" } else { "false" }
                aria-label=move || t_string!(i18n, switch_language).to_string()
                title=move || t_string!(i18n, switch_language).to_string()
            >
                <Icon icon=i::IoLanguage width="1.2em" height="1.2em" />
                <span class="uppercase text-xs font-bold">{move || i18n.get_locale().as_str()}</span>
                <Icon height="1em" width="1em" icon=i::BiChevronDownSolid />
            </button>

            <Show when=move || is_open()>
                <div
                    node_ref=panel_ref
                    class="absolute right-0 mt-2 min-w-[17rem]
                           panel rounded-xl shadow-xl border border-[color:var(--color-outline)]
                           bg-[color:var(--color-background-elevated)]
                           content-visible contain-content z-50"
                    role="menu"
                    tabindex="-1"
                >
                    <div class="p-2 flex flex-col gap-1">
                        {LANGUAGE_OPTIONS
                            .into_iter()
                            .map(|option| {
                                let selected_for_class = selected.clone();
                                let selected_for_aria = selected.clone();
                                let selected_for_show = selected.clone();
                                view! {
                                    <button
                                        type="button"
                                        class=move || {
                                            if selected_for_class.selected(&option.locale) {
                                                "nav-link w-full justify-start bg-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)]"
                                            } else {
                                                "nav-link w-full justify-start"
                                            }
                                        }
                                        role="menuitemradio"
                                        aria-checked=move || selected_for_aria.selected(&option.locale).to_string()
                                        on:click=move |_| set_language(option.locale)
                                    >
                                        <span class="w-10 shrink-0 text-xs font-bold uppercase text-[color:var(--color-text-muted)]">{option.locale.as_str()}</span>
                                        <span class="flex min-w-0 flex-1 flex-col items-start gap-0.5">
                                            <span class="truncate font-semibold">{option.native_name}</span>
                                            <span class="truncate text-xs text-[color:var(--color-text-muted)]">{option.name}</span>
                                        </span>
                                        <Show when=move || selected_for_show.selected(&option.locale)>
                                            <Icon icon=i::BsCheckCircleFill width="1em" height="1em" attr:class="text-[color:var(--brand-fg)]" />
                                        </Show>
                                    </button>
                                }
                            })
                            .collect::<Vec<_>>()}
                    </div>
                </div>
            </Show>
        </div>
    }
    .into_any()
}
