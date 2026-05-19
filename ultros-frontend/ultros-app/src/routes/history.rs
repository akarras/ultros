use crate::components::item_icon::ItemIcon;
use crate::components::meta::{MetaDescription, MetaRobotsNoIndex, MetaTitle};
use crate::components::recently_viewed::RecentItems;
use crate::components::skeleton::BoxSkeleton;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use leptos::either::Either;
use leptos::prelude::*;
use leptos_router::components::A;
use ultros_api_types::icon_size::IconSize;
use xiv_gen::ItemId;

#[component]
pub fn History() -> impl IntoView {
    let i18n = use_i18n();
    let item_data = use_context::<RecentItems>().unwrap();
    let items = item_data.reader();
    // Wrap the localStorage-backed signal in a LocalResource so SSR renders the
    // Suspense fallback (matching CSR's initial state) and the actual list only
    // materializes on the client. Reading `items()` directly here caused an
    // Either::Left vs Either::Right shape divergence between SSR and CSR, which
    // tachys's hydrator surfaces as an unrecoverable panic.
    let local_items = LocalResource::new(move || async move { items() });
    view! {
        <div class="main-content p-6">
            <MetaTitle title=move || t_string!(i18n, history_meta_title).to_string() />
            <MetaDescription text=move || t_string!(i18n, history_meta_desc).to_string() />
            <MetaRobotsNoIndex />

            <div class="container mx-auto max-w-7xl space-y-6">
                <div class="flex items-center justify-between">
                    <h1 class="text-3xl font-bold text-[color:var(--brand-fg)]">{t!(i18n, history_title)}</h1>
                    <button
                        class="btn-secondary"
                        on:click=move |_| item_data.clear_items()
                    >
                        {t!(i18n, history_clear)}
                    </button>
                </div>

                <div class="panel p-6 rounded-xl">
                    <Suspense fallback=move || {
                        view! {
                            <div class="h-[400px] animate-pulse">
                                <BoxSkeleton />
                            </div>
                        }
                    }>
                        {move || {
                            local_items
                                .get()
                                .map(|current_items| {
                                    if current_items.is_empty() {
                                        Either::Left(
                                            view! {
                                                <div class="text-center py-12">
                                                    <p class="text-lg text-[color:var(--color-text-muted)]">
                                                        {t!(i18n, history_empty)}
                                                    </p>
                                                    <p class="text-sm text-[color:var(--color-text-muted)] mt-2">
                                                        {t!(i18n, history_empty_hint)}
                                                    </p>
                                                </div>
                                            },
                                        )
                                    } else {
                                        Either::Right(
                                            view! {
                                                <div class="space-y-2">
                                                    {current_items
                                                        .iter()
                                                        .map(|item| {
                                                            let item_id = *item;
                                                            let item_data = tracked_data()
                                                                .items
                                                                .get(&ItemId(item_id));

                                                            view! {
                                                                <A href=format!("/item/{item_id}")>
                                                                    <div class="flex items-center gap-4 p-3 card transition-colors duration-200 hover:translate-x-1">
                                                                        <ItemIcon item_id icon_size=IconSize::Medium />

                                                                        <div class="flex flex-col min-w-0 flex-1">
                                                                            <div class="flex items-center gap-2">
                                                                                <span class="text-[color:var(--color-text)]">
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
                                            },
                                        )
                                    }
                                })
                        }}
                    </Suspense>
                </div>
            </div>
        </div>
    }.into_any()
}
