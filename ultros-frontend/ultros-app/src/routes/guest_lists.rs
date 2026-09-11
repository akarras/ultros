//! Account-independent device lists. Browser storage is the source of truth.
use crate::global_state::labs::{LAB_LISTS_SYNC, use_lab};
use crate::i18n::*;
use leptos::prelude::*;

#[component]
pub fn DeviceLists() -> impl IntoView {
    let i18n = use_i18n();
    let ready = RwSignal::new(false);
    Effect::new(move |_| ready.set(true));
    move || {
        ready.track();
        #[cfg(feature = "hydrate")]
        if ready.get() {
            return view! { <browser::DeviceDirectory /> }.into_any();
        }
        view! { <p>{t!(i18n, guest_workspace_loading)}</p> }.into_any()
    }
}

#[component]
pub fn GuestListRoute() -> impl IntoView {
    let i18n = use_i18n();
    let enabled = use_lab(LAB_LISTS_SYNC);
    let ready = RwSignal::new(false);
    Effect::new(move |_| ready.set(true));
    move || {
        if !enabled.get() {
            return view! { <p>{t!(i18n, guest_workspace_enable)}</p> }.into_any();
        }
        ready.track();
        #[cfg(feature = "hydrate")]
        if ready.get() {
            return view! { <browser::DeviceEditorLoader /> }.into_any();
        }
        view! { <p>{t!(i18n, guest_workspace_opening)}</p> }.into_any()
    }
}

#[cfg(feature = "hydrate")]
mod browser {
    use super::*;
    use crate::list_doc::{
        adapter::{self, Edit},
        guest::GuestListHandle,
    };
    use crate::routes::list_view_sync::{ListBuildWorkspace, ListWorkspaceSource};
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
        let i18n = use_i18n();
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
            <section class="panel rounded-xl p-5 space-y-4" aria-label={move || t_string!(i18n, guest_workspace_directory_label).to_string()}>
                <h1 class="text-2xl font-bold">{t!(i18n, guest_workspace_heading)}</h1>
                <p>{t!(i18n, guest_workspace_intro)}</p>
                <div class="flex flex-wrap gap-2">
                    <input class="input grow" data-testid="device-list-name" aria-label={move || t_string!(i18n, guest_workspace_new_name).to_string()} placeholder={move || t_string!(i18n, guest_workspace_placeholder).to_string()} prop:value=move || name.get() on:input=move |ev| name.set(event_target_value(&ev)) maxlength="100" />
                    <button class="btn-primary" data-testid="device-list-create" disabled=move || busy.get() || name.get().trim().is_empty() on:click=create>{t!(i18n, guest_workspace_create)}</button>
                </div>
                <p role="alert" class="text-red-400">{move || error.get()}</p>
                <div class="grid gap-3 md:grid-cols-2">
                    {move || summaries.get().into_iter().map(|list| view! {
                        <a class="panel rounded-lg p-4 hover:border-blue-400" href=format!("/list/device/{}?labs=lists-sync", list.id)>
                            <strong>{if list.name.is_empty() { t_string!(i18n, guest_workspace_damaged).to_string() } else { list.name }}</strong><p class="text-sm opacity-70">{list.error.unwrap_or_else(|| t_string!(i18n, guest_workspace_saved).to_string())}</p>
                        </a>
                    }).collect_view()}
                </div>
                <crate::routes::guest_list_adoption::DeviceListsAdoption summaries />
                <details>
                    <summary class="cursor-pointer">{t!(i18n, guest_workspace_restore_heading)}</summary>
                    <p class="text-sm my-3">{t!(i18n, guest_workspace_storage_warning)}</p>
                    <label class="block" for="device-restore">{t!(i18n, guest_workspace_paste_backup)}</label>
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
                    }>{t!(i18n, guest_workspace_restore)}</button>
                </details>
            </section>
        }
    }

    #[component]
    pub fn DeviceEditorLoader() -> impl IntoView {
        let i18n = use_i18n();
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
            <a class="inline-block text-sm text-[color:var(--color-text-muted)] hover:underline mb-2" href="/list?labs=lists-sync">{t!(i18n, guest_workspace_back)}</a>
            <Show when=move || !error.get().is_empty()><p role="alert" class="text-red-400">{move || error.get()}</p></Show>
            {move || loaded.get().map(|handle| view! { <DeviceEditor handle /> })}
        }
    }

    #[component]
    fn DeviceEditor(handle: GuestListHandle) -> impl IntoView {
        let i18n = use_i18n();
        let handle = StoredValue::new_local(handle);
        let revision = handle.with_value(|h| h.revision);
        let status = handle.with_value(|h| h.status);
        let error = RwSignal::new(String::new());
        let backup = RwSignal::new(String::new());
        let selected = RwSignal::new(HashSet::<i32>::new());
        let offers = RwSignal::new(std::collections::HashMap::<
            i32,
            Vec<ultros_api_types::ActiveListing>,
        >::new());
        let shop = RwSignal::new(false);
        let (home, _) = crate::global_state::home_world::use_home_world();
        let scope = RwSignal::new(
            home.get_untracked()
                .map(|world| ultros_api_types::world_helper::AnySelector::World(world.id)),
        );
        let recipe_open = RwSignal::new(false);
        let confirm_delete = RwSignal::new(false);
        let deleting = RwSignal::new(false);
        let navigate = StoredValue::new_local(use_navigate());
        on_cleanup(move || handle.with_value(|h| h.close()));
        // Explains a shortcut that found nothing to do (#1430); any later
        // edit clears it. Errors take precedence in the feedback line.
        let notice = RwSignal::new(String::new());
        let apply = Callback::new(move |edit| {
            notice.set(String::new());
            if let Err(e) = handle.with_value(|h| h.apply(edit)) {
                error.set(e);
            }
        });
        let undo = Callback::new(move |()| {
            if handle.with_value(|h| h.undo()) {
                notice.set(String::new());
            } else {
                notice.set(t_string!(i18n, lists_workspace_nothing_to_undo).to_string());
            }
        });
        let redo = Callback::new(move |()| {
            if handle.with_value(|h| h.redo()) {
                notice.set(String::new());
            } else {
                notice.set(t_string!(i18n, lists_workspace_nothing_to_redo).to_string());
            }
        });
        // The same window shortcuts account lists get (#1429). This
        // component is created per loaded list and disposed with it, so the
        // listener follows the document: none is left behind for a previous
        // list, and a closed handle ignores the callbacks anyway. The delete
        // confirmation is the one panel here that owns the keyboard.
        crate::list_doc::undo::install(crate::list_doc::undo::UndoBindings {
            undo,
            redo,
            modal_open: confirm_delete.into(),
        });
        let source = ListWorkspaceSource {
            hide_acquired: Signal::derive(|| false),
            list_id: Signal::derive(|| 0),
            add: Callback::new(move |item| apply.run(Edit::Add(item))),
            add_many: Callback::new(move |items| apply.run(Edit::AddMany(items))),
            undo,
            redo,
            can_undo: Signal::derive(move || handle.with_value(|h| h.can_undo())),
            can_redo: Signal::derive(move || handle.with_value(|h| h.can_redo())),
            pending: Signal::derive(|| false),
            feedback: Signal::derive(move || {
                let error = error.get();
                if error.is_empty() {
                    notice.get()
                } else {
                    error
                }
            }),
            recipe_open: recipe_open.into(),
            toggle_recipe: Callback::new(move |()| recipe_open.update(|open| *open = !*open)),
            rows: Signal::derive(move || {
                revision.track();
                handle.with_value(|h| {
                    h.rows()
                        .iter()
                        .filter_map(|row| adapter::to_list_item(0, row))
                        .map(|item| {
                            let prices = offers.with(|offers| {
                                offers.get(&item.item_id).cloned().unwrap_or_default()
                            });
                            (item, prices)
                        })
                        .collect()
                })
            }),
            can_write: Signal::derive(|| true),
            edit: Callback::new(move |item| apply.run(Edit::Edit(item))),
            remove: Callback::new(move |id| apply.run(Edit::Remove(id))),
        };
        view! {
            <section class="space-y-3">
                <header class="flex flex-wrap items-center justify-between gap-x-4 gap-y-2">
                    <div class="min-w-0 flex-1 basis-56">
                        <input class="w-full min-w-0 bg-transparent text-2xl font-bold rounded-md border border-transparent hover:border-[color:var(--color-outline)] focus:border-[color:var(--color-outline)] px-1 py-0.5" aria-label={move || t_string!(i18n, guest_workspace_name).to_string()} prop:value=move || { revision.track(); handle.with_value(|h| h.meta().name) } data-committed=move || { revision.track(); handle.with_value(|h| h.meta().name) } maxlength="100" on:change=move |ev| { if let Err(e) = handle.with_value(|h| h.rename(&event_target_value(&ev))) { error.set(e); } } />
                        <p class="text-xs text-[color:var(--color-text-muted)] px-1" data-testid="device-list-status" role="status">{move || status.get()}</p>
                    </div>
                    <div class="flex flex-wrap gap-2">
                    <Show when=move || !selected.get().is_empty()>
                        <button class="btn-secondary" on:click=move |_| {
                            apply.run(Edit::RemoveMany(selected.get_untracked().into_iter().collect()));
                            selected.set(HashSet::new());
                        }>{t!(i18n, guest_workspace_remove_selected)}</button>
                    </Show>
                    <Show when=move || handle.with_value(|h| h.needs_save_retry())>
                    <button class="btn-secondary" on:click=move |_| {
                        let h = handle.get_value();
                        leptos::task::spawn_local(async move { if let Err(e) = h.flush().await { let _ = error.try_set(e); } });
                    }>{t!(i18n, guest_workspace_retry)}</button>
                    </Show>
                    </div>
                </header>
                <Show when=move || !error.get().is_empty()><p role="alert" class="text-red-400">{move || error.get()}</p></Show>
                <crate::routes::list_view_sync::ListWorkspaceModes shop=shop.into() set_shop=Callback::new(move |value| shop.set(value)) />
                <Show when=move || shop.get()><DeviceShop handle=handle.get_value() offers scope /></Show>
                <div class:hidden=move || shop.get()>
                <p class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, guest_workspace_build_prices)}</p>
                <ListBuildWorkspace source selected_items=selected />
                </div>
                <details class="panel rounded-lg p-3" data-testid="device-list-storage-details">
                    <summary class="cursor-pointer text-sm font-medium" data-testid="device-list-storage-toggle">{t!(i18n, guest_workspace_storage_heading)}</summary>
                    <div class="space-y-3 pt-3">
                        <crate::routes::guest_list_adoption::DeviceListAdoption handle=handle.get_value() />
                        <p class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, guest_workspace_backup_warning)}</p>
                        <div class="flex flex-wrap gap-2">
                            <button class="btn-secondary" data-testid="device-list-export" on:click=move |_| {
                                let h = handle.get_value(); leptos::task::spawn_local(async move { match h.backup().await { Ok(text) => { let _ = backup.try_set(text); }, Err(e) => { let _ = error.try_set(e); } } });
                            }>{t!(i18n, guest_workspace_export)}</button>
                            <button class="btn-secondary" data-testid="device-list-delete" on:click=move |_| confirm_delete.set(true)>{t!(i18n, guest_workspace_delete)}</button>
                        </div>
                        <Show when=move || !backup.get().is_empty()>
                            <label class="block text-sm">{t!(i18n, guest_workspace_backup_help)}
                                <textarea class="input w-full h-32" readonly data-testid="device-list-backup" prop:value=move || backup.get() />
                            </label>
                        </Show>
                        <Show when=move || confirm_delete.get()>
                            <div class="rounded-lg border border-red-400/50 p-4 space-y-3" role="group" aria-label={move || t_string!(i18n, guest_workspace_delete_label).to_string()}>
                                <p>{t!(i18n, guest_workspace_delete_confirm)}</p>
                                <div class="flex flex-wrap gap-2">
                                    <button class="btn-secondary" disabled=move || deleting.get() on:click=move |_| confirm_delete.set(false)>{t!(i18n, guest_workspace_keep)}</button>
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
                                    }>{t!(i18n, guest_workspace_delete_permanent)}</button>
                                </div>
                            </div>
                        </Show>
                    </div>
                </details>
            </section>
        }
    }

    #[component]
    fn DeviceShop(
        handle: GuestListHandle,
        offers: RwSignal<std::collections::HashMap<i32, Vec<ultros_api_types::ActiveListing>>>,
        scope: RwSignal<Option<ultros_api_types::world_helper::AnySelector>>,
    ) -> impl IntoView {
        use crate::components::list_shop::{ListShop, ShopInput, ShopRow};
        let i18n = use_i18n();
        let handle = StoredValue::new_local(handle);
        let revision = handle.with_value(|h| h.revision);
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
                                .unwrap_or_else(|| {
                                    t_string!(i18n, guest_workspace_item, id = item.item_id)
                                        .to_string()
                                }),
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
                <p>{t!(i18n, guest_workspace_shop_intro)}</p>
                <div class="flex flex-wrap gap-2">
                    <crate::components::world_picker::WorldPicker current_world=scope.into() set_current_world=scope.into() />
                    <button class="btn-secondary" disabled=move || busy.get() || scope.get().is_none() on:click=move |_| {
                        busy.set(true); error.set(String::new());
                        let Some(scope) = scope.get_untracked().and_then(|scope| crate::global_state::use_world_helper().ok()?.lookup_selector(scope).map(|world| world.get_name().to_string())) else { busy.set(false); return; };
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
                    }>{t!(i18n, guest_workspace_prices)}</button>
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
