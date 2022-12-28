use axum::extract::{State, Path, Query};
use maud::html;
use serde::Deserialize;
use ultros_db::{entity::list, UltrosDb};

use crate::web::{templates::{page::{Page, RenderPage}, components::header::Header}, oauth::AuthDiscordUser, error::WebError};

#[derive(Deserialize)]
pub(crate) struct ListItemAddQueryParams {
    item_name: Option<String>,
    item_hq: Option<bool>
}

pub(crate) async fn list_item_add(user: AuthDiscordUser, Path(list_id): Path<i32>, State(db): State<UltrosDb>, Query(query): Query<ListItemAddQueryParams>) -> Result<RenderPage<ListItemAdd>, WebError> {
    let ListItemAddQueryParams { item_name, item_hq } = query;
    let list = db.get_list(list_id, user.id as i64).await?;
    match &item_name {
        Some(item_name) => {
            let (id, _) = xiv_gen_db::decompress_data().items.iter().find(|(_, i)| i.name == *item_name).ok_or(WebError::InvalidItem(0))?;
            db.add_item_to_list(&list, user.id as i64, id.0, item_hq).await?;
        },
        _ => {}
    }
    Ok(RenderPage(ListItemAdd { user, list, add_success: item_name }))
}

pub(crate) struct ListItemAdd {
    add_success: Option<String>,
    user: AuthDiscordUser,
    list: list::Model
}

impl Page for ListItemAdd {
    fn get_name(&'_ self) -> String {
        format!("Add item to {}", self.list.name)
    }

    fn draw_body(&self) -> maud::Markup {
        html!{
            ((Header { user: Some(&self.user) }))
            div class="container" {
                div class="main-content" {
                    @if let Some(name) = &self.add_success {
                        div class="content-well" {
                            "Successfully added item "((name))
                        }
                    }
                    div class="content-well" {
                        form {
                            label for="item_name" {"Item name"}
                            input type="text" id="item_name" name="item_name" {}
                            label for="item_hq" {"HQ?"}
                            input type="radio" id="item_hq" name="item_hq" value="true" {"HQ"}
                            input type="radio" id="item_hq" name="item_hq" value="false" {"not hq"}
                            input type="radio" id="item_hq" name="item_hq" value="" {"don't care"}
                            input type="submit" value="Add Item" {}
                        }
                    }
                }
            }
        }
    }
}

