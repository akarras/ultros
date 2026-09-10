//! Atomic guest projection import with durable account-scoped receipts.
use crate::{
    UltrosDb,
    entity::{list, list_activity, list_doc, list_item},
    lists::ListError,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ConnectionTrait, DbBackend, EntityTrait, Statement,
    TransactionTrait,
};
use ultros_api_types::list::{AdoptGuestList, AdoptGuestListResponse, ListActivityKind};
use ultros_api_types::world_helper::AnySelector;
use ultros_list_doc::{ListDocument, MetaSnapshot, RowKey, RowSnapshot};

/// Creation details are absent on replay, so callers never publish duplicate events.
pub struct AdoptionOutcome {
    pub response: AdoptGuestListResponse,
    pub created: Option<(list::Model, Vec<list_item::Model>, list_activity::Model)>,
}

pub fn validate_adoption(request: &AdoptGuestList) -> Result<(), ListError> {
    let scope_id = match request.wdr_filter {
        AnySelector::World(id) | AnySelector::Datacenter(id) | AnySelector::Region(id) => id,
    };
    if scope_id <= 0 {
        return Err(ListError::BadRequest("invalid list market scope"));
    }
    for value in [
        &request.adoption_key,
        &request.device_list_id,
        &request.source_revision,
    ] {
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.:".contains(&c))
        {
            return Err(ListError::BadRequest("invalid adoption identifier"));
        }
    }
    if request.name.trim().is_empty() || request.name.len() > 200 || request.items.len() > 2000 {
        return Err(ListError::BadRequest(
            "list name or item count exceeds limits",
        ));
    }
    let mut keys = std::collections::HashSet::new();
    for row in &request.items {
        if row.item_id <= 0
            || row.quantity < 0
            || row.acquired < 0
            || row.target_price.is_some_and(|p| p < 0)
            || !keys.insert((row.item_id, row.hq))
        {
            return Err(ListError::BadRequest(
                "invalid or duplicate guest list item",
            ));
        }
    }
    Ok(())
}

impl UltrosDb {
    pub async fn adopt_guest_list(
        &self,
        owner: i64,
        request: AdoptGuestList,
    ) -> anyhow::Result<AdoptGuestListResponse> {
        Ok(self
            .adopt_guest_list_with_outcome(owner, request)
            .await?
            .response)
    }

    pub async fn adopt_guest_list_with_outcome(
        &self,
        owner: i64,
        request: AdoptGuestList,
    ) -> anyhow::Result<AdoptionOutcome> {
        validate_adoption(&request)?;
        if request.expected_owner != owner {
            return Err(ListError::Forbidden(
                "signed-in account changed; review the destination account",
            )
            .into());
        }
        let txn = self.db.begin().await?;
        // Account lock covers both random attempt keys and two tabs generating
        // different keys for the same device list. Never lock an attacker key.
        let account = txn
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "SELECT id, username FROM discord_user WHERE id = $1 FOR UPDATE",
                [owner.into()],
            ))
            .await?;
        if account.is_none() {
            return Err(ListError::NotFound.into());
        }
        let receipt = txn.query_one_raw(Statement::from_sql_and_values(DbBackend::Postgres,
            "SELECT list_id, device_list_id, source_revision FROM guest_list_adoption WHERE owner = $1 AND (adoption_key = $2 OR device_list_id = $3) ORDER BY (adoption_key = $2) DESC LIMIT 1",
            [owner.into(), request.adoption_key.clone().into(), request.device_list_id.clone().into()])).await?;
        if let Some(receipt) = receipt {
            let device_list_id: String = receipt.try_get("", "device_list_id")?;
            if device_list_id != request.device_list_id {
                return Err(ListError::BadRequest(
                    "adoption key already belongs to another device list",
                )
                .into());
            }
            let list_id: i32 = receipt.try_get("", "list_id")?;
            let destination = list::Entity::find_by_id(list_id)
                .one(&txn)
                .await?
                .ok_or(ListError::NotFound)?;
            if destination.owner != owner {
                return Err(
                    ListError::Forbidden("destination account does not own this list").into(),
                );
            }
            let response = AdoptGuestListResponse {
                list_id,
                owner,
                device_list_id,
                source_revision: receipt.try_get("", "source_revision")?,
            };
            txn.commit().await?;
            return Ok(AdoptionOutcome {
                response,
                created: None,
            });
        }
        let scope = request.wdr_filter;
        let destination = list::ActiveModel {
            owner: Set(owner),
            name: Set(request.name.clone()),
            world_id: Set(match scope {
                AnySelector::World(id) => Some(id),
                _ => None,
            }),
            datacenter_id: Set(match scope {
                AnySelector::Datacenter(id) => Some(id),
                _ => None,
            }),
            region_id: Set(match scope {
                AnySelector::Region(id) => Some(id),
                _ => None,
            }),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
        let rows: Vec<_> = request
            .items
            .iter()
            .map(|row| RowSnapshot {
                key: RowKey::new(row.item_id, row.hq),
                need: i64::from(row.quantity),
                acquired: i64::from(row.acquired),
                target: row.target_price,
            })
            .collect();
        let doc = ListDocument::from_rows(
            MetaSnapshot {
                name: request.name.clone(),
                scope: Some(scope),
            },
            &rows,
        );
        // Batch the projection while holding the account lock.
        let inserted = if request.items.is_empty() {
            Vec::new()
        } else {
            list_item::Entity::insert_many(request.items.into_iter().map(|row| {
                list_item::ActiveModel {
                    list_id: Set(destination.id),
                    item_id: Set(row.item_id),
                    hq: Set(row.hq),
                    quantity: Set(Some(row.quantity)),
                    acquired: Set(Some(row.acquired)),
                    target_price: Set(row.target_price),
                    ..Default::default()
                }
            }))
            .exec_with_returning(&txn)
            .await?
        };
        // Activity and receipt commit together, preventing duplicate history on retry.
        let username: String = account
            .as_ref()
            .expect("account checked above")
            .try_get("", "username")?;
        let activity = list_activity::ActiveModel {
            list_id: Set(destination.id),
            actor_user_id: Set(owner),
            actor_username: Set(username.clone()),
            kind: Set(ListActivityKind::ListCreated.as_str().to_string()),
            payload: Set(serde_json::json!({"name": request.name, "source": "device", "item_count": inserted.len()})),
            message: Set(format!("{username} created list {} from a device list", destination.name)),
            created_at: Set(chrono::Utc::now().into()),
            ..Default::default()
        }.insert(&txn).await?;
        list_doc::ActiveModel {
            list_id: Set(destination.id),
            snapshot: Set(doc.export_snapshot()?),
            version: Set(doc.version()),
            changes_since_compaction: Set(0),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
        txn.execute_raw(Statement::from_sql_and_values(DbBackend::Postgres,
            "INSERT INTO guest_list_adoption (owner, adoption_key, device_list_id, source_revision, list_id) VALUES ($1, $2, $3, $4, $5)",
            [owner.into(), request.adoption_key.into(), request.device_list_id.clone().into(), request.source_revision.clone().into(), destination.id.into()])).await?;
        txn.commit().await?;
        Ok(AdoptionOutcome {
            response: AdoptGuestListResponse {
                list_id: destination.id,
                owner,
                device_list_id: request.device_list_id,
                source_revision: request.source_revision,
            },
            created: Some((destination, inserted, activity)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ColumnTrait, QueryFilter};
    use ultros_api_types::list::GuestListItem;
    fn request() -> AdoptGuestList {
        AdoptGuestList {
            expected_owner: 1,
            adoption_key: "attempt-1".into(),
            device_list_id: "device-1".into(),
            source_revision: "4".into(),
            name: "Raid supplies".into(),
            wdr_filter: AnySelector::World(1),
            items: vec![GuestListItem {
                item_id: 1,
                hq: None,
                quantity: 2,
                acquired: 0,
                target_price: None,
            }],
        }
    }
    #[test]
    fn rejects_invalid_projection_without_silent_normalization() {
        let mut value = request();
        assert!(validate_adoption(&value).is_ok());
        value.items.push(value.items[0].clone());
        assert!(validate_adoption(&value).is_err());
        value.items.pop();
        value.items[0].quantity = -1;
        assert!(validate_adoption(&value).is_err());
        value.items[0].quantity = 1;
        value.adoption_key = "".into();
        assert!(validate_adoption(&value).is_err());
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn concurrent_import_retries_preserve_receipt_and_account_isolation() {
        let conn =
            sea_orm::Database::connect(std::env::var("MIGRATION_TEST_DATABASE_URL").unwrap())
                .await
                .unwrap();
        let db = UltrosDb::from_connection(conn);
        db.get_connection().execute_unprepared("INSERT INTO region (id, name) VALUES (1, 'ScratchRegion') ON CONFLICT (id) DO NOTHING").await.unwrap();
        let owner = db
            .get_or_create_discord_user(990_000_000_391, "GuestImportOwner".into())
            .await
            .unwrap();
        let other = db
            .get_or_create_discord_user(990_000_000_392, "GuestImportOther".into())
            .await
            .unwrap();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_string();
        let mut value = request();
        value.adoption_key = nonce.clone();
        value.device_list_id = nonce;
        value.wdr_filter = AnySelector::Region(1);
        value.expected_owner = owner.id;
        let mut second_tab = value.clone();
        second_tab.adoption_key.push_str("-other-tab");
        let (a, b) = tokio::join!(
            db.adopt_guest_list(owner.id, value.clone()),
            db.adopt_guest_list(owner.id, second_tab)
        );
        let a = a.unwrap();
        assert_eq!(a.list_id, b.unwrap().list_id);
        let stored = db.list_doc_snapshot(a.list_id, owner.id).await.unwrap();
        assert!(ListDocument::from_snapshot(&stored.snapshot).is_ok());
        value.source_revision = "5".into();
        value.items[0].quantity = 99;
        let retry = db.adopt_guest_list(owner.id, value.clone()).await.unwrap();
        assert_eq!(retry.source_revision, "4");
        assert_eq!(retry.list_id, a.list_id);
        let activity = db
            .get_list_activity(a.list_id, owner.id, 50, None)
            .await
            .unwrap();
        assert_eq!(
            activity.len(),
            1,
            "retries must not duplicate creation history"
        );
        assert_eq!(activity[0].kind, ListActivityKind::ListCreated.as_str());
        let rows = list_item::Entity::find()
            .filter(list_item::Column::ListId.eq(a.list_id))
            .all(db.get_connection())
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].quantity,
            Some(2),
            "retry must not apply a newer payload"
        );
        let mut same_name = value.clone();
        same_name.adoption_key.push_str("-new");
        same_name.device_list_id.push_str("-new");
        same_name
            .items
            .extend([2, 3].into_iter().map(|item_id| GuestListItem {
                item_id,
                hq: Some(false),
                quantity: item_id,
                acquired: 1,
                target_price: Some(100),
            }));
        let independent = db
            .adopt_guest_list(owner.id, same_name.clone())
            .await
            .unwrap();
        let imported = list_item::Entity::find()
            .filter(list_item::Column::ListId.eq(independent.list_id))
            .all(db.get_connection())
            .await
            .unwrap();
        assert_eq!(
            imported.len(),
            3,
            "batch insert preserves all projected rows"
        );
        assert!(imported.iter().any(|row| row.item_id == 3
            && row.quantity == Some(3)
            && row.acquired == Some(1)
            && row.target_price == Some(100)));
        let mut empty = value.clone();
        empty.adoption_key.push_str("-empty");
        empty.device_list_id.push_str("-empty");
        empty.items.clear();
        let empty = db
            .adopt_guest_list_with_outcome(owner.id, empty)
            .await
            .unwrap();
        assert!(empty.created.as_ref().unwrap().1.is_empty());
        db.delete_list(empty.response.list_id, owner.id)
            .await
            .unwrap();
        assert_ne!(
            independent.list_id, a.list_id,
            "matching names must never merge lists"
        );
        same_name.adoption_key = value.adoption_key.clone();
        assert!(
            db.adopt_guest_list(owner.id, same_name).await.is_err(),
            "an existing key must not be rebound to another existing device list"
        );
        assert!(db.adopt_guest_list(other.id, value.clone()).await.is_err());
        let mut other_value = value.clone();
        other_value.expected_owner = other.id;
        let separate = db.adopt_guest_list(other.id, other_value).await.unwrap();
        assert_ne!(separate.list_id, a.list_id);
        db.delete_list(a.list_id, owner.id).await.unwrap();
        assert!(
            db.adopt_guest_list(owner.id, value).await.is_err(),
            "deleted imports must not resurrect"
        );
        db.delete_list(separate.list_id, other.id).await.unwrap();
        db.delete_list(independent.list_id, owner.id).await.unwrap();
    }
}
