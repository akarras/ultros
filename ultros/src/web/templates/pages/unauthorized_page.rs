use maud::html;

use crate::web::templates::{page::Page, components::header::Header};

pub(crate) struct UnauthorizedPage {}

impl Page for UnauthorizedPage {
    fn get_name<'a>(&self) -> &'a str {
        "Unauthorized"
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