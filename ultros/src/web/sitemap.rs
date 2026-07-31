use super::error::WebError;
use crate::analyzer_service::AnalyzerService;
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

// State(world_cache): State<Arc<WorldCache>>,)
pub(crate) async fn sitemap_index() -> Result<Xml, WebError> {
    // Get all the worlds from the world cache and then populate the listings sitemap to point to all the world subsitemaps
    // let mut sitemap_list: Vec<_> = world_cache
    //     .get_inner_data()
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
    // (url, priority, change_frequency). Order matters for sitemap consumers
    // that don't sort: high-priority entries first. We list the home page
    // and the highest-traffic tool routes near the top so crawlers don't
    // run out of budget on long-tail category pages.
    //
    // Excluded on purpose: /alerts, /retainers/*, /list, /list/*, /history,
    // /settings, /profile, /welcome (onboarding), /privacy, /cookie-policy,
    // and other login-gated or user-state pages — they emit `<meta robots
    // noindex>` from the route and don't belong in the sitemap.
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
        ("https://ultros.app/about", 0.5, ChangeFrequency::Monthly),
        ("https://ultros.app/help", 0.6, ChangeFrequency::Monthly),
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

    // Help articles — surface them so deep-linkable, evergreen content can
    // rank for task-specific queries ("ffxiv flip finder", "ultros lists").
    // Kept in sync with ultros-app/src/routes/help.rs HELP_TOPICS; adding a
    // slug there should add it here too.
    const HELP_SLUGS: &[&str] = &[
        "getting-started",
        "flip-finder",
        "vendor-resale",
        "recipe-analyzer",
        "leve-analyzer",
        "fc-crafting",
        "scrip-sources",
        "venture-analyzer",
        "market-trends",
        "lists-alerts-retainers",
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

    // Currency exchange per-currency pages.
    // This matches the logic inside CurrencySelection in currency_exchange.rs.
    let allowed_item_ui_categories = [100, 61, 63];
    let mut currencies = Vec::new();

    for special_shop in data.special_shops.values() {
        // Iterate over the rows of the shop
        let len = special_shop.item_receive_0.len();
        for i in 0..len {
            // Check if any received item in this row is marketable
            let mut has_marketable_receive = false;

            let recv_0 = special_shop.item_receive_0[i];
            let count_0 = special_shop.count_receive_0[i];
            if recv_0 != 0 && count_0 != 0 {
                if let Some(item) = data.items.get(&xiv_gen::ItemId(recv_0 as i32)) {
                    if item.item_search_category != 0 {
                        has_marketable_receive = true;
                    }
                }
            }

            let recv_1 = special_shop.item_receive_1[i];
            let count_1 = special_shop.count_receive_1[i];
            if recv_1 != 0 && count_1 != 0 {
                if let Some(item) = data.items.get(&xiv_gen::ItemId(recv_1 as i32)) {
                    if item.item_search_category != 0 {
                        has_marketable_receive = true;
                    }
                }
            }

            if has_marketable_receive {
                // Collect the cost items for this row
                let cost_0 = special_shop.item_cost_0[i];
                let amt_0 = special_shop.count_cost_0[i];
                if cost_0 != 0 && amt_0 != 0 {
                    if let Some(item) = data.items.get(&xiv_gen::ItemId(cost_0 as i32)) {
                        if allowed_item_ui_categories.contains(&item.item_ui_category) {
                            currencies.push(item.key_id.0);
                        }
                    }
                }

                let cost_1 = special_shop.item_cost_1[i];
                let amt_1 = special_shop.count_cost_1[i];
                if cost_1 != 0 && amt_1 != 0 {
                    if let Some(item) = data.items.get(&xiv_gen::ItemId(cost_1 as i32)) {
                        if allowed_item_ui_categories.contains(&item.item_ui_category) {
                            currencies.push(item.key_id.0);
                        }
                    }
                }

                let cost_2 = special_shop.item_cost_2[i];
                let amt_2 = special_shop.count_cost_2[i];
                if cost_2 != 0 && amt_2 != 0 {
                    if let Some(item) = data.items.get(&xiv_gen::ItemId(cost_2 as i32)) {
                        if allowed_item_ui_categories.contains(&item.item_ui_category) {
                            currencies.push(item.key_id.0);
                        }
                    }
                }
            }
        }
    }

    currencies.sort();
    currencies.dedup();

    // Now filter out disallowed items like "Gil" (ID 1) and "MGP" (ID 29) and build URLs
    for id in currencies {
        if id != 1 && id != 29 {
            let mut builder = Url::builder(format!("https://ultros.app/currency-exchange/{id}"));
            builder.priority(0.6);
            builder.change_frequency(ChangeFrequency::Daily);
            if let Ok(url) = builder.build() {
                urls.push(url);
            }
        }
    }

    let url_set = UrlSet::new(urls)?;
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
            .get_inner_data()
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
                if len < 2 { None } else { value.get(len / 2) }
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
                    if let Some(modified) = last_modified {
                        builder.last_modified(modified.and_utc().fixed_offset());
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
