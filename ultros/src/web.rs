use axum::error_handling::HandleErrorLayer;
use axum::extract::{Extension, FromRef, Path, State};
use axum::handler::HandlerWithoutStateExt;
use axum::http::{Method, StatusCode, Uri};
use axum::response::Html;
use axum::routing::get;
use axum::{BoxError, Json, Router};
use std::fmt::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use ultros_db::UltrosDb;
use universalis::{DataCentersView, ItemId, WorldId, WorldsView};

// basic handler that responds with a static string
async fn root() -> Html<&'static str> {
    Html("Hello, World!")
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
    );
    for (retainer, world) in retainers {
        write!(
            &mut string,
            "<tr><td>{}</td><td>{}</td><td>{}<td><td>{}</td></tr>",
            retainer.name,
            retainer.id,
            retainer.world_id,
            world
                .map(|w| w.name)
                .unwrap_or(retainer.world_id.to_string())
        );
    }
    write!(string, "</table>");
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
    if let Some((retainer, listings)) = data {
        let mut data = format!("<h1>{}</h1>", retainer.name);
        if let Ok(Some(world)) = db.get_world_from_retainer(&retainer).await {
            write!(data, "<h1>{}</h1>", world.name);
        }
        write!(
            data,
            "<table><th>item id</th><th>price per unit</th> <th>quantity</th><th>total</th>"
        );
        for listing in listings {
            write!(
                data,
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                listing.item_id,
                listing.price_per_unit,
                listing.quantity,
                listing.price_per_unit * listing.quantity,
                listing.timestamp
            );
        }
        write!(data, "</table>");
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
    write!(value, "<table><tr><th>id</th><th>price per unit</th><th>quantity</th><th>total</th><th>timestamp</th></tr>");
    for listing in listings {
        write!(
            &mut value,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
            listing.id,
            listing.price_per_unit,
            listing.quantity,
            listing.price_per_unit * listing.quantity,
            listing.timestamp
        );
    }
    write!(value, "</table>");
    Ok(Html(value))
}

#[derive(Clone, Debug)]
pub(crate) struct WebState {
    pub(crate) db: UltrosDb,
    pub(crate) datacenters: Arc<DataCentersView>,
    pub(crate) worlds: Arc<WorldsView>,
}

impl FromRef<WebState> for UltrosDb {
    fn from_ref(input: &WebState) -> Self {
        input.db.clone()
    }
}

pub(crate) async fn start_web(state: WebState) {
    // build our application with a route
    let app = Router::with_state(state)
        .route("/", get(root))
        .route("/retainer/search/:search", get(search_retainers))
        .route("/listings/:world/:itemid", get(world_item_listings))
        .route("/retainer/:id/listings", get(get_retainer_listings))
        .fallback(fallback);

    // run our app with hyper
    // `axum::Server` is a re-export of `hyper::Server`
    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    tracing::info!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn fallback() -> (StatusCode, &'static str) {
    (StatusCode::NOT_FOUND, "Not found")
}
