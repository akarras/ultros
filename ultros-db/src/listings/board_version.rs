use std::collections::HashMap;

use anyhow::Result;
use sea_orm::{
    ConnectionTrait, DatabaseTransaction, DbBackend, FromQueryResult, Statement, TransactionTrait,
};
use universalis::{ItemId, WorldId};

use crate::UltrosDb;

/// Capture before requesting a complete board from Universalis. A snapshot may
/// replace its board only if no other writer has committed since this token.
#[derive(Clone, Copy, Debug)]
pub struct BoardVersion {
    world: WorldId,
    item: ItemId,
    revision: i64,
}

#[derive(Debug, thiserror::Error)]
#[error("listing board changed while its snapshot was being fetched")]
pub struct StaleBoardSnapshot;

#[derive(FromQueryResult)]
struct RevisionRow {
    world_id: i32,
    item_id: i32,
    revision: i64,
}

impl UltrosDb {
    /// One indexed query for an entire REST batch. Unseen boards start at zero.
    pub async fn capture_listing_boards(
        &self,
        boards: &[(WorldId, ItemId)],
    ) -> Result<HashMap<(WorldId, ItemId), BoardVersion>> {
        let mut versions: HashMap<_, _> = boards
            .iter()
            .map(|&(world, item)| {
                (
                    (world, item),
                    BoardVersion {
                        world,
                        item,
                        revision: 0,
                    },
                )
            })
            .collect();
        if boards.is_empty() {
            return Ok(versions);
        }
        let _budget = self.ingest_budget.acquire().await?;
        let mut bindings = Vec::with_capacity(boards.len() * 2);
        let tuples = boards
            .iter()
            .enumerate()
            .map(|(i, (world, item))| {
                bindings.extend([world.0.into(), item.0.into()]);
                format!("(${},${})", i * 2 + 1, i * 2 + 2)
            })
            .collect::<Vec<_>>()
            .join(",");
        let rows = RevisionRow::find_by_statement(Statement::from_sql_and_values(
            DbBackend::Postgres,
            format!("SELECT world_id,item_id,revision FROM listing_board_revision WHERE (world_id,item_id) IN ({tuples})"),
            bindings,
        )).all(&self.db).await?;
        for row in rows {
            let world = WorldId(row.world_id);
            let item = ItemId(row.item_id);
            versions.insert(
                (world, item),
                BoardVersion {
                    world,
                    item,
                    revision: row.revision,
                },
            );
        }
        Ok(versions)
    }

    pub async fn capture_listing_board(
        &self,
        world: WorldId,
        item: ItemId,
    ) -> Result<BoardVersion> {
        Ok(self.capture_listing_boards(&[(world, item)]).await?[&(world, item)])
    }

    pub(crate) async fn lock_listing_board(
        &self,
        world: WorldId,
        item: ItemId,
        expected: Option<BoardVersion>,
    ) -> Result<DatabaseTransaction> {
        let transaction = self.db.begin().await?;
        self.lock_listing_board_on(&transaction, world, item, expected)
            .await?;
        Ok(transaction)
    }

    /// Apply the same writer fence to an existing transaction. Maintenance sets
    /// its PostgreSQL lock/statement deadlines before taking this lock.
    pub(crate) async fn lock_listing_board_on(
        &self,
        transaction: &DatabaseTransaction,
        world: WorldId,
        item: ItemId,
        expected: Option<BoardVersion>,
    ) -> Result<()> {
        if let Some(expected) = expected {
            anyhow::ensure!(
                expected.world == world && expected.item == item,
                "snapshot token belongs to another board"
            );
        }
        transaction.execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "INSERT INTO listing_board_revision(world_id,item_id,revision) VALUES($1,$2,0) ON CONFLICT DO NOTHING",
            [world.0.into(), item.0.into()],
        )).await?;
        let current = RevisionRow::find_by_statement(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT world_id,item_id,revision FROM listing_board_revision WHERE world_id=$1 AND item_id=$2 FOR UPDATE",
            [world.0.into(), item.0.into()],
        )).one(transaction).await?.expect("the board row was inserted in this transaction");
        if expected.is_some_and(|expected| expected.revision != current.revision) {
            return Err(StaleBoardSnapshot.into());
        }
        Ok(())
    }

    pub(super) async fn commit_listing_board(
        &self,
        transaction: DatabaseTransaction,
        world: WorldId,
        item: ItemId,
        mark_ingested: bool,
    ) -> Result<()> {
        transaction.execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "UPDATE listing_board_revision SET revision=revision+1 WHERE world_id=$1 AND item_id=$2",
            [world.0.into(), item.0.into()],
        )).await?;
        if mark_ingested {
            self.set_last_updated_on(&transaction, world, item).await?;
        }
        transaction.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test_db::{connect, listing, seed_market};

    #[tokio::test]
    #[ignore = "requires ULTROS_PLUGIN_TEST_DATABASE_URL pointing at a disposable loopback database"]
    async fn listing_snapshots_are_fenced_and_atomic_postgres() {
        let db = connect().await;
        seed_market(&db).await;
        let world = WorldId(30001);
        let item = ItemId(9001);

        // A delta arriving during an HTTP fetch wins over its old snapshot.
        let old = db.capture_listing_board(world, item).await.unwrap();
        let a = listing("A", 100);
        db.add_listings(vec![a.clone()], item, world).await.unwrap();
        let stale = db
            .update_listings(vec![listing("A", 200)], item, world, old)
            .await
            .unwrap_err();
        assert!(stale.downcast_ref::<StaleBoardSnapshot>().is_some());
        let rows = db
            .get_all_listings_in_worlds(&[world.0], item)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].price_per_unit, 100);

        // A slow snapshot cannot resurrect a listing removed during the fetch.
        let old = db.capture_listing_board(world, item).await.unwrap();
        db.remove_listings(vec![a.clone()], item, world)
            .await
            .unwrap();
        assert!(
            db.update_listings(vec![a.clone()], item, world, old)
                .await
                .is_err()
        );
        assert!(
            db.get_all_listings_in_worlds(&[world.0], item)
                .await
                .unwrap()
                .is_empty()
        );

        // Even an already-applied removal is evidence newer than that fetch.
        let old = db.capture_listing_board(world, item).await.unwrap();
        db.remove_listings(vec![a.clone()], item, world)
            .await
            .unwrap();
        assert!(
            db.update_listings(vec![a.clone()], item, world, old)
                .await
                .is_err()
        );

        // Two overlapping fetches cannot both apply against one revision.
        let version = db.capture_listing_board(world, item).await.unwrap();
        db.update_listings(vec![a, listing("B", 200)], item, world, version)
            .await
            .unwrap();
        assert!(
            db.update_listings(vec![], item, world, version)
                .await
                .is_err()
        );
        let before = db
            .get_all_listings_in_worlds(&[world.0], item)
            .await
            .unwrap();
        let marker = db
            .get_listing_last_updated_for_worlds(item, &[world.0])
            .await
            .unwrap()[0]
            .date_time;
        let version = db.capture_listing_board(world, item).await.unwrap();

        db.get_connection().execute_unprepared(
            "CREATE OR REPLACE FUNCTION ultros_audit_listing_fail() RETURNS trigger
             LANGUAGE plpgsql AS $fixture$ BEGIN RAISE EXCEPTION 'listing fixture failure'; END; $fixture$;
             DROP TRIGGER IF EXISTS ultros_audit_listing_fail ON active_listing;
             CREATE TRIGGER ultros_audit_listing_fail BEFORE INSERT OR UPDATE ON active_listing
             FOR EACH ROW WHEN (NEW.world_id=30001 AND NEW.price_per_unit=999999)
             EXECUTE FUNCTION ultros_audit_listing_fail()"
        ).await.unwrap();
        let failed = db
            .update_listings(
                vec![listing("C", 300), listing("D", 999999)],
                item,
                world,
                version,
            )
            .await;
        db.get_connection()
            .execute_unprepared(
                "DROP TRIGGER ultros_audit_listing_fail ON active_listing;
             DROP FUNCTION ultros_audit_listing_fail()",
            )
            .await
            .unwrap();
        assert!(failed.is_err());
        let after = db
            .get_all_listings_in_worlds(&[world.0], item)
            .await
            .unwrap();
        let row_states = |rows: Vec<crate::entity::active_listing::Model>| {
            let mut states: Vec<_> = rows
                .into_iter()
                .map(|row| (row.id, row.price_per_unit))
                .collect();
            states.sort_unstable();
            states
        };
        assert_eq!(
            row_states(before),
            row_states(after),
            "inserts and deletions must roll back together"
        );
        assert_eq!(
            db.get_listing_last_updated_for_worlds(item, &[world.0])
                .await
                .unwrap()[0]
                .date_time,
            marker
        );
        assert_eq!(
            db.capture_listing_board(world, item)
                .await
                .unwrap()
                .revision,
            version.revision
        );
        // The failed transaction did not consume the token; retrying succeeds.
        db.update_listings(vec![listing("C", 300)], item, world, version)
            .await
            .unwrap();

        // Board locks serialize writers without locking unrelated boards.
        let held = db.lock_listing_board(world, item, None).await.unwrap();
        let writer_db = db.clone();
        let blocked = tokio::spawn(async move {
            writer_db
                .add_listings(vec![listing("E", 400)], item, world)
                .await
        });
        let independent = db.add_listings(vec![listing("Other", 50)], ItemId(9002), world);
        tokio::time::timeout(std::time::Duration::from_secs(5), independent)
            .await
            .unwrap()
            .unwrap();
        held.rollback().await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), blocked)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}
