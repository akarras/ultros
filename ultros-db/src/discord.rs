use crate::{UltrosDb, entity::discord_user};
use anyhow::Result;
use migration::OnConflict;
use sea_orm::{EntityTrait, Set};
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
        Ok(discord_user::Entity::insert(user)
            .on_conflict(
                OnConflict::column(discord_user::Column::Id)
                    .update_column(discord_user::Column::Username)
                    .to_owned(),
            )
            .exec_with_returning(&self.db)
            .await?)
    }
}
