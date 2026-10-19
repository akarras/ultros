use super::crypto::{TokenProof, digest, equal, human_code, random};
use super::{PluginError, PluginKeys, Result, bounded_text, exec, lock_account, one, query};
use crate::UltrosDb;
use chrono::{DateTime, Utc};
use sea_orm::{DatabaseTransaction, QueryResult, TransactionTrait};
use serde_json::{Value as Json, json};
use ultros_api_types::plugin::{CreatePairing, DeviceSession, TokenPair, valid_scopes};

#[derive(Clone)]
pub struct PluginPrincipal {
    pub session: DeviceSession,
    pub(crate) owner: i64,
    proof: TokenProof,
}
impl std::fmt::Debug for PluginPrincipal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginPrincipal")
            .field("device", &self.session.id)
            .finish_non_exhaustive()
    }
}

pub struct ApprovalSelection {
    pub label: String,
    pub code: String,
    pub scopes: Vec<String>,
    pub nonce: String,
    pub csrf: String,
}

enum PairingInput<'a> {
    Code(&'a str),
    Link(&'a str),
}

impl PairingInput<'_> {
    fn invalid(&self) -> PluginError {
        PluginError::Invalid(match self {
            Self::Code(_) => "invalid_or_expired_code",
            Self::Link(_) => "invalid_or_expired_link",
        })
    }
}

fn scopes(row: &QueryResult, column: &str) -> Result<Vec<String>> {
    serde_json::from_value(row.try_get::<Json>("", column)?).map_err(|_| PluginError::Storage)
}
fn session(row: &QueryResult) -> Result<DeviceSession> {
    Ok(DeviceSession {
        id: row.try_get("", "id")?,
        label: row.try_get("", "label")?,
        account_id: row.try_get::<i64>("", "owner")?.to_string(),
        account_name: row.try_get("", "username")?,
        scopes: scopes(row, "scopes")?,
        created_at: row.try_get("", "created_at")?,
        last_seen_at: row.try_get("", "last_seen_at")?,
        expires_at: row.try_get("", "expires_at")?,
        endpoint_id: row.try_get("", "endpoint_id")?,
    })
}
const SESSION_QUERY: &str = "SELECT g.*,u.username,t.secret_hash FROM plugin_token t JOIN plugin_grant g ON g.id=t.grant_id JOIN discord_user u ON u.id=g.owner WHERE t.id=$1 AND t.kind='access' AND t.used_at IS NULL AND t.expires_at>now() AND g.revoked_at IS NULL AND g.expires_at>now() AND g.refresh_expires_at>now() AND (g.used_at IS NOT NULL OR g.created_at>now()-interval '10 minutes')";

pub(crate) async fn authorize(
    txn: &DatabaseTransaction,
    principal: &PluginPrincipal,
    scope: Option<&str>,
) -> Result<DeviceSession> {
    lock_account(txn, principal.owner).await?;
    let row = one(txn, SESSION_QUERY, vec![principal.proof.id.clone().into()]).await?;
    if !equal(
        &row.try_get::<Vec<u8>>("", "secret_hash")?,
        &principal.proof.hash,
    ) {
        return Err(PluginError::Unauthorized);
    }
    let current = session(&row)?;
    if current.id != principal.session.id || current.account_id != principal.owner.to_string() {
        return Err(PluginError::Unauthorized);
    }
    if scope.is_some_and(|required| !current.scopes.iter().any(|s| s == required)) {
        return Err(PluginError::Forbidden);
    }
    exec(
        txn,
        "UPDATE plugin_grant SET used_at=coalesce(used_at,now()),last_seen_at=now() WHERE id=$1",
        vec![current.id.clone().into()],
    )
    .await?;
    Ok(current)
}

async fn issue(txn: &DatabaseTransaction, device: &str) -> Result<TokenPair> {
    let (access_token, access) = TokenProof::mint(false)?;
    let (refresh_token, refresh) = TokenProof::mint(true)?;
    exec(txn,"INSERT INTO plugin_token(id,grant_id,kind,secret_hash,expires_at) SELECT $1,id,'access',$2,least(expires_at,now()+interval '15 minutes') FROM plugin_grant WHERE id=$3",vec![access.id.into(),access.hash.into(),device.into()]).await?;
    exec(txn,"INSERT INTO plugin_token(id,grant_id,kind,secret_hash,expires_at) SELECT $1,id,'refresh',$2,least(expires_at,now()+interval '30 days') FROM plugin_grant WHERE id=$3",vec![refresh.id.into(),refresh.hash.into(),device.into()]).await?;
    let row = one(txn,"SELECT g.*,u.username FROM plugin_grant g JOIN discord_user u ON u.id=g.owner WHERE g.id=$1",vec![device.into()]).await?;
    let current = session(&row)?;
    Ok(TokenPair {
        access_token,
        refresh_token,
        token_type: "Bearer".into(),
        expires_in: 900,
        device_id: current.id,
        account_id: current.account_id,
        account_name: current.account_name,
        scopes: current.scopes,
    })
}

pub(super) async fn revoke(
    txn: &DatabaseTransaction,
    device: &str,
    owner: i64,
    kind: &str,
) -> Result<()> {
    exec(
        txn,
        "UPDATE plugin_grant SET revoked_at=coalesce(revoked_at,now()) WHERE id=$1 AND owner=$2",
        vec![device.into(), owner.into()],
    )
    .await?;
    exec(txn,"UPDATE plugin_delivery SET cancelled_at=now() WHERE device_id=$1 AND acknowledged_at IS NULL AND cancelled_at IS NULL",vec![device.into()]).await?;
    exec(txn,"UPDATE notification_endpoint SET disabled_at=now(),last_error='Game client disconnected' WHERE id=(SELECT endpoint_id FROM plugin_grant WHERE id=$1 AND owner=$2)",vec![device.into(),owner.into()]).await?;
    exec(
        txn,
        "INSERT INTO plugin_security_event(owner,grant_id,kind) VALUES($1,$2,$3)",
        vec![owner.into(), device.into(), kind.into()],
    )
    .await?;
    Ok(())
}

impl UltrosDb {
    /// Shared Postgres counters deliberately ignore untrusted forwarding headers.
    pub async fn plugin_rate_limit(&self, key: &str, limit: i32, seconds: i32) -> Result<()> {
        let row = one(&self.db,"INSERT INTO plugin_rate_limit(key) VALUES($1) ON CONFLICT(key) DO UPDATE SET count=CASE WHEN plugin_rate_limit.window_at+make_interval(secs=>$2)>now() THEN plugin_rate_limit.count+1 ELSE 1 END,window_at=CASE WHEN plugin_rate_limit.window_at+make_interval(secs=>$2)>now() THEN plugin_rate_limit.window_at ELSE now() END RETURNING count",vec![key.into(),seconds.into()]).await?;
        if row.try_get::<i32>("", "count")? > limit {
            return Err(PluginError::RateLimited);
        }
        Ok(())
    }

    pub async fn authenticate_plugin(&self, token: &str) -> Result<PluginPrincipal> {
        let proof = TokenProof::parse(token)?;
        if proof.refresh {
            return Err(PluginError::Unauthorized);
        }
        let row = one(&self.db, SESSION_QUERY, vec![proof.id.clone().into()]).await?;
        if !equal(&row.try_get::<Vec<u8>>("", "secret_hash")?, &proof.hash) {
            return Err(PluginError::Unauthorized);
        }
        Ok(PluginPrincipal {
            owner: row.try_get("", "owner")?,
            session: session(&row)?,
            proof,
        })
    }

    pub async fn plugin_session(&self, principal: &PluginPrincipal) -> Result<DeviceSession> {
        let txn = self.db.begin().await?;
        let current = authorize(&txn, principal, None).await?;
        txn.commit().await?;
        Ok(current)
    }

    pub async fn create_plugin_pairing(
        &self,
        keys: &PluginKeys,
        request: &CreatePairing,
        ip: &str,
        replacement: Option<&PluginPrincipal>,
    ) -> Result<(String, String, String)> {
        if !bounded_text(&request.label, 64) || !valid_scopes(&request.scopes) {
            return Err(PluginError::Invalid("invalid_pairing_request"));
        }
        let ip_hash = keys.ip_hash(ip);
        self.plugin_rate_limit(&format!("pair:{ip_hash:x?}"), 5, 600)
            .await?;
        let txn = self.db.begin().await?;
        let replacement = match replacement {
            Some(p) => Some(authorize(&txn, p, None).await?),
            None => None,
        };
        // This row also serializes the live-IP cap across replicas.
        one(
            &txn,
            "SELECT key FROM plugin_rate_limit WHERE key=$1 FOR UPDATE",
            vec![format!("pair:{ip_hash:x?}").into()],
        )
        .await?;
        exec(
            &txn,
            "DELETE FROM plugin_pairing WHERE expires_at<now()",
            vec![],
        )
        .await?;
        let count=one(&txn,"SELECT count(*)::bigint AS n FROM plugin_pairing WHERE ip_hash=$1 AND expires_at>now() AND state IN ('pending','approved')",vec![ip_hash.clone().into()]).await?;
        if count.try_get::<i64>("", "n")? >= 5 {
            return Err(PluginError::RateLimited);
        }
        for _ in 0..4 {
            let verification = random(32)?;
            let code = human_code(&verification);
            let device = random(32)?;
            let id = random(16)?;
            let inserted=query(&txn,"INSERT INTO plugin_pairing(id,device_hash,human_hash,label,requested_scopes,ip_hash,replacement_id,verification_hash) VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT DO NOTHING RETURNING id",vec![id.into(),digest(device.as_bytes()).into(),keys.human_hash(&code)?.into(),request.label.clone().into(),json!(request.scopes).into(),ip_hash.clone().into(),replacement.as_ref().map(|r| r.id.clone()).into(),keys.verification_hash(&verification)?.into()]).await?;
            if !inserted.is_empty() {
                txn.commit().await?;
                return Ok((
                    device,
                    format!("{}-{}", &code[..4], &code[4..]),
                    verification,
                ));
            }
        }
        Err(PluginError::Storage)
    }

    pub async fn create_plugin_approval(&self, owner: i64) -> Result<(String, String)> {
        self.plugin_rate_limit(&format!("approval-page:{owner}"), 20, 600)
            .await?;
        let cookie = random(32)?;
        let csrf = random(32)?;
        exec(
            &self.db,
            "INSERT INTO plugin_approval(id,owner,csrf) VALUES($1,$2,$3)",
            vec![
                digest(cookie.as_bytes()).into(),
                owner.into(),
                digest(csrf.as_bytes()).into(),
            ],
        )
        .await?;
        Ok((cookie, csrf))
    }

    pub async fn select_plugin_pairing(
        &self,
        keys: &PluginKeys,
        owner: i64,
        cookie: &str,
        csrf: &str,
        code: &str,
        ip: &str,
    ) -> Result<ApprovalSelection> {
        self.select_plugin_pairing_input(keys, owner, cookie, csrf, PairingInput::Code(code), ip)
            .await
    }

    pub async fn validate_plugin_pairing_link(
        &self,
        keys: &PluginKeys,
        token: &str,
        ip: &str,
    ) -> Result<()> {
        self.plugin_rate_limit(&format!("pair-link:{:x?}", keys.ip_hash(ip)), 20, 600)
            .await?;
        let rows = query(&self.db, "SELECT id FROM plugin_pairing WHERE verification_hash=$1 AND expires_at>now() AND state='pending'", vec![keys.verification_hash(token)?.into()]).await?;
        if rows.is_empty() {
            return Err(PluginError::Invalid("invalid_or_expired_link"));
        }
        Ok(())
    }

    pub async fn select_plugin_pairing_link(
        &self,
        keys: &PluginKeys,
        owner: i64,
        cookie: &str,
        csrf: &str,
        token: &str,
        ip: &str,
    ) -> Result<ApprovalSelection> {
        self.select_plugin_pairing_input(keys, owner, cookie, csrf, PairingInput::Link(token), ip)
            .await
    }

    async fn select_plugin_pairing_input(
        &self,
        keys: &PluginKeys,
        owner: i64,
        cookie: &str,
        csrf: &str,
        input: PairingInput<'_>,
        ip: &str,
    ) -> Result<ApprovalSelection> {
        self.plugin_rate_limit(&format!("approval:{owner}"), 20, 600)
            .await?;
        let txn = self.db.begin().await?;
        lock_account(&txn, owner).await?;
        let approval=one(&txn,"SELECT csrf FROM plugin_approval WHERE id=$1 AND owner=$2 AND expires_at>now() FOR UPDATE",vec![digest(cookie.as_bytes()).into(),owner.into()]).await?;
        if !equal(
            &approval.try_get::<Vec<u8>>("", "csrf")?,
            &digest(csrf.as_bytes()),
        ) {
            return Err(PluginError::Denied);
        }
        let failure_key = format!("failed-code:{owner}:{:x?}", keys.ip_hash(ip));
        if !query(&txn,"SELECT key FROM plugin_rate_limit WHERE key=$1 AND count>=5 AND window_at>now()-interval '10 minutes'",vec![failure_key.clone().into()]).await?.is_empty(){return Err(PluginError::RateLimited);}
        let lookup = match &input {
            PairingInput::Code(code) => keys.human_hash(code).map(|hash| ("SELECT p.* FROM plugin_pairing p WHERE human_hash=$1 AND expires_at>now() AND state='pending' FOR UPDATE", hash, super::normalize_human_code(code).unwrap_or_default())),
            PairingInput::Link(token) => keys.verification_hash(token).map(|hash| ("SELECT p.* FROM plugin_pairing p WHERE verification_hash=$1 AND expires_at>now() AND state='pending' FOR UPDATE", hash, human_code(token))),
        };
        let (sql, hash, code) = match lookup {
            Ok(lookup) => lookup,
            Err(_) => {
                txn.rollback().await?;
                self.plugin_rate_limit(&failure_key, 5, 600).await?;
                return Err(input.invalid());
            }
        };
        let rows = query(&txn, sql, vec![hash.into()]).await?;
        let Some(pair) = rows.first() else {
            txn.rollback().await?;
            self.plugin_rate_limit(
                &format!("failed-code:{owner}:{:x?}", keys.ip_hash(ip)),
                5,
                600,
            )
            .await?;
            return Err(input.invalid());
        };
        if let Some(old) = pair.try_get::<Option<String>>("", "replacement_id")? {
            one(&txn,"SELECT id FROM plugin_grant WHERE id=$1 AND owner=$2 AND revoked_at IS NULL AND expires_at>now() AND refresh_expires_at>now() AND (used_at IS NOT NULL OR created_at>now()-interval '10 minutes')",vec![old.into(),owner.into()]).await?;
        }
        let nonce = random(32)?;
        let next_csrf = random(32)?;
        exec(
            &txn,
            "UPDATE plugin_approval SET csrf=$1,nonce=$2,pairing_id=$3,failures=0 WHERE id=$4",
            vec![
                digest(next_csrf.as_bytes()).into(),
                digest(nonce.as_bytes()).into(),
                pair.try_get::<String>("", "id")?.into(),
                digest(cookie.as_bytes()).into(),
            ],
        )
        .await?;
        let result = ApprovalSelection {
            label: pair.try_get("", "label")?,
            scopes: scopes(pair, "requested_scopes")?,
            code,
            nonce,
            csrf: next_csrf,
        };
        txn.commit().await?;
        Ok(result)
    }

    pub async fn approve_plugin_pairing(
        &self,
        owner: i64,
        cookie: &str,
        csrf: &str,
        nonce: &str,
        approved: &[String],
        deny: bool,
    ) -> Result<()> {
        self.plugin_rate_limit(&format!("approval:{owner}"), 20, 600)
            .await?;
        let txn = self.db.begin().await?;
        lock_account(&txn, owner).await?;
        let approval=one(&txn,"SELECT * FROM plugin_approval WHERE id=$1 AND owner=$2 AND expires_at>now() FOR UPDATE",vec![digest(cookie.as_bytes()).into(),owner.into()]).await?;
        if !equal(
            &approval.try_get::<Vec<u8>>("", "csrf")?,
            &digest(csrf.as_bytes()),
        ) || approval
            .try_get::<Option<Vec<u8>>>("", "nonce")?
            .is_none_or(|n| !equal(&n, &digest(nonce.as_bytes())))
        {
            exec(&txn,"UPDATE plugin_approval SET failures=failures+1,expires_at=CASE WHEN failures>=4 THEN now() ELSE expires_at END WHERE id=$1",vec![digest(cookie.as_bytes()).into()]).await?;
            txn.commit().await?;
            return Err(PluginError::Denied);
        }
        let id = approval
            .try_get::<Option<String>>("", "pairing_id")?
            .ok_or(PluginError::Denied)?;
        let pair=one(&txn,"SELECT * FROM plugin_pairing WHERE id=$1 AND expires_at>now() AND state='pending' FOR UPDATE",vec![id.clone().into()]).await?;
        if let Some(old) = pair.try_get::<Option<String>>("", "replacement_id")? {
            one(&txn,"SELECT id FROM plugin_grant WHERE id=$1 AND owner=$2 AND revoked_at IS NULL AND expires_at>now() AND refresh_expires_at>now() AND (used_at IS NOT NULL OR created_at>now()-interval '10 minutes')",vec![old.into(),owner.into()]).await?;
        }
        let requested = scopes(&pair, "requested_scopes")?;
        if !deny && (!valid_scopes(approved) || approved.iter().any(|s| !requested.contains(s))) {
            return Err(PluginError::Invalid("invalid_approved_scopes"));
        }
        exec(
            &txn,
            "UPDATE plugin_pairing SET state=$1,owner=$2,approved_scopes=$3 WHERE id=$4",
            vec![
                (if deny { "denied" } else { "approved" }).into(),
                owner.into(),
                json!(approved).into(),
                id.into(),
            ],
        )
        .await?;
        exec(
            &txn,
            "DELETE FROM plugin_approval WHERE id=$1",
            vec![digest(cookie.as_bytes()).into()],
        )
        .await?;
        exec(
            &txn,
            "INSERT INTO plugin_security_event(owner,kind) VALUES($1,$2)",
            vec![
                owner.into(),
                (if deny {
                    "pairing_denied"
                } else {
                    "pairing_approved"
                })
                .into(),
            ],
        )
        .await?;
        txn.commit().await?;
        Ok(())
    }

    pub async fn poll_plugin_pairing(&self, device_code: &str) -> Result<TokenPair> {
        if device_code.len() != 43 {
            return Err(PluginError::Expired);
        }
        let hash = digest(device_code.as_bytes());
        let owner = one(
            &self.db,
            "SELECT owner FROM plugin_pairing WHERE device_hash=$1",
            vec![hash.clone().into()],
        )
        .await?
        .try_get::<Option<i64>>("", "owner")?;
        let txn = self.db.begin().await?;
        if let Some(owner) = owner {
            lock_account(&txn, owner).await?;
        }
        let pair=one(&txn,"SELECT *,expires_at>now() AS active,next_poll_at>now() AS early FROM plugin_pairing WHERE device_hash=$1 FOR UPDATE",vec![hash.into()]).await?;
        if pair.try_get::<Option<i64>>("", "owner")? != owner {
            return Err(PluginError::Pending);
        }
        if !pair.try_get::<bool>("", "active")? {
            return Err(PluginError::Expired);
        }
        let id = pair.try_get::<String>("", "id")?;
        let state = pair.try_get::<String>("", "state")?;
        if state == "denied" {
            return Err(PluginError::Denied);
        }
        if state == "consumed" {
            return Err(PluginError::Expired);
        }
        let interval = pair.try_get::<i32>("", "poll_interval")?;
        if pair.try_get::<bool>("", "early")? {
            let interval = (interval + 5).min(60);
            exec(&txn,"UPDATE plugin_pairing SET poll_interval=$1,next_poll_at=now()+make_interval(secs=>$1) WHERE id=$2",vec![interval.into(),id.into()]).await?;
            txn.commit().await?;
            return Err(PluginError::SlowDown(interval as u32));
        }
        exec(
            &txn,
            "UPDATE plugin_pairing SET next_poll_at=now()+make_interval(secs=>$1) WHERE id=$2",
            vec![interval.into(), id.clone().into()],
        )
        .await?;
        if state == "pending" {
            txn.commit().await?;
            return Err(PluginError::Pending);
        }
        let owner = owner.ok_or(PluginError::Denied)?;
        let approved = scopes(&pair, "approved_scopes")?;
        if !valid_scopes(&approved) {
            return Err(PluginError::Storage);
        }
        let old = pair.try_get::<Option<String>>("", "replacement_id")?;
        let count=one(&txn,"SELECT count(*)::bigint AS n FROM plugin_grant WHERE owner=$1 AND revoked_at IS NULL AND expires_at>now() AND refresh_expires_at>now() AND (used_at IS NOT NULL OR created_at>now()-interval '10 minutes')",vec![owner.into()]).await?.try_get::<i64>("","n")?;
        if count >= 10 && old.is_none() {
            return Err(PluginError::Conflict("device_limit_reached"));
        }
        let device = random(16)?;
        exec(
            &txn,
            "INSERT INTO plugin_grant(id,owner,label,scopes) VALUES($1,$2,$3,$4)",
            vec![
                device.clone().into(),
                owner.into(),
                pair.try_get::<String>("", "label")?.into(),
                json!(approved).into(),
            ],
        )
        .await?;
        let mut endpoint = None;
        if let Some(old) = old {
            let old_row=one(&txn,"SELECT endpoint_id FROM plugin_grant WHERE id=$1 AND owner=$2 AND revoked_at IS NULL AND expires_at>now() AND refresh_expires_at>now() AND (used_at IS NOT NULL OR created_at>now()-interval '10 minutes') FOR UPDATE",vec![old.clone().into(),owner.into()]).await?;
            endpoint = old_row.try_get::<Option<i32>>("", "endpoint_id")?;
            revoke(&txn, &old, owner, "reauthorized").await?;
            if approved.iter().any(|s| s == "notifications:read")
                && let Some(endpoint) = endpoint
            {
                exec(&txn,"UPDATE notification_endpoint SET config=jsonb_build_object('device_id',$1::text),disabled_at=NULL,last_error=NULL WHERE id=$2",vec![device.clone().into(),endpoint.into()]).await?;
                exec(&txn,"UPDATE plugin_delivery SET device_id=$1,cancelled_at=NULL WHERE device_id=$2 AND acknowledged_at IS NULL AND expires_at>now()",vec![device.clone().into(),old.into()]).await?;
            } else {
                endpoint = None;
            }
        }
        if approved.iter().any(|s| s == "notifications:read") && endpoint.is_none() {
            endpoint=Some(one(&txn,"INSERT INTO notification_endpoint(user_id,name,method,config) VALUES($1,$2,'InGame',jsonb_build_object('device_id',$3::text)) RETURNING id",vec![owner.into(),pair.try_get::<String>("","label")?.into(),device.clone().into()]).await?.try_get::<i32>("","id")?);
        }
        exec(
            &txn,
            "UPDATE plugin_grant SET endpoint_id=$1 WHERE id=$2",
            vec![endpoint.into(), device.clone().into()],
        )
        .await?;
        exec(
            &txn,
            "UPDATE plugin_pairing SET state='consumed' WHERE id=$1",
            vec![id.into()],
        )
        .await?;
        exec(
            &txn,
            "INSERT INTO plugin_security_event(owner,grant_id,kind) VALUES($1,$2,'connected')",
            vec![owner.into(), device.clone().into()],
        )
        .await?;
        let result = issue(&txn, &device).await?;
        txn.commit().await?;
        Ok(result)
    }

    pub async fn refresh_plugin_session(&self, token: &str) -> Result<TokenPair> {
        let proof = TokenProof::parse(token)?;
        if !proof.refresh {
            return Err(PluginError::Unauthorized);
        }
        let owner=one(&self.db,"SELECT g.owner FROM plugin_token t JOIN plugin_grant g ON g.id=t.grant_id WHERE t.id=$1 AND t.kind='refresh'",vec![proof.id.clone().into()]).await?.try_get::<i64>("","owner")?;
        let txn = self.db.begin().await?;
        lock_account(&txn, owner).await?;
        let row=one(&txn,"SELECT t.*,g.revoked_at,g.expires_at>now() AND g.refresh_expires_at>now() AND t.expires_at>now() AND (g.used_at IS NOT NULL OR g.created_at>now()-interval '10 minutes') AS active FROM plugin_token t JOIN plugin_grant g ON g.id=t.grant_id WHERE t.id=$1 AND t.kind='refresh' FOR UPDATE OF t,g",vec![proof.id.into()]).await?;
        if !equal(&row.try_get::<Vec<u8>>("", "secret_hash")?, &proof.hash) {
            return Err(PluginError::Unauthorized);
        }
        let device = row.try_get::<String>("", "grant_id")?;
        if row
            .try_get::<Option<DateTime<Utc>>>("", "used_at")?
            .is_some()
        {
            revoke(&txn, &device, owner, "refresh_reuse").await?;
            txn.commit().await?;
            return Err(PluginError::Unauthorized);
        }
        if row
            .try_get::<Option<DateTime<Utc>>>("", "revoked_at")?
            .is_some()
            || !row.try_get::<bool>("", "active")?
        {
            return Err(PluginError::Unauthorized);
        }
        exec(
            &txn,
            "UPDATE plugin_token SET used_at=now() WHERE id=$1",
            vec![row.try_get::<String>("", "id")?.into()],
        )
        .await?;
        exec(&txn,"UPDATE plugin_grant SET used_at=coalesce(used_at,now()),last_seen_at=now(),refresh_expires_at=least(expires_at,now()+interval '30 days') WHERE id=$1",vec![device.clone().into()]).await?;
        let result = issue(&txn, &device).await?;
        txn.commit().await?;
        Ok(result)
    }

    /// A known refresh generation may only revoke its own family, even after use.
    pub async fn revoke_plugin_credential(&self, token: &str) -> Result<()> {
        let proof = TokenProof::parse(token)?;
        let txn = self.db.begin().await?;
        let row=one(&txn,"SELECT t.secret_hash,t.kind,g.owner,g.id FROM plugin_token t JOIN plugin_grant g ON g.id=t.grant_id WHERE t.id=$1",vec![proof.id.into()]).await?;
        if !equal(&row.try_get::<Vec<u8>>("", "secret_hash")?, &proof.hash)
            || (row.try_get::<String>("", "kind")? == "refresh") != proof.refresh
        {
            return Err(PluginError::Unauthorized);
        }
        let owner = row.try_get("", "owner")?;
        lock_account(&txn, owner).await?;
        revoke(
            &txn,
            &row.try_get::<String>("", "id")?,
            owner,
            "disconnected",
        )
        .await?;
        txn.commit().await?;
        Ok(())
    }

    pub async fn plugin_devices_for_browser(&self, owner: i64) -> Result<Vec<DeviceSession>> {
        query(&self.db,"SELECT g.*,u.username FROM plugin_grant g JOIN discord_user u ON u.id=g.owner WHERE g.owner=$1 AND g.revoked_at IS NULL AND g.expires_at>now() AND g.refresh_expires_at>now() AND (g.used_at IS NOT NULL OR g.created_at>now()-interval '10 minutes') ORDER BY g.created_at DESC",vec![owner.into()]).await?.iter().map(session).collect()
    }
    pub async fn revoke_plugin_device_for_browser(
        &self,
        owner: i64,
        device: &str,
        cookie: &str,
        csrf: &str,
    ) -> Result<()> {
        let txn = self.db.begin().await?;
        lock_account(&txn, owner).await?;
        let row=one(&txn,"DELETE FROM plugin_approval WHERE id=$1 AND owner=$2 AND expires_at>now() RETURNING csrf",vec![digest(cookie.as_bytes()).into(),owner.into()]).await?;
        if !equal(
            &row.try_get::<Vec<u8>>("", "csrf")?,
            &digest(csrf.as_bytes()),
        ) {
            return Err(PluginError::Denied);
        }
        one(
            &txn,
            "SELECT id FROM plugin_grant WHERE id=$1 AND owner=$2",
            vec![device.into(), owner.into()],
        )
        .await?;
        revoke(&txn, device, owner, "browser_revoked").await?;
        txn.commit().await?;
        Ok(())
    }

    pub async fn cleanup_plugin_data(&self) -> Result<()> {
        for sql in [
            "DELETE FROM plugin_approval WHERE expires_at<now()",
            "DELETE FROM plugin_pairing WHERE expires_at<now()",
            "DELETE FROM plugin_rate_limit WHERE window_at<now()-interval '1 day'",
            "DELETE FROM plugin_operation WHERE created_at<now()-interval '30 days'",
            "DELETE FROM plugin_snapshot WHERE received_at<now()-interval '30 days'",
            "DELETE FROM plugin_delivery WHERE coalesce(acknowledged_at,cancelled_at,expires_at)<now()-interval '7 days'",
            "DELETE FROM plugin_security_event WHERE created_at<now()-interval '90 days'",
            "DELETE FROM plugin_grant WHERE expires_at<now()-interval '7 days' OR revoked_at<now()-interval '7 days'",
        ] {
            exec(&self.db, sql, vec![]).await?;
        }
        Ok(())
    }
}
