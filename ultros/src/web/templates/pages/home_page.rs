use maud::{html, Markup};

use crate::web::{
    oauth::AuthDiscordUser,
    templates::{components::header::Header, page::Page},
};

pub(crate) struct HomePage {
    pub(crate) user: Option<AuthDiscordUser>,
}

impl Page for HomePage {
    fn get_name(&'_ self) -> &'_ str {
        "Home page"
    }

    fn draw_body(&self) -> Markup {
        html! {
            (Header {
                user: self.user.as_ref(),
            })
            div class="container" {
                h1 class="hero-title" {
                    "Dominate the marketboard"
                }
            }
        }
    }
}
