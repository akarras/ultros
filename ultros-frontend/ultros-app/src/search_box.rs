use crate::components::search_result::*;
use leptos::*;

#[component]
pub fn SearchBox(cx: Scope) -> impl IntoView {
    let (search, set_search) = create_signal(cx, String::new());
    let (active, set_active) = create_signal(cx, false);
    let on_input = move |ev| {
        set_search(event_target_value(&ev));
    };
    let focus_in = move |_| set_active(true);
    let focus_out = move |_| set_active(false);
    let items = &xiv_gen_db::decompress_data().items;
    let item_search = move || {
        search.with(|s| {
            items
                .into_iter()
                .filter(|(_, i)| i.item_search_category.0 > 0)
                .filter(|(_, i)| i.name.to_lowercase().contains(&s.to_lowercase()))
                .filter(|_| !s.is_empty())
                .take(25)
                .collect::<Vec<_>>()
        })
    };
    view! {
        cx,
        <div>
            <input on:input=on_input on:focusin=focus_in on:focusout=focus_out class="search-box" type="text" prop:value=search class:active={move || active()}/>
            <div class="search-results">
            <For
                each=item_search
                key=|(id, _)| id.0
                view=move |(id, item): (&xiv_gen::ItemId, &xiv_gen::Item)| {
                        let item_id = id.0;
                        view! { cx,  <ItemSearchResult item_id set_search /> }
                    }
            />
            </div>
        </div>
    }
}
