# Discord Bot — Product Backlog

Owner: Discord bot product surface
Source audit: 2026-05-12 (worktree `tender-golick-395399`)

Priorities use `P0`/`P1`/`P2`/`P3` (P0 = ship next, P3 = nice-to-have).
Effort is a rough T-shirt size (`S` ≤ half-day, `M` 1–2 days, `L` ≥ 3 days).

---

## Epic 1 — Security & data integrity

The single most important epic. Retainer-ownership currently has no proof of ownership; lists trust the handler layer. Fix the security gap first, then tighten the ownership story everywhere.

### [shipped 2026-05-12] B-1.1 · Gate retainer claims on verified character ownership · P0 · L
**Problem.** `/ffxiv retainer add` calls [`register_retainer`](ultros-db/src/retainers.rs:49) which only checks that the retainer row exists. Any Discord user can claim any retainer ID returned by the global autocomplete in [retainer.rs:63](ultros/src/discord/ffxiv/retainer.rs:63). This pollutes "my retainers" semantics and lets users register undercut alerts on retainers they don't own.

**Proposal.**
1. Require `/ffxiv character register` first; persist the verified character → Discord-user link.
2. Change the `add` autocomplete to only return retainers belonging to one of the caller's verified characters. The `owned_retainers.character_id` column already exists at [retainers.rs:76](ultros-db/src/retainers.rs:76) and is currently always null — set it on registration.
3. Add a unique constraint on `(retainer_id, discord_id)` to block double-claims.
4. Migration: leave existing (unverified) claims in place but flag them; print a one-time DM nudging owners to re-verify via Lodestone.

**Acceptance.**
- Calling `add` without a verified character returns a helpful error pointing at `/ffxiv character register`.
- Autocomplete for `add` returns ≤ the caller's character's retainer count.
- Trying to claim a retainer that already has an owner row for a different user returns an explicit conflict error.
- Existing tests still pass; new test covers cross-user claim attempt.

### B-1.2 · DB-layer ownership checks on list mutations · P1 · M
**Problem.** [`/ffxiv list remove`](ultros/src/discord/ffxiv/lists.rs:89), [`add_item`](ultros/src/discord/ffxiv/lists.rs:120), [`remove_item`](ultros/src/discord/ffxiv/lists.rs:160) trust the handler-layer Discord-ID filter. A future refactor that bypasses that filter would silently expose lists.

**Proposal.** Push the `WHERE discord_id = ?` check into the DB methods themselves (mirror the pattern in [`remove_owned_retainer`](ultros-db/src/retainers.rs:111) which returns an explicit "you do not own this" error).

**Acceptance.** All list-mutation DB methods take `discord_user_id` and return `NotOwned` when it doesn't match. Web routes that touch lists must thread the user ID through.

### B-1.3 · Audit alert ownership semantics · P2 · S
**Problem.** `remove_undercut_alert` is keyed `(channel_id, owner)` — fine, but the model is ambiguous: alerts live "in a channel" but are owned by a user. If the user leaves the guild, the alert stays.

**Proposal.** Document the lifecycle. Add a periodic sweep that removes alerts where the bot can no longer DM the user OR no longer sees them in the channel's guild. Surface alert ownership in `/ffxiv retainer add_undercut_alert` confirmation embed.

**Acceptance.** Sweep job runs hourly, logs removed alerts; confirmation embed shows "this alert is owned by @you and fires in #this-channel."

---

## Epic 2 — Command UX polish

The bot works but is unforgiving to first-time users. The `prices` group is the gold standard; bring everything else up to that bar.

### [shipped 2026-05-12 — partial: list-name on all four, item-name on add_item only] B-2.1 · Autocomplete for list-name and item-name in `/ffxiv list *` · P1 · M
**Problem.** [lists.rs:120](ultros/src/discord/ffxiv/lists.rs:120), [lists.rs:160](ultros/src/discord/ffxiv/lists.rs:160), [lists.rs:192](ultros/src/discord/ffxiv/lists.rs:192) take raw strings with exact-match lookup. Users must type list names from memory and items character-perfect.

**Proposal.**
- `list_name` → autocomplete from the caller's own lists.
- `item_name` for `add_item` → reuse [`autocomplete_item`](ultros/src/discord/ffxiv/item_prices.rs:19) from prices.
- `item_name` for `remove_item` → autocomplete from items already on the named list.

**Acceptance.** All three params show suggestions; exact-match fallback retained for power users typing fast.

### B-2.2 · Autocomplete for `region_datacenter_or_world` · P1 · S
**Problem.** `/ffxiv list create` ([lists.rs:55](ultros/src/discord/ffxiv/lists.rs:55)) requires the user to type "Aether" / "Crystal" / "Faerie" exactly.

**Proposal.** Reuse the world cache; suggest regions first, then DCs, then worlds, with each prefixed by its type (e.g. `🌐 Aether (Datacenter)`, `🌍 Faerie (World)`).

### [shipped 2026-05-12] B-2.3 · Defaults for `/ffxiv analyze profit` · P1 · S
**Problem.** Four required numeric parameters with no defaults ([analyze.rs:17](ultros/src/discord/ffxiv/analyze.rs:17)).

**Proposal.** Make `minimum_profit` (10_000), `number_recently_sold` (5), `threshold_days` (7) optional. Only `world` stays required. Add an example in the command description.

### [partial — retainer and analyze parents shipped] B-2.4 · Fill placeholder subgroup descriptions · P2 · S
**Problem.** [`character.rs:5`](ultros/src/discord/ffxiv/character.rs:5), [`retainer.rs:25`](ultros/src/discord/ffxiv/retainer.rs:25), [`analyze.rs:12`](ultros/src/discord/ffxiv/analyze.rs:12) all show `"Hello world"` or placeholders. These render in `/help` output.

**Acceptance.** Every command in the tree has a one-sentence description that reads cleanly in `/help`.

### B-2.5 · Standardize command naming · P3 · S
**Problem.** `check_listings` vs `list` vs `show_list` for conceptually similar verbs. Inconsistent and tutorial-hostile.

**Proposal.** Rename plan: `check_listings` → `listings`, `show_lists` → `lists`, `show_list` → `show`. Keep aliases for one release. Document in CHANGELOG.

### B-2.6 · User feedback on character-registration timeout · P2 · S
**Problem.** Character-registration dropdown has a 5-minute timeout with silent failure ([character.rs](ultros/src/discord/ffxiv/character.rs)).

**Acceptance.** On timeout, edit the original message to "Registration timed out — re-run `/ffxiv character register` to try again."

### B-2.7 · Move `/register` out of user-facing root · P3 · S
**Problem.** [`mod.rs:68`](ultros/src/discord/mod.rs:68) is owner-only but lives next to `/help` and `/ping`. Confusing in `/help`.

**Proposal.** Hide from `/help` output (poise supports `hide_in_help = true`) or move under an `/admin` group.

---

## Epic 3 — Documentation & discovery

The bot has zero end-user documentation today. Build it once, keep it auto-generated.

### B-3.1 · Build `/bot` page in the frontend · P0 · L
**Problem.** No documentation anywhere; users discover commands by typing `/` and hoping. README only mentions Discord OAuth, not the bot. [userguide/src](userguide/src/) has no chapter.

**Proposal.** Add a frontend route at `/bot` (or `/discord`) with three sections:
1. **Invite** — large button to existing [`/invitebot`](ultros/src/web.rs:1269) endpoint + screenshot of the auth screen.
2. **Command reference** — grouped by subgroup, auto-generated from the poise command tree at build time so it can't drift. Each command: signature, description, example, screenshot of resulting embed.
3. **Getting started** — three-step onboarding: register character → claim retainers → set undercut alert.

**Acceptance.** Page renders from a generated JSON manifest checked in alongside the bot code; CI fails if the manifest is out of sync with the command tree.

### B-3.2 · Add Discord bot chapter to userguide · P2 · M
**Problem.** [userguide/src](userguide/src/) covers analyzer, character, currency, lists, retainers — but nothing on the bot.

**Proposal.** New chapter mirroring the frontend `/bot` page, narrative-style (not just a command dump).

### B-3.3 · README bot section · P3 · S
**Problem.** README references Discord OAuth only ([README.md:64-65, 93-95](README.md)).

**Proposal.** Add a "Discord bot" section between the existing OAuth and dev-setup sections, linking to `/bot`.

---

## Epic 4 — Web ↔ Discord cross-promotion

Currently the web app and the bot are siloed. The user can hit the same intent on both surfaces but neither tells them about the other.

### B-4.1 · Alerts-page Discord banner · P1 · S
**Location.** [ultros-frontend/ultros-app/src/routes/alerts.rs](ultros-frontend/ultros-app/src/routes/alerts.rs)
**Treatment.** Inline tip near the delivery-method selector: *"Prefer Discord-native? Run `/ffxiv retainer add_undercut_alert` in any channel where the bot is installed."* Copy-to-clipboard button on the command string.
**Why this one first.** Highest intent overlap — users on this page are already configuring alerts.

### B-4.2 · Item-page Discord chip · P1 · S
**Location.** Item-detail pages (`/item/:world/:id`).
**Treatment.** Small chip showing the slash command to look up the same item: `/ffxiv prices current item:<Name> world:<World>` — pre-filled, copyable.
**Why.** Lowest effort, broadest reach (every item page).

### B-4.3 · Retainer pages bot tips · P2 · S
**Location.** `/retainers/listings`, `/retainers/undercuts`.
**Treatment.** Banner with copyable commands `/ffxiv retainer check_listings`, `/ffxiv retainer check_undercuts`.

### B-4.4 · Settings "Connect your character" CTA · P2 · M
**Problem.** A user who logs in via Discord OAuth has no on-rails path to character verification. The only path today is the Discord slash command, which is undiscoverable from the web.
**Treatment.** On profile/settings, if `verified_characters` is empty, show a CTA: *"Connect your FFXIV character to unlock retainer features."* Two paths: in-browser Lodestone flow (new — see B-5.1) or "register via Discord" with the slash-command snippet.

### B-4.5 · Home-page bot invite CTA · P3 · S
**Location.** [ultros-frontend/ultros-app/src/lib.rs:116](ultros-frontend/ultros-app/src/lib.rs) (already links to Discord community).
**Treatment.** Add an "Add the bot to your server" button next to the existing community link.

### B-4.6 · Lists-page bot mention · P3 · S
**Location.** `/list/:id`.
**Treatment.** Footer link to `/ffxiv list add_item`. Lower priority because the web list UI is materially better than the Discord one — don't push users to the worse experience.

---

## Epic 5 — Capability expansion (stretch)

Backlog of new commands worth considering once Epics 1–4 land. Triage these against real demand before building.

### B-5.1 · Web-based character verification · P2 · L
Currently character verification is Discord-only. Building it in the web app would let users self-serve from settings (B-4.4) and unblock the broader "claim retainers from the web" story.

### B-5.2 · Per-guild bot config command · P2 · M
`/ffxiv config set default_world <world>` so users in a guild don't have to specify `world` on every command. Stored per-guild, with per-user override.

### B-5.3 · Item-watch alerts (price drops) · P3 · M
Today's alerts only cover retainer undercuts. Add `/ffxiv alert price_below item:<X> world:<Y> price:<Z>` for buyer-side alerts.

### B-5.4 · Saved-search shortcuts · P3 · M
Let users save a `/ffxiv analyze profit` query as a named macro and re-run it with one command. Pairs well with B-5.2.

### B-5.5 · Message-context menu: "Look up item in this message" · P3 · M
Right-click a Discord message containing an item name → bot replies with current prices. Discoverable through Discord's native UI.

---

## Recommended sequencing

The early-action items from the audit map to specific tickets above. Suggested first slice:

1. **B-1.1** (retainer ownership) — security gap; nothing else in Epic 1 unblocks until this lands.
2. **B-3.1** (frontend `/bot` page) — unblocks Epic 4 (you need somewhere for the cross-promo links to point).
3. **B-2.1**, **B-2.3** (autocomplete on lists, defaults on analyze) — fastest UX wins.
4. **B-4.1**, **B-4.2** (alerts banner, item-page chip) — highest-intent web touchpoints.
5. **B-2.4** (fill placeholder descriptions) — cheap, makes `/help` look professional.

After that slice, re-triage based on what users actually ask for. The Epic 5 items are explicitly speculative.
