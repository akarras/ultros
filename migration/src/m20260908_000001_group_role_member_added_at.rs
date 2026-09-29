use sea_orm_migration::prelude::*;

/// Membership creation timestamps retained for audit and upgrade compatibility.
/// Snapshot correctness is enforced by the later group sync_revision migration:
/// timestamps on extant memberships cannot protect against stale additions after
/// a removal, because a removed row no longer carries a timestamp.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(GroupRoleMember::Table)
                    .add_column_if_not_exists(
                        ColumnDef::new(GroupRoleMember::AddedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(GroupRoleMember::Table)
                    .drop_column(GroupRoleMember::AddedAt)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum GroupRoleMember {
    Table,
    AddedAt,
}
