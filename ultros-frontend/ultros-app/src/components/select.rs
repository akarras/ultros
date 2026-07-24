use leptos::{
    html::{Div, Input},
    portal::Portal,
    prelude::*,
    reactive::wrappers::write::SignalSetter,
};
use web_sys::KeyboardEvent;
use web_sys::wasm_bindgen::JsCast;

#[component]
pub fn Select<T, EF, L, ViewOut>(
    items: Signal<Vec<T>>,
    as_label: L,
    choice: Signal<Option<T>>,
    set_choice: SignalSetter<Option<T>>,
    children: EF,
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
        // Reset highlighted index when results change
        final_result.track();
        set_highlighted_index(0);
    });

    let keydown = move |e: KeyboardEvent| {
        let key = e.key();
        if key == "ArrowDown" {
            e.prevent_default();
            set_highlighted_index.update(|i| {
                let len = final_result.with(|r| r.len());
                if len > 0 {
                    *i = (*i + 1) % len;
                    // Scroll into view logic could be added here if needed
                }
            });
        } else if key == "ArrowUp" {
            e.prevent_default();
            set_highlighted_index.update(|i| {
                let len = final_result.with(|r| r.len());
                if len > 0 {
                    *i = (*i + len - 1) % len;
                }
            });
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

    let default_input_class = "input w-full";
    let default_dropdown_class =
        "fixed max-h-96 overflow-y-auto panel rounded-lg shadow-lg z-[100]";
    let combined_input_class = format!("{} {}", default_input_class, class.unwrap_or(""));
    let combined_dropdown_class = format!(
        "{} {}",
        default_dropdown_class,
        dropdown_class.unwrap_or("")
    );

    let current_choice_view = move || {
        choice()
            .map(|c| children(c.clone(), as_label(&c).into_any()))
            .into_any()
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
                    set_focused(true)
                }
                on:focusout=move |_| set_focused(false)
                on:input=move |e| {
                    set_current_input(event_target_value(&e));
                    set_highlighted_index(0);
                }
                on:keydown=keydown
                prop:value=current_input
                role="combobox"
                aria-autocomplete="list"
                aria-expanded=move || (has_focus() || hovered()).to_string()
                aria-activedescendant=move || format!("select-item-{}", highlighted_index())
            />
            <div
                class="absolute top-1 left-1 select-none cursor flex items-center"
                class:invisible=move || has_focus() || !current_input().is_empty()
                on:click=move |_| {
                    if let Some(input) = input.get() {
                        let _ = input.focus();
                    }
                }
            >
                {current_choice_view}
            </div>
            <Portal>{dropdown_panel.clone()}</Portal>
        </div>
    }
    .into_any()
}
