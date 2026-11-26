//! The graphics API of Wasm guests, over the native ABI.
//!
//! Every function has the name, arguments and result of its Wasm guest
//! counterpart, so a Rust application written against the guest API builds
//! natively with little change. The module is generated from the same
//! declaration (`graphics_bindgen`); only `call` is written by hand.
//!
//! A call returns its result, or the non-zero [`Status`] the
//! host reported as an `i32`. Objects are 32-bit handles the host checks,
//! never LVGL pointers, and the host deletes whatever a process still owns
//! when it ends. Raw `lv_*` functions are not exposed: they would bypass the
//! graphics lock and that ownership.
//!
//! The functions need the entry runtime's startup data and run only inside
//! the entry call.

#[rustfmt::skip] // Formatted by the graphics generator.
mod generated;
pub use generated::*;

use crate::{Status, runtime};

/// Run one call through the host's graphics slot.
///
/// # Safety
/// Pointers among `arguments` must be valid for the call named by `function`.
unsafe fn call<T>(function: FunctionCall, arguments: &[usize]) -> Result<T> {
    let Some(startup) = runtime::startup() else {
        return Err(Status::Unavailable.0 as i32);
    };
    // SAFETY: the runtime validated the table before installing the startup data.
    let graphics_call = unsafe { (*startup.api).graphics_call };
    // SAFETY: results are integers and pointers, for which zero is a value.
    let mut result: T = unsafe { core::mem::zeroed() };
    // SAFETY: the host reads the words and writes at most a word at `result`,
    // which is a `T` of that size for calls that have a result.
    let status = unsafe {
        graphics_call(
            startup.process,
            function as u32,
            arguments.as_ptr(),
            arguments.len(),
            (&raw mut result).cast(),
        )
    };
    if status == Status::Ok {
        Ok(result)
    } else {
        Err(status.0 as i32)
    }
}

#[cfg(test)]
// The tests record the calls in statics, like `runtime.rs`: only load/store, which are
// also available on Cortex-M0.
#[allow(clippy::disallowed_types)]
mod tests {
    use super::*;
    use crate::{XILA_VERSION_MAJOR, XILA_VERSION_MINOR, XilaApi, XilaStartup};
    use core::{
        ffi::{CStr, c_void},
        mem::{MaybeUninit, size_of},
        ptr::{addr_of_mut, null, null_mut},
        sync::atomic::{AtomicU32, AtomicUsize, Ordering},
    };

    static FUNCTION: AtomicU32 = AtomicU32::new(0);
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    static WORDS: [AtomicUsize; 3] = [const { AtomicUsize::new(0) }; 3];

    /// A host that records the call and answers `label_create` with a handle.
    unsafe extern "C" fn host(
        _process: *const crate::Process,
        function: u32,
        arguments: *const usize,
        count: usize,
        result: *mut c_void,
    ) -> Status {
        FUNCTION.store(function, Ordering::Relaxed);
        COUNT.store(count, Ordering::Relaxed);
        for (index, word) in WORDS.iter().enumerate().take(count) {
            word.store(unsafe { *arguments.add(index) }, Ordering::Relaxed);
        }
        match function {
            f if f == FunctionCall::LabelCreate as u32 => {
                unsafe { result.cast::<usize>().write(0x1234) };
                Status::Ok
            }
            f if f == FunctionCall::ObjectGetWidth as u32 => {
                unsafe { result.cast::<i32>().write(-7) };
                Status::Ok
            }
            f if f == FunctionCall::WindowCreate as u32 => Status::InvalidArgument,
            _ => Status::Ok,
        }
    }

    unsafe fn entry(_: *const XilaStartup) -> i32 {
        // A result comes back as the type the call declares.
        let label = unsafe { label_create(0x10 as *mut Object) }.unwrap();
        assert_eq!(label as usize, 0x1234);
        assert_eq!(
            FUNCTION.load(Ordering::Relaxed),
            FunctionCall::LabelCreate as u32
        );
        assert_eq!(COUNT.load(Ordering::Relaxed), 1);
        assert_eq!(WORDS[0].load(Ordering::Relaxed), 0x10);

        // Arguments are packed in order, and a call without a result gets unit.
        let text: &CStr = c"hello";
        unsafe { label_set_text(label, text.as_ptr()) }.unwrap();
        assert_eq!(COUNT.load(Ordering::Relaxed), 2);
        assert_eq!(WORDS[0].load(Ordering::Relaxed), 0x1234);
        assert_eq!(WORDS[1].load(Ordering::Relaxed), text.as_ptr() as usize);

        // Signed results and signed arguments keep their value.
        assert_eq!(unsafe { object_get_width(label) }, Ok(-7));
        unsafe { object_set_size(label, -1, 40) }.unwrap();
        assert_eq!(WORDS[1].load(Ordering::Relaxed) as isize, -1);
        assert_eq!(WORDS[2].load(Ordering::Relaxed), 40);

        // A color is one word of its bytes.
        let red = Color {
            blue: 0,
            green: 0,
            red: 255,
        };
        unsafe { window_set_icon(label, text.as_ptr(), red) }.unwrap();
        assert_eq!(WORDS[2].load(Ordering::Relaxed), 0x00ff_0000);

        // A host failure is the status as an i32.
        assert_eq!(
            unsafe { window_create() }.map(|_| ()),
            Err(Status::InvalidArgument.0 as i32)
        );
        0
    }

    #[test]
    fn calls_reach_the_host_slot_with_packed_words() {
        let mut api = MaybeUninit::<XilaApi>::zeroed();
        let table = api.as_mut_ptr();
        // SAFETY: only the header and the graphics slot are ever read.
        unsafe {
            addr_of_mut!((*table).version).write((XILA_VERSION_MAJOR << 16) | XILA_VERSION_MINOR);
            addr_of_mut!((*table).size).write(size_of::<XilaApi>() as u32);
            addr_of_mut!((*table).graphics_call).write(host);
        }
        let mut startup = MaybeUninit::<XilaStartup>::zeroed();
        let block = startup.as_mut_ptr();
        // SAFETY: a startup block of null capabilities, with the table above.
        unsafe {
            addr_of_mut!((*block).size).write(size_of::<XilaStartup>() as u32);
            addr_of_mut!((*block).api).write(table);
            addr_of_mut!((*block).process).write(null());
            addr_of_mut!((*block).heap).write(null_mut());
        }
        assert_eq!(unsafe { runtime::start(block, entry) }, 0);
        // Outside the entry call there is no host to ask.
        assert_eq!(
            unsafe { window_create() }.map(|_| ()),
            Err(Status::Unavailable.0 as i32)
        );
    }
}
