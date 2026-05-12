//! `RequireListPermission<MIN>` Axum extractor — rejects with 403 if the
//! authenticated user's permission on the list is below `MIN`.

use axum::extract::{FromRef, FromRequestParts, Path};
use axum::http::request::Parts;
use ultros_api_types::list::ListPermission;
use ultros_db::UltrosDb;

use crate::web::error::ApiError;
use crate::web::oauth::{AuthDiscordUser, AuthUserCache};

/// Carries the authenticated viewer's permission on a list, gated to be
/// at least `MIN` (interpreted as a `ListPermission`).
///
/// Used as an extractor: `perm: RequireListPermission<{ READ }>` etc.
#[derive(Debug, Clone, Copy)]
pub struct RequireListPermission<const MIN: u8> {
    pub list_id: i32,
    pub user_id: i64,
    pub permission: ListPermission,
}

/// Map `u8` const-generic argument to a `ListPermission` value.
/// Delegates to `ListPermission::From<i16>` so the mapping lives in
/// exactly one place. Unknown values collapse to `None`, which fails
/// every permission gate.
fn min_to_permission(min: u8) -> ListPermission {
    ListPermission::from(min as i16)
}

pub const READ: u8 = 1;
pub const WRITE: u8 = 2;
pub const OWNER: u8 = 3;

impl<S, const MIN: u8> FromRequestParts<S> for RequireListPermission<MIN>
where
    S: Send + Sync,
    axum_extra::extract::cookie::Key: FromRef<S>,
    UltrosDb: FromRef<S>,
    AuthUserCache: FromRef<S>,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Path(list_id): Path<i32> = Path::from_request_parts(parts, state)
            .await
            .map_err(|e| ApiError::from(anyhow::anyhow!("invalid list id in path: {e}")))?;

        let user = AuthDiscordUser::from_request_parts(parts, state).await?;
        let db = UltrosDb::from_ref(state);

        let permission = db.get_permission(list_id, user.id as i64).await?;
        let required = min_to_permission(MIN);
        if permission < required {
            return Err(ApiError::from(anyhow::Error::from(
                ultros_db::lists::ListError::Forbidden("insufficient permission for this list"),
            )));
        }

        Ok(Self {
            list_id,
            user_id: user.id as i64,
            permission,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_to_permission_known_values() {
        assert_eq!(min_to_permission(READ), ListPermission::Read);
        assert_eq!(min_to_permission(WRITE), ListPermission::Write);
        assert_eq!(min_to_permission(OWNER), ListPermission::Owner);
    }

    #[test]
    fn min_to_permission_unknown_collapses_to_none() {
        assert_eq!(min_to_permission(0), ListPermission::None);
        assert_eq!(min_to_permission(4), ListPermission::None);
        assert_eq!(min_to_permission(u8::MAX), ListPermission::None);
    }

    #[test]
    fn require_permission_constants_match_repr() {
        // Defensive: if someone renumbers ListPermission, this test catches
        // the drift before it silently weakens the gate.
        assert_eq!(READ, ListPermission::Read as i16 as u8);
        assert_eq!(WRITE, ListPermission::Write as i16 as u8);
        assert_eq!(OWNER, ListPermission::Owner as i16 as u8);
    }
}
