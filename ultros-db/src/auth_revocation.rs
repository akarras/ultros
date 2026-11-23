//! Durable multi-replica auth revocation storage backed by Postgres.
//!
//! Raw Discord OAuth access tokens are never written to Postgres or `NOTIFY`:
//! every token is hashed with SHA-256 first.

use anyhow::Result;
use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use sha2::{Digest, Sha256};

use crate::UltrosDb;
use crate::wakeup::{AuthRevokeWakeup, CHANNEL_AUTH_REVOKE, pg_notify_on};

/// Discord OAuth2 access tokens expire after 7 days; retain token revocations
/// for 8 days so a revoked token can never outlive its revocation record.
pub const DEFAULT_AUTH_TOKEN_REVOCATION_TTL_SECS: i64 = 8 * 24 * 60 * 60;

/// Compute the 32-byte SHA-256 digest of a bearer token.
pub fn hash_auth_token(token: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// Encode a 32-byte SHA-256 token hash as 64 lowercase hex characters.
pub fn auth_token_hash_hex(hash: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in hash {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

/// Decode a 64-char hex string into a 32-byte token hash.
pub fn parse_auth_token_hash_hex(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 {
        return None;
    }
    let bytes = hex.as_bytes();
    let (pairs, remainder) = bytes.as_chunks::<2>();
    if !remainder.is_empty() {
        return None;
    }
    let mut out = [0u8; 32];
    for (idx, chunk) in pairs.iter().enumerate() {
        let hi = hex_nibble(chunk[0])?;
        let lo = hex_nibble(chunk[1])?;
        out[idx] = (hi << 4) | lo;
    }
    Some(out)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(crate) async fn upsert_token_revocation_on<C: ConnectionTrait>(
    conn: &C,
    token_hash: &[u8; 32],
    ttl_secs: i64,
) -> Result<(), sea_orm::DbErr> {
    let ttl_secs = ttl_secs.max(60);
    let now = Utc::now();
    let expires_at = now + chrono::Duration::seconds(ttl_secs);
    conn.execute_raw(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "INSERT INTO auth_token_revocation (token_hash, revoked_at, expires_at)
         VALUES ($1, $2, $3)
         ON CONFLICT (token_hash) DO UPDATE
         SET revoked_at = EXCLUDED.revoked_at,
             expires_at = GREATEST(auth_token_revocation.expires_at, EXCLUDED.expires_at)",
        [token_hash.to_vec().into(), now.into(), expires_at.into()],
    ))
    .await?;
    Ok(())
}

pub(crate) async fn upsert_user_revocation_on<C: ConnectionTrait>(
    conn: &C,
    discord_user_id: i64,
) -> Result<(), sea_orm::DbErr> {
    let now = Utc::now();
    conn.execute_raw(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "INSERT INTO auth_user_revocation (discord_user_id, revoked_at)
         VALUES ($1, $2)
         ON CONFLICT (discord_user_id) DO UPDATE
         SET revoked_at = GREATEST(auth_user_revocation.revoked_at, EXCLUDED.revoked_at)",
        [discord_user_id.into(), now.into()],
    ))
    .await?;
    Ok(())
}

pub(crate) async fn notify_auth_revoke_on<C: ConnectionTrait>(
    conn: &C,
    origin: &str,
    token_hash: Option<&[u8; 32]>,
    discord_user_id: Option<i64>,
) -> Result<(), sea_orm::DbErr> {
    let payload = AuthRevokeWakeup {
        o: origin.to_string(),
        t: token_hash.map(auth_token_hash_hex),
        u: discord_user_id,
    };
    if let Ok(json) = serde_json::to_string(&payload) {
        pg_notify_on(conn, CHANNEL_AUTH_REVOKE, &json).await?;
    }
    Ok(())
}

impl UltrosDb {
    /// Durably revoke a bearer token hash and notify remote replicas to evict it
    /// from their local `AuthUserCache` immediately.
    pub async fn revoke_auth_token(&self, token_hash: &[u8; 32], ttl_secs: i64) -> Result<()> {
        if self.db.get_database_backend() != DbBackend::Postgres {
            return Ok(());
        }
        let txn = self.db.begin().await?;
        upsert_token_revocation_on(&txn, token_hash, ttl_secs).await?;
        notify_auth_revoke_on(&txn, self.notify_origin(), Some(token_hash), None).await?;
        txn.commit().await?;
        Ok(())
    }

    /// Clear any durable revocation for a token hash (used by `test-auth`
    /// deterministic re-login fixtures).
    pub async fn clear_auth_token_revocation(&self, token_hash: &[u8; 32]) -> Result<()> {
        if self.db.get_database_backend() != DbBackend::Postgres {
            return Ok(());
        }
        self.db
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "DELETE FROM auth_token_revocation WHERE token_hash = $1",
                [token_hash.to_vec().into()],
            ))
            .await?;
        Ok(())
    }

    /// Fast pre-check before calling Discord on an `AuthUserCache` miss.
    pub async fn is_auth_token_revoked(&self, token_hash: &[u8; 32]) -> Result<bool> {
        if self.db.get_database_backend() != DbBackend::Postgres {
            return Ok(false);
        }
        let row = self
            .db
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "SELECT EXISTS(
                    SELECT 1 FROM auth_token_revocation
                    WHERE token_hash = $1 AND expires_at > now()
                ) AS revoked",
                [token_hash.to_vec().into()],
            ))
            .await?;
        Ok(row
            .and_then(|r| r.try_get::<bool>("", "revoked").ok())
            .unwrap_or(false))
    }

    /// Combined post-Discord revocation check covering both explicit token
    /// logout and account deletion of `discord_user_id` at or after
    /// `cookie_issued_at`.
    pub async fn is_auth_session_revoked(
        &self,
        token_hash: &[u8; 32],
        discord_user_id: i64,
        cookie_issued_at: DateTime<Utc>,
    ) -> Result<bool> {
        if self.db.get_database_backend() != DbBackend::Postgres {
            return Ok(false);
        }
        let row = self
            .db
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "SELECT (
                    EXISTS(
                        SELECT 1 FROM auth_token_revocation
                        WHERE token_hash = $1 AND expires_at > now()
                    )
                    OR EXISTS(
                        SELECT 1 FROM auth_user_revocation
                        WHERE discord_user_id = $2 AND revoked_at >= $3
                    )
                ) AS revoked",
                [
                    token_hash.to_vec().into(),
                    discord_user_id.into(),
                    cookie_issued_at.into(),
                ],
            ))
            .await?;
        Ok(row
            .and_then(|r| r.try_get::<bool>("", "revoked").ok())
            .unwrap_or(false))
    }

    /// Remove expired token revocation rows so the table stays bounded.
    pub async fn cleanup_expired_auth_token_revocations(&self) -> Result<u64> {
        if self.db.get_database_backend() != DbBackend::Postgres {
            return Ok(0);
        }
        let res = self
            .db
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "DELETE FROM auth_token_revocation WHERE expires_at <= now()",
                [],
            ))
            .await?;
        Ok(res.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_hash_hex_round_trips_and_rejects_malformed_input() {
        let hash = hash_auth_token("secret-bearer-token");
        let hex = auth_token_hash_hex(&hash);
        assert_eq!(hex.len(), 64);
        assert!(!hex.contains("secret"));
        assert_eq!(parse_auth_token_hash_hex(&hex), Some(hash));
        assert_eq!(parse_auth_token_hash_hex(&hex.to_uppercase()), Some(hash));
        assert_eq!(parse_auth_token_hash_hex("short"), None);
        assert_eq!(parse_auth_token_hash_hex(&"z".repeat(64)), None);
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn postgres_token_and_user_revocations_and_notify_work() {
        use migration::{Migrator, MigratorTrait};
        use sea_orm::{ConnectOptions, Database};

        let url = std::env::var("MIGRATION_TEST_DATABASE_URL").unwrap();
        let mut options = ConnectOptions::new(url.clone());
        options
            .max_connections(1)
            .min_connections(1)
            .sqlx_logging(false);
        let connection = Database::connect(options).await.unwrap();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let schema = format!("auth_revocation_{}_{nonce}", std::process::id());
        connection
            .execute_unprepared(&format!(
                "CREATE SCHEMA {schema}; SET search_path TO {schema}"
            ))
            .await
            .unwrap();
        Migrator::up(&connection, None).await.unwrap();
        let db = UltrosDb::from_connection(connection.clone());

        let mut listener = sea_orm::sqlx::postgres::PgListener::connect(&url)
            .await
            .unwrap();
        listener.listen(CHANNEL_AUTH_REVOKE).await.unwrap();

        let token_hash = hash_auth_token("live-token-1");
        assert!(!db.is_auth_token_revoked(&token_hash).await.unwrap());
        db.revoke_auth_token(&token_hash, DEFAULT_AUTH_TOKEN_REVOCATION_TTL_SECS)
            .await
            .unwrap();
        assert!(db.is_auth_token_revoked(&token_hash).await.unwrap());

        let notif = tokio::time::timeout(std::time::Duration::from_secs(5), listener.try_recv())
            .await
            .unwrap()
            .unwrap()
            .expect("expected auth revoke notification");
        let parsed: AuthRevokeWakeup = serde_json::from_str(notif.payload()).unwrap();
        assert_eq!(parsed.o, db.notify_origin());
        assert_eq!(
            parsed.t.as_deref(),
            Some(auth_token_hash_hex(&token_hash).as_str())
        );
        assert_eq!(parsed.u, None);

        // Clearing revocation allows deterministic test re-login.
        db.clear_auth_token_revocation(&token_hash).await.unwrap();
        assert!(!db.is_auth_token_revoked(&token_hash).await.unwrap());

        // Account deletion writes a durable user revocation record.
        let before_delete = Utc::now() - chrono::Duration::seconds(5);
        connection
            .execute_unprepared("INSERT INTO discord_user (id, username) VALUES (777, 'Del')")
            .await
            .unwrap();
        let other_token_hash = hash_auth_token("other-device-token");
        db.delete_discord_user_and_revoke(777, Some(&token_hash))
            .await
            .unwrap();
        assert!(
            db.is_auth_session_revoked(&other_token_hash, 777, before_delete)
                .await
                .unwrap(),
            "cookies issued before deletion must be rejected"
        );
        let after_delete = Utc::now() + chrono::Duration::seconds(5);
        assert!(
            !db.is_auth_session_revoked(&other_token_hash, 777, after_delete)
                .await
                .unwrap(),
            "a fresh login after deletion must be allowed"
        );
        assert!(
            db.is_auth_token_revoked(&token_hash).await.unwrap(),
            "the deleted session's own token must also be revoked"
        );

        connection
            .execute_unprepared(&format!("DROP SCHEMA {schema} CASCADE"))
            .await
            .unwrap();
    }
}
