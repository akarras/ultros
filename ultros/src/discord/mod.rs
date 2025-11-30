pub(crate) mod ffxiv;

use chrono::Local;
use poise::{builtins::HelpConfiguration, serenity_prelude as serenity};
use std::sync::Arc;
use ultros_api_types::world_helper::WorldHelper;
use ultros_db::{UltrosDb, world_cache::WorldCache};

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
) {
    let framework: poise::Framework<Data, Error> = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![age(), register(), ping(), ffxiv::ffxiv()],
            ..Default::default()
        })
        .setup(move |ctx: &serenity::Context, _ready, framework| {
            Box::pin(async move {
                // start the alert monitor
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
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
                    ctx.clone(),
                ));
                Ok(Data {
                    db,
                    lodestone_client: reqwest::Client::new(),
                    event_senders,
                    analyzer_service,
                    world_cache,
                    world_helper,
                    update_service,
                })
            })
        })
        .build();

    let mut client =
        serenity::Client::builder(discord_token, serenity::GatewayIntents::non_privileged())
            .framework(framework)
            .await
            .unwrap();

    client.start().await.unwrap();
}
