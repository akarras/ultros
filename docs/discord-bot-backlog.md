# Discord Bot — Product Backlog

Owner: Discord bot product surface
Source audit: 2026-05-12 (worktree `tender-golick-395399`)
Last status review: 2026-09-23 — every ticket re-checked against the code; line links refreshed.

Priorities use `P0`/`P1`/`P2`/`P3` (P0 = ship next, P3 = nice-to-have).
Effort is a rough T-shirt size (`S` ≤ half-day, `M` 1–2 days, `L` ≥ 3 days).

The bot lives in [ultros/src/discord/](ultros/src/discord/). Registered root commands
([mod.rs:185](ultros/src/discord/mod.rs:185)) are `age`, `register`, `ping` and `ffxiv`; the `ffxiv`
subgroups ([ffxiv/mod.rs:22](ultros/src/discord/ffxiv/mod.rs:22)) are `character`, `retainer`,
`analyze`, `list`, `prices`, `rescan_market` (owners only) and `alert`.

## Status at a glance (2026-09-23)

| Ticket | Status |
| --- | --- |
| B-1.1 Retainer claim gating | Shipped (world-level filter); per-character follow-up open |
| B-1.2 DB-layer list ownership | **Shipped** via Lists 2 |
| B-1.3 Alert ownership semantics | Partial (dead endpoints disabled; no sweep, no ownership line) |
| B-1.4 Bot copy still says "verified" | New · open |
| B-2.1 List autocomplete | Partial (`remove_item` item name still free text) |
| B-2.2 Scope autocomplete on `list create` | Open |
| B-2.3 Analyze defaults | Shipped |
| B-2.4 Subgroup descriptions | Partial (root `ffxiv` still says "Hello world"; many commands undocumented) |
| B-2.5 Command naming | Open (now also snake_case vs kebab-case) |
| B-2.6 Registration timeout feedback | Partial |
| B-2.7 Hide `/register` | Open, low value |
| B-2.8 `/help` not registered | New · open |
| B-2.9 `list remove_item` never replies | New · open |
| B-3.1 `/bot` page | Static v1 shipped; drifted; auto-generation open |
| B-3.2 Userguide chapter | Open |
| B-3.3 README bot mention | Shipped (bullets) |
| B-4.1 Alerts-page banner | Shipped |
| B-4.2 Item-page chip | **Removed** in the item-page refactor |
| B-4.3 Retainer pages bot tips | Partial (logged-out empty states only) |
| B-4.4 Settings character CTA | Partial |
| B-4.5 Home-page bot CTA | Shipped (home hero) |
| B-4.6 Lists-page bot mention | Open |
| B-5.1 Web character verification | Obsolete |
| B-5.2 Per-guild config | Open |
| B-5.3 Price-drop alerts | **Shipped** as `/ffxiv alert price` |
| B-5.4 Saved-search shortcuts | Open for the bot |
| B-5.5 Message context menu | Open |

---

## Epic 1 — Security & data integrity

The single most important epic. Retainer-ownership originally had no proof of ownership; lists trusted the handler layer. Both are addressed now; what remains is tightening and copy.

### [shipped 2026-05-12] B-1.1 · Gate retainer claims on verified character ownership · P0 · L
**Problem.** `/ffxiv retainer add` calls [`register_retainer`](ultros-db/src/retainers.rs:90) which only checked that the retainer row exists. Any Discord user could claim any retainer ID returned by the global autocomplete in [retainer.rs:71](ultros/src/discord/ffxiv/retainer.rs:71). This polluted "my retainers" semantics and let users register undercut alerts on retainers they don't own.

**Proposal.**
1. Require `/ffxiv character register` first; persist the character → Discord-user link.
2. Change the `add` autocomplete to only return retainers belonging to one of the caller's characters. The `owned_retainers.character_id` column exists but is not set on claim ([retainers.rs:117](ultros-db/src/retainers.rs:117)) — set it on registration.
3. ~~Add a unique constraint on `(retainer_id, discord_id)` to block double-claims.~~ Already present since the initial migration ([m20220101_000001_create_table.rs:249](migration/src/m20220101_000001_create_table.rs:249)); it blocks one user claiming twice. Cross-user exclusivity no longer applies — see below.
4. ~~Migration: flag unverified claims and DM owners to re-verify via Lodestone.~~ Obsolete: verification was dropped (#1061).

**Shipped behavior (2026-05-12).** Shipped as a **world-level filter**: autocomplete and the `add` gate accept retainers whose `world_id` matches one of the caller's characters' worlds ([`get_retainers_for_user_characters`](ultros-db/src/retainers.rs:66), used at [retainer.rs:71](ultros/src/discord/ffxiv/retainer.rs:71) and [retainer.rs:320](ultros/src/discord/ffxiv/retainer.rs:320)). Stricter than the prior global filter, broader than the original "≤ caller's character's retainer count" criterion.

**Status 2026-09-23.** Since #1061 character claims are authless and non-exclusive (several accounts can add the same character, migration `m20260802_000001_authless_character_claims`), so the cross-user "conflict" acceptance criterion no longer applies. Remaining follow-up: backfill `owned_retainers.character_id` on claim. Today it is only set by the separate web action [`assign_retainer_character`](ultros/src/web.rs:3424); neither the bot nor `ultros-character/src/character_claim.rs` sets it.

### [shipped] B-1.2 · DB-layer ownership checks on list mutations · P1 · M
**Problem.** [`/ffxiv list remove`](ultros/src/discord/ffxiv/lists.rs:298), [`add_item`](ultros/src/discord/ffxiv/lists.rs:325), [`remove_item`](ultros/src/discord/ffxiv/lists.rs:369) trusted the handler-layer Discord-ID filter.

**Shipped (Lists 2, migration `m20260907_000001_list_doc`).** `delete_list` takes the user and returns `ListError::Forbidden` for anyone but the owner ([ultros-db/src/lists.rs:214](ultros-db/src/lists.rs:214)). Item edits go through [`edit_list_doc`](ultros-db/src/list_doc.rs:424) / [`apply_list_update`](ultros-db/src/list_doc.rs:396), which require write permission; sharing and invite mutators check ownership too. The bot's `add_item`/`remove_item` write via `ListSync::edit_as_server` ([ultros-lists/src/sync.rs:69](ultros-lists/src/sync.rs:69)). The error is named `Forbidden`, not `NotOwned`; the retainer-side analogue is [`remove_owned_retainer`](ultros-db/src/retainers.rs:154).

Clean-up: the `Data.list_sync` comment in [mod.rs:24](ultros/src/discord/mod.rs:24) ("No bot command reads this yet", `allow(dead_code)`) is stale — `add_item`/`remove_item` use it.

### B-1.3 · Audit alert ownership semantics · P2 · S · partial
**Problem.** `remove_undercut_alert` is keyed `(channel_id, owner)` — fine, but the model is ambiguous: alerts live "in a channel" but are owned by a user. If the user leaves the guild, the alert stays.

**Proposal.** Document the lifecycle. Add a periodic sweep that removes alerts where the bot can no longer DM the user OR no longer sees them in the channel's guild. Surface alert ownership in `/ffxiv retainer add_undercut_alert` confirmation.

**Status 2026-09-23.** Partly covered by a different mechanism: delivery now disables a notification endpoint on permanent Discord failures (unknown channel/user, missing access, DMs closed, missing permissions — [ultros-alerts/src/delivery.rs:268](ultros-alerts/src/delivery.rs:268), migration `m20260809_000001_notification_endpoint_health`). The alert rows themselves stay, there is no guild-membership sweep (`GuildMemberRemoval` only feeds group sync, [mod.rs:131](ultros/src/discord/mod.rs:131)), and the confirmation at [retainer.rs:130](ultros/src/discord/ffxiv/retainer.rs:130) still doesn't show ownership. Lifecycle doc still missing.

### B-1.4 · Drop "verified" / Lodestone wording from bot copy · P2 · S · new
**Problem.** After #1061 there is no verification step, but the retainer group embed and the `add` gate error still talk about "verified" characters and a Lodestone challenge ([retainer.rs:33](ultros/src/discord/ffxiv/retainer.rs:33), [retainer.rs:336](ultros/src/discord/ffxiv/retainer.rs:336)).

**Acceptance.** Copy says "add your character with `/ffxiv character register`" and nothing about verification.

---

## Epic 2 — Command UX polish

The bot works but is unforgiving to first-time users. The `prices` and `alert` groups are the bar; bring everything else up to it.

### [shipped 2026-05-12 — partial: list-name on all, item-name on add_item only] B-2.1 · Autocomplete for list-name and item-name in `/ffxiv list *` · P1 · M
**Problem.** [lists.rs:325](ultros/src/discord/ffxiv/lists.rs:325), [lists.rs:369](ultros/src/discord/ffxiv/lists.rs:369), [lists.rs:403](ultros/src/discord/ffxiv/lists.rs:403) took raw strings with exact-match lookup.

**Proposal.**
- `list_name` → autocomplete from the caller's own lists. *(Done: [`autocomplete_list_name`](ultros/src/discord/ffxiv/lists.rs:115), including the share/invite commands.)*
- `item_name` for `add_item` → *(Done: `autocomplete_item_name_global`, [lists.rs:131](ultros/src/discord/ffxiv/lists.rs:131). A shared item autocomplete also lives in [helpers.rs:185](ultros/src/discord/ffxiv/helpers.rs:185).)*
- `item_name` for `remove_item` → autocomplete from items already on the named list. **Still open** ([lists.rs:374](ultros/src/discord/ffxiv/lists.rs:374)).

**Acceptance.** All three params show suggestions; exact-match fallback retained for power users typing fast.

### B-2.2 · Autocomplete for `region_datacenter_or_world` · P1 · S
**Problem.** `/ffxiv list create` ([lists.rs:264](ultros/src/discord/ffxiv/lists.rs:264)) requires the user to type "Aether" / "Crystal" / "Faerie" exactly.

**Proposal.** Reuse the world cache; suggest regions first, then DCs, then worlds, with each prefixed by its type (e.g. `🌐 Aether (Datacenter)`, `🌍 Faerie (World)`). [`autocomplete_world`](ultros/src/discord/ffxiv/item_prices.rs:32) is a starting point but has no type prefixes.

### [shipped 2026-05-12] B-2.3 · Defaults for `/ffxiv analyze profit` · P1 · S
**Problem.** Four required numeric parameters with no defaults ([analyze.rs:31](ultros/src/discord/ffxiv/analyze.rs:31)).

**Shipped.** `minimum_profit` (10_000), `number_recently_sold` (5), `threshold_days` (7) are optional; the group embed documents them ([analyze.rs:13](ultros/src/discord/ffxiv/analyze.rs:13)).

### [partial] B-2.4 · Fill placeholder subgroup descriptions · P2 · S
**Problem.** Group bodies and command descriptions showed `"Hello world"` or placeholders.

**Status 2026-09-23.** The `character`, `retainer` and `analyze` group bodies now reply with real embeds ([character.rs:5](ultros/src/discord/ffxiv/character.rs:5), [retainer.rs:27](ultros/src/discord/ffxiv/retainer.rs:27), [analyze.rs:12](ultros/src/discord/ffxiv/analyze.rs:12)). Still open: the root `ffxiv` replies "Hello world" ([ffxiv/mod.rs:36](ultros/src/discord/ffxiv/mod.rs:36)), and many commands have no `///` doc comment, so Discord shows no description — `ffxiv`, the `character`/`retainer`/`analyze`/`list` groups, `character register`, `retainer add_undercut_alert`/`remove_undercut_alert`, `analyze profit`, `list show_list`, and the root `ping`/`age`/`register`.

**Acceptance.** Every command in the tree has a one-sentence description.

### B-2.5 · Standardize command naming · P3 · S
**Problem.** `check_listings` vs `list` vs `show_list` for conceptually similar verbs ([retainer.rs:19](ultros/src/discord/ffxiv/retainer.rs:19), [lists.rs:22](ultros/src/discord/ffxiv/lists.rs:22)). Since the `alert` group landed, casing is inconsistent too: `alert` uses kebab-case (`list-subscribe`, `endpoint-list`, …) while `retainer`/`list` use snake_case. The `list create` reply also points at a `list view` command that doesn't exist ([lists.rs:289](ultros/src/discord/ffxiv/lists.rs:289)).

**Proposal.** Pick one casing. Rename plan: `check_listings` → `listings`, `show_lists` → `lists`, `show_list` → `show`. Keep aliases for one release. Document in the changelog.

### [partial] B-2.6 · User feedback on character-registration timeout · P2 · S
**Problem.** Character-registration dropdown has a 5-minute timeout ([character.rs:61](ultros/src/discord/ffxiv/character.rs:61)).

**Status 2026-09-23.** No longer silent — a new "No choice selected" message is posted ([character.rs:90](ultros/src/discord/ffxiv/character.rs:90)) — but the original select menu is neither edited nor disabled.

**Acceptance.** On timeout, edit the original message to "Registration timed out — re-run `/ffxiv character register` to try again." and remove the menu.

### B-2.7 · Move `/register` out of user-facing root · P3 · S
**Problem.** [`mod.rs:76`](ultros/src/discord/mod.rs:76) is prefix-only with no `hide_in_help`. Mostly moot while `/help` is unregistered (B-2.8).

**Proposal.** Add `hide_in_help = true` (and `owners_only`) or move under an `/admin` group.

### B-2.8 · Register `/help` · P2 · S · new
**Problem.** `help` is defined ([mod.rs:40](ultros/src/discord/mod.rs:40)) but not in the framework's command list ([mod.rs:185](ultros/src/discord/mod.rs:185)), so there is no `/help` at all — several tickets above assume one.

**Acceptance.** `/help` lists the command tree with descriptions (depends on B-2.4 for useful output).

### B-2.9 · `/ffxiv list remove_item` never replies · P1 · S · new
**Problem.** On success [`remove_item`](ultros/src/discord/ffxiv/lists.rs:369) neither defers nor replies, so Discord shows "The application did not respond" even though the item was removed.

**Acceptance.** Success replies with a confirmation, matching `add_item`.

---

## Epic 3 — Documentation & discovery

### [shipped 2026-05-12 — static v1; auto-generation is a follow-up] B-3.1 · Build `/bot` page in the frontend · P0 · L
**Problem.** No documentation anywhere; users discovered commands by typing `/` and hoping.

**Proposal.** Add a frontend route at `/bot` with three sections:
1. **Invite** — large button to the existing [`/invitebot`](ultros/src/web.rs:1498) endpoint.
2. **Command reference** — grouped by subgroup, auto-generated from the poise command tree at build time so it can't drift.
3. **Getting started** — register character → claim retainers → set undercut alert.

**Acceptance.** Page renders from a generated JSON manifest checked in alongside the bot code; CI fails if the manifest is out of sync with the command tree.

**Status 2026-09-23.** The static page is live ([routes/bot.rs](ultros-frontend/ultros-app/src/routes/bot.rs), side-nav entry) but has **drifted**, exactly as feared: it omits the whole `/ffxiv alert` group (11 subcommands) and `list share_user`/`create_invite`/`redeem_invite`. No manifest, generator or CI check exists yet. Until one does, update the page by hand in the same change as any command.

### B-3.2 · Add Discord bot chapter to userguide · P2 · M
**Problem.** [userguide/src](userguide/src/) has no bot chapter; the only bot page is `retainers/alerts.md`, which covers undercut alerts only.

**Proposal.** New chapter mirroring the frontend `/bot` page, narrative-style (not just a command dump), including the `alert` group.

### [shipped] B-3.3 · README bot section · P3 · S
**Status 2026-09-23.** Covered by bullets rather than a section: "**Discord bot** → ultros.app/bot" ([README.md:24](README.md)) and "Discord bot setup" ([README.md:49](README.md)). Good enough; close unless a fuller section is wanted.

---

## Epic 4 — Web ↔ Discord cross-promotion

### [shipped 2026-05-12] B-4.1 · Alerts-page Discord banner · P1 · S
**Location.** [ultros-frontend/ultros-app/src/routes/alerts.rs](ultros-frontend/ultros-app/src/routes/alerts.rs)
**Treatment.** Inline tip showing `/ffxiv retainer add_undercut_alert` / `add_sale_alert` with a link to `/bot#getting-started`. The copy-to-clipboard button from the original spec was not built.

### [removed] B-4.2 · Item-page Discord chip · P1 · S
Shipped 2026-05-12, then deliberately removed by the item-page refactor to put market data first (`docs/superpowers/plans/2026-08-29-item-page-refactor.md`). Don't reintroduce without revisiting that decision.

### [partial] B-4.3 · Retainer pages bot tips · P2 · S
**Location.** `/retainers/listings`, `/retainers/undercuts`.
**Treatment.** Banner with copyable commands `/ffxiv retainer check_listings`, `/ffxiv retainer check_undercuts`.
**Status 2026-09-23.** Only the logged-out empty states link to `/bot` ([routes/retainers.rs](ultros-frontend/ultros-app/src/routes/retainers.rs), [routes/edit_retainers.rs](ultros-frontend/ultros-app/src/routes/edit_retainers.rs)); logged-in users see no tip.

### [partial] B-4.4 · Settings "Connect your character" CTA · P2 · M
**Problem.** A user who logs in via Discord OAuth has no on-rails path to adding a character, so retainer grouping goes undiscovered.
**Treatment.** On profile/settings, if the user has no characters, show a CTA: *"Add your FFXIV character to organize your retainers."* Since #1061 the claim itself is one click from either the web profile or `/ffxiv character register` — this ticket is purely about discoverability.
**Status 2026-09-23.** `/profile` shows a plain "No characters added yet" empty state next to the add button ([routes/settings.rs](ultros-frontend/ultros-app/src/routes/settings.rs)); it doesn't mention retainers and nothing surfaces it elsewhere.

### [shipped] B-4.5 · Home-page bot invite CTA · P3 · S
Shipped in the home hero as a "Discord Bot" link to `/bot` ([routes/home_page.rs](ultros-frontend/ultros-app/src/routes/home_page.rs)) rather than next to the footer community link.

### B-4.6 · Lists-page bot mention · P3 · S
**Location.** `/list/:id`.
**Treatment.** Footer link to `/ffxiv list add_item`. Lower priority still: the web list UI (Lists 2, including device lists) is materially better than the Discord one — don't push users to the worse experience.

---

## Epic 5 — Capability expansion (stretch)

Triage these against real demand before building.

### B-5.1 · Web-based character verification · ~~P2 · L~~ · obsolete
Resolved by dropping verification entirely (#1061). Discord OAuth already
establishes who the user is, and a character claim only groups that user's own
retainers, so both the web and the bot now claim directly with no Lodestone
challenge.

### B-5.2 · Per-guild bot config command · P2 · M
`/ffxiv config set default_world <world>` so users in a guild don't have to specify `world` on every command. Stored per-guild, with per-user override. Nearest existing behavior: `/ffxiv alert price` falls back to the caller's character world ([helpers.rs:242](ultros/src/discord/ffxiv/helpers.rs:242)).

### [shipped] B-5.3 · Item-watch alerts (price drops) · P3 · M
Shipped as `/ffxiv alert price item:<X> price:<Z> [hq] [world/dc/region] [cooldown]` ([ffxiv/alert.rs:47](ultros/src/discord/ffxiv/alert.rs:47)), plus `list`, `mute`/`unmute`, `remove`, `list-subscribe`, `list-updates`, endpoint management and webhooks. Backed by the same threshold alerts as the web `/alerts` page (`ultros-alerts`, `docs/price-alerts.md`). Missing from the `/bot` page and userguide (B-3.1, B-3.2).

### B-5.4 · Saved-search shortcuts · P3 · M
Let users save a `/ffxiv analyze profit` query as a named macro and re-run it with one command. Pairs well with B-5.2. The web analyzers now have saved views (`ultros_ui_query` saved views); the bot has nothing equivalent.

### B-5.5 · Message-context menu: "Look up item in this message" · P3 · M
Right-click a Discord message containing an item name → bot replies with current prices. Discoverable through Discord's native UI. No context-menu commands exist yet.

---

## Recommended sequencing

The original first slice (B-1.1, B-3.1, B-2.1, B-2.3, B-4.1, B-4.2, B-2.4) has landed, apart from the gaps noted above. Suggested next slice:

1. **B-2.9** (`remove_item` reply) — user-visible bug, one-line fix.
2. **B-1.4**, **B-2.4** (stale "verified" copy, "Hello world", missing descriptions) — cheap, and prerequisites for B-2.8.
3. **B-2.8** (register `/help`).
4. **B-3.1** follow-up — add the `alert` and list-sharing commands to `/bot`, then the manifest + CI check so it stops drifting.
5. **B-2.1** remainder and **B-2.2** (autocomplete gaps).

After that, re-triage based on what users actually ask for. The Epic 5 items remain speculative.
