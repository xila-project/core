#[cfg(target_has_atomic = "ptr")]
pub use alloc::sync::{Arc, Weak};

mod arc_lock;

#[cfg(not(target_has_atomic = "ptr"))]
pub use arc_lock::{Arc, Weak};
