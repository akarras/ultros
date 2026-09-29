//! The page's persistent market-history view, shared by providers and controls.
use leptos::prelude::*;

use super::stat_columns::Window;
use crate::{i18n::*, query_defaults::query_signal_or_default};

#[derive(Clone, Copy)]
pub struct MarketWindow {
    pub selected: Memo<Window>,
    set: SignalSetter<Option<u16>>,
    choices: &'static [Window],
}

impl MarketWindow {
    pub fn new(default: Window, choices: &'static [Window]) -> Self {
        let (raw, set) = query_signal_or_default::<u16>(
            "window",
            leptos_router::NavigateOptions {
                scroll: false,
                ..Default::default()
            },
        );
        Self {
            selected: Memo::new(move |_| select_window(raw.get(), default, choices)),
            set,
            choices,
        }
    }
}

fn select_window(raw: Option<u16>, default: Window, choices: &[Window]) -> Window {
    choices
        .iter()
        .copied()
        .find(|w| Some(w.days()) == raw)
        .unwrap_or(default)
}

#[component]
pub fn MarketWindowControl(window: MarketWindow) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    view! {
        <label class="filter-chip">
            <span>{t!(i18n, trends_window_label)}</span>
            <select class="filter-chip-value" data-market-window
                prop:value=move || window.selected.get().days().to_string()
                on:change=move |ev| window.set.set(event_target_value(&ev).parse().ok())>
                {window.choices.iter().copied().map(|choice| view! {
                    <option value=choice.days().to_string() selected=move || window.selected.get() == choice>
                        {super::stat_columns::window_label(choice)}
                    </option>
                }).collect_view()}
            </select>
        </label>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_and_supported_windows_preserve_existing_urls() {
        for raw in [None, Some(0), Some(2)] {
            assert_eq!(select_window(raw, Window::D7, &Window::ALL), Window::D7);
        }
        assert_eq!(
            select_window(Some(30), Window::D7, &Window::ALL),
            Window::D30
        );
        let trends = [Window::D7, Window::D30, Window::D90];
        assert_eq!(select_window(None, Window::D30, &trends), Window::D30);
        assert_eq!(select_window(Some(1), Window::D30, &trends), Window::D30);
    }
}
