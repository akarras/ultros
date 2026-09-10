use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Keep the receipt when a destination is deleted, so a stale retry can
        // never resurrect a list. The authenticated owner is part of the key.
        manager
            .get_connection()
            .execute_unprepared(
                r#"
            CREATE TABLE guest_list_adoption (
                owner BIGINT NOT NULL REFERENCES discord_user(id) ON DELETE CASCADE,
                adoption_key VARCHAR(128) NOT NULL,
                device_list_id VARCHAR(128) NOT NULL,
                source_revision VARCHAR(128) NOT NULL,
                list_id INTEGER NOT NULL,
                PRIMARY KEY (owner, adoption_key),
                UNIQUE (owner, device_list_id)
            )
        "#,
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE guest_list_adoption")
            .await?;
        Ok(())
    }
}
