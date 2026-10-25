//! Platform-independent Wi-Fi control plane; packet traffic remains in smoltcp.
//! Long operations are scheduled by controls and performed by a driver runner.
//! Profiles, persistence and reconnect policy belong to user space.
mod command;
mod types;
pub use command::*;
pub use types::*;
