use axum::extract::{Path, State};
use maud::html;

use crate::web::{
    character_verifier_service::CharacterVerifierService,
    error::WebError,
    oauth::AuthDiscordUser,
    templates::{
        components::header::Header,
        page::{Page, RenderPage},
    },
};

pub(crate) async fn verify_character(
    State(service): State<CharacterVerifierService>,
    Path(verification_id): Path<i32>,
    user: AuthDiscordUser,
) -> Result<RenderPage<VerifyCharacter>, WebError> {
    service.check_verification(verification_id).await?;
    Ok(RenderPage(VerifyCharacter { user }))
}

pub(crate) struct VerifyCharacter {
    user: AuthDiscordUser,
}

impl Page for VerifyCharacter {
    fn get_name(&'_ self) -> String {
        "Verify Character".to_string()
    }

    fn draw_body(&self) -> maud::Markup {
        html! {
            ((Header {
                user: Some(&self.user)
            }))
            div class="container" {
                div class="main-content flex flex-column" {
                    span class="content-title" {
                        "Character verified"
                    }
                    a href="/profile" {
                        "Return to profile"
                    }
                }
            }
        }
    }
}
