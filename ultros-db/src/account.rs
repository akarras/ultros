use anyhow::Result;
use sea_orm::{
    ColumnTrait, Condition, EntityTrait, ModelTrait, QueryFilter, QuerySelect, QueryTrait,
    TransactionTrait,
};

use crate::{
    UltrosDb,
    entity::{alert, alert_price, discord_user, list, list_item, owned_retainers},
};

impl UltrosDb {
    pub async fn delete_discord_user(&self, discord_id: i64) -> Result<()> {
        let txn = self.db.begin().await?;
        let user = discord_user::Entity::find_by_id(discord_id)
            .one(&txn)
            .await?
            .ok_or_else(|| anyhow::anyhow!("user not found"))?;

        // These older tables do not cascade from the account. Lists do not
        // even have an owner foreign key, so deleting just the user leaves
        // their lists behind. Remove their restrictive children first.
        let owned_lists = list::Entity::find()
            .select_only()
            .column(list::Column::Id)
            .filter(list::Column::Owner.eq(discord_id))
            .into_query();
        let owned_alerts = alert::Entity::find()
            .select_only()
            .column(alert::Column::Id)
            .filter(alert::Column::Owner.eq(discord_id))
            .into_query();
        alert_price::Entity::delete_many()
            .filter(
                Condition::any()
                    .add(alert_price::Column::ListId.in_subquery(owned_lists.clone()))
                    .add(alert_price::Column::AlertId.in_subquery(owned_alerts)),
            )
            .exec(&txn)
            .await?;
        list_item::Entity::delete_many()
            .filter(list_item::Column::ListId.in_subquery(owned_lists))
            .exec(&txn)
            .await?;
        list::Entity::delete_many()
            .filter(list::Column::Owner.eq(discord_id))
            .exec(&txn)
            .await?;
        owned_retainers::Entity::delete_many()
            .filter(owned_retainers::Column::DiscordId.eq(discord_id))
            .exec(&txn)
            .await?;

        // Alerts, character claims, endpoints, and group/share memberships
        // already cascade. Keep public character and market data intact.
        user.delete(&txn).await?;
        txn.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use migration::{Migrator, MigratorTrait};
    use sea_orm::{ConnectOptions, ConnectionTrait, Database, DbBackend, Statement};

    async fn fixture() -> (UltrosDb, String) {
        let mut options =
            ConnectOptions::new(std::env::var("MIGRATION_TEST_DATABASE_URL").unwrap());
        // Keep search_path on the same session, isolated from other DB tests.
        options
            .max_connections(1)
            .min_connections(1)
            .sqlx_logging(false);
        let connection = Database::connect(options).await.unwrap();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let schema = format!("account_deletion_{}_{nonce}", std::process::id());
        connection
            .execute_unprepared(&format!(
                "CREATE SCHEMA {schema}; SET search_path TO {schema}"
            ))
            .await
            .unwrap();
        Migrator::up(&connection, None).await.unwrap();
        connection
            .execute_unprepared(
                "INSERT INTO discord_user (id, username) VALUES (1, 'Delete'), (2, 'Keep'), (3, 'Empty');
                 INSERT INTO region (id, name) VALUES (1, 'Test');
                 INSERT INTO datacenter (id, name, region_id) VALUES (1, 'Test', 1);
                 INSERT INTO world (id, name, datacenter_id) VALUES (1, 'Test', 1);
                 INSERT INTO retainer_city (id, name) VALUES (1, 'Test');
                 INSERT INTO retainer (id, world_id, name, retainer_city_id) VALUES (1, 1, 'Shared', 1);
                 INSERT INTO final_fantasy_character (id, first_name, last_name, world_id) VALUES (1, 'Shared', 'Character', 1);
                 INSERT INTO owned_retainers (id, retainer_id, discord_id, character_id) VALUES (1, 1, 1, 1), (2, 1, 2, 1);
                 INSERT INTO owned_ffxiv_character (ffxiv_character_id, discord_user_id) VALUES (1, 1), (1, 2);
                 INSERT INTO list (id, owner, name) VALUES (1, 1, 'Delete'), (2, 2, 'Keep');
                 INSERT INTO list_item (id, list_id, item_id, quantity) VALUES (1, 1, 5, 2), (2, 2, 5, 3);
                 INSERT INTO alert (id, owner) VALUES (1, 1), (2, 2);
                 INSERT INTO alert_price (id, list_id, alert_id, price) VALUES (1, 1, 1, 100), (2, 2, 2, 200), (3, 2, 1, 300);
                 INSERT INTO notification_endpoint (id, user_id, name, method, config) VALUES (1, 1, 'Delete', 'DiscordDm', '{}'), (2, 2, 'Keep', 'DiscordDm', '{}');
                 INSERT INTO user_group (id, name, owner_id) VALUES (1, 'Delete', 1), (2, 'Keep', 2);
                 INSERT INTO user_group_member (group_id, user_id) VALUES (1, 2), (2, 1);
                 INSERT INTO list_shared_user (list_id, user_id, permission) VALUES (1, 2, 1), (2, 1, 1);
                 INSERT INTO list_invite (id, list_id, permission) VALUES ('delete', 1, 1), ('keep', 2, 1);
                 INSERT INTO list_doc (list_id, snapshot, version) VALUES (1, ''::bytea, ''::bytea), (2, ''::bytea, ''::bytea);
                 INSERT INTO list_activity (list_id, actor_user_id, actor_username, kind, payload, message) VALUES (1, 1, 'Delete', 'Test', '{}', 'Delete'), (2, 2, 'Keep', 'Test', '{}', 'Keep'), (2, 1, 'Delete', 'Test', '{}', 'Delete');",
            )
            .await
            .unwrap();
        (UltrosDb::from_connection(connection), schema)
    }

    async fn assert_counts(db: &UltrosDb, expected: &[(&str, i64)]) {
        for (table, count) in expected {
            let row = db
                .db
                .query_one_raw(Statement::from_string(
                    DbBackend::Postgres,
                    format!("SELECT count(*) AS count FROM {table}"),
                ))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(row.try_get::<i64>("", "count").unwrap(), *count, "{table}");
        }
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn deletes_account_data_and_preserves_other_users_and_public_data() {
        let (db, schema) = fixture().await;
        assert!(
            discord_user::Entity::delete_by_id(1)
                .exec(&db.db)
                .await
                .is_err(),
            "the original user-only deletion must reproduce the restrictive foreign key failure"
        );
        db.delete_discord_user(1).await.unwrap();
        assert_counts(
            &db,
            &[
                ("discord_user", 2),
                ("owned_retainers", 1),
                ("owned_ffxiv_character", 1),
                ("retainer", 1),
                ("final_fantasy_character", 1),
                ("list", 1),
                ("list_item", 1),
                ("alert", 1),
                ("alert_price", 1),
                ("notification_endpoint", 1),
                ("user_group", 1),
                ("user_group_member", 0),
                ("list_shared_user", 0),
                ("list_invite", 1),
                ("list_doc", 1),
                ("list_activity", 1),
            ],
        )
        .await;
        assert!(
            discord_user::Entity::find_by_id(1)
                .one(&db.db)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            list::Entity::find_by_id(2)
                .one(&db.db)
                .await
                .unwrap()
                .unwrap()
                .owner,
            2
        );
        assert_eq!(
            owned_retainers::Entity::find_by_id(2)
                .one(&db.db)
                .await
                .unwrap()
                .unwrap()
                .discord_id,
            2
        );

        // An account with no related rows can also be deleted, and retrying
        // a deleted account must leave the remaining user's data alone.
        db.delete_discord_user(3).await.unwrap();
        assert!(db.delete_discord_user(1).await.is_err());
        assert_counts(&db, &[("discord_user", 1), ("list", 1)]).await;
        db.db
            .execute_unprepared(&format!("DROP SCHEMA {schema} CASCADE"))
            .await
            .unwrap();
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn deletion_failure_rolls_back_all_account_data() {
        let (db, schema) = fixture().await;
        db.db
            .execute_unprepared(
                "CREATE TABLE deletion_blocker (user_id bigint REFERENCES discord_user(id));
                 INSERT INTO deletion_blocker VALUES (1)",
            )
            .await
            .unwrap();
        assert!(db.delete_discord_user(1).await.is_err());
        assert_counts(
            &db,
            &[
                ("discord_user", 3),
                ("owned_retainers", 2),
                ("owned_ffxiv_character", 2),
                ("list", 2),
                ("list_item", 2),
                ("alert_price", 3),
                ("alert", 2),
                ("notification_endpoint", 2),
                ("user_group", 2),
                ("user_group_member", 2),
                ("list_shared_user", 2),
                ("list_invite", 2),
                ("list_doc", 2),
                ("list_activity", 3),
            ],
        )
        .await;
        db.db
            .execute_unprepared(&format!("DROP SCHEMA {schema} CASCADE"))
            .await
            .unwrap();
    }
}
