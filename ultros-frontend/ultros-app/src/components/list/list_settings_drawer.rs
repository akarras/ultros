use crate::api::{delete_list, leave_list};
use crate::components::icon::Icon;
use crate::components::list::share_list_modal::ShareListSection;
use crate::components::modal::Modal;
use crate::components::world_picker::*;
use crate::i18n::*;
use icondata as i;
use leptos::either::EitherOf3;
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;
use ultros_api_types::list::{List, ListCapabilities, ListPermission};

#[component]
pub fn ListSettingsDrawer(
    list: List,
    permission: ListPermission,
    self_user_id: Signal<Option<u64>>,
    edit_list: Action<List, Result<(), crate::error::AppError>>,
    refresh_signal: Signal<u32>,
    set_visible: WriteSignal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let caps = ListCapabilities::from(permission);
    let can_admin = caps.can_admin;
    let can_leave = caps.can_leave;
    let navigate = use_navigate();

    let list_id = list.id;
    let list_name = list.name.clone();
    let list_for_details = list.clone();
    let list_for_share = list.clone();

    let delete_action = Action::new(move |_: &()| async move { delete_list(list_id).await });
    let leave_action = Action::new(move |user_id: &u64| {
        let user_id = *user_id;
        async move { leave_list(list_id, user_id).await }
    });

    Effect::new(move |_| {
        if matches!(delete_action.value().get(), Some(Ok(_)))
            || matches!(leave_action.value().get(), Some(Ok(_)))
        {
            navigate("/list", Default::default());
        }
    });

    let (delete_confirm, set_delete_confirm) = signal(false);

    let title_string = if can_admin {
        t_string!(i18n, list_view_settings).to_string()
    } else {
        t_string!(i18n, list_view_list_options).to_string()
    };

    let (details_name, set_details_name) = signal(list_for_details.name.clone());
    let (details_world, set_details_world) = signal(Some(list_for_details.wdr_filter));

    view! {
        <Modal set_visible=set_visible max_width="max-w-5xl w-[96%] sm:w-[820px]".to_string()>
            <div class="space-y-6" data-testid="list-settings-drawer">
                <div class="pr-10">
                    <h2 class="text-3xl font-black text-[color:var(--color-text)]">{title_string.clone()}</h2>
                    <p class="text-sm text-[color:var(--color-text-muted)]">{list_name.clone()}</p>
                </div>

                {
                    let list_for_save = list_for_details.clone();
                    move || {
                        if can_admin {
                            Some(view! {
                                <section class="space-y-3" data-testid="list-settings-details">
                                    <h3 class="text-lg font-bold text-[color:var(--color-text)]">
                                        {t!(i18n, list_view_settings_details_heading)}
                                    </h3>
                                    <div class="grid gap-3 md:grid-cols-2">
                                        <div class="flex flex-col gap-1">
                                            <label class="label text-sm font-semibold">{t!(i18n, list_view_settings_rename_label)}</label>
                                            <input
                                                class="input w-full"
                                                prop:value=details_name
                                                on:input=move |ev| set_details_name(event_target_value(&ev))
                                                data-testid="drawer-rename-input"
                                            />
                                        </div>
                                        <div class="flex flex-col gap-1">
                                            <label class="label text-sm font-semibold">{t!(i18n, list_view_settings_world_label)}</label>
                                            <WorldPicker
                                                current_world=details_world.into()
                                                set_current_world=set_details_world.into()
                                            />
                                        </div>
                                    </div>
                                    <div class="flex justify-end">
                                        <button
                                            class="btn-primary"
                                            data-testid="drawer-save-details"
                                            on:click={
                                                let list_for_save = list_for_save.clone();
                                                move |_| {
                                                    let mut next = list_for_save.clone();
                                                    next.name = details_name().trim().to_string();
                                                    if let Some(w) = details_world() {
                                                        next.wdr_filter = w;
                                                    }
                                                    if !next.name.is_empty() {
                                                        edit_list.dispatch(next);
                                                    }
                                                }
                                            }
                                        >
                                            <Icon icon=i::BiSaveSolid />
                                            <span>{t!(i18n, list_view_settings_save)}</span>
                                        </button>
                                    </div>
                                </section>
                            })
                        } else {
                            None
                        }
                    }
                }

                {
                    let list_for_share = list_for_share.clone();
                    move || {
                        if can_admin {
                            Some(view! {
                                <section class="space-y-3" data-testid="list-settings-sharing">
                                    <h3 class="text-lg font-bold text-[color:var(--color-text)]">
                                        {t!(i18n, list_view_settings_sharing_heading)}
                                    </h3>
                                    <ShareListSection list=list_for_share.clone() refresh_signal=refresh_signal />
                                </section>
                            })
                        } else {
                            None
                        }
                    }
                }

                <div class="h-px bg-[color:var(--color-outline)]"></div>

                <section class="space-y-3" data-testid="list-settings-danger">
                    <h3 class="text-lg font-bold text-red-200">
                        {t!(i18n, list_view_settings_danger_zone)}
                    </h3>
                    {move || {
                        if can_admin {
                            EitherOf3::A(view! {
                                <div class="flex flex-wrap items-center gap-2">
                                    <button
                                        class=move || if delete_confirm() { "btn-danger" } else { "btn-secondary" }
                                        data-testid="list-delete-btn"
                                        on:click=move |_| {
                                            if delete_confirm() {
                                                delete_action.dispatch(());
                                            } else {
                                                set_delete_confirm(true);
                                            }
                                        }
                                    >
                                        <Icon icon=i::BiTrashSolid />
                                        <span>
                                            {move || if delete_confirm() {
                                                t_string!(i18n, list_view_delete_list_confirm).to_string()
                                            } else {
                                                t_string!(i18n, list_view_delete_list).to_string()
                                            }}
                                        </span>
                                    </button>
                                    <Show when=delete_confirm>
                                        <button
                                            class="btn-ghost"
                                            on:click=move |_| set_delete_confirm(false)
                                        >
                                            {t!(i18n, list_view_settings_cancel)}
                                        </button>
                                    </Show>
                                </div>
                            })
                        } else if can_leave {
                            EitherOf3::B(view! {
                                <button
                                    class="btn-danger"
                                    data-testid="list-leave-btn"
                                    on:click=move |_| {
                                        if let Some(uid) = self_user_id.get() {
                                            leave_action.dispatch(uid);
                                        }
                                    }
                                >
                                    <Icon icon=i::BiExitRegular />
                                    <span>{t!(i18n, leave_list)}</span>
                                </button>
                            })
                        } else {
                            EitherOf3::C(view! {
                                <button class="btn-secondary" disabled=true data-testid="list-leave-btn">
                                    <span>{t!(i18n, list_view_settings_danger_zone)}</span>
                                </button>
                            })
                        }
                    }}
                </section>
            </div>
        </Modal>
    }
}
