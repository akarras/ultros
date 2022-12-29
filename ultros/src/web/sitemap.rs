use super::error::WebError;
use crate::analyzer_service::AnalyzerService;
use anyhow::anyhow;
use axum::extract::{Path, State};
use itertools::Itertools;
use sitemap_rs::{sitemap::Sitemap, sitemap_index::SitemapIndex};
use std::{collections::HashSet, sync::Arc};
use ultros_db::world_cache::{AnyResult, AnySelector, WorldCache};

pub(crate) async fn sitemap_index(
    State(world_cache): State<Arc<WorldCache>>,
) -> Result<Vec<u8>, WebError> {
    // Get all the worlds from the world cache and then populate the listings sitemap to point to all the world subsitemaps
    let listings_sitemaps: Vec<_> = world_cache
        .get_all()
        .iter()
        .flat_map(|(r, dcs)| {
            [AnyResult::Region(r)]
                .into_iter()
                .chain(dcs.iter().flat_map(|(dc, worlds)| {
                    [AnyResult::Datacenter(dc)]
                        .into_iter()
                        .chain(worlds.iter().map(|w| AnyResult::World(w)))
                }))
        })
        .map(|name| {
            Sitemap::new(
                format!("https://ultros.app/sitemap/world/{}.xml", name.get_name()),
                None,
            )
        })
        .collect();
    let index = SitemapIndex::new(listings_sitemaps)?;
    let mut index_string = Vec::new();
    index
        .write(&mut index_string)
        .map_err(|_| anyhow!("Error creating sitemap"))?;
    Ok(index_string)
}

pub(crate) async fn world_sitemap(
    State(db): State<AnalyzerService>,
    State(world_cache): State<Arc<WorldCache>>,
    Path(world_name): Path<String>,
) -> Result<String, WebError> {
    // validate that this is a valid world name, then repeat back a sitemap using all the item ids
    world_cache.lookup_value_by_name(&world_name)?;
    let items: HashSet<_> = db
        .read_cheapest_items(&AnySelector::World(99), |items| {
            items.item_map.keys().map(|k| k.item_id).collect()
        })
        .await?;
    Ok(items
        .iter()
        .format_with("\n", |i, f| {
            f(&format_args!(
                "https://ultros.app/listings/{world_name}/{i}"
            ))
        })
        .to_string())
}
