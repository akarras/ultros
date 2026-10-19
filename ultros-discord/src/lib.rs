//! Discord transport and commands, independent of the web/SSR application.
#![recursion_limit = "256"]

mod ffxiv;
mod qa;

use ultros_alerts as alerts;
use ultros_analyzer::analyzer_service;
use ultros_character::character_claim;
use ultros_group_sync as group_sync;
use ultros_ingest::item_update_service;
use ultros_lists as lists;
#[cfg(feature = "test-auth")]
use ultros_server_core::test_market_isolation;
use ultros_server_core::{env_flag::env_flag_enabled, event, event::EventType};

pub use ffxiv::admin::spawn_market_reconciliation;

use chrono::Local;
use poise::{builtins::HelpConfiguration, serenity_prelude as serenity};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use ultros_api_types::world_helper::WorldHelper;
use ultros_clickhouse::ClickHouseClient;
use ultros_db::{UltrosDb, world_data::world_cache::WorldCache};

use crate::{
    analyzer_service::AnalyzerService, event::EventSenders, item_update_service::UpdateService,
};

pub type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

/// The server supplies its renderer so Discord never depends on the web app.
pub type PriceHistoryRenderer = Arc<
    dyn for<'a> Fn(
            &'a ClickHouseClient,
            &'a WorldCache,
            &'a WorldHelper,
            &'static xiv_gen::Item,
            &'a str,
        ) -> futures::future::BoxFuture<'a, Result<Vec<u8>, Error>>
        + Send
        + Sync,
>;
// User data, which is stored and accessible in all command invocations

pub(crate) struct Data {
    qa_scope: Option<qa::QaScope>,
    db: UltrosDb,
    list_sync: crate::lists::ListSync,
    lodestone_client: reqwest::Client,
    event_senders: EventSenders,
    analyzer_service: AnalyzerService,
    world_cache: Arc<WorldCache>,
    world_helper: Arc<WorldHelper>,
    update_service: Arc<UpdateService>,
    /// Backs `/prices history`'s chart image — same ClickHouse-sourced
    /// `PriceSeries` construction the web item-card PNG and JSON endpoint
    /// share through the injected renderer.
    ch_client: ClickHouseClient,
    price_history_renderer: PriceHistoryRenderer,
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
            // The global name, never the guild nickname: see
            // `group_sync::global_display_name`.
            &crate::group_sync::global_display_name(&new_member.user),
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
            // The global name, never the guild nickname: `discord_user` is
            // one row shared site-wide.
            &crate::group_sync::global_display_name(&update.user),
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

fn commands() -> Vec<poise::Command<Data, Error>> {
    vec![age(), register(), ping(), ffxiv::ffxiv()]
}

/// Shared application services supplied by the process hosting the bot.
pub struct DiscordServices {
    pub db: UltrosDb,
    pub list_sync: lists::ListSync,
    pub event_senders: EventSenders,
    pub analyzer_service: AnalyzerService,
    pub world_cache: Arc<WorldCache>,
    pub world_helper: Arc<WorldHelper>,
    pub update_service: Arc<UpdateService>,
    pub ch_client: ClickHouseClient,
    pub price_history_renderer: PriceHistoryRenderer,
}

pub async fn start_discord(
    services: DiscordServices,
    discord_token: String,
    token: CancellationToken,
) {
    let DiscordServices {
        db,
        list_sync,
        event_senders,
        analyzer_service,
        world_cache,
        world_helper,
        update_service,
        ch_client,
        price_history_renderer,
    } = services;
    let qa_scope = match qa::QaScope::from_env() {
        Ok(scope) => scope,
        Err(error) => {
            tracing::error!(%error, "Discord QA configuration is invalid; bot not started");
            return;
        }
    };
    let framework: poise::Framework<Data, Error> = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: commands(),
            skip_checks_for_owners: false,
            pre_command: |ctx| Box::pin(async move {
                tracing::info!(command = ctx.command().qualified_name,
                    channel_id = ctx.channel_id().get(), "Discord command started");
            }),
            command_check: Some(|ctx| Box::pin(async move {
                Ok(ctx.data().qa_scope.is_none_or(|scope|
                    scope.allows(ctx.guild_id().map(|guild| guild.get()), ctx.channel_id().get())))
            })),
            on_error: |error| Box::pin(async move {
                if let poise::FrameworkError::CommandCheckFailed { ctx, .. } = &error
                    && ctx.data().qa_scope.is_some_and(|scope|
                        !scope.allows(ctx.guild_id().map(|guild| guild.get()), ctx.channel_id().get())) {
                    return;
                }
                if let Err(error) = poise::builtins::on_error(error).await {
                    tracing::error!(%error, "Discord framework error handler failed");
                }
            }),
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
                let registration = if let Some(scope) = qa_scope {
                    tracing::info!(guild_id = scope.guild_id, channel_id = scope.channel_id,
                        "Discord QA enabled: registering guild commands and restricting command handling to the QA channel");
                    poise::builtins::register_in_guild(ctx, &framework.options().commands,
                        serenity::GuildId::new(scope.guild_id)).await
                } else {
                    poise::builtins::register_globally(ctx, &framework.options().commands).await
                };
                if let Err(e) = registration {
                    tracing::error!("failed to register Discord application commands: {e}");
                }
                tracing::info!(qa = qa_scope.is_some(), "Discord bot ready for commands");
                Ok(Data {
                    qa_scope,
                    db,
                    list_sync,
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
                    price_history_renderer,
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

    #[test]
    fn command_registration_preserves_retainer_options_and_admin_permissions() {
        let commands = commands();
        let ffxiv = commands.iter().find(|c| c.name == "ffxiv").unwrap();
        let retainer = ffxiv
            .subcommands
            .iter()
            .find(|c| c.name == "retainer")
            .unwrap();
        let add = retainer
            .subcommands
            .iter()
            .find(|c| c.name == "add")
            .unwrap();
        assert!(add.slash_action.is_some());
        assert_eq!(
            add.parameters
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["retainer", "world", "character"]
        );
        assert!(add.parameters.iter().all(|p| !p.required));
        let rescan = ffxiv
            .subcommands
            .iter()
            .find(|c| c.name == "rescan_market")
            .unwrap();
        assert!(rescan.owners_only);
        for name in ["character", "analyze", "list", "prices", "alert"] {
            assert!(ffxiv.subcommands.iter().any(|c| c.name == name));
        }
    }

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

    /// The name a synced member is stored under, and therefore the one every
    /// group and list share on the site shows them by.
    ///
    /// It is the *global* name, never the guild nickname. `discord_user` holds
    /// one row per person for the whole site, so taking the nickname would let
    /// any one server rename that person everywhere on Ultros — which is what
    /// this used to do, preferring `nick` the way `Member::display_name` does.
    #[test]
    fn a_members_stored_name_ignores_their_guild_nickname() {
        use crate::group_sync::global_display_name;

        let plain = user(serde_json::json!({}));
        let with_global = user(serde_json::json!({"global_name": "Global Name"}));

        assert_eq!(global_display_name(&with_global), "Global Name");
        assert_eq!(global_display_name(&plain), "raw_username");
    }
}
