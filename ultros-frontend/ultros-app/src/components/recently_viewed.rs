use std::collections::VecDeque;

use leptos::*;
use leptos_use::storage::use_local_storage;

use crate::components::{search_result::ItemSearchResult, skeleton::BoxSkeleton};

#[derive(Clone, Copy)]
pub struct RecentItems {
    read_signal: Signal<VecDeque<i32>>,
    write_signal: WriteSignal<VecDeque<i32>>,
}

impl RecentItems {
    pub fn new() -> Self {
        use leptos_use::utils::JsonCodec;

        let (read_signal, write_signal, _delete_fn) =
            use_local_storage::<VecDeque<i32>, JsonCodec>("recently_viewed");
        Self {
            read_signal,
            write_signal,
        }
    }

    pub fn reader(&self) -> Signal<VecDeque<i32>> {
        self.read_signal
    }

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
}

#[component]
pub fn RecentlyViewed() -> impl IntoView {
    let item_data = use_context::<RecentItems>().unwrap();
    let items = item_data.reader();
    let local_items = create_local_resource(move || items(), move |items| async move { items });
    let (empty_search, set_empty_search) = create_signal("".to_string());

    view! {
        <Suspense fallback=move || view!{<div class="h-[400px]"><BoxSkeleton/></div>}>
            <div class:hidden=move || {
                local_items.with(|i| i.as_ref().map(|i| i.is_empty()).unwrap_or(true))
            }>
                <h4 class="text-lg">"Recently Viewed"</h4>
                <div class="flex flex-col">
                    {move || {
                        let items = local_items();
                        Some(items?.iter().map(|item| {
                            view!{ <ItemSearchResult item_id=*item search=empty_search set_search=set_empty_search /> }
                        }).collect::<Vec<_>>())
                    }}
                    {}
                </div>
            </div>
        </Suspense>
    }
}
