use leptos::{
    html::{Div, Input},
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
    let hovered = {
        let _ = dropdown;
        ArcSignal::derive(move || false)
    };

    let labels =
        Memo::new(move |_| items.with(|i| i.iter().map(as_label).enumerate().collect::<Vec<_>>()));
    let search_results = Memo::new(move |_| {
        current_input.with(|input| {
            let input_lower = input.to_lowercase();
            labels.with(|s| {
                s.iter()
                    .filter_map(|(i, label)| {
                        if label.to_lowercase().contains(&input_lower) {
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
            labels()
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
        "absolute w-full max-h-96 overflow-y-auto top-12 panel rounded-lg shadow-lg z-[100]";

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

    view! {
        <div class="relative">
            <input
                node_ref=input
                class=move || format!("{} {}", default_input_class, class.unwrap_or(""))
                class:cursor=move || !has_focus()
                on:focus=move |_| set_focused(true)
                on:focusout=move |_| set_focused(false)
                on:input=move |e| {
                    set_current_input(event_target_value(&e));
                    set_highlighted_index(0);
                }
                on:keydown=keydown
                prop:value=current_input
                role="combobox"
                aria-autocomplete="list"
                aria-expanded={
                    #[cfg(not(feature = "hydrate"))]
                    let hovered = hovered.clone();
                    move || (has_focus() || hovered()).to_string()
                }
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
            <div
                node_ref=dropdown
                class=move || format!("{} {}", default_dropdown_class, dropdown_class.unwrap_or(""))
                class:hidden=move || !has_focus() && !hovered()
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
        </div>
    }
    .into_any()
}
