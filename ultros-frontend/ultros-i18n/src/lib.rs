//! Shared translations and locale context for Ultros's server and browser.
//!
//! Generate this module once per target so edits to application views can reuse
//! its compiled code. Consumers must forward either `ssr` or `hydrate`.

include!(concat!(env!("OUT_DIR"), "/i18n/mod.rs"));

pub mod fallback;
