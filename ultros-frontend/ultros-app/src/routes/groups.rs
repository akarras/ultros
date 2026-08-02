use crate::api::{
    add_group_member, create_group, create_group_from_guild, delete_group, get_group_members,
    get_groups, get_login, list_manageable_discord_guilds, remove_group_member,
};
use crate::components::icon::Icon;
use crate::components::loading::Loading;
use crate::components::meta::{MetaDescription, MetaRobotsNoIndex, MetaTitle};
use crate::components::tool_help::ActionableEmptyState;
use crate::global_state::toasts::use_toast;
use crate::i18n::*;
use icondata as i;
use leptos::either::Either;
use leptos::prelude::*;
use leptos_router::components::A;
use ultros_api_types::user::group::{CreateGroup, UserGroup};

/// Which creation panel is open, if any. The two are mutually exclusive so
/// opening one closes the other.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CreatePanel {
    Closed,
    Manual,
    Discord,
}

#[component]
pub fn Groups() -> impl IntoView {
    let i18n = use_i18n();
    let toasts = use_toast();
    let create_group_action = Action::new(move |group: &CreateGroup| create_group(group.clone()));
    let delete_group_action = Action::new(move |id: &i32| delete_group(*id));
    let create_from_guild_action =
        Action::new(move |guild_id: &i64| create_group_from_guild(*guild_id));

    Effect::new(move |_| {
        if let (Some(res), Some(toasts)) = (create_group_action.value().get(), toasts) {
            match res {
                Ok(_) => toasts.success(t_string!(i18n, groups_group_created)),
                Err(e) => toasts.error(format!("Failed to create group: {e}")),
            }
        }
    });

    Effect::new(move |_| {
        if let (Some(res), Some(toasts)) = (delete_group_action.value().get(), toasts) {
            match res {
                Ok(_) => toasts.success(t_string!(i18n, groups_group_deleted)),
                Err(e) => toasts.error(format!("Failed to delete group: {e}")),
            }
        }
    });

    Effect::new(move |_| {
        if let (Some(res), Some(toasts)) = (create_from_guild_action.value().get(), toasts) {
            match res {
                Ok(_) => toasts.success(t_string!(i18n, groups_group_created_from_guild)),
                Err(e) => toasts.error(format!("Failed to create group from Discord server: {e}")),
            }
        }
    });

    let groups_resource = Resource::new(
        move || {
            (
                create_group_action.version().get(),
                delete_group_action.version().get(),
                create_from_guild_action.version().get(),
            )
        },
        move |_| get_groups(),
    );

    let (panel, set_panel) = signal(CreatePanel::Closed);
    let (new_group_name, set_new_group_name) = signal(String::new());
    let user_resource = Resource::new(|| {}, |_| async move { get_login().await.ok() });

    // Gated on the picker being open: resolving this walks the bot's guilds and
    // makes live Discord calls, which is far too expensive to do on every visit
    // to the groups page. The `false` case also means SSR never calls Discord.
    let guilds_resource = Resource::new(
        move || {
            (
                panel.get() == CreatePanel::Discord,
                create_from_guild_action.version().get(),
            )
        },
        move |(open, _)| async move {
            if !open {
                return Ok(Vec::new());
            }
            list_manageable_discord_guilds().await
        },
    );

    let toggle_panel = move |target: CreatePanel| {
        set_panel(if panel() == target {
            CreatePanel::Closed
        } else {
            target
        });
    };

    view! {
        <MetaTitle title=move || t_string!(i18n, groups_meta_title).to_string() />
        <MetaDescription text=move || t_string!(i18n, groups_meta_desc).to_string() />
        <MetaRobotsNoIndex />

        <div class="flex flex-col gap-4">
            <Suspense fallback=move || view! { <Loading /> }>
                {move || match user_resource.get() {
                    None => view! { <Loading /> }.into_any(),
                    Some(None) => {
                        view! {
                            <ActionableEmptyState
                                title=t_string!(i18n, groups_empty_title).to_string()
                                body=t_string!(i18n, groups_empty_body).to_string()
                                action_href="/login?next=/groups"
                                action_label=t_string!(i18n, sign_in_discord).to_string()
                                action_external=true
                            />
                        }.into_any()
                    }
                    Some(Some(_)) => {
                        view! {
                            <div class="flex items-center gap-2 md:gap-3">
                                <A exact=true attr:class="nav-link" href="/groups">
                                    <Icon height="1.25em" width="1.25em" icon=i::BiGroupSolid />
                                    <span>{t!(i18n, groups)}</span>
                                </A>
                            </div>

                            <div class="flex flex-col md:flex-row justify-between items-start md:items-center gap-4">
                                <h1 class="text-3xl font-bold text-[color:var(--brand-fg)]">{t!(i18n, groups_page_heading)}</h1>
                                <div class="flex flex-wrap gap-2">
                                    <button class="btn-secondary" on:click=move |_| toggle_panel(CreatePanel::Discord)>
                                        <Icon icon=if panel() == CreatePanel::Discord { i::AiCloseOutlined } else { i::BsDiscord } />
                                        {move || if panel() == CreatePanel::Discord { Either::Left(t!(i18n, cancel)) } else { Either::Right(t!(i18n, groups_create_from_discord)) }}
                                    </button>
                                    <button class="btn-primary" on:click=move |_| toggle_panel(CreatePanel::Manual)>
                                        <Icon icon=if panel() == CreatePanel::Manual { i::AiCloseOutlined } else { i::BiPlusRegular } />
                                        {move || if panel() == CreatePanel::Manual { Either::Left(t!(i18n, cancel)) } else { Either::Right(t!(i18n, groups_create_group)) }}
                                    </button>
                                </div>
                            </div>

                            <Show when=move || panel() == CreatePanel::Discord>
                                <div class="panel p-6 rounded-xl animate-fade-in relative z-10 flex flex-col gap-4">
                                    <div class="flex flex-col gap-1">
                                        <h3 class="text-lg font-bold">{t!(i18n, groups_create_from_discord)}</h3>
                                        <p class="text-sm text-gray-400">{t!(i18n, groups_discord_picker_desc)}</p>
                                    </div>
                                    <Suspense fallback=move || view! { <Loading /> }>
                                        {move || {
                                            guilds_resource.get().map(|res| {
                                                match res {
                                                    Ok(guilds) if guilds.is_empty() => {
                                                        view! {
                                                            <p class="text-sm text-gray-400">{t!(i18n, groups_discord_no_guilds)}</p>
                                                        }.into_any()
                                                    }
                                                    Ok(guilds) => {
                                                        view! {
                                                            <div class="grid gap-2 sm:grid-cols-2">
                                                                {guilds.into_iter().map(|guild| {
                                                                    let guild_id = guild.id;
                                                                    let taken = guild.existing_group_id.is_some();
                                                                    let icon_url = guild.icon_url.clone();
                                                                    let guild_name = guild.name.clone();
                                                                    view! {
                                                                        <button
                                                                            type="button"
                                                                            class="flex items-center gap-2 rounded border border-[color:var(--color-outline)] p-2 text-left hover:bg-[color:var(--color-background-panel)] disabled:opacity-50"
                                                                            disabled=taken
                                                                            on:click=move |_| {
                                                                                create_from_guild_action.dispatch(guild_id);
                                                                                set_panel(CreatePanel::Closed);
                                                                            }
                                                                        >
                                                                            <GuildIcon icon_url=icon_url name=guild_name />
                                                                            <span class="min-w-0 flex-1 truncate font-medium">{guild.name}</span>
                                                                            {taken.then(|| view! {
                                                                                <span class="shrink-0 text-[10px] uppercase tracking-wider text-gray-400">
                                                                                    {t!(i18n, groups_discord_guild_taken)}
                                                                                </span>
                                                                            })}
                                                                        </button>
                                                                    }
                                                                }).collect_view()}
                                                            </div>
                                                        }.into_any()
                                                    }
                                                    Err(e) => {
                                                        view! {
                                                            <div class="alert alert-error">
                                                                {move || t!(i18n, groups_discord_error, error = e.to_string())}
                                                            </div>
                                                        }.into_any()
                                                    }
                                                }
                                            })
                                        }}
                                    </Suspense>
                                </div>
                            </Show>

                            <Show when=move || panel() == CreatePanel::Manual>
                                <div class="panel p-6 rounded-xl animate-fade-in relative z-10">
                                    <h3 class="text-lg font-bold mb-4">{t!(i18n, groups_create_group)}</h3>
                                    <div class="flex flex-col gap-4">
                                        <div class="flex flex-col gap-1">
                                            <label for="new-group-name" class="label font-semibold">{t!(i18n, list_name)}</label>
                                            <input
                                                class="input w-full"
                                                id="new-group-name"
                                                placeholder=t_string!(i18n, groups_new_group_placeholder)
                                                prop:value=new_group_name
                                                on:input=move |input| set_new_group_name(event_target_value(&input))
                                            />
                                        </div>
                                        <div class="flex justify-end">
                                            <button
                                                prop:disabled=move || new_group_name().is_empty()
                                                class="btn-primary"
                                                on:click=move |_| {
                                                    create_group_action.dispatch(CreateGroup { name: new_group_name() });
                                                    set_new_group_name(String::new());
                                                    set_panel(CreatePanel::Closed);
                                                }
                                            >
                                                <Icon icon=i::BiSaveSolid /> {t!(i18n, save)}
                                            </button>
                                        </div>
                                    </div>
                                </div>
                            </Show>

                            <Suspense fallback=move || view! { <Loading /> }>
                                {move || {
                                    groups_resource.get().map(|res| {
                                        match res {
                                            Ok(groups) => {
                                                if groups.is_empty() {
                                                    view! {
                                                        <div class="flex flex-col items-center justify-center py-12 text-gray-400">
                                                            <Icon icon=i::BiGroupSolid width="4em" height="4em" attr:class="mb-4 opacity-50"/>
                                                            <h3 class="text-xl font-semibold">{t!(i18n, groups_no_groups_found)}</h3>
                                                        </div>
                                                    }.into_any()
                                                } else {
                                                    view! {
                                                        <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                                                            <For
                                                                each=move || groups.clone()
                                                                key=move |group| group.id
                                                                children=move |group| {
                                                                    view! {
                                                                        <GroupCard
                                                                            group=group
                                                                            delete_group_action=delete_group_action
                                                                            user_id=Signal::derive(move || user_resource.get().flatten().map(|u| u.id))
                                                                        />
                                                                    }
                                                                }
                                                            />
                                                        </div>
                                                    }.into_any()
                                                }
                                            }
                                            Err(e) => {
                                                view! {
                                                    <div class="alert alert-error">
                                                        {move || t!(i18n, groups_error_loading, error = e.to_string())}
                                                    </div>
                                                }.into_any()
                                            }
                                        }
                                    })
                                }}
                            </Suspense>
                        }.into_any()
                    }
                }}
            </Suspense>
        </div>
    }
}

/// Guild avatar with a first-letter fallback, matching the endpoint picker.
/// The stored icon URL is a snapshot and can 404 if the guild changes its icon,
/// so `on:error` falls back rather than leaving a broken image.
#[component]
fn GuildIcon(icon_url: Option<String>, name: String) -> impl IntoView {
    let initial = name.chars().next().unwrap_or('?').to_string();
    let (failed, set_failed) = signal(false);
    move || {
        let initial = initial.clone();
        match icon_url.clone().filter(|_| !failed()) {
            Some(url) => Either::Left(view! {
                <img
                    src=url
                    class="h-8 w-8 shrink-0 rounded object-cover"
                    alt=""
                    on:error=move |_| set_failed(true)
                />
            }),
            None => Either::Right(view! {
                <span class="flex h-8 w-8 shrink-0 items-center justify-center rounded bg-[color:var(--color-background-panel)] text-xs font-bold">
                    {initial}
                </span>
            }),
        }
    }
}

#[component]
fn GroupCard(
    group: UserGroup,
    delete_group_action: Action<i32, Result<(), crate::error::AppError>>,
    user_id: Signal<Option<u64>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let toasts = use_toast();
    let (confirm_delete, set_confirm_delete) = signal(false);
    let add_member_action =
        Action::new(move |(group_id, user_id): &(i32, u64)| add_group_member(*group_id, *user_id));
    let remove_member_action = Action::new(move |(group_id, user_id): &(i32, u64)| {
        remove_group_member(*group_id, *user_id)
    });

    Effect::new(move |_| {
        if let (Some(res), Some(toasts)) = (add_member_action.value().get(), toasts) {
            match res {
                Ok(_) => toasts.success(t_string!(i18n, groups_member_added)),
                Err(e) => toasts.error(format!("Failed to add member: {e}")),
            }
        }
    });

    Effect::new(move |_| {
        if let (Some(res), Some(toasts)) = (remove_member_action.value().get(), toasts) {
            match res {
                Ok(_) => toasts.success(t_string!(i18n, groups_member_removed)),
                Err(e) => toasts.error(format!("Failed to remove member: {e}")),
            }
        }
    });

    let members_resource = Resource::new(
        move || {
            (
                group.id,
                add_member_action.version().get(),
                remove_member_action.version().get(),
            )
        },
        move |(group_id, _, _)| get_group_members(group_id),
    );

    let (new_member_id, set_new_member_id) = signal(String::new());
    let group_id = group.id;
    let group_name = group.name.clone();
    let guild_icon_url = group.guild_icon_url.clone();
    let is_guild_linked = group.guild_id.is_some();

    view! {
        <div class="panel p-4 rounded-xl flex flex-col gap-4">
            <div class="flex justify-between items-start gap-2">
                <div class="flex items-center gap-2 overflow-hidden">
                    {is_guild_linked.then(|| view! {
                        <GuildIcon icon_url=guild_icon_url name=group_name.clone() />
                    })}
                    <div class="flex flex-col gap-1 overflow-hidden">
                        <span class="text-xl font-bold truncate text-[color:var(--brand-fg)]">
                            {group_name.clone()}
                        </span>
                        {is_guild_linked.then(|| view! {
                            <span class="flex items-center gap-1 text-[10px] uppercase tracking-wider text-gray-400">
                                <Icon icon=i::BsDiscord />
                                {t!(i18n, groups_discord_linked)}
                            </span>
                        })}
                    </div>
                </div>
                <Show when=move || user_id().map(|uid| uid as i64 == group.owner_id).unwrap_or(false)>
                    <button
                        class=move || if confirm_delete() { "btn-danger btn-sm" } else { "btn-ghost btn-sm text-gray-400 hover:text-white" }
                        aria-label=move || if confirm_delete() { t_string!(i18n, groups_delete_group_confirm).to_string() } else { t_string!(i18n, groups_delete_group).to_string() }
                        on:click=move |_| {
                            if confirm_delete() {
                                delete_group_action.dispatch(group.id);
                            } else {
                                set_confirm_delete(true);
                            }
                        }
                    >
                        <Icon icon=if confirm_delete() { i::BiTrashSolid } else { i::BiTrashRegular } />
                        {move || confirm_delete().then_some(t!(i18n, groups_delete_group_confirm))}
                    </button>
                </Show>
            </div>

            <div class="flex flex-col gap-2">
                <h4 class="text-sm font-semibold text-gray-400 uppercase tracking-wider">{t!(i18n, groups_members_heading)}</h4>
                <Suspense fallback=move || view! { <div class="animate-pulse h-8 bg-gray-700/50 rounded" /> }>
                    {move || {
                        members_resource.get().map(|res| {
                            match res {
                                Ok(members) => {
                                    view! {
                                        <div class="flex flex-col gap-1">
                                            <For
                                                each=move || members.clone()
                                                key=move |member| member.user_id
                                                children=move |member| {
                                                    let member_id = member.user_id;
                                                    let is_owner = group.owner_id == member_id;
                                                    view! {
                                                        <div class="flex justify-between items-center p-2 rounded bg-black/20 group">
                                                            <div class="flex items-center gap-2">
                                                                <span class="text-sm">{member.username}</span>
                                                                {is_owner.then(|| view! {
                                                                    <span class="text-[10px] px-1.5 py-0.5 rounded border border-brand-500/50 text-brand-300 font-bold uppercase">"Owner"</span>
                                                                })}
                                                            </div>
                                                            <Show when=move || {
                                                                !is_owner && user_id().map(|uid| uid as i64 == group.owner_id).unwrap_or(false)
                                                            }>
                                                                <button
                                                                    class="opacity-0 group-hover:opacity-100 btn-ghost btn-xs text-red-400 hover:text-red-300"
                                                                    aria-label=move || t_string!(i18n, groups_remove_member).to_string()
                                                                    on:click=move |_| {
                                                                        remove_member_action
                                                                            .dispatch((group.id, member_id as u64));
                                                                    }
                                                                >
                                                                    <Icon icon=i::BiXRegular />
                                                                </button>
                                                            </Show>
                                                        </div>
                                                    }
                                                }
                                            />
                                        </div>
                                    }.into_any()
                                }
                                Err(e) => {
                                    view! {
                                        <div class="text-xs text-red-400">
                                            {move || t!(i18n, groups_error_loading_members, error = e.to_string())}
                                        </div>
                                    }.into_any()
                                }
                            }
                        })
                    }}
                </Suspense>
            </div>

            <Show when=move || user_id().map(|uid| uid as i64 == group.owner_id).unwrap_or(false)>
                <div class="flex flex-col gap-2 pt-2 border-t border-gray-700/50">
                    <label for=format!("add-member-{}", group_id) class="text-xs font-semibold text-gray-400">{t!(i18n, groups_add_member)}</label>
                    <div class="flex gap-2">
                        <input
                            id=format!("add-member-{}", group_id)
                            class="input input-sm flex-1"
                            placeholder=t_string!(i18n, groups_discord_id_placeholder)
                            prop:value=new_member_id
                            on:input=move |ev| set_new_member_id(event_target_value(&ev))
                        />
                        <button
                            class="btn-secondary btn-sm"
                            aria-label=move || t_string!(i18n, groups_add_member).to_string()
                            prop:disabled=move || new_member_id().is_empty()
                            on:click=move |_| {
                                if let Ok(uid) = new_member_id().parse::<u64>() {
                                    add_member_action.dispatch((group_id, uid));
                                    set_new_member_id(String::new());
                                }
                            }
                        >
                            <Icon icon=i::BiPlusRegular />
                        </button>
                    </div>
                </div>
            </Show>
        </div>
    }
}
