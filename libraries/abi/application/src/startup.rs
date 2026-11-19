//! Startup capabilities borrowed by a linked native C application.
use crate::{Directory, File, Heap, XILA_API, XilaApi};
use alloc::vec::Vec;
use core::{ffi::c_char, ptr};

/// UTF-8 bytes, not NUL-terminated. A null data pointer is valid only at length zero.
#[repr(C)]
pub struct XilaString {
    pub data: *const c_char,
    pub length: usize,
}

/// Host-owned startup data, valid only for the duration of the entry call.
/// The API is always present. Capability pointers may be null when not granted.
/// A null argument array has argument_count zero.
/// Applications must not close, retain or transfer these objects; closing them is
/// undefined behavior. Objects opened by the application must still be closed.
#[repr(C)]
pub struct XilaStartup {
    pub size: u32,
    pub api: *const XilaApi,
    pub heap: *mut Heap,
    pub arguments: *const XilaString,
    pub argument_count: usize,
    pub standard_input: *mut File,
    pub standard_output: *mut File,
    pub standard_error: *mut File,
    pub home: *mut Directory,
}

/// Invoke a linked C entry, then drop all granted objects, closing their streams.
/// Arguments use one small allocation; capabilities remain on the host stack.
/// # Safety
/// Entry must obey the startup lifetime and ownership rules, serialize object
/// access, and return normally without retaining any supplied pointers.
pub unsafe fn run_main(
    entry: unsafe extern "C" fn(*const XilaStartup) -> i32,
    arguments: &[&str],
    mut standard_input: Option<File>,
    mut standard_output: Option<File>,
    mut standard_error: Option<File>,
    mut home: Option<Directory>,
) -> i32 {
    let arguments: Vec<_> = arguments
        .iter()
        .map(|argument| XilaString {
            data: argument.as_ptr().cast(),
            length: argument.len(),
        })
        .collect();
    let mut heap = Heap::new();
    let startup = XilaStartup {
        size: core::mem::size_of::<XilaStartup>() as u32,
        api: &XILA_API,
        heap: &mut heap,
        arguments: if arguments.is_empty() {
            ptr::null()
        } else {
            arguments.as_ptr()
        },
        argument_count: arguments.len(),
        standard_input: standard_input
            .as_mut()
            .map_or(ptr::null_mut(), ptr::from_mut),
        standard_output: standard_output
            .as_mut()
            .map_or(ptr::null_mut(), ptr::from_mut),
        standard_error: standard_error
            .as_mut()
            .map_or(ptr::null_mut(), ptr::from_mut),
        home: home.as_mut().map_or(ptr::null_mut(), ptr::from_mut),
    };
    unsafe { entry(&startup) }
}
