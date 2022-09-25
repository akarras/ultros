use maud::{html, Markup};

use crate::web::{templates::{page::Page, components::header::Header}, oauth::AuthDiscordUser};

pub(crate) struct HomePage {
  pub(crate) user: Option<AuthDiscordUser>,
}

impl Page for HomePage {
  fn get_name<'a>(&self) -> &'a str {
      "Home page"
  }

  fn draw_body(&self) -> Markup {
      html! {
          (Header {
              user: &self.user
          })
          h1 class="hero-title" {
              "Dominate the marketboard"
          }
      }
  }
}