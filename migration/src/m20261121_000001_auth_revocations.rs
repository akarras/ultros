use sea_orm_migration::prelude::*;

/// Durable multi-replica revocation records for Discord OAuth web sessions.
///
/// `AuthUserCache` is process-local RAM. When multiple `ultros-web` replicas run
/// behind a load balancer, a logout or account deletion on replica A must revoke
/// the session on replica B even if a `LISTEN/NOTIFY` wake-up was missed or if
/// Discord's own token revocation is slow/fails.
///
/// - `auth_token_revocation` stores only the 32-byte SHA-256 digest of a revoked
///   bearer token (never the raw OAuth token) with an expiry matching Discord's
///   token lifetime so periodic cleanup keeps the table tiny.
/// - `auth_user_revocation` records the timestamp of an account deletion so any
///   other browser/device holding a cookie issued before deletion is rejected
///   before `get_or_create_discord_user` could recreate the deleted row, while a
///   fresh OAuth login after deletion (carrying a newer `discord_auth_iat`)
///   still works.
///
/// Expand-only: touches no existing tables so already-running N-1 replicas are
/// unaffected.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE TABLE IF NOT EXISTS auth_token_revocation (
                    token_hash BYTEA PRIMARY KEY,
                    revoked_at TIMESTAMPTZ NOT NULL DEFAULT now(),
                    expires_at TIMESTAMPTZ NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_auth_token_revocation_expires_at
                    ON auth_token_revocation (expires_at);
                CREATE TABLE IF NOT EXISTS auth_user_revocation (
                    discord_user_id BIGINT PRIMARY KEY,
                    revoked_at      TIMESTAMPTZ NOT NULL DEFAULT now()
                )",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "DROP TABLE IF EXISTS auth_token_revocation;
                 DROP TABLE IF EXISTS auth_user_revocation",
            )
            .await?;
        Ok(())
    }
}
