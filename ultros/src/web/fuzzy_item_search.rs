use axum::{extract::Path, response::Html};
use axum_extra::extract::CookieJar;
use maud::{html, Render};
use reqwest::StatusCode;

use crate::utils;

use super::item_search_index::do_query;

pub(crate) async fn search_items(
    Path(search_str): Path<String>,
    cookie_jar: CookieJar,
) -> Result<Html<String>, (StatusCode, String)> {
    let matches = do_query(&search_str).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("search issue: {e}"),
        )
    })?;
    let categories = &xiv_gen_db::decompress_data().item_ui_categorys;
    let world = cookie_jar
        .get("last_listing_view")
        .map(|c| c.value())
        .unwrap_or("North-America");
    Ok(Html(html!{
      div {
        @for (item_id, item) in matches {
          div class="search-result" {
            // todo this should be the logged in user's world if we can get it.
            a href= {"/listings/"(world)"/"(item_id)} {
              img src=((utils::get_item_icon_url(item_id)));
              div class="search-result-details" {
                span class="item-name" {
                  (&item.name)
                }
                span class="item-type" {
                  (categories.get(&item.item_ui_category).map(|i| i.name.as_str()).unwrap_or_default())
                }
              }
            }
          }
        }
      }
  }.render().0))
}
