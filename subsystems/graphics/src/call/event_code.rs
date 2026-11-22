//! Xila's event contract, shared with guest bindings. The source lives in the
//! generator crate, which also hands it to guest builds, so the two cannot drift.

include!("../../bindings_generator/src/event_code.rs");
