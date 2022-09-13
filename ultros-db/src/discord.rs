use crate::{entity::discord_user, UltrosDb};
use anyhow::Result;
use sea_orm::{ActiveModelTrait, Set};
use tracing::instrument;

impl UltrosDb {
    #[instrument(skip(self))]
    pub async fn get_or_create_discord_user(
        &self,
        user_id: u64,
        name: String,
    ) -> Result<discord_user::Model> {
        let user = discord_user::ActiveModel {
            id: Set(user_id as i64),
            username: Set(name),
        };

        Ok(user.insert(&self.db).await?)
    }
}
