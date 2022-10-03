use maud::{html, Markup};
use ultros_db::entity::{retainer, world};

use crate::web::{
    oauth::AuthDiscordUser,
    templates::{components::header::Header, page::Page},
};

pub(crate) struct AddRetainer {
    pub(crate) user: Option<AuthDiscordUser>,
    pub(crate) search_results: Vec<(retainer::Model, Option<world::Model>)>,
    pub(crate) search_text: String,
}

impl Page for AddRetainer {
    fn get_name<'a>(&'a self) -> &'a str {
        "Home page"
    }

    fn draw_body(&self) -> Markup {
        html! {
          (Header {
            user: self.user.as_ref(),
          })
          script src="/static/retainer.js"{}
          div class="container" {
            div class="main-content" {
              label for="retainer-name" {
                "retainer name"
              }
              input id="retainer-name" value=((self.search_text));
              a id="retainer-button" class="btn" href={ "/retainers/add?search=" ((urlencoding::encode(&self.search_text))) } {
                "search"
              }
              ul {
                @for (retainer, world) in &self.search_results {
                  li{
                    span { ((retainer.name)) " - " ((world.as_ref().map(|w| w.name.as_str()).unwrap_or_default())) }
                    a class="btn" href={"/retainers/add/" ((retainer.id))} {
                      "add"
                    }
                  }
                }
              }
            }
          }
        }
    }
}
