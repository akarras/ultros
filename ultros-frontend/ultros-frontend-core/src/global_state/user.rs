use ultros_api_types::user::UserData;

/// SSR-provided / bootstrap-provided initial value for the current user.
///
/// `None` inside the `Option` means "we know the user is not logged in"
/// (no auth cookie, or auth cookie was rejected at render time). The context
/// being absent entirely means we don't have a bootstrap value and callers
/// should fall back to fetching `/api/v1/current_user`.
#[derive(Clone, Debug)]
pub struct BootstrapUser(pub Option<UserData>);
