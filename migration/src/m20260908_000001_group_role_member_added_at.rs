use sea_orm_migration::prelude::*;

/// When a user joined a role, so a stale reconcile cannot undo a fresh one.
///
/// Reconciliation reads a guild's whole member list from Discord — minutes of
/// paginated HTTP for a large guild — and only then computes and applies its
/// plan. A gateway event that lands in that window is *newer* information than
/// the snapshot, but the plan does not know that: the member is in `current`
/// (read after the event) and missing from `desired` (computed before it), so
/// the stale plan removes them again.
///
/// `added_at` is what lets the apply step tell the two apart. Reconciliation
/// passes the time its snapshot started, and removes skip any role membership
/// created after it. It lives in the database rather than in process memory on
/// purpose: it keeps working when the reconcile and the event are handled by
/// different replicas.
///
/// Existing rows get `now()` at migration time, which is the conservative
/// default — it makes them look freshly added, so the first reconcile after
/// deploy leaves them alone and the second one, snapshotting later, applies
/// normally.
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
