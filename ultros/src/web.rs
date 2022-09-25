mod fuzzy_item_search;
mod templates;
pub mod item_search_index;

use axum::body::{Empty, Full};
use axum::extract::{FromRef, Path, Query, State};
use axum::http::{HeaderValue, Response, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::{body, Router};
use include_dir::include_dir;
use maud::{html, Markup, Render};
use reqwest::header;
use serde::Deserialize;
use std::fmt::Write;
use std::io::Read;
use std::net::SocketAddr;
use std::path::PathBuf;
use ultros_db::price_optimizer::BestResellResults;
use ultros_db::UltrosDb;
use universalis::{ItemId, WorldId};
use xiv_gen::ItemId as XivDBItemId;

use self::templates::page::{Page, RenderPage};
use crate::web::templates::components::header::Header;
use crate::web::templates::components::SearchBox;

struct HomePage;

impl Page for HomePage {
    fn get_name<'a>(&self) -> &'a str {
        "Home page"
    }

    fn draw_body(&self) -> Markup {
        html! {
            (Header)
            h1 class="hero-title" {
                "Own the marketboard"
            }
        }
    }
}

// basic handler that responds with a static string
async fn root() -> RenderPage<HomePage> {
    RenderPage(HomePage)
}

async fn search_retainers(
    State(db): State<ultros_db::UltrosDb>,
    Path(search): Path<String>,
) -> Result<Html<String>, StatusCode> {
    let retainers = db
        .search_retainers(&search)
        .await
        .map_err(|e| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut string = String::new();
    write!(
        string,
        "<table><tr><th>retainer name</th><th>retainer id</th><th>world id</th><th>world name</th></tr>"
    ).unwrap();
    for (retainer, world) in retainers {
        write!(
            &mut string,
            "<tr><td><a href=\"/listings/retainer/{}\">{}</a></td><td>{}<td><td>{}</td></tr>",
            retainer.id,
            retainer.name,
            retainer.world_id,
            world
                .map(|w| w.name)
                .unwrap_or(retainer.world_id.to_string())
        )
        .unwrap();
    }
    write!(string, "</table>").unwrap();
    Ok(Html(string))
}

async fn get_retainer_listings(
    State(db): State<ultros_db::UltrosDb>,
    Path(retainer_id): Path<i32>,
) -> Result<Html<String>, (StatusCode, String)> {
    let data = db.get_retainer_listings(retainer_id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error occured {e}"),
        )
    })?;

    let game_data = xiv_gen_db::decompress_data();
    let items = &game_data.items;
    if let Some((retainer, listings)) = data {
        let mut data = format!("<h1>{}</h1>", retainer.name);
        // get all listings from the retainer and calculate heuristics
        let multiple_listings = db
            .get_multiple_listings_for_worlds(
                [WorldId(retainer.world_id)].into_iter(),
                listings.iter().map(|i| ItemId(i.item_id)),
            )
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;
        let world = if let Ok(Some(world)) = db.get_world_from_retainer(&retainer).await {
            world
        } else {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to get world data for retainer".to_string(),
            ));
        };
        write!(data, "<h1>{}</h1>", world.name).unwrap();
        let world_name = world.name;
        write!(
            data,
            "<table><th>ranking</th><th>item id</th><th>price per unit</th> <th>quantity</th><th>total</th>"
        ).unwrap();
        for listing in listings {
            let item = items
                .get(&XivDBItemId(listing.item_id))
                .map(|m| m.name.as_str())
                .unwrap_or_default();
            // get the the ranking of this listing for the world
            let market_position = multiple_listings
                .iter()
                .filter(|m| m.item_id == listing.item_id)
                .enumerate()
                .find(|(_, m)| m.id == listing.id)
                .map(|(pos, _)| pos + 1)
                .unwrap_or_default();
            let item_id = listing.item_id;
            write!(
                data,
                r#"<tr><td><a href="/listings/{world_name}/{item_id}">{}</a></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>"#,
                item,
                market_position,
                listing.price_per_unit,
                listing.quantity,
                listing.price_per_unit * listing.quantity,
                listing.timestamp
            ).unwrap();
        }
        write!(data, "</table>").unwrap();
        Ok(Html(data))
    } else {
        Ok(Html(format!("Unable to find retainer")))
    }
}

async fn world_item_listings(
    State(db): State<UltrosDb>,
    Path((world, item_id)): Path<(String, i32)>,
) -> Result<Html<String>, (StatusCode, String)> {
    let world = db.get_world(&world).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to get world {e}"),
        )
    })?;
    let listings = db
        .get_listings_for_world(WorldId(world.id), ItemId(item_id))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to get listings".to_string(),
            )
        })?;
    let mut value = String::new();
    write!(value, "<table><tr><th>id</th><th>price per unit</th><th>quantity</th><th>total</th><th>timestamp</th></tr>").unwrap();
    for listing in listings {
        write!(
            &mut value,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
            listing.id,
            listing.price_per_unit,
            listing.quantity,
            listing.price_per_unit * listing.quantity,
            listing.timestamp
        )
        .unwrap();
    }
    write!(value, "</table>").unwrap();
    Ok(Html(value))
}

#[derive(Deserialize)]
struct ProfitParameters {
    sale_amount_threshold: i32,
    sale_window_days: i64,
}

async fn analyze_profits(
    State(db): State<UltrosDb>,
    Path(world): Path<String>,
    Query(parameters): Query<ProfitParameters>,
) -> Result<Html<String>, (StatusCode, String)> {
    let ProfitParameters {
        sale_amount_threshold,
        sale_window_days,
    } = &parameters;
    let world = db.get_world(&world).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("World not found {e:?}"),
        )
    })?;
    let best_items = db
        .get_best_item_to_resell_on_world(
            world.id,
            *sale_amount_threshold,
            chrono::Duration::days(*sale_window_days),
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;
    let game_data = xiv_gen_db::decompress_data();
    let mut html = format!(
        "<table><tr><th>{}</th><th>{}</th><th>{}</th></tr>",
        "Item name", "Margin %", "Profit Amount"
    );
    for item in best_items {
        let BestResellResults {
            item_id,
            margin,
            profit,
        } = &item;
        let item_name = game_data
            .items
            .get(&xiv_gen::ItemId(item.item_id))
            .map(|item| item.name.as_str())
            .unwrap_or_default();
        write!(
            &mut html,
            "<tr><td>{item_name}</td><td>{margin}</td><td>{profit}</td></tr>"
        )
        .unwrap();
    }
    write!(&mut html, "</table>").unwrap();
    Ok(Html(html))
}

#[derive(Clone, Debug)]
pub(crate) struct WebState {
    pub(crate) db: UltrosDb,
}

impl FromRef<WebState> for UltrosDb {
    fn from_ref(input: &WebState) -> Self {
        input.db.clone()
    }
}

/// In release mode, return the files from a statically included dir
#[cfg(not(debug_assertions))]
fn get_static_file(path: &str) -> Option<&'static [u8]> {
    static STATIC_DIR: include_dir::Dir = include_dir!("$CARGO_MANIFEST_DIR/static");
    let dir = &STATIC_DIR;
    None
}

/// In debug mode, just load the files from disk
#[cfg(debug_assertions)]
fn get_static_file(path: &str) -> Option<Vec<u8>> {
    let file = PathBuf::from("./ultros/static").join(path);
    let mut file = std::fs::File::open(file).ok()?;
    let mut vec = Vec::new();
    file.read_to_end(&mut vec).ok()?;
    Some(vec)
}

async fn static_path(Path(path): Path<String>) -> impl IntoResponse {
    let path = path.trim_start_matches('/');
    let mime_type = mime_guess::from_path(path).first_or_text_plain();
    match get_static_file(&path) {
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(body::boxed(Empty::new()))
            .unwrap(),
        Some(file) => Response::builder()
            .status(StatusCode::OK)
            .header(
                header::CONTENT_TYPE,
                HeaderValue::from_str(mime_type.as_ref()).unwrap(),
            )
            .body(body::boxed(Full::from(file)))
            .unwrap(),
    }
}

pub(crate) async fn start_web(state: WebState) {
    // build our application with a route
    let app = Router::with_state(state)
        .route("/", get(root))
        .route("/retainer/search/:search", get(search_retainers))
        .route("/listings/:world/:itemid", get(world_item_listings))
        .route("/listings/retainer/:id", get(get_retainer_listings))
        .route("/listings/analyze/:world", get(analyze_profits))
        .route("/items/:search", get(fuzzy_item_search::search_items))
        .route("/static/*path", get(static_path))
        .fallback(fallback);

    // run our app with hyper
    // `axum::Server` is a re-export of `hyper::Server`
    let port = std::env::var("PORT")
        .map(|p| p.parse::<u16>().ok())
        .ok()
        .flatten()
        .unwrap_or(8080);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn fallback() -> (StatusCode, &'static str) {
    (StatusCode::NOT_FOUND, "Not found")
}
