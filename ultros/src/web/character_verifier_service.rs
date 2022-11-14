use lodestone::{model::profile::Profile, LodestoneError};
use thiserror::Error;
use ultros_db::{entity::ffxiv_character_verification, UltrosDb};

#[derive(Debug, Clone)]
pub(crate) struct CharacterVerifierService {
    pub(crate) db: UltrosDb,
    pub(crate) client: reqwest::Client,
}

#[derive(Debug, Error)]
pub(crate) enum VerifierError {
    #[error("Error reading from lodestone {0}")]
    Lodestone(#[from] LodestoneError),
    #[error("Generic DB error {0}")]
    DbError(#[from] anyhow::Error),
    #[error("Verification string did not match")]
    VerificationFailure,
}

impl CharacterVerifierService {
    pub(crate) async fn start_verification(
        &self,
        character_id: u32,
        discord_user_id: i64,
    ) -> String {
        unimplemented!("Need to implement verification");
    }

    pub(crate) async fn check_verification(
        &self,
        verification_id: i32,
    ) -> Result<(), VerifierError> {
        let ffxiv_character_verification::Model {
            id,
            discord_user_id,
            ffxiv_character_id,
            challenge,
        } = self.db.get_character_challenge(verification_id).await?;
        self.compare_verification(&challenge, ffxiv_character_id as u32)
            .await?;
        // verification success, now add the owned character
        self.db
            .create_owned_character(discord_user_id, ffxiv_character_id)
            .await?;
        Ok(())
    }

    async fn compare_verification(
        &self,
        verifier_string: &str,
        character_id: u32,
    ) -> Result<(), VerifierError> {
        let profile = Profile::get_async(&self.client, character_id).await?;
        let intro = profile.self_introduction.contains(verifier_string);
        if intro {
            Ok(())
        } else {
            Err(VerifierError::VerificationFailure)
        }
    }
}
