use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Backfill any NULL created_at rows before flipping to NOT NULL.
        db.execute_unprepared(
            r#"UPDATE notification_endpoint SET created_at = now() WHERE created_at IS NULL"#,
        )
        .await?;

        // Convert TIMESTAMP -> TIMESTAMPTZ. Existing values were written as UTC
        // (see ultros-db: `created_at: Set(chrono::Utc::now())`), so interpret
        // the naive timestamp as UTC.
        db.execute_unprepared(
            r#"ALTER TABLE notification_endpoint
                ALTER COLUMN created_at TYPE timestamp with time zone
                USING created_at AT TIME ZONE 'UTC',
                ALTER COLUMN created_at SET NOT NULL,
                ALTER COLUMN created_at SET DEFAULT now()"#,
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            r#"ALTER TABLE notification_endpoint
                ALTER COLUMN created_at DROP NOT NULL,
                ALTER COLUMN created_at TYPE timestamp
                USING created_at AT TIME ZONE 'UTC',
                ALTER COLUMN created_at SET DEFAULT current_timestamp"#,
        )
        .await?;
        Ok(())
    }
}
