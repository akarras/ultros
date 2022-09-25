use maud::{html, PreEscaped, Render};

pub(crate) struct SearchBox;

impl Render for SearchBox {
    fn render(&self) -> maud::Markup {
        html! {
          div class="search-container" id="search-container" {
            input class="search-box" id="search-box" type="text";
            div class="search-results" id="search-results" {
              // search results get loaded by javasceript
            }
          }
        }
    }
}
