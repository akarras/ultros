use std::{error::Error, sync::Arc};

use axum::http::{HeaderValue, header};
/// Ultros UI server contains all the axum routes required to serve and bundle leptos wasm files
/// # Building
/// I recommend you use cargo-leptos, once you go through the steps to install cargo-leptos
/// you should be able to build and serve leptos with one install step.
///
use axum::{
    Router,
    body::Body,
    extract::State,
    http::Request,
    response::{IntoResponse, Response},
};
use leptos::prelude::*;
use leptos_axum::{LeptosRoutes, generate_route_list};
#[cfg(not(debug_assertions))]
use tower_http::set_header::SetResponseHeader;
use tracing::{info, instrument};
use ultros_api_types::user::UserData;
use ultros_api_types::world_helper::WorldHelper;
use ultros_app::*;

use crate::web::error::ApiError;
use crate::web::oauth::AuthDiscordUser;
use crate::web::{WebState, country_code_decoder::Region};

/// Escape a JSON string for safe embedding inside a `<script>` element.
///
/// JSON allows literal `<` characters inside strings; if any of them happen to
/// be followed by `/script>`, the parser would close the tag and start
/// executing arbitrary content as HTML. Replacing the handful of characters
/// below with their `\uXXXX` escapes keeps the payload as valid JSON and
/// inert to the HTML parser. U+2028 / U+2029 also need escaping because they
/// are JS line terminators (legal in JSON strings but break script parsing).
fn escape_for_script_tag(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    for c in json.chars() {
        match c {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            other => out.push(other),
        }
    }
    out
}

#[instrument(skip(worlds, options, req, user))]
#[axum::debug_handler(state = WebState)]
async fn custom_handler(
    State(worlds): State<Arc<WorldHelper>>,
    State(options): State<LeptosOptions>,
    region: Option<Region>,
    user: Result<AuthDiscordUser, ApiError>,
    req: Request<Body>,
) -> Response {
    info!("Custom handler");
    // The HTML now carries per-user data (region + current_user), so it must
    // never be cached by a shared proxy.
    let region_str = region.unwrap_or(Region::NorthAmerica).to_string();
    let current_user = user.ok().map(|u| UserData {
        id: u.id,
        username: u.name,
        avatar: u.avatar_url,
    });

    // Build the bootstrap script body once per request. We serialize a borrowed
    // view of WorldData to avoid cloning the (small but non-trivial) world tree.
    #[derive(serde::Serialize)]
    struct BootstrapRef<'a> {
        world_data: &'a ultros_api_types::world::WorldData,
        region: &'a str,
        current_user: &'a Option<UserData>,
    }
    let bootstrap_json = serde_json::to_string(&BootstrapRef {
        world_data: worlds.world_data(),
        region: &region_str,
        current_user: &current_user,
    })
    .unwrap_or_else(|_| "null".to_string());
    let bootstrap_script = format!(
        "window.__ULTROS_BOOTSTRAP__={};",
        escape_for_script_tag(&bootstrap_json)
    );

    let region_for_ctx = region_str.clone();
    let current_user_for_ctx = current_user.clone();
    let handler = leptos_axum::render_app_to_stream_with_context_and_replace_blocks(
        move || {
            provide_context(LocalWorldData(Ok(worlds.clone())));
            provide_context(GuessedRegion(region_for_ctx.clone()));
            provide_context(BootstrapUser(current_user_for_ctx.clone()));
        },
        move || shell(options.clone(), bootstrap_script.clone()),
        true,
    );
    let mut response = handler(req).await.into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response
}

pub(crate) async fn create_leptos_app(
    worlds: Arc<WorldHelper>,
) -> Result<Router<WebState>, Box<dyn Error>> {
    use tower_http::services::ServeDir;

    let conf = get_configuration(None)?;
    let mut leptos_options = conf.leptos_options;
    let site_root = &leptos_options.site_root;
    let pkg_dir = &leptos_options.site_pkg_dir;

    // The URL path of the generated JS/WASM bundle from cargo-leptos
    // let bundle_path = format!("/{site_root}/{pkg_dir}");
    // The filesystem path of the generated JS/WASM bundle from cargo-leptos
    let bundle_filepath = format!("./{site_root}/{pkg_dir}");
    let addr = leptos_options.site_addr;
    tracing::debug!("serving at {addr}");

    // simple_logger::init_with_level(log::Level::Debug).expect("couldn't initialize logging");

    // These are Tower Services that will serve files from the static and pkg repos.
    // HandleError is needed as Axum requires services to implement Infallible Errors
    // because all Errors are converted into Responses
    // let static_service = HandleError::new(ServeDir::new("./static"), handle_file_error);
    //let pkg_service = HandleError::new(ServeDir::new("./pkg"), handle_file_error);
    let git_hash = env!("GIT_HASH");
    leptos_options.site_pkg_dir = Arc::from(["pkg/", git_hash].concat());
    // let cargo_leptos_service = HandleError::new(ServeDir::new(&bundle_filepath), handle_file_error);
    let cargo_leptos_service = ServeDir::new(&bundle_filepath);
    #[cfg(not(debug_assertions))]
    let cargo_leptos_service = SetResponseHeader::appending(
        cargo_leptos_service,
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=86400, immutable"),
    );
    tracing::info!("Serving pkg dir: {bundle_filepath}");
    let worlds = Ok(worlds);
    let routes = generate_route_list(move || {
        let worlds = worlds.clone();
        provide_context(LocalWorldData(worlds));
        provide_context(GuessedRegion("North-America".to_string()));
        provide_context(BootstrapUser(None));
        view! { <App /> }
    });

    // simple_logger::init_with_level(log::Level::Debug).expect("couldn't initialize logging");

    // build our application with a route
    Ok(Router::new()
        // `GET /` goes to `root`
        .nest_service(
            &["/", &leptos_options.site_pkg_dir].concat(),
            cargo_leptos_service.clone(),
        ) // Only need if using wasm-pack. Can be deleted if using cargo-leptos
        // .nest_service(&bundle_path, cargo_leptos_service) // Only needed if using cargo-leptos. Can be deleted if using wasm-pack and cargo-run
        //.nest_service("/static", static_service)
        .leptos_routes_with_handler(routes, custom_handler))
    // .with_state(state)
    // .layer(Extension(Arc::new(leptos_options))))
}

#[cfg(test)]
mod tests {
    use super::escape_for_script_tag;

    #[test]
    fn script_bootstrap_json_cannot_close_script_tag() {
        let payload = format!(
            r#"{{"name":"</script><script>alert(1)</script>&{}{}"}}"#,
            '\u{2028}', '\u{2029}'
        );
        let escaped = escape_for_script_tag(&payload);

        assert!(!escaped.contains("</script>"));
        assert!(escaped.contains("\\u003c/script\\u003e"));
        assert!(escaped.contains("\\u0026"));
        assert!(escaped.contains("\\u2028"));
        assert!(escaped.contains("\\u2029"));
    }
}
