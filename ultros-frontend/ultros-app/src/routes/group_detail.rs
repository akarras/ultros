//! `/groups/:id` — everything about one group in one place.
//!
//! The summary grid at `/groups` is now only a set of links; delete, leave,
//! invites, the member list and the whole of roles live here.
//!
//! # SSR safety
//!
//! Every timer on this page (the member picker's debounce, the post-sync
//! poll) is started from inside an `Effect`, whose body never runs under
//! `leptos/ssr`. Nothing here reaches for `StoredValue::new_local` or a
//! `SendWrapper`: the only non-reactive state is a plain `u32` generation
//! counter, which is `Send + Sync` and so lives happily in a normal
//! `StoredValue` that can be disposed on any tokio worker thread. See
//! `crate::routes::retainer_live` for the failure mode this avoids.

use chrono::{DateTime, Utc};
use gloo_timers::future::TimeoutFuture;
use icondata as i;
use leptos::either::Either;
use leptos::prelude::*;
use leptos_router::hooks::{use_navigate, use_params_map};
use std::collections::HashMap;
use ultros_api_types::user::group::{
    DiscordGuildRole, GroupMemberSearchResult, GroupMemberSource, GroupRole, GroupRoleSource,
    GroupRoleSyncState, GroupSyncStatus, UserGroup, UserGroupMember,
};

use crate::api::{
    add_group_member, add_group_role_member, create_group_role, delete_group, delete_group_role,
    get_group_detail, get_group_discord_roles, get_group_members, get_group_role_members,
    get_login, import_group_discord_role, remove_group_member, remove_group_role_member,
    rename_group_role, search_group_member_candidates, sync_group,
};
use crate::components::app_link::AppLink;
use crate::components::icon::Icon;
use crate::components::meta::{MetaDescription, MetaRobotsNoIndex, MetaTitle};
use crate::components::relative_time::RelativeToNow;
use crate::components::skeleton::BoxSkeleton;
use crate::error::AppError;
use crate::global_state::toasts::use_toast;
use crate::i18n::*;
use crate::routes::groups::{GroupInvitePanel, GroupSourceBadge, GuildIcon};

/// Trailing debounce on the member picker, per the design: one Discord search
/// per pause in typing, not one per keystroke.
const SEARCH_DEBOUNCE_MS: u32 = 300;

/// Reconciliation runs off the request path, so an import or a "Sync now"
/// returns before anybody is in the role. These bound the poll that watches
/// `last_synced_at` move: 10 tries, 3s apart, then give up quietly and leave
/// the page showing whatever the last refetch found.
const SYNC_POLL_INTERVAL_MS: u32 = 3_000;
const SYNC_POLL_ATTEMPTS: usize = 10;

#[component]
pub fn GroupDetail() -> impl IntoView {
    let i18n = use_i18n();
    let params = use_params_map();
    let group_id = Memo::new(move |_| {
        params.with(|params| params.get("id").and_then(|id| id.parse::<i32>().ok()))
    });

    view! {
        <MetaTitle title=move || t_string!(i18n, groups_detail_meta_title).to_string() />
        <MetaDescription text=move || t_string!(i18n, groups_detail_meta_desc).to_string() />
        <MetaRobotsNoIndex />
        {move || match group_id.get() {
            Some(group_id) => Either::Left(view! { <GroupDetailPage group_id=group_id /> }),
            None => {
                Either::Right(
                    view! {
                        <div class="panel mx-auto max-w-xl rounded-xl p-6 text-center">
                            <AppLink href="/groups" attr:class="btn-secondary">
                                {t!(i18n, groups_back_to_groups_link)}
                            </AppLink>
                        </div>
                    },
                )
            }
        }}
    }
}

#[component]
fn GroupDetailPage(group_id: i32) -> impl IntoView {
    let i18n = use_i18n();
    let toasts = use_toast();
    let navigate = use_navigate();

    // One counter every mutation on the page bumps. Both resources key on it,
    // so a role rename and a member add both land without each caller having
    // to know which of the two requests it invalidated.
    let refresh = RwSignal::new(0u32);
    let bump = Callback::new(move |()| refresh.update(|version| *version += 1));

    let user_resource = Resource::new(|| {}, |_| async move { get_login().await.ok() });
    let viewer_id =
        Signal::derive(move || user_resource.get().flatten().map(|user| user.id as i64));

    let detail = Resource::new(
        move || (group_id, refresh.get()),
        move |(group_id, _)| get_group_detail(group_id),
    );
    let members = Resource::new(
        move || (group_id, refresh.get()),
        move |(group_id, _)| get_group_members(group_id),
    );
    let member_list = Signal::derive(move || members.get());

    // Which role's member list is open. Held here rather than inside the row
    // so it survives the refetch that follows every mutation — the roles
    // section is re-rendered wholesale when the group's detail lands again.
    let expanded_role = RwSignal::new(None::<i32>);

    let sync_action = Action::new(move |()| sync_group(group_id));
    let delete_action = Action::new(move |()| delete_group(group_id));
    let leave_action =
        Action::new(move |user_id: &i64| remove_group_member(group_id, *user_id as u64));

    // The most recent successful sync across the group's synced roles: what
    // the header prints, and the baseline the post-sync poll watches move.
    let latest_synced = Signal::derive(move || {
        detail
            .get()
            .and_then(|result| result.ok())
            .and_then(|detail| detail.roles.iter().filter_map(|r| r.last_synced_at).max())
    });

    let polling = RwSignal::new(false);
    // Plain `StoredValue`: a `u32` is `Send + Sync`, so disposing this owner
    // on an SSR worker thread is harmless. Never `new_local` here.
    let poll_generation = StoredValue::new(0u32);
    // Only ever called from an `Effect`, so the timer is client-only.
    let start_sync_poll = Callback::new(move |baseline: Option<DateTime<Utc>>| {
        let generation = poll_generation.get_value().wrapping_add(1);
        poll_generation.set_value(generation);
        polling.set(true);
        leptos::task::spawn_local(async move {
            for _ in 0..SYNC_POLL_ATTEMPTS {
                TimeoutFuture::new(SYNC_POLL_INTERVAL_MS).await;
                // Past the await the component can be gone (navigated away),
                // which disposes these signals: every read below is a `try_`.
                if poll_generation.try_get_value() != Some(generation) {
                    return; // a newer sync took over, or we were disposed
                }
                match latest_synced.try_get_untracked() {
                    None => return,
                    // `None < Some(_)`, so this covers both "it was never
                    // synced and now is" and "it synced again".
                    Some(current) if current > baseline => break,
                    Some(_) => {}
                }
                if refresh.try_update(|version| *version += 1).is_none() {
                    return;
                }
            }
            let _ = polling.try_set(false);
        });
    });

    Effect::new(move |_| {
        let (Some(result), Some(toasts)) = (sync_action.value().get(), toasts) else {
            return;
        };
        match result {
            Ok(response) => match response.status {
                GroupSyncStatus::Ran => {
                    toasts.info(t_string!(i18n, groups_sync_started));
                    start_sync_poll.run(response.last_synced_at);
                }
                GroupSyncStatus::RecentlySynced => {
                    toasts.warning(t_string!(i18n, groups_sync_recently))
                }
            },
            Err(e) => toasts.error(t_string!(i18n, groups_sync_error, error = e.to_string())),
        }
    });

    Effect::new({
        let navigate = navigate.clone();
        move |_| {
            let Some(result) = delete_action.value().get() else {
                return;
            };
            match (result, toasts) {
                (Ok(()), toasts) => {
                    if let Some(toasts) = toasts {
                        toasts.success(t_string!(i18n, groups_group_deleted));
                    }
                    navigate("/groups", Default::default());
                }
                (Err(e), Some(toasts)) => {
                    toasts.error(t_string!(i18n, groups_detail_error, error = e.to_string()))
                }
                (Err(_), None) => {}
            }
        }
    });

    Effect::new({
        let navigate = navigate.clone();
        move |_| {
            let Some(result) = leave_action.value().get() else {
                return;
            };
            match (result, toasts) {
                (Ok(()), toasts) => {
                    if let Some(toasts) = toasts {
                        toasts.success(t_string!(i18n, groups_left_group));
                    }
                    navigate("/groups", Default::default());
                }
                (Err(e), Some(toasts)) => toasts.error(t_string!(
                    i18n,
                    groups_member_remove_error,
                    error = e.to_string()
                )),
                (Err(_), None) => {}
            }
        }
    });

    view! {
        <div class="flex flex-col gap-4">
            <div class="flex items-center gap-2 md:gap-3">
                <AppLink exact=true attr:class="nav-link" href="/groups">
                    <Icon height="1.25em" width="1.25em" icon=i::BiGroupSolid />
                    <span>{t!(i18n, groups)}</span>
                </AppLink>
            </div>

            <Suspense fallback=move || view! { <BoxSkeleton rows=4 /> }>
                {move || {
                    detail
                        .get()
                        .map(|result| match result {
                            Ok(detail) => {
                                let group = detail.group.clone();
                                let owner_id = group.owner_id;
                                let guild_linked = group.guild_id.is_some();
                                let frozen_reason = group.frozen_reason.clone();
                                let roles = detail.roles.clone();
                                let member_count = detail.member_count;
                                let role_count = detail.roles.len();
                                let is_owner = Signal::derive(move || {
                                    viewer_id.get() == Some(owner_id)
                                });
                                let can_leave = Signal::derive(move || {
                                    let Some(viewer) = viewer_id.get() else { return false };
                                    if viewer == owner_id {
                                        return false;
                                    }
                                    // A synced member is refused by the server
                                    // — they leave by leaving the Discord role
                                    // — so the button is not offered at all.
                                    matches!(
                                        member_list.get(),
                                        Some(Ok(members)) if members.iter().any(|member| {
                                            member.user_id == viewer
                                                && member.source == GroupMemberSource::Manual
                                        })
                                    )
                                });
                                let syncing = Signal::derive(move || {
                                    sync_action.pending().get() || polling.get()
                                });
                                Either::Left(
                                    view! {
                                        <div class="panel flex flex-col gap-6 rounded-xl p-4 md:p-6">
                                            <GroupHeader
                                                group=group
                                                member_count=member_count
                                                role_count=role_count
                                                is_owner=is_owner
                                                last_synced_at=latest_synced
                                                syncing=syncing
                                                can_leave=can_leave
                                                on_sync=Callback::new(move |()| {
                                                    sync_action.dispatch(());
                                                })
                                                on_delete=Callback::new(move |()| {
                                                    delete_action.dispatch(());
                                                })
                                                on_leave=Callback::new(move |()| {
                                                    if let Some(viewer) = viewer_id.get_untracked() {
                                                        leave_action.dispatch(viewer);
                                                    }
                                                })
                                            />

                                            {frozen_reason
                                                .map(|reason| {
                                                    view! { <GroupFrozenBanner reason=reason /> }
                                                })}

                                            <GroupMembersSection
                                                group_id=group_id
                                                roles=roles.clone()
                                                members=member_list
                                                owner_id=owner_id
                                                is_owner=is_owner
                                                on_changed=bump
                                            />

                                            <GroupRolesSection
                                                group_id=group_id
                                                roles=roles
                                                is_owner=is_owner
                                                guild_linked=guild_linked
                                                expanded_role=expanded_role
                                                on_changed=bump
                                                on_import_started=start_sync_poll
                                                latest_synced=latest_synced
                                            />

                                            <Show when=move || is_owner.get()>
                                                <GroupInvitePanel group_id=group_id />
                                            </Show>
                                        </div>
                                    },
                                )
                            }
                            Err(e) => {
                                Either::Right(
                                    view! {
                                        <div class="alert alert-error">
                                            {move || t!(i18n, groups_detail_error, error = e.to_string())}
                                        </div>
                                    },
                                )
                            }
                        })
                }}
            </Suspense>
        </div>
    }
}

/// Icon, name, badge, sync state, and the owner's destructive controls.
///
/// Pure: every input is a prop and every action is a callback, so the SSR
/// render tests can drive it from a fixture group in any state.
#[component]
pub(crate) fn GroupHeader(
    group: UserGroup,
    member_count: i64,
    role_count: usize,
    #[prop(into)] is_owner: Signal<bool>,
    /// Most recent successful sync across the group's synced roles.
    #[prop(into)]
    last_synced_at: Signal<Option<DateTime<Utc>>>,
    /// True while a sync is running or being polled for.
    #[prop(into)]
    syncing: Signal<bool>,
    /// Whether this viewer is a member who may leave. A synced member cannot,
    /// so the button is hidden rather than offered and refused.
    #[prop(into)]
    can_leave: Signal<bool>,
    #[prop(into)] on_sync: Callback<()>,
    #[prop(into)] on_delete: Callback<()>,
    #[prop(into)] on_leave: Callback<()>,
) -> impl IntoView {
    let i18n = use_i18n();
    let (confirm_delete, set_confirm_delete) = signal(false);
    let name = group.name.clone();
    let icon_url = group.guild_icon_url.clone();
    let source = group.source;
    let frozen = group.frozen_reason.is_some();
    // Syncing needs a live guild link. A frozen group has neither, and
    // pressing the button would only earn a 400.
    let can_sync = group.guild_id.is_some() && !frozen;
    let guild_linked = group.guild_id.is_some();
    let role_count = role_count as i64;

    view! {
        <div class="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
            <div class="flex min-w-0 items-start gap-3">
                <GuildIcon icon_url=icon_url name=name.clone() size="h-12 w-12" />
                <div class="flex min-w-0 flex-col gap-1">
                    <h1 class="truncate text-3xl font-bold text-[color:var(--brand-fg)]">{name}</h1>
                    <div class="flex flex-wrap items-center gap-2">
                        <GroupSourceBadge source=source frozen=frozen />
                        <span class="text-sm text-[color:var(--color-text-muted)]">
                            {t!(i18n, groups_member_count, count = member_count)}
                        </span>
                        <span class="text-sm text-[color:var(--color-text-muted)]">
                            {t!(i18n, groups_role_count, count = role_count)}
                        </span>
                    </div>
                    <Show when=move || guild_linked>
                        <div class="flex items-center gap-1 text-xs text-[color:var(--color-text-muted)]">
                            {move || match last_synced_at.get() {
                                Some(timestamp) => {
                                    Either::Left(
                                        view! {
                                            <span>{t!(i18n, groups_last_synced)}</span>
                                            <RelativeToNow timestamp=timestamp.naive_utc() />
                                        },
                                    )
                                }
                                None => Either::Right(view! { <span>{t!(i18n, groups_never_synced)}</span> }),
                            }}
                        </div>
                    </Show>
                </div>
            </div>

            <div class="flex shrink-0 flex-wrap items-center gap-2">
                <Show when=move || is_owner.get() && can_sync>
                    <button
                        type="button"
                        class="btn-secondary btn-sm"
                        prop:disabled=move || syncing.get()
                        on:click=move |_| on_sync.run(())
                    >
                        <Icon icon=i::BiSyncRegular />
                        {move || {
                            if syncing.get() {
                                Either::Left(t!(i18n, groups_syncing))
                            } else {
                                Either::Right(t!(i18n, groups_sync_now))
                            }
                        }}
                    </button>
                </Show>

                <Show when=move || can_leave.get()>
                    <button
                        type="button"
                        class="btn-secondary btn-sm"
                        on:click=move |_| on_leave.run(())
                    >
                        <Icon icon=i::BiExitRegular />
                        <span>{t!(i18n, groups_leave_group)}</span>
                    </button>
                </Show>

                <Show when=move || is_owner.get()>
                    <button
                        type="button"
                        class=move || {
                            if confirm_delete.get() {
                                "btn-danger btn-sm"
                            } else {
                                "btn-ghost btn-sm text-[color:var(--color-text-muted)]"
                            }
                        }
                        aria-label=move || {
                            if confirm_delete.get() {
                                t_string!(i18n, groups_delete_group_confirm).to_string()
                            } else {
                                t_string!(i18n, groups_delete_group).to_string()
                            }
                        }
                        on:click=move |_| {
                            if confirm_delete.get_untracked() {
                                on_delete.run(());
                            } else {
                                set_confirm_delete.set(true);
                            }
                        }
                    >
                        <Icon icon=if confirm_delete.get() {
                            i::BiTrashSolid
                        } else {
                            i::BiTrashRegular
                        } />
                        {move || {
                            if confirm_delete.get() {
                                Either::Left(t!(i18n, groups_delete_group_confirm))
                            } else {
                                Either::Right(t!(i18n, groups_delete_group))
                            }
                        }}
                    </button>
                </Show>
            </div>
        </div>
    }
}

/// Shown when the bot was removed from the guild. Membership is kept, but
/// nothing updates it any more — which is the whole point of saying so here.
#[component]
pub(crate) fn GroupFrozenBanner(reason: String) -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <div
            role="status"
            class="flex gap-3 rounded-lg border border-amber-500/50 bg-amber-500/10 p-4"
        >
            <Icon icon=i::BiUnlinkRegular width="1.5em" height="1.5em" />
            <div class="flex flex-col gap-1">
                <h2 class="font-bold text-amber-200">{t!(i18n, groups_frozen_heading)}</h2>
                <p class="text-sm text-[color:var(--color-text-muted)]">
                    {t!(i18n, groups_frozen_body)}
                </p>
                // The server's own wording for why, kept verbatim: it is a
                // diagnostic, not a translated sentence.
                <p class="text-xs text-[color:var(--color-text-muted)]">{reason}</p>
            </div>
        </div>
    }
}

/// One member row's avatar. Discord avatars are not carried on the member
/// list, so this is always the initial.
#[component]
fn MemberInitial(name: String) -> impl IntoView {
    let initial = name
        .chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .to_string();
    view! {
        <span
            aria-hidden="true"
            class="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-[color:var(--color-background-panel)] text-xs font-bold"
        >
            {initial}
        </span>
    }
}

#[component]
fn GroupMembersSection(
    group_id: i32,
    roles: Vec<GroupRole>,
    #[prop(into)] members: Signal<Option<Result<Vec<UserGroupMember>, AppError>>>,
    owner_id: i64,
    #[prop(into)] is_owner: Signal<bool>,
    #[prop(into)] on_changed: Callback<()>,
) -> impl IntoView {
    let i18n = use_i18n();
    let toasts = use_toast();
    let role_names: HashMap<i32, String> = roles
        .iter()
        .map(|role| (role.id, role.name.clone()))
        .collect();
    let role_names = StoredValue::new(role_names);

    let add_member = Action::new(move |(user_id, display_name): &(i64, String)| {
        add_group_member(group_id, *user_id as u64, Some(display_name.clone()))
    });
    let remove_member =
        Action::new(move |user_id: &i64| remove_group_member(group_id, *user_id as u64));

    Effect::new(move |_| {
        let (Some(result), Some(toasts)) = (add_member.value().get(), toasts) else {
            return;
        };
        match result {
            Ok(()) => {
                toasts.success(t_string!(i18n, groups_member_added));
                on_changed.run(());
            }
            Err(e) => toasts.error(t_string!(
                i18n,
                groups_member_add_error,
                error = e.to_string()
            )),
        }
    });

    Effect::new(move |_| {
        let (Some(result), Some(toasts)) = (remove_member.value().get(), toasts) else {
            return;
        };
        match result {
            Ok(()) => {
                toasts.success(t_string!(i18n, groups_member_removed));
                on_changed.run(());
            }
            Err(e) => toasts.error(t_string!(
                i18n,
                groups_member_remove_error,
                error = e.to_string()
            )),
        }
    });

    view! {
        <section class="flex flex-col gap-3">
            <h2 class="text-sm font-semibold uppercase tracking-wider text-[color:var(--color-text-muted)]">
                {t!(i18n, groups_members_heading)}
            </h2>

            <Show when=move || is_owner.get()>
                <MemberSearchPicker
                    group_id=group_id
                    on_pick=Callback::new(move |candidate: GroupMemberSearchResult| {
                        add_member.dispatch((candidate.user_id, candidate.display_name));
                    })
                />
            </Show>

            {move || match members.get() {
                None => Either::Left(view! { <BoxSkeleton rows=3 /> }),
                Some(Ok(members)) if members.is_empty() => {
                    Either::Right(
                        view! {
                            <p class="text-sm text-[color:var(--color-text-muted)]">
                                {t!(i18n, groups_no_members)}
                            </p>
                        }
                            .into_any(),
                    )
                }
                Some(Ok(members)) => {
                    Either::Right(
                        view! {
                            <div class="flex flex-col gap-1">
                                <For
                                    each=move || members.clone()
                                    key=|member| (member.user_id, member.roles.clone())
                                    children=move |member| {
                                        let member_id = member.user_id;
                                        let is_group_owner = member_id == owner_id;
                                        let synced = member.source == GroupMemberSource::Synced;
                                        let chips = member
                                            .roles
                                            .iter()
                                            .filter_map(|role_id| {
                                                role_names.with_value(|names| names.get(role_id).cloned())
                                            })
                                            .collect::<Vec<_>>();
                                        view! {
                                            <div class="flex items-center gap-2 rounded bg-black/20 p-2">
                                                <MemberInitial name=member.username.clone() />
                                                <div class="flex min-w-0 flex-1 flex-wrap items-center gap-2">
                                                    <span class="truncate text-sm">{member.username.clone()}</span>
                                                    {is_group_owner
                                                        .then(|| {
                                                            view! {
                                                                <span class="rounded border border-brand-500/50 px-1.5 py-0.5 text-[10px] font-bold uppercase text-brand-300">
                                                                    {t!(i18n, groups_owner)}
                                                                </span>
                                                            }
                                                        })}
                                                    {chips
                                                        .into_iter()
                                                        .map(|name| {
                                                            view! {
                                                                <span class="rounded-full border border-[color:var(--color-outline)] px-2 py-0.5 text-[10px] text-[color:var(--color-text-muted)]">
                                                                    {name}
                                                                </span>
                                                            }
                                                        })
                                                        .collect_view()}
                                                </div>
                                                {if synced {
                                                    Either::Left(
                                                        view! {
                                                            <span
                                                                class="shrink-0 text-[color:var(--color-text-muted)]"
                                                                title=move || {
                                                                    t_string!(i18n, groups_managed_by_discord).to_string()
                                                                }
                                                            >
                                                                <Icon icon=i::BiLockAltSolid />
                                                                <span class="sr-only">
                                                                    {t!(i18n, groups_managed_by_discord)}
                                                                </span>
                                                            </span>
                                                        },
                                                    )
                                                } else {
                                                    Either::Right(
                                                        view! {
                                                            <Show when=move || {
                                                                is_owner.get() && !is_group_owner
                                                            }>
                                                                <button
                                                                    type="button"
                                                                    class="btn-ghost btn-xs shrink-0 text-red-400 hover:text-red-300"
                                                                    aria-label=move || {
                                                                        t_string!(i18n, groups_remove_member).to_string()
                                                                    }
                                                                    on:click=move |_| {
                                                                        remove_member.dispatch(member_id);
                                                                    }
                                                                >
                                                                    <Icon icon=i::BiXRegular />
                                                                </button>
                                                            </Show>
                                                        },
                                                    )
                                                }}
                                            </div>
                                        }
                                    }
                                />
                            </div>
                        }
                            .into_any(),
                    )
                }
                Some(Err(e)) => {
                    Either::Right(
                        view! {
                            <div class="text-xs text-red-400">
                                {move || t!(i18n, groups_error_loading_members, error = e.to_string())}
                            </div>
                        }
                            .into_any(),
                    )
                }
            }}
        </section>
    }
}

/// Trailing debounce over a text signal.
///
/// The timer is started inside an `Effect`, so it exists only on the client;
/// under SSR the returned signal simply never moves off its initial empty
/// value and the search resource is never triggered. The generation counter
/// is a plain `u32` in a plain `StoredValue` — see this module's header.
fn use_debounced(source: Signal<String>, delay_ms: u32) -> Signal<String> {
    let debounced = RwSignal::new(String::new());
    let generation = StoredValue::new(0u32);
    Effect::new(move |_| {
        let value = source.get();
        let this = generation.get_value().wrapping_add(1);
        generation.set_value(this);
        leptos::task::spawn_local(async move {
            TimeoutFuture::new(delay_ms).await;
            // Superseded by a later keystroke, or the owner is gone.
            if generation.try_get_value() != Some(this) {
                return;
            }
            let _ = debounced.try_set(value);
        });
    });
    debounced.into()
}

/// Owner's search-as-you-type picker. Backed by Discord for a guild-linked
/// group, which is why it never fires until typing pauses.
#[component]
fn MemberSearchPicker(
    group_id: i32,
    #[prop(optional)] role_id: Option<i32>,
    #[prop(into)] on_pick: Callback<GroupMemberSearchResult>,
) -> impl IntoView {
    let i18n = use_i18n();
    let (query, set_query) = signal(String::new());
    let debounced = use_debounced(query.into(), SEARCH_DEBOUNCE_MS);
    let input_id = match role_id {
        Some(role_id) => format!("group-member-search-{group_id}-role-{role_id}"),
        None => format!("group-member-search-{group_id}"),
    };
    let results = Resource::new(
        move || (group_id, debounced.get()),
        move |(group_id, query)| async move {
            let query = query.trim().to_string();
            if query.is_empty() {
                return Ok(Vec::new());
            }
            search_group_member_candidates(group_id, &query).await
        },
    );

    view! {
        <div class="flex flex-col gap-2">
            <label for=input_id.clone() class="text-xs font-semibold text-[color:var(--color-text-muted)]">
                {t!(i18n, groups_member_search_label)}
            </label>
            <input
                id=input_id
                class="input input-sm w-full"
                type="search"
                autocomplete="off"
                placeholder=t_string!(i18n, groups_member_search_placeholder)
                prop:value=query
                on:input=move |ev| set_query.set(event_target_value(&ev))
            />
            <Show when=move || !query.get().trim().is_empty()>
                <Suspense fallback=move || {
                    view! { <div class="skeleton-block skeleton-shimmer h-8 rounded"></div> }
                }>
                    {move || {
                        results
                            .get()
                            .map(|result| match result {
                                Ok(candidates) if candidates.is_empty() => {
                                    view! {
                                        <p class="text-xs text-[color:var(--color-text-muted)]">
                                            {t!(i18n, groups_member_search_empty)}
                                        </p>
                                    }
                                        .into_any()
                                }
                                Ok(candidates) => {
                                    view! {
                                        <div class="flex flex-col gap-1 rounded border border-[color:var(--color-outline)] p-1">
                                            {candidates
                                                .into_iter()
                                                .map(|candidate| {
                                                    let picked = candidate.clone();
                                                    let on_ultros = candidate.on_ultros;
                                                    view! {
                                                        <button
                                                            type="button"
                                                            class="flex items-center gap-2 rounded p-2 text-left hover:bg-[color:var(--color-background-panel)]"
                                                            on:click=move |_| {
                                                                on_pick.run(picked.clone());
                                                                set_query.set(String::new());
                                                            }
                                                        >
                                                            {match candidate.avatar_url.clone() {
                                                                Some(url) => {
                                                                    Either::Left(
                                                                        view! {
                                                                            <img
                                                                                src=url
                                                                                alt=""
                                                                                class="h-8 w-8 shrink-0 rounded-full object-cover"
                                                                            />
                                                                        },
                                                                    )
                                                                }
                                                                None => {
                                                                    Either::Right(
                                                                        view! {
                                                                            <MemberInitial name=candidate.display_name.clone() />
                                                                        },
                                                                    )
                                                                }
                                                            }}
                                                            <span class="min-w-0 flex-1 truncate text-sm">
                                                                {candidate.display_name.clone()}
                                                            </span>
                                                            {(!on_ultros)
                                                                .then(|| {
                                                                    view! {
                                                                        <span class="shrink-0 text-[10px] uppercase tracking-wider text-[color:var(--color-text-muted)]">
                                                                            {t!(i18n, groups_member_not_on_ultros)}
                                                                        </span>
                                                                    }
                                                                })}
                                                        </button>
                                                    }
                                                })
                                                .collect_view()}
                                        </div>
                                    }
                                        .into_any()
                                }
                                Err(e) => {
                                    view! {
                                        <div class="text-xs text-red-400">
                                            {move || {
                                                t!(i18n, groups_member_search_error, error = e.to_string())
                                            }}
                                        </div>
                                    }
                                        .into_any()
                                }
                            })
                    }}
                </Suspense>
            </Show>
        </div>
    }
}

/// The "Synced"/"Orphaned" marker on a role row. A manual role has neither.
#[component]
pub(crate) fn RoleSyncMarker(
    source: GroupRoleSource,
    sync_state: GroupRoleSyncState,
) -> impl IntoView {
    let i18n = use_i18n();
    if source != GroupRoleSource::DiscordRole {
        return None;
    }
    Some(match sync_state {
        GroupRoleSyncState::Synced => Either::Left(view! {
            <span class="inline-flex items-center gap-1 text-[10px] font-bold uppercase tracking-wider text-brand-300">
                <Icon icon=i::BsDiscord />
                {t!(i18n, groups_role_synced)}
            </span>
        }),
        GroupRoleSyncState::Orphaned => Either::Right(view! {
            <span
                class="inline-flex items-center gap-1 text-[10px] font-bold uppercase tracking-wider text-amber-300"
                title=move || t_string!(i18n, groups_role_orphaned_help).to_string()
            >
                <Icon icon=i::BiUnlinkRegular />
                {t!(i18n, groups_role_orphaned)}
            </span>
        }),
    })
}

/// Which owner-only panel is open above the role list, if any.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RolePanel {
    Closed,
    New,
    Import,
}

#[component]
fn GroupRolesSection(
    group_id: i32,
    roles: Vec<GroupRole>,
    #[prop(into)] is_owner: Signal<bool>,
    guild_linked: bool,
    expanded_role: RwSignal<Option<i32>>,
    #[prop(into)] on_changed: Callback<()>,
    /// Starts the page's post-import poll, given the sync timestamp as it
    /// stood before the import.
    #[prop(into)]
    on_import_started: Callback<Option<DateTime<Utc>>>,
    #[prop(into)] latest_synced: Signal<Option<DateTime<Utc>>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let toasts = use_toast();
    let (panel, set_panel) = signal(RolePanel::Closed);
    let (new_role_name, set_new_role_name) = signal(String::new());

    let create_role = Action::new(move |name: &String| create_group_role(group_id, name.clone()));
    let import_role = Action::new(move |discord_role_id: &i64| {
        import_group_discord_role(group_id, *discord_role_id)
    });
    let rename_role = Action::new(move |(role_id, name): &(i32, String)| {
        rename_group_role(group_id, *role_id, name.clone())
    });
    let delete_role = Action::new(move |role_id: &i32| delete_group_role(group_id, *role_id));

    Effect::new(move |_| {
        let (Some(result), Some(toasts)) = (create_role.value().get(), toasts) else {
            return;
        };
        match result {
            Ok(_) => {
                toasts.success(t_string!(i18n, groups_role_created));
                on_changed.run(());
            }
            Err(e) => toasts.error(t_string!(
                i18n,
                groups_role_create_error,
                error = e.to_string()
            )),
        }
    });

    Effect::new(move |_| {
        let (Some(result), Some(toasts)) = (import_role.value().get(), toasts) else {
            return;
        };
        match result {
            Ok(_) => {
                toasts.info(t_string!(i18n, groups_role_imported));
                // The role row exists but is empty until the reconcile the
                // server kicked off lands, so watch `last_synced_at` move
                // rather than leaving the page claiming nobody holds it.
                on_import_started.run(latest_synced.get_untracked());
                on_changed.run(());
            }
            Err(e) => toasts.error(t_string!(
                i18n,
                groups_role_import_failed,
                error = e.to_string()
            )),
        }
    });

    Effect::new(move |_| {
        let (Some(result), Some(toasts)) = (rename_role.value().get(), toasts) else {
            return;
        };
        match result {
            Ok(_) => {
                toasts.success(t_string!(i18n, groups_role_renamed));
                on_changed.run(());
            }
            Err(e) => toasts.error(t_string!(
                i18n,
                groups_role_rename_error,
                error = e.to_string()
            )),
        }
    });

    Effect::new(move |_| {
        let (Some(result), Some(toasts)) = (delete_role.value().get(), toasts) else {
            return;
        };
        match result {
            Ok(()) => {
                toasts.success(t_string!(i18n, groups_role_deleted));
                on_changed.run(());
            }
            Err(e) => toasts.error(t_string!(
                i18n,
                groups_role_delete_error,
                error = e.to_string()
            )),
        }
    });

    // Gated on the picker being open: resolving this makes a live Discord
    // call server-side, which must not happen on every visit — and the
    // `false` arm is what keeps SSR off Discord entirely.
    let discord_roles = Resource::new(
        move || {
            (
                group_id,
                panel.get() == RolePanel::Import,
                import_role.version().get(),
            )
        },
        move |(group_id, open, _)| async move {
            if !open {
                return Ok(Vec::new());
            }
            get_group_discord_roles(group_id).await
        },
    );

    let toggle = move |target: RolePanel| {
        set_panel.set(if panel.get_untracked() == target {
            RolePanel::Closed
        } else {
            target
        });
    };

    view! {
        <section class="flex flex-col gap-3">
            <div class="flex flex-wrap items-center justify-between gap-2">
                <h2 class="text-sm font-semibold uppercase tracking-wider text-[color:var(--color-text-muted)]">
                    {t!(i18n, groups_roles_heading)}
                </h2>
                <Show when=move || is_owner.get()>
                    <div class="flex flex-wrap gap-2">
                        <Show when=move || guild_linked>
                            <button
                                type="button"
                                class="btn-secondary btn-sm"
                                on:click=move |_| toggle(RolePanel::Import)
                            >
                                <Icon icon=i::BsDiscord />
                                <span>{t!(i18n, groups_role_import)}</span>
                            </button>
                        </Show>
                        <button
                            type="button"
                            class="btn-primary btn-sm"
                            on:click=move |_| toggle(RolePanel::New)
                        >
                            <Icon icon=i::BiPlusRegular />
                            <span>{t!(i18n, groups_role_new)}</span>
                        </button>
                    </div>
                </Show>
            </div>

            <Show when=move || panel.get() == RolePanel::New>
                <div class="flex gap-2">
                    <input
                        class="input input-sm min-w-0 flex-1"
                        aria-label=move || t_string!(i18n, groups_role_name_placeholder).to_string()
                        placeholder=t_string!(i18n, groups_role_name_placeholder)
                        prop:value=new_role_name
                        on:input=move |ev| set_new_role_name.set(event_target_value(&ev))
                    />
                    <button
                        type="button"
                        class="btn-primary btn-sm shrink-0"
                        prop:disabled=move || new_role_name.get().trim().is_empty()
                        on:click=move |_| {
                            let name = new_role_name.get_untracked().trim().to_string();
                            if name.is_empty() {
                                return;
                            }
                            create_role.dispatch(name);
                            set_new_role_name.set(String::new());
                            set_panel.set(RolePanel::Closed);
                        }
                    >
                        <Icon icon=i::BiSaveSolid />
                        <span>{t!(i18n, groups_role_create)}</span>
                    </button>
                </div>
            </Show>

            <Show when=move || panel.get() == RolePanel::Import>
                <div class="flex flex-col gap-2 rounded-lg border border-[color:var(--color-outline)] p-3">
                    <p class="text-sm text-[color:var(--color-text-muted)]">
                        {t!(i18n, groups_role_import_desc)}
                    </p>
                    <Suspense fallback=move || {
                        view! { <div class="skeleton-block skeleton-shimmer h-16 rounded"></div> }
                    }>
                        {move || {
                            discord_roles
                                .get()
                                .map(|result| match result {
                                    Ok(roles) if roles.is_empty() => {
                                        view! {
                                            <p class="text-sm text-[color:var(--color-text-muted)]">
                                                {t!(i18n, groups_role_import_empty)}
                                            </p>
                                        }
                                            .into_any()
                                    }
                                    Ok(roles) => {
                                        view! {
                                            <div class="grid gap-2 sm:grid-cols-2">
                                                {roles
                                                    .into_iter()
                                                    .map(|role: DiscordGuildRole| {
                                                        let discord_role_id = role.id;
                                                        let taken = role.existing_role_id.is_some();
                                                        let swatch = role
                                                            .color
                                                            .clone()
                                                            .unwrap_or_else(|| "currentColor".to_string());
                                                        view! {
                                                            <button
                                                                type="button"
                                                                class="flex items-center gap-2 rounded border border-[color:var(--color-outline)] p-2 text-left hover:bg-[color:var(--color-background-panel)] disabled:opacity-50"
                                                                disabled=taken
                                                                on:click=move |_| {
                                                                    import_role.dispatch(discord_role_id);
                                                                    set_panel.set(RolePanel::Closed);
                                                                }
                                                            >
                                                                <span
                                                                    aria-hidden="true"
                                                                    class="h-3 w-3 shrink-0 rounded-full"
                                                                    style=format!("background-color: {swatch}")
                                                                ></span>
                                                                <span class="min-w-0 flex-1 truncate font-medium">
                                                                    {role.name.clone()}
                                                                </span>
                                                                {taken
                                                                    .then(|| {
                                                                        view! {
                                                                            <span class="shrink-0 text-[10px] uppercase tracking-wider text-[color:var(--color-text-muted)]">
                                                                                {t!(i18n, groups_role_import_taken)}
                                                                            </span>
                                                                        }
                                                                    })}
                                                            </button>
                                                        }
                                                    })
                                                    .collect_view()}
                                            </div>
                                        }
                                            .into_any()
                                    }
                                    Err(e) => {
                                        view! {
                                            <div class="alert alert-error">
                                                {move || t!(i18n, groups_role_import_error, error = e.to_string())}
                                            </div>
                                        }
                                            .into_any()
                                    }
                                })
                        }}
                    </Suspense>
                </div>
            </Show>

            {if roles.is_empty() {
                Either::Left(
                    view! {
                        <p class="text-sm text-[color:var(--color-text-muted)]">
                            {t!(i18n, groups_role_none)}
                        </p>
                    },
                )
            } else {
                Either::Right(
                    view! {
                        <div class="flex flex-col gap-1">
                            <For
                                each=move || roles.clone()
                                key=|role| (role.id, role.name.clone(), role.member_count)
                                children=move |role| {
                                    view! {
                                        <GroupRoleRow
                                            group_id=group_id
                                            role=role
                                            is_owner=is_owner
                                            expanded_role=expanded_role
                                            on_changed=on_changed
                                            rename_role=rename_role
                                            delete_role=delete_role
                                        />
                                    }
                                }
                            />
                        </div>
                    },
                )
            }}
        </section>
    }
}

#[component]
fn GroupRoleRow(
    group_id: i32,
    role: GroupRole,
    #[prop(into)] is_owner: Signal<bool>,
    expanded_role: RwSignal<Option<i32>>,
    #[prop(into)] on_changed: Callback<()>,
    rename_role: Action<(i32, String), Result<GroupRole, AppError>>,
    delete_role: Action<i32, Result<(), AppError>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let toasts = use_toast();
    let role_id = role.id;
    let role_name = role.name.clone();
    let member_count = role.member_count;
    let source = role.source;
    let sync_state = role.sync_state;
    // A synced role's membership belongs to Discord: the server answers 400
    // for an add, so no picker and no remove buttons are offered.
    let editable_members = source == GroupRoleSource::Manual;
    let expanded = Signal::derive(move || expanded_role.get() == Some(role_id));
    let (renaming, set_renaming) = signal(false);
    let (rename_value, set_rename_value) = signal(role.name.clone());
    let (confirm_delete, set_confirm_delete) = signal(false);

    let add_role_member = Action::new(move |(user_id, display_name): &(i64, String)| {
        add_group_role_member(group_id, role_id, *user_id, display_name.clone())
    });
    let remove_role_member =
        Action::new(move |user_id: &i64| remove_group_role_member(group_id, role_id, *user_id));

    let role_members = Resource::new(
        move || {
            (
                expanded.get(),
                add_role_member.version().get(),
                remove_role_member.version().get(),
            )
        },
        move |(expanded, _, _)| async move {
            if !expanded {
                return Ok(Vec::new());
            }
            get_group_role_members(group_id, role_id).await
        },
    );

    Effect::new(move |_| {
        let (Some(result), Some(toasts)) = (add_role_member.value().get(), toasts) else {
            return;
        };
        match result {
            Ok(()) => {
                toasts.success(t_string!(i18n, groups_role_member_added));
                on_changed.run(());
            }
            Err(e) => toasts.error(t_string!(
                i18n,
                groups_role_member_error,
                error = e.to_string()
            )),
        }
    });

    Effect::new(move |_| {
        let (Some(result), Some(toasts)) = (remove_role_member.value().get(), toasts) else {
            return;
        };
        match result {
            Ok(()) => {
                toasts.success(t_string!(i18n, groups_role_member_removed));
                on_changed.run(());
            }
            Err(e) => toasts.error(t_string!(
                i18n,
                groups_role_member_error,
                error = e.to_string()
            )),
        }
    });

    view! {
        <div class="rounded bg-black/20">
            <div class="flex items-center gap-2 p-2">
                <button
                    type="button"
                    class="flex min-w-0 flex-1 items-center gap-2 text-left"
                    aria-expanded=move || expanded.get().to_string()
                    aria-label=move || {
                        if expanded.get() {
                            t_string!(i18n, groups_role_hide_members).to_string()
                        } else {
                            t_string!(i18n, groups_role_show_members).to_string()
                        }
                    }
                    on:click=move |_| {
                        expanded_role
                            .update(|open| {
                                *open = if *open == Some(role_id) { None } else { Some(role_id) };
                            });
                    }
                >
                    <Icon icon=Signal::derive(move || {
                        if expanded.get() { i::BiChevronUpRegular } else { i::BiChevronDownRegular }
                    }) />
                    <span class="truncate font-medium">{role_name.clone()}</span>
                    <span class="shrink-0 text-xs text-[color:var(--color-text-muted)]">
                        {t!(i18n, groups_member_count, count = member_count)}
                    </span>
                    <RoleSyncMarker source=source sync_state=sync_state />
                </button>

                <Show when=move || is_owner.get()>
                    <button
                        type="button"
                        class="btn-ghost btn-xs shrink-0"
                        aria-label=move || t_string!(i18n, groups_role_rename).to_string()
                        on:click=move |_| set_renaming.update(|open| *open = !*open)
                    >
                        <Icon icon=i::BiPencilSolid />
                    </button>
                    <button
                        type="button"
                        class=move || {
                            if confirm_delete.get() {
                                "btn-danger btn-xs shrink-0"
                            } else {
                                "btn-ghost btn-xs shrink-0 text-red-400 hover:text-red-300"
                            }
                        }
                        aria-label=move || {
                            if confirm_delete.get() {
                                t_string!(i18n, groups_role_delete_confirm).to_string()
                            } else {
                                t_string!(i18n, groups_role_delete).to_string()
                            }
                        }
                        on:click=move |_| {
                            if confirm_delete.get_untracked() {
                                delete_role.dispatch(role_id);
                            } else {
                                set_confirm_delete.set(true);
                            }
                        }
                    >
                        <Icon icon=i::BiTrashRegular />
                    </button>
                </Show>
            </div>

            <Show when=move || renaming.get() && is_owner.get()>
                <div class="flex gap-2 px-2 pb-2">
                    <input
                        class="input input-sm min-w-0 flex-1"
                        aria-label=move || t_string!(i18n, groups_role_rename).to_string()
                        prop:value=rename_value
                        on:input=move |ev| set_rename_value.set(event_target_value(&ev))
                    />
                    <button
                        type="button"
                        class="btn-secondary btn-sm shrink-0"
                        prop:disabled=move || rename_value.get().trim().is_empty()
                        aria-label=move || t_string!(i18n, save).to_string()
                        on:click=move |_| {
                            let name = rename_value.get_untracked().trim().to_string();
                            if name.is_empty() {
                                return;
                            }
                            rename_role.dispatch((role_id, name));
                            set_renaming.set(false);
                        }
                    >
                        <Icon icon=i::BiSaveSolid />
                    </button>
                </div>
            </Show>

            <Show when=move || expanded.get()>
                <div class="flex flex-col gap-2 border-t border-[color:var(--color-outline)] px-2 py-2">
                    {if editable_members {
                        Either::Left(
                            view! {
                                <Show when=move || is_owner.get()>
                                    <MemberSearchPicker
                                        group_id=group_id
                                        role_id=role_id
                                        on_pick=Callback::new(move |
                                            candidate: GroupMemberSearchResult|
                                        {
                                            add_role_member.dispatch((candidate.user_id, candidate.display_name));
                                        })
                                    />
                                </Show>
                            },
                        )
                    } else {
                        Either::Right(
                            view! {
                                <p class="text-xs text-[color:var(--color-text-muted)]">
                                    {t!(i18n, groups_role_read_only)}
                                </p>
                            },
                        )
                    }}

                    <Suspense fallback=move || {
                        view! { <div class="skeleton-block skeleton-shimmer h-8 rounded"></div> }
                    }>
                        {move || {
                            role_members
                                .get()
                                .map(|result| match result {
                                    Ok(members) if members.is_empty() => {
                                        view! {
                                            <p class="text-xs text-[color:var(--color-text-muted)]">
                                                {t!(i18n, groups_role_no_members)}
                                            </p>
                                        }
                                            .into_any()
                                    }
                                    Ok(members) => {
                                        view! {
                                            <div class="flex flex-col gap-1">
                                                {members
                                                    .into_iter()
                                                    .map(|member: UserGroupMember| {
                                                        let member_id = member.user_id;
                                                        view! {
                                                            <div class="flex items-center gap-2 rounded p-1">
                                                                <MemberInitial name=member.username.clone() />
                                                                <span class="min-w-0 flex-1 truncate text-sm">
                                                                    {member.username.clone()}
                                                                </span>
                                                                <Show when=move || is_owner.get() && editable_members>
                                                                    <button
                                                                        type="button"
                                                                        class="btn-ghost btn-xs shrink-0 text-red-400 hover:text-red-300"
                                                                        aria-label=move || {
                                                                            t_string!(i18n, groups_remove_from_role).to_string()
                                                                        }
                                                                        on:click=move |_| {
                                                                            remove_role_member.dispatch(member_id);
                                                                        }
                                                                    >
                                                                        <Icon icon=i::BiXRegular />
                                                                    </button>
                                                                </Show>
                                                            </div>
                                                        }
                                                    })
                                                    .collect_view()}
                                            </div>
                                        }
                                            .into_any()
                                    }
                                    Err(e) => {
                                        view! {
                                            <div class="text-xs text-red-400">
                                                {move || t!(i18n, groups_role_members_error, error = e.to_string())}
                                            </div>
                                        }
                                            .into_any()
                                    }
                                })
                        }}
                    </Suspense>
                </div>
            </Show>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use ultros_api_types::user::group::GroupSource;

    fn group(source: GroupSource, guild_id: Option<i64>, frozen: Option<&str>) -> UserGroup {
        UserGroup {
            id: 7,
            name: "Bahamut Bakers".to_string(),
            owner_id: 42,
            guild_id,
            guild_icon_url: None,
            source,
            frozen_reason: frozen.map(str::to_string),
        }
    }

    fn manual_group() -> UserGroup {
        group(GroupSource::Manual, None, None)
    }

    fn synced_group() -> UserGroup {
        group(GroupSource::DiscordGuildMirrored, Some(1234), None)
    }

    /// Freezing unlinks the guild and reverts the source, so a frozen group is
    /// a `Manual` group with a reason — not a third source value.
    fn frozen_group() -> UserGroup {
        group(
            GroupSource::Manual,
            None,
            Some("The Ultros bot was removed from the server"),
        )
    }

    /// Renders the detail page's header and frozen banner exactly as
    /// `GroupDetailPage` composes them, so a fixture group in any state can be
    /// rendered on the server without a database.
    ///
    /// The i18n context has to be provided (and an executor stood up for the
    /// effect it builds) the same way `components::term_badge`'s tests do it.
    fn render(group: UserGroup, is_owner: bool, last_synced: Option<DateTime<Utc>>) -> String {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let frozen_reason = group.frozen_reason.clone();
            view! {
                <GroupHeader
                    group=group
                    member_count=3
                    role_count=2
                    is_owner=Signal::stored(is_owner)
                    last_synced_at=Signal::stored(last_synced)
                    syncing=Signal::stored(false)
                    can_leave=Signal::stored(!is_owner)
                    on_sync=Callback::new(|()| {})
                    on_delete=Callback::new(|()| {})
                    on_leave=Callback::new(|()| {})
                />
                {frozen_reason.map(|reason| view! { <GroupFrozenBanner reason=reason /> })}
            }
            .to_html()
        })
    }

    fn synced_at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 8, 12, 0, 0).unwrap()
    }

    #[test]
    fn a_manual_group_carries_no_badge_and_no_sync_controls() {
        let html = render(manual_group(), true, None);
        assert!(html.contains("Bahamut Bakers"), "{html}");
        // None of the three badges apply to a group that was never linked.
        assert!(!html.contains("Synced with Discord"), "{html}");
        assert!(!html.contains("No longer synced"), "{html}");
        // Nothing to sync with, and nothing to say about a last sync.
        assert!(!html.contains("Sync now"), "{html}");
        assert!(!html.contains("Last synced"), "{html}");
        assert!(!html.contains("Never synced"), "{html}");
        assert!(html.contains("Delete Group"), "{html}");
    }

    #[test]
    fn a_synced_group_shows_the_mirrored_badge_and_the_owners_sync_button() {
        let html = render(synced_group(), true, Some(synced_at()));
        assert!(html.contains("Synced with Discord"), "{html}");
        assert!(html.contains("Sync now"), "{html}");
        assert!(html.contains("Last synced"), "{html}");
        // `RelativeToNow` deliberately renders the absolute timestamp on the
        // server and only flips to "12 minutes ago" after hydration, so the
        // two renders cannot disagree.
        assert!(html.contains("2026-09-08 12:00 UTC"), "{html}");
        assert!(!html.contains("No longer synced"), "{html}");
    }

    #[test]
    fn a_guild_linked_group_that_has_never_synced_says_so() {
        let mut group = synced_group();
        group.source = GroupSource::DiscordGuild;
        let html = render(group, true, None);
        assert!(html.contains("Never synced"), "{html}");
        assert!(!html.contains("Last synced"), "{html}");
    }

    #[test]
    fn a_frozen_group_swaps_the_badge_for_a_banner_and_drops_sync() {
        let html = render(frozen_group(), true, Some(synced_at()));
        assert!(html.contains("No longer synced with Discord"), "{html}");
        assert!(
            html.contains("The Ultros bot was removed from this Discord server"),
            "{html}"
        );
        // The server's own reason rides along as a diagnostic.
        assert!(
            html.contains("The Ultros bot was removed from the server"),
            "{html}"
        );
        // Freezing releases the guild link, so there is nothing left to sync.
        assert!(!html.contains("Sync now"), "{html}");
        assert!(!html.contains("Synced with Discord"), "{html}");
    }

    #[test]
    fn a_member_gets_leave_and_never_delete() {
        let html = render(synced_group(), false, Some(synced_at()));
        assert!(html.contains("Leave Group"), "{html}");
        assert!(!html.contains("Delete Group"), "{html}");
        // Syncing is the owner's button, even on a group that can be synced.
        assert!(!html.contains("Sync now"), "{html}");
    }

    #[test]
    fn the_role_marker_distinguishes_manual_synced_and_orphaned_roles() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        let render_marker = |source, sync_state| {
            owner.with(|| {
                provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
                view! { <RoleSyncMarker source=source sync_state=sync_state /> }.to_html()
            })
        };
        let manual = render_marker(GroupRoleSource::Manual, GroupRoleSyncState::Synced);
        assert!(!manual.contains("Synced"), "{manual}");
        assert!(!manual.contains("Orphaned"), "{manual}");

        let synced = render_marker(GroupRoleSource::DiscordRole, GroupRoleSyncState::Synced);
        assert!(synced.contains("Synced"), "{synced}");

        let orphaned = render_marker(GroupRoleSource::DiscordRole, GroupRoleSyncState::Orphaned);
        assert!(orphaned.contains("Orphaned"), "{orphaned}");
    }
}
