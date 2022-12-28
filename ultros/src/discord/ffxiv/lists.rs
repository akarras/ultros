use super::{Context, Error};
use anyhow::anyhow;
use ultros_db::world_cache::AnySelector;

#[poise::command(slash_command, subcommands("create", "remove", "add_item", "remove_item"))]
pub(crate) async fn list(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say("Hello world").await?;
    Ok(())
}

/// Create a list of items
#[poise::command(slash_command, prefix_command)]
async fn create(
    ctx: Context<'_>,
    #[description = "Name of the list to create"] list_name: String,
    #[description = "Region/Datacenter/World for the list"] region_datacenter_or_world: String,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let discord_user = ctx.author().id;
    let result = ctx
        .data()
        .world_cache
        .lookup_value_by_name(&region_datacenter_or_world)?;
    let user = ctx
        .data()
        .db
        .get_or_create_discord_user(discord_user.0, ctx.author().name.clone())
        .await?;
    let list = ctx
        .data()
        .db
        .create_list(user, list_name, AnySelector::from(&result))
        .await?;
    ctx.send(|r| {
        r.embed(|e| {
            e.title(format!("List `{}` Created!", list.name))
                .description("Follow up commands\n* `list add_item`\n* `list view`")
        })
    })
    .await?;
    Ok(())
}

/// Remove a list
#[poise::command(slash_command, prefix_command)]
async fn remove(
    ctx: Context<'_>,
    #[description = "Name of the list to remove"] list_name: String,
) -> Result<(), Error> {
    Ok(())
}

/// Add an item to a list
#[poise::command(slash_command, prefix_command)]
async fn add_item(
    ctx: Context<'_>,
    #[description = "name of the list to add an item to"] list_name: String,
    #[description = "item to add"] item_name: String,
    #[description = "hq? Leave blank for no filter"] hq: Option<bool>,
) -> Result<(), Error> {
    let author_id = ctx.author().id.0 as i64;
    let (i, item) = xiv_gen_db::decompress_data()
        .items
        .iter()
        .find(|(_, i)| i.name == item_name)
        .ok_or(anyhow!("Unable to find item"))?;
    let list = ctx
        .data()
        .db
        .get_lists_for_user(author_id)
        .await?
        .into_iter()
        .find(|l| l.name == list_name)
        .ok_or(anyhow!("List not found"))?;
    ctx.data()
        .db
        .add_item_to_list(&list, author_id, i.0, hq)
        .await?;
    ctx.send(|s| {
        s.embed(|e| {
            e.title("Item added")
                .description(format!("{} added to list {}", item.name, list.name))
        })
    })
    .await?;
    Ok(())
}

/// Remove an item from a list
#[poise::command(slash_command, prefix_command)]
async fn remove_item(
    ctx: Context<'_>,
    #[description = "name of the list to remove an item from"] list_name: String,
    #[description = "item to remove"] item_name: String,
    #[description = "hq? Leave blank for no filter"] hq: Option<bool>,
) -> Result<(), Error> {
    Ok(())
}