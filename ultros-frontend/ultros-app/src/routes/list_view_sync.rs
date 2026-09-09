//! `/list/:id` behind the `lists-sync` Labs toggle: the same page as
//! [`crate::routes::list_view`], reading and writing the browser's local
//! document (spec sections 3.1-3.3) instead of the REST list endpoints.
//!
//! Everything that renders is imported from `list_view.rs` rather than
//! copied, so the two pages cannot drift apart; what differs here is the
//! data source and the handle's lifecycle.

use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::global_state::xiv_data::tracked_data;

use crate::components::data_table::header_cells;
use crate::components::icon::Icon;
use crate::global_state::LocalWorldData;
use icondata as i;
use leptos::either::Either;
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use ultros_api_types::{
    ActiveListing,
    list::{ListCapabilities, ListItem, ListPermission, ListWithPermission},
    result::ApiError,
};

use crate::api::{get_list_activity, get_list_items_with_listings};
use crate::components::{
    add_recipe_to_current_list::AddRecipeToCurrentListModal,
    item_icon::*,
    list::{
        auto_mark_purchases::AutoMarkPurchases,
        buying_view::BuyingView,
        filter_row::{ListFilterRow, SortSpec, worlds_in_listings},
        list_item_row::ListItemRow,
        list_settings_drawer::ListSettingsDrawer,
        list_summary::*,
    },
    list_subscribe_drawer::ListSubscribeDrawer,
    loading::*,
    make_place_importer::*,
    meta::{MetaDescription, MetaRobotsNoIndex, MetaTitle},
    modal::Modal,
    realtime_status::RealtimeStatus,
    skeleton::TableSkeleton,
    tooltip::*,
};
use crate::error::AppError;
use crate::global_state::labs::{LAB_LISTS_SYNC, use_lab};
use crate::i18n::*;
use crate::list_doc::adapter::Edit;
use crate::list_doc::handle::ListDocHandle;
use crate::query_defaults::filter_query_signal;
use crate::routes::list_view::{
    ActivityFeed, IdList, ListView, ListViewResult, MenuState, NameList, filter_excluded,
    list_item_table_columns, list_item_table_skeleton_columns, remaining_quantity, sort_list_items,
};
use crate::ws::realtime::{RealtimeSubscription, use_realtime};
use ultros_api_types::websocket::{
    EventType as WEvent, FilterPredicate, ListEventData, ServerClient, SocketMessageType,
    is_list_market_update_relevant,
};
use xiv_gen::ItemId;

/// Prices for the rows, fetched once per list and again only when the market
/// subscription or an import says so. The document — not this cache —
/// supplies the rows themselves, so a local edit never refetches prices.
#[derive(Clone)]
struct ListingsCache {
    list_id: i32,
    version: u32,
    /// The value of the page's `revalidate_version` this entry was fetched
    /// at. A revalidation (Global Constraint 2) must reach the server: if
    /// the cache could satisfy it, an unshared or deleted list would keep
    /// rendering from prices fetched while the client still had access, and
    /// the 403/404 that `is_denial` acts on would never arrive.
    revalidate: u32,
    list: ListWithPermission,
    listings: HashMap<i32, Vec<ActiveListing>>,
    /// The item ids the fetch that filled this cache covered — every row the
    /// *server* had, including rows whose price lookup came back empty. A
    /// row added locally is not in here until the socket has delivered the
    /// add and a later fetch sees it, so `load_view` treats "the document
    /// has an item this set doesn't" as a miss and refetches once. Caching
    /// the covered set (rather than the keys of `listings`) is what stops
    /// that from looping when the server genuinely has no price for the id.
    covered: HashSet<i32>,
}

/// The ids a fetch covered, built from the rows the server returned.
fn covered_ids(items: &[(ListItem, Vec<ActiveListing>)]) -> HashSet<i32> {
    items.iter().map(|(item, _)| item.item_id).collect()
}

/// How long a burst of relayed list broadcasts is allowed to coalesce into
/// a single revalidation. Every row edit on a shared list comes back to us
/// as a `ListItem` broadcast, and the Labs page already has those rows from
/// its document — the revalidation exists only so a *permission* change
/// (Global Constraint 2) is noticed by an idle page, so paying one REST
/// fetch per remote keystroke would be pure waste.
#[cfg(feature = "hydrate")]
const REVALIDATE_DEBOUNCE_MS: u32 = 1000;

/// The trailing-debounce timer. A real `Timeout` on the client; a unit
/// placeholder on the SSR half, where `Effect`s never run and so no
/// subscription is ever created to schedule one.
#[cfg(feature = "hydrate")]
type RevalidateTimer = gloo_timers::callback::Timeout;
#[cfg(not(feature = "hydrate"))]
type RevalidateTimer = ();

/// Ask for a revalidation `REVALIDATE_DEBOUNCE_MS` from now, replacing any
/// request already pending. Dropping the previous `Timeout` cancels it, so
/// N broadcasts inside the window cost exactly one fetch, fired after the
/// last of them.
#[cfg(feature = "hydrate")]
fn schedule_revalidate(slot: &Rc<RefCell<Option<RevalidateTimer>>>, bump: WriteSignal<u32>) {
    let timer = gloo_timers::callback::Timeout::new(REVALIDATE_DEBOUNCE_MS, move || {
        bump.update(|v| *v += 1);
    });
    *slot.borrow_mut() = Some(timer);
}

#[cfg(not(feature = "hydrate"))]
fn schedule_revalidate(_slot: &Rc<RefCell<Option<RevalidateTimer>>>, _bump: WriteSignal<u32>) {}

/// A failure that means the browser must stop keeping a local copy of this
/// list (Global Constraint 2): the server says the list is gone, or that
/// this session may not read it. Everything else — a dropped connection, a
/// 5xx, a body that would not parse — is transient, and the cached listings
/// or the offline document carry the page through it.
///
/// [`AppError::BadList`] is included because it is the client-side stand-in
/// for "there is no such list" (`api::get_list_items_with_listings` returns
/// it for a list id of `0`).
///
/// [`ApiError::NotAuthenticated`] is deliberately **not** a denial: a lapsed
/// session says nothing about whether this user still owns the list, and
/// destroying the snapshot would lose edits the user made offline and never
/// got to sync. It is handled exactly like signing out — close the handle,
/// keep the local copy — so signing back in resumes where they left off.
fn is_denial(error: &AppError) -> bool {
    matches!(
        error,
        AppError::BadList | AppError::ApiError(ApiError::Forbidden | ApiError::NotFound)
    )
}

/// Every write goes through the document; there is no REST fallback. A
/// signed-in visitor has a handle within the first client tick, and an
/// anonymous one cannot write to a list at all.
fn apply_edit(handle: RwSignal<Option<ListDocHandle>>, edit: Edit) -> Result<(), AppError> {
    match handle.get_untracked() {
        // A closed handle is detached from its document's subscriptions, so
        // applying an edit through it would change nothing anyone can see.
        // Say so rather than reporting a success that never happened; the
        // page normally clears `handle` alongside every `close`, so this is
        // the last line of defence, not the usual path.
        Some(handle) if handle.is_closed_or_disposed() => {
            Err(AppError::ListDoc("document is closed".to_string()))
        }
        Some(handle) => handle
            .apply(edit)
            .map_err(|error| AppError::ListDoc(error.to_string())),
        None => Err(AppError::ListDoc("document is not open yet".to_string())),
    }
}

/// The page's rows. With no document open — the SSR render, the first client
/// paint, an anonymous visitor — this is exactly the legacy REST read. With
/// one open, the listings come from the (cached) endpoint and the rows from
/// the document, so an offline edit renders immediately and a server that is
/// merely unreachable still leaves a usable page.
async fn load_view(
    id: i32,
    handle: RwSignal<Option<ListDocHandle>>,
    cache: StoredValue<Option<ListingsCache>>,
    listings_version: u32,
    revalidate_version: u32,
) -> ListViewResult {
    let Some(doc_handle) = handle.get_untracked() else {
        // No document yet (SSR, the first client paint, an anonymous
        // visitor). Cache what the REST read already paid for, so the first
        // handle-backed run below is a cache hit rather than a second fetch
        // of the same prices.
        let result = get_list_items_with_listings(id).await;
        if let Ok((list, items)) = &result {
            cache.set_value(Some(ListingsCache {
                list_id: id,
                version: listings_version,
                revalidate: revalidate_version,
                list: list.clone(),
                listings: items
                    .iter()
                    .map(|(item, listings)| (item.item_id, listings.clone()))
                    .collect(),
                covered: covered_ids(items),
            }));
        }
        return result;
    };
    // A row added locally is in the document but in no cache entry, so it
    // would render priceless until an unrelated market event bumped
    // `listings_version`. Missing coverage is a miss.
    let wanted_ids: HashSet<i32> = doc_handle
        .rows()
        .into_iter()
        .map(|row| row.key.item_id)
        .collect();
    let cached = cache.get_value().filter(|c| {
        c.list_id == id
            && c.version == listings_version
            && c.revalidate == revalidate_version
            && wanted_ids.is_subset(&c.covered)
    });
    let base = match cached {
        Some(cached) => cached,
        None => match get_list_items_with_listings(id).await {
            Ok((list, items)) => {
                doc_handle.remember_permission(list.permission as i16);
                let covered = covered_ids(&items);
                let fresh = ListingsCache {
                    list_id: id,
                    version: listings_version,
                    revalidate: revalidate_version,
                    list,
                    listings: items
                        .into_iter()
                        .map(|(item, listings)| (item.item_id, listings))
                        .collect(),
                    // Whatever the server had this time. If it hasn't seen
                    // the new row yet, the id stays uncovered — but the
                    // cache is only consulted again when the id *set*
                    // changes, so this refetches once, not in a loop.
                    covered: covered.union(&wanted_ids).copied().collect(),
                };
                cache.set_value(Some(fresh.clone()));
                fresh
            }
            // Forbidden or deleted: the local copy must not outlive the
            // server's answer. Clearing the handle also drops the sync
            // subscription (the Effect that owns it reads this signal).
            Err(error) if is_denial(&error) => {
                doc_handle.purge();
                cache.set_value(None);
                handle.set(None);
                return Err(error);
            }
            Err(error) => match cache.get_value().filter(|c| c.list_id == id) {
                // Stale prices beat no page.
                Some(stale) => stale,
                None => match offline_list(id, doc_handle) {
                    Some(list) => ListingsCache {
                        list_id: id,
                        version: listings_version,
                        revalidate: revalidate_version,
                        list,
                        listings: HashMap::new(),
                        covered: wanted_ids.clone(),
                    },
                    None => return Err(error),
                },
            },
        },
    };
    // `ListDocument` clones share the underlying document, so this hands
    // `view_result` a reference without holding the stored value across it.
    let Some(doc) = doc_handle.with_doc(|doc| doc.clone()) else {
        // The page was torn down while this fetch was in flight; there is no
        // document left to render and nothing to render it into.
        return Err(AppError::ListDoc("document is closed".to_string()));
    };
    Ok(crate::list_doc::adapter::view_result(
        &base.list,
        &doc,
        &base.listings,
    ))
}

/// The list as far as the browser knows it with no server at all: the cached
/// permission and the document's own name and scope. `None` when the cached
/// permission says this user never had access, so an empty document can't
/// masquerade as a readable list.
fn offline_list(id: i32, handle: ListDocHandle) -> Option<ListWithPermission> {
    let meta = handle.meta();
    let permission = ListPermission::from(handle.permission.get_untracked());
    if permission == ListPermission::None {
        return None;
    }
    Some(ListWithPermission {
        list: ultros_api_types::list::List {
            id,
            owner: 0,
            name: meta.name,
            wdr_filter: meta.scope?,
        },
        permission,
        owner_name: None,
    })
}

#[component]
pub fn ListViewSync() -> impl IntoView {
    let i18n = use_i18n();
    let params = use_params_map();
    let list_id = Memo::new(move |_| {
        params
            .with(|p| p.get("id").as_ref().and_then(|id| id.parse::<i32>().ok()))
            .unwrap_or_default()
    });
    // ---- Local-first document (spec sections 3.1, 3.2) ----
    //
    // The handle keeps its document in `StoredValue::new_local`, which must
    // never be built on the SSR half (repo issue #1332), and its browser
    // storage is scoped to the signed-in user — so it can only open on the
    // client, once `get_login` has resolved. Until then, and forever for an
    // anonymous visitor, this stays `None` and the page is the same
    // read-only REST render the non-Labs page produces.
    let handle: RwSignal<Option<ListDocHandle>> = RwSignal::new(None);
    provide_context(handle);

    let add_item = Action::new(move |list_item: &ListItem| {
        let edit = Edit::Add(list_item.clone());
        async move { apply_edit(handle, edit) }
    });
    let delete_item = Action::new(move |list_item: &i32| {
        let edit = Edit::Remove(*list_item);
        async move { apply_edit(handle, edit) }
    });

    let edit_item = Action::new(move |item: &ListItem| {
        // `Edit::Edit` finds the row by the id it was rendered with, so the
        // item arrives exactly as `ListItemRow` hands it back — original id
        // included, even when the edit changes its quality.
        let edit = Edit::Edit(item.clone());
        async move { apply_edit(handle, edit) }
    });
    let delete_items = Action::new(move |items: &Vec<i32>| {
        let edit = Edit::RemoveMany(items.clone());
        async move { apply_edit(handle, edit) }
    });
    let edit_items_hq = Action::new(move |(items, hq): &(Vec<i32>, Option<bool>)| {
        let edit = Edit::SetQuality(items.clone(), *hq);
        async move { apply_edit(handle, edit) }
    });
    let edit_list_action = Action::new(move |list: &ultros_api_types::list::List| {
        let edit = Edit::Rename {
            name: list.name.clone(),
            scope: list.wdr_filter,
        };
        async move { apply_edit(handle, edit) }
    });

    let bulk_pending =
        Signal::derive(move || delete_items.pending().get() || edit_items_hq.pending().get());
    let bulk_error = Signal::derive(move || {
        delete_items
            .value()
            .get()
            .and_then(|result| result.err().map(|e| e.to_string()))
            .or_else(|| {
                edit_items_hq
                    .value()
                    .get()
                    .and_then(|result| result.err().map(|e| e.to_string()))
            })
    });

    // Listings come from the existing endpoint and are cached per list; the
    // document supplies rows, so a local edit never refetches prices.
    let (external_update_version, set_external_update_version) = signal(0);
    let (activity_update_version, set_activity_update_version) = signal(0);
    // Global Constraint 2: bumped (on a debounce) by ANY list broadcast for
    // this list, and part of the `list_view` resource's key, so an idle page
    // re-asks the server whether it may still read this list. An unshare or
    // a delete both reach a still-subscribed client as a list broadcast
    // (`ultros/src/web.rs`: `unshare_list_from_user` -> `record_list_activity`
    // + `broadcast_list_update`; `delete_list` -> `EventType::removed`), and
    // the refetch's 403/404 is what `is_denial` turns into a purge.
    let (revalidate_version, set_revalidate_version) = signal(0u32);
    let (listings_version, set_listings_version) = signal(0u32);
    let (last_update_at, set_last_update_at) =
        signal::<Option<chrono::DateTime<chrono::Utc>>>(None);
    let listings_cache: StoredValue<Option<ListingsCache>> = StoredValue::new(None);

    let list_view = Resource::new(
        move || {
            (
                list_id(),
                handle.get().map(|handle| handle.revision.get()),
                listings_version.get(),
                external_update_version.get(),
                revalidate_version.get(),
            )
        },
        move |(id, _, listings_v, _, revalidate_v)| {
            load_view(id, handle, listings_cache, listings_v, revalidate_v)
        },
    );
    let user_resource = Resource::new(|| {}, |_| async move { crate::api::get_login().await.ok() });
    let self_user_id = Signal::derive(move || user_resource.get().flatten().map(|u| u.id));

    let activity_view = Resource::new(
        move || (list_id(), activity_update_version.get()),
        move |(id, _)| get_list_activity(id),
    );

    let realtime = use_realtime();
    let socket_status = realtime.as_ref().map(|client| client.status);
    // The document reports its own sync state once it is open; before that
    // (and for a reader with no document) the socket's own state is the
    // honest answer, rather than a status stuck on "connecting" forever.
    let realtime_status = Signal::derive(move || match handle.get() {
        Some(handle) => handle.status.get(),
        None => socket_status
            .map(|status| status.get())
            .unwrap_or_else(|| "offline".to_string()),
    });
    let activity_subscription = StoredValue::new(None::<RealtimeSubscription>);
    let list_market_subscription = StoredValue::new(None::<RealtimeSubscription>);

    // The legacy list subscription only drives the activity feed now; rows
    // arrive on the document's own subscription instead.
    let realtime_for_activity = realtime.clone();
    Effect::new(move |_| {
        activity_subscription.update_value(|sub| *sub = None);
        let id = list_id.get();
        let Some(realtime) = realtime_for_activity.clone() else {
            return;
        };
        if id != 0 {
            // Created inside the Effect body so the Effect's own closure
            // stays `Send + Sync` (it captures no `Rc`); the handler it is
            // moved into has no such bound.
            let revalidate_timer: Rc<RefCell<Option<RevalidateTimer>>> =
                Rc::new(RefCell::new(None));
            let sub = realtime.subscribe_list(id, move |message| {
                let ServerClient::ListUpdate(event) = message else {
                    return;
                };
                if matches!(event, WEvent::Added(ListEventData::Activity(_))) {
                    set_activity_update_version.update(|v| *v += 1);
                }
                // Any broadcast for this list — activity, the list row
                // itself, a row event — is a reason to re-ask the server
                // whether we may still read it. The page cannot tell a
                // revocation from a rename by the payload (an unshare
                // broadcasts an ordinary `List` update), so it revalidates
                // on all of them and lets the REST answer decide.
                schedule_revalidate(&revalidate_timer, set_revalidate_version);
            });
            activity_subscription.set_value(Some(sub));
        }
    });
    let realtime_for_market = realtime.clone();
    Effect::new(move |_| {
        list_market_subscription.update_value(|sub| *sub = None);
        let Some(Ok((list, items))) = list_view.get() else {
            return;
        };
        let item_ids = items
            .iter()
            .map(|(item, _)| item.item_id)
            .collect::<Vec<_>>();
        if item_ids.is_empty() {
            return;
        }
        let Some(realtime) = realtime_for_market.clone() else {
            return;
        };
        let filter = FilterPredicate::World(list.list.wdr_filter)
            .and(FilterPredicate::Items(item_ids.clone()));
        let sub = realtime.subscribe_market(filter, SocketMessageType::Listings, move |message| {
            if is_list_market_update_relevant(&message, &item_ids) {
                set_last_update_at.set(Some(chrono::Utc::now()));
                set_listings_version.update(|v| *v += 1);
            }
        });
        list_market_subscription.set_value(Some(sub));
    });
    on_cleanup(move || {
        activity_subscription.update_value(|sub| *sub = None);
        list_market_subscription.update_value(|sub| *sub = None);
        if let Some(handle) = handle.get_untracked() {
            handle.close();
        }
    });

    let (menu, set_menu) = signal(MenuState::None);
    let (item_modal_open, set_item_modal_open) = signal(false);
    let (recipe_modal_open, set_recipe_modal_open) = signal(false);
    let (subscribe_open, set_subscribe_open) = signal(false);
    let (settings_open, set_settings_open) = signal(false);
    let (rename_open, set_rename_open) = signal(false);
    let (rename_value, set_rename_value) = signal(String::new());
    let (confirm_bulk_delete, set_confirm_bulk_delete) = signal(false);

    // ---- The document's lifecycle (spec sections 3.1, 3.3, 5) ----
    //
    // Both Effects below are hydrate-only: `ListDocHandle::open` builds
    // `StoredValue::new_local`s that must never exist on the SSR half
    // (#1332), and `SyncSubscription` owns `Rc`s, so it can only live in a
    // thread-local slot.
    let modal_open = Signal::derive(move || {
        item_modal_open()
            || recipe_modal_open()
            || subscribe_open()
            || settings_open()
            || confirm_bulk_delete()
    });
    let (resync, set_resync) = signal(0u32);
    #[cfg(feature = "hydrate")]
    {
        let sync_subscription: StoredValue<
            Option<crate::list_doc::sync::SyncSubscription>,
            LocalStorage,
        > = StoredValue::new_local(None);
        // The (user, list) pair the open handle belongs to, so a re-run that
        // changed neither doesn't throw the document away.
        let open_for: StoredValue<Option<(i64, i32)>> = StoredValue::new(None);
        // The page's owner. An Effect runs its body under a short-lived
        // child owner that is disposed on every re-run *and* on unmount —
        // before the page's own `on_cleanup`. `ListDocHandle::open` builds
        // `StoredValue`s and `RwSignal`s, so opening it inside the Effect
        // body would hand the handle nodes that are already gone by the time
        // anything closes it: reading them panics, and on wasm a panic is an
        // `unreachable` that kills the module. Opening under this owner
        // instead keeps the handle alive exactly as long as the page.
        let page_owner = Owner::current();
        // What the page *should* have open, diffed. The Effect below
        // registers a cleanup that closes the handle it opened, so it must
        // only re-run when the answer genuinely changed — a `Resource` that
        // notifies twice with the same login (hydration, a refetch) would
        // otherwise close a document the page is still using. `None` means
        // login hasn't resolved; `Some(None)` means signed out or no list.
        let doc_target = Memo::new(move |_| {
            let id = list_id.get();
            let login = user_resource.get()?;
            Some(match login {
                Some(user) if id != 0 => Some((user.id as i64, id)),
                _ => None,
            })
        });

        Effect::new(move |_| {
            let Some(target) = doc_target.get() else {
                // Login hasn't resolved yet: nothing to open or close.
                return;
            };
            match target {
                Some(wanted) => {
                    if open_for.get_value() == Some(wanted) {
                        return;
                    }
                    // An account switch (or a move to another list): flush and
                    // detach the old document before its successor claims
                    // the signal, then release its reactive nodes. The page
                    // owner outlives every list it shows, so without the
                    // `dispose` each superseded document, undo stack and
                    // signal set would sit in the arena until navigation
                    // away from the page. Every accessor on the handle is
                    // `try_*`-based, so the copies still held by a queued
                    // timer or socket callback keep behaving as closed.
                    if let Some(previous) = handle.get_untracked() {
                        previous.close();
                        handle.set(None);
                        previous.dispose();
                    }
                    let opened = match page_owner.clone() {
                        Some(owner) => owner.with(|| ListDocHandle::open(wanted.0, wanted.1)),
                        None => ListDocHandle::open(wanted.0, wanted.1),
                    };
                    open_for.set_value(Some(wanted));
                    handle.set(Some(opened));
                    // Flushed and detached here rather than only in the
                    // page's `on_cleanup`: an owner runs its own cleanups
                    // before it disposes anything, so this fires while the
                    // handle (owned by the page) is still readable, on every
                    // re-run and on unmount. The page-level cleanup calls
                    // `close` again; it is idempotent.
                    on_cleanup(move || opened.close());
                    // Re-installed alongside each handle so the keys always
                    // reach the live document. `install` takes the handle by
                    // value, and one window listener per open is cheap:
                    // switching accounts without a page load isn't reachable
                    // (signing in navigates away and back), so in practice
                    // this runs exactly once per page. Left under the
                    // Effect's own owner on purpose: it creates no node the
                    // handle needs, and its listener is then removed when
                    // this run is superseded, instead of piling up.
                    crate::list_doc::undo::install(opened, modal_open);
                }
                _ => {
                    // Signed out, or no list id. Drop the document; the page
                    // falls back to the read-only REST render. `handle` is
                    // cleared unconditionally so no `close`d handle is ever
                    // left reachable through it. The nodes are NOT disposed:
                    // nothing replaces this document, and the page's own
                    // `on_cleanup` still closes whatever it finds here.
                    if let Some(previous) = handle.get_untracked() {
                        previous.close();
                    }
                    handle.set(None);
                    open_for.set_value(None);
                }
            }
        });

        let realtime_for_doc = realtime.clone();
        Effect::new(move |_| {
            resync.track();
            let open = handle.get();
            // Dropping the old subscription unsubscribes and disposes the
            // outbox drain. A denial that cleared `handle` lands here too,
            // which is what stops the socket traffic for a purged document.
            sync_subscription.set_value(None);
            let Some(open) = open else {
                return;
            };
            let Some(realtime) = realtime_for_doc.clone() else {
                open.set_status("offline");
                return;
            };
            let subscription = crate::list_doc::sync::start(
                open,
                realtime,
                move || set_resync.update(|n| *n += 1),
                move || set_last_update_at.set(Some(chrono::Utc::now())),
                move |kind| {
                    // Global Constraint 2: the server says this list is gone
                    // or not ours, so the local copy must not survive it.
                    // Clearing the handle re-runs this Effect, which drops
                    // the subscription. Safe from inside the callback: sync
                    // defers every callback past the socket's dispatch.
                    //
                    // `NotSignedIn` is the exception: a lapsed session says
                    // nothing about whether the user still has this list, so
                    // the snapshot is kept and only the sync stops — signing
                    // back in resumes where they left off. `classify_error`
                    // makes that distinction from the server's own wording,
                    // rather than the page guessing it from a login resource
                    // that never refetches (and so still reads "signed in"
                    // for exactly the cookie that just expired).
                    use crate::list_doc::sync::ErrorKind;
                    let purge = matches!(kind, ErrorKind::Denied | ErrorKind::NotFound);
                    if let Some(denied) = handle.get_untracked() {
                        if purge {
                            denied.purge();
                        } else {
                            denied.close();
                        }
                    }
                    handle.set(None);
                },
            );
            sync_subscription.set_value(Some(subscription));
        });

        on_cleanup(move || sync_subscription.set_value(None));
    }
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = (modal_open, resync, set_resync);
    }

    let edit_list_mode = RwSignal::new(false);
    let selected_items = RwSignal::new(HashSet::new());

    // Shopping-view state lives in the URL so a shared link reproduces the
    // sender's exact view. Query params resolve on the server too, so SSR
    // renders the same view a hydrated client shows.
    let (buying_view_param, set_buying_view_param) = filter_query_signal::<bool>("buy");
    let buying_view = Memo::new(move |_| buying_view_param.get().unwrap_or(false));

    let (excluded_worlds_param, set_excluded_worlds_param) =
        filter_query_signal::<IdList>("excluded-worlds");
    let excluded_worlds = Memo::new(move |_| {
        excluded_worlds_param
            .get()
            .map(|list| list.0.into_iter().collect::<HashSet<i32>>())
            .unwrap_or_default()
    });
    let set_excluded_worlds = Callback::new(move |set: HashSet<i32>| {
        set_excluded_worlds_param.set((!set.is_empty()).then(|| IdList::from_set(set)));
    });

    let (excluded_datacenters_param, set_excluded_datacenters_param) =
        filter_query_signal::<NameList>("excluded-datacenters");
    let excluded_datacenters = Memo::new(move |_| {
        excluded_datacenters_param
            .get()
            .map(|list| list.0.into_iter().collect::<HashSet<String>>())
            .unwrap_or_default()
    });
    let set_excluded_datacenters = Callback::new(move |set: HashSet<String>| {
        set_excluded_datacenters_param.set((!set.is_empty()).then(|| NameList::from_set(set)));
    });

    let (hide_acquired_param, set_hide_acquired_param) =
        filter_query_signal::<bool>("hide-acquired");
    let hide_acquired = Memo::new(move |_| hide_acquired_param.get().unwrap_or(false));

    let (sort_spec, set_sort_spec) = filter_query_signal::<SortSpec>("sort");

    let game_items = &tracked_data().items;

    type RowSnapshot = std::collections::HashMap<i32, (Option<i32>, Option<i32>)>;
    let recently_changed: RwSignal<HashSet<i32>> = RwSignal::new(HashSet::new());
    let prev_snapshot: StoredValue<RowSnapshot> = StoredValue::new(RowSnapshot::new());

    Effect::new(move |_| {
        let Some(Ok((_list, items))) = list_view.get() else {
            return;
        };
        let new_snapshot: RowSnapshot = items
            .iter()
            .map(|(i, _)| (i.id, (i.quantity, i.acquired)))
            .collect();
        let mut newly_changed: HashSet<i32> = HashSet::new();
        let prev = prev_snapshot.get_value();
        for (id, current) in &new_snapshot {
            if let Some(prior) = prev.get(id)
                && prior != current
            {
                newly_changed.insert(*id);
            }
        }
        prev_snapshot.set_value(new_snapshot);

        if !newly_changed.is_empty() {
            recently_changed.update(|set| set.extend(newly_changed.iter().copied()));
            #[cfg(not(feature = "ssr"))]
            {
                use gloo_timers::callback::Timeout;
                let ids: Vec<i32> = newly_changed.into_iter().collect();
                Timeout::new(1500, move || {
                    recently_changed.update(|set| {
                        for id in &ids {
                            set.remove(id);
                        }
                    });
                })
                .forget();
            }
        }
    });

    let view_caps = RwSignal::new(ListCapabilities::default());
    Effect::new(move |_| {
        let next = match list_view.get() {
            Some(Ok((list_with_perm, _))) => ListCapabilities::from(list_with_perm.permission),
            _ => ListCapabilities::default(),
        };
        view_caps.set(next);
    });

    let drawer_refresh = Signal::derive(move || {
        last_update_at
            .get()
            .map(|t| t.timestamp_millis() as u32)
            .unwrap_or(0)
    });

    // Auto-mark logic moved to AutoMarkPurchases component

    let list_name_for_meta = Signal::derive(move || {
        list_view
            .get()
            .and_then(|r| r.ok().map(|(l, _)| l.list.name))
            .unwrap_or_default()
    });
    let meta_title = move || {
        let name = list_name_for_meta.get();
        if name.is_empty() {
            t_string!(i18n, list_view_default_meta_title).to_string()
        } else {
            t_string!(i18n, list_view_meta_title)
                .to_string()
                .replace("%name%", &name)
        }
    };

    view! {
        <MetaTitle title=meta_title />
        <MetaDescription text=move || t_string!(i18n, list_view_meta_desc).to_string() />
        <MetaRobotsNoIndex />
        <div class="flex flex-col gap-4" data-testid="list-view-sync">
            <div class="sticky-bar rounded-lg px-3 py-3">
                // `list-toolbar` no longer carries any CSS (the compact
                // button-sizing rule it used to scope moved to the shared
                // `.sticky-bar-button` class on each button below and was
                // deleted from tailwind.css) — kept purely as a stable
                // `querySelector(".list-toolbar")` hook for
                // `integration/list-flow.cjs`, `screenshots.cjs` and
                // `shared-list.cjs`, which locate this row by that class.
                <div class="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between list-toolbar">
                    <div class="flex flex-wrap items-center gap-2">
                        <Show when=move || view_caps.with(|c| c.can_write)>
                            <>
                                <Tooltip tooltip_text=t_string!(i18n, list_view_tooltip_add_item).to_string()>
                                    <button
                                        class="sticky-bar-button sticky-bar-button-shrink"
                                        class:bg-brand-900=move || item_modal_open.get()
                                        class:border-brand-500=move || item_modal_open.get()
                                        on:click=move |_| set_item_modal_open(true)
                                    >
                                        <Icon icon=i::BiPlusRegular />
                                        <span class="sticky-bar-button-label">{t!(i18n, list_view_add_item)}</span>
                                    </button>
                                </Tooltip>
                                <Tooltip tooltip_text=t_string!(i18n, list_view_tooltip_add_recipe).to_string()>
                                    <button
                                        class="sticky-bar-button sticky-bar-button-shrink"
                                        class:bg-brand-900=move || recipe_modal_open.get()
                                        class:border-brand-500=move || recipe_modal_open.get()
                                        on:click=move |_| set_recipe_modal_open(true)
                                    >
                                        <Icon icon=i::BiBookAddRegular />
                                        <span class="sticky-bar-button-label">{t!(i18n, list_view_add_recipe)}</span>
                                    </button>
                                </Tooltip>
                                <Tooltip tooltip_text=t_string!(i18n, list_view_tooltip_import_item).to_string()>
                                    <button
                                        class="sticky-bar-button sticky-bar-button-shrink"
                                        class:bg-brand-900=move || menu() == MenuState::MakePlace
                                        class:border-brand-500=move || menu() == MenuState::MakePlace
                                        on:click=move |_| set_menu(
                                            match menu() {
                                                MenuState::MakePlace => MenuState::None,
                                                _ => MenuState::MakePlace,
                                            },
                                        )
                                    >
                                        <Icon icon=i::BiImportRegular />
                                        <span class="sticky-bar-button-label">{t!(i18n, list_view_make_place)}</span>
                                    </button>
                                </Tooltip>
                            </>
                        </Show>
                    </div>

                    <div class="flex flex-wrap gap-2 self-start lg:self-auto">
                        <Show when=move || view_caps.with(|c| c.can_write)>
                            <Tooltip tooltip_text=t_string!(i18n, list_auto_mark_description).to_string()>
                                <AutoMarkPurchases
                                    list_view=list_view
                                    on_purchase=Callback::new(move |(item_id, hq): (i32, bool)| {
                                        if let Some(handle) = handle.get_untracked()
                                            && let Err(error) = handle
                                                .apply(Edit::AddAcquired {
                                                    item_id,
                                                    hq: Some(hq),
                                                    delta: 1,
                                                })
                                        {
                                            log::warn!(
                                                "auto-mark failed for item {item_id}: {error}"
                                            );
                                        }
                                    })
                                />
                            </Tooltip>
                        </Show>
                        <Tooltip tooltip_text=t_string!(i18n, list_view_subscribe_tooltip).to_string()>
                            <button
                                class="sticky-bar-button sticky-bar-button-shrink"
                                aria-label=t_string!(i18n, list_view_subscribe_aria)
                                on:click=move |_| set_subscribe_open(true)
                            >
                                <Icon icon=i::BsBell />
                                <span class="sticky-bar-button-label">{t!(i18n, list_view_subscribe_button)}</span>
                            </button>
                        </Tooltip>
                        <Tooltip tooltip_text=t_string!(i18n, list_view_tooltip_purchasing_view).to_string()>
                            <button
                                class="sticky-bar-button sticky-bar-button-shrink"
                                class:bg-brand-900=buying_view
                                class:border-brand-500=buying_view
                                on:click=move |_| {
                                    let next = !buying_view.get_untracked();
                                    set_buying_view_param.set(next.then_some(true));
                                }
                            >
                                <Icon icon=i::BiCartRegular />
                                <span class="sticky-bar-button-label">{t!(i18n, list_view_purchasing_view)}</span>
                            </button>
                        </Tooltip>
                        <Tooltip tooltip_text=t_string!(i18n, list_view_settings_tooltip).to_string()>
                            <button
                                class="sticky-bar-button sticky-bar-button-shrink"
                                aria-label=t_string!(i18n, list_view_settings)
                                data-testid="list-settings-btn"
                                on:click=move |_| set_settings_open(true)
                            >
                                <Icon icon=i::BsGear />
                                <span class="sticky-bar-button-label">{t!(i18n, list_view_settings)}</span>
                            </button>
                        </Tooltip>
                    </div>
                </div>
            </div>

            <Show when=recipe_modal_open>
                <AddRecipeToCurrentListModal
                    list_id=list_id
                    set_visible=set_recipe_modal_open
                    on_success=move || {
                        set_external_update_version.update(|v| *v += 1);
                        set_activity_update_version.update(|v| *v += 1);
                        set_recipe_modal_open(false);
                    }
                />
            </Show>

            <Show when=subscribe_open>
                {move || {
                    let name = list_view
                        .get()
                        .and_then(|r| r.ok().map(|(l, _)| l.list.name))
                        .unwrap_or_else(|| format!("List {}", list_id()));
                    view! {
                        <ListSubscribeDrawer
                            list_id=list_id()
                            list_name=name
                            set_visible=set_subscribe_open.into()
                        />
                    }
                }}
            </Show>

            <Show when=item_modal_open>
                {move || {
                    let (search, set_search) = signal("".to_string());
                    // Lowercase the searchable item names once per modal open instead of
                    // once per item per keystroke. `tracked_data()` is read here, so a
                    // locale swap re-runs this block and rebuilds the index against the
                    // new names — the index is never keyed on stale English strings.
                    // Same shape as components/add_recipe_to_current_list.rs.
                    // `StoredValue` so `item_search` stays `Copy` — the view closure
                    // below captures it by move.
                    let search_index = StoredValue::new(
                        tracked_data()
                            .items
                            .iter()
                            .filter(|(_, i)| i.item_search_category > 0)
                            .map(|(id, i)| (id, i, i.name.to_lowercase()))
                            .collect::<Vec<_>>(),
                    );
                    let item_search = move || {
                        search
                            .with(|s| {
                                if s.is_empty() {
                                    return Vec::new();
                                }
                                let s_lower = s.to_lowercase();
                                let mut score = search_index.with_value(|index| {
                                    index
                                        .iter()
                                        .filter(|(_, _, lower)| lower.contains(&s_lower))
                                        .map(|(id, i, _)| (*id, *i))
                                        .collect::<Vec<_>>()
                                });
                                // ⚡ Bolt Optimization: Use select_nth_unstable_by_key to avoid O(N log N) full sort
                                // when we only need the top 100 results. This reduces time complexity to O(N).
                                if score.len() > 100 {
                                    score.select_nth_unstable_by_key(100, |(_, i)| (
                                        Reverse(i.level_item),
                                    ));
                                    score.truncate(100);
                                }
                                score
                                    .sort_unstable_by_key(|(_, i)| (
                                        Reverse(i.level_item),
                                    ));
                                score
                            })
                    };
                    let adding = add_item.pending();
                    let add_result = add_item.value();
                    view! {
                        <Modal set_visible=set_item_modal_open max_width="max-w-[90vw] w-[90vw] sm:w-[640px]">
                            <div class="flex flex-col gap-4 h-[70vh]">
                                <div class="flex flex-col gap-2 shrink-0">
                                    <h2 class="text-xl font-bold text-[color:var(--brand-fg)]">{t!(i18n, list_view_add_item_to_list)}</h2>
                                    <input
                                        class="input w-full"
                                        placeholder=t_string!(i18n, list_view_search_items).to_string()
                                        aria-label=t_string!(i18n, list_view_search_items).to_string()
                                        autofocus
                                        prop:value=search
                                        on:input=move |input| set_search(event_target_value(&input))
                                    />
                                    {move || add_result.get().map(|v| {
                                        let text = match v {
                                            Ok(()) => t_string!(i18n, list_view_added_to_list_success).to_string(),
                                            Err(e) => format!("{} {e}", t_string!(i18n, list_view_failed_to_add)),
                                        };
                                        view! { <div class="text-sm text-[color:var(--color-text-muted)]">{text}</div> }.into_view()
                                    })}
                                </div>
                                <div class="grid gap-2 flex-1 min-h-0 content-start overflow-y-auto pr-1">
                                    {move || {
                                        item_search()
                                            .into_iter()
                                            .map(move |(id, item)| {
                                                let (quantity, set_quantity) = signal(1);
                                                let read_input_quantity = move |input| {
                                                    if let Ok(quantity) = event_target_value(&input).parse() {
                                                        set_quantity(quantity)
                                                    }
                                                };
                                                view! {
                                                    <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] p-2 flex flex-col gap-3 sm:flex-row sm:items-center">
                                                        <div class="flex min-w-0 flex-1 items-center gap-3">
                                                            <ItemIcon item_id=id.0 icon_size=IconSize::Medium />
                                                            <span class="min-w-0 truncate font-semibold">{item.name.as_str()}</span>
                                                        </div>
                                                        <div class="flex items-center gap-2">
                                                            <label class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, list_view_qty)}</label>
                                                            <input
                                                                type="number"
                                                                min="1"
                                                                class="input w-20"
                                                                on:input=read_input_quantity
                                                                prop:value=quantity
                                                            />
                                                            <button
                                                                class="btn-primary"
                                                                disabled=adding
                                                                on:click=move |_| {
                                                                    let item = ListItem {
                                                                        item_id: id.0,
                                                                        list_id: params
                                                                            .with(|p| {
                                                                                p.get("id").as_ref().and_then(|id| id.parse::<i32>().ok())
                                                                            })
                                                                            .unwrap_or_default(),
                                                                        quantity: Some(quantity()),
                                                                        ..Default::default()
                                                                    };
                                                                    add_item.dispatch(item);
                                                                }
                                                            >
                                                                {move || if adding() {
                                                                    Either::Left(view! { <span>{t!(i18n, list_view_adding)}</span> })
                                                                } else {
                                                                    Either::Right(view! {
                                                                        <>
                                                                            <Icon icon=i::BiPlusRegular />
                                                                            <span>{t!(i18n, list_view_add)}</span>
                                                                        </>
                                                                    })
                                                                }}
                                                            </button>
                                                        </div>
                                                    </div>
                                                }
                                            })
                                            .collect::<Vec<_>>()
                                    }}

                                </div>
                            </div>
                        </Modal>
                    }
                }}
            </Show>

            {move || match menu() {
                MenuState::None => None,
                MenuState::MakePlace => {
                    Some(
                        view! {
                            <section class="panel rounded-lg p-4">
                                <MakePlaceImporter
                                    list_id=Signal::derive(move || {
                                        params
                                            .with(|p| {
                                                p.get("id").as_ref().map(|id| id.parse::<i32>().ok())
                                            })
                                            .flatten()
                                            .unwrap_or_default()
                                    })

                                    refresh=move || set_listings_version.update(|v| *v += 1)
                                />
                            </section>
                        },
                    )
                }
            }}

            <Transition fallback=move || {
                view! {
                    <section class="panel rounded-lg overflow-hidden">
                        <TableSkeleton
                            columns=list_item_table_skeleton_columns()
                            rows=6
                            row_class="px-1"
                        />
                    </section>
                }
            }>
                {move || {
                    list_view
                        .get()
                        .map(move |list| match list {
                            Ok((list, items)) => {
                                let items = StoredValue::new(items);
                                Either::Left(move || {
                                    let item_snapshot = items.get_value();
                                    let total_items = item_snapshot.len();
                                    let remaining_items = item_snapshot
                                        .iter()
                                        .filter(|(item, _)| {
                                            item.quantity.unwrap_or(1)
                                                > item.acquired.unwrap_or(0)
                                        })
                                        .count();
                                    let acquired_items = total_items.saturating_sub(remaining_items);
                                    let total_quantity: i32 = item_snapshot
                                        .iter()
                                        .map(|(i, _)| i.quantity.unwrap_or(1).max(1))
                                        .sum();
                                    let total_acquired: i32 = item_snapshot
                                        .iter()
                                        .map(|(i, _)| {
                                            let q = i.quantity.unwrap_or(1).max(1);
                                            i.acquired.unwrap_or(0).clamp(0, q)
                                        })
                                        .sum();
                                    let pct: i32 = if total_quantity > 0 {
                                        100 * total_acquired / total_quantity
                                    } else {
                                        0
                                    };
                                    let list_name = list.list.name.clone();
                                    let world_helper = use_context::<LocalWorldData>()
                                        .and_then(|world_data| world_data.0.ok());
                                    let filtered_item_snapshot = filter_excluded(
                                        &item_snapshot,
                                        &excluded_worlds.get(),
                                        &excluded_datacenters.get(),
                                        world_helper.as_deref(),
                                    );
                                    let filtered_items_for_buying = filtered_item_snapshot.clone();
                                    let mut filtered_items_for_rows = filtered_item_snapshot.clone();
                                    if hide_acquired.get() {
                                        filtered_items_for_rows.retain(|(item, _)| remaining_quantity(item) > 0);
                                    }
                                    if let Some(spec) = sort_spec.get() {
                                        sort_list_items(&mut filtered_items_for_rows, spec, |item_id| {
                                            game_items.get(&ItemId(item_id)).map(|item| item.name.as_str())
                                        });
                                    }
                                    let filtered_items_for_summary = filtered_item_snapshot.clone();

                                    // Built here, inside the Transition, so its SSR render
                                    // comes from the resolved resource — a read of
                                    // `list_view` outside a suspense boundary doesn't
                                    // register, and the shell/first-client-render disagree
                                    // when the resource resolves after the shell flushes
                                    // (an unrecoverable hydration mismatch).
                                    let datacenters = world_helper
                                        .as_deref()
                                        .and_then(|helper| {
                                            helper.lookup_selector(list.list.wdr_filter).map(
                                                |result| {
                                                    helper
                                                        .get_datacenters(&result)
                                                        .into_iter()
                                                        .map(|dc| dc.name.clone())
                                                        .collect::<Vec<_>>()
                                                },
                                            )
                                        })
                                        .unwrap_or_default();
                                    let filter_row = view! {
                                        <ListFilterRow
                                            worlds=worlds_in_listings(
                                                &item_snapshot,
                                                world_helper.as_deref(),
                                            )
                                            datacenters=datacenters
                                            excluded_worlds=excluded_worlds
                                            set_excluded_worlds=set_excluded_worlds
                                            excluded_datacenters=excluded_datacenters
                                            set_excluded_datacenters=set_excluded_datacenters
                                            sort_spec=Signal::derive(move || sort_spec.get())
                                            set_sort_spec=Callback::new(move |spec| {
                                                set_sort_spec.set(spec)
                                            })
                                            hide_acquired=hide_acquired
                                            set_hide_acquired=Callback::new(move |hide: bool| {
                                                set_hide_acquired_param.set(hide.then_some(true));
                                            })
                                        />
                                    };

                                    if buying_view() {
                                        Either::Left(
                                            view! {
                                                {filter_row}
                                                <section class="panel rounded-lg overflow-hidden">
                                                    <div class="border-b border-[color:var(--color-outline)] p-4 sm:p-5">
                                                        <div class="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
                                                            <div>
                                                                <p class="text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]">{t!(i18n, list_view_shopping_route)}</p>
                                                                <h1 class="text-xl sm:text-2xl font-bold text-[color:var(--brand-fg)]">{list_name.clone()}</h1>
                                                            </div>
                                                            <div class="flex flex-wrap gap-2 text-sm">
                                                                <RealtimeStatus
                                                                    status=realtime_status
                                                                    last_update=last_update_at
                                                                />
                                                                <span class="rounded-lg border border-[color:var(--color-outline)] px-3 py-1 text-[color:var(--color-text-muted)]">
                                                                    {t!(i18n, list_view_count_remaining, count = remaining_items)}
                                                                </span>
                                                            </div>
                                                        </div>
                                                    </div>
                                                    <div class="p-4 sm:p-5">
                                                        <BuyingView
                                                            items=filtered_items_for_buying
                                                            edit_item=edit_item
                                                            excluded_datacenters=excluded_datacenters
                                                        />
                                                    </div>
                                                </section>
                                            },
                                        )
                                    } else {
                                        Either::Right(
                                            view! {
                                                {filter_row}
                                                <section class="panel rounded-lg overflow-hidden">
                                                    <div class="border-b border-[color:var(--color-outline)] p-4 sm:p-5">
                                                        <div class="flex flex-col gap-4 xl:flex-row xl:items-end xl:justify-between">
                                                            <div>
                                                                <p class="text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]">{t!(i18n, list_view_list_label)}</p>
                                                                <div class="flex items-center gap-2">
                                                                    {
                                                                        let list_for_title = list.list.clone();
                                                                        let display_name = list_for_title.name.clone();
                                                                        move || {
                                                                            if rename_open() && view_caps.with(|c| c.can_admin) {
                                                                                let list_for_save = list_for_title.clone();
                                                                                Either::Left(view! {
                                                                                    <div class="flex flex-wrap items-center gap-2">
                                                                                        <input
                                                                                            class="input text-xl font-bold"
                                                                                            prop:value=rename_value
                                                                                            on:input=move |ev| set_rename_value(event_target_value(&ev))
                                                                                            data-testid="list-rename-input"
                                                                                        />
                                                                                        <button
                                                                                            class="btn-primary"
                                                                                            data-testid="list-rename-save"
                                                                                            on:click={
                                                                                                let list_for_save = list_for_save.clone();
                                                                                                move |_| {
                                                                                                    let mut new_list = list_for_save.clone();
                                                                                                    new_list.name = rename_value().trim().to_string();
                                                                                                    if !new_list.name.is_empty() {
                                                                                                        edit_list_action.dispatch(new_list);
                                                                                                        set_rename_open(false);
                                                                                                    }
                                                                                                }
                                                                                            }
                                                                                        >
                                                                                            <Icon icon=i::BiSaveSolid />
                                                                                            <span>{t!(i18n, list_view_settings_save)}</span>
                                                                                        </button>
                                                                                        <button
                                                                                            class="btn-secondary"
                                                                                            on:click=move |_| set_rename_open(false)
                                                                                        >
                                                                                            {t!(i18n, list_view_settings_cancel)}
                                                                                        </button>
                                                                                    </div>
                                                                                })
                                                                            } else {
                                                                                let display_name = display_name.clone();
                                                                                Either::Right(view! {
                                                                                    <>
                                                                                        <h1 class="text-xl sm:text-2xl font-bold text-[color:var(--brand-fg)]">{display_name.clone()}</h1>
                                                                                        <Show when=move || view_caps.with(|c| c.can_admin)>
                                                                                            <button
                                                                                                class="btn-ghost p-1"
                                                                                                aria-label=t_string!(i18n, edit_list).to_string()
                                                                                                data-testid="list-rename-btn"
                                                                                                on:click={
                                                                                                    let name = display_name.clone();
                                                                                                    move |_| {
                                                                                                        set_rename_value(name.clone());
                                                                                                        set_rename_open(true);
                                                                                                    }
                                                                                                }
                                                                                            >
                                                                                                <Icon icon=i::BsPencilFill />
                                                                                            </button>
                                                                                        </Show>
                                                                                    </>
                                                                                })
                                                                            }
                                                                        }
                                                                    }
                                                                </div>
                                                                <div class="mt-2">
                                                                    <RealtimeStatus
                                                                        status=realtime_status
                                                                        last_update=last_update_at
                                                                    />
                                                                </div>
                                                                <div class="mt-3 flex items-center gap-3 text-sm">
                                                                    {if total_quantity > 0 {
                                                                        Either::Left(view! {
                                                                            <div
                                                                                class="flex min-w-0 flex-1 flex-col gap-1"
                                                                                aria-label=t_string!(i18n, list_view_units_acquired_aria, acquired = total_acquired, quantity = total_quantity).to_string()
                                                                            >
                                                                                <span class="text-[color:var(--color-text-muted)]">
                                                                                    {t!(i18n, list_view_units_acquired_progress, acquired = total_acquired, quantity = total_quantity, pct = pct)}
                                                                                </span>
                                                                                <progress
                                                                                    class="progress progress-primary h-2 w-full rounded"
                                                                                    value=total_acquired
                                                                                    max=total_quantity
                                                                                ></progress>
                                                                            </div>
                                                                        })
                                                                    } else {
                                                                        Either::Right(view! {
                                                                            <span class="text-[color:var(--color-text-muted)]">
                                                                                {t!(i18n, list_view_no_items_yet)}
                                                                            </span>
                                                                        })
                                                                    }}
                                                                </div>
                                                            </div>
                                                            <div class="grid grid-cols-3 gap-2 text-center text-sm">
                                                                <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] px-3 py-2">
                                                                    <div class="text-lg font-bold">{total_items}</div>
                                                                    <div class="text-xs text-[color:var(--color-text-muted)]">{t!(i18n, item_explorer_items)}</div>
                                                                </div>
                                                                <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] px-3 py-2">
                                                                    <div class="text-lg font-bold">{remaining_items}</div>
                                                                    <div class="text-xs text-[color:var(--color-text-muted)]">{t!(i18n, list_view_remaining)}</div>
                                                                </div>
                                                                <Tooltip tooltip_text=Signal::derive(move || {
                                                                    t_string!(i18n, list_view_acquired_items_tooltip, count = acquired_items, total = total_items).to_string()
                                                                })>
                                                                    <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] px-3 py-2">
                                                                        <div class="text-lg font-bold">{acquired_items}</div>
                                                                        <div class="text-xs text-[color:var(--color-text-muted)]">{t!(i18n, list_view_acquired)}</div>
                                                                    </div>
                                                                </Tooltip>
                                                            </div>
                                                        </div>
                                                    </div>

                                                    <Show when=move || view_caps.with(|c| c.can_write)>
                                                        <div class="flex flex-col gap-3 border-b border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)]/60 p-3 lg:flex-row lg:items-center lg:justify-between">
                                                            <div class="flex flex-wrap items-center gap-2">
                                                                <button
                                                                    class="btn-secondary"
                                                                    class:bg-brand-950=edit_list_mode
                                                                    on:click=move |_| {
                                                                        edit_list_mode
                                                                            .update(|u| {
                                                                                *u = !*u;
                                                                            })
                                                                    }
                                                                >
                                                                    <Icon icon=i::BsPencilFill />
                                                                    <span>{t!(i18n, list_view_bulk_edit)}</span>
                                                                </button>
                                                                <div
                                                                    class="flex flex-wrap items-center gap-2"
                                                                    class:hidden=move || !edit_list_mode()
                                                                >
                                                                    <button
                                                                        class="btn-danger"
                                                                        disabled=bulk_pending
                                                                        on:click=move |_| {
                                                                            if !selected_items.with_untracked(|s| s.is_empty()) {
                                                                                set_confirm_bulk_delete(true);
                                                                            }
                                                                        }
                                                                    >
                                                                        <Icon icon=i::BiTrashSolid />
                                                                        <span>{t!(i18n, list_view_delete)}</span>
                                                                    </button>
                                                                    <button
                                                                        class="btn-secondary"
                                                                        disabled=bulk_pending
                                                                        on:click=move |_| {
                                                                            let items = selected_items
                                                                                .with_untracked(|s| {
                                                                                    s.iter().copied().collect::<Vec<_>>()
                                                                                });
                                                                            edit_items_hq.dispatch((items, Some(true)));
                                                                        }
                                                                    >
                                                                        <span>{t!(i18n, list_view_bulk_set_hq)}</span>
                                                                    </button>
                                                                    <button
                                                                        class="btn-secondary"
                                                                        disabled=bulk_pending
                                                                        on:click=move |_| {
                                                                            let items = selected_items
                                                                                .with_untracked(|s| {
                                                                                    s.iter().copied().collect::<Vec<_>>()
                                                                                });
                                                                            edit_items_hq.dispatch((items, None));
                                                                        }
                                                                    >
                                                                        <span>{t!(i18n, list_view_bulk_any_quality)}</span>
                                                                    </button>
                                                                    <Show when=move || bulk_pending.get()>
                                                                        <span class="flex items-center gap-2 text-sm text-[color:var(--color-text-muted)]">
                                                                            <Loading />
                                                                            {t!(i18n, list_view_bulk_pending)}
                                                                        </span>
                                                                    </Show>
                                                                    {move || {
                                                                        (!bulk_pending.get())
                                                                            .then(|| bulk_error.get())
                                                                            .flatten()
                                                                            .map(|error| {
                                                                                view! {
                                                                                    <span class="text-sm text-red-200">
                                                                                        {format!(
                                                                                            "{} {error}",
                                                                                            t_string!(i18n, list_view_bulk_failed),
                                                                                        )}
                                                                                    </span>
                                                                                }
                                                                            })
                                                                    }}
                                                                </div>
                                                            </div>
                                                            <div
                                                                class="flex flex-wrap items-center gap-2"
                                                                class:hidden=move || !edit_list_mode()
                                                            >
                                                                <button
                                                                    class="btn-secondary"
                                                                    on:click=move |_| {
                                                                        selected_items
                                                                            .update(|i| {
                                                                                for (item, _) in items.get_value() {
                                                                                    i.insert(item.id);
                                                                                }
                                                                            })
                                                                    }
                                                                >
                                                                    {t!(i18n, list_view_select_all)}
                                                                </button>
                                                                <button
                                                                    class="btn-secondary"
                                                                    on:click=move |_| {
                                                                        selected_items.update(|i| i.clear());
                                                                    }
                                                                >
                                                                    {t!(i18n, list_view_deselect_all)}
                                                                </button>
                                                            </div>
                                                        </div>
                                                        <Show when=confirm_bulk_delete>
                                                            <Modal set_visible=set_confirm_bulk_delete>
                                                                <div class="flex flex-col gap-4">
                                                                    <h2 class="text-xl font-bold text-[color:var(--brand-fg)]">
                                                                        {t!(i18n, list_view_bulk_delete_confirm_title)}
                                                                    </h2>
                                                                    <p class="text-sm text-[color:var(--color-text-muted)]">
                                                                        {move || t!(
                                                                            i18n,
                                                                            list_view_bulk_delete_confirm_body,
                                                                            count = selected_items.with(|s| s.len()),
                                                                        )}
                                                                    </p>
                                                                    <div class="flex justify-end gap-2">
                                                                        <button
                                                                            class="btn-secondary"
                                                                            on:click=move |_| set_confirm_bulk_delete(false)
                                                                        >
                                                                            {t!(i18n, cancel)}
                                                                        </button>
                                                                        <button
                                                                            class="btn-danger"
                                                                            on:click=move |_| {
                                                                                let items = selected_items
                                                                                    .with_untracked(|s| {
                                                                                        s.iter().copied().collect::<Vec<_>>()
                                                                                    });
                                                                                selected_items.update(|i| i.clear());
                                                                                delete_items.dispatch(items);
                                                                                set_confirm_bulk_delete(false);
                                                                            }
                                                                        >
                                                                            <Icon icon=i::BiTrashSolid />
                                                                            <span>{t!(i18n, list_view_delete)}</span>
                                                                        </button>
                                                                    </div>
                                                                </div>
                                                            </Modal>
                                                        </Show>
                                                    </Show>

                                                    <div class="overflow-x-auto">
                                                        <table class="w-full min-w-[760px] text-sm">
                                                            <thead>
                                                                <tr class="border-b border-[color:var(--color-outline)] bg-[color:var(--color-background)]/80 text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]">
                                                                    {header_cells(&list_item_table_columns(i18n, edit_list_mode))}
                                                                </tr>
                                                            </thead>
                                                            <tbody class="divide-y divide-[color:var(--color-outline)]">
                                                                <For
                                                                    each=move || filtered_items_for_rows.clone()
                                                                    key=|(item, _)| item.id
                                                                    children=move |(item, listings)| {
                                                                        view! {
                                                                            <ListItemRow
                                                                                item=item
                                                                                listings=listings
                                                                                edit_list_mode=edit_list_mode.into()
                                                                                selected_items=selected_items
                                                                                delete_item=delete_item
                                                                                edit_item=edit_item
                                                                                recently_changed=recently_changed
                                                                                can_write=Signal::derive(move || view_caps.with(|c| c.can_write))
                                                                                excluded_worlds=&[]
                                                                                excluded_datacenters=excluded_datacenters
                                                                            />
                                                                        }
                                                                    }
                                                                />

                                                            </tbody>
                                                        </table>
                                                    </div>
                                                    <div class="p-4 sm:p-5">
                                                        <ListSummary
                                                            items=filtered_items_for_summary.clone()
                                                            excluded_worlds=&[]
                                                            excluded_datacenters=excluded_datacenters
                                                        />
                                                    </div>
                                                    <div class="border-t border-[color:var(--color-outline)] p-4 sm:p-5">
                                                        <ActivityFeed activity=activity_view />
                                                    </div>
                                                </section>
                                            },
                                        )
                                    }
                                })
                            }
                            Err(e) => {
                                Either::Right(
                                    view! {
                                        <div class="panel rounded-lg p-4">{format!("{}\n{e}", t_string!(i18n, list_view_failed_to_get_items))}</div>
                                    },
                                )
                            }
                        })
                }}

            </Transition>

            <Show when=settings_open>
                {move || {
                    let Some(Ok((list_with_perm, _))) = list_view.get() else {
                        return view! { <div></div> }.into_any();
                    };
                    view! {
                        <ListSettingsDrawer
                            list=list_with_perm.list.clone()
                            permission=list_with_perm.permission
                            self_user_id=self_user_id
                            edit_list=edit_list_action
                            refresh_signal=drawer_refresh
                            set_visible=set_settings_open
                        />
                    }
                    .into_any()
                }}
            </Show>
        </div>
    }.into_any()
}

/// Picks the page for `/list/:id`. The `LABS` cookie is server-visible, so
/// the server and the hydrating client make the same choice. The id is
/// tracked so moving between lists builds a fresh page and document.
#[component]
pub fn ListRoute() -> impl IntoView {
    let sync = use_lab(LAB_LISTS_SYNC);
    let params = use_params_map();
    let id = Memo::new(move |_| params.with(|p| p.get("id").unwrap_or_default()));
    move || {
        id.track();
        if sync.get() {
            view! { <ListViewSync /> }.into_any()
        } else {
            view! { <ListView /> }.into_any()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Global Constraint 2: only "you may not have this list" purges the
    /// browser's copy.
    #[test]
    fn permission_and_missing_list_errors_are_denials() {
        for error in [
            AppError::BadList,
            AppError::ApiError(ApiError::Forbidden),
            AppError::ApiError(ApiError::NotFound),
        ] {
            assert!(is_denial(&error), "{error:?} must discard the local copy");
        }
    }

    /// A lapsed session is not a denial: the handle closes and the snapshot
    /// stays, so signing back in resumes the user's offline edits.
    #[test]
    fn a_lapsed_session_keeps_the_local_copy() {
        assert!(!is_denial(&AppError::ApiError(ApiError::NotAuthenticated)));
    }

    /// The complement: a transport failure, a 5xx flattened into a message,
    /// a body that would not parse, or an SSR timeout must leave the
    /// document alone so an offline edit survives the outage.
    #[test]
    fn transport_and_server_failures_are_not_denials() {
        for error in [
            AppError::ApiError(ApiError::Message("upstream exploded".to_string())),
            AppError::ApiError(ApiError::BadRequest("nope".to_string())),
            AppError::Json("expected value at line 1 column 1".to_string()),
            AppError::SystemError(crate::error::SystemError::Message(
                "connection reset".to_string(),
            )),
            AppError::InternalApiTimeout,
            AppError::ListDoc("document is not open yet".to_string()),
        ] {
            assert!(!is_denial(&error), "{error:?} is transient");
        }
    }

    /// With no document open, every write is refused rather than silently
    /// dropped — the page has no REST fallback to fall back to.
    #[test]
    fn edits_without_a_document_report_it() {
        let owner = Owner::new();
        owner.with(|| {
            let handle: RwSignal<Option<ListDocHandle>> = RwSignal::new(None);
            let error = apply_edit(handle, Edit::Remove(1)).unwrap_err();
            assert!(matches!(error, AppError::ListDoc(_)), "{error:?}");
        });
    }
}
