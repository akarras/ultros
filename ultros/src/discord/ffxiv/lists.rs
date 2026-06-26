use super::{Context, Error};
use crate::discord::ffxiv::helpers::{
    discord_locale_to_xiv_language, localized_item_matches, localized_item_name,
    resolve_item_id_any_locale, truncate_100,
};
use anyhow::anyhow;
use itertools::Itertools;
use poise::serenity_prelude::User;
use ultros_api_types::list::ListPermission;
use ultros_db::world_data::world_cache::AnySelector;
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
        "show_lists",
        "share_user",
        "create_invite",
        "redeem_invite"
    )
)]
pub(crate) async fn list(ctx: Context<'_>) -> Result<(), Error> {
    ctx.send(
        poise::CreateReply::default().embed(
            poise::serenity_prelude::CreateEmbed::new()
                .title("List")
                .description("Get started with `list create`, then share with `list share_user` or `list create_invite`."),
        ),
    )
    .await?;
    Ok(())
}

fn permission_name(permission: ListPermission) -> &'static str {
    match permission {
        ListPermission::None => "None",
        ListPermission::Read => "Read",
        ListPermission::Write => "Write",
        ListPermission::Owner => "Owner",
    }
}

fn parse_share_permission(permission: Option<String>) -> Result<ListPermission, Error> {
    match permission
        .unwrap_or_else(|| "read".to_string())
        .to_lowercase()
        .as_str()
    {
        "read" | "r" => Ok(ListPermission::Read),
        "write" | "w" => Ok(ListPermission::Write),
        value => Err(anyhow!("Unsupported permission `{value}`. Use `read` or `write`.").into()),
    }
}

/// Shows the lists that you have
#[poise::command(slash_command, prefix_command)]
pub(crate) async fn show_lists(ctx: Context<'_>) -> Result<(), Error> {
    let user = ctx
        .data()
        .db
        .get_or_create_discord_user(ctx.author().id.get(), ctx.author().name.clone())
        .await?;
    let lists = ctx.data().db.get_lists_for_user(user.id).await?;
    let mut names = Vec::with_capacity(lists.len());
    for list in lists {
        let permission = ctx.data().db.get_permission(list.id, user.id).await?;
        names.push(format!("{} ({})", list.name, permission_name(permission)));
    }
    let names = names.join("\n");
    ctx.send(
        poise::CreateReply::default().embed(
            poise::serenity_prelude::CreateEmbed::new()
                .title("Lists")
                .description(names),
        ),
    )
    .await?;
    Ok(())
}

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
        .map(|l| {
            poise::serenity_prelude::AutocompleteChoice::new(
                truncate_100(&l.name),
                truncate_100(&l.name),
            )
        })
}

async fn autocomplete_item_name_global(
    ctx: Context<'_>,
    partial: &str,
) -> impl Iterator<Item = poise::serenity_prelude::AutocompleteChoice> {
    let user_lang = discord_locale_to_xiv_language(ctx.locale());
    let en = xiv_gen_db::data_for(xiv_gen::Language::En);
    localized_item_matches(partial, user_lang)
        .into_iter()
        .filter_map(move |m| {
            let en_name = en.items.get(&ItemId(m.item_id))?.name.to_string();
            if en_name.is_empty() {
                return None;
            }
            Some(poise::serenity_prelude::AutocompleteChoice::new(
                m.label,
                truncate_100(&en_name),
            ))
        })
        .take(25)
        .collect::<Vec<_>>()
        .into_iter()
}

/// Share a list directly with a Discord user
#[poise::command(slash_command, prefix_command)]
async fn share_user(
    ctx: Context<'_>,
    #[description = "Name of the list to share"]
    #[autocomplete = "autocomplete_list_name"]
    list_name: String,
    #[description = "Discord user to share with"] user: User,
    #[description = "read or write. Defaults to read"] permission: Option<String>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let owner = ctx
        .data()
        .db
        .get_or_create_discord_user(ctx.author().id.get(), ctx.author().name.clone())
        .await?;
    let target = ctx
        .data()
        .db
        .get_or_create_discord_user(user.id.get(), user.name.clone())
        .await?;
    let permission = parse_share_permission(permission)?;
    let list = ctx
        .data()
        .db
        .get_list_by_name_for_user(owner.id, &list_name)
        .await?
        .ok_or(anyhow!("Unable to find list `{list_name}`"))?;

    ctx.data()
        .db
        .share_list_with_user(list.id, owner.id, target.id, permission)
        .await?;
    ctx.send(
        poise::CreateReply::default().embed(
            poise::serenity_prelude::CreateEmbed::new()
                .title("List shared")
                .description(format!(
                    "`{}` shared with {} as {}",
                    list.name,
                    user.name,
                    permission_name(permission)
                )),
        ),
    )
    .await?;
    Ok(())
}

/// Create an invite token for a list
#[poise::command(slash_command, prefix_command)]
async fn create_invite(
    ctx: Context<'_>,
    #[description = "Name of the list to invite people to"]
    #[autocomplete = "autocomplete_list_name"]
    list_name: String,
    #[description = "read or write. Defaults to read"] permission: Option<String>,
    #[description = "Maximum invite uses. Leave blank for unlimited"] max_uses: Option<i32>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let owner = ctx
        .data()
        .db
        .get_or_create_discord_user(ctx.author().id.get(), ctx.author().name.clone())
        .await?;
    let permission = parse_share_permission(permission)?;
    let list = ctx
        .data()
        .db
        .get_list_by_name_for_user(owner.id, &list_name)
        .await?
        .ok_or(anyhow!("Unable to find list `{list_name}`"))?;
    let invite = ctx
        .data()
        .db
        .create_invite(list.id, owner.id, permission, max_uses)
        .await?;

    ctx.send(
        poise::CreateReply::default().embed(
            poise::serenity_prelude::CreateEmbed::new()
                .title("List invite created")
                .description(format!(
                    "Invite token: `{}`\nList: `{}`\nPermission: {}",
                    invite.id,
                    list.name,
                    permission_name(permission)
                )),
        ),
    )
    .await?;
    Ok(())
}

/// Redeem a list invite token
#[poise::command(slash_command, prefix_command)]
async fn redeem_invite(
    ctx: Context<'_>,
    #[description = "Invite token"] invite: String,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let user = ctx
        .data()
        .db
        .get_or_create_discord_user(ctx.author().id.get(), ctx.author().name.clone())
        .await?;
    let share = ctx.data().db.use_invite(invite, user.id).await?;
    ctx.send(
        poise::CreateReply::default().embed(
            poise::serenity_prelude::CreateEmbed::new()
                .title("List invite redeemed")
                .description(format!(
                    "You now have {} access to list #{}.",
                    permission_name(ListPermission::from(share.permission)),
                    share.list_id
                )),
        ),
    )
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
        .get_or_create_discord_user(discord_user.get(), ctx.author().name.clone())
        .await?;
    let list = ctx
        .data()
        .db
        .create_list(user, list_name, Some(AnySelector::from(&result)))
        .await?;
    ctx.send(
        poise::CreateReply::default().embed(
            poise::serenity_prelude::CreateEmbed::new()
                .title(format!("List `{}` Created!", list.name))
                .description("Follow up commands\n* `list add_item`\n* `list view`"),
        ),
    )
    .await?;
    Ok(())
}

/// Remove a list
#[poise::command(slash_command, prefix_command)]
async fn remove(
    ctx: Context<'_>,
    #[description = "Name of the list to remove"]
    #[autocomplete = "autocomplete_list_name"]
    list_name: String,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let lists = ctx
        .data()
        .db
        .get_list_by_name_for_user(ctx.author().id.get() as i64, &list_name)
        .await?
        .ok_or(anyhow!("Failed to find list {list_name}"))?;
    ctx.data()
        .db
        .delete_list(lists.id, ctx.author().id.get() as i64)
        .await?;
    ctx.send(
        poise::CreateReply::default().embed(
            poise::serenity_prelude::CreateEmbed::new()
                .title(format!("List `{}` Deleted!", list_name))
                .description("Create a new list with \n * `list create`"),
        ),
    )
    .await?;
    Ok(())
}

/// Add an item to a list
#[poise::command(slash_command, prefix_command)]
async fn add_item(
    ctx: Context<'_>,
    #[description = "name of the list to add an item to"]
    #[autocomplete = "autocomplete_list_name"]
    list_name: String,
    #[description = "item to add"]
    #[autocomplete = "autocomplete_item_name_global"]
    item_name: String,
    #[description = "quantity of the item to add. Leave blank for no quantity"] quantity: Option<
        i32,
    >,
    #[description = "hq? Leave blank for no filter"] hq: Option<bool>,
) -> Result<(), Error> {
    let author_id = ctx.author().id.get() as i64;
    let user_lang = discord_locale_to_xiv_language(ctx.locale());
    let item_id = resolve_item_id_any_locale(&item_name).ok_or(anyhow!("Unable to find item"))?;
    let display_name = localized_item_name(item_id, user_lang);
    let list = ctx
        .data()
        .db
        .get_list_by_name_for_user(author_id, &list_name)
        .await?
        .ok_or(anyhow!("List not found"))?;
    ctx.data()
        .db
        .add_item_to_list(&list, author_id, item_id, hq, quantity, None)
        .await?;
    ctx.send(
        poise::CreateReply::default().embed(
            poise::serenity_prelude::CreateEmbed::new()
                .title("Item added")
                .description(format!("{} added to list {}", display_name, list.name)),
        ),
    )
    .await?;
    Ok(())
}

/// Remove an item from a list
#[poise::command(slash_command, prefix_command)]
async fn remove_item(
    ctx: Context<'_>,
    #[description = "name of the list to remove an item from"]
    #[autocomplete = "autocomplete_list_name"]
    list_name: String,
    #[description = "item to remove"] item_name: String,
) -> Result<(), Error> {
    let author_id = ctx.author().id.get() as i64;
    let id = resolve_item_id_any_locale(&item_name).ok_or(anyhow!("Unable to find item"))?;
    let list = ctx
        .data()
        .db
        .get_list_by_name_for_user(author_id, &list_name)
        .await?
        .ok_or(anyhow!("Unable to find list"))?;
    let item = ctx
        .data()
        .db
        .get_list_items(list.id, author_id)
        .await?
        .into_iter()
        .find(|i| i.item_id == id)
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
    #[description = "list to show"]
    #[autocomplete = "autocomplete_list_name"]
    list_name: String,
) -> Result<(), Error> {
    ctx.defer_or_broadcast().await?;
    let discord_user = ctx.author().id.get() as i64;
    let list = ctx
        .data()
        .db
        .get_list_by_name_for_user(discord_user, &list_name)
        .await?
        .ok_or(anyhow!("Unable to get list"))?;
    let mut list_listings = ctx
        .data()
        .db
        .get_listings_for_list(discord_user, list.id, &ctx.data().world_cache)
        .await?;
    list_listings
        .iter_mut()
        .for_each(|(_, listings)| listings.sort_by_key(|(l, _)| l.price_per_unit));
    let user_lang = discord_locale_to_xiv_language(ctx.locale());
    let description = list_listings
        .iter()
        .map(|(list, listings)| {
            (
                localized_item_name(list.item_id, user_lang),
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
    ctx.send(
        poise::CreateReply::default().embed(
            poise::serenity_prelude::CreateEmbed::new()
                .title(list.name.to_string())
                .description(description),
        ),
    )
    .await?;
    Ok(())
}
