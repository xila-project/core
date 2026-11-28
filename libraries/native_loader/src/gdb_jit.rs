//! GDB JIT ABI, version 1.
mod symbol_file;

use alloc::alloc::{alloc, dealloc};
use core::{
    alloc::Layout,
    ptr::{self, NonNull},
    sync::atomic::{Ordering, compiler_fence},
};

use crate::elf::{Elf, Native};
use symbol_file::SymbolFile;

const JIT_REGISTER_FN: u32 = 1;
const JIT_UNREGISTER_FN: u32 = 2;

#[repr(C)]
struct JitCodeEntry {
    next_entry: *mut JitCodeEntry,
    prev_entry: *mut JitCodeEntry,
    symfile_addr: *const u8,
    symfile_size: u64,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct JitDescriptor {
    version: u32,
    action_flag: u32,
    relevant_entry: *mut JitCodeEntry,
    first_entry: *mut JitCodeEntry,
}

// Only access the descriptor and linked entries inside the critical section.
#[used]
#[unsafe(no_mangle)]
static mut __jit_debug_descriptor: JitDescriptor = JitDescriptor {
    version: 1,
    action_flag: 0,
    relevant_entry: ptr::null_mut(),
    first_entry: ptr::null_mut(),
};

#[inline(never)]
#[unsafe(no_mangle)]
extern "C" fn __jit_debug_register_code() {
    // GDB breaks here. The volatile read keeps the call and descriptor writes
    // observable with link-time optimization, on targets without inline assembly too.
    compiler_fence(Ordering::SeqCst);
    unsafe { ptr::read_volatile(&raw const __jit_debug_descriptor) };
}

fn entry_layout(size: usize) -> Option<(Layout, usize)> {
    Layout::new::<JitCodeEntry>()
        .extend(Layout::array::<u8>(size).ok()?)
        .ok()
}

/// Owns one linked entry and its trailing ELF bytes in a stable allocation.
pub(crate) struct Registration(NonNull<JitCodeEntry>);

impl Registration {
    pub(crate) fn new(elf: &Elf<'_, Native>, data_base: usize, exec_base: usize) -> Option<Self> {
        let symbol_file = SymbolFile::new(elf)?;
        let size = symbol_file.size()?;
        let (layout, offset) = entry_layout(size)?;
        let entry = NonNull::new(unsafe { alloc(layout) })?.cast::<JitCodeEntry>();
        // SAFETY: this exclusive allocation has room for the entry and ELF.
        let bytes = unsafe {
            let start = entry.as_ptr().cast::<u8>().add(offset);
            start.write_bytes(0, size);
            core::slice::from_raw_parts_mut(start, size)
        };
        if symbol_file
            .write(bytes, elf, data_base, exec_base)
            .is_none()
        {
            unsafe { dealloc(entry.as_ptr().cast(), layout) };
            return None;
        }
        let symfile_addr = bytes.as_ptr();
        critical_section::with(|_| {
            // SAFETY: the critical section excludes concurrent list changes.
            unsafe {
                let descriptor = &raw mut __jit_debug_descriptor;
                let first = (*descriptor).first_entry;
                entry.as_ptr().write(JitCodeEntry {
                    next_entry: first,
                    prev_entry: ptr::null_mut(),
                    symfile_addr,
                    symfile_size: size as u64,
                });
                if !first.is_null() {
                    (*first).prev_entry = entry.as_ptr();
                }
                (*descriptor).first_entry = entry.as_ptr();
                notify(descriptor, entry.as_ptr(), JIT_REGISTER_FN);
            }
        });
        Some(Self(entry))
    }
}

/// The caller holds the critical section and both pointers are valid.
unsafe fn notify(descriptor: *mut JitDescriptor, entry: *mut JitCodeEntry, action: u32) {
    unsafe {
        (*descriptor).relevant_entry = entry;
        (*descriptor).action_flag = action;
        __jit_debug_register_code();
        (*descriptor).action_flag = 0;
        (*descriptor).relevant_entry = ptr::null_mut();
    }
}

impl Drop for Registration {
    // Keep the unregister code in one place across image cleanup paths.
    #[inline(never)]
    fn drop(&mut self) {
        let entry = self.0.as_ptr();
        let size = critical_section::with(|_| {
            // SAFETY: the entry stays linked until this drop. The critical
            // section excludes concurrent access to its links.
            unsafe {
                let descriptor = &raw mut __jit_debug_descriptor;
                let next = (*entry).next_entry;
                let prev = (*entry).prev_entry;
                if prev.is_null() {
                    (*descriptor).first_entry = next;
                } else {
                    (*prev).next_entry = next;
                }
                if !next.is_null() {
                    (*next).prev_entry = prev;
                }
                notify(descriptor, entry, JIT_UNREGISTER_FN);
                (*entry).symfile_size as usize
            }
        });
        // The debugger has completed the unregister event. No links point at
        // the entry, and the image is still mapped.
        let (layout, _) = entry_layout(size).unwrap();
        unsafe { dealloc(entry.cast(), layout) };
    }
}
