//! Deterministic Wi-Fi control-plane mock. It never changes the host's Wi-Fi.
//! Pair with a smoltcp packet device and spawn the returned runner explicitly.
mod controller;
mod runner;
pub use controller::*;
pub use runner::*;
