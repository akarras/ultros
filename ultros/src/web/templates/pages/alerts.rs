use maud::html;

use crate::web::{
    oauth::AuthDiscordUser,
    templates::{components::header::Header, page::Page},
};

pub(crate) struct AlertsPage {
    pub(crate) discord_user: AuthDiscordUser,
}

impl Page for AlertsPage {
    fn get_name(&self) -> String {
        "Alerts".to_string()
    }

    fn draw_body(&self) -> maud::Markup {
        html! {
            ((Header { user: Some(&self.discord_user) }))
            script src="/static/alerts.js" {}
            div class="container" {
                div class="main-content" {
                    div id="alert-frame" {
                        h1 {
                            "alerts"
                        }
                        ul {
                            "retainer undercuts"
                        }
                        button onclick="notifyMe()" {
                            "enable notifications"
                        }
                    }
                }
            }
        }
    }
}
