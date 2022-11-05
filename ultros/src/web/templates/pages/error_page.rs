use maud::html;

use crate::web::templates::{components::header::Header, page::Page};

pub struct ErrorPage {}

impl Page for ErrorPage {
    fn get_name(&'_ self) -> &'_ str {
        "Error"
    }

    fn draw_body(&self) -> maud::Markup {
        html! {
          ((Header{
            user: None
          }))
          div class="container" {
            div class="main-content" {
              h2 {
                "Error"
              }
              span {
                "Server error has occured"
              }
            }
          }
        }
    }
}
