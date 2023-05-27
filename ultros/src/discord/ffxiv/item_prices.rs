use anyhow::anyhow;
use itertools::Itertools;
use plotters_svg::SVGBackend;
use poise::serenity_prelude::AttachmentType;
use resvg::tiny_skia;
use resvg::usvg::{self, fontdb, TreeParsing, TreeTextToPath};
use ultros_api_types::SaleHistory;
use ultros_db::world_cache::AnySelector;
use xiv_gen::ItemId;

use super::{Context, Error};

/// Lookup price information from the marketboard
#[poise::command(slash_command, prefix_command, subcommands("current", "history"))]
pub(crate) async fn prices(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

async fn autocomplete_item<'a>(
    _ctx: Context<'_>,
    partial: &'a str,
) -> impl Iterator<Item = poise::AutocompleteChoice<i32>> + 'a {
    let items = xiv_gen_db::decompress_data().items.values();
    let partial = partial.to_lowercase();
    items
        .filter(move |item| item.name.to_lowercase().contains(&partial))
        .map(|item| poise::AutocompleteChoice {
            name: item.name.to_string(),
            value: item.key_id.0,
        })
        .take(99)
}

async fn autocomplete_world<'a>(
    ctx: Context<'a>,
    partial: &'a str,
) -> impl Iterator<Item = String> + 'a {
    let partial = partial.to_lowercase();
    ctx.data()
        .world_cache
        .get_all_results()
        .filter(move |w| w.get_name().to_lowercase().contains(&partial))
        .map(|w| w.get_name().to_string())
        .take(99)
}

/// Get the real time prices from highest to lowest
#[poise::command(slash_command, prefix_command)]
async fn current(
    ctx: Context<'_>,
    #[autocomplete = "autocomplete_item"] item: i32,
    #[autocomplete = "autocomplete_world"] world: String,
    hq_only: Option<bool>,
) -> Result<(), Error> {
    ctx.defer().await?;
    let worlds = &ctx.data().world_cache;
    let world = worlds.lookup_value_by_name(&world)?;
    let world_ids = worlds
        .get_all_worlds_in(&world)
        .ok_or(anyhow!("bad world data"))?;
    let item_data = xiv_gen_db::decompress_data()
        .items
        .get(&ItemId(item))
        .ok_or(anyhow!("bad item id"))?;
    let mut listings = ctx
        .data()
        .db
        .get_all_listings_in_worlds(&world_ids, universalis::ItemId(item))
        .await?;
    listings.sort_by_key(|l| l.price_per_unit);
    let listings = listings
        .into_iter()
        .filter(|w| hq_only.map(|hq| w.hq == hq).unwrap_or(true))
        .take(10)
        .format_with("\n", |l, f| {
            f(&format_args!(
                "{:<10} {:3} {:<7} {}",
                l.price_per_unit,
                l.hq.then(|| "✅").unwrap_or_default(),
                l.quantity,
                worlds
                    .lookup_selector(&AnySelector::World(l.world_id))
                    .as_ref()
                    .map(|w| w.get_name())
                    .unwrap_or_default()
            ))
        })
        .to_string();
    ctx.send(|msg| {
        msg.embed(|e| {
            e.title(&item_data.name).description(format!(
                "```\n{:<10} {:3} {:<7} {}\n{}\n```",
                "price", "hq", "quantity", "world", listings,
            ))
        })
    })
    .await?;
    Ok(())
}

/// Get the recent prices for an item
#[poise::command(slash_command, prefix_command)]
async fn history(
    ctx: Context<'_>,
    #[autocomplete = "autocomplete_item"]
    #[description = "Item to get the price history for"]
    item: i32,
    #[description = "World, Datacenter, or Region to get prices for"]
    #[autocomplete = "autocomplete_world"]
    world: String,
) -> Result<(), Error> {
    ctx.defer().await?;
    let item = xiv_gen_db::decompress_data()
        .items
        .get(&ItemId(item))
        .ok_or(anyhow!("Invalid item id"))?;
    let world = ctx.data().world_cache.lookup_value_by_name(&world)?;
    let world_ids = ctx
        .data()
        .world_cache
        .get_all_worlds_in(&world)
        .ok_or(anyhow!("invalid world"))?;
    let sales: Vec<SaleHistory> = ctx
        .data()
        .db
        .get_sale_history_from_multiple_worlds(world_ids.into_iter(), item.key_id.0, 1000)
        .await?
        .into_iter()
        .map(|sales| SaleHistory::from(sales))
        .collect();
    const SIZE: (u32, u32) = (1920 / 3, 1080 / 3);
    let buffer = {
        let mut buffer = String::new();
        // let mut image = RgbImage::new(size.0, size.1);

        let backend = SVGBackend::with_string(&mut buffer, SIZE);

        let world_helper = &*ctx.data().world_helper;
        if let Err(e) = ultros_charts::draw_sale_history_scatter_plot(backend, world_helper, &sales)
        {
            Err(anyhow!("can't draw scatter plot {e}"))?
        }
        buffer
    };

    let png = {
        let mut opt = usvg::Options::default();
        // Get file's absolute directory.
        opt.resources_dir = std::fs::canonicalize(&buffer)
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()));

        let mut fontdb = fontdb::Database::new();
        fontdb.load_system_fonts();

        let mut tree = usvg::Tree::from_str(&buffer, &opt).unwrap();
        tree.convert_text(&fontdb);
        let rtree = resvg::Tree::from_usvg(&tree);
        let pixmap_size = resvg::IntSize::from_usvg(rtree.size);
        let mut pixmap = tiny_skia::Pixmap::new(pixmap_size.width(), pixmap_size.height())
            .ok_or(anyhow!("failed to make pixmap"))?;
        rtree.render(tiny_skia::Transform::default(), &mut pixmap.as_mut());
        pixmap.encode_png()?
    };

    let attachment = AttachmentType::Bytes {
        data: png.into(),
        filename: "chart.png".to_string(),
    };
    ctx.send(|r| {
        r.embed(|e| e.title(&item.name).image("attachment://chart.png"))
            .attachment(attachment)
    })
    .await?;
    Ok(())
}
