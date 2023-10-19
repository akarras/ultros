use super::error::WebError;
use crate::analyzer_service::AnalyzerService;
use anyhow::anyhow;
use axum::{
    extract::{Path, State},
    http::HeaderValue,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use futures::future::try_join_all;
use itertools::Itertools;
use mime_guess::mime;
use reqwest::header;
use sitemap_rs::{
    image::Image,
    sitemap::Sitemap,
    sitemap_index::SitemapIndex,
    url::{ChangeFrequency, Url},
    url_set::UrlSet,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use ultros_api_types::world_helper::WorldHelper;
use ultros_db::world_cache::{AnySelector, WorldCache};

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

pub(crate) async fn sitemap_index(// State(world_cache): State<Arc<WorldCache>>,
) -> Result<Xml, WebError> {
    // Get all the worlds from the world cache and then populate the listings sitemap to point to all the world subsitemaps
    // let mut sitemap_list: Vec<_> = world_cache
    //     .get_all()
    //     .iter()
    //     .flat_map(|(r, dcs)| {
    //         [AnyResult::Region(r)]
    //             .into_iter()
    //             .chain(dcs.iter().flat_map(|(dc, worlds)| {
    //                 [AnyResult::Datacenter(dc)]
    //                     .into_iter()
    //                     .chain(worlds.iter().map(|w| AnyResult::World(w)))
    //             }))
    //     })
    //     .map(|name| {
    //         Sitemap::new(
    //             format!("https://ultros.app/sitemap/world/{}.xml", name.get_name()),
    //             None,
    //         )
    //     })
    //     .collect();
    // add general page sitemap
    let sitemap_list = vec![
        Sitemap::new("https://ultros.app/sitemap/pages.xml".to_string(), None),
        Sitemap::new("https://ultros.app/sitemap/items.xml".to_string(), None),
    ];

    let index = SitemapIndex::new(sitemap_list)?;
    let mut index_xml = Vec::new();
    index
        .write(&mut index_xml)
        .map_err(|_| anyhow!("Error creating sitemap"))?;
    Ok(Xml(index_xml))
}

pub(crate) async fn generic_pages_sitemap() -> Result<Xml, WebError> {
    let sitemap_urls = ["https://ultros.app", "https://ultros.app/items"]
        .iter()
        .map(|i| i.to_string());
    let mut url_xml = Vec::new();
    let data = xiv_gen_db::data();
    let class_jobs = data
        .class_jobs
        .values()
        .map(|class| ["https://ultros.app/items/jobset/", &class.abbreviation].concat());
    let item_categories = data
        .item_search_categorys
        .values()
        .filter(|cat| (1..=4).contains(&cat.category))
        .map(|cat| ["https://ultros.app/items/category/", &cat.name].concat());
    let url_set = UrlSet::new(
        sitemap_urls
            .chain(class_jobs)
            .chain(item_categories)
            .map(|url| Url::builder(url).build().unwrap())
            .collect(),
    )?;
    url_set
        .write(&mut url_xml)
        .map_err(|_| anyhow!("Error creating sitemap"))?;
    Ok(Xml(url_xml))
}

pub(crate) async fn world_sitemap(
    State(db): State<AnalyzerService>,
    State(world_cache): State<Arc<WorldCache>>,
    Path(world_name): Path<String>,
) -> Result<Xml, WebError> {
    // validate that this is a valid world name, then repeat back a sitemap using all the item ids

    // handle .xml being in the path potentially
    let world_name = match world_name.split_once('.') {
        Some((left, _)) => left,
        None => &world_name,
    };

    let result = world_cache.lookup_value_by_name(world_name)?;
    // Create a unique list of item ids
    let items: HashSet<_> = db
        .read_cheapest_items(&AnySelector::from(&result), |items| {
            items.item_map.keys().map(|k| k.item_id).collect()
        })
        .await?;
    // format those item ids into urls based on the world name and generate a url set
    let url_set = UrlSet::new(
        items
            .iter()
            .map(|i| {
                Url::builder(format!("https://ultros.app/item/{world_name}/{i}"))
                    .build()
                    .unwrap()
            })
            .collect(),
    )?;
    let mut url_xml = Vec::new();
    url_set
        .write(&mut url_xml)
        .map_err(|_| anyhow!("Error creating sitemap"))?;
    Ok(Xml(url_xml))
}

pub(crate) async fn item_sitemap(
    State(world_cache): State<Arc<WorldHelper>>,
    State(analyzer_service): State<AnalyzerService>,
) -> Result<Xml, WebError> {
    let mut item_id_map: HashMap<_, Vec<_>> = HashMap::new();
    let a = &analyzer_service;
    let sales = try_join_all(
        world_cache
            .get_all()
            .regions
            .iter()
            .flat_map(move |region| {
                region.datacenters.iter().flat_map(move |datacenter| {
                    datacenter.worlds.iter().map(move |world| async move {
                        a.read_sale_history(&AnySelector::World(world.id), |w| w.clone())
                            .await
                    })
                })
            }),
    )
    .await?;
    for (item, sales) in sales.into_iter().flat_map(|sale| sale.item_map.into_iter()) {
        let entry = item_id_map.entry(item.item_id).or_default();
        for sale in sales {
            entry.push(sale);
        }
    }
    let frequency_map: HashMap<_, _> = item_id_map
        .into_iter()
        .map(|(key, mut value)| {
            value.sort_by_key(|f| f.sale_date);
            let first = value.first().map(|f| f.sale_date);
            let median = {
                let len = value.len();
                if len < 2 {
                    None
                } else {
                    value.get(len / 2)
                }
            };
            if let Some(median) = median {
                (
                    key,
                    (
                        first,
                        match (Utc::now()
                            .naive_utc()
                            .signed_duration_since(median.sale_date))
                        .num_days()
                        .abs()
                        {
                            0 => ChangeFrequency::Always,
                            1 => ChangeFrequency::Daily,
                            2..=6 => ChangeFrequency::Weekly,
                            7..=30 => ChangeFrequency::Monthly,
                            31..=360 => ChangeFrequency::Yearly,
                            _ => ChangeFrequency::Never,
                        },
                    ),
                )
            } else {
                (key, (first, ChangeFrequency::Never))
            }
        })
        .collect();
    let items = UrlSet::new(
        xiv_gen_db::data()
            .items
            .iter()
            .filter(|(_, item)| item.item_search_category.0 > 0)
            .map(|(key, _)| key.0)
            .sorted()
            .map(|id| {
                let mut builder = Url::builder(format!("https://ultros.app/item/{id}"));
                if let Some((last_modified, change)) = frequency_map.get(&id) {
                    if let Some(modified) = last_modified {
                        builder.last_modified(modified.and_utc().fixed_offset());
                    }
                    builder.change_frequency(*change);
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
