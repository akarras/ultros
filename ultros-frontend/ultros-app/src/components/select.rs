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
    // _view_out: PhantomData<ViewOut>,
) -> impl IntoView
where
    T: Clone + PartialEq + 'static + Send + Sync,
    EF: Fn(T, AnyView) -> View<ViewOut> + 'static + Copy + Send + Sync,
    // N: 'static,
    ViewOut: RenderHtml + 'static,
    L: Fn(&T) -> String + 'static + Copy + Send + Sync,
{
    let (current_input, set_current_input) = signal("".to_string());
    let (has_focus, set_focused) = signal(false);
    let dropdown = NodeRef::<Div>::new();
    let input = NodeRef::<Input>::new();
    #[cfg(feature = "hydrate")]
    let hovered = leptos_use::use_element_hover(dropdown);
    #[cfg(not(feature = "hydrate"))]
    let hovered = {
        let _ = dropdown;
        ArcSignal::derive(move || false)
    };

    let (highlighted_index, set_highlighted_index) = signal(0usize);

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

    // Memo stores ((original_index, label), visual_index)
    let final_result = Memo::new(move |_| {
        let search_results = search_results();
        let list = if search_results.is_empty() {
            labels()
        } else {
            search_results
        };
        list.into_iter().enumerate().map(|(v, item)| (item, v)).collect::<Vec<_>>()
    });

    // Reset highlight when results change
    Effect::new(move |_| {
        final_result.track();
        set_highlighted_index(0);
    });

    let keydown = move |e: KeyboardEvent| {
        let key = e.key();
        if key == "ArrowDown" {
            e.prevent_default();
            let len = final_result.with_untracked(|l| l.len());
            if len > 0 {
                set_highlighted_index.update(|i| *i = (*i + 1).min(len.saturating_sub(1)));
            }
        } else if key == "ArrowUp" {
            e.prevent_default();
            set_highlighted_index.update(|i| *i = i.saturating_sub(1));
        } else if key == "Enter" {
            e.prevent_default();
            // Use highlighted_index to pick from final_result
            let idx = highlighted_index.get_untracked();
            // final_result item is ((original_id, label), visual_id)
            if let Some(((id, _), _)) = final_result.with_untracked(|s| s.get(idx).cloned())
                && let Some(item) = items.with(|i| i.get(id).cloned())
            {
                set_choice(Some(item));
                set_current_input("".to_string());
                set_focused(false); // Close dropdown on selection
                if let Some(element) = document()
                    .active_element()
                    .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
                {
                    element.blur().unwrap();
                }
                // Maintain focus/blur sequence if it was intentional for other reasons
                if let Some(input) = input.get_untracked() {
                    let _ = input.focus();
                    let _ = input.blur();
                }
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
    // Optimization: Calculate the selected index once and use a Selector.
    // This avoids O(N) signal checks where every row listens to `choice`.
    // Instead, we only notify the row that matches the index.
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

    // class="invisible" thank you tailwind.
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
                }
                on:keydown=keydown
                prop:value=current_input
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
            >
                <For each=final_result key=move |((l, _), _)| *l let:data>
                    {
                        // data is ((original_id, label), visual_id)
                        let ((original_id, label), visual_id) = data;
                        let is_selected_selector = is_selected_selector.clone();
                        view! {
                    <button
                        class="w-full text-left"
                        // Sync highlighted index on mouse hover
                        on:mouseenter=move |_| set_highlighted_index(visual_id)
                        on:click=move |_| {
                            if let Some(item) = items.with(|i| i.get(original_id).cloned()) {
                                set_choice(Some(item));
                                set_focused(false);
                                set_current_input("".to_string());
                                if let Some(element) = document()
                                    .active_element()
                                    .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
                                {
                                    element.blur().unwrap();
                                }
                            }
                        }
                    >
                        <div class=move || {
                            let is_selected = is_selected_selector.selected(&Some(original_id));
                            let is_highlighted = highlighted_index.get() == visual_id;

                            if is_selected {
                                "flex items-center rounded-lg p-2 transition-colors duration-200 bg-[color:color-mix(in_srgb,var(--brand-ring)_18%,transparent)]"
                            } else if is_highlighted {
                                "flex items-center rounded-lg p-2 transition-colors duration-200 bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)]"
                            } else {
                                "flex items-center rounded-lg p-2 transition-colors duration-200 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)]"
                            }
                        }>
                            {move || items
                                .with(|i| i.get(original_id).cloned())
                                .map(|c| children(
                                    c,
                                    {
                                        view! { <div>{label.to_string()}</div> }.into_any()
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
