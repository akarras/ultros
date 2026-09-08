pub(crate) mod ffxiv;

use chrono::Local;
use poise::{builtins::HelpConfiguration, serenity_prelude as serenity};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use ultros_api_types::world_helper::WorldHelper;
use ultros_clickhouse::ClickHouseClient;
use ultros_db::{UltrosDb, world_data::world_cache::WorldCache};

use crate::{
    alerts::alert_manager::AlertManager,
    analyzer_service::AnalyzerService,
    event::{EventReceivers, EventSenders},
    item_update_service::UpdateService,
};

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;
// User data, which is stored and accessible in all command invocations

pub(crate) struct Data {
    db: UltrosDb,
    lodestone_client: reqwest::Client,
    event_senders: EventSenders,
    analyzer_service: AnalyzerService,
    world_cache: Arc<WorldCache>,
    world_helper: Arc<WorldHelper>,
    update_service: Arc<UpdateService>,
    /// Backs `/prices history`'s chart image — same ClickHouse-sourced
    /// `PriceSeries` construction the web item-card PNG and JSON endpoint
    /// share (`crate::web::build_price_series`).
    ch_client: ClickHouseClient,
}

#[poise::command(slash_command, prefix_command)]
async fn help(
    ctx: Context<'_>,
    #[description = "Specific command to show help for"] command: Option<String>,
) -> Result<(), Error> {
    let config = HelpConfiguration {
        extra_text_at_bottom: "ultros 🦑",
        ..Default::default()
    };
    poise::builtins::help(ctx, command.as_deref(), config).await?;
    Ok(())
}

#[poise::command(slash_command, prefix_command)]
async fn ping(ctx: Context<'_>) -> Result<(), Error> {
    let timestamp = ctx.created_at();
    let duration = timestamp.signed_duration_since(Local::now());
    ctx.say(format!(
        "ping received in : {}ms",
        duration.num_milliseconds()
    ))
    .await?;
    Ok(())
}

#[poise::command(slash_command, prefix_command)]
async fn age(
    ctx: Context<'_>,
    #[description = "Selected user"] user: Option<serenity::User>,
) -> Result<(), Error> {
    let u = user.as_ref().unwrap_or_else(|| ctx.author());
    let response = format!("{}'s account was created at {}", u.name, u.created_at());
    ctx.say(response).await?;
    Ok(())
}

#[poise::command(prefix_command)]
async fn register(ctx: Context<'_>) -> Result<(), Error> {
    poise::builtins::register_application_commands_buttons(ctx).await?;
    Ok(())
}

/// A member's name as Discord would show it in the guild: nickname, then the
/// global display name, then the username. Mirrors `serenity::Member`'s own
/// `display_name`, which the raw update event has no equivalent of.
fn display_name(nick: Option<&str>, user: &serenity::User) -> String {
    nick.or(user.global_name.as_deref())
        .unwrap_or(user.name.as_str())
        .to_string()
}

/// Gateway events that feed group membership sync.
///
/// Every branch that touches the database opens with one indexed early-out, so
/// events from the guilds that have no synced roles — which is most of them —
/// cost a single query and nothing else. The handlers themselves live in
/// [`crate::group_sync::events`], taking parsed fields rather than a context,
/// so they can be tested without a gateway connection; this function is only
/// the unwrapping.
///
/// Failures are logged and swallowed. None of this is load-bearing for the
/// bot's other duties, and reconciliation will pick up whatever was dropped.
async fn handle_event(event: &serenity::FullEvent, data: &Data) {
    use crate::group_sync::events;

    let db = &data.db;
    let result: Result<(), anyhow::Error> = match event {
        serenity::FullEvent::GuildMemberAddition { new_member } => events::on_member_upsert(
            db,
            new_member.guild_id.get() as i64,
            new_member.user.id.get() as i64,
            new_member.display_name(),
            &new_member
                .roles
                .iter()
                .map(|role| role.get() as i64)
                .collect::<Vec<_>>(),
        )
        .await
        .map(|_| ()),
        // The raw event is used rather than `new`, which is only populated
        // from the cache: a role grant has to be handled whether or not the
        // member happens to be cached.
        serenity::FullEvent::GuildMemberUpdate { event: update, .. } => events::on_member_upsert(
            db,
            update.guild_id.get() as i64,
            update.user.id.get() as i64,
            &display_name(update.nick.as_deref(), &update.user),
            &update
                .roles
                .iter()
                .map(|role| role.get() as i64)
                .collect::<Vec<_>>(),
        )
        .await
        .map(|_| ()),
        serenity::FullEvent::GuildMemberRemoval { guild_id, user, .. } => {
            events::on_member_removal(db, guild_id.get() as i64, user.id.get() as i64)
                .await
                .map(|_| ())
        }
        serenity::FullEvent::GuildRoleDelete {
            guild_id,
            removed_role_id,
            ..
        } => events::on_role_delete(db, guild_id.get() as i64, removed_role_id.get() as i64).await,
        // `unavailable` separates a Discord outage from a real removal. Only
        // the latter freezes the group.
        serenity::FullEvent::GuildDelete { incomplete, .. } => {
            events::on_guild_delete(db, incomplete.id.get() as i64, incomplete.unavailable)
                .await
                .map(|frozen| {
                    if let Some(group_id) = frozen {
                        tracing::warn!(
                            guild_id = incomplete.id.get(),
                            group_id,
                            "the bot was removed from a guild; its group is frozen and \
                             membership is now manual"
                        );
                    }
                })
        }
        serenity::FullEvent::GuildCreate { guild, .. } => {
            events::on_guild_create(guild.id.get() as i64);
            Ok(())
        }
        _ => Ok(()),
    };
    if let Err(error) = result {
        tracing::warn!("group membership sync failed to handle a gateway event: {error:?}");
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn start_discord(
    db: UltrosDb,
    event_senders: EventSenders,
    event_receivers: EventReceivers,
    analyzer_service: AnalyzerService,
    world_cache: Arc<WorldCache>,
    world_helper: Arc<WorldHelper>,
    update_service: Arc<UpdateService>,
    discord_token: String,
    token: CancellationToken,
    ch_client: ClickHouseClient,
) {
    let setup_token = token.clone();
    let framework: poise::Framework<Data, Error> = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![age(), register(), ping(), ffxiv::ffxiv()],
            event_handler: |_ctx, event, _framework, data| {
                Box::pin(async move {
                    handle_event(event, data).await;
                    Ok(())
                })
            },
            ..Default::default()
        })
        .setup(move |ctx: &serenity::Context, _ready, framework| {
            Box::pin(async move {
                // Make the live serenity context available to web handlers
                // (endpoint test + alert-event resend) before slash-command
                // registration. Command registration can fail independently of
                // the gateway connection, and alert delivery should still work.
                crate::alerts::delivery::set_serenity_ctx(ctx.clone());
                if let Err(e) =
                    poise::builtins::register_globally(ctx, &framework.options().commands).await
                {
                    tracing::error!("failed to register Discord application commands: {e}");
                }
                let (item_events, alert_events) = (
                    (
                        event_receivers.retainers.resubscribe(),
                        event_receivers.listings.resubscribe(),
                    ),
                    (
                        event_receivers.alerts.resubscribe(),
                        event_receivers.retainer_undercut.resubscribe(),
                    ),
                );
                tokio::spawn(AlertManager::start_manager(
                    db.clone(),
                    item_events,
                    alert_events,
                    (
                        event_receivers.lists.resubscribe(),
                        event_receivers.history.resubscribe(),
                    ),
                    ctx.clone(),
                    setup_token,
                    world_cache.clone(),
                ));
                Ok(Data {
                    db,
                    lodestone_client: reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(10))
                        .build()
                        .unwrap(),
                    event_senders,
                    analyzer_service,
                    world_cache,
                    world_helper,
                    update_service,
                    ch_client,
                })
            })
        })
        .build();

    // GUILD_MEMBERS is privileged and must be enabled for the application in
    // the Discord developer portal before this process starts, or the gateway
    // refuses the connection outright. It is what delivers member add/update/
    // remove events and what makes the guild member list readable, which is
    // the whole basis of group membership sync.
    let mut client = serenity::Client::builder(
        discord_token,
        serenity::GatewayIntents::non_privileged() | serenity::GatewayIntents::GUILD_MEMBERS,
    )
    .framework(framework)
    .await
    .unwrap();
    let shard_manager = client.shard_manager.clone();
    tokio::spawn(async move {
        token.cancelled().await;
        shard_manager.shutdown_all().await;
    });

    client.start().await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(overrides: serde_json::Value) -> serenity::User {
        let mut base = serde_json::json!({
            "id": "1", "username": "raw_username", "discriminator": "0",
            "global_name": null, "avatar": null, "bot": false
        });
        let serde_json::Value::Object(overrides) = overrides else {
            panic!("user overrides must be an object");
        };
        for (key, value) in overrides {
            base[key] = value;
        }
        serde_json::from_value(base).unwrap()
    }

    /// The name a synced member is stored under, and therefore the one the
    /// group page shows. `GuildMemberUpdate` carries the raw payload rather
    /// than a `Member`, so this precedence has to be reproduced by hand and is
    /// easy to get subtly wrong.
    #[test]
    fn a_members_name_prefers_nickname_then_global_name_then_username() {
        let plain = user(serde_json::json!({}));
        let with_global = user(serde_json::json!({"global_name": "Global Name"}));

        assert_eq!(
            display_name(Some("Server Nick"), &with_global),
            "Server Nick"
        );
        assert_eq!(display_name(None, &with_global), "Global Name");
        assert_eq!(display_name(None, &plain), "raw_username");
        assert_eq!(display_name(Some("Server Nick"), &plain), "Server Nick");
    }
}
