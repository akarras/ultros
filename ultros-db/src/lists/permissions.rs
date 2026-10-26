//! One SQL permission expression for single-list checks, batches, and authorized
//! reads. No permission value is cached across requests or trusted from callers.

use super::ListError;
use crate::{
    UltrosDb,
    entity::{list, list_item},
};
use anyhow::Result;
use sea_orm::{
    AccessMode, ColumnTrait, EntityTrait, FromQueryResult, IsolationLevel, QueryFilter, Statement,
    TransactionTrait,
};
use std::collections::HashMap;
use ultros_api_types::list::ListPermission;

// Keep the existing grant rules, including implicit group-owner access and
// role-member-only role access. Frozen membership is still membership. Normalize
// stored values before MAX so an invalid value cannot mask a legitimate grant.
const EFFECTIVE_PERMISSION: &str = r#"
CASE WHEN l.owner=$1 THEN 3 ELSE COALESCE((
    SELECT MAX(grant_value) FROM (
        SELECT CASE permission WHEN 1 THEN 1 WHEN 2 THEN 2 WHEN 3 THEN 3 ELSE 0 END AS grant_value
        FROM list_shared_user WHERE list_id=l.id AND user_id=$1
        UNION ALL
        SELECT CASE s.permission WHEN 1 THEN 1 WHEN 2 THEN 2 WHEN 3 THEN 3 ELSE 0 END
        FROM list_shared_group s JOIN user_group g ON g.id=s.group_id
        WHERE s.list_id=l.id AND (g.owner_id=$1 OR EXISTS(
            SELECT 1 FROM user_group_member m WHERE m.group_id=g.id AND m.user_id=$1))
        UNION ALL
        SELECT CASE s.permission WHEN 1 THEN 1 WHEN 2 THEN 2 WHEN 3 THEN 3 ELSE 0 END
        FROM list_shared_role s JOIN group_role r ON r.id=s.role_id
        JOIN group_role_member m ON m.role_id=r.id
        WHERE s.list_id=l.id AND m.user_id=$1
    ) grants
),0) END
"#;

#[derive(FromQueryResult)]
struct PermissionRow {
    id: i32,
    permission: i32,
}

#[derive(FromQueryResult)]
struct AuthorizedRow {
    #[sea_orm(nested)]
    list: list::Model,
    owner_name: Option<String>,
    permission: i32,
}

pub struct AuthorizedList {
    pub list: list::Model,
    pub owner_name: String,
    pub permission: ListPermission,
    pub items: Vec<list_item::Model>,
}

impl UltrosDb {
    pub async fn get_permissions(
        &self,
        ids: &[i32],
        user: i64,
    ) -> Result<HashMap<i32, ListPermission>> {
        let mut result = HashMap::new();
        for chunk in ids.chunks(500) {
            let placeholders = (0..chunk.len())
                .map(|i| format!("${}", i + 2))
                .collect::<Vec<_>>()
                .join(",");
            let mut values = vec![user.into()];
            values.extend(chunk.iter().map(|id| (*id).into()));
            let statement = Statement::from_sql_and_values(
                self.db.get_database_backend(),
                format!(
                    "SELECT l.id,({EFFECTIVE_PERMISSION}) AS permission FROM \"list\" l WHERE l.id IN ({placeholders})"
                ),
                values,
            );
            let rows = PermissionRow::find_by_statement(statement)
                .all(&self.db)
                .await?;
            result.extend(
                rows.into_iter()
                    .map(|r| (r.id, ListPermission::from(r.permission as i16))),
            );
        }
        Ok(result)
    }

    /// Preserve the existing candidate visibility while batching its grants.
    pub async fn get_lists_with_permissions(
        &self,
        user: i64,
    ) -> Result<Vec<(list::Model, Option<String>, ListPermission)>> {
        let lists = self.get_lists_for_user(user).await?;
        let ids = lists.iter().map(|(list, _)| list.id).collect::<Vec<_>>();
        let mut permissions = self.get_permissions(&ids, user).await?;
        lists
            .into_iter()
            .map(|(list, name)| {
                let permission = permissions.remove(&list.id).ok_or(ListError::NotFound)?;
                Ok((list, name, permission))
            })
            .collect()
    }

    pub async fn get_permission(&self, id: i32, user: i64) -> Result<ListPermission> {
        self.get_permissions(&[id], user)
            .await?
            .remove(&id)
            .ok_or_else(|| ListError::NotFound.into())
    }

    /// Read the authorization, metadata and items in one request-local snapshot.
    /// The caller supplies only an authenticated user ID, never a permission proof.
    pub async fn get_authorized_list(&self, id: i32, user: i64) -> Result<AuthorizedList> {
        let tx = self
            .db
            .begin_with_config(
                Some(IsolationLevel::RepeatableRead),
                Some(AccessMode::ReadOnly),
            )
            .await?;
        let statement = Statement::from_sql_and_values(
            self.db.get_database_backend(),
            format!(
                "SELECT l.*,u.username AS owner_name,({EFFECTIVE_PERMISSION}) AS permission FROM \"list\" l LEFT JOIN discord_user u ON u.id=l.owner WHERE l.id=$2"
            ),
            [user.into(), id.into()],
        );
        let row = AuthorizedRow::find_by_statement(statement)
            .one(&tx)
            .await?
            .ok_or(ListError::NotFound)?;
        let permission = ListPermission::from(row.permission as i16);
        if permission < ListPermission::Read {
            return Err(ListError::Forbidden("Insufficient permissions to read list").into());
        }
        let items = list_item::Entity::find()
            .filter(list_item::Column::ListId.eq(id))
            .all(&tx)
            .await?;
        tx.commit().await?;
        Ok(AuthorizedList {
            owner_name: row.owner_name.unwrap_or_else(|| row.list.owner.to_string()),
            list: row.list,
            permission,
            items,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ActiveModelTrait, ActiveValue::Set, ConnectionTrait};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires an explicit disposable PostgreSQL database"]
    async fn permission_matrix_revocation_and_bounded_read_queries() {
        let mut db = crate::common::test_db::connect().await;
        db.db.execute_unprepared(r#"
            DELETE FROM list_item WHERE list_id IN (SELECT id FROM "list" WHERE owner BETWEEN 9044000 AND 9044999);
            DELETE FROM "list" WHERE owner BETWEEN 9044000 AND 9044999;
            DELETE FROM user_group WHERE owner_id BETWEEN 9044000 AND 9044999;
            DELETE FROM discord_user WHERE id BETWEEN 9044000 AND 9044999;
            INSERT INTO discord_user(id,username) SELECT i,'Permission fixture '||i FROM generate_series(9044000,9044010) i;
        "#).await.unwrap();
        let owner = 9044000;
        let direct = 9044001;
        let member = 9044002;
        let group_owner = 9044003;
        let role_member = 9044004;
        let outsider = 9044005;
        let list = list::ActiveModel {
            owner: Set(owner),
            name: Set("Permission matrix".into()),
            ..Default::default()
        }
        .insert(&db.db)
        .await
        .unwrap();
        let group = crate::entity::user_group::ActiveModel {
            name: Set("Frozen grants".into()),
            owner_id: Set(group_owner),
            source: Set(1),
            frozen_reason: Set(Some("Bot removed".into())),
            ..Default::default()
        }
        .insert(&db.db)
        .await
        .unwrap();
        let role = crate::entity::group_role::ActiveModel {
            group_id: Set(group.id),
            name: Set("Shared role".into()),
            ..Default::default()
        }
        .insert(&db.db)
        .await
        .unwrap();
        db.db
            .execute_raw(Statement::from_sql_and_values(
                db.db.get_database_backend(),
                "INSERT INTO list_shared_user(list_id,user_id,permission) VALUES($1,$2,1)",
                [list.id.into(), direct.into()],
            ))
            .await
            .unwrap();
        db.db
            .execute_raw(Statement::from_sql_and_values(
                db.db.get_database_backend(),
                "INSERT INTO user_group_member(group_id,user_id,source) VALUES($1,$2,1)",
                [group.id.into(), member.into()],
            ))
            .await
            .unwrap();
        db.db
            .execute_raw(Statement::from_sql_and_values(
                db.db.get_database_backend(),
                "INSERT INTO list_shared_group(list_id,group_id,permission) VALUES($1,$2,2)",
                [list.id.into(), group.id.into()],
            ))
            .await
            .unwrap();
        db.db
            .execute_raw(Statement::from_sql_and_values(
                db.db.get_database_backend(),
                "INSERT INTO list_shared_role(list_id,role_id,permission) VALUES($1,$2,1)",
                [list.id.into(), role.id.into()],
            ))
            .await
            .unwrap();
        db.db
            .execute_raw(Statement::from_sql_and_values(
                db.db.get_database_backend(),
                "INSERT INTO group_role_member(role_id,user_id,added_at) VALUES($1,$2,NOW())",
                [role.id.into(), role_member.into()],
            ))
            .await
            .unwrap();
        let item = list_item::ActiveModel {
            list_id: Set(list.id),
            item_id: Set(42),
            quantity: Set(Some(7)),
            ..Default::default()
        }
        .insert(&db.db)
        .await
        .unwrap();
        for (user, expected) in [
            (owner, ListPermission::Owner),
            (direct, ListPermission::Read),
            (member, ListPermission::Write),
            (group_owner, ListPermission::Write),
            (role_member, ListPermission::Read),
            (outsider, ListPermission::None),
        ] {
            assert_eq!(db.get_permission(list.id, user).await.unwrap(), expected);
            assert_eq!(
                db.get_permissions(&[list.id], user).await.unwrap()[&list.id],
                expected
            );
            let result = db.get_authorized_list(list.id, user).await;
            if expected >= ListPermission::Read {
                let result = result.unwrap();
                assert_eq!(result.permission, expected);
                assert_eq!(result.owner_name, "Permission fixture 9044000");
                assert_eq!(result.items, vec![item.clone()]);
            } else {
                assert!(matches!(
                    result.err().unwrap().downcast_ref::<ListError>(),
                    Some(ListError::Forbidden(_))
                ));
            }
        }
        // Role grants do not implicitly apply to a role's group owner.
        db.db
            .execute_raw(Statement::from_sql_and_values(
                db.db.get_database_backend(),
                "DELETE FROM list_shared_group WHERE list_id=$1",
                [list.id.into()],
            ))
            .await
            .unwrap();
        assert_eq!(
            db.get_permission(list.id, group_owner).await.unwrap(),
            ListPermission::None
        );
        // Invalid stored grants cannot mask a valid role grant, and a fresh
        // authorization observes revocation without a process-global cache.
        db.db
            .execute_raw(Statement::from_sql_and_values(
                db.db.get_database_backend(),
                "INSERT INTO list_shared_user(list_id,user_id,permission) VALUES($1,$2,99)",
                [list.id.into(), role_member.into()],
            ))
            .await
            .unwrap();
        assert_eq!(
            db.get_permission(list.id, role_member).await.unwrap(),
            ListPermission::Read
        );
        db.db
            .execute_raw(Statement::from_sql_and_values(
                db.db.get_database_backend(),
                "DELETE FROM group_role_member WHERE role_id=$1",
                [role.id.into()],
            ))
            .await
            .unwrap();
        assert_eq!(
            db.get_permission(list.id, role_member).await.unwrap(),
            ListPermission::None
        );
        assert!(db.get_authorized_list(list.id, role_member).await.is_err());
        db.db
            .execute_raw(Statement::from_sql_and_values(
                db.db.get_database_backend(),
                "UPDATE list_shared_user SET permission=2 WHERE list_id=$1 AND user_id=$2",
                [list.id.into(), direct.into()],
            ))
            .await
            .unwrap();
        assert_eq!(
            db.get_authorized_list(list.id, direct)
                .await
                .unwrap()
                .permission,
            ListPermission::Write
        );
        db.db
            .execute_raw(Statement::from_sql_and_values(
                db.db.get_database_backend(),
                "DELETE FROM list_shared_user WHERE list_id=$1 AND user_id=$2",
                [list.id.into(), direct.into()],
            ))
            .await
            .unwrap();
        assert!(db.get_authorized_list(list.id, direct).await.is_err());
        assert!(matches!(
            db.get_permission(i32::MAX, owner)
                .await
                .unwrap_err()
                .downcast_ref::<ListError>(),
            Some(ListError::NotFound)
        ));
        assert!(matches!(
            db.get_authorized_list(i32::MAX, owner)
                .await
                .err()
                .unwrap()
                .downcast_ref::<ListError>(),
            Some(ListError::NotFound)
        ));

        // Batching and the authorized detail read have fixed SELECT counts.
        db.db.execute_raw(Statement::from_sql_and_values(db.db.get_database_backend(),"INSERT INTO \"list\"(owner,name) SELECT $1,'Batch '||i FROM generate_series(1,1000) i",[owner.into()])).await.unwrap();
        let selects = Arc::new(AtomicUsize::new(0));
        let observed = selects.clone();
        db.db.set_metric_callback(move |info| {
            if info.statement.sql.trim_start().starts_with("SELECT") {
                observed.fetch_add(1, Ordering::SeqCst);
            }
        });
        let lists = db.get_lists_with_permissions(owner).await.unwrap();
        assert_eq!(lists.len(), 1001);
        assert!(lists.iter().all(|(_, _, p)| *p == ListPermission::Owner));
        assert_eq!(
            selects.swap(0, Ordering::SeqCst),
            7,
            "four candidate reads and three 500-ID batches"
        );
        let detail = db.get_authorized_list(list.id, owner).await.unwrap();
        assert_eq!(detail.items, vec![item]);
        assert_eq!(
            selects.swap(0, Ordering::SeqCst),
            2,
            "one authorized header and one item read"
        );
        assert!(db.get_permissions(&[], owner).await.unwrap().is_empty());
        assert_eq!(selects.load(Ordering::SeqCst), 0);

        // Pause after the authorized header. Revocation and a concurrent item
        // edit must not mix two versions in the in-flight response; the next
        // request must observe the revoked grant.
        db.db
            .execute_raw(Statement::from_sql_and_values(
                db.db.get_database_backend(),
                "INSERT INTO list_shared_user(list_id,user_id,permission) VALUES($1,$2,1)",
                [list.id.into(), direct.into()],
            ))
            .await
            .unwrap();
        let (seen_tx, seen_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let pause = std::sync::Mutex::new(Some((seen_tx, release_rx)));
        db.db.set_metric_callback(move |info| {
            if info.statement.sql.starts_with("SELECT l.*")
                && let Some((seen, release)) = pause.lock().unwrap().take()
            {
                let _ = seen.send(());
                tokio::task::block_in_place(|| {
                    release
                        .recv_timeout(std::time::Duration::from_secs(10))
                        .unwrap();
                });
            }
        });
        let reader = db.clone();
        let read = tokio::spawn(async move { reader.get_authorized_list(list.id, direct).await });
        tokio::time::timeout(std::time::Duration::from_secs(10), seen_rx)
            .await
            .unwrap()
            .unwrap();
        let changed = db
            .db
            .execute_raw(Statement::from_sql_and_values(
                db.db.get_database_backend(),
                "DELETE FROM list_shared_user WHERE list_id=$1 AND user_id=$2",
                [list.id.into(), direct.into()],
            ))
            .await;
        let updated = db
            .db
            .execute_raw(Statement::from_sql_and_values(
                db.db.get_database_backend(),
                "UPDATE list_item SET quantity=99 WHERE list_id=$1",
                [list.id.into()],
            ))
            .await;
        release_tx.send(()).unwrap();
        changed.unwrap();
        updated.unwrap();
        assert_eq!(read.await.unwrap().unwrap().items[0].quantity, Some(7));
        assert!(db.get_authorized_list(list.id, direct).await.is_err());
    }
}
