//! Private, unverified character/retainer observations. Raw game content IDs
//! are HMACed in memory and never enter a database row or public market data.
use super::auth::{PluginPrincipal, authorize};
use super::crypto::{digest, equal};
use super::{PluginError, PluginKeys, Result, bounded_text, exec, one, operation_id, query};
use crate::UltrosDb;
use chrono::Utc;
use sea_orm::{DatabaseTransaction, TransactionTrait};
use serde_json::{Value as Json, json};
use ultros_api_types::plugin::*;

fn validate_observation(value: &ObserveCharacter) -> Result<()> {
    if !bounded_text(&value.name, 64) || value.home_world <= 0 || value.retainers.len() > 10 {
        return Err(PluginError::Invalid("invalid_roster"));
    }
    for (i, retainer) in value.retainers.iter().enumerate() {
        if !bounded_text(&retainer.name, 64)
            || retainer
                .city
                .is_some_and(|c| ![1, 2, 3, 4, 7, 10, 12, 14].contains(&c))
            || value.retainers[..i]
                .iter()
                .any(|r| r.content_id == retainer.content_id)
        {
            return Err(PluginError::Invalid("invalid_retainer"));
        }
    }
    Ok(())
}

async fn observe(
    txn: &DatabaseTransaction,
    keys: &PluginKeys,
    owner: i64,
    value: &ObserveCharacter,
    create: bool,
) -> Result<i64> {
    validate_observation(value)?;
    let fingerprint = keys.private_fingerprint(owner, "character", &value.content_id)?;
    let existing = query(
        txn,
        "SELECT id FROM plugin_character WHERE owner=$1 AND fingerprint=$2",
        vec![owner.into(), fingerprint.clone().into()],
    )
    .await?;
    if existing.is_empty() {
        if !create {
            return Err(PluginError::Conflict("registration_required"));
        }
        let count = one(
            txn,
            "SELECT count(*)::bigint AS n FROM plugin_character WHERE owner=$1",
            vec![owner.into()],
        )
        .await?
        .try_get::<i64>("", "n")?;
        if count >= 32 {
            return Err(PluginError::Conflict("character_limit_reached"));
        }
    }
    let character=one(txn,"INSERT INTO plugin_character(owner,fingerprint,name,home_world) VALUES($1,$2,$3,$4) ON CONFLICT(owner,fingerprint) DO UPDATE SET name=excluded.name,home_world=excluded.home_world,observed_at=now() RETURNING id",vec![owner.into(),fingerprint.into(),value.name.clone().into(),value.home_world.into()]).await?.try_get::<i64>("","id")?;
    for retainer in &value.retainers {
        let fingerprint = keys.private_fingerprint(owner, "retainer", &retainer.content_id)?;
        if !create {
            // Upload-only grants may refresh known entries, never add or group one.
            exec(txn,"UPDATE plugin_retainer SET name=$1,city=$2,observed_at=now() WHERE owner=$3 AND fingerprint=$4 AND registered_character=$5",vec![retainer.name.clone().into(),retainer.city.into(),owner.into(),fingerprint.into(),character.into()]).await?;
        } else {
            exec(txn,"INSERT INTO plugin_retainer(owner,fingerprint,name,city,observed_character) VALUES($1,$2,$3,$4,$5) ON CONFLICT(owner,fingerprint) DO UPDATE SET name=excluded.name,city=excluded.city,observed_character=excluded.observed_character,observed_at=now()",vec![owner.into(),fingerprint.into(),retainer.name.clone().into(),retainer.city.into(),character.into()]).await?;
        }
    }
    Ok(character)
}

async fn roster(txn: &DatabaseTransaction, owner: i64) -> Result<Vec<UnverifiedCharacter>> {
    let mut result = Vec::new();
    for row in query(
        txn,
        "SELECT * FROM plugin_character WHERE owner=$1 ORDER BY name,id",
        vec![owner.into()],
    )
    .await?
    {
        let id = row.try_get::<i64>("", "id")?;
        let mut retainers = Vec::new();
        for r in query(txn,"SELECT * FROM plugin_retainer WHERE owner=$1 AND (observed_character=$2 OR registered_character=$2) ORDER BY name,id",vec![owner.into(),id.into()]).await? {
            let assigned=r.try_get::<Option<i64>>("","registered_character")?;
            retainers.push(PrivateRetainer { id:r.try_get::<i64>("","id")?.to_string(),name:r.try_get("","name")?,city:r.try_get("","city")?,registered:assigned==Some(id),revision:r.try_get("","revision")?,observed_at:r.try_get("","observed_at")?,grouping_conflict:assigned.is_some_and(|a| a!=id),public_retainer_id:r.try_get("","public_retainer_id")? });
        }
        result.push(UnverifiedCharacter {
            id: id.to_string(),
            name: row.try_get("", "name")?,
            home_world: row.try_get("", "home_world")?,
            registered: row.try_get("", "registered")?,
            verified: false,
            upload_enabled: row.try_get("", "upload_enabled")?,
            capture_epoch: row.try_get("", "capture_epoch")?,
            observed_at: row.try_get("", "observed_at")?,
            retainers,
        });
    }
    Ok(result)
}

/// Create missing retainer identities, then link an unambiguous record. Private
/// observations never overwrite existing city data or create public listings.
/// The caller holds the account transaction lock, including across retries.
async fn link_public_retainers(
    txn: &DatabaseTransaction,
    owner: i64,
    character: i64,
) -> Result<()> {
    exec(txn,
        "INSERT INTO retainer(name,world_id,retainer_city_id)
         SELECT DISTINCT ON(c.home_world,p.name) p.name,c.home_world,coalesce(p.city,0)
         FROM plugin_retainer p JOIN plugin_character c ON c.id=p.registered_character AND c.owner=p.owner
         WHERE p.owner=$1 AND p.registered_character=$2 ORDER BY c.home_world,p.name,p.id
         ON CONFLICT(world_id,name) DO NOTHING",
        vec![owner.into(),character.into()]).await?;
    exec(
        txn,
        "UPDATE plugin_retainer p SET public_retainer_id=NULL FROM plugin_character c,retainer r
         WHERE p.owner=$1 AND p.registered_character=$2 AND c.id=$2 AND c.owner=p.owner
         AND r.id=p.public_retainer_id AND (r.name<>p.name OR r.world_id<>c.home_world)",
        vec![owner.into(), character.into()],
    )
    .await?;
    let matches=query(txn,
        "SELECT p.id,min(r.id) AS public_id FROM plugin_retainer p
         JOIN plugin_character c ON c.id=p.registered_character AND c.owner=p.owner
         JOIN retainer r ON r.name=p.name AND r.world_id=c.home_world
         WHERE p.owner=$1 AND p.registered_character=$2 AND p.public_retainer_id IS NULL
         AND NOT EXISTS(SELECT 1 FROM plugin_retainer other WHERE other.owner=p.owner AND other.public_retainer_id=r.id)
         AND NOT EXISTS(SELECT 1 FROM plugin_retainer other JOIN plugin_character oc ON oc.id=other.registered_character AND oc.owner=other.owner
           WHERE other.owner=p.owner AND other.id<>p.id AND other.name=p.name AND oc.home_world=c.home_world)
         GROUP BY p.id HAVING count(*)=1",
        vec![owner.into(),character.into()]).await?;
    for row in matches {
        exec(
            txn,
            "UPDATE plugin_retainer SET public_retainer_id=$1 WHERE owner=$2 AND id=$3",
            vec![
                row.try_get::<i32>("", "public_id")?.into(),
                owner.into(),
                row.try_get::<i64>("", "id")?.into(),
            ],
        )
        .await?;
    }
    // Existing website assignments and order are never overwritten. A repeated
    // registration adds no duplicate owned rows, even with a fresh operation ID.
    exec(txn,
        "INSERT INTO owned_retainers(discord_id,retainer_id,weight)
         SELECT $1,p.public_retainer_id,COALESCE((SELECT max(weight) FROM owned_retainers WHERE discord_id=$1),0)+row_number() OVER(ORDER BY p.id)::integer
         FROM plugin_retainer p WHERE p.owner=$1 AND p.registered_character=$2 AND p.public_retainer_id IS NOT NULL
         AND NOT EXISTS(SELECT 1 FROM owned_retainers o WHERE o.discord_id=$1 AND o.retainer_id=p.public_retainer_id)
         ON CONFLICT(discord_id,retainer_id) DO NOTHING",
        vec![owner.into(),character.into()]).await?;
    Ok(())
}

async fn receipt(
    txn: &DatabaseTransaction,
    owner: i64,
    kind: &str,
    id: &str,
    hash: &[u8],
) -> Result<Option<Json>> {
    let rows = query(
        txn,
        "SELECT body_hash,response FROM plugin_operation WHERE owner=$1 AND kind=$2 AND id=$3",
        vec![owner.into(), kind.into(), id.into()],
    )
    .await?;
    let Some(row) = rows.first() else {
        return Ok(None);
    };
    if !equal(&row.try_get::<Vec<u8>>("", "body_hash")?, hash) {
        return Err(PluginError::Conflict("operation_id_reused"));
    }
    Ok(Some(row.try_get("", "response")?))
}

impl UltrosDb {
    /// Website-cookie callers use their authenticated account ID. This contains
    /// only explicitly registered entries, never raw game IDs or draft rosters.
    pub async fn registered_game_characters(&self, owner: i64) -> Result<Vec<UnverifiedCharacter>> {
        let txn = self.db.begin().await?;
        let mut result = roster(&txn, owner).await?;
        result.retain(|character| character.registered);
        for character in &mut result {
            character.retainers.retain(|retainer| retainer.registered);
        }
        txn.commit().await?;
        Ok(result)
    }
    pub async fn plugin_characters(
        &self,
        principal: &PluginPrincipal,
    ) -> Result<Vec<UnverifiedCharacter>> {
        let txn = self.db.begin().await?;
        authorize(&txn, principal, Some("retainers:read")).await?;
        let result = roster(&txn, principal.owner).await?;
        txn.commit().await?;
        Ok(result)
    }
    pub async fn observe_plugin_character(
        &self,
        principal: &PluginPrincipal,
        keys: &PluginKeys,
        value: &ObserveCharacter,
    ) -> Result<Vec<UnverifiedCharacter>> {
        let txn = self.db.begin().await?;
        authorize(&txn, principal, Some("retainers:register")).await?;
        observe(&txn, keys, principal.owner, value, true).await?;
        let result = roster(&txn, principal.owner).await?;
        txn.commit().await?;
        Ok(result)
    }
    pub async fn register_plugin_retainers(
        &self,
        principal: &PluginPrincipal,
        keys: &PluginKeys,
        request: &RegisterRetainers,
    ) -> Result<Vec<UnverifiedCharacter>> {
        if !operation_id(&request.operation_id) {
            return Err(PluginError::Invalid("invalid_operation_id"));
        }
        validate_observation(&request.character)?;
        let hash = digest(
            &serde_json::to_vec(request).map_err(|_| PluginError::Invalid("invalid_roster"))?,
        );
        let txn = self.db.begin().await?;
        authorize(&txn, principal, Some("retainers:register")).await?;
        if let Some(response) = receipt(
            &txn,
            principal.owner,
            "register",
            &request.operation_id,
            &hash,
        )
        .await?
        {
            return serde_json::from_value(response).map_err(|_| PluginError::Storage);
        }
        let character = observe(&txn, keys, principal.owner, &request.character, true).await?;
        exec(
            &txn,
            "UPDATE plugin_character SET registered=true WHERE owner=$1 AND id=$2",
            vec![principal.owner.into(), character.into()],
        )
        .await?;
        // Preserve every existing assignment; another character is a visible conflict.
        exec(&txn,"UPDATE plugin_retainer SET registered_character=$1 WHERE owner=$2 AND observed_character=$1 AND registered_character IS NULL",vec![character.into(),principal.owner.into()]).await?;
        link_public_retainers(&txn, principal.owner, character).await?;
        let result = roster(&txn, principal.owner).await?;
        exec(&txn,"INSERT INTO plugin_operation(owner,kind,id,body_hash,response) VALUES($1,'register',$2,$3,$4)",vec![principal.owner.into(),request.operation_id.clone().into(),hash.into(),json!(result).into()]).await?;
        txn.commit().await?;
        Ok(result)
    }
    pub async fn set_plugin_upload_consent(
        &self,
        principal: &PluginPrincipal,
        character: i64,
        request: &UploadConsent,
    ) -> Result<Vec<UnverifiedCharacter>> {
        let txn = self.db.begin().await?;
        authorize(&txn, principal, Some("retainers:upload")).await?;
        let current=one(&txn,"SELECT upload_enabled,registered FROM plugin_character WHERE owner=$1 AND id=$2 FOR UPDATE",vec![principal.owner.into(),character.into()]).await?;
        if !current.try_get::<bool>("", "registered")? {
            return Err(PluginError::Conflict("registration_required"));
        }
        if current.try_get::<bool>("", "upload_enabled")? != request.enabled
            || request.delete_snapshots
        {
            exec(&txn,"UPDATE plugin_character SET upload_enabled=$1,capture_epoch=capture_epoch+1 WHERE owner=$2 AND id=$3",vec![request.enabled.into(),principal.owner.into(),character.into()]).await?;
        }
        if request.delete_snapshots {
            exec(&txn,"DELETE FROM plugin_snapshot WHERE owner=$1 AND retainer_id IN (SELECT id FROM plugin_retainer WHERE owner=$1 AND registered_character=$2)",vec![principal.owner.into(),character.into()]).await?;
            exec(&txn,"DELETE FROM plugin_operation WHERE owner=$1 AND kind IN (SELECT 'capture/'||id::text FROM plugin_retainer WHERE owner=$1 AND registered_character=$2)",vec![principal.owner.into(),character.into()]).await?;
        }
        let result = roster(&txn, principal.owner).await?;
        txn.commit().await?;
        Ok(result)
    }
    pub async fn upload_plugin_listings(
        &self,
        principal: &PluginPrincipal,
        retainer: i64,
        capture: &ListingCapture,
    ) -> Result<CaptureReceipt> {
        if !operation_id(&capture.capture_id)
            || !capture.complete
            || capture.listings.len() > 20
            || capture.observed_at > Utc::now() + chrono::Duration::minutes(2)
            || capture.observed_at < Utc::now() - chrono::Duration::minutes(10)
        {
            return Err(PluginError::Invalid("incomplete_or_expired_capture"));
        }
        for (i, item) in capture.listings.iter().enumerate() {
            if item.slot >= 20
                || item.item_id <= 0
                || !(1..=999).contains(&item.quantity)
                || !(1..=999_999_999).contains(&item.unit_price)
                || capture.listings[..i]
                    .iter()
                    .any(|other| other.slot == item.slot)
            {
                return Err(PluginError::Invalid("invalid_listing"));
            }
        }
        let hash = digest(
            &serde_json::to_vec(capture).map_err(|_| PluginError::Invalid("invalid_capture"))?,
        );
        let kind = format!("capture/{retainer}");
        let txn = self.db.begin().await?;
        authorize(&txn, principal, Some("retainers:upload")).await?;
        // Dedup precedes revision/epoch checks: a response lost after commit is safe to retry.
        if let Some(response) =
            receipt(&txn, principal.owner, &kind, &capture.capture_id, &hash).await?
        {
            return serde_json::from_value(response).map_err(|_| PluginError::Storage);
        }
        let row=one(&txn,"SELECT r.revision,c.upload_enabled,c.capture_epoch FROM plugin_retainer r JOIN plugin_character c ON c.id=r.registered_character AND c.owner=r.owner WHERE r.id=$1 AND r.owner=$2 AND c.registered FOR UPDATE OF r,c",vec![retainer.into(),principal.owner.into()]).await?;
        if !row.try_get::<bool>("", "upload_enabled")?
            || row.try_get::<i64>("", "capture_epoch")? != capture.capture_epoch
        {
            return Err(PluginError::Conflict("upload_consent_changed"));
        }
        let revision = row.try_get::<i64>("", "revision")?;
        if revision != capture.base_revision {
            return Err(PluginError::Conflict("revisit_retainer_required"));
        }
        let revision = revision.checked_add(1).ok_or(PluginError::Storage)?;
        let response = CaptureReceipt {
            revision,
            hash: hash.iter().map(|b| format!("{b:02x}")).collect(),
        };
        exec(&txn,"INSERT INTO plugin_snapshot(owner,retainer_id,revision,capture_id,body_hash,listings,observed_at) VALUES($1,$2,$3,$4,$5,$6,$7)",vec![principal.owner.into(),retainer.into(),revision.into(),capture.capture_id.clone().into(),hash.clone().into(),json!(capture.listings).into(),capture.observed_at.into()]).await?;
        exec(
            &txn,
            "UPDATE plugin_retainer SET revision=$1 WHERE id=$2 AND owner=$3",
            vec![revision.into(), retainer.into(), principal.owner.into()],
        )
        .await?;
        exec(
            &txn,
            "INSERT INTO plugin_operation(owner,kind,id,body_hash,response) VALUES($1,$2,$3,$4,$5)",
            vec![
                principal.owner.into(),
                kind.into(),
                capture.capture_id.clone().into(),
                hash.into(),
                json!(response).into(),
            ],
        )
        .await?;
        exec(&txn,"DELETE FROM plugin_snapshot WHERE id IN (SELECT id FROM plugin_snapshot WHERE owner=$1 AND retainer_id=$2 ORDER BY revision DESC OFFSET 100)",vec![principal.owner.into(),retainer.into()]).await?;
        txn.commit().await?;
        Ok(response)
    }
    pub async fn plugin_retainer_snapshots(
        &self,
        principal: &PluginPrincipal,
    ) -> Result<Vec<RetainerSnapshot>> {
        let txn = self.db.begin().await?;
        authorize(&txn, principal, Some("retainers:read")).await?;
        let rows=query(&txn,"SELECT DISTINCT ON (retainer_id) * FROM plugin_snapshot WHERE owner=$1 ORDER BY retainer_id,revision DESC",vec![principal.owner.into()]).await?;
        let result = rows
            .iter()
            .map(|r| {
                Ok(RetainerSnapshot {
                    retainer_id: r.try_get::<i64>("", "retainer_id")?.to_string(),
                    revision: r.try_get("", "revision")?,
                    received_at: r.try_get("", "received_at")?,
                    observed_at: r.try_get("", "observed_at")?,
                    listings: serde_json::from_value(r.try_get::<Json>("", "listings")?)
                        .map_err(|_| PluginError::Storage)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        txn.commit().await?;
        Ok(result)
    }
}
