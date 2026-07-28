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
) -> impl IntoView {
    let i18n = use_i18n();
    let editing = RwSignal::new(false);
    let input_ref = NodeRef::<Input>::new();
    let resting_label = label.clone();

    // The chip is entered by clicking it, so the input has to take focus or
    // the user has to click a second time to type into it.
    Effect::new(move |_| {
        if editing.get()
            && let Some(el) = input_ref.get()
        {
            let _ = el.focus();
        }
    });

    // Enter and Escape both tear the input down, which can raise a trailing
    // blur. Committing again there would re-commit on Enter (harmless) and
    // *defeat* Escape (not harmless), so blur only commits while the chip
    // still considers itself in edit mode.
    let commit_from_blur = move |ev: leptos::ev::FocusEvent| {
        if !editing.get_untracked() {
            return;
        }
        on_commit.run(committed_value(&event_target_value(&ev)));
        editing.set(false);
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
                                        {label.clone()} " " {move || value.get().unwrap_or_default()}
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
                                        {label.clone()} " " {move || value.get().unwrap_or_default()}
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
                <input
                    node_ref=input_ref
                    class="input input-sm w-24"
                    type=if numeric { "number" } else { "text" }
                    prop:value=move || value.get().unwrap_or_default()
                    on:blur=commit_from_blur
                    on:keydown=move |ev| {
                        if ev.key() == "Enter" {
                            on_commit.run(committed_value(&event_target_value(&ev)));
                            editing.set(false);
                        } else if ev.key() == "Escape" {
                            editing.set(false);
                        }
                    }
                />
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
}
