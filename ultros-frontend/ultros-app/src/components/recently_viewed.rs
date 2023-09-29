use std::collections::VecDeque;

use leptos::*;
#[cfg(feature = "hydrate")]
use leptos_use::storage::use_local_storage;

use crate::components::search_result::ItemSearchResult;

#[derive(Clone, Copy)]
pub struct RecentItems {
    #[cfg(feature = "hydrate")]
    read_signal: Signal<VecDeque<i32>>,
    #[cfg(feature = "hydrate")]
    write_signal: WriteSignal<VecDeque<i32>>,
}

impl RecentItems {
    #[cfg(feature = "hydrate")]
    pub fn new() -> Self {
        let (read_signal, write_signal, _delete_fn) =
            use_local_storage::<VecDeque<i32>, _>("recently_viewed", VecDeque::new());
        Self {
            read_signal,
            write_signal,
        }
    }

    #[cfg(not(feature = "hydrate"))]
    pub fn new() -> Self {
        Self {}
    }

    #[cfg(feature = "hydrate")]
    pub fn reader(&self) -> Signal<VecDeque<i32>> {
        self.read_signal
    }

    #[cfg(not(feature = "hydrate"))]
    pub fn reader(&self) -> Signal<VecDeque<i32>> {
        create_memo(|_| VecDeque::new()).into()
    }

    #[cfg(feature = "hydrate")]
    pub fn add_item(&self, item_id: i32) {
        use itertools::Itertools;

        self.write_signal.update(|items| {
            items.push_front(item_id);
            *items = items.iter().copied().unique().collect();
            if items.len() > 10 {
                items.pop_back();
            }
        });
    }

    #[cfg(not(feature = "hydrate"))]
    pub fn add_item(&self, _item_id: i32) {
        use log::warn;

        warn!("added item to recently view ssr side.");
    }
}

#[component]
pub fn RecentlyViewed() -> impl IntoView {
    let item_data = use_context::<RecentItems>().unwrap();
    let items = item_data.reader();
    let local_items = create_local_resource(move || items(), move |items| async move { items });
    let (empty_search, set_empty_search) = create_signal("".to_string());
    view! {
        <div>
            <h4 class="text-lg">"Recently Viewed"</h4>
            <div class="flex flex-col">
                {move || {
                    let items = local_items();
                    Some(items?.iter().map(|item| {
                        view!{ <ItemSearchResult item_id=*item search=empty_search set_search=set_empty_search /> }
                    }).collect::<Vec<_>>())
                }}
            </div>
        </div>
    }
}
