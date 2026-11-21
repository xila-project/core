//! Entry and process-heap support for standalone native Rust applications.

// The runtime uses only load/store, which are also available on Cortex-M0.
#![allow(clippy::disallowed_types)]
use core::{
    alloc::{GlobalAlloc, Layout},
    ffi::c_void,
    mem::size_of,
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};

use crate::{Status, XILA_HEAP_ALIGNMENT, XILA_VERSION_MAJOR, XILA_VERSION_MINOR};
use crate::{XilaApi, XilaStartup};

/// The Xila version this crate was generated for, packed as `(major << 16) | minor`.
const VERSION: u32 = (XILA_VERSION_MAJOR << 16) | XILA_VERSION_MINOR;
static STARTUP: AtomicPtr<XilaStartup> = AtomicPtr::new(ptr::null_mut());

/// ABI note consumed by the native ELF loader: an ELF note named `XILA` of type 1
/// whose four-byte descriptor is the Xila version the executable was built for,
/// packed as `(major << 16) | minor`. The loader runs the executable only when the
/// running Xila version is compatible with it, by the same rule that `start`
/// applies to the host's `XilaApi.version`.
#[doc(hidden)]
#[repr(C)]
pub struct AbiNote {
    name_size: u32,
    descriptor_size: u32,
    kind: u32,
    name: [u8; 8],
    version: u32,
}

#[doc(hidden)]
pub const ABI_NOTE: AbiNote = AbiNote {
    name_size: 5,
    descriptor_size: 4,
    kind: 1,
    name: *b"XILA\0\0\0\0",
    version: VERSION,
};

/// Emit the image entry point and the required ELF ABI note. Invoke once with
/// a function of type `unsafe fn(*const XilaStartup) -> i32`. It receives the
/// host's borrowed startup data and returns the application's status.
#[macro_export]
macro_rules! entry {
    ($main:path) => {
        #[used]
        #[unsafe(link_section = ".note.xila.abi")]
        static XILA_ABI_NOTE: $crate::runtime::AbiNote = $crate::runtime::ABI_NOTE;

        /// # Safety
        /// The host must supply the startup data described by Xila's native ABI
        /// and call this entry once per loaded image.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn _xila_start(startup: *const $crate::XilaStartup) -> i32 {
            unsafe { $crate::runtime::start(startup, $main) }
        }
    };
}

/// # Safety
/// The loader calls this once per image. Non-null startup and table pointers
/// must be aligned and readable through their size fields and cover the bytes
/// they advertise. A table that passes validation must be immutable and valid
/// for the lifetime of the image. The host keeps startup and all granted
/// capabilities live until the process ends. The application must obey the native ABI,
/// including keeping calls on the process's cooperative executor.
#[doc(hidden)]
pub unsafe fn start(
    startup: *const XilaStartup,
    main: unsafe fn(*const XilaStartup) -> i32,
) -> i32 {
    // Read only the prefixes until both structures are known to be large enough.
    if startup.is_null() || (unsafe { (*startup).size } as usize) < size_of::<XilaStartup>() {
        return -1;
    }
    let api = unsafe { (*startup).api };
    if api.is_null()
        || (unsafe { (*api).size } as usize) < size_of::<XilaApi>()
        || !compatible(VERSION, unsafe { (*api).version })
    {
        return -1;
    }

    STARTUP.store(startup.cast_mut(), Ordering::Release);
    let result = unsafe { main(startup) };
    STARTUP.store(ptr::null_mut(), Ordering::Release);
    result
}

/// Whether a host running the packed Xila version `running` can run an
/// executable built for the packed version `declared`. Before 1.0 the minor
/// versions must be equal; from 1.0 the majors must be equal and the running
/// minor at least the declared one. This mirrors `xila_version::compatible` in
/// Core and `xila_package_manifest::compatible` in package_manager.
const fn compatible(declared: u32, running: u32) -> bool {
    let (major, minor) = (declared >> 16, declared & 0xffff);
    let (running_major, running_minor) = (running >> 16, running & 0xffff);
    major == running_major
        && if major == 0 {
            minor == running_minor
        } else {
            minor <= running_minor
        }
}

/// Return the host's startup data during entry.
pub fn startup() -> Option<&'static XilaStartup> {
    // SAFETY: start installs validated startup data for the duration of entry.
    unsafe { STARTUP.load(Ordering::Acquire).as_ref() }
}

/// Return the installed API table during entry.
pub fn api() -> Option<&'static XilaApi> {
    // SAFETY: start installs validated startup data for the duration of entry.
    // The host's immutable table remains valid for the lifetime of the image.
    let startup = unsafe { STARTUP.load(Ordering::Acquire).as_ref() }?;
    unsafe { startup.api.as_ref() }
}

/// Terminate the native process with status 134. Call from the application's
/// `#[panic_handler]` with `panic = "abort"`. The host reclaims the process
/// heap and image. Rust destructors do not run.
pub fn panic_abort() -> ! {
    if let Some(startup) = unsafe { STARTUP.load(Ordering::Acquire).as_ref() }
        && let Some(api) = unsafe { startup.api.as_ref() }
    {
        // The host grants this capability for native processes; built-in entries
        // may run without it. A successful process_exit never returns.
        unsafe {
            let message = b"panicked\n";
            if !startup.standard_error.is_null() {
                let mut written = 0;
                let _ = (api.file_write)(
                    startup.standard_error,
                    message.as_ptr().cast(),
                    message.len(),
                    &mut written,
                );
            }
            let _ = (api.process_exit)(startup.process, 134);
        }
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    unsafe {
        core::arch::asm!("ud2", options(noreturn, nostack));
    }
    #[cfg(target_arch = "arm")]
    unsafe {
        core::arch::asm!("udf #0", options(noreturn, nostack));
    }
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("brk #0", options(noreturn, nostack));
    }
    #[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
    unsafe {
        core::arch::asm!("unimp", options(noreturn, nostack));
    }
    #[cfg(target_arch = "wasm32")]
    core::arch::wasm32::unreachable();
    #[cfg(not(any(
        target_arch = "x86",
        target_arch = "x86_64",
        target_arch = "arm",
        target_arch = "aarch64",
        target_arch = "riscv32",
        target_arch = "riscv64",
        target_arch = "wasm32",
    )))]
    loop {
        core::hint::spin_loop();
    }
}

/// Optional `#[global_allocator]` for native applications using `alloc`.
/// Allocation is available during the entry call when the host grants a heap.
/// Over-aligned blocks store the original heap pointer immediately before
/// the aligned address. Reallocation uses the host heap directly when the
/// requested alignment is within the heap's guarantee.
pub struct ProcessAllocator;

// SAFETY: the runtime installs a process heap for the duration of guest
// execution. Ordinary blocks use the heap's published alignment, and
// over-aligned blocks recover the original allocation when freed.
unsafe impl GlobalAlloc for ProcessAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let startup = STARTUP.load(Ordering::Acquire);
        if startup.is_null() {
            return ptr::null_mut();
        }

        let over_aligned = layout.align() > XILA_HEAP_ALIGNMENT;
        let size = if over_aligned {
            let Some(size) = layout.size().checked_add(layout.align()) else {
                return ptr::null_mut();
            };
            size
        } else {
            layout.size()
        };

        // SAFETY: the table and heap capability are live; the host rejects a null heap.
        let mut base = ptr::null_mut();
        let result = unsafe { ((*(*startup).api).heap_allocate)((*startup).heap, size, &mut base) };
        if result != Status::Ok {
            return ptr::null_mut();
        }
        let bytes = base.cast::<u8>();
        if !over_aligned {
            return bytes;
        }
        // SAFETY: the heap base and requested alignment are multiples of
        // XILA_HEAP_ALIGNMENT, which is at least a pointer wide. The next aligned
        // address strictly after the base is therefore at least a pointer and
        // at most layout.align() bytes away, leaving room for both header and data.
        unsafe {
            let after_header = bytes.add(size_of::<*mut c_void>());
            let aligned = after_header.add(after_header.align_offset(layout.align()));
            aligned.cast::<*mut c_void>().sub(1).write(base);
            aligned
        }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        let startup = STARTUP.load(Ordering::Acquire);
        if startup.is_null() {
            // The host reclaims any remaining blocks when the process exits.
            return;
        }
        unsafe {
            let base = if layout.align() > XILA_HEAP_ALIGNMENT {
                pointer.cast::<*mut c_void>().sub(1).read()
            } else {
                pointer.cast()
            };
            let _ = ((*(*startup).api).heap_free)((*startup).heap, base);
        }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if layout.align() > XILA_HEAP_ALIGNMENT {
            // SAFETY: GlobalAlloc::realloc requires new_size to fit this alignment.
            let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
            let resized = unsafe { self.alloc(new_layout) };
            if !resized.is_null() {
                unsafe {
                    ptr::copy_nonoverlapping(pointer, resized, layout.size().min(new_size));
                    self.dealloc(pointer, layout);
                }
            }
            return resized;
        }

        let startup = STARTUP.load(Ordering::Acquire);
        if startup.is_null() {
            return ptr::null_mut();
        }
        let mut resized = ptr::null_mut();
        let result = unsafe {
            ((*(*startup).api).heap_reallocate)(
                (*startup).heap,
                pointer.cast(),
                new_size,
                &mut resized,
            )
        };
        if result != Status::Ok {
            return ptr::null_mut();
        }
        resized.cast()
    }
}

#[cfg(test)]
mod tests {
    use super::compatible;

    // The same cases as `xila_version` and `xila_package_manifest`.
    #[test]
    fn checks_compatibility_at_major_and_minor_boundaries() {
        let version = |major: u32, minor: u32| (major << 16) | minor;
        for (declared, running, expected) in [
            ((0, 1), (0, 1), true),
            ((0, 1), (0, 2), false),
            ((0, 2), (0, 1), false),
            ((1, 3), (1, 2), false),
            ((1, 3), (1, 3), true),
            ((1, 3), (1, 9), true),
            ((1, 3), (2, 0), false),
            ((0, 9), (1, 0), false),
            ((0, 0), (0, 0), true),
            ((0, 0), (0, 1), false),
            ((0, 1), (0, 0), false),
        ] {
            assert_eq!(
                compatible(
                    version(declared.0, declared.1),
                    version(running.0, running.1)
                ),
                expected,
                "{declared:?} on {running:?}"
            );
        }
    }
}
