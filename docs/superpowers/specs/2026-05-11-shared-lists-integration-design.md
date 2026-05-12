# Shared Lists Integration & Onboarding — Design

**Date:** 2026-05-11
**Status:** Awaiting user review
**Scope:** Make the already-built Shared Lists backend usable, and extend it with Discord guild integration. Four tiers: (1) surface ownership and permissions in existing UI, (2) ship the missing sharing/invite/group UI, (2b) bind lists/groups to Discord guilds with role-based access and channel notifications, (3) thread discovery into onboarding and analyzer flows. Includes targeted backend refactors (`web.rs` split, permission extractor, scoped WebSocket broadcasts).

---

## Why

Shared Lists has an almost complete backend and almost no frontend.

Concrete findings from walking the code:

- **All sharing endpoints exist and are reachable.** `share_list_with_user`, `share_list_with_group`, `unshare_list_from_*`, `create_invite`, `use_invite`, `delete_invite`, full group CRUD, and group-member management are all wired in [ultros/src/web.rs:1195-1232](ultros/src/web.rs:1195). Permission enforcement is in place via `ListPermission` ([ultros-api-types/src/list.rs:5-23](ultros-api-types/src/list.rs:5)).
- **The frontend shows shared lists silently.** [`get_lists`](ultros-frontend/ultros-app/src/api.rs:225) returns owned and shared lists merged, and [`EditLists`](ultros-frontend/ultros-app/src/routes/lists.rs:124) renders them in one undifferentiated grid. There is no badge for permission, no owner attribution, no separation between "Mine" and "Shared with me". The delete button is shown on every card even when the user has only `Read` — the backend will reject it, but the affordance is misleading.
- **There is no UI to *share* a list.** No share modal, no user picker, no permission selector, no invite-link generator, no copy-link affordance. The "edit list" panel ([lists.rs:40-91](ultros-frontend/ultros-app/src/routes/lists.rs:40)) jumps straight from name/world to a Danger Zone.
- **There is no UI for groups.** `user_group` exists in the DB, all CRUD endpoints exist, but no Leptos component creates, names, or manages a group's members. Group-based sharing is unreachable from the UI.
- **There is no UI for invites.** `create_invite`/`use_invite` exist; users have no way to generate or redeem a link. This is the cleanest fit for "share with a friend who hasn't logged in yet."
- **`/welcome` doesn't mention lists.** [routes/welcome.rs:1-163](ultros-frontend/ultros-app/src/routes/welcome.rs) is a home-world / price-zone / language picker. Lists are arguably the most engaging logged-in feature and don't appear in the discovery surface at all.
- **`add_to_list` doesn't communicate permission.** [components/add_to_list.rs](ultros-frontend/ultros-app/src/components/add_to_list.rs) presents every list the user can see in one dropdown. A user with `Read`-only on a shared list will see it offered, click it, and get a server error.
- **WebSocket broadcasts are unscoped.** [list_view.rs:88](ultros-frontend/ultros-app/src/routes/list_view.rs:88) subscribes to all `ListUpdate` events; the server fans every list change out to every connected client. Works at current scale, doesn't at 10×.
- **`web.rs` is 1322 lines and growing.** It's the only file in `ultros/src/` that contains list, group, invite, retainer, alert, and auth handlers. Each `share_*` handler re-implements ownership check, error mapping, and broadcast plumbing. Adding the missing endpoints (none — already done) would have pushed it further.
- **Stale comment in DB layer.** [ultros-db/src/lists.rs:173](ultros-db/src/lists.rs:173) reads "This should probably also include lists shared with the user" — but `get_lists_for_user` *does* now merge owned + shared + group-shared. The comment is misleading.

Net: the product can ship Shared Lists end-to-end in roughly a week of frontend work plus a focused backend cleanup. The biggest risk is *not* doing the cleanup — adding a share modal, an invite UI, and a group page on top of the current `web.rs` and the current permission check sprawl makes the file untenable.

## Non-goals

- **No new permission levels.** Stays at None/Read/Write/Owner; no fine-grained per-item permissions, no "expiring share", no transfer-ownership UI.
- **No general in-app notifications system.** "X shared a list with you" via a bell icon is a separate spec. Discord *channel* notifications for list events are in scope (Tier 2b) because they reuse the existing alert-delivery pipeline. In-app and Discord-DM notifications are not.
- **No `GUILD_MEMBERS` privileged intent.** That intent requires Discord verification past 100 guilds and a privacy review. We resolve guild role membership lazily via the bot's own REST calls — see Tier 2b.4. If the bot ever needs to react to live role changes (someone gets promoted mid-session), that's a future expansion, not this spec.
- **No new OAuth scopes.** We continue to request only `Identify`. The Discord user id we already have is sufficient — the *bot* is the source of truth for guild membership and roles. Existing users do not get re-prompted.
- **No tutorial / interactive walkthrough.** Onboarding is contextual nudges, not a guided tour.
- **No public lists.** A list is private, shared with named users, shared with groups the owner controls, invite-link gated, or guild-bound (Tier 2b). No "discoverable to all" mode.
- **No mobile-specific layout work beyond responsive defaults.**
- **No rename of `Lists` → `Collections` or similar.**

## Tiers

The work is structured so each tier is independently shippable.

- **Tier 1 — Make ownership visible.** Permission badges, owner attribution, sectioning. No new endpoints.
- **Tier 2 — Sharing UI.** Share modal, invite link UI, groups page. Uses existing endpoints.
- **Tier 2b — Discord guild integration.** Bind a list or group to a guild role for auto-membership; bind a list to a guild channel for event notifications. Adds OAuth scope, bot guild-join handler, new DB tables, reuses existing alert delivery pipeline.
- **Tier 3 — Discovery & onboarding.** `/welcome` step, post-creation nudge, analyzer integration.
- **Backend cleanup (parallel).** `web.rs` split, `RequireListPermission` extractor, scoped WebSocket broadcasts. Lands alongside Tier 2.

---

## Tier 1 — Make ownership visible

Goal: a user who already has shared lists today can immediately tell which is which, without us building any new endpoint.

### 1.1 Permission badge on list cards

`List` ([ultros-api-types/src/list.rs](ultros-api-types/src/list.rs)) does not currently carry the viewer's permission — the frontend only knows that a list came back from `get_lists_for_user`, not *why* it came back. Add a `viewer_permission: ListPermission` field to the response DTO. Server-side, `get_lists_for_user` already computes this implicitly via the join path; we surface it.

Render as a small pill on each `ListCard` ([routes/lists.rs:33](ultros-frontend/ultros-app/src/routes/lists.rs:33)):

- `Owner` → no badge (default state).
- `Write` → blue "Shared · Editor" pill.
- `Read` → grey "Shared · Viewer" pill.

### 1.2 Owner attribution on shared cards

For cards where `viewer_permission != Owner`, show the owner's Discord username under the world line. Requires `owner: Option<UserDisplay>` on the list DTO (already-public Discord avatar + username — same shape used in retainer ownership display).

### 1.3 Hide destructive actions on shared lists

In edit mode for a non-Owner card:

- Hide the **Delete** button (Read or Write — only the owner can delete).
- Hide the **Edit name / world** controls if `Read`.
- Show "Leave list" instead of "Delete" — a new client-side affordance that calls `DELETE /api/v1/list/{id}/share/user/{me}`. This already works on the backend (a user can unshare themselves); we just expose it.

### 1.4 Section the grid

In [`EditLists`](ultros-frontend/ultros-app/src/routes/lists.rs:124), partition `lists` into two groups by `viewer_permission == Owner`. Render under two headings: **My Lists** and **Shared With Me**. Keep the search filter applying to both sections. Empty-state copy ("no_lists_found") becomes specific to "My Lists" — having only shared lists is fine.

### 1.5 `add_to_list` shows permission and disables read-only

In [`components/add_to_list.rs`](ultros-frontend/ultros-app/src/components/add_to_list.rs), use the new `viewer_permission` to:

- Show the pill next to each list name in the dropdown.
- Disable selection (greyed out, tooltip "You have read-only access") for `Read` lists.

### 1.6 Drop the stale comment

Remove the line at [ultros-db/src/lists.rs:173](ultros-db/src/lists.rs:173). The function does include shared lists; the comment now misleads.

**Out:** No new pages, no new endpoints, no behaviour change to backend logic.

---

## Tier 2 — Sharing UI

Goal: every existing backend sharing endpoint becomes reachable from the UI.

### 2.1 Share modal (`<ShareListModal />`)

Triggered from a new "Share" button on the owner's list-edit panel ([lists.rs:57](ultros-frontend/ultros-app/src/routes/lists.rs:57)) and on the list-view page header. Three tabs:

1. **People** — search/select existing Ultros users (Discord display) → choose Read/Write → submit. Backed by `POST /api/v1/list/{id}/share/user`. Below the picker, a list of current shares with revoke buttons (`DELETE /api/v1/list/{id}/share/user/{user_id}`).
2. **Groups** — same shape, scoped to groups the current user owns. Backed by `*/share/group`. If the user has no groups, show a "Create a group" CTA that opens the groups page.
3. **Invite link** — list active invites with their remaining uses; "Create new link" with a permission selector and optional max-uses. Backed by `POST /api/v1/list/{id}/invite/create`. Each row has copy-to-clipboard for `https://ultros.app/list/invite/{invite_id}` and a revoke button.

User search is the riskiest sub-piece. The current backend has no user-search endpoint. Two options:

- **(A)** Add `GET /api/v1/user/search?q=...`. Discord display name prefix match, capped at ~20 results, requires auth.
- **(B)** Defer the People tab; ship Tier 2 with only Groups and Invite link. Users who want to share with a specific friend create a one-use invite link and DM it.

**Recommendation: B.** Invite links are *better* product than a name-search — they don't require the recipient to have logged into Ultros yet, and they sidestep username-collision UX. People-tab can come later if there's demand. Skipping it cuts the Tier 2 surface significantly.

### 2.2 Invite redemption flow

New route `/list/invite/{invite_id}` → server-rendered Leptos page that:

- If the viewer is not logged in: show "Sign in with Discord to accept this invite", store the invite id in a cookie, redirect to OAuth, redirect back, redeem.
- If logged in: call `POST /api/v1/invite/{id}/use`, then redirect to `/list/{list_id}`.
- If the invite is exhausted or revoked: show "This invite is no longer valid" with a button back to the list owner's profile if available, else home.

### 2.3 Groups page (`/groups`)

A new route. List the groups the user owns. Each group:

- Name (editable inline).
- Member list (Discord avatar + name) with a "Remove" button per row.
- "Add member" search → `POST /api/v1/group/{id}/member/{user_id}`. Same dependency on user-search as 2.1 People tab; same recommendation — **defer the search and add members via "Invite to group" link**, which becomes a new lightweight endpoint mirroring list invites. Or simpler: members are added implicitly when someone redeems a list invite that targets a group. (See open question below.)
- "Delete group" with a confirmation.
- "Create new group" button at the top.

Nav: add `/groups` to the side nav alongside `/list`.

### 2.4 Sharing status surface on `/list/{id}`

The list-view page ([routes/list_view.rs](ultros-frontend/ultros-app/src/routes/list_view.rs)) gains a small "Shared with N people" indicator next to the title, clickable to open `<ShareListModal />`. For non-owners, it reads "Shared by {owner}" and is non-interactive.

**Open question deferred to implementation:** how do users get added to a group without a user-search? Options: invite-to-group links analogous to invite-to-list, or "members are auto-added when they redeem an invite to a list shared with this group." Pick during plan-writing; both are small. Tier 2b makes a third option viable — *guild-backed groups* (group membership is whoever has the bound Discord role).

---

## Tier 2b — Discord guild integration

Goal: a Discord guild owner / officer can bind one of their guild's roles to an Ultros group (or directly to a list), and bind a guild channel to receive list-change notifications. Members of the role automatically gain the corresponding list permission the next time they log into Ultros. The bot must be in the guild.

This tier is the most net-new — new tables, new OAuth scope, new bot handlers — but the architecture is straightforward because we deliberately avoid the privileged `GUILD_MEMBERS` intent.

### 2b.1 Bot guild lifecycle

When the Ultros bot joins or leaves a guild, sync minimal metadata:

- New table `discord_guild(id, name, icon_url, joined_at, left_at)`. `left_at IS NOT NULL` is the soft-deleted state — keep the row so historical bindings can be re-activated if the bot is re-added.
- New table `discord_guild_role(guild_id, role_id, name, color, position, is_managed)`. Composite primary key. Synced from the bot.

Wire two handlers in [ultros/src/discord/mod.rs](ultros/src/discord/mod.rs):

- `EventHandler::guild_create` — bot joined, or a guild came back online. Upsert `discord_guild`, fetch role list via REST, upsert `discord_guild_role` rows, mark missing roles as deleted.
- `EventHandler::guild_role_create / update / delete` — keep `discord_guild_role` in sync without a full re-fetch.

These events are part of the **non-privileged** GUILD intent set; no Discord verification needed.

### 2b.2 Bot-side guild discovery (no OAuth scope change)

OAuth stays at `Identify` — we already have the Discord user id, and that's all we need. The bot is the source of truth for "what guilds does this user share with us, and what roles do they have."

To populate the bind UI ("guilds where you're an officer and Ultros is present"), the web handler iterates the bot's known guild list (from `discord_guild`), calls `GET /guilds/{g}/members/{user_id}` via the bot's REST client for each, and filters to those where (a) the call returns 200 (user is a member) and (b) the member's role-permission bitfield includes `MANAGE_GUILD`. Results are cached per `(user_id, guild_id)` for ~5 minutes to keep modal opens cheap.

Worst case the bot is in N guilds and the call is O(N) per modal open. Ultros today is in a small number of guilds; if that ever grows past a few hundred, switch to a more scalable strategy (e.g., subscribe to `GUILD_MEMBER_UPDATE` events under the `GUILD_MEMBERS` privileged intent, or maintain a `discord_guild_member` cache). Not this spec.

### 2b.3 Binding schema

New tables:

- `discord_guild_list_binding(id, guild_id, list_id, role_id, permission, created_at, created_by_user_id)` — binds a guild role to a list permission. `role_id` is nullable; `NULL` means `@everyone` (every member of the guild). `permission` is `ListPermission` (Read or Write — not Owner; owner-grant via role would be wild).
- `discord_guild_group_binding(id, guild_id, group_id, role_id, created_at, created_by_user_id)` — binds a guild role to a group. Same nullability semantics. Permission is whatever the group is shared with on each list (separate concern).
- `discord_guild_notification(id, guild_id, channel_id, list_id, events, created_at, created_by_user_id)` — binds a guild channel to a list. `events` is a bitfield of `ListChanged | ItemAddedRemoved | InviteUsed`.

All three FK to `discord_guild(id)` with `ON DELETE CASCADE` so removing the bot tears down bindings cleanly. The `list_id` / `group_id` FKs also cascade.

Authorization invariant: to create any binding, the calling user must have `MANAGE_GUILD` on the guild (computed from the bot's `GET /guilds/{g}/members/{user_id}` call described in 2b.2, not from anything the client supplied). The Axum handler runs this check before each binding mutation; the cached result from 2b.2 is acceptable for the read path but revalidated server-side on every write.

### 2b.4 Lazy membership resolution

We don't subscribe to live member events. Instead, role membership is re-evaluated at well-defined moments, all going through the bot's REST client:

- **At login.** After the existing `Identify` callback, for each guild that has at least one binding (`discord_guild_list_binding ∪ discord_guild_group_binding`), call `GET /guilds/{guild_id}/members/{user_id}` via the bot. 404 means "not a member, no bindings apply"; 200 returns the role list. Upsert/delete the corresponding `list_shared_user` and `user_group_member` rows so the user's access matches their current roles.
- **On binding create / edit.** When an officer creates or modifies a binding, enqueue a one-shot resolution for every Ultros user we've seen log in recently. Bounded by `discord_user` rows with a recent `last_login`, not the full guild member count. (Implies adding `last_login` to `discord_user` — small, useful elsewhere too.)
- **Hourly drift sweep.** A background tokio task re-resolves users who logged in more than 24 hours ago, capped at N per minute. Nice to have, not required for shipping.

Lazy resolution intentionally accepts a "user gets promoted on Discord, doesn't see Editor permission on Ultros until they refresh / re-login" lag. The product reads as "permissions sync on login" — an acceptable mental model that saves us the privileged intent.

Iterating per-binding-guild at login is cheap (a binding-set is small per user; most users are in zero bound guilds). For users in many bound guilds, calls fan out in parallel with a per-request timeout so a slow Discord API doesn't gate the login.

### 2b.5 Notification dispatch

New events emitted by the list backend:

- `ListChanged` — list metadata (name/world/filters) edited.
- `ItemAddedRemoved` — item added to or removed from the list.
- `InviteUsed` — someone redeemed an invite link.

The existing alert delivery pipeline ([ultros/src/alerts/delivery.rs:11-18](ultros/src/alerts/delivery.rs:11), [ultros/src/alerts/delivery.rs:40-88](ultros/src/alerts/delivery.rs:40)) already handles `DiscordChannel { channel_id }` dispatch with embed rendering. We add a thin `list_event_dispatcher` that, on each event, looks up `discord_guild_notification` rows matching `list_id` and `events`, builds an embed, and hands off to the existing delivery function. No new transport.

Rate-limit and de-dup: an item bulk-add of 50 items should produce one batched embed, not 50. The dispatcher coalesces events for the same `(list_id, event_type)` over a 30-second window.

### 2b.6 UI: guild tab in `<ShareListModal />`

A fourth tab in the share modal (alongside Groups, Invite link, and the deferred People tab): **Discord guild**.

Visible only to the list owner. Content:

- A dropdown sourced from a new endpoint `GET /api/v1/discord/eligible-guilds` that returns the bot-known guilds where the calling user has `MANAGE_GUILD` (computed via 2b.2). If the list is empty, show "Invite Ultros to a Discord server where you're an admin" with the bot install URL and the minimum required permissions (`Send Messages`, `Embed Links` — guild-only, no member-list intent, no message-content intent).
- After picking a guild:
  - **Roles**: list current `discord_guild_list_binding` rows for this guild+list. Each row: role-name pill (with guild's color), permission selector (Read/Write), revoke. "Add binding" → role dropdown + permission selector. `@everyone` is one of the role choices, prefixed with an icon to differentiate.
  - **Notifications**: list current `discord_guild_notification` rows. Each row: channel name, event-type checkboxes, revoke. "Add notification" → channel dropdown (filtered to text channels the bot can post to) + event checkboxes.

### 2b.7 Groups page integration

The `/groups` page (Tier 2.3) gains the same guild-binding UI per group, scoped to `discord_guild_group_binding`. A group with a guild binding shows a Discord icon and the guild name below the group name.

### 2b.8 Bot slash command (optional, post-Tier-2b)

A `/ffxiv list` already exists ([ultros/src/discord/ffxiv/mod.rs:20-28](ultros/src/discord/ffxiv/mod.rs:20)). Extend with `/ffxiv list link` — an officer-only slash command that links the calling guild to an Ultros list by id (the user must own the list on Ultros). Convenience over the web UI; nice to have, not required for the tier.

### 2b.9 `DiscordGuildClient` trait — the seam for testability

Every cross-process call to Discord goes through one trait. The web handlers, list-event dispatcher, lazy resolver, and binding-create endpoints all depend on a `&dyn DiscordGuildClient` (or `Arc<dyn DiscordGuildClient>` for tasks), never on `serenity::Http` directly.

```rust
#[async_trait]
pub trait DiscordGuildClient: Send + Sync {
    async fn list_bot_guilds(&self) -> Result<Vec<GuildInfo>, DiscordError>;
    async fn get_guild_roles(&self, guild_id: GuildId) -> Result<Vec<RoleInfo>, DiscordError>;
    async fn get_guild_text_channels(&self, guild_id: GuildId)
        -> Result<Vec<ChannelInfo>, DiscordError>;
    async fn get_member(&self, guild_id: GuildId, user_id: UserId)
        -> Result<Option<MemberInfo>, DiscordError>; // None = 404 / not a member
    async fn send_channel_embed(&self, channel_id: ChannelId, embed: Embed)
        -> Result<(), DiscordError>;
}
```

Two implementations:

- **`SerenityGuildClient`** — production. Wraps `serenity::CacheAndHttp`. Lives next to the existing bot bootstrap in [ultros/src/discord/mod.rs](ultros/src/discord/mod.rs). Built once at startup, shared via the existing app-state pattern.
- **`MockGuildClient`** — tests. `HashMap`-backed; tests preload guilds/roles/members and assert on captured `send_channel_embed` calls. Lives in a `#[cfg(test)]` module under the same crate.

The web handlers take `Arc<dyn DiscordGuildClient>` as Axum state. The list-event dispatcher takes it as a constructor arg. The lazy resolver takes it as a function arg. No production code path constructs the serenity client directly except `main.rs`. This makes the entire Tier 2b unit-testable without a live bot.

`MemberInfo`, `RoleInfo`, `ChannelInfo`, `GuildInfo` are local structs in `ultros/src/discord/types.rs` — not serenity types. Keeps the trait's surface stable across serenity major-version bumps and prevents serenity types leaking into web handlers.

**General-pattern note (not required for shipping this spec):** the codebase has no pervasive mocking pattern today. Adopting `DiscordGuildClient` is a deliberate first beachhead; if the pattern works, it can extend to the universalis HTTP client and the FFXIV character-fetcher next. We do **not** retrofit existing untested code in this spec — only the new Tier 2b code is built behind a trait. That keeps scope honest while still leaving the codebase in a better testable state than we found it.

---

## Tier 3 — Discovery & onboarding

Goal: a new user learns Shared Lists exists without us building a tutorial.

### 3.1 `/welcome` gets a fourth step (logged-in only)

After Language, add **Step 4: Make a list** ([routes/welcome.rs:122-134](ultros-frontend/ultros-app/src/routes/welcome.rs:122)). Render only when the user is authenticated (the current `/welcome` is reached pre-login too via "set my preferences"). The step explains in one sentence what a list is, links to `/list`, and is skippable. No form on the welcome page itself — just a nudge with an icon.

For unauthenticated visitors, replace Step 4 with a "Sign in to save lists, share with friends, and get price alerts" prompt. Same icon, same slot — the welcome page picks the variant based on auth state.

### 3.2 First-list-created nudge

After `create_list` succeeds in [`EditLists`](ultros-frontend/ultros-app/src/routes/lists.rs:209) for the first time (detect: this user's `lists` length transitions 0 → 1), show a one-time toast: *"List created. You can share it with friends or a group — click the share icon any time."* Use `localStorage` to fire it at most once per user. No backend changes.

### 3.3 Share affordance on cards with items

A `ListCard` whose item count is > 0 and `viewer_permission == Owner` shows a small "Share" icon in its header. Clicking opens `<ShareListModal />` for that list. (Cards without items skip the affordance — there's nothing useful to share.)

### 3.4 Analyzer → list integration awareness

Today, the Recipe and Crafting analyzers can bulk-add ingredients to a list ([bulk_add_item_to_list](ultros-frontend/ultros-app/src/api.rs:250)) but the call sites are sparse. Audit each analyzer route ([analyzer.rs](ultros-frontend/ultros-app/src/routes/analyzer.rs), recipe analyzer, FC crafting analyzer, leve analyzer, venture analyzer) for an "Add results to a list" affordance. Where missing, add one. The dropdown filters to lists where `viewer_permission >= Write` and matches the analyzer's world/DC filter. This is mechanical and low-risk; it makes shared lists genuinely useful to a guild ("our weekly crafting run targets list").

### 3.5 Empty-state copy on `/list`

When `My Lists` is empty but `Shared With Me` has items, the empty-state copy changes from "create your first list" to "You don't own any lists yet — but {N} have been shared with you." This is a 2-line copy change in [lists.rs:248-254](ultros-frontend/ultros-app/src/routes/lists.rs:248).

---

## Backend cleanup (alongside Tier 2)

These are not optional. Tier 2 doubles the number of list-handler call sites; without these, `web.rs` becomes unreadable.

### B.1 Split `web.rs` into `web/handlers/{lists,groups,invites,...}.rs`

[ultros/src/web.rs](ultros/src/web.rs) is 1322 lines and contains handlers for at least six bounded contexts. Move handlers into submodules; keep route registration in `web.rs` so the URL map stays in one place. This is a no-behavior-change refactor — should ship as its own commit before Tier 2 handlers grow.

### B.2 `RequireListPermission` Axum extractor

Every list-write handler currently calls `get_permission()` ([ultros-db/src/lists.rs:209, 225, 246, …](ultros-db/src/lists.rs:209)) and maps the result manually. Replace with an extractor:

```rust
struct RequireListPermission<const MIN: u8>(pub ListAccess);
// implements FromRequestParts; rejects with 403 if user.perm < MIN
```

Handlers become `async fn share_list_with_user(perm: RequireListPermission<{Owner as u8}>, ...)`. Removes ~10 duplicated check sites, makes the auth contract visible at the function signature.

### B.3 Scope WebSocket broadcasts by list id

[ultros-api-types/src/websocket.rs:134](ultros-api-types/src/websocket.rs:134) defines `ListUpdate` with no addressing — every client gets every update. Add a per-connection subscription set on the server (the client already calls `subscribe_to_list` at [list_view.rs:88](ultros-frontend/ultros-app/src/routes/list_view.rs:88)); only forward `ListUpdate` events whose `list_id` is in the connection's subscribed set. Existing client API doesn't change; server filtering is added. Fixes the unbounded fan-out and prevents leaking other users' list change notifications (currently *every* list change is broadcast to *every* connected client — minor info leak today, real one once groups exist).

### B.4 Drop `let _ = …` on broadcast sends

[ultros/src/web.rs:602-606](ultros/src/web.rs:602) ignores broadcast send errors with `let _ =`. After B.3 the broadcaster has bounded fan-out and the only legitimate failure is "channel closed" (server shutting down). Log the failure at `warn!` rather than discarding — silent failures here mean a client missed a delete and shows stale UI until refresh.

---

## Architecture & data flow

```
Owner clicks "Share"
   └─→ <ShareListModal /> opens (Tier 2.1)
        ├─→ People tab        (deferred — uses invite link instead)
        ├─→ Groups tab        POST /list/{id}/share/group
        └─→ Invite link tab   POST /list/{id}/invite/create
                              ─ user copies https://ultros.app/list/invite/{id}
                              ─ shares via Discord/etc.

Recipient opens invite URL
   └─→ /list/invite/{id}      (Tier 2.2)
        ├─ not logged in → cookie + OAuth → return
        └─ logged in → POST /invite/{id}/use
                       → 200 OK → redirect /list/{list_id}
                       → server emits ListUpdate (Tier B.3: scoped to subscribers of list_id)
                       → owner's open tab refreshes share count

Officer binds a guild role             (Tier 2b)
   └─→ <ShareListModal /> "Discord" tab
        ├─ GET /api/v1/discord/eligible-guilds
        │     └─ DiscordGuildClient.list_bot_guilds() ⨯ get_member(g, user)
        │        filtered to MANAGE_GUILD = "officer in a bot-present guild"
        ├─ pick role + permission → POST /api/v1/list/{id}/guild-binding
        │     └─ enqueue lazy resolve for recently-active Ultros users
        └─ pick channel + events → POST /api/v1/list/{id}/guild-notification

User logs in
   └─→ /oauth callback (Identify only — no scope change)
        ├─ existing flow: upsert discord_user, set session cookie
        └─ new (Tier 2b.4): for each guild with at least one binding,
                            DiscordGuildClient.get_member(g, user_id)
                            → reconcile list_shared_user + user_group_member
                              rows against current roles

List event fires (item added, edited, invite used)
   └─→ list_event_dispatcher (Tier 2b.5)
        ├─ look up discord_guild_notification rows
        ├─ coalesce within 30s window
        └─ delivery.rs → DiscordChannel { channel_id } → embed
```

State on the recipient side:

- Their `get_lists()` now returns the new list (via `list_shared_user` join, already wired).
- It appears under "Shared With Me" with a badge and the owner's name.
- `add_to_list` dropdowns pick it up automatically.

## Error handling

- **Modal submission fails** → toast with server error message; modal stays open. Same pattern as the existing `edit_list` flow.
- **Invite expired or revoked** → recipient lands on a dedicated "This invite is no longer valid" page (Tier 2.2). Owner sees zero remaining uses in the invite list and a "Revoked" tag.
- **Permission mismatch on `add_to_list`** → after Tier 1.5, read-only lists are non-selectable so the server-side 403 should not fire; if it does (race with a revoke), show the error toast and refresh the list cache.
- **Group deletion with active shares** → backend already cascades; UI confirmation explicitly says "This group is shared on N lists; those shares will be removed."

## Testing

- **Unit**: `RequireListPermission` extractor (B.2) — happy path per permission level + 403 for insufficient.
- **Unit**: scoped broadcast filter (B.3) — subscribed clients receive, non-subscribed do not.
- **Unit**: lazy role resolver (2b.4) — given a mocked `GET /guilds/{g}/members/{u}` response, produces the expected `list_shared_user` upsert/delete set.
- **Unit**: event coalescer (2b.5) — N rapid `ItemAddedRemoved` events for the same list collapse to one embed.
- **Integration / e2e**: the existing Puppeteer harness in [integration/](integration/) gains:
  - **Invite redemption**: log in as user A, create list, generate invite, redeem as user B (via test-auth login flow added in commit 1cbf5b4c), assert list appears in B's `/list` with "Shared · Editor" pill.
  - **Guild bind happy path**: the e2e server is started with `MockGuildClient` as the `DiscordGuildClient` Axum-state binding instead of the serenity-backed one. Preload it with a fake guild + roles + a fake member. Assert that creating a binding + logging in user B reconciles their permissions, and that an item-add fires a captured `send_channel_embed` call on the mock. The real bot never runs in CI.
- **Visual**: screenshots of `/list` with owned-only, shared-only, and mixed states; `<ShareListModal />` Groups/Invite/Discord tabs; `/groups` page.

## Roadmap / explicitly deferred

Sketched for context; not in scope:

- **User search** for a People tab in `<ShareListModal />`. Needs a `/api/v1/user/search` endpoint and rate limiting. Re-evaluate after Tier 2 ships if invite-link sharing turns out to be friction.
- **Notifications.** "X shared a list with you" in a bell-icon dropdown. Wants a notification table, read/unread state, optionally Discord DM delivery. Whole separate spec.
- **Public lists / discoverable lists.** "Top crafting lists this week." Different content model, different moderation problem.
- **Per-item permission** (e.g., one user can mark items bought, another can only view). Likely YAGNI for the FFXIV use case.
- **Transfer ownership.** Rare; can be done with a backend script for now.
- **Mobile UX pass** on the share modal and groups page.

## Open questions for plan-writing

- How do non-owner members get added to a group? Invite-link analog vs. auto-add on list-invite-redemption-targeting-group vs. guild-backed (Tier 2b). All three can coexist; planning picks the minimum for Tier 2 ship.
- Does `viewer_permission` go on the `List` DTO or a sibling `ListWithViewerContext` DTO? *Decided: on `List` as `Option<ListPermission>`, populated by `get_lists_for_user`.*
- Should "Leave list" require a confirm modal? Probably yes; cheap to add.
- **Tier 2b**: does the bot need any additional Discord OAuth permissions at install time beyond `Send Messages + Embed Links`? `View Channels` is implicit; `Read Message History` is not needed. Pin during planning.
- **Tier 2b**: bot install URL — generate per-deploy or hard-code the application id? Hard-code is fine; the bot's application id is public.
- **Tier 2b**: how do we handle a guild where the bot was removed but bindings still exist? Lazy resolution stops syncing (the REST call fails); UI shows a "bot removed from this guild" warning on the binding row with a re-invite link. Don't auto-delete bindings — the officer may re-add the bot.
- **Tier 2b**: `eligible-guilds` cache TTL — start at 5 minutes. If users complain that newly-added bots don't appear in the picker, drop to 1 minute or add a "refresh" button.
- **Tier 2b**: should `DiscordGuildClient` be a workspace-level trait (in `ultros-api-types` or a new `ultros-discord` crate) or stay private to the `ultros` binary? Lean private — only one consumer today. Promote if a second crate ever needs it.
