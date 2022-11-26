use maud::html;

use crate::web::templates::{components::header::Header, page::Page};

pub(crate) struct UnauthorizedPage {}

impl Page for UnauthorizedPage {
    fn get_name(&'_ self) -> String {
        "Unauthorized".to_string()
    }

    fn draw_body(&self) -> maud::Markup {
        html! {
          ((Header{
            user: None
          }))
          div class="container" {
            div class="main-content" {
              h1 {
                "Not logged in"
              }
              span {
                "To view this page you must be logged in."
              }
            }
          }
        }
    }
}
