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
    // Read once per mount, like `ListRoute`: the flag only changes with the
    // route, and tracking it rebuilt this view inside a route the router was
    // already leaving.
    let enabled = use_lab(LAB_LISTS_SYNC).get_untracked();
    let ready = RwSignal::new(false);
    Effect::new(move |_| ready.set(true));
    move || {
        if !enabled {
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

/// Editor URL for a freshly created device list. `online` appends the
/// `make_online=1` flag that `DeviceListAdoption` resumes in the editor, so
/// the upload reuses the same consent and scope handling as "Make online".
#[cfg(any(feature = "hydrate", test))]
fn device_list_href(id: &str, online: bool) -> String {
    if online {
        format!("/list/device/{id}?labs=lists-sync&make_online=1")
    } else {
        format!("/list/device/{id}?labs=lists-sync")
    }
}

#[cfg(feature = "hydrate")]
mod browser {
    use super::*;
    use crate::components::app_link::use_query_map_or_default;
    use crate::components::cart::{ListCart, use_legacy_cart};
    use crate::components::list::filter_row::SortSpec;
    use crate::list_doc::{
        adapter::{self, Edit},
        guest::GuestListHandle,
    };
    use crate::routes::list_view_sync::{ListBuildWorkspace, ListWorkspaceSource};
    use leptos::either::Either;
    use leptos_router::hooks::{use_navigate, use_params_map};
    use std::collections::{BTreeSet, HashSet};
    use ultros_api_types::world_helper::AnySelector;
    use ultros_calc::list_estimate::{LookupTicket, MissingReason, PriceFeed};
    use ultros_calc::list_travel::TravelPolicy;

    /// The document cannot see uncommitted browser text. Use the same
    /// committed-value boundary as native text Undo, including composer drafts.
    fn editor_has_drafts() -> bool {
        use wasm_bindgen::JsCast;
        let Ok(inputs) = document().query_selector_all(
            "[data-testid='device-list-editor'] input[data-committed], [data-testid='device-list-editor'] [data-handoff-committed]",
        ) else { return true; };
        (0..inputs.length()).any(|index| {
            let Some(element) = inputs
                .item(index)
                .and_then(|node| node.dyn_into::<web_sys::Element>().ok())
            else {
                return false;
            };
            let value = element
                .dyn_ref::<web_sys::HtmlInputElement>()
                .map(|input| input.value())
                .or_else(|| {
                    element
                        .dyn_ref::<web_sys::HtmlSelectElement>()
                        .map(|select| select.value())
                });
            let committed = element
                .get_attribute("data-handoff-committed")
                .or_else(|| element.get_attribute("data-committed"));
            value
                .zip(committed)
                .is_some_and(|(value, committed)| value.trim() != committed.trim())
        })
    }

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
        use crate::api::{
            delete_list, edit_list, get_lists_with_permissions, get_login, leave_list,
            use_list_invite,
        };
        use crate::components::icon::Icon;
        use crate::components::meta::{MetaDescription, MetaRobotsNoIndex, MetaTitle};
        use crate::components::modal::Modal;
        use crate::routes::lists::ListCard;
        use icondata as i;
        use ultros_api_types::list::List;
        let i18n = use_i18n();
        prepare_offline();
        let name = RwSignal::new(String::new());
        let filter = RwSignal::new(String::new());
        let error = RwSignal::new(String::new());
        let busy = RwSignal::new(false);
        // Online is the default for signed-in users; reset each time the
        // modal opens so a previous Local choice does not stick.
        let storage_online = RwSignal::new(true);
        let summaries = RwSignal::new(Vec::<crate::list_doc::guest::GuestListSummary>::new());
        // A rename or delete from a local card re-reads the device directory
        // so the card re-keys on its new storage revision (or disappears).
        let reload_local = move || {
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
        };
        let local_loaded = RwSignal::new(false);
        let backup = RwSignal::new(String::new());
        let (creating, set_creating) = signal(false);
        let (restoring, set_restoring) = signal(false);
        let (joining, set_joining) = signal(false);
        let invite = RwSignal::new(String::new());
        let login = Resource::new(|| (), |_| get_login());
        let user_id = Signal::derive(move || login.get().and_then(Result::ok).map(|u| u.id));
        let delete_list = Action::new(|id: &i32| delete_list(*id));
        let edit_list = Action::new(|list: &List| edit_list(list.clone()));
        let leave_list_action = Action::new(|(id, user): &(i32, u64)| leave_list(*id, *user));
        let redeem = Action::new(|code: &String| use_list_invite(code.clone()));
        Effect::new(move |_| {
            if let Some(result) = redeem.value().get() {
                match result {
                    Ok(_) => {
                        set_joining(false);
                        invite.set(String::new());
                    }
                    Err(e) => error.set(e.to_string()),
                }
            }
        });
        let accounts = Resource::new(
            move || {
                (
                    user_id.get(),
                    delete_list.version().get(),
                    edit_list.version().get(),
                    leave_list_action.version().get(),
                    redeem.version().get(),
                )
            },
            |(user, ..)| async move {
                if user.is_some() {
                    get_lists_with_permissions().await
                } else {
                    Ok(Vec::new())
                }
            },
        );
        Effect::new(move |_| {
            let owner = user_id.get();
            leptos::task::spawn_local(async move {
                match GuestListHandle::list().await {
                    Ok(lists) => {
                        let _ = summaries.try_set(lists.clone());
                        // Resume only work explicitly bound to this account.
                        // Preserve an offline/error entry rather than hiding it.
                        for list in lists {
                            if list.online.as_ref().is_some_and(|b| {
                                !b.legacy
                                    && owner.is_some_and(|id| id.to_string() == b.owner)
                                    && b.acknowledged < list.revision
                            }) && let Ok(h) = GuestListHandle::open(&list.id).await
                            {
                                let _ = h.flush().await;
                                h.close();
                            }
                        }
                        if let Ok(lists) = GuestListHandle::list().await {
                            let _ = summaries.try_set(lists);
                        }
                        if !accounts.is_disposed() {
                            accounts.refetch();
                        }
                    }
                    Err(e) => {
                        let _ = error.try_set(e);
                    }
                }
                let _ = local_loaded.try_set(true);
            });
        });
        let go = StoredValue::new_local(use_navigate());
        let create = move |_| {
            if busy.get_untracked() || name.get_untracked().trim().is_empty() {
                return;
            }
            busy.set(true);
            error.set(String::new());
            let name = name.get_untracked();
            // Signed-out users never see the toggle, so never hand off online.
            let online = storage_online.get_untracked() && user_id.get_untracked().is_some();
            leptos::task::spawn_local(async move {
                match GuestListHandle::create(name.trim()).await {
                    Ok(handle) => {
                        let id = handle.id();
                        handle.close();
                        go.try_with_value(|go| {
                            go(&device_list_href(&id, online), Default::default())
                        });
                    }
                    Err(e) => {
                        let _ = error.try_set(e);
                    }
                }
                let _ = busy.try_set(false);
            });
        };
        let local_cards = Signal::derive(move || {
            let text = filter.get().to_lowercase();
            let owner = user_id.get();
            let account_loaded = accounts.get().is_some_and(|r| r.is_ok());
            summaries
                .get()
                .into_iter()
                .filter(|l| match &l.online {
                    None => true,
                    Some(b) => {
                        owner.is_some_and(|id| id.to_string() == b.owner)
                            && !(account_loaded && (b.legacy || b.acknowledged >= l.revision))
                    }
                })
                .filter(|l| l.name.to_lowercase().contains(&text))
                .collect::<Vec<_>>()
        });
        let online_cards = Signal::derive(move || {
            let text = filter.get().to_lowercase();
            let owner = user_id.get();
            let linked: HashSet<i32> = summaries
                .get()
                .iter()
                .filter(|l| {
                    l.online
                        .as_ref()
                        .is_some_and(|b| !b.legacy && b.acknowledged < l.revision)
                })
                .filter_map(|l| l.online.as_ref())
                .filter(|b| owner.is_some_and(|id| id.to_string() == b.owner))
                .filter_map(|b| b.list_id)
                .collect();
            accounts
                .get()
                .and_then(Result::ok)
                .unwrap_or_default()
                .into_iter()
                .filter(|l| {
                    !linked.contains(&l.list.id) && l.list.name.to_lowercase().contains(&text)
                })
                .collect::<Vec<_>>()
        });
        view! {
            <MetaTitle title=move || t_string!(i18n, lists_meta_title).to_string() />
            <MetaDescription text=move || t_string!(i18n, lists_meta_desc).to_string() />
            <MetaRobotsNoIndex />
            <section class="space-y-4" data-testid="lists-workspace">
                <header class="flex flex-wrap items-center justify-between gap-3">
                    <h1 class="text-2xl font-bold">{t!(i18n, lists_page_title)}</h1>
                    <div class="flex flex-wrap gap-2">
                        <button class="btn-secondary" data-testid="list-restore-open" on:click=move |_| {error.set(String::new());set_restoring(true);} >{t!(i18n, online_restore)}</button>
                        <Show when=move || user_id.get().is_some()><button class="btn-secondary" data-testid="list-join-open" on:click=move |_| {error.set(String::new());set_joining(true);} >{t!(i18n, lists_redeem_invite_label)}</button></Show>
                        <button class="btn-primary" data-testid="list-new" on:click=move |_| {error.set(String::new());storage_online.set(true);set_creating(true);} >{t!(i18n, online_new)}</button>
                    </div>
                </header>
                <input type="search" class="input w-full" data-testid="lists-search" aria-label=move || t_string!(i18n, search_your_lists).to_string() placeholder=move || t_string!(i18n, search_your_lists).to_string() prop:value=move || filter.get() on:input=move |e| filter.set(event_target_value(&e)) />
                <Show when=move || !creating() && !restoring() && !joining() && !error.get().is_empty()><p role="alert" class="text-red-400">{move || error.get()}</p></Show>
                {move || accounts.get().and_then(Result::err).map(|error|view! {
                    <p role="alert" class="text-red-400">{t!(i18n,error_loading_lists,error=error.to_string())}</p>
                })}
                <div class="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3" data-testid="lists-grid">
                    <For each=move || local_cards.get() key=move |l| (l.id.clone(),l.revision,l.online.as_ref().map(|b|(b.list_id,b.acknowledged,b.owner.clone())),user_id.get()) children=move |list| {
                        let id=StoredValue::new(list.id.clone());
                        let binding=list.online.clone();
                        let matching=binding.as_ref().filter(|b| user_id.get_untracked().is_some_and(|u|u.to_string()==b.owner));
                        let destination=matching.filter(|b| b.legacy || b.acknowledged>=list.revision).and_then(|b|b.list_id);
                        let link=destination.map(|id|format!("/list/{id}?labs=lists-sync")).unwrap_or_else(||format!("/list/device/{}?labs=lists-sync",list.id));
                        let local=list.online.is_none();
                        let status=list.error.clone().unwrap_or_else(|| if destination.is_some() {t_string!(i18n,online_connected).to_string()} else if binding.is_some() {t_string!(i18n,online_pending).to_string()} else {t_string!(i18n,guest_workspace_saved).to_string()});
                        let original=StoredValue::new(list.name.clone());
                        let link=StoredValue::new(link);
                        let editing=RwSignal::new(false);
                        let (confirm_delete,set_confirm_delete)=signal(false);
                        let saving=RwSignal::new(false);
                        let draft=RwSignal::new(list.name.clone());
                        // Rename through a short-lived handle so the card never holds the
                        // document open; the directory re-keys on the new storage revision.
                        let save=move || {
                            let next=draft.get_untracked();
                            if saving.get_untracked() || next.trim().is_empty() { return; }
                            if next.trim()==original.get_value().trim() { editing.set(false); return; }
                            saving.set(true);
                            error.set(String::new());
                            let list_id=id.get_value();
                            leptos::task::spawn_local(async move {
                                let result=async {
                                    let h=GuestListHandle::open(&list_id).await?;
                                    let renamed=h.rename(&next);
                                    let flushed=h.flush().await;
                                    h.close();
                                    renamed.and(flushed)
                                }.await;
                                match result {
                                    Ok(()) => { let _=editing.try_set(false); reload_local(); }
                                    Err(e) => { let _=error.try_set(e); }
                                }
                                let _=saving.try_set(false);
                            });
                        };
                        let remove=move |_| {
                            if saving.get_untracked() { return; }
                            saving.set(true);
                            error.set(String::new());
                            let list_id=id.get_value();
                            leptos::task::spawn_local(async move {
                                let result=async {
                                    let h=GuestListHandle::open(&list_id).await?;
                                    h.remove().await
                                }.await;
                                match result {
                                    Ok(()) => { set_confirm_delete(false); let _=editing.try_set(false); reload_local(); }
                                    Err(e) => { let _=error.try_set(e); }
                                }
                                let _=saving.try_set(false);
                            });
                        };
                        view! {
                            <article class="panel rounded-xl p-4 flex flex-col gap-3" data-testid="list-card">
                                <Show when=move || local && editing.get() fallback=move || view! {
                                    <div class="flex justify-between items-start gap-2">
                                        <a class="text-lg font-semibold hover:underline break-words min-w-0" href=link.get_value()>{original.get_value()}</a>
                                        <Show when=move || local>
                                            <button type="button" class="btn-ghost btn-sm shrink-0 text-gray-400 hover:text-white" data-testid="list-card-edit" aria-label=move || t_string!(i18n,edit_list).to_string() title=move || t_string!(i18n,edit_list).to_string() on:click=move |_| { draft.set(original.get_value()); editing.set(true); }>
                                                <Icon icon=i::BsPencilFill />
                                            </button>
                                        </Show>
                                    </div>
                                    <p class="text-sm text-[color:var(--color-text-muted)]">{status.clone()}</p>
                                    <Show when=move || local><a class="btn-secondary self-start" data-testid="list-card-make-online" href=format!("/list/device/{}?labs=lists-sync&make_online=1",id.get_value())>{t!(i18n,online_make)}</a></Show>
                                    {destination.map(|dest|view! { <a class="btn-secondary self-start" href=format!("/list/{dest}?labs=lists-sync")>{t!(i18n,online_open)}</a> })}
                                }>
                                    <div class="flex flex-col gap-3 w-full">
                                        <div>
                                            <label class="label text-sm font-semibold">{t!(i18n,list_name)}</label>
                                            <input class="input w-full" data-testid="list-card-name" aria-label=move || t_string!(i18n,list_name).to_string() maxlength="100" prop:value=move || draft.get() on:input=move |ev| draft.set(event_target_value(&ev)) on:keydown=move |ev| { if ev.key()=="Enter" { ev.prevent_default(); save(); } else if ev.key()=="Escape" { editing.set(false); } } />
                                        </div>
                                        <div class="flex gap-2 justify-end mt-2">
                                            <button type="button" class="btn-secondary btn-sm" data-testid="list-card-cancel" disabled=move || saving.get() on:click=move |_| editing.set(false)>
                                                <Icon icon=i::AiCloseOutlined /> {t!(i18n,cancel)}
                                            </button>
                                            <button type="button" class="btn-primary btn-sm" data-testid="list-card-save" disabled=move || saving.get() || draft.get().trim().is_empty() on:click=move |_| save()>
                                                <Icon icon=i::BiSaveSolid /> {t!(i18n,save)}
                                            </button>
                                        </div>
                                        <div class="border-t border-gray-600/50 my-2"></div>
                                        <div class="flex justify-between items-center">
                                            <span class="text-red-400 text-sm font-semibold">{t!(i18n,danger_zone)}</span>
                                            <button type="button" class="btn-danger btn-sm" data-testid="list-card-delete" disabled=move || saving.get() on:click=move |_| set_confirm_delete(true)>
                                                <Icon icon=i::BiTrashSolid /> {t!(i18n,delete)}
                                            </button>
                                        </div>
                                    </div>
                                </Show>
                                <Show when=confirm_delete><Modal set_visible=set_confirm_delete aria_label=Signal::derive(move || t_string!(i18n,guest_workspace_delete_label).to_string())>
                                    <div class="flex flex-col gap-4">
                                        <h2 class="text-xl font-bold text-[color:var(--brand-fg)]">{t!(i18n,list_delete_confirm_title)}</h2>
                                        <p class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n,guest_workspace_delete_confirm)}</p>
                                        <div class="flex justify-end gap-2">
                                            <button type="button" class="btn-secondary" disabled=move || saving.get() on:click=move |_| set_confirm_delete(false)>
                                                <Icon icon=i::AiCloseOutlined /> {t!(i18n,guest_workspace_keep)}
                                            </button>
                                            <button type="button" class="btn-danger" data-testid="list-card-confirm-delete" disabled=move || saving.get() on:click=remove>
                                                <Icon icon=i::BiTrashSolid /> {t!(i18n,guest_workspace_delete_permanent)}
                                            </button>
                                        </div>
                                    </div>
                                </Modal></Show>
                            </article>
                        }
                    } />
                    <For each=move || online_cards.get() key=|l|l.list.id children=move |list| view! { <ListCard list edit_list delete_list leave_list_action user_id /> } />
                </div>
                <Show when=move || local_loaded.get() && accounts.get().is_some_and(|r|r.is_ok()) && local_cards.get().is_empty() && online_cards.get().is_empty()>
                    <p class="py-8 text-center text-[color:var(--color-text-muted)]">{t!(i18n,online_empty)}</p>
                </Show>
                <Show when=move || user_id.get().is_none()><p class="text-sm"><a class="underline" rel="external" href="/login?next=/list%3Flabs%3Dlists-sync">{t!(i18n,lists_device_sign_in)}</a></p></Show>
                <Show when=creating><Modal set_visible=set_creating aria_label=Signal::derive(move || t_string!(i18n,online_new).to_string())>
                    <div class="space-y-3"><h2 class="text-xl font-bold">{t!(i18n,online_new)}</h2>
                    <input class="input w-full" data-testid="device-list-name" aria-label=move || t_string!(i18n,list_name).to_string() placeholder=move || t_string!(i18n,guest_workspace_placeholder).to_string() prop:value=move || name.get() on:input=move |ev| name.set(event_target_value(&ev)) maxlength="100" />
                    <Show when=move || user_id.get().is_some()>
                        <div class="space-y-2" data-testid="device-list-storage">
                            <p id="device-list-storage-label" class="label font-semibold">{t!(i18n,online_new_storage_label)}</p>
                            <div class="flex flex-wrap gap-2" role="group" aria-labelledby="device-list-storage-label">
                                <button type="button" class=move || if storage_online.get() { "btn-primary min-h-11" } else { "btn-secondary min-h-11" } aria-pressed=move || storage_online.get().to_string() data-testid="device-list-storage-online" on:click=move |_| storage_online.set(true)>{t!(i18n,online_new_storage_online)}</button>
                                <button type="button" class=move || if storage_online.get() { "btn-secondary min-h-11" } else { "btn-primary min-h-11" } aria-pressed=move || (!storage_online.get()).to_string() data-testid="device-list-storage-local" on:click=move |_| storage_online.set(false)>{t!(i18n,online_new_storage_local)}</button>
                            </div>
                            <p class="text-sm text-[color:var(--color-text-muted)]" data-testid="device-list-storage-desc">{move || if storage_online.get() { Either::Left(t!(i18n,online_new_storage_online_desc)) } else { Either::Right(t!(i18n,online_new_storage_local_desc)) }}</p>
                        </div>
                    </Show>
                    <Show when=move || !error.get().is_empty()><p role="alert" class="text-red-400">{move || error.get()}</p></Show>
                    <button class="btn-primary" data-testid="device-list-create" disabled=move || busy.get() || name.get().trim().is_empty() on:click=create>{t!(i18n,create_list)}</button></div>
                </Modal></Show>
                <Show when=restoring><Modal set_visible=set_restoring aria_label=Signal::derive(move || t_string!(i18n,online_restore).to_string())>
                    <div class="space-y-3"><h2 class="text-xl font-bold">{t!(i18n,online_restore)}</h2>
                    <div class="flex flex-col gap-2">{move || summaries.get().into_iter().filter(|l|l.online.as_ref().is_some_and(|b|user_id.get().is_some_and(|id|id.to_string()==b.owner))).map(|l|view! {
                        <a class="underline" href=format!("/list/device/{}?labs=lists-sync&recovery=1",l.id)>{t!(i18n,guest_workspace_export)}": "{l.name}</a>
                    }).collect_view()}</div>
                    <label for="device-restore">{t!(i18n,guest_workspace_paste_backup)}</label>
                    <textarea id="device-restore" class="input w-full h-32" data-testid="device-list-backup" prop:value=move || backup.get() on:input=move |ev|backup.set(event_target_value(&ev)) />
                    <Show when=move || !error.get().is_empty()><p role="alert" class="text-red-400">{move || error.get()}</p></Show>
                    <button class="btn-primary" data-testid="device-list-restore" disabled=move || busy.get() || backup.get().trim().is_empty() on:click=move |_| {
                        busy.set(true); error.set(String::new()); let text=backup.get_untracked();
                        leptos::task::spawn_local(async move {
                            match GuestListHandle::restore(&text).await {
                                Ok(h) => {let id=h.id();h.close();go.try_with_value(|go|go(&format!("/list/device/{id}?labs=lists-sync"),Default::default()));}
                                Err(e) => {let _=error.try_set(e);}
                            }
                            let _=busy.try_set(false);
                        });
                    }>{t!(i18n,guest_workspace_restore)}</button></div>
                </Modal></Show>
                <Show when=joining><Modal set_visible=set_joining aria_label=Signal::derive(move || t_string!(i18n,lists_redeem_invite_label).to_string())><div class="space-y-3">
                    <h2 class="text-xl font-bold">{t!(i18n,lists_redeem_invite_label)}</h2>
                    <input class="input w-full" data-testid="list-join-code" aria-label=move ||t_string!(i18n,lists_invite_code_placeholder).to_string() prop:value=move ||invite.get() disabled=move ||redeem.pending().get() on:input=move |e|invite.set(event_target_value(&e)) />
                    <Show when=move || !error.get().is_empty()><p role="alert" class="text-red-400">{move || error.get()}</p></Show>
                    <button class="btn-primary" data-testid="list-join-submit" disabled=move ||invite.get().trim().is_empty() || redeem.pending().get() on:click=move |_|{error.set(String::new());redeem.dispatch(invite.get_untracked());}>{t!(i18n,lists_redeem_button)}</button>
                </div></Modal></Show>
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
        let account_required = RwSignal::new(false);
        Effect::new(move |_| {
            let id = params.with(|p| p.get("device_id").unwrap_or_default());
            // The router is leaving this route: its params already describe
            // the next match. The editor's own cleanup closes the handle, so
            // opening "" here would only report a missing list on the way out.
            if id.is_empty() {
                return;
            }
            if let Some(old) = loaded.get_untracked() {
                old.close();
            }
            loaded.set(None);
            generation.update(|n| *n += 1);
            let request = generation.get_untracked();
            account_required.set(false);
            error.set(String::new());
            leptos::task::spawn_local(async move {
                match GuestListHandle::open(&id).await {
                    Ok(handle) => {
                        if let Some(binding) = handle.online() {
                            let allowed = crate::api::get_login_fresh()
                                .await
                                .is_ok_and(|u| u.id.to_string() == binding.owner);
                            if !allowed {
                                if generation.try_get_untracked() == Some(request) {
                                    account_required.set(true);
                                }
                                handle.close();
                                return;
                            }
                        }
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
            <Show when=move || loaded.with(Option::is_none)><a class="inline-block text-sm text-[color:var(--color-text-muted)] hover:underline mb-2" href="/list?labs=lists-sync">{t!(i18n, guest_workspace_back)}</a></Show>
            <Show when=move || !error.get().is_empty()><p role="alert" class="text-red-400">{move || error.get()}</p></Show>
            <Show when=move ||account_required.get()><div class="panel rounded-xl p-5 space-y-3" data-testid="list-online-account-required">
                <p>{t!(i18n,online_account_required)}</p>
                <a class="btn-primary" rel="external" href=move || format!("/login?next={}",String::from(js_sys::encode_uri_component(&format!("/list/device/{}?labs=lists-sync",params.with(|p|p.get("device_id").unwrap_or_default())))))>{t!(i18n,lists_device_sign_in)}</a>
            </div></Show>
            {move || loaded.get().map(|handle| view! { <DeviceEditor handle /> })}
        }
    }

    #[component]
    fn DeviceEditor(handle: GuestListHandle) -> impl IntoView {
        let i18n = use_i18n();
        let query = use_query_map_or_default();
        let recovery =
            Memo::new(move |_| query.with(|q| q.get("recovery").as_deref() == Some("1")));
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
        // Build and Shop share fetched offers and exact item coverage. The
        // served scope only changes with a successful replacement response.
        let feed = RwSignal::new(PriceFeed::Missing(MissingReason::NotRequested));
        let offers_scope = RwSignal::new(None::<String>);
        let fetched_items = RwSignal::new(HashSet::<i32>::new());
        let price_error = RwSignal::new(String::new());
        let busy = RwSignal::new(false);
        let pending_scope = RwSignal::new(None);
        let ticket = StoredValue::new(LookupTicket::default());
        // `?buy=true` reopens Shop after a sign-in round trip, the same way
        // the account editor reads it, so the handoff keeps the player's mode.
        let shop = RwSignal::new(query.with_untracked(|q| q.get("buy").as_deref() == Some("true")));
        let shop_mounted = Memo::new(move |previous: Option<&bool>| {
            shop.get() || previous.copied().unwrap_or(false)
        });
        // Editor-owned travel limit and exclusions (#1480): device lists have
        // no other exclusion filter, so the policy alone narrows served rows.
        let travel = crate::components::list_travel_state::use_list_travel();
        let device_id = StoredValue::new(handle.with_value(|h| h.id()));
        // A list that has never chosen a scope prices against the player's
        // price zone (the site-wide "prices for" setting), like every other
        // market page — not their home world, which is where trips *start*.
        let (zone, _) = crate::global_state::home_world::get_price_zone();
        let scope = Signal::derive(move || {
            revision.track();
            handle
                .with_value(|h| h.meta().scope)
                .or_else(|| zone.get().map(Into::<AnySelector>::into))
        });
        let set_scope = move |value: Option<AnySelector>| {
            if recovery.get_untracked() {
                return;
            }
            if let Some(value) = value
                && let Err(e) = handle.with_value(|h| h.set_scope(value))
            {
                error.set(e);
            }
        };
        // One lookup for both the button and the eager fetch below. Neither
        // writes the scope to the document: only the picker does, so the
        // default can keep following the price zone until the player picks.
        let fetch_prices = move |selector: AnySelector| {
            if busy.get_untracked() && pending_scope.get_untracked() == Some(selector) {
                return;
            }
            let Some(scope_name) =
                crate::global_state::use_world_helper()
                    .ok()
                    .and_then(|helper| {
                        helper
                            .lookup_selector(selector)
                            .map(|scope| scope.get_name().to_string())
                    })
            else {
                return;
            };
            let ids: HashSet<_> = handle
                .with_value(|h| h.rows())
                .into_iter()
                .map(|row| row.key.item_id)
                .collect();
            let request = ticket
                .try_update_value(|ticket| ticket.begin())
                .unwrap_or_default();
            busy.set(true);
            pending_scope.set(Some(selector));
            price_error.set(String::new());
            feed.update(|feed| *feed = feed.begin_fetch());
            leptos::task::spawn_local(async move {
                let result =
                    crate::api::get_bulk_listings(scope_name.trim(), ids.iter().copied()).await;
                if !ticket
                    .try_with_value(|ticket| ticket.accepts(request))
                    .unwrap_or(false)
                {
                    return;
                }
                match result {
                    Ok(data) => {
                        let _ = offers.try_set(
                            data.into_iter()
                                .filter(|(id, _)| ids.contains(id))
                                .map(|(id, rows)| {
                                    (id, rows.into_iter().map(|(listing, _)| listing).collect())
                                })
                                .collect(),
                        );
                        let _ = fetched_items.try_set(ids);
                        let _ = offers_scope.try_set(Some(scope_name));
                        let _ = feed
                            .try_update(|feed| *feed = feed.after_fetch(Some(chrono::Utc::now())));
                    }
                    Err(e) => {
                        tracing::warn!(error = ?e, "Device list price lookup failed");
                        let _ = price_error
                            .try_set(t_string!(i18n, guest_workspace_prices_failed).to_string());
                        let _ = feed.try_update(|feed| *feed = feed.after_fetch(None));
                    }
                }
                let _ = busy.try_set(false);
            });
        };
        let refresh_prices = move |_| {
            if let Some(selector) = scope.get_untracked() {
                fetch_prices(selector);
            }
        };
        // Prices load on their own: as soon as the list has a scope and
        // rows, and again whenever either changes. Two exceptions — a
        // trip the player is following must not have its stacks repriced
        // underneath them (the Refresh prices button remains for that),
        // and a failed lookup is not retried until something changes, so
        // an outage never turns into a request loop. Edits are debounced
        // so pasting a recipe's ingredients costs one request, not one
        // per ingredient.
        let trip_active = RwSignal::new(false);
        let auto_key = StoredValue::new(None::<(AnySelector, BTreeSet<i32>)>);
        let auto_timer: StoredValue<Option<gloo_timers::callback::Timeout>, LocalStorage> =
            StoredValue::new_local(None);
        Effect::new(move |_| {
            if recovery.get() || trip_active.get() {
                auto_timer.set_value(None);
                return;
            }
            let Some(selector) = scope.get() else {
                return;
            };
            revision.track();
            let ids: BTreeSet<i32> = handle
                .with_value(|h| h.rows())
                .into_iter()
                .map(|row| row.key.item_id)
                .collect();
            if ids.is_empty() {
                return;
            }
            let key = (selector, ids);
            if auto_key.with_value(|last| last.as_ref() == Some(&key)) {
                return;
            }
            auto_key.set_value(Some(key));
            auto_timer.set_value(Some(gloo_timers::callback::Timeout::new(400, move || {
                fetch_prices(selector);
            })));
        });
        on_cleanup(move || auto_timer.set_value(None));
        let recipe_open = RwSignal::new(false);
        let storage_open = RwSignal::new(recovery.get_untracked());
        let confirm_delete = RwSignal::new(false);
        let deleting = RwSignal::new(false);
        let navigate = StoredValue::new_local(use_navigate());
        let following = RwSignal::new(false);
        let follow_retry = RwSignal::new(0u64);
        let draft_changed = RwSignal::new(0u64);
        Effect::new(move |_| {
            follow_retry.track();
            draft_changed.track();
            status.track();
            revision.track();
            // Keep these dependencies while an auth check is in flight, so
            // closing a dialog can resume a handoff it temporarily deferred.
            let recipe_visible = recipe_open.get();
            let storage_visible = storage_open.get();
            if recovery.get()
                || following.get_untracked()
                || recipe_visible
                || storage_visible
                || editor_has_drafts()
                || !handle.with_value(|h| h.is_saved() && !h.is_saving())
            {
                return;
            }
            let Some(binding) = handle.with_value(|h| h.online()) else {
                return;
            };
            let Some(id) = binding.list_id else {
                return;
            };
            if !binding.legacy
                && binding.acknowledged.to_string() != handle.with_value(|h| h.storage_revision())
            {
                return;
            }
            let h = handle.get_value();
            let observed_revision = revision.get_untracked();
            let observed_storage_revision = h.storage_revision();
            following.set(true);
            leptos::task::spawn_local(async move {
                let user = crate::api::get_login_fresh().await;
                if following.is_disposed() {
                    return;
                }
                // Do not navigate on an earlier acknowledgement: the player
                // may have edited again while the session request was pending.
                let current_matches = h.online().is_some_and(|current| {
                    current.owner == binding.owner
                        && current.list_id == Some(id)
                        && current.legacy == binding.legacy
                        && (current.legacy
                            || current.acknowledged.to_string() == h.storage_revision())
                });
                let can_follow = !recovery.get_untracked()
                    && !recipe_open.get_untracked()
                    && !storage_open.get_untracked()
                    && !editor_has_drafts()
                    && h.is_saved()
                    && !h.is_saving()
                    && current_matches;
                if let Ok(user) = user
                    && user.id.to_string() == binding.owner
                    && can_follow
                {
                    navigate.try_with_value(|go| {
                        go(
                            &travel.online_href(id, shop.get_untracked()),
                            Default::default(),
                        )
                    });
                    return;
                }
                let _ = following.try_set(false);
                // Effects may have observed a completed save while following
                // was true. Give changed state one fresh attempt, but never
                // poll an unchanged failed/account-mismatched session.
                if revision.try_get_untracked() != Some(observed_revision)
                    || h.storage_revision() != observed_storage_revision
                {
                    let _ = follow_retry.try_update(|value| *value += 1);
                }
            });
        });
        // Old device URLs and a reload after a lost acknowledgement drain their
        // durable continuation before the effect above follows the destination.
        let initial = handle.get_value();
        leptos::task::spawn_local(async move {
            let _ = initial.flush().await;
        });
        on_cleanup(move || handle.with_value(|h| h.close()));
        // Explains a shortcut that found nothing to do (#1430); any later
        // edit clears it. Errors take precedence in the feedback line.
        let notice = RwSignal::new(String::new());
        let apply = Callback::new(move |edit| {
            if recovery.get_untracked() {
                return;
            }
            notice.set(String::new());
            if let Err(e) = handle.with_value(|h| h.apply(edit)) {
                error.set(e);
            }
        });
        let undo = Callback::new(move |()| {
            if recovery.get_untracked() {
                return;
            }
            if handle.with_value(|h| h.undo()) {
                notice.set(String::new());
            } else {
                notice.set(t_string!(i18n, lists_workspace_nothing_to_undo).to_string());
            }
        });
        let redo = Callback::new(move |()| {
            if recovery.get_untracked() {
                return;
            }
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
            modal_open: Signal::derive(move || {
                confirm_delete.get() || storage_open.get() || recovery.get()
            }),
        });
        let sort = RwSignal::new(None::<SortSpec>);
        // In-memory: device lists have no URL-carried filters.
        let hide_acquired = RwSignal::new(false);
        let guest_rows = Signal::derive(move || {
            revision.track();
            let policy = travel.policy.get();
            handle.with_value(|h| {
                h.rows()
                    .iter()
                    .filter_map(|row| adapter::to_list_item(0, row))
                    .map(|item| {
                        let prices = offers
                            .with(|offers| offers.get(&item.item_id).cloned().unwrap_or_default());
                        let prices = if policy.narrows() {
                            policy.filter_listings(&prices)
                        } else {
                            prices
                        };
                        (item, prices)
                    })
                    .collect::<Vec<_>>()
            })
        });
        let source = ListWorkspaceSource {
            hide_acquired: hide_acquired.into(),
            set_hide_acquired: Callback::new(move |hide| hide_acquired.set(hide)),
            reset_filters: Callback::new(move |()| hide_acquired.set(false)),
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
            rows: guest_rows,
            estimate_available: Signal::derive(|| true),
            estimate: Memo::new(move |_| {
                guest_rows.with(|rows| {
                    fetched_items.with(|fetched| {
                        ultros_calc::list_estimate::estimate_list_items_with_coverage(rows, fetched)
                    })
                })
            })
            .into(),
            market: feed.into(),
            scope_name: offers_scope.into(),
            can_write: Signal::derive(move || !recovery.get()),
            edit: Callback::new(move |item| apply.run(Edit::Edit(item))),
            remove: Callback::new(move |id| apply.run(Edit::Remove(id))),
            remove_many: Callback::new(move |ids| apply.run(Edit::RemoveMany(ids))),
            set_quality_many: Callback::new(move |(ids, hq)| apply.run(Edit::SetQuality(ids, hq))),
            bulk_pending: Signal::derive(|| false),
            sort: sort.into(),
            set_sort: Callback::new(move |spec| sort.set(spec)),
        };
        let highlighted = crate::components::cart::use_changed_row_highlight(source.rows);
        let legacy_cart = use_legacy_cart();
        let name = Signal::derive(move || {
            revision.track();
            handle.with_value(|h| h.meta().name)
        });
        let refreshing = Signal::derive(move || busy.get() && scope.get() == pending_scope.get());
        view! {
            <section data-testid="device-list-editor"
                on:input=move |_| draft_changed.update(|value| *value += 1)
                on:change=move |_| draft_changed.update(|value| *value += 1)
                on:keyup=move |_| draft_changed.update(|value| *value += 1)
                on:focusout=move |_| draft_changed.update(|value| *value += 1)>
                <crate::components::list_workspace_shell::ListWorkspaceShell
                    name
                    can_rename=Signal::derive(move || !recovery.get())
                    on_rename=Callback::new(move |value: String| {
                        if let Err(e) = handle.with_value(|h| h.rename(&value)) {
                            error.set(e);
                        }
                    })
                    status
                    status_testid="device-list-status"
                    primary=move || view! {
                        <Show when=move || !recovery.get()><crate::routes::guest_list_adoption::DeviceListAdoption handle=handle.get_value() continuation=Signal::derive(move || travel.device_continue_href(&device_id.get_value(), shop.get())) /></Show>
                        <Show when=move || legacy_cart.get() && !selected.get().is_empty()>
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
                    }
                    menu_testid="device-list-storage-toggle"
                    menu_open=storage_open
                    menu=move || view! {
                        <Show when=move || !error.get().is_empty()><p role="alert" class="text-red-400">{move || error.get()}</p></Show>
                        <crate::routes::guest_list_adoption::DeviceListSeparateUpload handle=handle.get_value() on_connect=Callback::new(move |()| storage_open.set(false)) />
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
                    }
                    notices=move || view! {
                        <Show when=move || !storage_open.get() && !error.get().is_empty()><p role="alert" class="text-red-400">{move || error.get()}</p></Show>
                        <Show when=move || {
                            draft_changed.track();
                            handle.with_value(|h| h.online().is_some_and(|online| online.list_id.is_some())) && editor_has_drafts()
                        }><p class="text-sm" role="status">{t!(i18n, online_finish_edit)}</p></Show>
                        <Show when=move || !recovery.get() && !price_error.get().is_empty()><p role="alert" class="text-sm text-red-400" data-testid="device-prices-error">{move || price_error.get()}</p></Show>
                    }
                    show_controls=Signal::derive(move || !recovery.get())
                    shop
                    set_shop=Callback::new(move |value| shop.set(value))
                    scope
                    set_scope=Callback::new(set_scope)
                    can_set_scope=Signal::derive(|| true)
                    refresh=move || view! {
                        <button type="button" class="btn-secondary" data-testid="device-prices-refresh" aria-busy=move || busy.get().to_string() disabled=move || scope.get().is_none() || refreshing.get() on:click=refresh_prices>
                            {move || if refreshing.get() { t_string!(i18n, guest_workspace_refreshing).to_string() } else if feed.get().has_prices() { t_string!(i18n, guest_workspace_refresh_prices).to_string() } else { t_string!(i18n, guest_workspace_prices).to_string() }}
                        </button>
                    }
                    price_row_testid="device-price-controls"
                    travel
                >
                    // Mounted on first use and then only hidden, so a return to
                    // Build keeps the chosen trip and its recorded stacks.
                    <div class:hidden=move || !shop.get()>
                        <Show when=move || shop_mounted.get()><DeviceShop handle=handle.get_value() source travel_policy=travel.policy trip_active /></Show>
                    </div>
                    <div class:hidden=move || shop.get()>
                    {move || if legacy_cart.get() {
                        view! { <ListBuildWorkspace source selected_items=selected highlighted /> }.into_any()
                    } else {
                        view! { <ListCart source selected_items=selected highlighted /> }.into_any()
                    }}
                    </div>
                </crate::components::list_workspace_shell::ListWorkspaceShell>
            </section>
        }
    }

    #[component]
    fn DeviceShop(
        handle: GuestListHandle,
        source: ListWorkspaceSource,
        travel_policy: Signal<TravelPolicy>,
        /// Raised while a trip is adopted, so the editor's eager price
        /// lookups pause instead of repricing the stacks being followed.
        trip_active: RwSignal<bool>,
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
        let error = RwSignal::new(String::new());
        let input = Signal::derive(move || {
            revision.track();
            let data = crate::global_state::xiv_data::tracked_data();
            ShopInput {
                title: handle.with_value(|h| h.meta().name),
                price_feed: source.market.get(),
                build_estimate: source.estimate.get(),
                estimate_available: source.estimate_available.get(),
                rows: source
                    .rows
                    .get()
                    .into_iter()
                    .map(|(item, listings)| ShopRow {
                        key: item.id.to_string(),
                        name: data
                            .items
                            .get(&xiv_gen::ItemId(item.item_id))
                            .map(|i| i.name.to_string())
                            .unwrap_or_else(|| {
                                t_string!(i18n, guest_workspace_item, id = item.item_id).to_string()
                            }),
                        item_id: item.item_id,
                        hq: item.hq,
                        needed: item.quantity.unwrap_or(1),
                        acquired: item.acquired.unwrap_or(0),
                        listings,
                    })
                    .collect(),
                observed_at: None,
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
                <p role="alert">{move || error.get()}</p>
                <ListShop input on_purchase=Callback::new(move |(key, delta): (String, i32)| {
                    if let Ok(id) = key.parse::<i32>()
                        && let Some(key) = adapter::key_of(id)
                        && let Err(e) = handle.with_value(|h| h.apply(Edit::RecordPurchase { key, quantity: i64::from(delta) })) {
                        error.set(e);
                    }
                }) on_undo=Callback::new(move |()| {
                    if let Err(e) = handle.with_value(|h| h.apply(Edit::UndoPurchase)) { error.set(e); }
                }) can_undo_purchase=Signal::derive(move || handle.with_value(|h| h.can_undo_purchase())) can_edit=Signal::derive(|| true) travel_policy on_trip_active=Callback::new(move |active: bool| trip_active.set(active)) />
            </div>
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_list_opens_the_plain_device_editor() {
        assert_eq!(
            device_list_href("abc-123", false),
            "/list/device/abc-123?labs=lists-sync"
        );
    }

    #[test]
    fn online_list_resumes_make_online_in_the_editor() {
        assert_eq!(
            device_list_href("abc-123", true),
            "/list/device/abc-123?labs=lists-sync&make_online=1"
        );
    }
}
