use leptos::{html::Div, *};
#[cfg(feature = "hydrate")]
use leptos_use::use_element_hover;

use crate::components::search_result::MatchFormatter;

use super::search_box::fuzzy_search;

#[component]
pub fn Select<T, EF, N, L>(
    items: Signal<Vec<T>>,
    as_label: L,
    choice: Signal<Option<T>>,
    set_choice: SignalSetter<Option<T>>,
    children: EF,
) -> impl IntoView
where
    T: Clone + Eq + 'static,
    EF: Fn(T, View) -> N + 'static + Copy,
    N: IntoView + 'static,
    L: Fn(&T) -> String + 'static + Copy,
{
    let (current_input, set_current_input) = create_signal("".to_string());
    let (has_focus, set_focused) = create_signal(false);
    let dropdown = create_node_ref::<Div>();
    #[cfg(feature = "hydrate")]
    let hovered = use_element_hover(dropdown);
    #[cfg(not(feature = "hydrate"))]
    let hovered: Signal<bool> = create_memo(move |_| false).into();
    #[cfg(not(feature = "hydrate"))]
    let _dropdown = dropdown;
    let labels = create_memo(move |_| {
        items.with(|i| {
            i.iter()
                .map(|i| as_label(i))
                .enumerate()
                .collect::<Vec<_>>()
        })
    });
    let search_results = create_memo(move |_| {
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
    let final_result = create_memo(move |_| {
        let search_results = search_results();
        if search_results.is_empty() {
            labels()
        } else {
            search_results
        }
    });
    view! {
        <div class="relative">
            <input class="p-2 rounded-sm bg-purple-900 border-solid border-purple-950 w-96"
                on:focus=move |_| set_focused(true)
                on:focusout=move |_| set_focused(false)
                on:input=move |e| { set_current_input(event_target_value(&e)); }
                prop:value=current_input />
            <div class="absolute top-2 left-2 select-none" class:hidden=has_focus>
                {move || choice().map(|c| {
                    children(c.clone(), as_label(&c).into_view())
                })}
            </div>
            <div node_ref=dropdown class:hidden=move || !has_focus() && !hovered()
                class="focus-within:visible absolute w-96 h-96 overflow-y-auto top-10 bg-purple-950">
                <For each=final_result
                    key=move |(l, _)| *l
                    let:data
                >
                    <button class="flex flex-col w-full" on:click=move |_| {
                        if let Some(item) = items.with(|i| i.get(data.0).cloned()) {
                            set_choice(Some(item));
                            set_focused(false);
                            set_current_input("".to_string());
                        }
                    }>
                        <div class="hover:bg-purple-700 hover:border-solid hover:border-violet-600 rounded-sm p-2" class:bg-purple-500=move || {
                            choice.with(|choice| choice.as_ref().and_then(|choice| items.with(|i| i.get(data.0).map(|item| item == choice)))).unwrap_or_default()
                        }>{items.with(|i| i.get(data.0).cloned()).map(|c| children(c, {move || {
                            if let Some(m) = fuzzy_search(&current_input(), &data.1){
                                let target = data.1.clone();
                                view!{ <div class="flex flex-row"><MatchFormatter m=m target=target /></div> }
                            } else {
                                view!{ <div>{&data.1}</div>}
                            }
                        }}.into_view()))}</div>
                    </button>
                </For>
            </div>
        </div>
    }
}
