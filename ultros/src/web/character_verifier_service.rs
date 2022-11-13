use lodestone::{model::profile::Profile, LodestoneError};
use sha2::{Digest, Sha256};
use thiserror::Error;
use ultros_db::{entity::ffxiv_character_verification, UltrosDb};

#[derive(Debug, Clone)]
pub(crate) struct CharacterVerifierService {
    pub(crate) db: UltrosDb,
    pub(crate) client: reqwest::Client,
}

#[derive(Debug, Error)]
pub enum VerifierError {
    #[error("Error reading from lodestone {0}")]
    Lodestone(#[from] LodestoneError),
    #[error("Generic DB error {0}")]
    DbError(#[from] anyhow::Error),
    #[error("Verification string did not match")]
    VerificationFailure,
}

impl CharacterVerifierService {
    /// Creates the verification token for the user to put in their bio and stores it in the database.
    pub(crate) async fn start_verification(
        &self,
        character_id: u32,
        discord_user_id: i64,
    ) -> Result<String, VerifierError> {
        let mut hasher = Sha256::new();
        hasher.update(&discord_user_id.to_le_bytes());
        hasher.update(&character_id.to_le_bytes());
        let auth_token = hasher.finalize();
        let challenge_string = base64::encode(auth_token);
        self.db
            .create_verification_challenge(&challenge_string, discord_user_id, character_id as i32)
            .await?;

        Ok(challenge_string)
    }

    pub(crate) async fn check_verification(
        &self,
        verification_id: i32,
    ) -> Result<(), VerifierError> {
        let verification = self.db.get_verification_challenge(verification_id).await?;
        let ffxiv_character_verification::Model {
            discord_user_id,
            ffxiv_character_id,
            challenge,
            ..
        } = &verification;
        self.compare_verification(&challenge, *ffxiv_character_id as u32)
            .await?;
        // verification success, now add the owned character
        self.db
            .create_owned_character(*discord_user_id, *ffxiv_character_id)
            .await?;
        self.db.remove_verification_challenge(verification).await?;
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
