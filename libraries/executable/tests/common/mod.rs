//! Executable memory and device fixtures shared by the native integration tests.
#![allow(dead_code)]

use core::{
    alloc::Layout,
    ptr::NonNull,
    task::{Context, Poll},
};
use portable_atomic::{AtomicUsize, Ordering};

use file_system::{DirectBaseOperations, DirectCharacterDevice, MountOperations, Size};
use native_loader::{Error as LoaderError, ExecutableMemory, Region};

pub const PAGE: usize = 4096;
pub static MAPPINGS: AtomicUsize = AtomicUsize::new(0);

pub struct Mmap;

// SAFETY: the initial private mapping is writable and respects layout. Final
// protections apply to complete pages. x86-64 has coherent I and D caches.
unsafe impl ExecutableMemory for Mmap {
    fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, LoaderError> {
        if layout.align() > PAGE {
            return Err(LoaderError::AllocationFailed);
        }
        let pointer = unsafe {
            libc::mmap(
                core::ptr::null_mut(),
                layout.size(),
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if pointer == libc::MAP_FAILED {
            return Err(LoaderError::AllocationFailed);
        }
        MAPPINGS.fetch_add(1, Ordering::Relaxed);
        let pointer = NonNull::<u8>::new(pointer.cast()).unwrap();
        // Verify the loader clears BSS even when the allocator returns dirty memory.
        unsafe { pointer.as_ptr().write_bytes(0xa5, layout.size()) };
        Ok(pointer)
    }

    unsafe fn finish_writes(
        &self,
        allocation: NonNull<u8>,
        layout: Layout,
        regions: &mut dyn Iterator<Item = Region>,
    ) -> Result<(), LoaderError> {
        if unsafe { libc::mprotect(allocation.as_ptr().cast(), layout.size(), libc::PROT_NONE) }
            != 0
        {
            return Err(LoaderError::FinalizationFailed);
        }
        for region in regions {
            if !region.offset.is_multiple_of(PAGE) || (region.executable && region.writable) {
                return Err(LoaderError::FinalizationFailed);
            }
            let protection = if region.executable {
                libc::PROT_READ | libc::PROT_EXEC
            } else if region.writable {
                libc::PROT_READ | libc::PROT_WRITE
            } else {
                libc::PROT_READ
            };
            let address = unsafe { allocation.as_ptr().add(region.offset) };
            if unsafe { libc::mprotect(address.cast(), region.size, protection) } != 0 {
                return Err(LoaderError::FinalizationFailed);
            }
        }
        Ok(())
    }

    unsafe fn free(&self, allocation: NonNull<u8>, layout: Layout) {
        assert_eq!(
            unsafe { libc::munmap(allocation.as_ptr().cast(), layout.size()) },
            0
        );
        MAPPINGS.fetch_sub(1, Ordering::Relaxed);
    }
}

/// A character device that yields 0xa5 bytes, at most three per read.
pub struct FixtureEntropy;
impl DirectBaseOperations for FixtureEntropy {
    fn read(
        &self,
        _: &mut Context<'_>,
        buffer: &mut [u8],
        _: Size,
    ) -> Poll<file_system::Result<usize>> {
        let count = buffer.len().min(3);
        buffer[..count].fill(0xa5);
        Poll::Ready(Ok(count))
    }

    fn write(&self, _: &mut Context<'_>, _: &[u8], _: Size) -> Poll<file_system::Result<usize>> {
        Poll::Ready(Err(file_system::IoErrorKind::Unsupported))
    }
}
impl MountOperations for FixtureEntropy {
    fn unmount(&self) -> file_system::Result<()> {
        Ok(())
    }
}
impl DirectCharacterDevice for FixtureEntropy {}
