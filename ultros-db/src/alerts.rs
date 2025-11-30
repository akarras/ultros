use crate::UltrosDb;
use crate::entity::*;
use anyhow::Result;
use futures::future::try_join_all;
use sea_orm::*;

impl UltrosDb {
    pub async fn get_alert(&self, alert_id: i32) -> Result<Option<alert::Model>> {
        Ok(alert::Entity::find_by_id(alert_id).one(&self.db).await?)
    }

    pub async fn get_alert_discord_destinations(
        &self,
        alert_id: i32,
    ) -> Result<Vec<alert_discord_destination::Model>> {
        Ok(alert_discord_destination::Entity::find()
            .filter(alert_discord_destination::Column::AlertId.eq(alert_id))
            .all(&self.db)
            .await?)
    }

    pub async fn get_retainer_alert(
        &self,
        retainer_alert_id: i32,
    ) -> Result<Option<alert_retainer_undercut::Model>> {
        Ok(
            alert_retainer_undercut::Entity::find_by_id(retainer_alert_id)
                .one(&self.db)
                .await?,
        )
    }

    pub async fn get_all_alerts(&self) -> Result<Vec<alert::Model>> {
        Ok(alert::Entity::find().all(&self.db).await?)
    }

    pub async fn get_retainer_alerts_for_related_alert_id(
        &self,
        alert_id: i32,
    ) -> Result<Vec<alert_retainer_undercut::Model>> {
        Ok(alert_retainer_undercut::Entity::find()
            .filter(alert_retainer_undercut::Column::AlertId.eq(alert_id))
            .all(&self.db)
            .await?)
    }

    pub async fn add_discord_retainer_alert(
        &self,
        channel_id: i64,
        discord_user: i64,
        margin_percent: i32,
    ) -> Result<alert_retainer_undercut::Model> {
        let alert = alert::Entity::insert(alert::ActiveModel {
            id: ActiveValue::default(),
            owner: Set(discord_user),
        })
        .exec_with_returning(&self.db)
        .await?;
        let _ = alert_discord_destination::Entity::insert(alert_discord_destination::ActiveModel {
            id: ActiveValue::default(),
            alert_id: Set(alert.id),
            channel_id: Set(channel_id),
        })
        .exec(&self.db)
        .await?;
        let retainer_margin =
            alert_retainer_undercut::Entity::insert(alert_retainer_undercut::ActiveModel {
                id: ActiveValue::default(),
                alert_id: Set(alert.id),
                margin_percent: Set(margin_percent),
            })
            .exec_with_returning(&self.db)
            .await?;
        Ok(retainer_margin)
    }

    /// Attempts to delete the retainer alert from the database. Returns an error if the channel_id/discord_user do not exist in the database
    /// or if another database error occured.
    pub async fn delete_discord_alert(
        &self,
        channel_id: i64,
        discord_user: i64,
    ) -> Result<(alert::Model, Vec<alert_retainer_undercut::Model>)> {
        let (discord, alert) = alert_discord_destination::Entity::find()
            .find_also_related(alert::Entity)
            .filter(
                alert_discord_destination::Column::ChannelId
                    .eq(channel_id)
                    .and(alert::Column::Owner.eq(discord_user)),
            )
            .one(&self.db)
            .await?
            .ok_or(anyhow::Error::msg(
                "Alert not found for this discord channel",
            ))?;
        let alert =
            alert.expect("Since we're querying based on FK we shoudln't ever panic here...");
        // now query to ensure this alert has a retainer undercut associated
        let undercut = alert_retainer_undercut::Entity::find()
            .filter(alert_retainer_undercut::Column::AlertId.eq(alert.id))
            .all(&self.db)
            .await?;
        discord.delete(&self.db).await?;
        let _ = try_join_all(undercut.clone().into_iter().map(|u| u.delete(&self.db))).await?;
        alert.clone().delete(&self.db).await?;
        Ok((alert, undercut))
    }
}
