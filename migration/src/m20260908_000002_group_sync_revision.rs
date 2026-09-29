use sea_orm_migration::prelude::*;
#[derive(DeriveMigrationName)]
pub struct Migration;
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(UserGroup::Table)
                    .add_column_if_not_exists(
                        ColumnDef::new(UserGroup::SyncRevision)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .to_owned(),
            )
            .await
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(UserGroup::Table)
                    .drop_column(UserGroup::SyncRevision)
                    .to_owned(),
            )
            .await
    }
}
#[derive(DeriveIden)]
enum UserGroup {
    Table,
    SyncRevision,
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm_migration::sea_orm::{
        ConnectionTrait, Database, DbBackend, Statement, TransactionTrait,
    };

    /// Temporary tables shadow real tables only on this transaction's connection,
    /// so testing an upgrade never removes columns from application fixtures.
    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn upgrade_preserves_existing_group_and_role_membership() {
        let db = Database::connect(std::env::var("DATABASE_URL").expect("DATABASE_URL"))
            .await
            .unwrap();
        let txn = db.begin().await.unwrap();
        for sql in [
            "CREATE TEMP TABLE user_group (id integer PRIMARY KEY, name text NOT NULL) ON COMMIT DROP",
            "CREATE TEMP TABLE group_role_member (role_id integer, user_id bigint, PRIMARY KEY(role_id,user_id)) ON COMMIT DROP",
            "INSERT INTO user_group VALUES (1, 'Existing group')",
            "INSERT INTO group_role_member VALUES (2, 3)",
            "SET LOCAL search_path = pg_temp",
        ] {
            txn.execute_raw(Statement::from_string(DbBackend::Postgres, sql.to_owned()))
                .await
                .unwrap();
        }
        let manager = SchemaManager::new(&txn);
        crate::m20260908_000001_group_role_member_added_at::Migration
            .up(&manager)
            .await
            .unwrap();
        Migration.up(&manager).await.unwrap();
        let row = txn.query_one_raw(Statement::from_string(DbBackend::Postgres,
            "SELECT g.name, g.sync_revision, m.role_id, m.user_id, m.added_at IS NOT NULL AS stamped FROM user_group g CROSS JOIN group_role_member m".to_owned())).await.unwrap().unwrap();
        assert_eq!(row.try_get::<String>("", "name").unwrap(), "Existing group");
        assert_eq!(row.try_get::<i64>("", "sync_revision").unwrap(), 0);
        assert_eq!(row.try_get::<i32>("", "role_id").unwrap(), 2);
        assert_eq!(row.try_get::<i64>("", "user_id").unwrap(), 3);
        assert!(row.try_get::<bool>("", "stamped").unwrap());
        txn.rollback().await.unwrap();
    }
}
