//! Supplies this app build's version to the shared update observer.
pub use ultros_frontend_core::global_state::app_update::*;

pub const CLIENT_COMMIT: &str = env!("GIT_HASH");

pub fn provide_app_update_context() -> AppUpdate {
    ultros_frontend_core::global_state::app_update::provide_app_update_context(CLIENT_COMMIT)
}

#[cfg(test)]
mod tests {
    #[test]
    fn client_commit_is_baked_in() {
        assert!(!super::CLIENT_COMMIT.is_empty());
    }
}
