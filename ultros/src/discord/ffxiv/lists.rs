use super::{Context, Error};
use anyhow::anyhow;
use itertools::Itertools;
use ultros_db::world_cache::AnySelector;
use xiv_gen::ItemId;

#[poise::command(
    slash_command,
    prefix_command,
    subcommands(
        "create",
        "remove",
        "add_item",
        "remove_item",
        "show_list",
        "show_lists"
    )
)]
pub(crate) async fn list(ctx: Context<'_>) -> Result<(), Error> {
    ctx.send(|r| r.embed(|e| e.title("List").description("Get started with list create")))
        .await?;
    Ok(())
}

/// Shows the lists that you have
#[poise::command(slash_command, prefix_command)]
pub(crate) async fn show_lists(ctx: Context<'_>) -> Result<(), Error> {
    let user = ctx
        .data()
        .db
        .get_or_create_discord_user(ctx.author().id.0, ctx.author().name.clone())
        .await?;
    let lists = ctx.data().db.get_lists_for_user(user.id).await?;
    let names: Vec<_> = lists.into_iter().map(|l| l.name).collect();
    let names = names.join("\n");
    ctx.send(|r| r.embed(|e| e.title("Lists").description(names)))
        .await?;
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
        .create_list(user, list_name, Some(AnySelector::from(&result)))
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
    ctx.defer_ephemeral().await?;
    let user_lists = ctx
        .data()
        .db
        .get_lists_for_user(ctx.author().id.0 as i64)
        .await?;
    let lists = user_lists
        .into_iter()
        .find(|list| list.name == list_name)
        .ok_or(anyhow!("Failed to find list {list_name}"))?;
    ctx.data()
        .db
        .delete_list(lists.id, ctx.author().id.0 as i64)
        .await?;
    ctx.send(|msg| {
        msg.embed(|e| {
            e.title(format!("List `{}` Deleted!", list_name))
                .description("Create a new list with \n * `list create`")
        })
    })
    .await?;
    Ok(())
}

/// Add an item to a list
#[poise::command(slash_command, prefix_command)]
async fn add_item(
    ctx: Context<'_>,
    #[description = "name of the list to add an item to"] list_name: String,
    #[description = "item to add"] item_name: String,
    #[description = "quantity of the item to add. Leave blank for no quantity"] quantity: Option<
        i32,
    >,
    #[description = "hq? Leave blank for no filter"] hq: Option<bool>,
) -> Result<(), Error> {
    let author_id = ctx.author().id.0 as i64;
    let (i, item) = xiv_gen_db::data()
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
        .add_item_to_list(&list, author_id, i.0, hq, quantity, None)
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
) -> Result<(), Error> {
    let items = &xiv_gen_db::data().items;
    let author_id = ctx.author().id.0 as i64;
    let (id, _) = items
        .iter()
        .find(|(_, item)| item.name == item_name)
        .ok_or(anyhow!("Unable to find item"))?;
    let lists = ctx.data().db.get_lists_for_user(author_id).await?;
    let list = lists
        .into_iter()
        .find(|l| l.name == list_name)
        .ok_or(anyhow!("Unable to find list"))?;
    let item = ctx
        .data()
        .db
        .get_list_items(list.id, author_id)
        .await?
        .into_iter()
        .find(|i| i.item_id == id.0)
        .ok_or(anyhow!("Unable to find item on list"))?;
    ctx.data()
        .db
        .remove_item_from_list(author_id, item.id)
        .await?;
    Ok(())
}

#[poise::command(slash_command, prefix_command)]
async fn show_list(
    ctx: Context<'_>,
    #[description = "list to show"] list_name: String,
) -> Result<(), Error> {
    ctx.defer_or_broadcast().await?;
    let discord_user = ctx.author().id.0 as i64;
    let lists = ctx.data().db.get_lists_for_user(discord_user).await?;
    let list = lists
        .into_iter()
        .find(|list| list.name == list_name)
        .ok_or(anyhow!("Unable to get list"))?;
    let mut list_listings = ctx
        .data()
        .db
        .get_listings_for_list(discord_user, list.id, &ctx.data().world_cache)
        .await?;
    list_listings
        .iter_mut()
        .for_each(|(_, listings)| listings.sort_by_key(|(l, _)| l.price_per_unit));
    let items = &xiv_gen_db::data().items;
    let description = list_listings
        .iter()
        .map(|(list, listings)| {
            // get the item name, and first listing
            (
                items
                    .get(&ItemId(list.item_id))
                    .map(|i| i.name.as_str())
                    .unwrap_or_default(),
                listings
                    .first()
                    .map(|(l, _)| format!("{}", l.price_per_unit))
                    .unwrap_or_default(),
            )
        })
        .format_with("\n", |(item_name, price), f| {
            f(&format_args!("- {item_name} 🪙{price}"))
        })
        .to_string();
    ctx.send(|r| r.embed(|e| e.title(list.name.to_string()).description(description)))
        .await?;
    Ok(())
}
