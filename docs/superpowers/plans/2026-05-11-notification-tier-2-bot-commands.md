# Notification Tier 2 — Bot commands — Implementation Plan

**Goal:** Mirror the web alert/endpoint surface as Discord slash commands under the existing `/ffxiv` group.

**Scope:** Tier 2 from the spec only. Excludes web push, SSE inbox, shared list notifications.

**Architecture:** Two new poise command modules in `ultros/src/discord/ffxiv/`: `alert.rs` and `endpoint.rs`. Both shell out to `ultros_db::UltrosDb` methods that already exist (PR #627). No DB migrations.

**Tech Stack:** poise + serenity, Rust, Sea-ORM (already wired).

---

## Files

| File | Action |
|---|---|
| `ultros/src/discord/ffxiv/alert.rs` | Create — `/ffxiv alert price|list|mute|unmute|remove` |
| `ultros/src/discord/ffxiv/endpoint.rs` | Create — `/ffxiv alert endpoint list|remove|here` (sub-subcommand under `alert`) |
| `ultros/src/discord/ffxiv/mod.rs` | Modify — register the new `alert` subcommand on `ffxiv` |
| `ultros-db/src/alerts.rs` | Modify — add `get_or_create_dm_endpoint(owner) -> i32` helper; add `find_endpoint_by_method_and_config(owner, method, config) -> Option<i32>` |

---

## Task 1: DB helper for idempotent DM endpoint creation

**File:** `ultros-db/src/alerts.rs`

- [ ] Add:

```rust
/// Find an existing endpoint owned by `owner` whose method+config matches; otherwise create
/// a new one. Returns the endpoint id. Used by bot commands to bind alerts to the caller's
/// default DM endpoint without dup-ing rows on repeat use.
pub async fn get_or_create_dm_endpoint(&self, owner: i64, name: &str) -> Result<i32> {
    let cfg = serde_json::json!({ "user_id": owner });
    if let Some(existing) = notification_endpoint::Entity::find()
        .filter(notification_endpoint::Column::UserId.eq(owner))
        .filter(notification_endpoint::Column::Method.eq("DiscordDm"))
        .filter(Expr::cust_with_values(
            "config::jsonb = ?::jsonb",
            vec![cfg.clone()],
        ))
        .one(&self.db)
        .await?
    {
        return Ok(existing.id);
    }
    self.create_endpoint(owner, name, "DiscordDm", cfg).await
}

/// Same as `get_or_create_dm_endpoint` but for a DiscordChannel pointed at `channel_id`.
pub async fn get_or_create_channel_endpoint(
    &self,
    owner: i64,
    channel_id: i64,
    name: &str,
) -> Result<i32> {
    let cfg = serde_json::json!({ "channel_id": channel_id });
    if let Some(existing) = notification_endpoint::Entity::find()
        .filter(notification_endpoint::Column::UserId.eq(owner))
        .filter(notification_endpoint::Column::Method.eq("DiscordChannel"))
        .filter(Expr::cust_with_values(
            "config::jsonb = ?::jsonb",
            vec![cfg.clone()],
        ))
        .one(&self.db)
        .await?
    {
        return Ok(existing.id);
    }
    self.create_endpoint(owner, name, "DiscordChannel", cfg).await
}
```

- [ ] Run `cargo check -p ultros-db`. Expected: clean.
- [ ] Commit: `feat(ultros-db): get_or_create_{dm,channel}_endpoint helpers`

---

## Task 2: `/ffxiv alert` subcommand module

**File:** `ultros/src/discord/ffxiv/alert.rs` (new)

The module contains:
- Top-level `alert` command with subcommands `price`, `list`, `mute`, `unmute`, `remove`, `endpoint` (which itself has subcommands `list`, `remove`, `here`).
- Use the existing item autocomplete pattern from `item_prices.rs` for the `<item>` arg.
- Use AnySelector for world resolution (look at `prices.rs` for the pattern).

Skeleton (each subcommand a separate poise fn):

```rust
use crate::discord::ffxiv::helpers;
use itertools::Itertools;
use poise::CreateReply;
use poise::serenity_prelude::CreateEmbed;
use ultros_db::world_data::world_cache::AnySelector as DbAnySelector;
use ultros_api_types::world_helper::AnySelector;
use xiv_gen::ItemId;

use super::{Context, Error, ULTROS_COLOR};

#[poise::command(
    slash_command,
    prefix_command,
    subcommands("price", "list", "mute", "unmute", "remove", "endpoint")
)]
pub(crate) async fn alert(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say(
        "Use one of: `price`, `list`, `mute`, `unmute`, `remove`, `endpoint`.\n\
         e.g. `/ffxiv alert price item:Tsai_tou_Vounou price:50000`"
    ).await?;
    Ok(())
}

/// Create a price alert. Sends a Discord DM when the item is listed below the threshold.
#[poise::command(slash_command, prefix_command)]
async fn price(
    ctx: Context<'_>,
    #[description = "Item name"]
    #[autocomplete = "helpers::autocomplete_item"]
    item: String,
    #[description = "Alert when price ≤ this gil"]
    price: i32,
    #[description = "Only match HQ listings (default: any)"]
    hq: Option<bool>,
    #[description = "World/DC/region to watch (default: your home world)"]
    world: Option<String>,
) -> Result<(), Error> {
    let owner = ctx.author().id.get() as i64;
    if price <= 0 {
        ctx.say("Price must be positive.").await?;
        return Ok(());
    }

    // Resolve item name → item_id via the existing helper
    let item_id = helpers::resolve_item_id(&item)
        .ok_or_else(|| anyhow::anyhow!("unknown item: {item}"))?;

    // Resolve world arg → AnySelector. If absent, use the user's home world from db; if no
    // home world is set, error out.
    let world_selector = match world {
        Some(s) => helpers::parse_world_selector(&ctx, &s).await?,
        None => helpers::user_home_world_selector(&ctx).await?,
    };

    let world_json = serde_json::to_value(world_selector)?;

    // Auto-create the user's DM endpoint if missing.
    let dm_endpoint = ctx
        .data()
        .db
        .get_or_create_dm_endpoint(owner, &format!("DM to {}", ctx.author().name))
        .await?;

    // Create the alert (no inline endpoint) and bind the rule.
    let alert = ctx
        .data()
        .db
        .create_threshold_alert_without_endpoint(
            owner,
            item_id,
            world_json,
            price,
            hq.unwrap_or(false),
            3600,
        )
        .await?;
    ctx.data()
        .db
        .set_alert_rules(owner, alert.id, &[dm_endpoint])
        .await?;

    ctx.send(
        CreateReply::default().embed(
            CreateEmbed::new()
                .color(ULTROS_COLOR)
                .title("Price alert created")
                .description(format!(
                    "**{item}** ≤ {price} gil{hq_str}. Alert id: `{id}`.\nDelivery: Discord DM to you.",
                    hq_str = if hq.unwrap_or(false) { " (HQ only)" } else { "" },
                    id = alert.id,
                )),
        ),
    )
    .await?;
    Ok(())
}

/// List your active alerts.
#[poise::command(slash_command, prefix_command)]
async fn list(ctx: Context<'_>) -> Result<(), Error> {
    let owner = ctx.author().id.get() as i64;
    let rows = ctx.data().db.get_user_threshold_alerts(owner).await?;
    if rows.is_empty() {
        ctx.say("You have no alerts. Create one with `/ffxiv alert price`.").await?;
        return Ok(());
    }
    let items = xiv_gen_db::data().items.clone();
    let lines = rows.into_iter().format_with("\n", |(a, t), f| {
        let item_name = items
            .get(&ItemId(t.item_id))
            .map(|it| it.name.as_str())
            .unwrap_or("?");
        let status = if a.enabled { "✅" } else { "⏸" };
        f(&format_args!(
            "{status} `#{id}` {name} ≤ {price} gil{hq}",
            id = a.id,
            name = item_name,
            price = t.price_threshold,
            hq = if t.hq_only { " (HQ)" } else { "" },
        ))
    });
    ctx.send(
        CreateReply::default().embed(
            CreateEmbed::new()
                .color(ULTROS_COLOR)
                .title("Your alerts")
                .description(format!("{lines}")),
        ),
    )
    .await?;
    Ok(())
}

/// Disable an alert. Use `unmute` to re-enable.
#[poise::command(slash_command, prefix_command)]
async fn mute(
    ctx: Context<'_>,
    #[description = "Alert id (see `/ffxiv alert list`)"] id: i32,
) -> Result<(), Error> {
    let owner = ctx.author().id.get() as i64;
    ctx.data().db.set_alert_enabled(owner, id, false).await?;
    ctx.say(format!("Alert `#{id}` muted.")).await?;
    Ok(())
}

/// Re-enable an alert.
#[poise::command(slash_command, prefix_command)]
async fn unmute(
    ctx: Context<'_>,
    #[description = "Alert id (see `/ffxiv alert list`)"] id: i32,
) -> Result<(), Error> {
    let owner = ctx.author().id.get() as i64;
    ctx.data().db.set_alert_enabled(owner, id, true).await?;
    ctx.say(format!("Alert `#{id}` unmuted.")).await?;
    Ok(())
}

/// Delete an alert.
#[poise::command(slash_command, prefix_command)]
async fn remove(
    ctx: Context<'_>,
    #[description = "Alert id (see `/ffxiv alert list`)"] id: i32,
) -> Result<(), Error> {
    let owner = ctx.author().id.get() as i64;
    ctx.data().db.delete_alert_owned_by(owner, id).await?;
    ctx.say(format!("Alert `#{id}` deleted.")).await?;
    Ok(())
}

// ----- endpoint sub-subcommand -----

#[poise::command(
    slash_command,
    prefix_command,
    subcommands("endpoint_list", "endpoint_remove", "endpoint_here")
)]
async fn endpoint(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say("Use one of: `list`, `remove`, `here`.").await?;
    Ok(())
}

/// List your delivery endpoints.
#[poise::command(slash_command, prefix_command, rename = "list")]
async fn endpoint_list(ctx: Context<'_>) -> Result<(), Error> {
    let owner = ctx.author().id.get() as i64;
    let endpoints = ctx.data().db.list_endpoints(owner).await?;
    if endpoints.is_empty() {
        ctx.say("You have no endpoints. Create one with `/ffxiv alert endpoint here` or `/ffxiv alert price` (which creates a DM endpoint automatically).").await?;
        return Ok(());
    }
    let lines = endpoints.iter().format_with("\n", |e, f| {
        let kind = match e.method.as_str() {
            "DiscordDm" => "DM",
            "DiscordChannel" => "channel",
            "Webhook" => "webhook",
            other => other,
        };
        f(&format_args!("`#{}` [{kind}] {}", e.id, e.name))
    });
    ctx.send(
        CreateReply::default().embed(
            CreateEmbed::new()
                .color(ULTROS_COLOR)
                .title("Your endpoints")
                .description(format!("{lines}")),
        ),
    )
    .await?;
    Ok(())
}

/// Delete an endpoint (also unlinks it from any alert).
#[poise::command(slash_command, prefix_command, rename = "remove")]
async fn endpoint_remove(
    ctx: Context<'_>,
    #[description = "Endpoint id (see `/ffxiv alert endpoint list`)"] id: i32,
) -> Result<(), Error> {
    let owner = ctx.author().id.get() as i64;
    ctx.data().db.delete_endpoint(owner, id).await?;
    ctx.say(format!("Endpoint `#{id}` deleted.")).await?;
    Ok(())
}

/// Register the current channel as a delivery endpoint and bind it to one of your alerts.
#[poise::command(slash_command, prefix_command, rename = "here")]
async fn endpoint_here(
    ctx: Context<'_>,
    #[description = "Alert id to also bind to this channel (optional)"] bind_to: Option<i32>,
) -> Result<(), Error> {
    let owner = ctx.author().id.get() as i64;
    let channel_id = ctx.channel_id().get() as i64;
    let name = format!("Channel {}", ctx.channel_id());
    let endpoint_id = ctx
        .data()
        .db
        .get_or_create_channel_endpoint(owner, channel_id, &name)
        .await?;
    if let Some(alert_id) = bind_to {
        // Replace the rules so this channel is also included
        let mut current = ctx
            .data()
            .db
            .list_endpoint_ids_for_alert(alert_id)
            .await?;
        if !current.contains(&endpoint_id) {
            current.push(endpoint_id);
        }
        ctx.data().db.set_alert_rules(owner, alert_id, &current).await?;
        ctx.say(format!(
            "Endpoint `#{endpoint_id}` registered for this channel and bound to alert `#{alert_id}`."
        ))
        .await?;
    } else {
        ctx.say(format!(
            "Endpoint `#{endpoint_id}` registered for this channel. Use `/ffxiv alert endpoint here bind_to:<alert_id>` to attach it to an alert."
        ))
        .await?;
    }
    Ok(())
}
```

- [ ] Verify `helpers::autocomplete_item`, `helpers::resolve_item_id`, `helpers::parse_world_selector`, `helpers::user_home_world_selector` exist; if not, write thin shims that delegate to the existing autocomplete/resolution patterns in `item_prices.rs` / `prices.rs`. Inline them in `helpers.rs` rather than re-inventing.
- [ ] Run `./check_ci.sh`. Expected: pass.
- [ ] Commit: `feat(bot): /ffxiv alert {price,list,mute,unmute,remove,endpoint}`

---

## Task 3: Register the `alert` subcommand

**File:** `ultros/src/discord/ffxiv/mod.rs`

- [ ] Add `mod alert;` and `use alert::alert;`
- [ ] Extend the `subcommands(...)` list on the `ffxiv` command:
  ```rust
  subcommands("character", "retainer", "analyze", "list", "prices", "rescan_market", "alert")
  ```
- [ ] Run `./check_ci.sh`.
- [ ] Commit: `feat(bot): register /ffxiv alert subcommand`

---

## Task 4: Helpers (if missing)

**File:** `ultros/src/discord/ffxiv/helpers.rs`

If any of `autocomplete_item`, `resolve_item_id`, `parse_world_selector`, `user_home_world_selector` don't exist:

- [ ] **autocomplete_item**: copy/adapt the existing item autocomplete used by `prices.rs`. If `prices.rs` has an inline `autocomplete_item_id`, factor it into helpers.rs.
- [ ] **resolve_item_id**: case-insensitive lookup against `xiv_gen_db::data().items` returning `Option<i32>`.
- [ ] **parse_world_selector**: tries `World`, then `Datacenter`, then `Region` via `ctx.data().world_cache`. Returns `AnySelector`.
- [ ] **user_home_world_selector**: reads from `ctx.data().db.get_discord_user(...)`'s home_world (look at existing patterns — `ctx.data().db.get_discord_user_for_id` or similar). If no home world, return Err.

Each helper is small. Commit: `refactor(bot): factor item/world helpers used by alert commands`.

---

## Task 5: Final CI + commit

- [ ] `./check_ci.sh` passes.
- [ ] All commits made.

---

## Out of scope

- Web push (Tier 3).
- Shared list notifications (Tier 4).
- Webhook endpoint creation from bot (`/ffxiv alert endpoint webhook <url>`) — only `here` (channel) + auto-DM are exposed in v1. Webhook stays web-only.
- Edit-name on endpoints. Just create/delete in v1.
- `mute <duration>` — for v1 just toggle enabled. Adding `muted_until` would require a new column.
