use crate::search_result::*;
use leptos::*;

#[component]
pub fn SearchBox(cx: Scope) -> impl IntoView {
    let (search, set_search) = create_signal(cx, String::new());
    let (active, set_active) = create_signal(cx, false);
    let on_input = move |ev| {
        set_search(event_target_value(&ev));
        set_active(true);
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
                .take(25)
                .collect::<Vec<_>>()
        })
    };
    let clear_search = move || {
        set_search.set("".to_string());
    };
    view! {
        cx,
        <div on:focus=focus_in on:focusout=focus_out >
            <input on:input=on_input class="search-box" type="text" value=search class:active={move || !search.get().is_empty()}/>
            <div class="search-results" on:focus=focus_in> // on:focusout=focus_out // TODO Figure out how to replicate search.js's timer
            <For
                each=item_search
                key=|(id, _)| id.0
                view=move |(id, item): (&xiv_gen::ItemId, &xiv_gen::Item)| {
                        let item_id = id.0;
                        view! { cx,  <ItemSearchResult item_id item set_search /> }
                    }
            />
            </div>
        </div>
    }
}
