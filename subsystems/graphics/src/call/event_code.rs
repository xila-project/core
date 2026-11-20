//! Xila's event contract, shared with guest bindings. The source lives in the
//! generator crate, which also hands it to guest builds, so the two cannot drift.
//!
//! Xila's guest event contract, independent of LVGL's native discriminants.
//! Keep the event order stable; the executor translates at the host boundary.

include!("../../bindings_generator/src/event_code.rs");
