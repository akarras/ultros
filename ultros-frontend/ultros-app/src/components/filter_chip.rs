//! Editable filter chip. Resting state shows `label value`; clicking the
//! value turns it into an inline input. The `x` clears the filter.
//!
//! This is the *only* representation of a filter on the Flip Finder — the
//! page previously rendered each filter twice (a toolbar input plus a chip
//! echoing it), which cost 198px of vertical space for one piece of state.

use crate::components::icon::Icon;
use crate::i18n::*;
use leptos::either::Either;
use leptos::html::Input;
use leptos::prelude::*;

/// Height reserved for the sticky control bar. Feeds
/// `ScrollSource::Window { sticky_offset }` so rows hidden behind the bar
/// are not counted as visible.
///
/// The bar's markup pins this exact height (`h-[76px]`) rather than letting
/// content grow it: the table header sticks at `top: STICKY_BAR_HEIGHT`, so
/// a bar that is taller than the constant hides its own column headers.
pub const STICKY_BAR_HEIGHT: f64 = 76.0;

/// Normalize raw input text into a filter value.
///
/// Trims, and maps blank input to "no filter". Without the trim, a value of
/// `" "` round-trips into the URL as `?next-sale=%20`: every parser then
/// rejects it (so nothing is filtered) while the chip still claims a filter
/// is active.
pub fn committed_value(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Resting-state display for a chip value. Select-variant chips store a
/// machine token (`hq`, `medium`) in the URL; the chip shows the localized
/// label for it. Plain chips show the raw value unchanged.
pub fn option_label(options: Option<&[(&'static str, String)]>, raw: String) -> String {
    match options {
        Some(opts) => opts
            .iter()
            .find(|(v, _)| *v == raw)
            .map(|(_, l)| l.clone())
            .unwrap_or(raw),
        None => raw,
    }
}

#[component]
pub fn FilterChip(
    #[prop(into)] label: String,
    #[prop(into)] value: Signal<Option<String>>,
    #[prop(into)] on_commit: Callback<Option<String>>,
    #[prop(optional)] numeric: bool,
    /// Filters whose value cannot sensibly be typed — a world picked off a
    /// row, a category chosen from a list, a boolean. They render the same
    /// chip and still clear with `x`, but the value is not an input.
    #[prop(optional)]
    readonly: bool,
    /// `min` / `max` / `step` for the inline input, carried over from the
    /// toolbar fields these chips replaced. They are what stops the spinner
    /// walking a count-of-6 filter to 40 or a gil figure to -1.
    #[prop(optional, into)]
    min: Option<String>,
    #[prop(optional, into)] max: Option<String>,
    #[prop(optional, into)] step: Option<String>,
    /// (value, localized label) pairs. When set, the chip edits via an
    /// inline `<select>` instead of a text input, and the resting state
    /// shows the current value's label rather than the raw token.
    /// `into` so call sites pass a bare `vec![...]` (std's
    /// `From<T> for Option<T>` provides the conversion).
    #[prop(optional, into)]
    options: Option<Vec<(&'static str, String)>>,
    /// Mount already in edit state. Used by chips whose seed value is
    /// empty (name search): a resting chip with no value is just a label.
    #[prop(optional)]
    start_editing: bool,
) -> impl IntoView {
    let i18n = use_i18n();
    let editing = RwSignal::new(start_editing);
    let input_ref = NodeRef::<Input>::new();
    let select_ref = NodeRef::<leptos::html::Select>::new();
    // StoredValue: both `Show` branches are `Fn` closures and both need the
    // options; storing once avoids a clone per render.
    let options = StoredValue::new(options);
    // Same treatment for the input attributes: the input now sits inside an
    // extra `move ||` closure (the select/input dispatch), and a `move`
    // closure inside an `Fn` closure cannot take the raw `Option<String>`s
    // by value. StoredValue is Copy, so both layers can capture it freely.
    let min = StoredValue::new(min);
    let max = StoredValue::new(max);
    let step = StoredValue::new(step);
    let resting_label = label.clone();

    // The chip is entered by clicking it, so the input has to take focus or
    // the user has to click a second time to type into it.
    Effect::new(move |_| {
        if editing.get() {
            if let Some(el) = input_ref.get() {
                let _ = el.focus();
            } else if let Some(el) = select_ref.get() {
                let _ = el.focus();
            }
        }
    });

    // A `type=number` input reports content it cannot parse (`1e`, `--`, a
    // pasted word) as an *empty* value, which is indistinguishable from a
    // deliberate clear and would delete the filter the user is editing.
    // `badInput` is the only way to tell the two apart; when it is set, leave
    // the filter exactly as it was.
    let commit_from = move |el: &web_sys::HtmlInputElement| {
        if !el.validity().bad_input() {
            on_commit.run(committed_value(&el.value()));
        }
        editing.set(false);
    };

    // Enter and Escape both tear the input down, which can raise a trailing
    // blur. Committing again there would re-commit on Enter (harmless) and
    // *defeat* Escape (not harmless), so blur only commits while the chip
    // still considers itself in edit mode.
    let commit_from_blur = move |ev: leptos::ev::FocusEvent| {
        if !editing.get_untracked() {
            return;
        }
        commit_from(&event_target::<web_sys::HtmlInputElement>(&ev));
    };

    view! {
        <Show
            when=move || editing.get()
            fallback=move || {
                let label = resting_label.clone();
                view! {
                    <span class="filter-chip">
                        {if readonly {
                            Either::Left(
                                view! {
                                    <span class="filter-chip-static">
                                        {label.clone()} " " {move || options.with_value(|o| option_label(o.as_deref(), value.get().unwrap_or_default()))}
                                    </span>
                                },
                            )
                        } else {
                            Either::Right(
                                view! {
                                    <button
                                        class="filter-chip-value"
                                        on:click=move |_| editing.set(true)
                                    >
                                        {label.clone()} " " {move || options.with_value(|o| option_label(o.as_deref(), value.get().unwrap_or_default()))}
                                    </button>
                                },
                            )
                        }}
                        <button
                            class="filter-chip-x"
                            aria-label=t_string!(i18n, aria_remove_filter)
                            on:click=move |_| on_commit.run(None)
                        >
                            <Icon icon=icondata::MdiClose />
                        </button>
                    </span>
                }
            }
        >
            <span class="filter-chip filter-chip-editing">
                <span class="filter-chip-label">{label.clone()}</span>
                {move || {
                    match options.with_value(|opts| opts.clone()) {
                        Some(opts) => Either::Left(view! {
                            <select
                                node_ref=select_ref
                                class="input input-sm"
                                on:change=move |ev| {
                                    on_commit.run(committed_value(&event_target_value(&ev)));
                                    editing.set(false);
                                }
                                on:keydown=move |ev| {
                                    if ev.key() == "Escape" {
                                        editing.set(false);
                                    }
                                }
                                on:blur=move |_| editing.set(false)
                                prop:value=move || value.get().unwrap_or_default()
                            >
                                {opts
                                    .into_iter()
                                    .map(|(val, lab)| {
                                        view! {
                                            <option value=val selected=move || value.get().as_deref() == Some(val)>
                                                {lab}
                                            </option>
                                        }
                                    })
                                    .collect_view()}
                            </select>
                        }),
                        None => Either::Right(view! {
                            <input
                                node_ref=input_ref
                                class="input input-sm w-24"
                                type=if numeric { "number" } else { "text" }
                                min=min.get_value()
                                max=max.get_value()
                                step=step.get_value()
                                prop:value=move || value.get().unwrap_or_default()
                                on:blur=commit_from_blur
                                on:keydown=move |ev| {
                                    if ev.key() == "Enter" {
                                        commit_from(&event_target::<web_sys::HtmlInputElement>(&ev));
                                    } else if ev.key() == "Escape" {
                                        editing.set(false);
                                    }
                                }
                            />
                        }),
                    }
                }}
            </span>
        </Show>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_input_clears_the_filter() {
        assert_eq!(committed_value(""), None);
    }

    #[test]
    fn whitespace_only_input_clears_the_filter() {
        // `?next-sale=%20` parses as neither a duration nor "unset", so the
        // chip would sit there claiming to filter nothing.
        assert_eq!(committed_value("   "), None);
        assert_eq!(committed_value("\t"), None);
    }

    #[test]
    fn surrounding_whitespace_is_trimmed_off_the_value() {
        assert_eq!(committed_value(" 5000 "), Some("5000".to_string()));
        assert_eq!(committed_value("7d"), Some("7d".to_string()));
    }

    #[test]
    fn option_label_maps_value_to_its_label() {
        let opts = vec![("hq", "HQ".to_string()), ("nq", "NQ".to_string())];
        assert_eq!(option_label(Some(&opts), "nq".to_string()), "NQ");
    }

    #[test]
    fn option_label_falls_back_to_raw_value_when_unknown() {
        let opts = vec![("hq", "HQ".to_string())];
        // A stale URL value the options no longer contain still renders
        // something rather than a blank chip.
        assert_eq!(option_label(Some(&opts), "zz".to_string()), "zz");
    }

    #[test]
    fn option_label_passes_plain_values_through() {
        assert_eq!(option_label(None, "5000".to_string()), "5000");
    }
}
