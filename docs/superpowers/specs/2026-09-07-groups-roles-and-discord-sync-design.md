# Groups: roles, member picker, and Discord membership sync

Date: 2026-09-07
Closes: #1062, #1076, #1077
Builds on: #1090 (phase 1, create from guild), #1146 (invite codes), #1213 (leave), #1270 (OAuth guild discovery)

## Problem

Groups exist on the site but are half-built. A group can be created from a Discord
server and carries its name and icon, yet membership is entirely manual: the owner
types a raw Discord snowflake into a box on the card. That box fails with an opaque
error for anyone who has never logged into Ultros, because `user_group_member` has a
foreign key to `discord_user`. Nothing ever syncs from Discord, usernames only refresh
on login, and there is no group page at all, just a card.

## Decisions taken

- The Server Members privileged intent **will be enabled** in the Discord developer
  portal. The bot is under 100 guilds, so no verification review is needed.
- Roles live **inside** a group. A group has named roles; each role is a member set.
  A role is either hand-built or imported from a Discord role and kept in sync.
- When the bot is removed from a guild, the group is **frozen, not deleted**:
  unlinked from Discord, members kept, a banner explains it is no longer synced.
- Sync uses gateway events for low latency **and** periodic reconciliation as the
  source of truth.
- Lists can be shared to a group **or to a specific role**.

## 1. Data model

### New tables

`group_role`

| column            | type      | notes                                                     |
|-------------------|-----------|-----------------------------------------------------------|
| `id`              | serial PK |                                                           |
| `group_id`        | int       | FK `user_group.id`, cascade                               |
| `name`            | text      |                                                           |
| `discord_role_id` | bigint    | nullable; unique index on `(group_id, discord_role_id)`   |
| `source`          | smallint  | `0 = Manual`, `1 = DiscordRole`                           |
| `sync_state`      | smallint  | `0 = Synced`, `1 = Orphaned` (Discord role or guild gone) |
| `last_synced_at`  | timestamptz | nullable                                                |
| `position`        | int       | ordering; mirrors Discord's role position when imported   |

`group_role_member`

| column    | type   | notes                          |
|-----------|--------|--------------------------------|
| `role_id` | int    | PK part, FK `group_role.id`, cascade |
| `user_id` | bigint | PK part, FK `discord_user.id`, cascade |

`list_shared_role`

| column       | type     | notes                                        |
|--------------|----------|----------------------------------------------|
| `list_id`    | int      | PK part, FK `list.id`, cascade               |
| `role_id`    | int      | PK part, FK `group_role.id`, cascade         |
| `permission` | smallint | same encoding as `list_shared_group`         |

A separate table rather than a nullable column on `list_shared_group`, whose primary
key is `(list_id, group_id)`.

### Changes to existing tables

- `user_group.source` gains `2 = DiscordGuildMirrored`: membership is owned by sync.
  Set when the first Discord role is imported; reverts to `1 = DiscordGuild` when the
  last synced role is deleted; reverts to `0 = Manual` when the group is frozen.
- `user_group.frozen_reason` (text, nullable). Non-null drives the frozen banner.
- `user_group_member.source` (smallint, not null, default `0`): `0 = Manual` (owner
  added, invite redeemed, or group creator), `1 = Synced` (added by reconciliation or
  a gateway event). Distinguishes who reconciliation may remove.

### Invariant

Every `group_role_member` row has a matching `user_group_member` row. The DB layer
enforces it: adding a user to a role inserts the group membership if missing (with
`source` matching the caller); removing a user from the group deletes all their role
memberships in the same transaction. Callers never maintain this themselves.

### Whole-server sync

Discord's `@everyone` role has the guild's id. Syncing the whole server is importing
`@everyone`; there is no separate "sync all members" mode.

## 2. Sync engine

Lives in a new `ultros/src/group_sync/` module with three files: `diff.rs` (pure set
logic), `reconcile.rs` (Discord fetch plus DB apply), `events.rs` (gateway handlers).

### Member identity

Guild members who have never logged in get a `discord_user` row through the existing
`get_or_create_discord_user` upsert, using their Discord global display name (falling
back to username). Every reconciliation refreshes the name, which also fixes stale
usernames. When such a user later logs in they are already in their groups.

### Reconciliation (source of truth)

`reconcile_guild(guild_id)`:

1. Load the guild's group and its synced roles. If there are none, return.
2. Fetch the guild's roles. Any synced role whose Discord role is missing becomes
   `Orphaned`; its members are kept.
3. Paginate `GET /guilds/{id}/members` at 1000 per page.
4. For each synced role compute the desired member set (members holding that role,
   or every member for `@everyone`). `diff::plan(desired, current)` yields adds and
   removes. Apply in one transaction per guild:
   - adds: upsert `discord_user`, insert `group_role_member`, insert
     `user_group_member` with `source = Synced` if absent;
   - removes: delete `group_role_member`; if the user now holds no synced role in the
     group **and** their `user_group_member.source` is `Synced`, delete the group
     membership too. Manually added or invited members are never removed by sync.
5. Stamp `last_synced_at` on each role and log a summary at `info!`.

Scheduling:

- A `tokio::time::interval` of **6 hours** walks every guild that has at least one
  synced role, sequentially, with a short pause between guilds.
- `POST /api/v1/group/{id}/sync` runs it on demand. A per-guild rate limit of one run
  per 5 minutes returns a "recently synced" response instead of running.
- Importing a role triggers a reconcile for that guild immediately, off the request
  path. The import endpoint returns as soon as the role row exists; the page polls
  `last_synced_at` until it is set.

Reconciliation errors: transient Discord failures are logged at `warn!` (repo
convention, see `is_transient()`), the guild is skipped, and existing membership is
left untouched. A 403 on the members endpoint is logged at `error!` once per process
with a message naming the Server Members intent.

### Gateway events (latency optimization)

`poise::FrameworkOptions::event_handler` is wired for the first time. Each handler
first runs one indexed query and returns early otherwise, so the bot's other guilds
cost nothing: member and role events check "does this guild have any synced roles";
`GuildDelete` checks "does any group have this `guild_id`", because a group created
from a guild must freeze even if it never imported a role.

| event                  | action                                                                                  |
|------------------------|-----------------------------------------------------------------------------------------|
| `GuildMemberAddition`  | apply the member's roles against the guild's synced roles                               |
| `GuildMemberUpdate`    | same; covers role grants and removals                                                   |
| `GuildMemberRemoval`   | remove from all synced roles; remove from the group if `source = Synced`                |
| `GuildRoleDelete`      | mark the matching role `Orphaned`, keep members                                         |
| `GuildDelete`          | if `unavailable` is false (kicked, not an outage): **freeze** the group (below)         |
| `GuildCreate`          | debug log only; it replays on every reconnect and reconciliation already covers it      |

Freeze: `guild_id` set to null, `source` to `Manual`, `frozen_reason` set, every synced
role set to `source = Manual, sync_state = Orphaned`, all members kept. The guild's
unique index slot is released so the server can be re-linked later by creating a new
group.

Intents become `GatewayIntents::non_privileged() | GatewayIntents::GUILD_MEMBERS`.

## 3. Server API

All under `/api/v1`. "Owner" means `user_group.owner_id`; "member" means a
`user_group_member` row exists.

| method | path                                          | who    | purpose                                                               |
|--------|-----------------------------------------------|--------|-----------------------------------------------------------------------|
| GET    | `/group/{id}`                                 | member | group, roles with member counts, sync metadata, `frozen_reason`       |
| GET    | `/group/{id}/discord-roles`                   | owner  | guild roles with `existing_role_id`; managed and bot roles hidden     |
| POST   | `/group/{id}/roles`                           | owner  | create a manual role `{ name }`                                       |
| POST   | `/group/{id}/roles/import`                    | owner  | `{ discord_role_id }`; creates the role, kicks off a reconcile        |
| PATCH  | `/group/{id}/roles/{role_id}`                 | owner  | rename                                                                |
| DELETE | `/group/{id}/roles/{role_id}`                 | owner  | delete role; group members untouched                                  |
| GET    | `/group/{id}/roles/{role_id}/members`         | member | role members                                                          |
| POST   | `/group/{id}/roles/{role_id}/members/{uid}`   | owner  | manual roles only; synced roles return 400 "managed by Discord"       |
| DELETE | `/group/{id}/roles/{role_id}/members/{uid}`   | owner  | manual roles only                                                     |
| GET    | `/group/{id}/member-search?q=`                | owner  | see below                                                             |
| POST   | `/group/{id}/sync`                            | owner  | reconcile now, rate limited                                           |
| POST   | `/list/{id}/share/role`                       | list owner | `{ role_id, permission }`                                          |
| DELETE | `/list/{id}/share/role/{role_id}`             | list owner |                                                                    |

Member search returns `{ user_id, display_name, avatar_url, on_ultros: bool }` rows,
capped at 10:

- guild-linked group: Discord's member search endpoint (`/guilds/{id}/members/search`,
  prefix match on username or nickname, no privileged intent required);
- manual group: `discord_user.username` prefix match, case-insensitive.

`POST /group/{id}/members/{uid}` (existing `add_group_member`) now takes an optional
JSON body `{ display_name }` and upserts the `discord_user` row before inserting, so
the foreign-key failure is gone. Members of a group whose `source = Synced` cannot be
removed through this endpoint by anyone, the owner or themselves (400, "managed by
Discord"); reconciliation would re-add them within hours, so the honest answer is
that they leave by leaving the Discord role. The frontend hides the leave button
for synced members.

`GET /group/{id}/members` gains `roles: Vec<i32>` and `source` per member.

`get_permission` in `ultros-db/src/lists.rs` gains a second join, `list_shared_role`
to `group_role_member`, and takes the max of all three sources. `get_list_shares`
returns role shares with `group_name` and `role_name`.

## 4. Frontend

### `/groups` (summary grid)

Each card: guild icon or initial, name, member count, role count, and one badge:
"Synced with Discord" (`source = DiscordGuildMirrored`), "Discord" (`DiscordGuild`),
or "No longer synced" (`frozen_reason` set). The card is a link to the detail page.
Delete, leave, invites, and the member box move off the card. The two create panels
stay as they are.

### `/groups/:id` (new detail page)

Built on the `.panel` surface. Sections top to bottom:

1. **Header**: icon, name, source badge, "last synced 12 min ago", owner-only
   "Sync now" (guild-linked and not frozen), leave (members) or delete-with-confirm
   (owner).
2. **Frozen banner** when `frozen_reason` is set, explaining the bot left the server
   and membership is now manual.
3. **Members**: list rows with avatar initial, name, role chips, and either a remove
   button (manual members, owner only) or a lock icon with a tooltip "managed by
   Discord" (synced members). Owner sees a search-as-you-type picker: 300 ms debounce,
   results show avatar, name, and a "not on Ultros yet" hint when `on_ultros` is
   false. Picking a result calls the add endpoint.
4. **Roles**: rows with name, member count, and a "Synced" or "Orphaned" marker.
   Owner has "New role" (inline name input) and "Import from Discord" (opens a picker
   of the guild's roles, imported ones shown as taken, same pattern as the guild
   picker). Clicking a role expands its member list; manual roles get the same search
   picker; synced roles are read-only. Rename and delete per role, owner only.
5. **Invite links**: the existing `GroupInvitePanel`, moved here.

Loading states use the palette-driven skeleton classes; the hardcoded gray pulse in
the current card is removed.

### Share list modal

The group picker becomes group first, then an optional role from that group. Existing
shares list role shares as "Group / Role" with an unshare button.

### Routing and navigation

`lib.rs` registers `groups/:id`. The side nav entry is unchanged. Every new string is
added to all seven locale files with real translations.

## 5. Error handling and edge cases

- Bot offline or intent not enabled: import, search, and sync return a clear message;
  the rest of the page works. A 403 from the members list maps to "the Ultros bot
  needs the Server Members intent enabled" so a misconfigured deploy is diagnosable
  from the UI.
- Deleting a role never removes members from the group.
- Importing `@everyone` on a large guild runs in the reconcile task, bounded by
  pagination, never inline in the request.
- Two concurrent imports of the same Discord role: the unique index wins; the loser
  gets a 400 "already imported".
- Reconcile and a gateway event racing on the same guild: both apply idempotent
  upserts and deletes, so the end state is the same regardless of order.
- A user removed from the Discord role who was also added manually keeps their group
  membership (`source = Manual`) but loses the role chip.

## 6. Testing

- DB layer (`ultros-db`, existing test setup): the invariant in both directions;
  sync-removal skips manual members; `get_permission` through a role share; freeze
  releases the guild slot; delete role keeps group members.
- `group_sync::diff`: pure function, table-driven tests for adds, removes, no-ops,
  and `@everyone`.
- `group_sync::events`: handler functions take the parsed event fields, not a live
  context, and are tested against a DB fixture.
- Frontend: SSR `to_html()` tests for the detail page with a fixture group in each
  state (manual, synced, frozen), following the repo's existing pattern; a Puppeteer
  screenshot of `/groups/:id` added to the e2e harness.

## 7. Delivery

Three stacked PRs, each rebased onto main before merge so CI runs:

1. **Schema and DB layer**: migrations, entities, `GroupSource::DiscordGuildMirrored`,
   role CRUD, invariant, `get_permission` role join, tests.
2. **Sync engine and API**: intents, event handler, reconciliation loop, all new
   endpoints, member search, API types.
3. **Frontend**: summary cards, detail page, pickers, share modal, locales, changelog
   entry under `ultros-changelog/changes/`.

Deploy note for PR 2: flip Server Members Intent in the developer portal before the
container restarts, otherwise the gateway rejects the connection.

## Out of scope

- Mapping Discord roles to Ultros permission tiers (owner vs member).
- Showing servers the bot is not in with an "invite the bot" call to action.
- Notifying members when they are added to or removed from a group.
