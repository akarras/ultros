use std::cmp::Reverse;

use crate::components::{search_result::*, virtual_scroller::*};
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
    let item_search = create_memo(cx, move |_| {
        search.with(|s| {
            let mut score = items
                .into_iter()
                .filter(|(_, i)| i.item_search_category.0 > 0)
                .filter(|_| !s.is_empty())
                .flat_map(|(id, i)| sublime_fuzzy::best_match(s, &i.name).map(|m| (id, i, m)))
                .collect::<Vec<_>>();
            score.sort_by_key(|(_, i, m)| (Reverse(m.score()), Reverse(i.level_item.0)));
            score
                .into_iter()
                // .filter(|(_, _, ma)| ma.score() > 0)
                .map(|(id, item, _ma)| (id, item))
                // .take(100)
                .collect::<Vec<_>>()
        })
    });
    view! {
        cx,
        <div style="height: 36px;">
            <input on:input=on_input on:focusin=focus_in on:focusout=focus_out class="search-box" type="text" prop:value=search class:active={move || active()}/>
            <div class="search-results">
            <VirtualScroller
                each=item_search.into()
                key=move |(id, _item)| id.0
                view=move |cx, (id, _): (&xiv_gen::ItemId, &xiv_gen::Item)| {
                        let item_id = id.0;
                        view! { cx,  <ItemSearchResult item_id set_search search /> }
                    }
                viewport_height=500.0
                row_height=42.0
            />
            </div>
        </div>
    }
}
