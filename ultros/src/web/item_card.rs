use std::{cell::RefCell, rc::Rc, sync::Arc};

use super::{error::WebError, WebState};
use anyhow::{anyhow, Result};
use axum::{
    body::{self, Full},
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use hyper::header;
use plotters_svg::SVGBackend;
use resvg::{
    tiny_skia,
    usvg::{self, fontdb, Options, TreeParsing, TreeTextToPath},
};
use ultros_api_types::{
    world_helper::{AnyResult, WorldHelper},
    SaleHistory,
};
use ultros_charts::ChartOptions;
use ultros_db::UltrosDb;
use xiv_gen::{Item, ItemId};

pub(crate) async fn generate_image<'a>(
    db: &UltrosDb,
    world_helper: &WorldHelper,
    item: &'static Item,
    world: &'a AnyResult<'_>,
) -> Result<Vec<u8>> {
    let world_ids: Vec<_> = world.all_worlds().map(|w| w.id).collect();
    let sales: Vec<SaleHistory> = db
        .get_sale_history_from_multiple_worlds(world_ids.into_iter(), item.key_id.0, 1000)
        .await?
        .into_iter()
        .map(SaleHistory::from)
        .collect();
    const SIZE: (u32, u32) = (1920 / 2, 1080 / 2);
    let buffer = {
        let mut buffer = String::new();
        // let mut image = RgbImage::new(size.0, size.1);

        let backend = SVGBackend::with_string(&mut buffer, SIZE);
        if let Err(e) = ultros_charts::draw_sale_history_scatter_plot(
            Rc::new(RefCell::new(backend)),
            &world_helper,
            &sales,
            ChartOptions {
                remove_outliers: true,
                icon_item_id: item.key_id.0,
                draw_icon: true,
            },
        ) {
            Err(anyhow!("can't draw scatter plot {e}"))?
        }

        buffer
    };

    let opt = Options {
        resources_dir: std::fs::canonicalize(&buffer)
            .ok()
            // Get file's absolute directory.
            .and_then(|p| p.parent().map(|p| p.to_path_buf())),
        ..Default::default()
    };

    let mut fontdb = fontdb::Database::new();
    fontdb.load_system_fonts();

    let mut tree = usvg::Tree::from_str(&buffer, &opt).unwrap();
    tree.convert_text(&fontdb);
    let rtree = resvg::Tree::from_usvg(&tree);
    let pixmap_size = resvg::IntSize::from_usvg(rtree.size);
    let mut pixmap = tiny_skia::Pixmap::new(pixmap_size.width(), pixmap_size.height())
        .ok_or(anyhow!("failed to make pixmap"))?;
    rtree.render(tiny_skia::Transform::default(), &mut pixmap.as_mut());
    Ok(pixmap.encode_png()?)
}

#[axum::debug_handler(state = WebState)]
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
        .body(body::boxed(Full::from(bytes)))?)
}
