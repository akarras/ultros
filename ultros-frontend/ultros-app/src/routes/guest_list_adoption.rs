//! Explicit, retry-safe transfer of a device list into the signed-in account.
#[cfg(feature = "hydrate")]
pub use browser::{DeviceListAdoption, DeviceListsAdoption};

#[cfg(feature = "hydrate")]
mod browser {
    use crate::api::{adopt_guest_list, get_login};
    use crate::components::world_picker::WorldPicker;
    use crate::global_state::home_world::get_price_zone;
    use crate::i18n::*;
    use crate::list_doc::guest::{GuestListHandle, GuestListSummary};
    use leptos::prelude::*;
    use leptos_i18n::I18nContext;
    use std::collections::{BTreeMap, BTreeSet};
    use ultros_api_types::list::{AdoptGuestList, AdoptGuestListResponse, GuestListItem};
    use ultros_api_types::world_helper::AnySelector;
    use wasm_bindgen::prelude::*;

    // The complete immutable attempt is durable before the first network request.
    // Keeping it after acknowledgement also makes reload/retry idempotent. The
    // server additionally deduplicates account + device identity across tabs.
    #[wasm_bindgen(inline_js = r#"
export function deviceAdoptionAttempt(key, proposed) {
  const previous = localStorage.getItem(key);
  if (previous !== null) return previous;
  localStorage.setItem(key, proposed);
  if (localStorage.getItem(key) !== proposed) throw new Error('Could not save the transfer attempt.');
  return proposed;
}
export function deviceAdoptionReceipt(key, receipt) {
  if (receipt !== '') localStorage.setItem(key, receipt);
  return localStorage.getItem(key) || '';
}
"#)]
    extern "C" {
        #[wasm_bindgen(catch, js_name = deviceAdoptionAttempt)]
        fn attempt(key: &str, proposed: &str) -> Result<String, JsValue>;
        #[wasm_bindgen(catch, js_name = deviceAdoptionReceipt)]
        fn receipt(key: &str, value: &str) -> Result<String, JsValue>;
    }

    async fn transfer(
        i18n: I18nContext<Locale, I18nKeys>,
        h: &GuestListHandle,
        owner: u64,
        wdr_filter: AnySelector,
    ) -> Result<AdoptGuestListResponse, String> {
        let expected_owner = i64::try_from(owner)
            .map_err(|_| t_string!(i18n, adoption_invalid_account).to_string())?;
        let current = get_login().await.map_err(|e| e.to_string())?;
        if current.id != owner {
            return Err(t_string!(i18n, adoption_account_changed).to_string());
        }
        h.flush().await?;
        let request = AdoptGuestList {
            expected_owner,
            adoption_key: format!("device-{}-{}", owner, h.id()),
            device_list_id: h.id(),
            source_revision: h.storage_revision(),
            name: h.meta().name,
            wdr_filter,
            items: h
                .rows()
                .into_iter()
                .map(|r| {
                    Ok(GuestListItem {
                        item_id: r.key.item_id,
                        hq: r.key.hq(),
                        quantity: i32::try_from(r.need)
                            .map_err(|_| t_string!(i18n, adoption_quantity_large).to_string())?,
                        acquired: i32::try_from(r.acquired)
                            .map_err(|_| t_string!(i18n, adoption_owned_large).to_string())?,
                        target_price: r.target,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?,
        };
        let proposed = serde_json::to_string(&request).map_err(|e| e.to_string())?;
        let saved = attempt(
            &format!("ultros:device-adoption:v1:{}:{}", owner, h.id()),
            &proposed,
        )
        .map_err(|_| t_string!(i18n, adoption_attempt_storage).to_string())?;
        let request: AdoptGuestList = serde_json::from_str(&saved)
            .map_err(|_| t_string!(i18n, adoption_attempt_unreadable).to_string())?;
        if request.expected_owner != expected_owner || request.device_list_id != h.id() {
            return Err(t_string!(i18n, adoption_attempt_mismatch).to_string());
        }
        let response = adopt_guest_list(request).await.map_err(|e| e.to_string())?;
        let current = get_login().await.map_err(|e| e.to_string())?;
        if current.id != owner
            || response.owner != expected_owner
            || response.device_list_id != h.id()
        {
            return Err(t_string!(i18n, adoption_account_changed_during).to_string());
        }
        let text = serde_json::to_string(&response).map_err(|e| e.to_string())?;
        receipt(
            &format!("ultros:device-adoption:v1:{}:{}:receipt", owner, h.id()),
            &text,
        )
        .map_err(|_| t_string!(i18n, adoption_receipt_storage).to_string())?;
        // Never delete the source: it may have changed during HTTP,
        // or this may acknowledge an older attempt after a reload.
        Ok(response)
    }

    fn read_receipt(owner: u64, id: &str) -> Option<AdoptGuestListResponse> {
        let text = receipt(
            &format!("ultros:device-adoption:v1:{owner}:{id}:receipt"),
            "",
        )
        .ok()?;
        let response: AdoptGuestListResponse = serde_json::from_str(&text).ok()?;
        (u64::try_from(response.owner).ok() == Some(owner) && response.device_list_id == id)
            .then_some(response)
    }

    #[component]
    pub fn DeviceListsAdoption(summaries: RwSignal<Vec<GuestListSummary>>) -> impl IntoView {
        let i18n = use_i18n();
        let login = Resource::new(|| (), |_| get_login());
        let selected = RwSignal::new(BTreeSet::<String>::new());
        let results =
            RwSignal::new(BTreeMap::<String, Result<AdoptGuestListResponse, String>>::new());
        let previous_owner = RwSignal::new(None::<u64>);
        let pending = RwSignal::new(false);
        let (global, _) = get_price_zone();
        let (scope, set_scope) = signal(global.get_untracked().map(Into::<AnySelector>::into));
        Effect::new(move |_| {
            let owner = login.get().and_then(Result::ok).map(|u| u.id);
            let lists = summaries.get();
            // Restore durable per-list acknowledgements, retaining in-session
            // failures while this account stays selected.
            let mut saved = if previous_owner.get_untracked() == owner {
                results.get_untracked()
            } else {
                selected.set(BTreeSet::new());
                BTreeMap::new()
            };
            previous_owner.set(owner);
            if let Some(owner) = owner {
                for list in lists {
                    if let Some(response) = read_receipt(owner, &list.id) {
                        saved.insert(list.id, Ok(response));
                    }
                }
            }
            results.set(saved);
        });
        let add_selected = move |_| {
            if pending.get_untracked() {
                return;
            }
            let Some(Ok(user)) = login.get_untracked() else {
                return;
            };
            let Some(fallback_scope) = scope.get_untracked() else {
                return;
            };
            let ids = selected.get_untracked();
            if ids.is_empty() {
                return;
            }
            pending.set(true);
            leptos::task::spawn_local(async move {
                for id in ids {
                    // Navigation ends the queue. A request already dispatched
                    // still saves its durable receipt before returning.
                    if pending.is_disposed() {
                        break;
                    }
                    let result = match GuestListHandle::open(&id).await {
                        Ok(handle) => {
                            let filter = handle.meta().scope.unwrap_or(fallback_scope);
                            let result = transfer(i18n, &handle, user.id, filter).await;
                            handle.close();
                            result
                        }
                        Err(error) => Err(error),
                    };
                    if result.is_ok() {
                        let _ = selected.try_update(|selection| {
                            selection.remove(&id);
                        });
                    }
                    let _ = results.try_update(|outcomes| {
                        outcomes.insert(id, result);
                    });
                }
                if let Ok(lists) = GuestListHandle::list().await {
                    let _ = summaries.try_set(lists);
                }
                let _ = pending.try_set(false);
            });
        };
        view! {
            <Show when=move || !summaries.get().is_empty()>
                <details class="panel rounded-xl p-4 space-y-3" data-testid="device-lists-adoption">
                    <summary class="cursor-pointer font-semibold">{t!(i18n, adoption_heading)}</summary>
                    <p class="text-sm">{t!(i18n, adoption_retry_note)}</p>
                    <p class="text-sm">{t!(i18n, adoption_intro)}</p>
                    <Suspense fallback=move || view! { <p>{t!(i18n, adoption_checking)}</p> }>
                        {move || match login.get() {
                            Some(Ok(user)) => view! {
                                <p>{t_string!(i18n, adoption_account, name = user.username).to_string()}</p>
                                <p class="text-sm">{t!(i18n, adoption_scope)}</p>
                                <WorldPicker current_world=scope.into() set_current_world=set_scope.into() />
                                <button class="btn-secondary" disabled=move || pending.get() on:click=move |_| {
                                    selected.set(summaries.get_untracked().into_iter()
                                        .filter(|l| l.error.is_none() && !matches!(results.get_untracked().get(&l.id), Some(Ok(_))))
                                        .map(|l| l.id).collect());
                                }>{t!(i18n, adoption_select_all)}</button>
                                <div class="space-y-2">
                                    <For each=move || summaries.get() key=|l| (l.id.clone(), l.revision) children=move |list| {
                                        let id = StoredValue::new(list.id);
                                        let revision = list.revision.to_string();
                                        let damaged = list.error.is_some();
                                        let name = if list.name.is_empty() { t_string!(i18n, adoption_damaged).to_string() } else { list.name };
                                        let label_name = name.clone();
                                        view! {
                                            <div class="rounded-lg border p-3 space-y-1">
                                                <label class="flex items-center gap-2">
                                                    <input type="checkbox" aria-label=move || t_string!(i18n, adoption_select_label, name = label_name.clone()).to_string()
                                                        prop:checked=move || selected.with(|s| s.contains(&id.get_value()))
                                                        disabled=move || damaged || pending.get() || matches!(results.get().get(&id.get_value()), Some(Ok(_)))
                                                        on:change=move |ev| selected.update(|selection| {
                                                            if event_target_checked(&ev) { selection.insert(id.get_value()); } else { selection.remove(&id.get_value()); }
                                                        }) />
                                                    <span>{name}</span>
                                                </label>
                                                {list.error.map(|e| view! { <p role="alert">{e}</p> })}
                                                {move || results.get().get(&id.get_value()).cloned().map(|result| match result {
                                                    Ok(response) => view! {
                                                        <p role="status">{if response.source_revision != revision {
                                                            t_string!(i18n, adoption_older).to_string()
                                                        } else { t_string!(i18n, adoption_done).to_string() }}</p>
                                                        <p class="text-sm">{t!(i18n, adoption_newer_guidance)}</p>
                                                        <a class="text-[color:var(--link-color)] underline" href=format!("/list/device/{}?labs=lists-sync", id.get_value())>{t!(i18n, adoption_open_device)}</a>
                                                        <a class="text-[color:var(--link-color)] underline" href=format!("/list/{}?labs=lists-sync", response.list_id)>{t!(i18n, adoption_open_account)}</a>
                                                    }.into_any(),
                                                    Err(error) => view! { <p role="alert" class="text-red-400">{t_string!(i18n, adoption_retry, error = error).to_string()}</p> }.into_any(),
                                                })}
                                            </div>
                                        }
                                    } />
                                </div>
                                <button class="btn-primary" data-testid="device-lists-adopt-selected" on:click=add_selected
                                    disabled=move || pending.get() || scope.get().is_none() || selected.get().is_empty()>
                                    {move || if pending.get() { t_string!(i18n, adoption_adding_many).to_string() } else { t_string!(i18n, adoption_add_selected, count = selected.get().len()).to_string() }}
                                </button>
                            }.into_any(),
                            _ => view! {
                                <a class="btn-secondary" rel="external" href="/login?next=/list%3Flabs%3Dlists-sync">{t!(i18n, adoption_sign_in_many)}</a>
                            }.into_any(),
                        }}
                    </Suspense>
                </details>
            </Show>
        }
    }

    #[component]
    pub fn DeviceListAdoption(handle: GuestListHandle) -> impl IntoView {
        let i18n = use_i18n();
        let handle = StoredValue::new_local(handle);
        let login = Resource::new(|| (), |_| get_login());
        let (global, _) = get_price_zone();
        let (scope, set_scope) = signal(
            handle
                .with_value(|h| h.meta().scope)
                .or_else(|| global.get_untracked().map(Into::into)),
        );
        let pending = RwSignal::new(false);
        let error = RwSignal::new(String::new());
        let accepted = RwSignal::new(None::<AdoptGuestListResponse>);
        Effect::new(move |_| {
            let owner = login
                .get()
                .and_then(Result::ok)
                .and_then(|u| i64::try_from(u.id).ok());
            let saved = owner.and_then(|owner| {
                let id = handle.with_value(|h| h.id());
                let text = receipt(
                    &format!("ultros:device-adoption:v1:{owner}:{id}:receipt"),
                    "",
                )
                .ok()?;
                let response: AdoptGuestListResponse = serde_json::from_str(&text).ok()?;
                (response.owner == owner && response.device_list_id == id).then_some(response)
            });
            accepted.set(saved);
        });
        let on_adopt = move |_| {
            if pending.get_untracked() {
                return;
            }
            let Some(Ok(user)) = login.get_untracked() else {
                return;
            };
            let Some(wdr_filter) = scope.get_untracked() else {
                return;
            };
            let h = handle.get_value();
            pending.set(true);
            error.set(String::new());
            leptos::task::spawn_local(async move {
                let result = transfer(i18n, &h, user.id, wdr_filter).await;
                match result {
                    Ok(response) => {
                        let _ = accepted.try_set(Some(response));
                    }
                    Err(message) => {
                        let _ = error.try_set(message);
                    }
                }
                let _ = pending.try_set(false);
                if !login.is_disposed() {
                    login.refetch();
                }
            });
        };
        view! {
            <section class="panel rounded-xl p-4 space-y-3" aria-label=move || t_string!(i18n, adoption_section_label).to_string()>
                <p class="text-sm">{t!(i18n, adoption_intro_one)}</p>
                <Suspense fallback=move || view! { <p>{t!(i18n, adoption_checking)}</p> }>
                    {move || match login.get() {
                        Some(Ok(user)) => view! {
                            <p>{t_string!(i18n, adoption_account, name = user.username).to_string()}</p>
                            <WorldPicker current_world=scope.into() set_current_world=set_scope.into() />
                            <button class="btn-primary" data-testid="device-list-adopt"
                                disabled=move || pending.get() || scope.get().is_none() || accepted.get().is_some()
                                on:click=on_adopt>
                                {move || if pending.get() { t_string!(i18n, adoption_adding).to_string() } else { t_string!(i18n, adoption_add_one).to_string() }}
                            </button>
                        }.into_any(),
                        _ => view! {
                            <a class="btn-secondary" rel="external"
                                href=handle.with_value(|h| format!("/login?next=/list/device/{}%3Flabs%3Dlists-sync", h.id()))>
                                {t!(i18n, adoption_sign_in_one)}
                            </a>
                        }.into_any(),
                    }}
                </Suspense>
                <p class="text-sm">{t!(i18n, adoption_retry_note)}</p>
                <p role="alert" class="text-red-400">{move || error.get()}</p>
                {move || accepted.get().map(|response| {
                    let revision = handle.with_value(|h| h.revision);

                    view! {
                        <div role="status" class="space-y-2">
                            <p>{move || {
                                revision.track();
                                if !handle.with_value(|h| h.is_saved()) || handle.with_value(|h| h.storage_revision()) != response.source_revision {
                                    t_string!(i18n, adoption_older_one).to_string()
                                } else {
                                    t_string!(i18n, adoption_done_one).to_string()
                                }
                            }}</p>
                            <p class="text-sm">{t!(i18n, adoption_newer_guidance)}</p>
                            <a class="btn-primary" href=format!("/list/{}?labs=lists-sync", response.list_id)>{t!(i18n, adoption_open_account)}</a>
                        </div>
                    }
                })}
            </section>
        }
    }
}
