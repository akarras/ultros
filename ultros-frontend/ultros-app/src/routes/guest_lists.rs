//! Account-independent device lists. Browser storage is the source of truth.
use crate::global_state::labs::{LAB_LISTS_SYNC, use_lab};
use leptos::prelude::*;

#[component]
pub fn DeviceLists() -> impl IntoView {
    let ready = RwSignal::new(false);
    Effect::new(move |_| ready.set(true));
    move || {
        ready.track();
        #[cfg(feature = "hydrate")]
        if ready.get() {
            return view! { <browser::DeviceDirectory /> }.into_any();
        }
        view! { <p>"Loading lists saved on this device…"</p> }.into_any()
    }
}

#[component]
pub fn GuestListRoute() -> impl IntoView {
    let enabled = use_lab(LAB_LISTS_SYNC);
    let ready = RwSignal::new(false);
    Effect::new(move |_| ready.set(true));
    move || {
        if !enabled.get() {
            return view! { <p>"Enable Lists in Labs to open your device lists. Your saved lists stay on this device."</p> }.into_any();
        }
        ready.track();
        #[cfg(feature = "hydrate")]
        if ready.get() {
            return view! { <browser::DeviceEditorLoader /> }.into_any();
        }
        view! { <p>"Opening your device list…"</p> }.into_any()
    }
}

#[cfg(feature = "hydrate")]
mod browser {
    use super::*;
    use crate::list_doc::{
        adapter::{self, Edit},
        guest::GuestListHandle,
    };
    use crate::routes::list_view_sync::{BuildListRow, InlineListAdd};
    use leptos_router::hooks::{use_navigate, use_params_map};
    use std::collections::HashSet;

    fn prepare_offline() {
        if let (Some(window), Ok(event)) = (
            web_sys::window(),
            web_sys::Event::new("ultros:guest-list-opened"),
        ) {
            let _ = window.dispatch_event(&event);
        }
    }

    #[component]
    pub fn DeviceDirectory() -> impl IntoView {
        prepare_offline();
        let name = RwSignal::new(String::new());
        let error = RwSignal::new(String::new());
        let busy = RwSignal::new(false);
        let summaries = RwSignal::new(Vec::new());
        let backup = RwSignal::new(String::new());
        let navigate = use_navigate();
        Effect::new(move |_| {
            leptos::task::spawn_local(async move {
                match GuestListHandle::list().await {
                    Ok(lists) => {
                        let _ = summaries.try_set(lists);
                    }
                    Err(e) => {
                        let _ = error.try_set(e);
                    }
                }
            });
        });
        let go = StoredValue::new_local(navigate);
        let create = move |_| {
            if busy.get_untracked() || name.get_untracked().trim().is_empty() {
                return;
            }
            busy.set(true);
            error.set(String::new());
            let name = name.get_untracked();
            leptos::task::spawn_local(async move {
                match GuestListHandle::create(name.trim()).await {
                    Ok(handle) => {
                        let id = handle.id();
                        handle.close();
                        go.try_with_value(|go| {
                            go(
                                &format!("/list/device/{id}?labs=lists-sync"),
                                Default::default(),
                            )
                        });
                    }
                    Err(e) => {
                        let _ = error.try_set(e);
                    }
                }
                let _ = busy.try_set(false);
            });
        };
        view! {
            <section class="panel rounded-xl p-5 space-y-4" aria-label="Device lists">
                <h1 class="text-2xl font-bold">"Your next project starts here"</h1>
                <p>"Create a list now. No account needed. Your lists save on this device."</p>
                <div class="flex flex-wrap gap-2">
                    <input class="input grow" data-testid="device-list-name" aria-label="New device list name" placeholder="Raid supplies, new gear, a new home…" prop:value=move || name.get() on:input=move |ev| name.set(event_target_value(&ev)) maxlength="100" />
                    <button class="btn-primary" data-testid="device-list-create" disabled=move || busy.get() || name.get().trim().is_empty() on:click=create>"Create a device list"</button>
                </div>
                <p role="alert" class="text-red-400">{move || error.get()}</p>
                <div class="grid gap-3 md:grid-cols-2">
                    {move || summaries.get().into_iter().map(|list| view! {
                        <a class="panel rounded-lg p-4 hover:border-blue-400" href=format!("/list/device/{}?labs=lists-sync", list.id)>
                            <strong>{if list.name.is_empty() { "Damaged device list".to_string() } else { list.name }}</strong><p class="text-sm opacity-70">{list.error.unwrap_or_else(|| "Saved on this device".to_string())}</p>
                        </a>
                    }).collect_view()}
                </div>
                <crate::routes::guest_list_adoption::DeviceListsAdoption summaries />
                <details>
                    <summary class="cursor-pointer">"Storage and restore a backup"</summary>
                    <p class="text-sm my-3">"Clearing browser data removes device lists. Export a backup from a list, or sync it to an account to keep it elsewhere."</p>
                    <label class="block" for="device-restore">"Paste a list backup"</label>
                    <textarea id="device-restore" class="input w-full h-24" data-testid="device-list-backup" prop:value=move || backup.get() on:input=move |ev| backup.set(event_target_value(&ev)) />
                    <button class="btn-secondary" data-testid="device-list-restore" disabled=move || busy.get() || backup.get().trim().is_empty() on:click=move |_| {
                        busy.set(true);
                        let text = backup.get_untracked();
                        leptos::task::spawn_local(async move {
                            match GuestListHandle::restore(&text).await {
                                Ok(handle) => {
                                    let id = handle.id(); handle.close();
                                    go.try_with_value(|go| go(&format!("/list/device/{id}?labs=lists-sync"), Default::default()));
                                }
                                Err(e) => { let _ = error.try_set(e); },
                            }
                            let _ = busy.try_set(false);
                        });
                    }>"Restore as a new device list"</button>
                </details>
            </section>
        }
    }

    #[component]
    pub fn DeviceEditorLoader() -> impl IntoView {
        prepare_offline();
        let params = use_params_map();
        let loaded = RwSignal::new_local(None::<GuestListHandle>);
        let generation = RwSignal::new(0u64);
        let error = RwSignal::new(String::new());
        Effect::new(move |_| {
            let id = params.with(|p| p.get("device_id").unwrap_or_default());
            if let Some(old) = loaded.get_untracked() {
                old.close();
            }
            loaded.set(None);
            generation.update(|n| *n += 1);
            let request = generation.get_untracked();
            error.set(String::new());
            leptos::task::spawn_local(async move {
                match GuestListHandle::open(&id).await {
                    Ok(handle) => {
                        if generation.try_get_untracked() == Some(request) {
                            loaded.set(Some(handle));
                        } else {
                            handle.close();
                        }
                    }
                    Err(e) => {
                        if generation.try_get_untracked() == Some(request) {
                            error.set(e);
                        }
                    }
                }
            });
        });
        view! {
            <a class="inline-block text-sm text-[color:var(--color-text-muted)] hover:underline mb-2" href="/list?labs=lists-sync">"← All lists"</a>
            <Show when=move || !error.get().is_empty()><p role="alert" class="text-red-400">{move || error.get()}</p></Show>
            {move || loaded.get().map(|handle| view! { <DeviceEditor handle /> })}
        }
    }

    #[component]
    fn DeviceEditor(handle: GuestListHandle) -> impl IntoView {
        let handle = StoredValue::new_local(handle);
        let revision = handle.with_value(|h| h.revision);
        let status = handle.with_value(|h| h.status);
        let error = RwSignal::new(String::new());
        let backup = RwSignal::new(String::new());
        let selected = RwSignal::new(HashSet::<i32>::new());
        let shop = RwSignal::new(false);
        let filter = RwSignal::new(String::new());
        let confirm_delete = RwSignal::new(false);
        let deleting = RwSignal::new(false);
        let navigate = StoredValue::new_local(use_navigate());
        on_cleanup(move || handle.with_value(|h| h.close()));
        let apply = Callback::new(move |edit| {
            if let Err(e) = handle.with_value(|h| h.apply(edit)) {
                error.set(e);
            }
        });
        view! {
            <section class="space-y-3">
                <header class="flex flex-wrap items-center justify-between gap-x-4 gap-y-2">
                    <div class="min-w-0 flex-1 basis-56">
                        <input class="w-full min-w-0 bg-transparent text-2xl font-bold rounded-md border border-transparent hover:border-[color:var(--color-outline)] focus:border-[color:var(--color-outline)] px-1 py-0.5" aria-label="List name" prop:value=move || { revision.track(); handle.with_value(|h| h.meta().name) } maxlength="100" on:change=move |ev| { if let Err(e) = handle.with_value(|h| h.rename(&event_target_value(&ev))) { error.set(e); } } />
                        <p class="text-xs text-[color:var(--color-text-muted)] px-1" data-testid="device-list-status" role="status">{move || status.get()}</p>
                    </div>
                    <div class="flex flex-wrap gap-2">
                    <button class="btn-secondary" on:click=move |_| { handle.with_value(|h| { h.undo(); }); }>"Undo"</button>
                    <button class="btn-secondary" on:click=move |_| { handle.with_value(|h| { h.redo(); }); }>"Redo"</button>
                    <Show when=move || !selected.get().is_empty()>
                        <button class="btn-secondary" on:click=move |_| {
                            apply.run(Edit::RemoveMany(selected.get_untracked().into_iter().collect()));
                            selected.set(HashSet::new());
                        }>"Remove selected · Undo available"</button>
                    </Show>
                    <Show when=move || { let value = status.get(); value != "Saved on this device" && !value.starts_with("Saving on this device") }>
                    <button class="btn-secondary" on:click=move |_| {
                        let h = handle.get_value();
                        leptos::task::spawn_local(async move { if let Err(e) = h.flush().await { let _ = error.try_set(e); } });
                    }>"Retry save"</button>
                    </Show>
                    </div>
                </header>
                <Show when=move || !error.get().is_empty()><p role="alert" class="text-red-400">{move || error.get()}</p></Show>
                <div class="inline-flex w-fit gap-1 rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background)] p-1" role="group" aria-label="List mode">
                    <button class=move || if !shop.get() { "btn-primary min-w-20 justify-center font-semibold shadow-sm" } else { "btn-ghost min-w-20 justify-center text-[color:var(--color-text-muted)]" } aria-pressed=move || (!shop.get()).to_string() on:click=move |_| shop.set(false)>"Build"</button>
                    <button class=move || if shop.get() { "btn-primary min-w-20 justify-center font-semibold shadow-sm" } else { "btn-ghost min-w-20 justify-center text-[color:var(--color-text-muted)]" } data-testid="guest-shop-mode" aria-pressed=move || shop.get().to_string() on:click=move |_| shop.set(true)>"Shop"</button>
                </div>
                <Show when=move || shop.get()><DeviceShop handle=handle.get_value() /></Show>
                <div class:hidden=move || shop.get()>
                <InlineListAdd list_id=Signal::derive(|| 0) on_add=Callback::new(move |item| apply.run(Edit::Add(item))) />
                <details class="panel rounded-lg p-3">
                    <summary class="cursor-pointer">"Add a recipe"</summary>
                    <crate::routes::list_view_sync::InlineRecipeAdd list_id=Signal::derive(|| 0) on_add=Callback::new(move |items| apply.run(Edit::AddMany(items))) />
                </details>
                <input class="input w-full my-3" aria-label="Filter items already in this list" placeholder="Filter this list…" prop:value=move || filter.get() on:input=move |ev| filter.set(event_target_value(&ev)) />
                <div class="overflow-x-auto panel rounded-xl">
                    <table class="w-full"><thead><tr>
                        <th>"Select"</th><th>"Item"</th><th>"Quality"</th><th>"Needed"</th><th>"Owned"</th><th>"Target price"</th><th>"Actions"</th>
                    </tr></thead><tbody>
                        <For each=move || {
                            revision.track();
                            let query = filter.get().to_lowercase();
                            let data = crate::global_state::xiv_data::tracked_data();
                            handle.with_value(|h| h.rows().iter().filter_map(|r| adapter::to_list_item(0,r)).filter(|item| query.is_empty() || data.items.get(&xiv_gen::ItemId(item.item_id)).is_some_and(|i| i.name.to_lowercase().contains(&query)) || item.item_id.to_string().contains(&query)).collect::<Vec<_>>())
                        }
                            key=|item| (item.id, item.quantity, item.acquired, item.target_price)
                            children=move |item| view! { <BuildListRow item selected_items=selected on_edit=Callback::new(move |item| apply.run(Edit::Edit(item))) on_delete=Callback::new(move |id| apply.run(Edit::Remove(id))) can_write=Signal::derive(|| true) /> } />
                    </tbody></table>
                </div>
                </div>
                <details class="panel rounded-lg p-3" data-testid="device-list-storage-details">
                    <summary class="cursor-pointer text-sm font-medium" data-testid="device-list-storage-toggle">"Sync to an account, backups and storage"</summary>
                    <div class="space-y-3 pt-3">
                        <crate::routes::guest_list_adoption::DeviceListAdoption handle=handle.get_value() />
                        <p class="text-sm text-[color:var(--color-text-muted)]">"Clearing browser data removes device lists. Keep a backup or add this list to your account."</p>
                        <div class="flex flex-wrap gap-2">
                            <button class="btn-secondary" data-testid="device-list-export" on:click=move |_| {
                                let h = handle.get_value(); leptos::task::spawn_local(async move { match h.backup().await { Ok(text) => { let _ = backup.try_set(text); }, Err(e) => { let _ = error.try_set(e); } } });
                            }>"Export backup"</button>
                            <button class="btn-secondary" data-testid="device-list-delete" on:click=move |_| confirm_delete.set(true)>"Delete device list"</button>
                        </div>
                        <Show when=move || !backup.get().is_empty()>
                            <label class="block text-sm">"Copy and save this backup in a text file. Restore it from All lists."
                                <textarea class="input w-full h-32" readonly data-testid="device-list-backup" prop:value=move || backup.get() />
                            </label>
                        </Show>
                        <Show when=move || confirm_delete.get()>
                            <div class="rounded-lg border border-red-400/50 p-4 space-y-3" role="group" aria-label="Confirm device list deletion">
                                <p>"Delete this list from this device? This cannot be undone. Export a backup first if you want to keep it. Any copy already added to your account remains there."</p>
                                <div class="flex flex-wrap gap-2">
                                    <button class="btn-secondary" disabled=move || deleting.get() on:click=move |_| confirm_delete.set(false)>"Keep list"</button>
                                    <button class="btn-primary" data-testid="device-list-confirm-delete" disabled=move || deleting.get() on:click=move |_| {
                                        if deleting.get_untracked() { return; }
                                        deleting.set(true);
                                        let h = handle.get_value();
                                        leptos::task::spawn_local(async move {
                                            match h.remove().await {
                                                Ok(()) => { navigate.try_with_value(|go| go("/list?labs=lists-sync", Default::default())); }
                                                Err(e) => { let _ = error.try_set(e); }
                                            }
                                            let _ = deleting.try_set(false);
                                        });
                                    }>"Delete permanently"</button>
                                </div>
                            </div>
                        </Show>
                    </div>
                </details>
            </section>
        }
    }

    #[component]
    fn DeviceShop(handle: GuestListHandle) -> impl IntoView {
        use crate::components::list_shop::{ListShop, ShopInput, ShopRow};
        use std::collections::HashMap;
        let handle = StoredValue::new_local(handle);
        let revision = handle.with_value(|h| h.revision);
        let scope = RwSignal::new(String::new());
        let (home, _) = crate::global_state::home_world::use_home_world();
        let worlds = StoredValue::new(
            crate::global_state::use_world_helper()
                .ok()
                .map(|h| {
                    h.iter()
                        .filter_map(|w| w.as_world().cloned())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
        );
        let offers = RwSignal::new(HashMap::<i32, Vec<ultros_api_types::ActiveListing>>::new());
        let observed = RwSignal::new(None::<String>);
        let error = RwSignal::new(String::new());
        let busy = RwSignal::new(false);
        let input = Signal::derive(move || {
            revision.track();
            let data = crate::global_state::xiv_data::tracked_data();
            ShopInput {
                title: handle.with_value(|h| h.meta().name),
                rows: handle
                    .with_value(|h| h.rows())
                    .into_iter()
                    .filter_map(|row| {
                        let item = adapter::to_list_item(0, &row)?;
                        Some(ShopRow {
                            key: item.id.to_string(),
                            name: data
                                .items
                                .get(&xiv_gen::ItemId(item.item_id))
                                .map(|i| i.name.to_string())
                                .unwrap_or_else(|| format!("Item {}", item.item_id)),
                            item_id: item.item_id,
                            hq: item.hq,
                            needed: item.quantity.unwrap_or(1),
                            acquired: item.acquired.unwrap_or(0),
                            listings: offers
                                .with(|m| m.get(&item.item_id).cloned().unwrap_or_default()),
                        })
                    })
                    .collect(),
                observed_at: observed.get(),
                home_world: home.get().map(|w| w.id).unwrap_or(0),
                world_names: worlds
                    .with_value(|worlds| worlds.iter().map(|w| (w.id, w.name.clone())).collect()),
                datacenters: worlds
                    .with_value(|worlds| worlds.iter().map(|w| (w.id, w.datacenter_id)).collect()),
            }
        });
        view! {
            <div class="space-y-3">
                <p>"Choose a world or datacenter to look up current listings. Your list and purchase progress remain available offline."</p>
                <div class="flex flex-wrap gap-2">
                    <input class="input" aria-label="Shopping world or datacenter" placeholder="World or datacenter name" prop:value=move || scope.get() on:input=move |ev| scope.set(event_target_value(&ev)) />
                    <button class="btn-secondary" disabled=move || busy.get() || scope.get().trim().is_empty() on:click=move |_| {
                        busy.set(true); error.set(String::new());
                        let scope = scope.get_untracked();
                        let ids: Vec<_> = handle.with_value(|h| h.rows()).into_iter().map(|r| r.key.item_id).collect();
                        leptos::task::spawn_local(async move {
                            match crate::api::get_bulk_listings(scope.trim(), ids.into_iter()).await {
                                Ok(data) => {
                                    let _ = offers.try_set(data.into_iter().map(|(id, rows)| (id, rows.into_iter().map(|(listing, _)| listing).collect())).collect());
                                    // This endpoint carries seller review times, not ingest times.
                                    // Do not label this fetch time as a market observation.
                                    let _ = observed.try_set(None);
                                }
                                Err(e) => { let _ = error.try_set(e.to_string()); }
                            }
                            let _ = busy.try_set(false);
                        });
                    }>"Look up prices"</button>
                </div>
                <p role="alert">{move || error.get()}</p>
                <ListShop input on_purchase=Callback::new(move |(key, delta): (String, i32)| {
                    if let Ok(id) = key.parse::<i32>()
                        && let Some(key) = adapter::key_of(id)
                        && let Err(e) = handle.with_value(|h| h.apply(Edit::AddAcquired { item_id: key.item_id, hq: key.hq(), delta: i64::from(delta) })) {
                        error.set(e);
                    }
                }) on_undo=Callback::new(move |()| { handle.with_value(|h| { h.undo(); }); }) can_edit=Signal::derive(|| true) />
            </div>
        }
    }
}
