//! Account-scoped device grants and private observations. No plugin credential is
//! ever converted into the website's Discord principal.
pub(crate) mod auth;
pub(crate) mod crypto;
mod notifications;
mod retainers;
#[cfg(test)]
mod tests;

pub use auth::{ApprovalSelection, PluginPrincipal};
pub use crypto::{PluginKeys, normalize_human_code};
use sea_orm::{ConnectionTrait, DbBackend, DbErr, QueryResult, Statement, Value};

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("invalid_token")]
    Unauthorized,
    #[error("insufficient_scope")]
    Forbidden,
    #[error("expired_token")]
    Expired,
    #[error("authorization_pending")]
    Pending,
    #[error("access_denied")]
    Denied,
    #[error("slow_down")]
    SlowDown(u32),
    #[error("rate_limited")]
    RateLimited,
    #[error("{0}")]
    Invalid(&'static str),
    #[error("{0}")]
    Conflict(&'static str),
    #[error("service_unavailable")]
    Storage,
}

impl From<DbErr> for PluginError {
    fn from(_: DbErr) -> Self {
        Self::Storage
    }
}
pub type Result<T> = std::result::Result<T, PluginError>;

pub(crate) async fn query(
    db: &impl ConnectionTrait,
    sql: &str,
    values: Vec<Value>,
) -> Result<Vec<QueryResult>> {
    Ok(db
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            sql,
            values,
        ))
        .await?)
}
pub(crate) async fn one(
    db: &impl ConnectionTrait,
    sql: &str,
    values: Vec<Value>,
) -> Result<QueryResult> {
    query(db, sql, values)
        .await?
        .into_iter()
        .next()
        .ok_or(PluginError::Unauthorized)
}
pub(crate) async fn exec(db: &impl ConnectionTrait, sql: &str, values: Vec<Value>) -> Result<u64> {
    Ok(db
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            sql,
            values,
        ))
        .await?
        .rows_affected())
}
pub(crate) async fn lock_account(db: &impl ConnectionTrait, owner: i64) -> Result<()> {
    query(
        db,
        "SELECT pg_advisory_xact_lock(1924558123,hashint8($1))",
        vec![owner.into()],
    )
    .await?;
    Ok(())
}
fn bounded_text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.chars().count() <= max && !value.chars().any(char::is_control)
}
fn operation_id(value: &str) -> bool {
    (16..=64).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
