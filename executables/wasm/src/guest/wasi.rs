//! Generic WASI descriptor operations not exposed by `std`.

/// Runs a file-system-specific control command on an open descriptor.
///
/// `input` and `output` must point to guest memory, use 16-byte-aligned
/// storage, and not overlap. The host validates both spans and limits payloads
/// to 256 bytes. The command is opaque to WASI and forwarded to the file.
#[allow(clippy::too_many_arguments)]
pub unsafe fn fd_control(
    fd: i32,
    command: u32,
    input: *const u8,
    input_length: u32,
    output: *mut u8,
    output_length: u32,
) -> i32 {
    #[link(wasm_import_module = "wasi_snapshot_preview1")]
    unsafe extern "C" {
        fn fd_control(
            fd: i32,
            command: u32,
            input: u32,
            input_length: u32,
            output: u32,
            output_length: u32,
        ) -> i32;
    }

    unsafe {
        fd_control(
            fd,
            command,
            input as usize as u32,
            input_length,
            output as usize as u32,
            output_length,
        )
    }
}
