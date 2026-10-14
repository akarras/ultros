//! Read-only list surfaces for the operator kill-switch
//! (`ULTROS_LISTS_READONLY`, see `ultros_server_core::lists_readonly`).
//!
//! These components are the rollback lever for the Lists 2.0 promotion:
//! they read only the REST projection (`list` / `list_item`, which
//! `ListSync` maintains from every accepted document update) and never
//! open a list document, subscribe to the sync socket, or write anything.
//! They render only while the switch is on; the normal editor is untouched.

use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use ultros_api_types::list::ListItem;
use xiv_gen::ItemId;

use crate::api::{get_list_items_with_listings, get_lists_with_permissions};
use crate::components::{
    item_icon::{IconSize, ItemIcon},
    meta::{MetaRobotsNoIndex, MetaTitle},
    skeleton::BoxSkeleton,
};
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;

/// The paused-editing banner shared by every list route while the
/// kill-switch is on.
#[component]
pub fn ListsPausedBanner() -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <div class="panel rounded-xl p-5 space-y-2" role="status" data-testid="lists-paused">
            <h2 class="text-xl font-bold text-[color:var(--brand-fg)]">
                {t!(i18n, lists_paused_title)}
            </h2>
            <p class="text-sm text-[color:var(--color-text-muted)]">
                {t!(i18n, lists_paused_body)}
            </p>
        </div>
    }
}

fn row_progress(item: &ListItem) -> String {
    let acquired = item.acquired.unwrap_or(0);
    match item.quantity {
        Some(quantity) => format!("{acquired} / {quantity}"),
        None => acquired.to_string(),
    }
}

/// `/list/:id` while paused: the list's projected contents, read-only.
#[component]
pub fn ListReadOnly() -> impl IntoView {
    let i18n = use_i18n();
    let params = use_params_map();
    let id = Memo::new(move |_| {
        params
            .with(|p| p.get("id").unwrap_or_default())
            .parse::<i32>()
            .unwrap_or_default()
    });
    let list_view = Resource::new(move || id.get(), get_list_items_with_listings);
    view! {
        <MetaRobotsNoIndex />
        <div class="space-y-4">
            <ListsPausedBanner />
            <Suspense fallback=move || view! { <BoxSkeleton rows=3 /> }>
                {move || match list_view.get() {
                    None => view! { <BoxSkeleton rows=3 /> }.into_any(),
                    Some(Err(error)) => view! {
                        <p role="alert" class="text-negative">
                            {t!(i18n, error_loading_lists, error = error.to_string())}
                        </p>
                    }.into_any(),
                    Some(Ok((list, items))) => {
                        let title = list.list.name.clone();
                        view! {
                            <MetaTitle title=title.clone() />
                            <section class="space-y-3" data-testid="list-readonly">
                                <h1 class="text-2xl font-bold">{title}</h1>
                                <ul class="divide-y divide-[color:var(--color-outline)]">
                                    {items.into_iter().map(|(item, _listings)| {
                                        let name = tracked_data()
                                            .items
                                            .get(&ItemId(item.item_id))
                                            .map(|entry| entry.name.to_string())
                                            .unwrap_or_default();
                                        let progress = row_progress(&item);
                                        let hq = item.hq == Some(true);
                                        view! {
                                            <li class="flex items-center gap-3 py-2">
                                                <ItemIcon item_id=item.item_id icon_size=IconSize::Small />
                                                <span class="min-w-0 flex-1 truncate">{name}</span>
                                                <Show when=move || hq>
                                                    <span class="text-xs font-semibold">{t!(i18n, hq)}</span>
                                                </Show>
                                                <span class="text-sm text-[color:var(--color-text-muted)]">
                                                    {t!(i18n, lists_workspace_owned)} " " {progress}
                                                </span>
                                            </li>
                                        }
                                    }).collect_view()}
                                </ul>
                            </section>
                        }.into_any()
                    }
                }}
            </Suspense>
        </div>
    }
}

/// `/list` while paused: account lists stay reachable read-only; device
/// lists are not opened at all.
#[component]
pub fn ListsReadOnlyDirectory() -> impl IntoView {
    let i18n = use_i18n();
    let accounts = Resource::new(|| (), |_| get_lists_with_permissions());
    view! {
        <div class="space-y-4">
            <ListsPausedBanner />
            <p class="text-sm text-[color:var(--color-text-muted)]">
                {t!(i18n, lists_paused_device_body)}
            </p>
            <Suspense fallback=move || view! { <BoxSkeleton rows=2 /> }>
                {move || match accounts.get() {
                    None => view! { <BoxSkeleton rows=2 /> }.into_any(),
                    Some(Err(_)) => ().into_any(),
                    Some(Ok(lists)) => view! {
                        <ul class="space-y-2" data-testid="lists-readonly-directory">
                            {lists.into_iter().map(|entry| {
                                let href = format!("/list/{}", entry.list.id);
                                view! {
                                    <li>
                                        <a class="btn-secondary min-h-11 w-full justify-start" href=href>
                                            {entry.list.name}
                                        </a>
                                    </li>
                                }
                            }).collect_view()}
                        </ul>
                    }.into_any(),
                }}
            </Suspense>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(quantity: Option<i32>, acquired: Option<i32>) -> ListItem {
        ListItem {
            id: 1,
            item_id: 2,
            list_id: 3,
            hq: None,
            quantity,
            acquired,
            target_price: None,
        }
    }

    #[test]
    fn progress_shows_acquired_against_quantity_when_both_exist() {
        assert_eq!(row_progress(&item(Some(10), Some(4))), "4 / 10");
        assert_eq!(row_progress(&item(Some(10), None)), "0 / 10");
        assert_eq!(row_progress(&item(None, Some(4))), "4");
        assert_eq!(row_progress(&item(None, None)), "0");
    }
}
