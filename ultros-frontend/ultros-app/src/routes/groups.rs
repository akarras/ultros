use crate::api::{
    create_group, create_group_from_guild, create_group_invite, delete_group_invite,
    get_group_detail, get_group_invites, get_groups, get_login, list_manageable_discord_guilds,
    use_group_invite,
};
use crate::components::app_link::AppLink;
use crate::components::icon::Icon;
use crate::components::invite_link;
use crate::components::loading::Loading;
use crate::components::meta::{MetaDescription, MetaRobotsNoIndex, MetaTitle};
use crate::components::skeleton::BoxSkeleton;
use crate::components::tool_help::ActionableEmptyState;
use crate::global_state::clipboard_text::GlobalLastCopiedText;
use crate::global_state::toasts::use_toast;
use crate::i18n::*;
use icondata as i;
use leptos::either::Either;
use leptos::prelude::*;
use leptos_router::hooks::{use_navigate, use_params_map};
use ultros_api_types::user::group::{CreateGroup, CreateGroupInvite, GroupSource, UserGroup};

/// Route prefix minted into invite links, matching the `group/invite/:invite_id`
/// route registered in `lib.rs`.
const GROUP_INVITE_PATH: &str = "/group/invite";

/// Path of a group's detail page. Kept next to the invite prefix so both
/// route shapes registered in `lib.rs` are named in one place.
pub(crate) fn group_detail_path(group_id: i32) -> String {
    format!("/groups/{group_id}")
}

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
                                <AppLink exact=true attr:class="nav-link" href="/groups">
                                    <Icon height="1.25em" width="1.25em" icon=i::BiGroupSolid />
                                    <span>{t!(i18n, groups)}</span>
                                </AppLink>
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

                            <Suspense fallback=move || view! { <BoxSkeleton rows=3 /> }>
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
                                                                    view! { <GroupCard group=group /> }
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

/// Landing page for `/group/invite/:invite_id`.
///
/// Mirrors `ListInviteAccept`: redeem as soon as we know the visitor is logged
/// in, otherwise send them through `/login?next=` and let the redirect bring
/// them back here. Redeeming an invite you've already used succeeds, so a
/// re-visit lands on the groups page rather than an error.
#[component]
pub fn GroupInviteAccept() -> impl IntoView {
    let i18n = use_i18n();
    let params = use_params_map();
    let navigate = use_navigate();
    let invite_id =
        Memo::new(move |_| params.with(|p| p.get("invite_id").clone().unwrap_or_default()));
    let login = Resource::new(|| (), |_| async move { get_login().await });
    let redeem_invite = Action::new(move |invite_id: &String| use_group_invite(invite_id.clone()));
    // Guards against the effect re-firing and double-dispatching once the
    // action's own state change re-runs it.
    let redeem_started = RwSignal::new(false);

    Effect::new(move |_| {
        if redeem_started.get() {
            return;
        }
        let Some(Ok(_)) = login.get() else {
            return;
        };
        let invite_id = invite_id();
        if invite_id.is_empty() {
            return;
        }
        redeem_started.set(true);
        redeem_invite.dispatch(invite_id);
    });

    Effect::new(move |_| {
        if let Some(Ok(_)) = redeem_invite.value().get() {
            navigate("/groups", Default::default());
        }
    });

    view! {
        <MetaTitle title=move || t_string!(i18n, groups_invite_meta_title).to_string() />
        <MetaRobotsNoIndex />
        <div class="panel mx-auto max-w-xl rounded-xl p-6">
            <div class="space-y-4">
                <div>
                    <h1 class="text-2xl font-bold text-[color:var(--brand-fg)]">{t!(i18n, groups_accept_invite_heading)}</h1>
                    <p class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, groups_accept_invite_body)}</p>
                </div>

                <Suspense fallback=move || view! { <Loading /> }>
                    {move || match login.get() {
                        None => view! { <Loading /> }.into_any(),
                        Some(Err(e)) => {
                            if matches!(
                                e,
                                crate::error::AppError::ApiError(
                                    ultros_api_types::result::ApiError::NotAuthenticated
                                )
                            ) {
                                let href = format!("/login?next=/group/invite/{}", invite_id());
                                view! {
                                    <div class="space-y-4">
                                        <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] p-4 text-sm text-[color:var(--color-text-muted)]">
                                            {t!(i18n, groups_invite_login_required)}
                                        </div>
                                        <a class="btn-primary" rel="external" href=href>
                                            <Icon icon=i::BsPersonCircle />
                                            <span>{t!(i18n, sign_in_discord)}</span>
                                        </a>
                                    </div>
                                }.into_any()
                            } else {
                                view! {
                                    <div class="alert alert-error">{t!(i18n, groups_invite_load_error, error = e.to_string())}</div>
                                }.into_any()
                            }
                        }
                        Some(Ok(_)) => {
                            match redeem_invite.value().get() {
                                Some(Ok(_)) => view! {
                                    <div class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, groups_opening_group)}</div>
                                }.into_any(),
                                Some(Err(e)) => view! {
                                    <div class="space-y-3">
                                        <div class="alert alert-error">{t!(i18n, groups_invite_accept_error, error = e.to_string())}</div>
                                        <AppLink href="/groups" attr:class="btn-secondary">{t!(i18n, groups_back_to_groups_link)}</AppLink>
                                    </div>
                                }.into_any(),
                                None => view! {
                                    <div class="text-sm text-[color:var(--color-text-muted)]">
                                        {move || if redeem_invite.pending().get() {
                                            t_string!(i18n, groups_invite_accepting).to_string()
                                        } else {
                                            t_string!(i18n, groups_invite_preparing).to_string()
                                        }}
                                    </div>
                                }.into_any(),
                            }
                        }
                    }}
                </Suspense>
            </div>
        </div>
    }
}

/// Guild avatar with a first-letter fallback, matching the endpoint picker.
/// The stored icon URL is a snapshot and can 404 if the guild changes its icon,
/// so `on:error` falls back rather than leaving a broken image.
#[component]
pub(crate) fn GuildIcon(
    icon_url: Option<String>,
    name: String,
    /// Tailwind size classes, so the detail page's header can render a larger
    /// icon out of the same fallback logic.
    #[prop(default = "h-8 w-8")]
    size: &'static str,
) -> impl IntoView {
    let initial = name.chars().next().unwrap_or('?').to_string();
    let (failed, set_failed) = signal(false);
    move || {
        let initial = initial.clone();
        match icon_url.clone().filter(|_| !failed()) {
            Some(url) => Either::Left(view! {
                <img
                    src=url
                    class=format!("{size} shrink-0 rounded object-cover")
                    alt=""
                    on:error=move |_| set_failed(true)
                />
            }),
            None => Either::Right(view! {
                <span class=format!(
                    "{size} flex shrink-0 items-center justify-center rounded bg-[color:var(--color-background-panel)] text-xs font-bold",
                )>{initial}</span>
            }),
        }
    }
}

/// The one badge a group carries: frozen wins over mirrored, which wins over
/// a plain guild link. A manual group gets nothing.
///
/// Pure — no resources, no context beyond i18n — so the SSR render tests can
/// drive it straight from a fixture.
#[component]
pub(crate) fn GroupSourceBadge(source: GroupSource, frozen: bool) -> impl IntoView {
    let i18n = use_i18n();
    let badge = move |class: &'static str, icon, label: AnyView| {
        view! {
            <span class=format!(
                "inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-bold uppercase tracking-wider {class}",
            )>
                <Icon icon=icon />
                {label}
            </span>
        }
    };
    move || match (frozen, source) {
        (true, _) => Some(badge(
            "border border-amber-500/50 text-amber-300",
            i::BiUnlinkRegular,
            t!(i18n, groups_badge_frozen).into_any(),
        )),
        (false, GroupSource::DiscordGuildMirrored) => Some(badge(
            "border border-brand-500/50 text-brand-300",
            i::BsDiscord,
            t!(i18n, groups_badge_synced).into_any(),
        )),
        (false, GroupSource::DiscordGuild) => Some(badge(
            "border border-[color:var(--color-outline)] text-[color:var(--color-text-muted)]",
            i::BsDiscord,
            t!(i18n, groups_badge_discord).into_any(),
        )),
        (false, GroupSource::Manual) => None,
    }
}

#[component]
fn GroupCard(group: UserGroup) -> impl IntoView {
    let i18n = use_i18n();
    let group_id = group.id;
    let group_name = group.name.clone();
    let guild_icon_url = group.guild_icon_url.clone();
    let is_guild_linked = group.guild_id.is_some();
    let source = group.source;
    let frozen = group.frozen_reason.is_some();
    // Counts are not on `UserGroup`, so the card asks for the group's detail.
    // One request per card, the same shape the member list had before it moved
    // to the detail page.
    let detail = Resource::new(move || group_id, get_group_detail);

    view! {
        <AppLink
            href=group_detail_path(group_id)
            attr:class="panel flex flex-col gap-3 rounded-xl p-4 transition-colors hover:border-[color:var(--brand-ring)]"
            attr:aria-label=move || {
                format!("{}: {}", t_string!(i18n, groups_open_group), group_name.clone())
            }
        >
            <div class="flex items-center gap-2 overflow-hidden">
                {is_guild_linked
                    .then(|| {
                        view! { <GuildIcon icon_url=guild_icon_url name=group.name.clone() /> }
                    })}
                <span class="truncate text-xl font-bold text-[color:var(--brand-fg)]">
                    {group.name.clone()}
                </span>
            </div>

            <GroupSourceBadge source=source frozen=frozen />

            <Suspense fallback=move || {
                view! { <div class="skeleton-block skeleton-shimmer h-4 w-32 rounded"></div> }
            }>
                {move || {
                    detail
                        .get()
                        .map(|res| match res {
                            Ok(detail) => {
                                let members = detail.member_count;
                                let roles = detail.roles.len() as i64;
                                Either::Left(
                                    view! {
                                        <div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm text-[color:var(--color-text-muted)]">
                                            <span>{t!(i18n, groups_member_count, count = members)}</span>
                                            <span>{t!(i18n, groups_role_count, count = roles)}</span>
                                        </div>
                                    },
                                )
                            }
                            Err(e) => {
                                Either::Right(
                                    view! {
                                        <div class="text-xs text-red-400">
                                            {move || t!(i18n, groups_detail_error, error = e.to_string())}
                                        </div>
                                    },
                                )
                            }
                        })
                }}
            </Suspense>
        </AppLink>
    }
}

/// Owner-only invite link management for a single group. Lives on the group
/// detail page.
///
/// Kept a separate component so the invites resource is only created for a
/// group the visitor owns — a member would just get a 403 from every one of
/// these calls.
#[component]
pub(crate) fn GroupInvitePanel(group_id: i32) -> impl IntoView {
    let i18n = use_i18n();
    let toasts = use_toast();
    let last_copied = use_context::<GlobalLastCopiedText>();
    let create_invite = Action::new(move |max_uses: &Option<i32>| {
        create_group_invite(
            group_id,
            CreateGroupInvite {
                max_uses: *max_uses,
            },
        )
    });
    let delete_invite =
        Action::new(move |invite_id: &String| delete_group_invite(invite_id.clone()));

    Effect::new(move |_| {
        if let (Some(res), Some(toasts)) = (create_invite.value().get(), toasts) {
            match res {
                // Copying on creation is the whole point of the button — the
                // owner wants the link, not a row in a table.
                Ok(invite) => {
                    invite_link::copy_invite_url(
                        GROUP_INVITE_PATH,
                        &invite.id,
                        last_copied,
                        Some(toasts),
                        t_string!(i18n, groups_invite_copied).to_string(),
                    );
                }
                Err(e) => toasts.error(format!("Failed to create invite: {e}")),
            }
        }
    });

    Effect::new(move |_| {
        if let (Some(res), Some(toasts)) = (delete_invite.value().get(), toasts) {
            match res {
                Ok(_) => toasts.success(t_string!(i18n, groups_invite_deleted)),
                Err(e) => toasts.error(format!("Failed to delete invite: {e}")),
            }
        }
    });

    let invites = Resource::new(
        move || (create_invite.version().get(), delete_invite.version().get()),
        move |_| get_group_invites(group_id),
    );

    let (max_uses, set_max_uses) = signal(String::new());

    view! {
        <div class="flex flex-col gap-2 pt-2 border-t border-gray-700/50">
            <h4 class="text-xs font-semibold text-gray-400 uppercase tracking-wider">{t!(i18n, groups_invite_links_heading)}</h4>
            <p class="text-xs text-[color:var(--color-text-muted)]">{t!(i18n, groups_invite_help)}</p>
            <div class="flex gap-2">
                <input
                    id=format!("invite-max-uses-{}", group_id)
                    class="input input-sm flex-1 min-w-0"
                    inputmode="numeric"
                    aria-label=move || t_string!(i18n, groups_invite_max_uses_placeholder).to_string()
                    placeholder=t_string!(i18n, groups_invite_max_uses_placeholder)
                    prop:value=max_uses
                    on:input=move |ev| set_max_uses(event_target_value(&ev))
                />
                <button
                    type="button"
                    class="btn-secondary btn-sm shrink-0"
                    prop:disabled=create_invite.pending()
                    on:click=move |_| {
                        // A blank box means "no limit"; anything unparseable is
                        // treated the same rather than silently capping at 0.
                        create_invite.dispatch(max_uses().trim().parse::<i32>().ok());
                        set_max_uses(String::new());
                    }
                >
                    <Icon icon=i::BiLinkRegular />
                    <span>{t!(i18n, groups_invite_create_button)}</span>
                </button>
            </div>
            <Suspense fallback=move || view! { <div class="skeleton-block skeleton-shimmer h-6 rounded" /> }>
                {move || {
                    invites.get().map(|res| {
                        match res {
                            Ok(invites) if invites.is_empty() => view! {
                                <p class="text-xs text-[color:var(--color-text-muted)]">{t!(i18n, groups_invite_none)}</p>
                            }.into_any(),
                            Ok(invites) => view! {
                                <div class="flex flex-col gap-1">
                                    <For
                                        each=move || invites.clone()
                                        key=|invite| invite.id.clone()
                                        children=move |invite| {
                                            let copy_id = invite.id.clone();
                                            let delete_id = invite.id.clone();
                                            // The full code is 48 hex chars and
                                            // would blow out the card; the copy
                                            // button hands over the real link.
                                            let short = invite.id.chars().take(10).collect::<String>();
                                            let uses = invite_link::uses_label(invite.uses, invite.max_uses);
                                            view! {
                                                <div class="flex items-center gap-2 p-2 rounded bg-black/20">
                                                    <span class="font-mono text-xs truncate min-w-0 flex-1">{short}</span>
                                                    <span class="shrink-0 text-xs text-[color:var(--color-text-muted)]">{uses}</span>
                                                    <button
                                                        type="button"
                                                        class="btn-ghost btn-xs shrink-0"
                                                        aria-label=move || t_string!(i18n, groups_invite_copy_link).to_string()
                                                        on:click=move |_| {
                                                            invite_link::copy_invite_url(
                                                                GROUP_INVITE_PATH,
                                                                &copy_id,
                                                                last_copied,
                                                                toasts,
                                                                t_string!(i18n, groups_invite_copied).to_string(),
                                                            );
                                                        }
                                                    >
                                                        <Icon icon=i::BsClipboard2Fill />
                                                    </button>
                                                    <button
                                                        type="button"
                                                        class="btn-ghost btn-xs shrink-0 text-red-400 hover:text-red-300"
                                                        aria-label=move || t_string!(i18n, groups_invite_delete).to_string()
                                                        on:click=move |_| {
                                                            delete_invite.dispatch(delete_id.clone());
                                                        }
                                                    >
                                                        <Icon icon=i::BiXRegular />
                                                    </button>
                                                </div>
                                            }
                                        }
                                    />
                                </div>
                            }.into_any(),
                            Err(e) => view! {
                                <div class="text-xs text-red-400">
                                    {move || t!(i18n, groups_invite_list_error, error = e.to_string())}
                                </div>
                            }.into_any(),
                        }
                    })
                }}
            </Suspense>
        </div>
    }
}
