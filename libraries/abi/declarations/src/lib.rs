#![allow(clippy::missing_safety_doc)]
#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]

include!(concat!(env!("OUT_DIR"), "/declarations.generated.rs"));

/// Version 1 identifies the current Xila application ABI as of the first
/// versioned package manifests. Unversioned historical builds have no
/// declared compatibility; increment this when making a breaking ABI change.
pub const APPLICATION_ABI_VERSION: u32 = 1;
