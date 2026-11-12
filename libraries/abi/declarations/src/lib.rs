#![no_std]
#![allow(clippy::missing_safety_doc)]
#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]

include!(concat!(env!("OUT_DIR"), "/declarations.generated.rs"));

/// Version 1 identifies the current Xila application ABI as of the first
/// versioned package manifests. Unversioned historical builds have no
/// declared compatibility; increment this when making a breaking ABI change.
pub const APPLICATION_ABI_VERSION: u32 = 1;

#[cfg(test)]
mod tests {
    /// Entry points defined through `abi_unsafe_function!` must reach the
    /// header: cbindgen only expands macros of crates named by package.
    #[test]
    fn macro_defined_entry_points_are_declared() {
        let _ = super::xila_file_system_file_open as *const ();
        let _ = super::xila_file_system_directory_read as *const ();
        let _ = super::xila_network_send as *const ();
    }
}
