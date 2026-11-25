use super::error::WebError;
use crate::{
    analyzer_service::AnalyzerService,
    market_client::{MarketClient, SitemapItemSaleSummary},
};
use anyhow::anyhow;
use axum::{
    extract::State,
    http::HeaderValue,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use futures::future::try_join_all;
use hyper::header;
use itertools::Itertools;
use jiff::{Timestamp, tz::TimeZone};
use mime_guess::mime;
use sitemap_rs::{
    image::Image,
    sitemap::Sitemap,
    sitemap_index::SitemapIndex,
    url::{ChangeFrequency, Url},
    url_set::UrlSet,
};
use std::{collections::HashMap, sync::Arc};
use ultros_api_types::world_helper::WorldHelper;
use ultros_db::world_data::world_cache::AnySelector;

pub(crate) struct Xml(Vec<u8>);

impl IntoResponse for Xml {
    fn into_response(self) -> Response {
        let mut response = self.0.into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(mime::XML.as_ref()),
        );
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("max-age=86400"),
        );
        response
    }
}

pub(crate) async fn sitemap_index() -> Result<Xml, WebError> {
    let sitemap_list = vec![
        Sitemap::new("https://ultros.app/sitemap/pages.xml".to_string(), None),
        Sitemap::new("https://ultros.app/sitemap/items.xml".to_string(), None),
        Sitemap::new("https://ultros.app/sitemap/recipes.xml".to_string(), None),
        Sitemap::new("https://ultros.app/sitemap/jobsets.xml".to_string(), None),
        Sitemap::new("https://ultros.app/sitemap/npcs.xml".to_string(), None),
    ];

    let index = SitemapIndex::new(sitemap_list)?;
    let mut index_xml = Vec::new();
    index
        .write(&mut index_xml)
        .map_err(|_| anyhow!("Error creating sitemap"))?;
    Ok(Xml(index_xml))
}

pub(crate) async fn generic_pages_sitemap(
    State(worlds): State<Arc<WorldHelper>>,
) -> Result<Xml, WebError> {
    // (url, priority, change_frequency). Order matters for sitemap consumers
    // that don't sort: high-priority entries first. We list the home page
    // and the highest-traffic tool routes near the top so crawlers don't
    // run out of budget on long-tail category pages.
    //
    // Excluded on purpose: /alerts, /retainers/*, /list, /list/*, /history,
    // /settings, /profile, /welcome (onboarding), and other login-gated or
    // user-state pages — they emit `<meta robots noindex>` from the route
    // and don't belong in the sitemap. /privacy and /cookie-policy ARE
    // indexable (trust signal) and listed below.
    let tool_pages: &[(&str, f32, ChangeFrequency)] = &[
        ("https://ultros.app/", 1.0, ChangeFrequency::Hourly),
        ("https://ultros.app/items", 0.9, ChangeFrequency::Daily),
        (
            "https://ultros.app/flip-finder",
            0.9,
            ChangeFrequency::Hourly,
        ),
        (
            "https://ultros.app/vendor-resale",
            0.8,
            ChangeFrequency::Daily,
        ),
        (
            "https://ultros.app/vendor-sell",
            0.8,
            ChangeFrequency::Daily,
        ),
        (
            "https://ultros.app/recipe-analyzer",
            0.8,
            ChangeFrequency::Daily,
        ),
        (
            "https://ultros.app/leve-analyzer",
            0.7,
            ChangeFrequency::Weekly,
        ),
        (
            "https://ultros.app/venture-analyzer",
            0.7,
            ChangeFrequency::Weekly,
        ),
        (
            "https://ultros.app/fc-crafting-analyzer",
            0.7,
            ChangeFrequency::Daily,
        ),
        (
            "https://ultros.app/scrip-sources",
            0.7,
            ChangeFrequency::Weekly,
        ),
        (
            "https://ultros.app/currency-exchange",
            0.7,
            ChangeFrequency::Daily,
        ),
        ("https://ultros.app/trends", 0.8, ChangeFrequency::Hourly),
        ("https://ultros.app/bot", 0.6, ChangeFrequency::Monthly),
        ("https://ultros.app/api-docs", 0.6, ChangeFrequency::Weekly),
        ("https://ultros.app/about", 0.5, ChangeFrequency::Monthly),
        ("https://ultros.app/help", 0.6, ChangeFrequency::Monthly),
        ("https://ultros.app/changelog", 0.5, ChangeFrequency::Weekly),
        ("https://ultros.app/privacy", 0.3, ChangeFrequency::Yearly),
        (
            "https://ultros.app/cookie-policy",
            0.3,
            ChangeFrequency::Yearly,
        ),
    ];

    let mut urls: Vec<Url> = tool_pages
        .iter()
        .map(|(href, priority, freq)| {
            let mut builder = Url::builder((*href).to_string());
            builder.priority(*priority);
            builder.change_frequency(*freq);
            builder.build().unwrap()
        })
        .collect();

    // Keep bare tool entries for visitors without a world. World-qualified
    // entries use the same canonical path contract as the pickers and sidebar.
    for world in worlds
        .get_inner_data()
        .regions
        .iter()
        .flat_map(|region| &region.datacenters)
        .flat_map(|dc| &dc.worlds)
    {
        for tool in [
            "recipe-analyzer",
            "leve-analyzer",
            "venture-analyzer",
            "scrip-sources",
        ] {
            let mut destination = url::Url::parse("https://ultros.app").unwrap();
            destination.set_path(&format!("/{tool}/{}", world.name));
            let mut builder = Url::builder(destination.to_string());
            builder.priority(0.6);
            builder.change_frequency(ChangeFrequency::Daily);
            urls.push(
                builder
                    .build()
                    .map_err(|e| anyhow!("Error generating sitemap: {e}"))?,
            );
        }
    }

    // Help articles — surface them so deep-linkable, evergreen content can
    // rank for task-specific queries ("ffxiv flip finder", "ultros lists").
    // Kept in sync with ultros-app/src/routes/help.rs HELP_TOPICS; adding a
    // slug there should add it here too.
    const HELP_SLUGS: &[&str] = &[
        "getting-started",
        "flip-finder",
        "vendor-resale",
        "vendor-sell",
        "recipe-analyzer",
        "leve-analyzer",
        "fc-crafting",
        "scrip-sources",
        "venture-analyzer",
        "market-trends",
        "lists-alerts-retainers",
        "ai-agents",
    ];
    for slug in HELP_SLUGS {
        let mut builder = Url::builder(format!("https://ultros.app/help/{slug}"));
        builder.priority(0.5);
        builder.change_frequency(ChangeFrequency::Monthly);
        if let Ok(url) = builder.build() {
            urls.push(url);
        }
    }

    let data = xiv_gen_db::data();
    // Class/jobset pages — medium priority, weekly change frequency
    // because the items in them only shift when expansions/patches add gear.
    for class in data.class_jobs.values() {
        let mut builder =
            Url::builder(["https://ultros.app/items/jobset/", &class.abbreviation].concat());
        builder.priority(0.6);
        builder.change_frequency(ChangeFrequency::Weekly);
        if let Ok(url) = builder.build() {
            urls.push(url);
        }
    }
    // Item category pages — same rationale.
    for cat in data
        .item_search_categorys
        .values()
        .filter(|cat| (1..=4).contains(&cat.category))
    {
        // Keyed by id, not `cat.name`: the name is localized, and the SSR that
        // answers these URLs always renders with English game data, so a
        // name-keyed link only resolves for English visitors. See
        // `resolve_category_param` in `item_explorer.rs`.
        let mut builder = Url::builder(format!(
            "https://ultros.app/items/category/{}",
            cat.key_id.0
        ));
        builder.priority(0.6);
        builder.change_frequency(ChangeFrequency::Weekly);
        if let Ok(url) = builder.build() {
            urls.push(url);
        }
    }

    let url_set = UrlSet::new(urls)?;
    let mut url_xml = Vec::new();
    url_set
        .write(&mut url_xml)
        .map_err(|_| anyhow!("Error creating sitemap"))?;
    Ok(Xml(url_xml))
}

/// Collect compact per-item `(first_sale_unix, median_sale_unix)` summaries
/// from the in-process [`AnalyzerService`] for `/sitemap/items.xml`.
pub(crate) async fn collect_sitemap_item_sales(
    world_cache: &WorldHelper,
    analyzer_service: &AnalyzerService,
) -> Result<Vec<SitemapItemSaleSummary>, WebError> {
    let mut item_id_map: HashMap<i32, Vec<i64>> = HashMap::new();
    let sales = try_join_all(
        world_cache
            .get_inner_data()
            .regions
            .iter()
            .flat_map(move |region| {
                region.datacenters.iter().flat_map(move |datacenter| {
                    datacenter.worlds.iter().map(move |world| async move {
                        analyzer_service
                            .read_sale_history(&AnySelector::World(world.id), |w| w.clone())
                            .await
                    })
                })
            }),
    )
    .await?;
    for (item, sales) in sales.into_iter().flat_map(|sale| sale.item_map.into_iter()) {
        let entry = item_id_map.entry(item.item_id).or_default();
        for sale in sales {
            entry.push(sale.sale_date.and_utc().timestamp());
        }
    }
    Ok(item_id_map
        .into_iter()
        .map(|(item_id, mut timestamps)| {
            timestamps.sort_unstable();
            let first_sale_unix = timestamps.first().copied();
            let median_sale_unix = if timestamps.len() < 2 {
                None
            } else {
                timestamps.get(timestamps.len() / 2).copied()
            };
            SitemapItemSaleSummary {
                item_id,
                first_sale_unix,
                median_sale_unix,
            }
        })
        .collect())
}

pub(crate) async fn item_sitemap(
    State(market): State<Arc<dyn MarketClient>>,
) -> Result<Xml, WebError> {
    let summaries = market.sitemap_item_sales().await?;
    let now_unix = Utc::now().timestamp();
    let frequency_map: HashMap<i32, (Option<i64>, ChangeFrequency)> = summaries
        .into_iter()
        .map(|summary| {
            let change = match summary.median_sale_unix {
                Some(median_unix) => match ((now_unix - median_unix) / 86_400).abs() {
                    0 => ChangeFrequency::Always,
                    1 => ChangeFrequency::Daily,
                    2..=6 => ChangeFrequency::Weekly,
                    7..=30 => ChangeFrequency::Monthly,
                    31..=360 => ChangeFrequency::Yearly,
                    _ => ChangeFrequency::Never,
                },
                None => ChangeFrequency::Never,
            };
            (summary.item_id, (summary.first_sale_unix, change))
        })
        .collect();
    let items = UrlSet::new(
        xiv_gen_db::data()
            .items
            .iter()
            .filter(|(_, item)| item.item_search_category > 0)
            .map(|(key, _)| key.0)
            .sorted()
            .map(|id| {
                let mut builder = Url::builder(format!("https://ultros.app/item/{id}"));
                // Items with recent sales get higher priority than dead-stock
                // items — same /item sitemap entry, but signal to crawlers
                // that the page changes meaningfully more often. Dead items
                // (no sales seen) stay at low priority so we don't waste
                // crawl budget on never-traded gear.
                if let Some((last_modified, change)) = frequency_map.get(&id) {
                    if let Some(modified_unix) = last_modified {
                        // jiff rejects timestamps outside its range, so a bad
                        // row just drops <lastmod> instead of failing the
                        // sitemap.
                        if let Ok(timestamp) = Timestamp::from_second(*modified_unix) {
                            builder.last_modified(timestamp.to_zoned(TimeZone::UTC));
                        }
                    }
                    builder.change_frequency(*change);
                    let priority = match change {
                        ChangeFrequency::Always | ChangeFrequency::Hourly => 0.7,
                        ChangeFrequency::Daily => 0.6,
                        ChangeFrequency::Weekly => 0.5,
                        ChangeFrequency::Monthly => 0.4,
                        _ => 0.3,
                    };
                    builder.priority(priority);
                } else {
                    builder.priority(0.3);
                }
                builder.images(vec![Image::new(format!(
                    "https://ultros.app/static/itemicon/{id}?size=Large"
                ))]);
                builder.build()
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| anyhow!("Error generating sitemap: {e}"))?,
    )?;
    let mut url_xml = Vec::new();
    items
        .write(&mut url_xml)
        .map_err(|_| anyhow!("Error creating site map"))?;
    Ok(Xml(url_xml))
}

/// `/npc/<id>` for every NPC that offers a gil shop, an exchange or a
/// collectables counter, ascending. Their pages are static game data, so they
/// sit at one low, weekly priority.
pub(crate) fn npc_page_ids(data: &xiv_gen::Data) -> Vec<i32> {
    let mut ids: Vec<i32> = data
        .gil_shop_npcs
        .values()
        .chain(data.special_shop_npcs.values())
        .chain(data.collectables_shop_npcs.values())
        .flatten()
        .map(|npc| npc.0)
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

pub(crate) async fn npc_sitemap() -> Result<Xml, WebError> {
    let urls = UrlSet::new(
        npc_page_ids(xiv_gen_db::data())
            .into_iter()
            .map(|id| {
                Url::builder(format!("https://ultros.app/npc/{id}"))
                    .change_frequency(ChangeFrequency::Weekly)
                    .priority(0.4)
                    .build()
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| anyhow!("Error generating npc sitemap: {e}"))?,
    )?;
    let mut url_xml = Vec::new();
    urls.write(&mut url_xml)
        .map_err(|_| anyhow!("Error creating npc site map"))?;
    Ok(Xml(url_xml))
}

/// `/recipe/<id>` for every craftable recipe whose output item can be listed
/// on the market board, ascending.
pub(crate) fn recipe_page_ids(data: &xiv_gen::Data) -> Vec<i32> {
    let mut ids: Vec<i32> = data
        .recipes
        .values()
        .filter(|recipe| {
            data.items
                .get(&xiv_gen::ItemId(recipe.item_result))
                .is_some_and(|item| item.item_search_category > 0)
        })
        .map(|recipe| recipe.key_id.0)
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

pub(crate) async fn recipe_sitemap() -> Result<Xml, WebError> {
    let urls = UrlSet::new(
        recipe_page_ids(xiv_gen_db::data())
            .into_iter()
            .map(|id| {
                Url::builder(format!("https://ultros.app/recipe/{id}"))
                    .change_frequency(ChangeFrequency::Weekly)
                    .priority(0.5)
                    .build()
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| anyhow!("Error generating recipe sitemap: {e}"))?,
    )?;
    let mut url_xml = Vec::new();
    urls.write(&mut url_xml)
        .map_err(|_| anyhow!("Error creating recipe site map"))?;
    Ok(Xml(url_xml))
}

pub(crate) async fn jobset_sitemap() -> Result<Xml, WebError> {
    let urls = UrlSet::new(
        ultros_app::job_set_sitemap_paths(xiv_gen_db::data())
            .into_iter()
            .map(|path| {
                Url::builder(format!("https://ultros.app{path}"))
                    .change_frequency(ChangeFrequency::Weekly)
                    .priority(0.5)
                    .build()
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| anyhow!("Error generating jobset sitemap: {e}"))?,
    )?;
    let mut url_xml = Vec::new();
    urls.write(&mut url_xml)
        .map_err(|_| anyhow!("Error creating jobset site map"))?;
    Ok(Xml(url_xml))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipe_and_jobset_sitemaps_return_sorted_unique_entries() {
        let data = xiv_gen_db::data();
        let recipes = recipe_page_ids(data);
        assert!(!recipes.is_empty());
        assert!(recipes.windows(2).all(|w| w[0] < w[1]));

        let jobsets = ultros_app::job_set_sitemap_paths(data);
        assert!(!jobsets.is_empty());
        assert!(jobsets.iter().all(|p| p.starts_with("/items/jobset/")));
    }
}
