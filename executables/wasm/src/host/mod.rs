mod error;
mod executable;
pub mod execution;
#[cfg(feature = "graphics")]
pub mod graphics;
mod main;
mod registrable;
mod store;
pub mod translation;
mod wasi;

pub use executable::*;
pub use store::GlobalStore;
pub use wasi::WasiContext;
pub use wasi::register::add_wasi_to_linker;

use core::sync::atomic::{AtomicBool, Ordering};

static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Request cooperative cancellation of the currently executing native WASM guest.
/// This is safe to call from a native signal handler.
pub fn request_shutdown() {
    SHUTDOWN_REQUESTED.store(true, Ordering::Relaxed);
}

/// Clear a prior native shutdown request before starting a new guest.
pub fn clear_shutdown_request() {
    SHUTDOWN_REQUESTED.store(false, Ordering::Relaxed);
}

pub(crate) fn shutdown_requested() -> bool {
    SHUTDOWN_REQUESTED.load(Ordering::Relaxed)
}
