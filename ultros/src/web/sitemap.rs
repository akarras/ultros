use super::error::WebError;
use crate::analyzer_service::AnalyzerService;
use axum::extract::{Path, State};
use itertools::Itertools;
use std::{collections::HashSet, sync::Arc};
use ultros_db::world_cache::{AnySelector, WorldCache};

pub(crate) async fn listings(State(world_cache): State<Arc<WorldCache>>) {
  // Get all the worlds from the world cache and then populate the listings sitemap to point to all the world subsitemaps
  todo!("Implement");
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
