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

pub(crate) async fn claim_character(
    State(verification_service): State<CharacterVerifierService>,
    Path(character_id): Path<i32>,
    user: AuthDiscordUser,
) -> Result<RenderPage<ClaimCharacter>, WebError> {
    let (id, verification_string) = verification_service
        .start_verification(character_id as u32, user.id as i64)
        .await?;
    Ok(RenderPage(ClaimCharacter {
        verification_string,
        verificiation_id: id,
        user,
    }))
}

pub(crate) struct ClaimCharacter {
    verification_string: String,
    verificiation_id: i32,
    user: AuthDiscordUser,
}

impl Page for ClaimCharacter {
    fn get_name(&'_ self) -> &'_ str {
        "Verification Page"
    }

    fn draw_body(&self) -> maud::Markup {
        html! {
            ((Header {
                user: Some(&self.user)
              }))
            div class="container" {
                div class="main-content" {
                    div class="content-well flex-column" {
                        span class="content-title" {
                            "Verification"
                        }
                        span {
                            "Add " ((self.verification_string)) " to your profile and come back here to verify your account"
                        }
                        a href={ "/characters/verify/" ((self.verificiation_id)) } {
                            "Verify Character"
                        }
                    }
                }
            }
        }
    }
}
