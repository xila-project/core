//! The native transport of Core's graphics call executor.
//!
//! One slot, [`xila_graphics_call`], runs one graphics call for the calling
//! process. The SDK's generated `xila/graphics.h` and `xila-sys` bindings
//! name every call like the Wasm guest API and pack their arguments into
//! words; this module hands those words to `graphics::call::execute_locked`.
//! Pointer, string and buffer arguments are host pointers read in place:
//! native code is trusted, so [`HostMemory`] checks only that a range is
//! representable as a slice, and the executor bounds strings and buffers.
//!
//! The process's identity table ([`State`]) lives in its host-owned
//! [`Resources`](crate::Resources), not on the coroutine's stack, and is
//! allocated by the process's first graphics call: a process that never draws
//! pays one empty pointer. `xila_process_exit`, a return from the entry and the
//! host that cancels the process (`HostState::release`) delete the objects the
//! process still owns and drop the state, with the task identity file pickers act
//! with, so a reused task identifier cannot inherit it.
//!
//! Raw `lv_*` functions are not exposed. They would bypass the graphics lock
//! and the per-process ownership that lets a process's objects be deleted
//! when it ends.

use crate::{Process, Status, process::this, suspend_dynamic};
use alloc::boxed::Box;
use core::{ffi::CStr, ffi::c_void, pin::pin, slice};
use graphics::call::{Call, Error, State, Transport, Word, execute_locked};

/// Most words a call takes: the executor's call frame.
const MAX_ARGUMENTS: usize = 7;

/// Addresses are host pointers.
struct HostMemory;

/// A slice may span at most `isize::MAX` bytes and must not wrap the address
/// space; a size a caller computes from a bad length would otherwise be
/// undefined behavior to build. Native code is trusted to pass memory it owns,
/// so this is the only check.
fn check_range(address: Word, length: usize) -> Result<(), Error> {
    if address == 0 || length > isize::MAX as usize || address.checked_add(length).is_none() {
        return Err(Error::InvalidMemory);
    }
    Ok(())
}

impl Transport for HostMemory {
    fn word_bits(&self) -> u32 {
        usize::BITS
    }

    fn bytes(&self, address: Word, length: usize) -> Result<&[u8], Error> {
        if length == 0 {
            return Ok(&[]);
        }
        check_range(address, length)?;
        // SAFETY: a trusted native caller passes a readable range of this length.
        Ok(unsafe { slice::from_raw_parts(address as *const u8, length) })
    }

    fn bytes_mut(&mut self, address: Word, length: usize) -> Result<&mut [u8], Error> {
        if length == 0 {
            return Ok(&mut []);
        }
        check_range(address, length)?;
        // SAFETY: a trusted native caller passes a writable range of this length.
        Ok(unsafe { slice::from_raw_parts_mut(address as *mut u8, length) })
    }

    fn c_string(&self, address: Word, limit: usize) -> Result<&CStr, Error> {
        if address == 0 {
            return Err(Error::InvalidMemory);
        }
        // A bounded scan, as `strnlen` does: never read past the limit, so the
        // executor's limits cap how much of an unterminated string is touched.
        // A string with no NUL inside the limit is, to a caller, either
        // unterminated or too long: an invalid argument either way.
        let start = address as *const u8;
        for length in 0..limit {
            // SAFETY: the caller passes a NUL-terminated string, and the scan
            // stops at the first NUL or the limit, whichever comes first.
            if unsafe { *start.add(length) } == 0 {
                // SAFETY: the bytes up to this first NUL hold no other NUL.
                return Ok(unsafe {
                    CStr::from_bytes_with_nul_unchecked(slice::from_raw_parts(start, length + 1))
                });
            }
        }
        Err(Error::InvalidValue)
    }
}

fn status(error: Error) -> Status {
    match error {
        Error::InvalidArgumentsCount
        | Error::InvalidMemory
        | Error::InvalidHandle
        | Error::InvalidValue => Status::InvalidArgument,
        Error::Unavailable => Status::Unavailable,
        Error::Unsupported => Status::Unsupported,
        Error::ResourceLimit => Status::OutOfMemory,
        Error::NativeFailure => Status::InputOutput,
    }
}

/// Run one graphics call: `function` is a call ID from the SDK's
/// `xila/graphics.h`, `arguments` points to `argument_count` words (at most
/// seven; null at zero) and `result` receives the call's result, a word or a
/// 32-bit value, where the call has one. The call waits for the graphics lock
/// by suspending the process, not by blocking the executor. Unsupported when
/// the runtime has no graphics support, Unavailable without a display.
/// # Safety
/// Process must be a live capability. Every pointer argument must satisfy the
/// call named by the ID. The call must not be made from a graphics callback.
pub unsafe extern "C" fn xila_graphics_call(
    process: *const Process,
    function: u32,
    arguments: *const usize,
    argument_count: usize,
    result: *mut c_void,
) -> Status {
    let process = match this(process) {
        Ok(process) => process,
        Err(error) => return error,
    };
    if argument_count > MAX_ARGUMENTS || (argument_count != 0 && arguments.is_null()) {
        return Status::InvalidArgument;
    }
    let Some(manager) = process.host.graphics_manager() else {
        return Status::Unavailable;
    };
    let mut words = [0; MAX_ARGUMENTS];
    if argument_count != 0 {
        // SAFETY: the caller passes `argument_count` readable words.
        words[..argument_count]
            .copy_from_slice(unsafe { slice::from_raw_parts(arguments, argument_count) });
    }
    let call = Call {
        function: i32::try_from(function).unwrap_or(-1),
        arguments: words,
        count: argument_count as i32,
        result: result as Word,
    };

    let mut outcome = Err(Error::Unavailable);
    suspend_dynamic(pin!(async {
        let _lock = manager.lock().await;
        // SAFETY: `resources` is the host-owned state of this process, which
        // the loader keeps live while the process runs. The graphics lock is
        // held and nothing below awaits, so this is the only reference into
        // the table: another thread of the process, or the exit path, reaches
        // it only through the same lock. No reference or pointer into it
        // outlives the locked section, which is the only place it is formed.
        let slot = unsafe { &mut (*process.resources).graphics };
        let state = slot.get_or_insert_with(|| Box::new(State::new(Some(manager))));
        state.set_file_system(process.host.virtual_file_system(), process.task);
        // SAFETY: the graphics lock is held, and it is `manager`'s.
        outcome = unsafe { execute_locked(state, &mut HostMemory, &call) };
    }));
    outcome.map_or_else(status, |()| Status::Ok)
}

/// Delete what a process's graphics state still owns. Waits for the graphics
/// lock by suspending, never by blocking the executor. Idempotent.
///
/// # Safety
/// `state` points to the process's live graphics state.
pub(crate) unsafe fn release(state: *mut State) {
    // SAFETY: the caller passes a live state, used only under the lock.
    let Some(manager) = (unsafe { (*state).manager() }) else {
        return;
    };
    suspend_dynamic(pin!(async {
        let _lock = manager.lock().await;
        // SAFETY: the lock is held and nothing below awaits.
        unsafe { (*state).release_locked() };
    }));
}
