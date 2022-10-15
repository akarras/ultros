use maud::{html, Render};

use crate::web::{oauth::AuthDiscordUser, templates::components::SearchBox};

pub(crate) struct Header<'a> {
    pub(crate) user: Option<&'a AuthDiscordUser>,
}

impl<'a> Render for Header<'a> {
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
              a class="btn nav-item" href="/invitebot" {
                "Invite Bot"
              }
              @if let Some(user) = self.user {
                a class="btn nav-item" href="/logout" {
                  "Logout"
                }
                a href="/profile" {
                  img class="avatar" src=((user.avatar_url)) alt=((user.name));
                }
              } @else {
                a class="btn nav-item" href="/login" {
                  "Login"
                }
              }
            }
          }
        }
    }
}
