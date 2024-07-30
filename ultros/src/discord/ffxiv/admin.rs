use super::{Context, Error};

#[poise::command(slash_command, prefix_command, owners_only)]
pub(crate) async fn rescan_market(ctx: Context<'_>) -> Result<(), Error> {
    ctx.reply("Beginning scan").await?;
    ctx.data().update_service.do_full_world_sweep().await?;
    ctx.reply("Scan finished.").await?;
    Ok(())
}
