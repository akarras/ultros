use std::collections::VecDeque;

use codee::string::JsonSerdeCodec;
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_use::storage::use_local_storage;
use ultros_api_types::icon_size::IconSize;
use xiv_gen::ItemId;

use crate::components::{
    item_icon::ItemIcon, search_result::ItemSearchResult, skeleton::BoxSkeleton,
};

#[derive(Clone, Copy)]
pub struct RecentItems {
    read_signal: Signal<VecDeque<i32>>,
    write_signal: WriteSignal<VecDeque<i32>>,
}

impl RecentItems {
    pub fn new() -> Self {
        let (read_signal, write_signal, _delete_fn) =
            use_local_storage::<VecDeque<i32>, JsonSerdeCodec>("recently_viewed");
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
            if items.len() > 1000 {
                items.pop_back();
            }
        });
    }

    pub fn clear_items(&self) {
        self.write_signal.update(|items| items.clear());
    }
}

#[component]
pub fn RecentlyViewed() -> impl IntoView {
    let item_data = use_context::<RecentItems>().unwrap();
    let items = item_data.reader();
    let local_items = LocalResource::new(move || async move { items() });
    view! {
        <div class="p-6 rounded-xl bg-gradient-to-br from-violet-950/20 to-violet-900/20
                    border border-white/10 backdrop-blur-sm">
            <Suspense
                fallback=move || {
                    view! {
                        <div class="h-[400px] animate-pulse">
                            <BoxSkeleton/>
                        </div>
                    }
                }
            >
                <div
                    class="space-y-4"
                    class:hidden=move || {
                        local_items.with(|i| i.as_ref().map(|i| i.is_empty()).unwrap_or(true))
                    }
                >
                    <div class="flex items-center justify-between">
                        <h4 class="text-xl font-bold text-amber-200">"Recently Viewed"</h4>
                        <button
                            class="text-sm text-gray-400 hover:text-amber-200 transition-colors"
                            on:click=move |_| item_data.clear_items()
                        >
                            "Clear All"
                        </button>
                    </div>

                    <div class="space-y-2 max-h-[400px] overflow-y-auto overflow-x-hidden
                              scrollbar-thin scrollbar-thumb-violet-600/50 scrollbar-track-transparent">
                        {move || {
                            let items = local_items.get();
                            Some(
                                items?
                                    .iter()
                                    .map(|item| {
                                        let item_id = *item;
                                        let item_data = xiv_gen_db::data().items.get(&ItemId(item_id));

                                        view! {
                                            <A href=format!("/item/{item_id}")>

                                                <div class="flex items-center gap-4 p-3 rounded-lg
                                                           bg-violet-950/30 border border-white/5
                                                           hover:bg-violet-900/30 hover:border-white/10
                                                           transition-all duration-200 group">
                                                    <div class="flex items-center gap-4 w-full transform transition-transform duration-200 group-hover:translate-x-1">
                                                        <ItemIcon item_id icon_size=IconSize::Medium/>

                                                        <div class="flex flex-col min-w-0 flex-1">
                                                            <div class="flex items-center gap-2 truncate">
                                                                <span class="text-gray-200 truncate">
                                                                    {item_data.map(|i| i.name.as_str()).unwrap_or_default()}
                                                                </span>
                                                            </div>
                                                        </div>
                                                    </div>
                                                </div>
                                            </A>
                                        }
                                    })
                                    .collect::<Vec<_>>(),
                            )
                        }}
                    </div>

                    <div class="text-center pt-2 border-t border-white/5">
                        <a href="/history"
                           class="text-sm text-gray-400 hover:text-amber-200 transition-colors">
                            "View All Recently Viewed"
                        </a>
                    </div>
                </div>
            </Suspense>
        </div>
    }
}
