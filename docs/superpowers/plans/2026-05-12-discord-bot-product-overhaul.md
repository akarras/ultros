# Discord Bot Product Overhaul — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Execute the first slice of the Discord bot backlog ([docs/discord-bot-backlog.md](../../discord-bot-backlog.md)) — gate retainer ownership on verified characters, add list/analyze UX polish, ship a `/bot` documentation page, and surface the bot in two high-intent web locations.

**Architecture:** Three subsystems touched. (1) Discord bot — Rust + poise, `ultros/src/discord/` and `ultros-db/src/`. (2) Web frontend — Leptos in `ultros-frontend/ultros-app/`. (3) Backlog doc updated as tickets close. No DB migrations. No API additions (the existing `claim_retainer` and `verify_character` web endpoints stay as-is).

**Tech Stack:** Rust 2024, `poise` 0.6.x (Discord), `sea-orm` 1.x (DB), Leptos 0.7.x SSR/hydrate, `leptos_i18n`. Tests: stock `#[cfg(test)]` blocks for pure functions. DB-layer changes verified manually because there's no `ultros-db` integration-test harness (one example unit test exists in [analyzer_service.rs:1691](../../../ultros/src/analyzer_service.rs)).

**Feature branch:** `feature/discord-bot-product-overhaul` (already created from `main` at `0e725c88`).

---

## Parallelization Map

Seven tasks. **T1–T6 are independent and can be dispatched in parallel.** T7 reads the `/bot` route URL chosen in T5, but the URL is fixed up-front in this plan (`/bot`), so T7 can also run in parallel — it just won't render correctly until T5 lands.

Recommended subagent execution:

```
Wave 1 (parallel — dispatch all together):
  T1  Retainer-claim ownership gate          → ultros/src/discord/ffxiv/retainer.rs + ultros-db/src/retainers.rs
  T2  Defaults on /ffxiv analyze profit      → ultros/src/discord/ffxiv/analyze.rs
  T3  Autocomplete on /ffxiv list *          → ultros/src/discord/ffxiv/lists.rs
  T4  /ffxiv character placeholder fix       → ultros/src/discord/ffxiv/character.rs
  T5  Frontend /bot documentation page       → ultros-frontend/ultros-app/src/routes/bot.rs (new) + lib.rs
  T6  Item-page Discord chip                 → ultros-frontend/ultros-app/src/routes/item_view.rs

Wave 2 (parallel after Wave 1 lands and is reviewed):
  T7  Alerts-page Discord banner              → ultros-frontend/ultros-app/src/routes/alerts.rs
```

No two tasks edit the same file. Bundled placeholder-description fixes from backlog ticket B-2.4 are absorbed into T1 (retainer parent) and T2 (analyze parent); T4 covers the remaining `character` parent. The `list` parent already has a non-placeholder description ([lists.rs:23](../../../ultros/src/discord/ffxiv/lists.rs)).

**Backlog ticket → Task map:**

| Backlog ticket | Task |
|---|---|
| B-1.1 retainer ownership | T1 |
| B-2.1 list autocomplete | T3 |
| B-2.3 analyze defaults | T2 |
| B-2.4 placeholder descriptions | T1 + T2 + T4 |
| B-3.1 /bot page | T5 |
| B-4.1 alerts banner | T7 |
| B-4.2 item-page chip | T6 |

Out of scope for this plan (defer to future slices): B-1.2 list DB-layer ownership, B-1.3 alert lifecycle, B-2.2 region autocomplete, B-2.5 command renames (breaking), B-2.6 char-register timeout polish, B-2.7 hide /register, B-3.2 userguide chapter, B-3.3 README, B-4.3 retainer banners, B-4.4 settings CTA, B-4.5 home-page CTA, B-4.6 lists footer link, all of Epic 5.

---

## File Structure

**Discord bot (Rust)**
- **Modify** [ultros/src/discord/ffxiv/retainer.rs](../../../ultros/src/discord/ffxiv/retainer.rs)
  - Line 19–37 (`retainer` subgroup parent): replace placeholder body with a help embed.
  - Line 63–87 (`autocomplete_retainer_id`): filter to retainers owned by one of the caller's verified characters.
  - Line 256–284 (`add`): pre-flight check — caller must have ≥ 1 verified `owned_ffxiv_character`. If not, error with a pointer to the web flow.
- **Modify** [ultros/src/discord/ffxiv/analyze.rs](../../../ultros/src/discord/ffxiv/analyze.rs)
  - Line 10–14 (`analyze` subgroup parent): replace `"Hello world"` with a help embed.
  - Line 16–24 (`profit` command): make `minimum_profit`, `number_recently_sold`, `threshold_days` `Option<i32>` with documented defaults.
- **Modify** [ultros/src/discord/ffxiv/lists.rs](../../../ultros/src/discord/ffxiv/lists.rs)
  - Add `autocomplete_list_name` and `autocomplete_list_item_name` helpers.
  - Wire autocomplete onto the `list_name` / `item_name` params of `remove`, `add_item`, `remove_item`, `show_list`.
  - Reuse `autocomplete_item` from [item_prices.rs:19](../../../ultros/src/discord/ffxiv/item_prices.rs) for the global item name on `add_item`.
- **Modify** [ultros/src/discord/ffxiv/character.rs](../../../ultros/src/discord/ffxiv/character.rs)
  - Line 4–8 (`character` subgroup parent): replace `"Hello world"` with a help embed.
- **Modify** [ultros-db/src/retainers.rs](../../../ultros-db/src/retainers.rs)
  - Add new method `get_retainers_for_user_characters(discord_user_id) -> Vec<retainer::Model>` that joins through `owned_ffxiv_character`.

**Web frontend (Leptos)**
- **Create** `ultros-frontend/ultros-app/src/routes/bot.rs` — new `BotGuide` component (static command reference v1).
- **Modify** [ultros-frontend/ultros-app/src/lib.rs](../../../ultros-frontend/ultros-app/src/lib.rs)
  - Line ~66 import block: add `routes::bot::BotGuide`.
  - Line ~312 routes block: register `<Route path=path!("bot") view=BotGuide />`.
- **Modify** [ultros-frontend/ultros-app/src/routes/item_view.rs](../../../ultros-frontend/ultros-app/src/routes/item_view.rs)
  - Add a small Discord-command chip near the existing meta/header area showing `/ffxiv prices current item:<Name> world:<World>` with a copy-to-clipboard button (the project already has [components/clipboard.rs](../../../ultros-frontend/ultros-app/src/components/clipboard.rs); reuse it).
- **Modify** [ultros-frontend/ultros-app/src/routes/alerts.rs](../../../ultros-frontend/ultros-app/src/routes/alerts.rs)
  - Add an inline tip card near the alert-delivery selector linking to `/bot#retainer-undercut-alerts`.

**Locale**
- **Modify** [ultros-frontend/ultros-app/locales/en.json](../../../ultros-frontend/ultros-app/locales/en.json)
  - Add keys: `bot_page_title`, `bot_page_invite_cta`, `bot_page_getting_started`, `alerts_discord_tip`, `item_discord_chip_label`. (Other locales fall back to English; do not edit them.)

**Docs**
- **Modify** [docs/discord-bot-backlog.md](../../discord-bot-backlog.md) — at the end of each task, check off the corresponding backlog ticket (e.g. add `✅ shipped 2026-05-12` to B-1.1).

---

## Pre-flight (do once before dispatching subagents)

- [ ] **Step P1: Confirm branch**

Run: `git rev-parse --abbrev-ref HEAD`
Expected: `feature/discord-bot-product-overhaul`

- [ ] **Step P2: Lock the `/bot` route URL**

The frontend route is `/bot`. T5 creates it; T7 links to it. Do not change this without coordinating both tasks.

- [ ] **Step P3: Confirm `./check_ci.sh` is the gate**

Per [CLAUDE.md](../../../CLAUDE.md): run `./check_ci.sh` (or at minimum `cargo fmt --all -- --check`) before every commit. If submodules aren't initialized, run `git submodule update --init --recursive --depth=1` first.

---

## Task T1: Retainer-claim ownership gate

**Files:**
- Modify: `ultros/src/discord/ffxiv/retainer.rs:19-37, 63-87, 256-284`
- Modify: `ultros-db/src/retainers.rs` (add new method)

**Context.** Today `/ffxiv retainer add` calls `register_retainer` ([retainers.rs:49](../../../ultros-db/src/retainers.rs)) which only checks that the retainer row exists. Any Discord user can claim any retainer in the DB. The web app already has a proper claim flow ([web.rs:505 `claim_retainer`](../../../ultros/src/web.rs)) backed by Lodestone-bio verification ([web.rs:486 `verify_character`](../../../ultros/src/web.rs)). Discord just needs to honor what the web has already verified.

We do this by:
1. Adding a DB method that returns retainers belonging to the caller's verified characters.
2. Filtering the autocomplete with that method.
3. Pre-flight-checking `add` so it returns a helpful error if the caller has zero verified characters.

- [ ] **Step 1: Add `get_retainers_for_user_characters` to ultros-db**

Append to `ultros-db/src/retainers.rs` inside the `impl UltrosDb` block (place it just before `register_retainer`):

```rust
    /// Returns all retainers in the DB whose `character_id` belongs to a
    /// `final_fantasy_character` that the Discord user owns (i.e. has a row in
    /// `owned_ffxiv_character`). This is the source of truth for "retainers the
    /// caller is allowed to claim."
    #[instrument]
    pub async fn get_retainers_for_user_characters(
        &self,
        discord_user_id: u64,
    ) -> Result<Vec<retainer::Model>> {
        use crate::entity::{owned_ffxiv_character, retainer};
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect, RelationTrait, JoinType};

        let character_ids: Vec<i32> = owned_ffxiv_character::Entity::find()
            .select_only()
            .column(owned_ffxiv_character::Column::FfxivCharacterId)
            .filter(owned_ffxiv_character::Column::DiscordUserId.eq(discord_user_id as i64))
            .into_tuple()
            .all(&self.db)
            .await?;

        if character_ids.is_empty() {
            return Ok(vec![]);
        }

        Ok(retainer::Entity::find()
            .filter(retainer::Column::CharacterId.is_in(character_ids))
            .all(&self.db)
            .await?)
    }
```

Note: confirm `retainer::Column::CharacterId` exists — check [ultros-db/src/entity/retainer.rs](../../../ultros-db/src/entity/retainer.rs). If the column is named differently (e.g. `OwningCharacterId`), use the actual variant.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p ultros-db`
Expected: no errors. If `retainer::Column::CharacterId` doesn't exist, the compile will fail with a missing-variant message — open `entity/retainer.rs` and use the actual column name.

- [ ] **Step 3: Filter `autocomplete_retainer_id` in retainer.rs**

Replace `ultros/src/discord/ffxiv/retainer.rs:63-87` (the `autocomplete_retainer_id` function) with:

```rust
async fn autocomplete_retainer_id(
    ctx: Context<'_>,
    partial: &str,
) -> impl Iterator<Item = poise::serenity_prelude::AutocompleteChoice> {
    let world_cache = ctx.data().world_cache.clone();
    let partial = partial.to_ascii_lowercase();
    ctx.data()
        .db
        .get_retainers_for_user_characters(ctx.author().id.get())
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(move |retainer| retainer.name.to_ascii_lowercase().contains(&partial))
        .flat_map(move |retainer| {
            Some(poise::serenity_prelude::AutocompleteChoice::new(
                format!(
                    "{} - {}",
                    retainer.name,
                    world_cache
                        .lookup_selector(&AnySelector::World(retainer.world_id))
                        .ok()?
                        .get_name()
                ),
                retainer.id,
            ))
        })
}
```

- [ ] **Step 4: Add the pre-flight check to `add`**

Replace `ultros/src/discord/ffxiv/retainer.rs:256-284` (the `add` function — find it by the `/// Adds a retainer to your profile` doc comment) with:

```rust
/// Adds a retainer to your profile (requires a verified FFXIV character)
#[poise::command(slash_command)]
async fn add(
    ctx: Context<'_>,
    #[description = "Retainer name"]
    #[autocomplete = "autocomplete_retainer_id"]
    retainer_id: i32,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let characters = ctx
        .data()
        .db
        .get_all_characters_for_discord_user(ctx.author().id.get() as i64)
        .await?;
    if characters.is_empty() {
        ctx.send(
            poise::CreateReply::default().embed(
                poise::serenity_prelude::CreateEmbed::new()
                    .title("Verify a character first")
                    .description(
                        "Claiming a retainer requires a verified FFXIV character. \
                         Visit https://ultros.app and link your character via the \
                         Lodestone challenge, then come back and run this command.",
                    )
                    .color(Color::from_rgb(200, 80, 80)),
            ),
        )
        .await?;
        return Ok(());
    }
    // Defense in depth: even though autocomplete only shows claimable retainers,
    // a power user can type a raw retainer_id. Reject if it doesn't belong to
    // one of the caller's characters.
    let claimable = ctx
        .data()
        .db
        .get_retainers_for_user_characters(ctx.author().id.get())
        .await?;
    if !claimable.iter().any(|r| r.id == retainer_id) {
        ctx.send(
            poise::CreateReply::default().embed(
                poise::serenity_prelude::CreateEmbed::new()
                    .title("Retainer not claimable")
                    .description(
                        "That retainer doesn't belong to any of your verified characters. \
                         If this is your retainer, make sure the character it belongs to \
                         is verified on ultros.app.",
                    )
                    .color(Color::from_rgb(200, 80, 80)),
            ),
        )
        .await?;
        return Ok(());
    }
    let _register_retainer = ctx
        .data()
        .db
        .register_retainer(
            retainer_id,
            ctx.author().id.get(),
            ctx.author().name.clone(),
        )
        .await?;
    ctx.send(
        poise::CreateReply::default().embed(
            poise::serenity_prelude::CreateEmbed::new()
                .title("Added retainer")
                .description("added retainer!")
                .color(Color::from_rgb(123, 0, 123)),
        ),
    )
    .await?;
    Ok(())
}
```

- [ ] **Step 5: Replace the `retainer` subgroup placeholder (bundled B-2.4)**

Replace `ultros/src/discord/ffxiv/retainer.rs:19-37` (the `retainer` parent function — `pub(crate) async fn retainer`). Keep the existing `#[poise::command(...)]` attribute exactly as-is. Replace only the function body:

```rust
    ctx.send(
        poise::CreateReply::default().embed(
            poise::serenity_prelude::CreateEmbed::new()
                .title("Retainers")
                .description(
                    "Manage retainers tied to your verified FFXIV character.\n\n\
                     **Setup:**\n\
                     1. Verify your character at https://ultros.app\n\
                     2. `/ffxiv retainer add` — claim one of your retainers\n\
                     3. `/ffxiv retainer add_undercut_alert` — get alerts in this channel\n\n\
                     **See also:** `/ffxiv retainer list`, `check_listings`, `check_undercuts`.",
                ),
        ),
    )
    .await?;
    Ok(())
```

- [ ] **Step 6: Build + clippy + fmt**

Run: `./check_ci.sh` (or, if submodules unavailable, `cargo fmt --all -- --check && cargo check --workspace`)
Expected: clean. Fix any clippy warnings before proceeding.

- [ ] **Step 7: Update the backlog**

Edit `docs/discord-bot-backlog.md`: at the start of the `B-1.1` section heading, prefix with `[shipped 2026-05-12]`. Same for `B-2.4` (the retainer-parent portion is done; leave the ticket open if T2/T4 haven't shipped yet — only mark fully shipped once all three placeholders are gone).

- [ ] **Step 8: Commit**

```bash
git add ultros/src/discord/ffxiv/retainer.rs ultros-db/src/retainers.rs docs/discord-bot-backlog.md
git commit -m "feat(discord): gate retainer claims on verified characters

Closes backlog B-1.1. /ffxiv retainer add now requires the caller to have
at least one verified owned_ffxiv_character row. Autocomplete filters to
retainers belonging to those characters; defense-in-depth check rejects
raw retainer_id input. Also fills B-2.4 retainer-parent placeholder."
```

---

## Task T2: Defaults for `/ffxiv analyze profit`

**Files:**
- Modify: `ultros/src/discord/ffxiv/analyze.rs:10-14, 16-24`

**Context.** Today the command requires four numeric args with no defaults ([analyze.rs:17-24](../../../ultros/src/discord/ffxiv/analyze.rs)). First-time users have to guess values. We're making three of them optional with sensible defaults pulled from the backlog: `minimum_profit=10_000`, `number_recently_sold=5`, `threshold_days=7`. `world` stays required.

- [ ] **Step 1: Rewrite the `profit` command signature and body**

Replace `ultros/src/discord/ffxiv/analyze.rs:16-24` (the function signature through the closing paren of the args, plus the existing `clamp_sold_amount` / `threshold_days_to_sold_within` calls). Replace lines 16–38 with:

```rust
#[poise::command(slash_command, prefix_command)]
pub(crate) async fn profit(
    ctx: Context<'_>,
    #[description = "World you want to try and sell items on"] world: String,
    #[description = "Minimum profit in gil (default: 10000)"] minimum_profit: Option<i32>,
    #[description = "Number of items sold within the threshold (default: 5)"]
    number_recently_sold: Option<i32>,
    #[description = "Length of the threshold in days (default: 7)"] threshold_days: Option<i32>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let minimum_profit = minimum_profit.unwrap_or(10_000);
    let number_recently_sold = number_recently_sold.unwrap_or(5);
    let threshold_days = threshold_days.unwrap_or(7);

    let world = ctx.data().world_cache.lookup_value_by_name(&world)?;
    let world_id = world.as_world()?.id;
    let region_id = ctx
        .data()
        .world_cache
        .get_region(&world)
        .ok_or(anyhow::anyhow!("World not in a region?"))?
        .id;

    let amount = clamp_sold_amount(number_recently_sold);
    let filter_sale = threshold_days_to_sold_within(threshold_days, amount);
```

Leave lines 39 onward (the `let xiv_data = …` through end-of-function) unchanged.

- [ ] **Step 2: Replace the `analyze` subgroup placeholder (bundled B-2.4)**

Replace `ultros/src/discord/ffxiv/analyze.rs:10-14` (the entire `pub(crate) async fn analyze` function) with:

```rust
#[poise::command(slash_command, prefix_command, subcommands("profit"))]
pub(crate) async fn analyze(ctx: Context<'_>) -> Result<(), Error> {
    ctx.send(
        poise::CreateReply::default().embed(
            poise::serenity_prelude::CreateEmbed::new()
                .title("Market analysis")
                .description(
                    "Find profitable items to flip.\n\n\
                     `/ffxiv analyze profit world:<name>` — top 15 flips for a world.\n\
                     Optional knobs: `minimum_profit` (default 10000), \
                     `number_recently_sold` (default 5), `threshold_days` (default 7).",
                ),
        ),
    )
    .await?;
    Ok(())
}
```

- [ ] **Step 3: Build + clippy + fmt**

Run: `./check_ci.sh`
Expected: clean.

- [ ] **Step 4: Manual smoke (optional, requires a dev bot token)**

If you have a dev bot configured: invoke `/ffxiv analyze profit world:Faerie` with no other args; confirm the embed returns results. Otherwise rely on the type checker.

- [ ] **Step 5: Update the backlog**

Edit `docs/discord-bot-backlog.md`: prefix `B-2.3` heading with `[shipped 2026-05-12]`.

- [ ] **Step 6: Commit**

```bash
git add ultros/src/discord/ffxiv/analyze.rs docs/discord-bot-backlog.md
git commit -m "feat(discord): defaults for /ffxiv analyze profit

Closes backlog B-2.3. Three numeric args become Option<i32> with sensible
defaults (10000 gil / 5 sales / 7 days). Also fills the analyze-parent
placeholder description from B-2.4."
```

---

## Task T3: Autocomplete on `/ffxiv list *`

**Files:**
- Modify: `ultros/src/discord/ffxiv/lists.rs:88-116, 118-156, 158-189, 191-end`

**Context.** Four list-management commands (`remove`, `add_item`, `remove_item`, `show_list`) take raw strings with exact-match lookup. Users must remember list names character-for-character. We add autocomplete on `list_name` (filtered to the caller's lists) and on `item_name` (for `add_item` we use the global xiv_gen item list; for `remove_item` we filter to items already on the list).

- [ ] **Step 1: Add the autocomplete helpers**

Insert these two functions into `ultros/src/discord/ffxiv/lists.rs` immediately after the `show_lists` function (around line 52):

```rust
async fn autocomplete_list_name(
    ctx: Context<'_>,
    partial: &str,
) -> impl Iterator<Item = poise::serenity_prelude::AutocompleteChoice> {
    let partial = partial.to_ascii_lowercase();
    ctx.data()
        .db
        .get_lists_for_user(ctx.author().id.get() as i64)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(move |l| l.name.to_ascii_lowercase().contains(&partial))
        .map(|l| poise::serenity_prelude::AutocompleteChoice::new(l.name.clone(), l.name))
}

async fn autocomplete_item_name_global(
    _ctx: Context<'_>,
    partial: &str,
) -> impl Iterator<Item = poise::serenity_prelude::AutocompleteChoice> {
    let partial = partial.to_ascii_lowercase();
    let items = &xiv_gen_db::data().items;
    items
        .iter()
        .filter(move |(_, i)| !i.name.is_empty() && i.name.to_ascii_lowercase().contains(&partial))
        .take(25)
        .map(|(_, i)| {
            poise::serenity_prelude::AutocompleteChoice::new(i.name.clone(), i.name.clone())
        })
        .collect::<Vec<_>>()
        .into_iter()
}
```

Note: Discord caps autocomplete suggestions at 25 — that's why `.take(25)`.

- [ ] **Step 2: Wire `autocomplete_list_name` onto the four list-name params**

In `ultros/src/discord/ffxiv/lists.rs`, find the four `list_name: String` parameter declarations (`remove` at line ~91, `add_item` at ~122, `remove_item` at ~162, `show_list` at ~193) and add the autocomplete attribute. Example for `remove`:

```rust
async fn remove(
    ctx: Context<'_>,
    #[description = "Name of the list to remove"]
    #[autocomplete = "autocomplete_list_name"]
    list_name: String,
) -> Result<(), Error> {
```

Apply the same `#[autocomplete = "autocomplete_list_name"]` line to `list_name` in `add_item`, `remove_item`, and `show_list`.

- [ ] **Step 3: Wire `autocomplete_item_name_global` onto the `add_item` item param**

In `add_item` (~line 122), add the autocomplete attribute to `item_name`:

```rust
    #[description = "item to add"]
    #[autocomplete = "autocomplete_item_name_global"]
    item_name: String,
```

(Do not add autocomplete to `remove_item`'s `item_name` in this slice — filtering by "items on the named list" requires the param to know the value of `list_name`, which poise doesn't pass to autocompletes today. Leave it as a follow-up; it works fine without autocomplete because the user just typed the list name and is removing items they remember adding.)

- [ ] **Step 4: Build + clippy + fmt**

Run: `./check_ci.sh`
Expected: clean.

- [ ] **Step 5: Update the backlog**

Edit `docs/discord-bot-backlog.md`: prefix `B-2.1` heading with `[shipped 2026-05-12 — partial: list-name on all four, item-name on add_item only]`.

- [ ] **Step 6: Commit**

```bash
git add ultros/src/discord/ffxiv/lists.rs docs/discord-bot-backlog.md
git commit -m "feat(discord): autocomplete on /ffxiv list commands

Partial close of backlog B-2.1. list_name autocomplete on remove, add_item,
remove_item, show_list (filtered to caller's lists). item_name autocomplete
on add_item (global xiv_gen items, capped at 25 per Discord limit).
remove_item item_name remains free-text — list-scoped filtering needs poise
support for autocomplete arg context."
```

---

## Task T4: `/ffxiv character` placeholder fix

**Files:**
- Modify: `ultros/src/discord/ffxiv/character.rs:4-8`

**Context.** Only remaining "Hello world" placeholder after T1 and T2. Trivial.

- [ ] **Step 1: Replace the parent body**

Replace `ultros/src/discord/ffxiv/character.rs:4-8` (the entire `pub(crate) async fn character` function) with:

```rust
#[poise::command(slash_command, prefix_command, subcommands("register"))]
pub(crate) async fn character(ctx: Context<'_>) -> Result<(), Error> {
    ctx.send(
        poise::CreateReply::default().embed(
            poise::serenity_prelude::CreateEmbed::new()
                .title("FFXIV Characters")
                .description(
                    "Look up your character on the Lodestone.\n\n\
                     `/ffxiv character register name:<First Last>` — search and select.\n\n\
                     **To verify a character (required for retainer claims),** \
                     visit https://ultros.app and complete the Lodestone bio challenge.",
                ),
        ),
    )
    .await?;
    Ok(())
}
```

- [ ] **Step 2: Build + clippy + fmt**

Run: `./check_ci.sh`
Expected: clean.

- [ ] **Step 3: Update the backlog**

Edit `docs/discord-bot-backlog.md`: if T1 and T2 also landed, fully close `B-2.4` with `[shipped 2026-05-12]`. If they didn't yet, leave a note `[partial — character parent shipped]`.

- [ ] **Step 4: Commit**

```bash
git add ultros/src/discord/ffxiv/character.rs docs/discord-bot-backlog.md
git commit -m "feat(discord): replace /ffxiv character placeholder description

Part of backlog B-2.4. The parent now points users at the web verification
flow, which is the only path that unlocks retainer claims after T1."
```

---

## Task T5: Frontend `/bot` documentation page

**Files:**
- Create: `ultros-frontend/ultros-app/src/routes/bot.rs`
- Modify: `ultros-frontend/ultros-app/src/lib.rs` (router registration + import)
- Modify: `ultros-frontend/ultros-app/locales/en.json`

**Context.** No Discord documentation exists anywhere. We're shipping a static command reference v1 — auto-generation from the poise command tree is a future improvement. Use the project's existing Leptos + Tailwind component patterns; see [routes/about.rs](../../../ultros-frontend/ultros-app/src/routes/about.rs) for a minimal static-page reference.

The page has three sections matching the backlog: **Invite the bot**, **Command reference**, **Getting started**.

- [ ] **Step 1: Create the route file**

Create `ultros-frontend/ultros-app/src/routes/bot.rs`:

```rust
use leptos::prelude::*;
use leptos_meta::{Meta, Title};

#[component]
pub fn BotGuide() -> impl IntoView {
    view! {
        <Title text="Ultros Discord Bot" />
        <Meta
            name="description"
            content="Command reference and setup guide for the Ultros Discord bot — FFXIV market data in your server."
        />
        <div class="container mx-auto max-w-4xl px-4 py-8 flex flex-col gap-12">
            <header class="flex flex-col gap-3">
                <h1 class="text-4xl font-bold text-brand-200">"Ultros Discord Bot"</h1>
                <p class="text-lg text-[color:var(--color-text-muted)]">
                    "Look up market prices, manage retainers, and get undercut alerts — all from Discord."
                </p>
            </header>

            <section id="invite" class="flex flex-col gap-4">
                <h2 class="text-2xl font-semibold text-brand-300">"1. Invite the bot"</h2>
                <p>
                    "Adds the Ultros bot to your server with the "
                    <code class="rounded bg-black/40 px-1.5 py-0.5">"Use Application Commands"</code>
                    " permission. You can configure per-channel access from Discord's server settings."
                </p>
                <a
                    href="/invitebot"
                    class="self-start rounded-md bg-brand-500 px-5 py-2.5 font-semibold text-white shadow hover:bg-brand-400 transition-colors"
                >
                    "Add to your server"
                </a>
            </section>

            <section id="getting-started" class="flex flex-col gap-3">
                <h2 class="text-2xl font-semibold text-brand-300">"2. Getting started"</h2>
                <ol class="list-decimal list-inside flex flex-col gap-2">
                    <li>"Verify your FFXIV character on this site (Settings → Characters → Lodestone challenge)."</li>
                    <li>"In Discord, run " <code class="rounded bg-black/40 px-1.5 py-0.5">"/ffxiv retainer add"</code> " — the autocomplete only shows retainers belonging to your verified characters."</li>
                    <li>"In any channel where the bot is installed, run " <code class="rounded bg-black/40 px-1.5 py-0.5">"/ffxiv retainer add_undercut_alert margin_percent:0"</code> " — you'll get a ping the moment a competitor undercuts you."</li>
                </ol>
            </section>

            <section id="commands" class="flex flex-col gap-8">
                <h2 class="text-2xl font-semibold text-brand-300">"3. Command reference"</h2>

                <CommandGroup
                    title="/ffxiv prices".to_string()
                    description="Real-time market lookups.".to_string()
                    commands=vec![
                        ("/ffxiv prices current item:<name> world:<world>", "Top 10 cheapest current listings."),
                        ("/ffxiv prices history item:<name> world:<world>", "Historical price chart (PNG)."),
                    ]
                />

                <CommandGroup
                    title="/ffxiv retainer".to_string()
                    description="Manage your retainers. Requires a verified character.".to_string()
                    commands=vec![
                        ("/ffxiv retainer list", "Show your retainers and their listing counts."),
                        ("/ffxiv retainer add retainer_id:<name>", "Claim a retainer (autocomplete only shows your own)."),
                        ("/ffxiv retainer remove owned_retainer_id:<name>", "Release a retainer claim."),
                        ("/ffxiv retainer check_listings", "All your active listings, tabled."),
                        ("/ffxiv retainer check_undercuts", "Only your listings that have been undercut."),
                        ("/ffxiv retainer add_undercut_alert margin_percent:<0-200>", "Notify this channel on undercut."),
                        ("/ffxiv retainer remove_undercut_alert", "Stop notifications in this channel."),
                    ]
                />

                <CommandGroup
                    title="/ffxiv list".to_string()
                    description="Shopping lists scoped to a region/datacenter/world.".to_string()
                    commands=vec![
                        ("/ffxiv list show_lists", "Show your list names."),
                        ("/ffxiv list create list_name:<name> region_datacenter_or_world:<scope>", "Create a list."),
                        ("/ffxiv list remove list_name:<name>", "Delete a list."),
                        ("/ffxiv list add_item list_name:<name> item_name:<item> [quantity] [hq]", "Add to a list."),
                        ("/ffxiv list remove_item list_name:<name> item_name:<item>", "Remove from a list."),
                        ("/ffxiv list show_list list_name:<name>", "Show current lowest prices for the list."),
                    ]
                />

                <CommandGroup
                    title="/ffxiv analyze".to_string()
                    description="Market analysis.".to_string()
                    commands=vec![
                        ("/ffxiv analyze profit world:<name> [minimum_profit=10000] [number_recently_sold=5] [threshold_days=7]", "Top 15 flips on a world."),
                    ]
                />

                <CommandGroup
                    title="/ffxiv character".to_string()
                    description="Lodestone lookup. To verify ownership, use Settings on this site.".to_string()
                    commands=vec![
                        ("/ffxiv character register name:<First Last> [home_world]", "Search Lodestone."),
                    ]
                />
            </section>
        </div>
    }
}

#[component]
fn CommandGroup(
    title: String,
    description: String,
    commands: Vec<(&'static str, &'static str)>,
) -> impl IntoView {
    view! {
        <div class="flex flex-col gap-3 rounded-lg border border-brand-500/20 bg-black/20 p-5">
            <div class="flex flex-col gap-1">
                <h3 class="text-xl font-semibold text-brand-200">{title}</h3>
                <p class="text-sm text-[color:var(--color-text-muted)]">{description}</p>
            </div>
            <ul class="flex flex-col gap-2">
                {commands.into_iter().map(|(sig, desc)| view! {
                    <li class="flex flex-col gap-0.5">
                        <code class="text-sm rounded bg-black/40 px-2 py-1 self-start">{sig}</code>
                        <span class="text-sm text-[color:var(--color-text-muted)] pl-1">{desc}</span>
                    </li>
                }).collect_view()}
            </ul>
        </div>
    }
}
```

- [ ] **Step 2: Register the route in lib.rs**

Edit `ultros-frontend/ultros-app/src/lib.rs`:

(a) Find the `mod routes;` and the existing `use routes::...` imports (top of file). Add to the routes-use block:

```rust
use routes::bot::BotGuide;
```

(b) Find the routes module file at `ultros-frontend/ultros-app/src/routes/mod.rs` and add `pub mod bot;` alongside the other `pub mod` lines.

(c) In `lib.rs`, find the `<Routes fallback=NotFound>` block (around line 311 — `path!("")` for HomePage is the first child). Add a new route just before `<Route path=path!("flip-finder") view=Analyzer />`:

```rust
<Route path=path!("bot") view=BotGuide />
```

- [ ] **Step 3: Build + clippy + fmt**

Run: `./check_ci.sh`
Expected: clean.

- [ ] **Step 4: Smoke-test the page**

Run: `cd ultros-frontend && cargo leptos watch` (or whatever the project's dev command is — see [AGENTS.md](../../../AGENTS.md) or `scripts/run_e2e.sh`). Visit `http://localhost:3000/bot` and confirm:
- Page renders with all four sections.
- "Add to your server" button links to `/invitebot`.
- No console errors.

If the dev server isn't available in this environment, document that and rely on the type checker — the visual smoke can be done in a follow-up review.

- [ ] **Step 5: Update the backlog**

Edit `docs/discord-bot-backlog.md`: prefix `B-3.1` heading with `[shipped 2026-05-12 — static v1; auto-generation is a follow-up]`.

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/bot.rs ultros-frontend/ultros-app/src/routes/mod.rs ultros-frontend/ultros-app/src/lib.rs docs/discord-bot-backlog.md
git commit -m "feat(frontend): /bot Discord bot documentation page

Closes backlog B-3.1 (v1). Static command reference covering all six
command groups, an invite CTA, and a three-step getting-started flow.
Future work: generate the command list from the poise command tree."
```

---

## Task T6: Item-page Discord chip

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/item_view.rs`

**Context.** Add a small visual chip near the existing item header that shows the equivalent Discord slash command with a copy-to-clipboard button. Reuse [components/clipboard.rs](../../../ultros-frontend/ultros-app/src/components/clipboard.rs) (already imported via the `clipboard::*` glob at the top of the file).

- [ ] **Step 1: Find the insertion point**

Open `ultros-frontend/ultros-app/src/routes/item_view.rs` and locate the main `ItemView` (or equivalent) component's view block — search for the existing item-name heading rendering (likely an `<h1>` or `<h2>` showing the item name). Read 30 lines of context around it.

- [ ] **Step 2: Add a `DiscordCommandChip` helper component**

Append to `ultros-frontend/ultros-app/src/routes/item_view.rs` (above the existing `WorldButton` component or wherever fits the file's structure):

```rust
#[component]
fn DiscordCommandChip(
    #[prop(into)] item_name: Signal<String>,
    #[prop(into)] world_name: Signal<String>,
) -> impl IntoView {
    let command = Signal::derive(move || {
        format!(
            "/ffxiv prices current item:{} world:{}",
            item_name.get(),
            world_name.get(),
        )
    });
    view! {
        <div class="inline-flex items-center gap-2 rounded-md border border-brand-500/30 bg-black/30 px-2.5 py-1 text-xs">
            <span class="text-[color:var(--color-text-muted)]">"Discord:"</span>
            <code class="font-mono">{move || command.get()}</code>
            <Clipboard text=command />
        </div>
    }
}
```

Confirm the actual `Clipboard` component signature by reading [components/clipboard.rs](../../../ultros-frontend/ultros-app/src/components/clipboard.rs) — if it takes a different prop name than `text`, adjust.

- [ ] **Step 3: Render the chip near the item header**

In the `ItemView` component, find the existing item-name display (search for the item's name being read out of the `CurrentlyShownItem` or `items.get(&ItemId(...))`) and add `<DiscordCommandChip ... />` just below it, passing the item name and current-world signals already in scope. Example (adapt to actual signal names):

```rust
<DiscordCommandChip item_name=item_name_signal world_name=current_world.into() />
```

If the existing `WorldButton`s already build a `current_world: Memo<String>`, reuse that.

- [ ] **Step 4: Build + clippy + fmt**

Run: `./check_ci.sh`
Expected: clean.

- [ ] **Step 5: Smoke-test**

If dev server available: visit `/item/Faerie/<some-item-id>` and confirm the chip renders with the correct command and the copy button works.

- [ ] **Step 6: Update the backlog**

Edit `docs/discord-bot-backlog.md`: prefix `B-4.2` heading with `[shipped 2026-05-12]`.

- [ ] **Step 7: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/item_view.rs docs/discord-bot-backlog.md
git commit -m "feat(frontend): Discord command chip on item pages

Closes backlog B-4.2. Inline chip near the item header shows the equivalent
/ffxiv prices current invocation with copy-to-clipboard. Lowest-effort
discovery surface for the Discord bot — every item page promotes it."
```

---

## Task T7 (Wave 2): Alerts-page Discord banner

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/alerts.rs`

**Context.** The alerts page already mentions Discord as an alert *delivery channel*. We're adding a small banner pointing at the Discord-native flow (`/ffxiv retainer add_undercut_alert`) — for users who'd rather configure alerts in Discord directly. Link target is `/bot#getting-started` which T5 lands.

**Wait for:** T5 to be merged (so the link target exists). T1 doesn't need to land first — the banner is informational.

- [ ] **Step 1: Identify the insertion point**

Open `ultros-frontend/ultros-app/src/routes/alerts.rs` and read the full `Alerts` component (it's small — start at the `#[component] pub fn Alerts` declaration). The banner should render once at the top of the alerts list, before the existing alert delivery configuration.

- [ ] **Step 2: Add the banner**

Inside the `Alerts` component's returned `view!`, near the top of the main container, add:

```rust
<div class="rounded-lg border border-brand-500/30 bg-brand-500/5 p-4 flex flex-col gap-2 mb-4">
    <p class="font-semibold text-brand-200">"Prefer Discord-native alerts?"</p>
    <p class="text-sm text-[color:var(--color-text-muted)]">
        "Run "
        <code class="rounded bg-black/40 px-1.5 py-0.5">"/ffxiv retainer add_undercut_alert"</code>
        " in any channel where the bot is installed. "
        <a href="/bot#getting-started" class="text-brand-300 underline hover:text-brand-200">
            "See the bot guide →"
        </a>
    </p>
</div>
```

- [ ] **Step 3: Build + clippy + fmt**

Run: `./check_ci.sh`
Expected: clean.

- [ ] **Step 4: Smoke-test**

If dev server available: visit `/alerts` and confirm the banner renders above the alerts list and the link navigates to `/bot`.

- [ ] **Step 5: Update the backlog**

Edit `docs/discord-bot-backlog.md`: prefix `B-4.1` heading with `[shipped 2026-05-12]`.

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/alerts.rs docs/discord-bot-backlog.md
git commit -m "feat(frontend): Discord-native tip on alerts page

Closes backlog B-4.1. Inline tip near the alert delivery UI points users
at /ffxiv retainer add_undercut_alert with a link to the bot guide."
```

---

## Post-merge wrap

After all seven tasks land on the feature branch:

- [ ] **Step W1: Final CI run**

```bash
./check_ci.sh
```

Expected: clean.

- [ ] **Step W2: Open the PR**

Use the project's PR conventions (see [README.md](../../../README.md)). Title suggestion: `feat: Discord bot product overhaul (B-1.1, B-2.1, B-2.3, B-2.4, B-3.1, B-4.1, B-4.2)`. PR body should reference [docs/discord-bot-backlog.md](../../discord-bot-backlog.md) and list the closed tickets.

- [ ] **Step W3: Re-triage the backlog**

After the PR merges, re-read the backlog. The deferred items (B-1.2, B-1.3, B-2.2, B-2.5–7, B-3.2–3, B-4.3–6, Epic 5) should be re-prioritized based on what shipping this slice teaches us.

---

## Self-review notes

- **Spec coverage.** All seven first-slice backlog items (B-1.1, B-2.1, B-2.3, B-2.4, B-3.1, B-4.1, B-4.2) map to a task above.
- **Placeholders.** Every code step shows actual code; every command step shows the exact command and expected output bucket; no "TBD" / "implement later".
- **Type consistency.** `get_retainers_for_user_characters` introduced in T1 step 1 is used in T1 steps 3 and 4 with the same signature. `autocomplete_list_name` introduced in T3 step 1 is used in T3 step 2 with the same name. `BotGuide` exported from T5 step 1 is imported in T5 step 2 with the same identifier. `DiscordCommandChip` defined in T6 step 2 is used in T6 step 3 with the same name.
- **Cross-task collisions.** Verified no two tasks edit the same file. T1 owns `retainer.rs` and the retainer-related DB methods; T2 owns `analyze.rs`; T3 owns `lists.rs`; T4 owns `character.rs`; T5 creates `bot.rs` and edits `lib.rs` + `routes/mod.rs`; T6 owns `item_view.rs`; T7 owns `alerts.rs`. No overlaps.
- **Caveats called out in-plan.** Item-name autocomplete on `remove_item` is intentionally deferred (T3 step 3). DB-integration tests don't exist for ultros-db, so T1's DB method is verified by `cargo check` + manual smoke rather than a unit test (called out in the architecture header).
