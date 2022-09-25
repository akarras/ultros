use maud::{html, PreEscaped, Render};

use crate::web::templates::components::SearchBox;

pub(crate) struct Header;

impl Render for Header {
    fn render(&self) -> maud::Markup {
        html! {
          div class="gradient-outer"{div class="gradient"{}};
          header {
            div class="header" {
              a class="nav-item" href="/alerts" {
                "Alerts"
              };
              a class="nav-item" href="/analyzer" {
                "Analyzer"
              };
              a class="nav-item" href="/retainers" {
                "Retainers"
              };
              (SearchBox);
              a class="btn nav-item" href="/discord" {
                "Invite"
              }
              a class="btn nav-item" href="/login" {
                "Login"
              }
            }
          }
        }
    }
}
