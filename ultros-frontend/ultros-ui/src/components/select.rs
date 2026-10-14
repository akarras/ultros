use icondata as i;
#[cfg(feature = "hydrate")]
use leptos::portal::Portal;
use leptos::{
    html::{Div, Input},
    prelude::*,
    reactive::wrappers::write::SignalSetter,
};
use web_sys::KeyboardEvent;
#[cfg(feature = "hydrate")]
use web_sys::wasm_bindgen::JsCast;

use crate::components::icon::Icon;
use crate::i18n::{t_string, use_i18n};

#[component]
pub fn Select<T, EF, L, ViewOut>(
    #[prop(into)] label: Signal<String>,
    #[prop(optional_no_strip)] input_id: Option<String>,
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
    let i18n = use_i18n();
    let id = RwSignal::new(String::new());
    Effect::new(move |_| id.set(format!("select-{}", uuid::Uuid::new_v4())));
    let (current_input, set_current_input) = signal("".to_string());
    let (has_focus, set_focused) = signal(false);
    let dropdown = NodeRef::<Div>::new();
    let input = NodeRef::<Input>::new();
    let (highlighted_index, set_highlighted_index) = signal(0_usize);

    // Portal into the owning dialog (or body) to escape panel clipping while
    // remaining inside the native modal's focus boundary.
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

    let update_dropdown_position = StoredValue::new(update_dropdown_position);
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
            let input_lower = if choice.get().is_some_and(|value| as_label(&value) == *input) {
                String::new()
            } else {
                input.to_lowercase()
            };
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
    let final_result = search_results;

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
                .get_element_by_id(&format!("{}-item-{render_idx}", id.get_untracked()))
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
            set_focused(true);
            e.prevent_default();
            set_highlighted_index.update(|i| {
                let len = final_result.with(|r| r.len());
                if len > 0 {
                    *i = (*i + 1) % len;
                }
            });
            scroll_highlight_into_view(highlighted_index.get_untracked());
        } else if key == "ArrowUp" {
            set_focused(true);
            e.prevent_default();
            set_highlighted_index.update(|i| {
                let len = final_result.with(|r| r.len());
                if len > 0 {
                    *i = (*i + len - 1) % len;
                }
            });
            scroll_highlight_into_view(highlighted_index.get_untracked());
        } else if key == "Enter" && has_focus.get_untracked() {
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
            }
        } else if key == "Escape" && has_focus.get_untracked() {
            e.prevent_default();
            set_focused(false);
            set_current_input(String::new());
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

    // The input exposes the selected label as its actual value. Dropdown row
    // decoration stays separate from that value.
    let current_choice_view = move || choice().map(|c| as_label(&c));

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
                    class:hidden=move || !has_focus()
                    style=move || dropdown_position.get()
                    id=move || format!("{}-list", id.get())
                    aria-label=move || label.get()
                    role="listbox"
                >
                    <For each=move || final_result.get().into_iter().enumerate() key=move |(render, (original, _))| (*render, *original) let:data>
                        {
                            let (render_idx, (original_idx, label)) = data;
                            let is_selected_selector = is_selected_selector.clone();
                            view! {
                                <button
                                    id=format!("{}-item-{render_idx}", id.get_untracked())
                                    class="w-full text-left scroll-mt-2"
                                    type="button"
                                    tabindex="-1"
                                    on:pointerdown=move |e| e.prevent_default()
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
                    <Show when=move || final_result.with(Vec::is_empty)><p role="status" class="p-3">{t_string!(i18n, a11y_no_worlds)}</p></Show>
                </div>
            }
        }
    };

    view! {
        <div class="relative">
            <input
                node_ref=input
                id=input_id
                class=combined_input_class
                class:cursor=move || !has_focus()
                style:padding-left=move || if !has_focus() && choice.get().is_some() && selected_prefix.is_some() { "2.5rem" } else { "" }
                on:focus=move |_| {
                    // Re-measure before opening: the bounding signals start at
                    // zero when the node ref was already set before the
                    // watcher's first run (hydration).
                    update_dropdown_position.with_value(|update| update());
                    set_current_input(choice.get_untracked().map(|value| as_label(&value)).unwrap_or_default());
                    set_focused(true);
                    #[cfg(feature = "hydrate")]
                    request_animation_frame(move || {
                        if let Some(input) = input.try_get_untracked().flatten()
                            && document().active_element().is_some_and(|active| active.is_same_node(Some(&input)))
                        { input.select(); }
                    });
                    let render_idx = highlight_current_choice();
                    scroll_highlight_into_view(render_idx);
                }
                on:focusout=move |_| { set_focused(false); set_current_input(String::new()); }
                on:input=move |e| {
                    // Read the browser value before notifying any reactive
                    // property bindings, which may restore the previous value.
                    let value = event_target_value(&e);
                    set_current_input(value);
                    set_focused(true);
                    set_highlighted_index(0);
                }
                on:keydown=keydown
                aria-label=move || label.get()
                aria-controls=move || has_focus().then(|| format!("{}-list", id.get()))
                on:click=move |_| {
                    update_dropdown_position.with_value(|update| update());
                    if !has_focus.get_untracked() {
                        set_current_input(choice.get_untracked().map(|value| as_label(&value)).unwrap_or_default());
                        set_focused(true);
                        highlight_current_choice();
                        #[cfg(feature = "hydrate")]
                        request_animation_frame(move || {
                            if let Some(input) = input.try_get_untracked().flatten()
                                && document().active_element().is_some_and(|active| active.is_same_node(Some(&input)))
                            { input.select(); }
                        });
                    }
                }
                // Serialize the selected value for SSR; the property binding
                // keeps the editable query current after hydration.
                value=move || current_choice_view().unwrap_or_default()
                prop:value=move || if has_focus() { current_input.get() } else { current_choice_view().unwrap_or_default() }
                // Keep the selection visible when a search query is cleared.
                placeholder=move || current_choice_view().unwrap_or_else(|| t_string!(i18n, a11y_choose_world).to_string())
                role="combobox"
                aria-autocomplete="list"
                aria-expanded=move || (has_focus()).to_string()
                aria-activedescendant=move || (has_focus() && highlighted_index() < final_result.with(Vec::len)).then(|| format!("{}-item-{}", id.get(), highlighted_index()))
            />
            {move || if !has_focus() { choice.get().and_then(|value| selected_prefix.map(|prefix| view! { <span aria-hidden="true" class="absolute inset-y-0 left-3 flex items-center pointer-events-none">{prefix.run(value)}</span> })) } else { None }}
            <div
                class="absolute inset-y-0 right-0 flex items-center pr-3 text-[color:var(--color-text-muted)] pointer-events-none"
                aria-hidden="true"
            >
                <Icon
                    icon=i::BsChevronDown
                    attr:class=move || {
                        if has_focus() {
                            "transition-transform duration-200 rotate-180"
                        } else {
                            "transition-transform duration-200"
                        }
                    }
                />
            </div>
            {move || {
                #[cfg(feature = "hydrate")]
                { has_focus().then(|| {
                    let dropdown_panel = dropdown_panel.clone();
                    let mount = input.get_untracked().and_then(|el| el.closest("dialog").ok().flatten());
                    view! { <Portal mount=mount.unwrap_or_else(|| document().body().expect("document body").into())>{dropdown_panel.clone()}</Portal> }
                }) }
                #[cfg(not(feature = "hydrate"))]
                { let _ = &dropdown_panel; None::<AnyView> }
            }}
        </div>
    }
    .into_any()
}
