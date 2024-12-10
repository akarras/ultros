use crate::components::item_icon::ItemIcon;
use crate::components::meta::{MetaDescription, MetaTitle};
use crate::components::recently_viewed::RecentItems;
use crate::components::skeleton::BoxSkeleton;
use leptos::either::Either;
use leptos::prelude::*;
use leptos_router::components::A;
use ultros_api_types::icon_size::IconSize;
use xiv_gen::ItemId;

#[component]
pub fn History() -> impl IntoView {
    let item_data = use_context::<RecentItems>().unwrap();
    let items = item_data.reader();
    view! {
        <div class="main-content p-6">
            <MetaTitle title="History - Ultros"/>
            <MetaDescription text="View your recently viewed items on Ultros"/>

            <div class="container mx-auto max-w-7xl space-y-6">
                <div class="flex items-center justify-between">
                    <h1 class="text-3xl font-bold text-amber-200">"Viewing History"</h1>
                    <button
                        class="px-4 py-2 rounded-lg bg-violet-600/20 hover:bg-violet-600/30
                                   border border-violet-400/10 hover:border-violet-400/20
                                   transition-all duration-300 text-gray-200 hover:text-amber-200"
                        on:click=move |_| item_data.clear_items()
                    >
                        "Clear History"
                    </button>
                </div>

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
                        {move || {
                            let current_items = items();
                            if current_items.is_empty() {
                                Either::Left(view! {
                                    <div class="text-center py-12">
                                        <p class="text-lg text-gray-400">
                                            "No items in your viewing history yet."
                                        </p>
                                        <p class="text-sm text-gray-500 mt-2">
                                            "Items you view will appear here."
                                        </p>
                                    </div>
                                })
                            } else {
                                Either::Right(view! {
                                    <div class="space-y-2">
                                        {current_items
                                            .iter()
                                            .map(|item| {
                                                let item_id = *item;
                                                let item_data = xiv_gen_db::data().items.get(&ItemId(item_id));

                                                view! {
                                                    <A href=format!("/item/{item_id}")>
                                                        <div class="flex items-center gap-4 p-3 rounded-lg
                                                                       bg-violet-950/30 border border-white/5
                                                                       hover:bg-violet-900/30 hover:border-white/10
                                                                       transition-all duration-200 hover:translate-x-1">
                                                            <ItemIcon item_id icon_size=IconSize::Medium/>

                                                            <div class="flex flex-col min-w-0 flex-1">
                                                                <div class="flex items-center gap-2">
                                                                    <span class="text-gray-200">
                                                                        {item_data.map(|i| i.name.as_str()).unwrap_or_default()}
                                                                    </span>
                                                                </div>
                                                            </div>
                                                        </div>
                                                    </A>
                                                }
                                            })
                                            .collect_view()}
                                    </div>
                                })
                            }
                        }}
                    </Suspense>
                </div>
            </div>
        </div>
    }
}
