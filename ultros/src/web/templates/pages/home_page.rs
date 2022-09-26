use maud::{html, Markup};

use crate::web::{
    oauth::AuthDiscordUser,
    templates::{components::header::Header, page::Page},
};

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
                user: self.user.as_ref(),
            })
            h1 class="hero-title" {
                "Dominate the marketboard"
            }
        }
    }
}
