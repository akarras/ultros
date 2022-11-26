use maud::{html, Markup};

use crate::web::{
    oauth::AuthDiscordUser,
    templates::{components::header::Header, page::Page},
};

pub(crate) struct HomePage {
    pub(crate) user: Option<AuthDiscordUser>,
}

impl Page for HomePage {
    fn get_name(&'_ self) -> String {
        "Ultros Home".to_string()
    }

    fn get_description(&'_ self) -> Option<String> {
        Some("Ultros is a ffxiv marketboard analysis tool that enables users to engage in abritrage, keep listings lowest, and get real time alerts".to_string())
    }

    fn get_tags(&'_ self) -> Option<String> {
        Some("ffxiv, final fantasy 14, marketboard, realtime, listings, fast".to_string())
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
