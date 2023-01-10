use std::cmp::Reverse;

use crate::components::search_result::*;
use leptos::*;
use sublime_fuzzy::Match;

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
            let mut score = items
                .into_iter()
                .filter(|(_, i)| i.item_search_category.0 > 0)
                .filter(|_| !s.is_empty())
                .flat_map(|(id, i)| sublime_fuzzy::best_match(s, &i.name).map(|m| (id, i, m)))
                .collect::<Vec<_>>();
            score.sort_by_key(|(_, _, m)| Reverse(m.score()));
            score
                .into_iter()
                .filter(|(_, _, ma)| ma.score() > 0)
                .map(|(id, item, ma)| (id, item, ma))
                .take(50)
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
                key=move |(id, _, _)| (id.0, search())
                view=move |(id, _, match_): (&xiv_gen::ItemId, &xiv_gen::Item, Match)| {
                        let item_id = id.0;
                        view! { cx,  <ItemSearchResult item_id set_search matches=match_ /> }
                    }
            />
            </div>
        </div>
    }
}
