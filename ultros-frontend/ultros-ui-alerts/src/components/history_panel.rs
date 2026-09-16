use icondata as i;
use leptos::{prelude::*, task::spawn_local};
use ultros_api_types::alert::AlertEvent;
use ultros_ui::components::relative_time::RelativeToNow;
use xiv_gen::ItemId;

use crate::api::{get_alert_events_page, resend_alert_event};
use crate::components::icon::Icon;
use crate::components::loading::Loading;
use crate::components::skeleton::{SkeletonCell, SkeletonColumn, TableSkeleton};
use crate::global_state::notifications::use_inbox;
use crate::global_state::toasts::use_toast;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::{t, t_string, use_i18n};

/// How many rows a page of `get_alert_events_page` asks for. Also the
/// threshold [`has_more`] compares a page's length against: a page shorter
/// than this is the last one.
const PAGE_SIZE: u32 = 50;

/// Skeleton columns matching the table below's six columns, in the same
/// order, so the loading state has the real table's rhythm.
fn history_skeleton_columns() -> Vec<SkeletonColumn> {
    vec![
        SkeletonColumn::new("w-32 p-1", SkeletonCell::Text),
        SkeletonColumn::new("flex-1 min-w-32 p-1", SkeletonCell::IconText),
        SkeletonColumn::new("w-24 p-1", SkeletonCell::Number),
        SkeletonColumn::new("w-16 p-1", SkeletonCell::Badge),
        SkeletonColumn::new("w-12 p-1", SkeletonCell::Badge),
        SkeletonColumn::new("w-20 p-1", SkeletonCell::Blank),
    ]
}

/// Whether a "Load older" control should be offered after a page of
/// `page_len` rows came back for a request that asked for `page_size`. A
/// short page (fewer rows than asked for) is the last page — there is
/// nothing older left to fetch.
fn has_more(page_len: usize, page_size: usize) -> bool {
    page_len >= page_size && page_size > 0
}

/// Resets pagination and re-fetches page one — used both by a manual
/// "Mark all read" and by a successful resend, which can flip an event's
/// `delivered`/`read_at` state in a way this accumulated buffer can't patch
/// in place.
///
/// `try_*` throughout, not the panicking variants: both call sites run this
/// after an `await` (the mark-all-read sync round-trip, the resend request),
/// by which point the panel may already be disposed (e.g. the visitor
/// navigated away before the request resolved) — `try_*` just no-ops instead
/// of panicking on a disposed signal.
fn reset_and_refetch(
    version: RwSignal<u64>,
    before_id: RwSignal<Option<i64>>,
    rows: RwSignal<Vec<AlertEvent>>,
    loading: RwSignal<bool>,
) {
    let _ = before_id.try_set(None);
    let _ = rows.try_set(Vec::new());
    let _ = loading.try_set(true);
    let _ = version.try_update(|v| *v += 1);
}

#[component]
pub fn HistoryPanel() -> impl IntoView {
    let i18n = use_i18n();
    let inbox = use_inbox();
    let toasts = use_toast();

    // `version` forces a full reset (bumped by "Mark all read" and by a
    // successful resend); `before_id` drives "Load older" without touching
    // `version`. Both feed the same resource, and an `Effect` below folds
    // each page it returns into `rows` — replacing on a fresh load
    // (`before_id == None`), appending otherwise — since a plain `Resource`
    // only ever holds the *latest* page, not the accumulated history this
    // panel shows.
    let version = RwSignal::new(0u64);
    let before_id = RwSignal::new(None::<i64>);
    let rows = RwSignal::new(Vec::<AlertEvent>::new());
    let last_page_len = RwSignal::new(0usize);
    // Tracked explicitly rather than derived from `page.get().is_none()`:
    // a `Resource` keeps returning its last-resolved value while a refetch
    // (source key change) is in flight, so `.get()` alone can't tell "no
    // page has ever arrived" apart from "a fresh page is loading and the
    // stale one is still sitting there". Set on every fetch kickoff (initial
    // load, "Load older", `reset_and_refetch`) and cleared once that fetch's
    // result has been folded into `rows` below.
    let loading = RwSignal::new(true);

    let page = Resource::new(
        move || (version.get(), before_id.get()),
        move |(_, before_id)| async move { get_alert_events_page(PAGE_SIZE, before_id).await },
    );

    Effect::new(move |_| {
        let Some(result) = page.get() else {
            return;
        };
        loading.set(false);
        let Ok(new_rows) = result else {
            return;
        };
        last_page_len.set(new_rows.len());
        if before_id.get_untracked().is_none() {
            rows.set(new_rows);
        } else {
            rows.update(|existing| existing.extend(new_rows));
        }
    });

    let mark_all_read = move |_| {
        let Some(inbox) = inbox else {
            return;
        };
        spawn_local(async move {
            // Awaited, not fire-and-forget (`Inbox::mark_all_read`): the
            // optimistic local flip on `inbox` happens synchronously inside
            // `mark_all_read_and_sync` before its first `await`, but the
            // refetch below must not fire until the server's `read_at`
            // write has actually landed — otherwise the page this refetch
            // brings back can still show the pre-mark unread state, and
            // nothing here would ever refetch again to correct it. On a
            // failed server sync the local flip already happened, so the
            // refetch still runs (the view converges on what the local
            // state now says) and the error is logged rather than surfaced,
            // matching every other best-effort sync in this app.
            if let Err(error) = inbox.mark_all_read_and_sync().await {
                log::error!("failed to mark all alert events read: {error}");
            }
            reset_and_refetch(version, before_id, rows, loading);
        });
    };

    let load_more = move |_| {
        if let Some(last) = rows.get_untracked().last() {
            loading.set(true);
            before_id.set(Some(last.id));
        }
    };

    view! {
        <div class="space-y-3">
            <div class="flex justify-end">
                <button class="btn-ghost" on:click=mark_all_read>
                    <Icon icon=i::BsCheck2All />
                    <span class="ml-1">{t!(i18n, inbox_mark_all_read)}</span>
                </button>
            </div>
            {move || {
                if loading.get() && rows.with(Vec::is_empty) {
                    view! { <TableSkeleton columns=history_skeleton_columns() rows=4 /> }.into_any()
                } else if let Some(Err(e)) = page.get() {
                    view! { <div class="text-red-500">{format!("{e}")}</div> }.into_any()
                } else {
                    let events = rows.get();
                    if events.is_empty() {
                        view! {
                            <p class="opacity-70">{t!(i18n, history_no_fires)}</p>
                        }
                            .into_any()
                    } else {
                        view! {
                            <div class="overflow-x-auto">
                                <table class="w-full text-sm">
                                    <thead>
                                        <tr>
                                            <th scope="col" class="text-left p-1">{t!(i18n, col_time)}</th>
                                            <th scope="col" class="text-left p-1">{t!(i18n, history_col_title)}</th>
                                            <th scope="col" class="text-left p-1">{t!(i18n, history_col_matched_price)}</th>
                                            <th scope="col" class="text-left p-1">{t!(i18n, history_col_delivered)}</th>
                                            <th scope="col" class="text-left p-1">{t!(i18n, history_col_read)}</th>
                                            <th scope="col" class="text-left p-1">{t!(i18n, actions)}</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        // ⚡ Bolt Optimization: Using collect_view() instead of <For> to prevent unnecessary cloning of rows inside a conditional block that completely recreates the view.
                                        {events.into_iter().map(|e: AlertEvent| {
                                                let item_name = tracked_data().items.get(&ItemId(e.item_id))
                                                    .map(|it| it.name.as_str().to_string())
                                                    .unwrap_or_else(|| format!("Item {}", e.item_id));
                                                let title_str = e.title.clone().unwrap_or_else(|| item_name.clone());
                                                let body_str = e.body.clone();
                                                let fired_abs = e.fired_at.to_rfc3339();
                                                let price_str = e.matched_price.map(|p| p.to_string()).unwrap_or_else(|| "\u{2014}".into());
                                                let delivered_str = if e.delivered {
                                                    "\u{2713}".to_string()
                                                } else {
                                                    e.delivery_error.as_deref().unwrap_or("\u{2717}").to_string()
                                                };
                                                let read = e.read_at.is_some();
                                                let event_id = e.id;
                                                let delivered = e.delivered;
                                                let label_item_name = item_name.clone();
                                                let (is_resending, set_is_resending) = RwSignal::new(false).split();
                                                view! {
                                                    <tr class="border-t">
                                                        <td class="p-1">
                                                            <RelativeToNow timestamp=e.fired_at.naive_utc() attr:title=fired_abs />
                                                        </td>
                                                        <td class="p-1">
                                                            <div class="font-medium">{title_str}</div>
                                                            {body_str.map(|body| view! {
                                                                <div class="text-xs opacity-60">{body}</div>
                                                            })}
                                                        </td>
                                                        <td class="p-1">{price_str}</td>
                                                        <td class="p-1">{delivered_str}</td>
                                                        <td class="p-1">
                                                            {if read {
                                                                view! { <Icon icon=i::BsCheck2All aria_hidden=true /> }.into_any()
                                                            } else {
                                                                view! {
                                                                    <span
                                                                        class="inline-block w-2 h-2 rounded-full bg-brand-400"
                                                                        aria-hidden="true"
                                                                    ></span>
                                                                }.into_any()
                                                            }}
                                                        </td>
                                                        <td class="p-1">
                                                            <Show when=move || !delivered>
                                                                {
                                                                    let label = label_item_name.clone();
                                                                    view! {
                                                                        <button
                                                                            class="btn-ghost"
                                                                            aria-label=move || format!("{} {}", t_string!(i18n, history_resend_button), label)
                                                                            disabled=move || is_resending.get()
                                                                            on:click=move |_| {
                                                                                set_is_resending.set(true);
                                                                                spawn_local(async move {
                                                                                    match resend_alert_event(event_id).await {
                                                                                        Ok(r) if r.delivered => {
                                                                                            if let Some(t) = toasts {
                                                                                                t.success(t_string!(i18n, history_resend_success_toast).to_string());
                                                                                            }
                                                                                            reset_and_refetch(version, before_id, rows, loading);
                                                                                        }
                                                                                        Ok(r) => {
                                                                                            if let Some(t) = toasts {
                                                                                                t.error(r.error.unwrap_or_else(|| {
                                                                                                    t_string!(i18n, history_resend_failed_toast).to_string()
                                                                                                }));
                                                                                            }
                                                                                            set_is_resending.set(false);
                                                                                        }
                                                                                        Err(e) => {
                                                                                            if let Some(t) = toasts {
                                                                                                t.error(format!("{e}"));
                                                                                            }
                                                                                            set_is_resending.set(false);
                                                                                        }
                                                                                    }
                                                                                });
                                                                            }
                                                                        >
                                                                            <Show when=move || !is_resending.get() fallback=|| view! { <Loading /> }>
                                                                                <Icon icon=i::BsArrowRepeat />
                                                                                <span class="ml-1">{t!(i18n, history_resend_button)}</span>
                                                                            </Show>
                                                                        </button>
                                                                    }
                                                                }
                                                            </Show>
                                                        </td>
                                                    </tr>
                                                }
                                            }).collect_view()}
                                    </tbody>
                                </table>
                            </div>
                        }
                            .into_any()
                    }
                }
            }}
            <Show when=move || has_more(last_page_len.get(), PAGE_SIZE as usize)>
                <div class="flex justify-center">
                    <button class="btn-ghost" on:click=load_more>
                        {t!(i18n, history_load_more)}
                    </button>
                </div>
            </Show>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_more_true_for_a_full_page() {
        assert!(has_more(50, 50));
    }

    #[test]
    fn has_more_false_for_a_short_page() {
        assert!(!has_more(12, 50));
        assert!(!has_more(0, 50));
    }

    #[test]
    fn has_more_false_when_page_size_is_zero() {
        assert!(!has_more(0, 0));
    }
}
