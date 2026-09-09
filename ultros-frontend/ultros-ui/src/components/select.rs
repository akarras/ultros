use icondata as i;
use leptos::{
    html::{Div, Input},
    portal::Portal,
    prelude::*,
    reactive::wrappers::write::SignalSetter,
};
use web_sys::KeyboardEvent;
use web_sys::wasm_bindgen::JsCast;

use crate::components::icon::Icon;

#[component]
pub fn Select<T, EF, L, ViewOut>(
    items: Signal<Vec<T>>,
    as_label: L,
    choice: Signal<Option<T>>,
    set_choice: SignalSetter<Option<T>>,
    children: EF,
    /// Optional leading adornment (icon, swatch, ...) rendered beside the
    /// collapsed value. Kept separate from `children` so the field can carry a
    /// compact marker without inheriting the dropdown row's decoration.
    #[prop(into, optional)]
    selected_prefix: Option<Callback<T, AnyView>>,
    #[prop(optional)] class: Option<&'static str>,
    #[prop(optional)] dropdown_class: Option<&'static str>,
) -> impl IntoView
where
    T: Clone + PartialEq + 'static + Send + Sync,
    EF: Fn(T, AnyView) -> View<ViewOut> + 'static + Copy + Send + Sync,
    ViewOut: RenderHtml + 'static,
    L: Fn(&T) -> String + 'static + Copy + Send + Sync,
{
    let (current_input, set_current_input) = signal("".to_string());
    let (has_focus, set_focused) = signal(false);
    let dropdown = NodeRef::<Div>::new();
    let input = NodeRef::<Input>::new();
    let (highlighted_index, set_highlighted_index) = signal(0_usize);

    #[cfg(feature = "hydrate")]
    let hovered = leptos_use::use_element_hover(dropdown);
    #[cfg(not(feature = "hydrate"))]
    let hovered = Signal::derive(move || false);

    // The dropdown is rendered in a portal at the document body so ancestor
    // stacking contexts (e.g. `.panel`'s backdrop-filter) and overflow clipping
    // can't hide it. Position it under the input in viewport coordinates.
    #[cfg(feature = "hydrate")]
    let (dropdown_position, update_dropdown_position) = {
        let leptos_use::UseElementBoundingReturn {
            bottom,
            left,
            width,
            update,
            ..
        } = leptos_use::use_element_bounding(input);
        let position = Signal::derive(move || {
            format!(
                "top: {}px; left: {}px; width: {}px;",
                bottom.get() + 4.0,
                left.get(),
                width.get()
            )
        });
        (position, update)
    };
    #[cfg(not(feature = "hydrate"))]
    let (dropdown_position, update_dropdown_position) = (Signal::derive(String::new), || {});

    let labels = Memo::new(move |_| {
        items.with(|i| {
            i.iter()
                .map(as_label)
                .enumerate()
                .map(|(idx, label)| {
                    let lower = label.to_lowercase();
                    (idx, label, lower)
                })
                .collect::<Vec<_>>()
        })
    });
    let search_results = Memo::new(move |_| {
        current_input.with(|input| {
            let input_lower = input.to_lowercase();
            labels.with(|s| {
                s.iter()
                    .filter_map(|(i, label, lower)| {
                        if lower.contains(&input_lower) {
                            Some((*i, label.clone()))
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
            })
        })
    });
    let final_result = Memo::new(move |_| {
        let search_results = search_results();
        if search_results.is_empty() {
            labels().into_iter().map(|(i, l, _)| (i, l)).collect()
        } else {
            search_results
        }
    });

    Effect::new(move |_| {
        // Typing re-filters the list, so start again from the top of the new
        // results. Deliberately keyed on the query rather than on
        // `final_result` - the latter also fires when the item list itself
        // arrives, which would yank the highlight away from the open selection.
        current_input.track();
        set_highlighted_index(0);
    });

    // Keep the highlighted row inside the scroll viewport. Only the dropdown's
    // own scroll offset is touched (rather than `scroll_into_view`, which can
    // pull the whole page around a `fixed` panel).
    let scroll_highlight_into_view = move |render_idx: usize| {
        #[cfg(feature = "hydrate")]
        {
            let Some(panel) = dropdown.get_untracked() else {
                return;
            };
            let Some(item) = document()
                .get_element_by_id(&format!("select-item-{}", render_idx))
                .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
            else {
                return;
            };
            // `offset_*` / `client_height` are i32, `scroll_top` is f64.
            let (top, height) = (item.offset_top() as f64, item.offset_height() as f64);
            let (view_top, view_height) = (panel.scroll_top(), panel.client_height() as f64);
            if top < view_top {
                panel.set_scroll_top(top);
            } else if top + height > view_top + view_height {
                panel.set_scroll_top(top + height - view_height);
            }
        }
        #[cfg(not(feature = "hydrate"))]
        let _ = render_idx;
    };

    let keydown = move |e: KeyboardEvent| {
        let key = e.key();
        if key == "ArrowDown" {
            e.prevent_default();
            set_highlighted_index.update(|i| {
                let len = final_result.with(|r| r.len());
                if len > 0 {
                    *i = (*i + 1) % len;
                }
            });
            scroll_highlight_into_view(highlighted_index.get_untracked());
        } else if key == "ArrowUp" {
            e.prevent_default();
            set_highlighted_index.update(|i| {
                let len = final_result.with(|r| r.len());
                if len > 0 {
                    *i = (*i + len - 1) % len;
                }
            });
            scroll_highlight_into_view(highlighted_index.get_untracked());
        } else if key == "Enter" {
            e.prevent_default();
            let idx = highlighted_index.get_untracked();
            let item_opt = final_result.with_untracked(|res| {
                res.get(idx).and_then(|(original_idx, _)| {
                    items.with_untracked(|i| i.get(*original_idx).cloned())
                })
            });

            if let Some(item) = item_opt {
                set_choice(Some(item));
                set_current_input("".to_string());
                set_focused(false);
                if let Some(element) = document()
                    .active_element()
                    .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
                {
                    let _ = element.blur();
                }
            }
        } else if key == "Escape" {
            e.prevent_default();
            set_focused(false);
            if let Some(element) = document()
                .active_element()
                .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
            {
                let _ = element.blur();
            }
        }
    };

    // `pr-9` reserves the trailing gutter for the chevron.
    let default_input_class = "input w-full pr-9";
    let default_dropdown_class =
        "fixed max-h-96 overflow-y-auto panel rounded-lg shadow-lg z-[100]";
    let combined_input_class = format!("{} {}", default_input_class, class.unwrap_or(""));
    let combined_dropdown_class = format!(
        "{} {}",
        default_dropdown_class,
        dropdown_class.unwrap_or("")
    );

    // The collapsed value is rendered as plain text sized to match the input's
    // own padding. It deliberately does *not* reuse `children` - the dropdown
    // row decoration (hover fill, row padding, per-item badges) is taller than
    // the input's content box and spills over the field's border.
    let current_choice_view = move || choice().map(|c| as_label(&c));
    let current_prefix_view = move || {
        let prefix = selected_prefix?;
        choice().map(|c| prefix.run(c))
    };

    let selected_index_memo = Memo::new(move |_| {
        choice.with(|c| {
            if let Some(c) = c {
                items.with(|items| items.iter().position(|i| i == c))
            } else {
                None
            }
        })
    });
    let is_selected_selector = Selector::new(move || selected_index_memo.get());

    // Opening the list should highlight what is already chosen, so that an
    // immediate Enter re-confirms the current value instead of silently
    // swapping it for whichever entry happens to sort first.
    let highlight_current_choice = move || {
        let render_idx = selected_index_memo
            .get_untracked()
            .and_then(|selected| {
                final_result
                    .with_untracked(|r| r.iter().position(|(original, _)| *original == selected))
            })
            .unwrap_or(0);
        set_highlighted_index(render_idx);
        render_idx
    };

    let dropdown_panel = {
        let is_selected_selector = is_selected_selector.clone();
        move || {
            let is_selected_selector = is_selected_selector.clone();
            view! {
                <div
                    node_ref=dropdown
                    class=combined_dropdown_class.clone()
                    class:hidden=move || !has_focus() && !hovered()
                    style=move || dropdown_position.get()
                    role="listbox"
                >
                    <For each=move || final_result.get().into_iter().enumerate() key=move |(_, (l, _))| *l let:data>
                        {
                            let (render_idx, (original_idx, label)) = data;
                            let is_selected_selector = is_selected_selector.clone();
                            view! {
                                <button
                                    id=format!("select-item-{}", render_idx)
                                    class="w-full text-left scroll-mt-2"
                                    role="option"
                                    aria-selected={
                                        let is_selected_selector = is_selected_selector.clone();
                                        move || is_selected_selector.selected(&Some(original_idx)).to_string()
                                    }
                                    on:click=move |_| {
                                        if let Some(item) = items.with(|i| i.get(original_idx).cloned()) {
                                            set_choice(Some(item));
                                            set_focused(false);
                                            set_current_input("".to_string());
                                            if let Some(element) = document()
                                                .active_element()
                                                .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
                                            {
                                                let _ = element.blur();
                                            }
                                        }
                                    }
                                    on:mousemove=move |_| {
                                        set_highlighted_index(render_idx);
                                    }
                                >
                                    <div class={
                                        let is_selected_selector = is_selected_selector.clone();
                                        move || {
                                            let is_selected = is_selected_selector.selected(&Some(original_idx));
                                            let is_highlighted = highlighted_index() == render_idx;

                                            if is_highlighted {
                                                 "flex items-center rounded-lg p-2 transition-colors duration-200 bg-[color:color-mix(in_srgb,var(--brand-ring)_18%,transparent)] ring-1 ring-[color:var(--brand-ring)]"
                                            } else if is_selected {
                                                "flex items-center rounded-lg p-2 transition-colors duration-200 bg-[color:color-mix(in_srgb,var(--brand-ring)_18%,transparent)]"
                                            } else {
                                                "flex items-center rounded-lg p-2 transition-colors duration-200 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)]"
                                            }
                                        }
                                    }>
                                        {move || items
                                            .with(|i| i.get(original_idx).cloned())
                                            .map(|c| children(
                                                c,
                                                {
                                                    view! { <div>{label.clone()}</div> }.into_any()
                                                }
                                            ))}
                                    </div>
                                </button>
                            }
                        }
                    </For>
                </div>
            }
        }
    };

    view! {
        <div class="relative">
            <input
                node_ref=input
                class=combined_input_class
                class:cursor=move || !has_focus()
                on:focus=move |_| {
                    // Re-measure before opening: the bounding signals start at
                    // zero when the node ref was already set before the
                    // watcher's first run (hydration).
                    update_dropdown_position();
                    set_focused(true);
                    let render_idx = highlight_current_choice();
                    scroll_highlight_into_view(render_idx);
                }
                on:focusout=move |_| set_focused(false)
                on:input=move |e| {
                    set_current_input(event_target_value(&e));
                    set_highlighted_index(0);
                }
                on:keydown=keydown
                prop:value=current_input
                // While the field is open the overlay is hidden, so the current
                // value is echoed as a placeholder - you can still see what you
                // are replacing as you type over it.
                prop:placeholder=move || {
                    if has_focus() { current_choice_view().unwrap_or_default() } else { String::new() }
                }
                role="combobox"
                aria-autocomplete="list"
                aria-expanded=move || (has_focus() || hovered()).to_string()
                aria-activedescendant=move || format!("select-item-{}", highlighted_index())
            />
            <div
                class="absolute inset-y-0 right-0 flex items-center pr-3 text-[color:var(--color-text-muted)] pointer-events-none"
                aria-hidden="true"
            >
                <Icon
                    icon=i::BsChevronDown
                    attr:class=move || {
                        if has_focus() || hovered() {
                            "transition-transform duration-200 rotate-180"
                        } else {
                            "transition-transform duration-200"
                        }
                    }
                />
            </div>
            <div
                class="absolute inset-0 flex items-center gap-2 pl-3 pr-9 py-2 border border-transparent select-none cursor overflow-hidden"
                class:invisible=move || has_focus() || !current_input().is_empty()
                on:click=move |_| {
                    if let Some(input) = input.get() {
                        let _ = input.focus();
                    }
                }
            >
                {current_prefix_view}
                <span class="truncate">{current_choice_view}</span>
            </div>
            <Portal>{dropdown_panel.clone()}</Portal>
        </div>
    }
    .into_any()
}
