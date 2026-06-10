use std::sync::{Arc, OnceLock};

use super::{WebState, error::WebError};
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
    SaleHistory,
    world_helper::{AnyResult, WorldHelper},
};
use ultros_charts::charts::price_history::{PriceChartOptions, build_price_history_scene};
use ultros_charts::svg::scene_to_svg;
use ultros_db::UltrosDb;
use xiv_gen::{Item, ItemId};

pub(crate) async fn generate_image(
    db: &UltrosDb,
    world_helper: &WorldHelper,
    item: &'static Item,
    world: &AnyResult<'_>,
) -> Result<Vec<u8>> {
    let world_ids: Vec<_> = world.all_worlds().map(|w| w.id).collect();
    let sales: Vec<SaleHistory> = db
        .get_sale_history_from_multiple_worlds(world_ids.into_iter(), item.key_id.0, 200)
        .await?
        .into_iter()
        .map(SaleHistory::from)
        .collect();
    let scene = build_price_history_scene(
        world_helper,
        &sales,
        &PriceChartOptions {
            remove_outliers: true,
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
    State(db): State<UltrosDb>,
    State(world_helper): State<Arc<WorldHelper>>,
) -> Result<impl IntoResponse, WebError> {
    let item = xiv_gen_db::data()
        .items
        .get(&ItemId(item_id))
        .ok_or(WebError::InvalidItemId(item_id))?;
    let world = world_helper
        .lookup_world_by_name(&world)
        .ok_or_else(|| WebError::WorldNotFound(world))?;
    let bytes = generate_image(&db, &world_helper, item, &world).await?;
    let mime_type = mime_guess::from_path("icon.png").first_or_text_plain();
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, mime_type.as_ref())
        .body(Body::new(http_body_util::Full::from(bytes)))?)
}
