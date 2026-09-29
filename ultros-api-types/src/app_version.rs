//! Version handshake between the server and the wasm client.
//!
//! The server stamps every response with the commit it was built from so a
//! long-lived tab can tell when it is running an older bundle. See
//! `docs/superpowers/specs/2026-09-05-app-update-system-design.md`.

/// Response header carrying the server's short git hash (`GIT_HASH`).
///
/// Lowercase and hyphenated on purpose: `http::HeaderName::from_static`
/// requires lowercase, and nginx-style proxies drop underscored names.
pub const APP_COMMIT_HEADER: &str = "x-ultros-commit";
