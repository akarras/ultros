use std::sync::{Arc, OnceLock};

use super::{PriceSeriesArgs, WebState, build_price_series, error::WebError};
use anyhow::{Result, anyhow};
use axum::{
    body::Body,
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use hyper::header;
use resvg::{
    tiny_skia,
    usvg::{self, Options},
};
use ultros_api_types::{
    price_series::{HqFilter, SeriesGroup},
    world_helper::WorldHelper,
};
use ultros_charts::charts::price_history::{PriceChartOptions, build_price_history_scene};
use ultros_charts::svg::scene_to_svg;
use ultros_clickhouse::ClickHouseClient;
use ultros_db::world_data::world_cache::WorldCache;
use xiv_gen::{Item, ItemId};

/// Window shown on the item card: the last 30 days, ending now. The card is
/// a single static snapshot (Discord embed / PNG download) with no
/// timeline-slicer UI, so there's no "requested range" to preserve — this
/// picks a fixed, reasonable default.
const CARD_WINDOW_DAYS: i64 = 30;

pub(crate) async fn generate_image(
    ch: &ClickHouseClient,
    world_cache: &WorldCache,
    world_helper: &WorldHelper,
    item: &'static Item,
    world: &str,
) -> Result<Vec<u8>> {
    let to = chrono::Utc::now();
    let from = to - chrono::Duration::days(CARD_WINDOW_DAYS);
    let series = build_price_series(
        ch,
        world_cache,
        PriceSeriesArgs {
            world,
            item_id: item.key_id.0,
            from,
            to,
            group: SeriesGroup::World,
            hq: HqFilter::Any,
            bucket: None,
        },
    )
    .await?;
    let scene = build_price_history_scene(
        world_helper,
        &series,
        &PriceChartOptions {
            title: Some(format!("{} - Sale History", item.name)),
            icon_data_uri: ultros_charts::item_icon_data_uri(item.key_id.0),
            ..Default::default()
        },
    );
    svg_to_png(&scene_to_svg(&scene))
}

fn font_db() -> Arc<usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = usvg::fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone()
}

fn svg_to_png(svg: &str) -> Result<Vec<u8>> {
    let opt = Options {
        fontdb: font_db(),
        ..Default::default()
    };
    let tree = usvg::Tree::from_str(svg, &opt)?;
    let pixmap_size = tree.size().to_int_size();
    let mut pixmap = tiny_skia::Pixmap::new(pixmap_size.width(), pixmap_size.height())
        .ok_or(anyhow!("failed to make pixmap"))?;
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
    Ok(pixmap.encode_png()?)
}

#[axum_macros::debug_handler(state = WebState)]
pub(crate) async fn item_card(
    Path((world, item_id)): Path<(String, i32)>,
    State(ch): State<ClickHouseClient>,
    State(world_cache): State<Arc<WorldCache>>,
    State(world_helper): State<Arc<WorldHelper>>,
) -> Result<impl IntoResponse, WebError> {
    let item = xiv_gen_db::data()
        .items
        .get(&ItemId(item_id))
        .ok_or(WebError::InvalidItemId(item_id))?;
    // Validated up front against `WorldHelper` (cheap, in-memory) so an
    // unknown world name 400s immediately rather than after a ClickHouse
    // round trip — `build_price_series` would otherwise still catch it via
    // `WorldCache::lookup_value_by_name`, just as a 404 instead.
    if world_helper.lookup_world_by_name(&world).is_none() {
        return Err(WebError::WorldNotFound(world));
    }
    let bytes = generate_image(&ch, &world_cache, &world_helper, item, &world).await?;
    let mime_type = mime_guess::from_path("icon.png").first_or_text_plain();
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, mime_type.as_ref())
        .body(Body::new(http_body_util::Full::from(bytes)))?)
}
