//! Connect a device list privately, then continue editing its online identity.
#[cfg(feature = "hydrate")]
pub use browser::{DeviceListAdoption, DeviceListSeparateUpload};

#[cfg(feature = "hydrate")]
mod browser {
    use crate::api::get_login;
    use crate::components::app_link::use_query_map_or_default;
    use crate::components::icon::Icon;
    use crate::components::world_picker::WorldPicker;
    use crate::global_state::home_world::get_price_zone;
    use crate::i18n::*;
    use crate::list_doc::guest::GuestListHandle;
    use icondata as i;
    use leptos::prelude::*;
    use ultros_api_types::list::AdoptGuestListResponse;
    use ultros_api_types::world_helper::AnySelector;

    fn legacy_receipt(owner: u64, id: &str) -> Option<AdoptGuestListResponse> {
        let text = web_sys::window()?
            .local_storage()
            .ok()??
            .get_item(&format!("ultros:device-adoption:v1:{owner}:{id}:receipt"))
            .ok()??;
        let response: AdoptGuestListResponse = serde_json::from_str(&text).ok()?;
        (u64::try_from(response.owner).ok() == Some(owner) && response.device_list_id == id)
            .then_some(response)
    }

    /// Shared state for the header action and the secondary upload control.
    #[derive(Clone, Copy)]
    struct Adoption {
        handle: StoredValue<GuestListHandle, LocalStorage>,
        login: Resource<crate::error::AppResult<ultros_api_types::user::UserData>>,
        scope: ReadSignal<Option<AnySelector>>,
        set_scope: WriteSignal<Option<AnySelector>>,
        pending: RwSignal<bool>,
        error: RwSignal<String>,
        legacy: Signal<Option<AdoptGuestListResponse>>,
        connect: Callback<()>,
    }

    fn use_adoption(handle: GuestListHandle) -> Adoption {
        let handle = StoredValue::new_local(handle);
        let login = Resource::new(|| (), |_| get_login());
        let (global, _) = get_price_zone();
        let (scope, set_scope) = signal(
            handle
                .with_value(|h| h.meta().scope)
                .or_else(|| global.get_untracked().map(Into::<AnySelector>::into)),
        );
        let pending = RwSignal::new(false);
        let error = RwSignal::new(String::new());
        let legacy = Signal::derive(move || {
            login
                .get()
                .and_then(Result::ok)
                .and_then(|u| legacy_receipt(u.id, &handle.with_value(|h| h.id())))
        });
        let connect = Callback::new(move |()| {
            if pending.get_untracked() {
                return;
            }
            let Some(Ok(user)) = login.get_untracked() else {
                return;
            };
            let Some(scope) = scope.get_untracked() else {
                return;
            };
            pending.set(true);
            error.set(String::new());
            let h = handle.get_value();
            leptos::task::spawn_local(async move {
                let result = h.make_online(user.id, scope).await;
                match result {
                    // DeviceEditor alone owns the handoff, including edits that
                    // arrive while its final session check is in flight.
                    Ok(()) => {}
                    Err(e) => {
                        let _ = error.try_set(e);
                    }
                }
                let _ = pending.try_set(false);
            });
        });
        Adoption {
            handle,
            login,
            scope,
            set_scope,
            pending,
            error,
            legacy,
            connect,
        }
    }

    /// The header's single storage action: make a device-only list online,
    /// or open the online copy a list already has. A list is presented once;
    /// the device bytes stay as a backup behind the storage menu.
    #[component]
    pub fn DeviceListAdoption(
        handle: GuestListHandle,
        /// Sign-in return target that keeps the editor's travel and Shop
        /// settings; defaults to the plain device URL.
        #[prop(optional)]
        continuation: Option<Signal<String>>,
    ) -> impl IntoView {
        let i18n = use_i18n();
        let Adoption {
            handle,
            login,
            scope,
            set_scope,
            pending,
            error,
            legacy,
            connect,
        } = use_adoption(handle);
        let started = RwSignal::new(false);
        let query = use_query_map_or_default();
        Effect::new(move |_| {
            let resume = query.with(|q| q.get("make_online").as_deref() == Some("1"));
            if resume
                && !started.get_untracked()
                && login.get().is_some_and(|u| u.is_ok())
                && legacy.get().is_none()
                && scope.get().is_some()
            {
                started.set(true);
                connect.run(());
            }
        });
        view! {
            <div class="flex flex-wrap items-center gap-2" data-testid="list-online-controls">
                <Suspense fallback=move || view! { <span class="text-sm">{t!(i18n, adoption_checking)}</span> }>
                    {move || match login.get() {
                        Some(Ok(user)) => {
                            if let Some(receipt) = legacy.get() {
                                return view! {
                                    <button class="btn-secondary inline-flex items-center gap-2" data-testid="device-list-open-online" disabled=move || pending.get() on:click=move |_| {
                                        let h = handle.get_value(); pending.set(true);
                                        leptos::task::spawn_local(async move {
                                            match h.continue_legacy(user.id, receipt.list_id).await {
                                                // DeviceEditor protects drafts during
                                                // explicit legacy continuation too.
                                                Ok(()) => {}
                                                Err(e) => { let _ = error.try_set(e); }
                                            }
                                            let _ = pending.try_set(false);
                                        });
                                    }><Icon icon=i::BsCloudCheck aria_hidden=true />{t!(i18n, online_open_copy)}</button>
                                }.into_any();
                            }
                            view! {
                                <Show when=move || scope.get().is_none()><WorldPicker current_world=scope.into() set_current_world=set_scope.into() /></Show>
                                <button class="btn-secondary inline-flex items-center gap-2" data-testid="device-list-adopt" disabled=move || pending.get() || scope.get().is_none() on:click=move |_| connect.run(())>
                                    <Icon icon=i::BsCloudArrowUp aria_hidden=true />
                                    {move || if pending.get() { t_string!(i18n, online_connecting).to_string() } else { t_string!(i18n, online_make).to_string() }}
                                </button>
                            }.into_any()
                        },
                        _ => view! {
                            <a class="btn-secondary inline-flex items-center gap-2" data-testid="device-list-make-online-sign-in" rel="external" href=move || {
                                let next = continuation.map(|next| next.get()).unwrap_or_else(|| handle.with_value(|h| format!("/list/device/{}?labs=lists-sync&make_online=1", h.id())));
                                format!("/login?next={}", String::from(js_sys::encode_uri_component(&next)))
                            }><Icon icon=i::BsCloudArrowUp aria_hidden=true />{t!(i18n, online_make)}</a>
                        }.into_any(),
                    }}
                </Suspense>
                <Show when=move || pending.get()><p class="basis-full text-sm" role="status">{t!(i18n, online_private_note)}</p></Show>
                <Show when=move || !error.get().is_empty()><p role="alert" class="basis-full text-sm text-red-400">{t!(i18n, online_retry_note)}" "{move || error.get()}</p></Show>
            </div>
        }
    }

    /// Secondary path, shown in the storage menu only when the list already
    /// has an online copy: upload this device version as a new online list
    /// instead of continuing the existing one.
    #[component]
    pub fn DeviceListSeparateUpload(
        handle: GuestListHandle,
        /// Runs as the upload starts, so the caller can close the storage
        /// menu: the editor defers its online handoff while a dialog is open.
        #[prop(optional)]
        on_connect: Option<Callback<()>>,
    ) -> impl IntoView {
        let i18n = use_i18n();
        let Adoption {
            scope,
            set_scope,
            pending,
            error,
            legacy,
            connect,
            ..
        } = use_adoption(handle);
        view! {
            <Show when=move || legacy.get().is_some()>
                <div class="space-y-2" data-testid="device-list-separate">
                    <p class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, online_upload_separate_hint)}</p>
                    <div class="flex flex-wrap items-center gap-2">
                        <Show when=move || scope.get().is_none()><WorldPicker current_world=scope.into() set_current_world=set_scope.into() /></Show>
                        <button class="btn-secondary inline-flex items-center gap-2" data-testid="device-list-upload-separate" disabled=move || pending.get() || scope.get().is_none() on:click=move |_| {
                            connect.run(());
                            if let Some(on_connect) = on_connect {
                                on_connect.run(());
                            }
                        }>
                            <Icon icon=i::BsCloudPlus aria_hidden=true />
                            {move || if pending.get() { t_string!(i18n, online_connecting).to_string() } else { t_string!(i18n, online_upload_separate).to_string() }}
                        </button>
                    </div>
                    <Show when=move || !error.get().is_empty()><p role="alert" class="text-sm text-red-400">{t!(i18n, online_retry_note)}" "{move || error.get()}</p></Show>
                </div>
            </Show>
        }
    }
}
