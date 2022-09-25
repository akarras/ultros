use aho_corasick::Match;
use axum::{extract::Path, response::Html};
use maud::{html, Render};
use reqwest::StatusCode;
use std::fmt::Write;

use super::item_search_index::do_query;

fn insert_start_end(
    base_str: &str,
    start_tag: &str,
    end_tag: &str,
    matches: &Vec<Match>,
) -> String {
    let mut last_i = 0;
    let item_name = base_str;
    let mut joined_str = "".to_string();
    println!("all matches {matches:?}");
    for m in matches {
        let start = m.start();
        let end = m.end();
        write!(
            &mut joined_str,
            "{}{start_tag}{}{end_tag}",
            &item_name[last_i..start],
            &item_name[start..end]
        )
        .unwrap();
        last_i = end;
        println!("printing match {start} {end} {joined_str}");
    }
    if last_i < item_name.len() {
        joined_str += &item_name[last_i..];
    }
    joined_str
}

fn calculate_score(matches: &Vec<Match>) -> usize {
    matches.iter().map(|m| m.len()).sum()
}

pub(crate) async fn search_items(
    Path(search_str): Path<String>,
) -> Result<Html<String>, (StatusCode, String)> {
    let matches = do_query(&search_str).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("search issue: {e}"),
        )
    })?;
    // todo highlight with ahochorasick the parts of the string that match the name
    let categories = &xiv_gen_db::decompress_data().item_ui_categorys;

    Ok(Html(html!{
    div {
      @for (item_id, item) in matches {
        div class="search-result" {
          // todo this should be the logged in user's world if we can get it.
          a href= {"/listings/Sargatanas/"(item_id)} {
            img src={"https://universalis-ffxiv.github.io/universalis-assets/icon2x/" (item_id) ".png"};
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

#[cfg(test)]
mod test {
    use axum::response::Html;

    use super::search_items;

    #[test]
    fn test_item_print() {}
}
