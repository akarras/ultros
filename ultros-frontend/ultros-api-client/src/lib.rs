//! API error types and the server's in-process API transport.
//!
//! The consuming app supplies the API router and each request's headers.
//! This crate has no Leptos, application-state, or game-data dependency.

pub mod error;

#[cfg(feature = "ssr")]
pub mod ssr_api;
