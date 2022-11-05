use maud::Render;

use crate::web::oauth::AuthDiscordUser;

struct EditRetainers {
    user: Option<AuthDiscordUser>,
}

impl Render for EditRetainers {}
