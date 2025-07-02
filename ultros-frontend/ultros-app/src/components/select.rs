use leptos::{
    either::Either,
    html::{Div, Input},
    prelude::*,
    reactive::wrappers::write::SignalSetter,
};
use leptos_use::use_element_hover;
use web_sys::wasm_bindgen::JsCast;
use web_sys::KeyboardEvent;

use crate::components::search_result::MatchFormatter;

use super::search_box::fuzzy_search;

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
    let hovered = use_element_hover(dropdown);
    let labels = Memo::new(move |_| {
        items.with(|i| {
            i.iter()
                .map(|i| as_label(i))
                .enumerate()
                .collect::<Vec<_>>()
        })
    });
    let search_results = Memo::new(move |_| {
        current_input.with(|input| {
            let mut results = labels.with(|s| {
                s.iter()
                    .filter_map(|(i, label)| {
                        fuzzy_search(input, label).map(|m| (*i, label.clone(), m))
                    })
                    .collect::<Vec<_>>()
            });
            results.sort_by_key(|(_, _, l)| l.score());
            results
                .into_iter()
                .map(|(i, l, _)| (i, l))
                .collect::<Vec<_>>()
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
    let keydown = move |e: KeyboardEvent| {
        if e.key() == "Enter" {
            if let Some(id) = search_results.with_untracked(|s| s.first().map(|(i, _)| *i)) {
                if let Some(item) = items.with(|i| i.get(id).cloned()) {
                    set_choice(Some(item));
                    set_current_input("".to_string());
                    if let Some(element) = document()
                        .active_element()
                        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
                    {
                        element.blur().unwrap();
                    }
                    let input = input.get_untracked().unwrap();
                    input.focus().unwrap();
                    input.blur().unwrap();
                }
            }
        }
    };

    let default_input_class = "p-2 rounded-lg bg-violet-950 \
                             border border-violet-800/30 w-full \
                             hover:bg-violet-900 hover:border-violet-700/50 \
                             focus:bg-violet-900/90 focus:border-violet-600/50 \
                             transition-colors duration-200 outline-none";

    let default_dropdown_class = "absolute w-full max-h-96 overflow-y-auto top-12 \
                                bg-gradient-to-br from-violet-950/95 to-violet-900/95 \
                                border border-violet-800/30 rounded-lg \
                                shadow-lg shadow-violet-950/50 \
                                backdrop-blur-md z-[100]";

    let current_choice_view = move || {
        choice()
            .map(|c| children(c.clone(), as_label(&c).into_any()))
            .into_any()
    };

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
                <For each=final_result key=move |(l, _)| *l let:data>
                    <button
                        class="w-full text-left"
                        on:click=move |_| {
                            if let Some(item) = items.with(|i| i.get(data.0).cloned()) {
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
                            let is_selected = choice
                                .with(|choice| {
                                    choice
                                        .as_ref()
                                        .and_then(|choice| {
                                            items.with(|i| i.get(data.0).map(|item| item == choice))
                                        })
                                })
                                .unwrap_or_default();
                            if is_selected {
                                "flex items-center bg-violet-800/50 rounded-lg p-2 transition-colors duration-200"
                            } else {
                                "flex items-center hover:bg-violet-800/30 rounded-lg p-2 transition-colors duration-200"
                            }
                        }>
                            {move || items
                                .with(|i| i.get(data.0).cloned())
                                .map(|c| children(
                                    c,
                                    {
                                        if let Some(m) = fuzzy_search(&current_input(), &data.1) {
                                            let target = data.1.clone();
                                            Either::Left(view! { <MatchFormatter m=m target=target /> })
                                        } else {
                                            Either::Right(view! { <div>{data.1.to_string()}</div> })
                                        }.into_any()
                                    }
                                ))}
                        </div>
                    </button>
                </For>
            </div>
        </div>
    }.into_any()
}
