use std::cmp::Reverse;

use crate::components::{search_result::*, virtual_scroller::*};
use gloo_timers::future::TimeoutFuture;
use leptos::*;
use leptos_icons::*;
use sublime_fuzzy::{FuzzySearch, Match, Scoring};

pub(crate) fn fuzzy_search(query: &str, target: &str) -> Option<Match> {
    let scoring = Scoring::emphasize_distance();
    let search = FuzzySearch::new(query, target)
        .case_insensitive()
        .score_with(&scoring);
    search.best_match()
}

/// SearchBox primarily searches through item names- there might be better ways to filter the views down the line.
#[component]
pub fn SearchBox() -> impl IntoView {
    let (search, set_search) = create_signal(String::new());
    let (active, set_active) = create_signal(false);
    let on_input = move |ev| {
        set_search(event_target_value(&ev));
    };
    let focus_in = move |_| set_active(true);
    let focus_out = move |_| {
        spawn_local(async move {
            TimeoutFuture::new(250).await;
            set_active(false);
        })
    };
    let items = &xiv_gen_db::data().items;
    let item_search = move || {
        search.with(|s| {
            if s.is_empty() {
                return vec![];
            }
            let mut score = items
                .iter()
                .filter(|(_, i)| i.item_search_category.0 > 0)
                .filter(|_| !s.is_empty())
                .flat_map(|(id, i)| fuzzy_search(s, &i.name).map(|m| (id, i, m)))
                .collect::<Vec<_>>();
            score.sort_by_key(|(_, i, m)| (Reverse(m.score()), Reverse(i.level_item.0)));
            score
                .into_iter()
                .map(|(id, item, _ma)| (id, item))
                .collect::<Vec<_>>()
        })
    };
    view! {

        <div class="absolute top-0 left-0 right-0 sm:relative" style="height: 36px;">
            <input on:input=on_input on:focusin=focus_in on:focusout=focus_out class="search-box w-screen m-0 sm:w-[424px]" type="text" prop:value=search class:active={active}/>
            <div class="absolute right-4 top-4 z-10"><Icon icon=Icon::from(AiIcon::AiSearchOutlined) /></div>
            <div class="search-results w-screen sm:w-[424px] z-50">
            // WHY DOES THIS BREAK HYDRATION?
            // <WasmLoadingIndicator />
            <VirtualScroller
                each=Signal::derive(item_search)
                key=move |(id, _item)| id.0
                view=move |(id, _): (&xiv_gen::ItemId, &xiv_gen::Item)| {
                        let item_id = id.0;
                        view! {  <ItemSearchResult item_id set_search search /> }
                    }
                viewport_height=500.0
                row_height=42.0
            />
            </div>
        </div>
    }
}
