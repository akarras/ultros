//! Foundation shared by the Ultros server's service crates (`ultros-analyzer`,
//! `ultros-ingest`, `ultros-alerts`, ...) and the `ultros` binary that wires
//! them together: the in-process event buses and process-wide switches.
//!
//! Kept small and free of the web stack so a change to any one service only
//! rebuilds that service and the binary, not its siblings.

pub mod env_flag;
pub mod event;
#[cfg(feature = "test-auth")]
pub mod test_market_isolation;
