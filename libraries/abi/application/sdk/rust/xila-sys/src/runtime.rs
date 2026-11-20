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

use crate::{Status, XILA_ABI_VERSION_MAJOR, XILA_ABI_VERSION_MINOR, XILA_HEAP_ALIGNMENT};
use crate::{XilaApi, XilaStartup};

const VERSION: u32 = (XILA_ABI_VERSION_MAJOR << 16) | XILA_ABI_VERSION_MINOR;
static STARTUP: AtomicPtr<XilaStartup> = AtomicPtr::new(ptr::null_mut());

/// ABI version note consumed by the native ELF loader.
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
/// they advertise. On success the host keeps them and all granted capabilities
/// live until the process ends. The application must obey the native ABI,
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
        || unsafe { (*api).version } != VERSION
    {
        return -1;
    }

    STARTUP.store(startup.cast_mut(), Ordering::Release);
    let result = unsafe { main(startup) };
    STARTUP.store(ptr::null_mut(), Ordering::Release);
    result
}

/// Return the installed API pointer, or null outside the entry call.
/// Dereferencing it requires the startup lifetime and the ABI's safety rules.
pub fn api() -> *const XilaApi {
    let startup = STARTUP.load(Ordering::Acquire);
    if startup.is_null() {
        return ptr::null();
    }
    // The host keeps startup live for as long as this image can run guest code.
    unsafe { (*startup).api }
}

/// Terminate the native process with status 134. Call from the application's
/// `#[panic_handler]` with `panic = "abort"`. The host reclaims the process
/// heap and image. Rust destructors do not run.
pub fn panic_abort() -> ! {
    let startup = STARTUP.load(Ordering::Acquire);
    if !startup.is_null() {
        // The host grants this capability for native processes; built-in entries
        // may run without it. A successful process_exit never returns.
        unsafe {
            let _ = ((*(*startup).api).process_exit)((*startup).process, 134);
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
/// the aligned address. Reallocation uses `GlobalAlloc`'s allocate/copy/free.
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
            let Some(size) = layout
                .size()
                .checked_add(layout.align() - 1)
                .and_then(|size| size.checked_add(size_of::<*mut c_void>()))
            else {
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
        // SAFETY: the allocation has room for the pointer header, any padding,
        // and the requested bytes. The aligned address also aligns the header.
        unsafe {
            let after_header = bytes.add(size_of::<*mut c_void>());
            let aligned = after_header.add(after_header.align_offset(layout.align()));
            aligned.cast::<*mut c_void>().sub(1).write(base);
            aligned
        }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: a live allocation can only have been obtained during this
        // process's entry call and must be released within that same lifetime.
        let startup = STARTUP.load(Ordering::Acquire);
        unsafe {
            let base = if layout.align() > XILA_HEAP_ALIGNMENT {
                pointer.cast::<*mut c_void>().sub(1).read()
            } else {
                pointer.cast()
            };
            let _ = ((*(*startup).api).heap_free)((*startup).heap, base);
        }
    }
}
