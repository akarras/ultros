use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ListActivity::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ListActivity::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(ListActivity::ListId).integer().not_null())
                    .col(
                        ColumnDef::new(ListActivity::ActorUserId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ListActivity::ActorUsername)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(ListActivity::Kind).text().not_null())
                    .col(ColumnDef::new(ListActivity::ListItemId).integer())
                    .col(ColumnDef::new(ListActivity::ItemId).integer())
                    .col(ColumnDef::new(ListActivity::Payload).json().not_null())
                    .col(ColumnDef::new(ListActivity::Message).text().not_null())
                    .col(
                        ColumnDef::new(ListActivity::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_list_activity_list_id")
                            .from(ListActivity::Table, ListActivity::ListId)
                            .to(List::Table, List::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_list_activity_actor_user_id")
                            .from(ListActivity::Table, ListActivity::ActorUserId)
                            .to(DiscordUser::Table, DiscordUser::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_list_activity_list_created")
                    .table(ListActivity::Table)
                    .col(ListActivity::ListId)
                    .col(ListActivity::CreatedAt)
                    .to_owned(),
            )
            .await?;

        // Dedupe list-update alerts per owner/list and persist owner on the
        // trigger row so future inserts can be protected by a normal unique
        // index instead of an application-only join check.
        manager
            .alter_table(
                Table::alter()
                    .table(AlertListUpdate::Table)
                    .add_column(ColumnDef::new(AlertListUpdate::Owner).big_integer())
                    .to_owned(),
            )
            .await?;

        let db = manager.get_connection();
        db.execute_unprepared(
            r#"
            UPDATE alert_list_update alu
            SET owner = a.owner
            FROM alert a
            WHERE a.id = alu.alert_id
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            WITH duplicate_pairs AS (
                SELECT DISTINCT
                    loser.alert_id AS loser_alert_id,
                    winner.alert_id AS winner_alert_id
                FROM alert_list_update loser
                JOIN alert_list_update winner
                  ON loser.owner = winner.owner
                 AND loser.list_id = winner.list_id
                 AND loser.id > winner.id
            )
            INSERT INTO alert_notification_rule (alert_id, endpoint_id)
            SELECT duplicate_pairs.winner_alert_id, rules.endpoint_id
            FROM duplicate_pairs
            JOIN alert_notification_rule rules
              ON rules.alert_id = duplicate_pairs.loser_alert_id
            ON CONFLICT DO NOTHING
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            WITH duplicate_alerts AS (
                SELECT DISTINCT loser.alert_id
                FROM alert_list_update loser
                JOIN alert_list_update winner
                  ON loser.owner = winner.owner
                 AND loser.list_id = winner.list_id
                 AND loser.id > winner.id
            )
            DELETE FROM alert a
            USING duplicate_alerts
            WHERE a.id = duplicate_alerts.alert_id
            "#,
        )
        .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(AlertListUpdate::Table)
                    .modify_column(
                        ColumnDef::new(AlertListUpdate::Owner)
                            .big_integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_alert_list_update_owner_list_unique")
                    .table(AlertListUpdate::Table)
                    .col(AlertListUpdate::Owner)
                    .col(AlertListUpdate::ListId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_alert_list_update_owner_list_unique")
                    .table(AlertListUpdate::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(AlertListUpdate::Table)
                    .drop_column(AlertListUpdate::Owner)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(ListActivity::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum ListActivity {
    Table,
    Id,
    ListId,
    ActorUserId,
    ActorUsername,
    Kind,
    ListItemId,
    ItemId,
    Payload,
    Message,
    CreatedAt,
}

#[derive(DeriveIden)]
enum List {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum DiscordUser {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum AlertListUpdate {
    Table,
    ListId,
    Owner,
}
