//! Load trusted, statically linked position-independent native applications.
#![no_std]

mod elf;

use core::{alloc::Layout, ptr::NonNull};
use elf::{Elf, Native};

pub const ELF_MAGIC: &[u8; 4] = b"\x7fELF";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidHeader,
    Truncated,
    WrongClass,
    WrongEndian,
    WrongMachine,
    NotPositionIndependent,
    Interpreter,
    ThreadLocalStorage,
    InvalidAlignment,
    /// `e_flags` the target cannot run: Arm EABI version or float ABI, RISC-V float ABI or RVC.
    IncompatibleFlags(u32),
    InvalidSegment,
    OverlappingSegments,
    InvalidEntry,
    InvalidAbiNote,
    MissingAbiNote,
    IncompatibleAbi(u32),
    InvalidDynamic,
    /// DT_INIT, DT_FINI or an init, fini or preinit array: the loader runs no
    /// constructors or destructors.
    Constructors,
    DynamicDependency,
    UnsupportedDynamicTag(usize),
    InvalidRelocationTable,
    InvalidRelocationTarget,
    InvalidRelocationAddend,
    UnsupportedRelocation(u32),
    SymbolRelocation,
    ImageTooLarge,
    AllocationFailed,
    FinalizationFailed,
}

/// A loadable region, relative to the beginning of the allocated image.
#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub offset: usize,
    pub size: usize,
    pub executable: bool,
    pub writable: bool,
}

/// Platform-specific executable RAM or PSRAM.
///
/// # Safety
/// Successful allocations supply an exclusive writable allocation of the given
/// layout, with the same virtual addresses used for data access and execution.
/// Finalization must make written instructions visible to instruction fetch,
/// apply permissions supported by the platform and preserve region contents.
/// Memory must remain mapped until freed, including on finalization failure.
pub unsafe trait ExecutableMemory: Sync {
    fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, Error>;

    /// Synchronize caches and apply final executable/data permissions.
    /// Return an error if the region layout cannot be supported.
    ///
    /// # Safety
    /// The allocation must have come from `allocate` with this layout. All
    /// writes by the loader are complete and no references to the bytes remain.
    unsafe fn finish_writes(
        &self,
        allocation: NonNull<u8>,
        layout: Layout,
        regions: &mut dyn Iterator<Item = Region>,
    ) -> Result<(), Error>;

    /// Release the allocation, regardless of whether finalization succeeded.
    ///
    /// # Safety
    /// The allocation must have come from `allocate` with this layout. No code
    /// may still be executing from or referencing it.
    unsafe fn free(&self, allocation: NonNull<u8>, layout: Layout);
}

/// An installed image. The input ELF bytes may be discarded after loading.
pub struct LoadedImage {
    memory: &'static dyn ExecutableMemory,
    allocation: NonNull<u8>,
    layout: Layout,
    entry_offset: usize,
}

impl LoadedImage {
    /// Validate, copy and relocate the ELF. Reject images whose full virtual
    /// span (including gaps and BSS) exceeds the caller's memory budget.
    pub fn load(
        bytes: &[u8],
        memory: &'static dyn ExecutableMemory,
        max_image_size: usize,
    ) -> Result<Self, Error> {
        let elf = Elf::<Native>::parse(bytes)?;
        elf.check_abi()?;
        if elf.layout.size() > max_image_size {
            return Err(Error::ImageTooLarge);
        }
        let allocation = memory.allocate(elf.layout)?;
        let loaded = Self {
            memory,
            allocation,
            layout: elf.layout,
            entry_offset: elf.entry_offset,
        };
        let bias = allocation.as_ptr().addr().wrapping_sub(elf.start);
        // Initialize raw memory before constructing a byte slice.
        unsafe { allocation.as_ptr().write_bytes(0, elf.layout.size()) };
        let image =
            unsafe { core::slice::from_raw_parts_mut(allocation.as_ptr(), elf.layout.size()) };
        elf.copy_to(image);
        elf.relocate(image, bias)?;
        unsafe { memory.finish_writes(allocation, elf.layout, &mut elf.regions()) }?;
        Ok(loaded)
    }

    pub fn base(&self) -> NonNull<u8> {
        self.allocation
    }

    pub fn entry_address(&self) -> NonNull<u8> {
        // SAFETY: the ELF parser proved the entry lies in the allocation.
        unsafe { self.allocation.add(self.entry_offset) }
    }

    pub fn image_size(&self) -> usize {
        self.layout.size()
    }
}

impl Drop for LoadedImage {
    fn drop(&mut self) {
        unsafe { self.memory.free(self.allocation, self.layout) };
    }
}

#[cfg(any(
    all(
        any(target_arch = "x86_64", target_arch = "aarch64"),
        target_os = "linux"
    ),
    all(
        any(
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "arm",
            target_arch = "riscv32"
        ),
        target_os = "none"
    )
))]
mod run;
#[cfg(any(
    all(
        any(target_arch = "x86_64", target_arch = "aarch64"),
        target_os = "linux"
    ),
    all(
        any(
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "arm",
            target_arch = "riscv32"
        ),
        target_os = "none"
    )
))]
pub use run::Startup;

#[cfg(all(test, target_arch = "x86_64", target_endian = "little"))]
mod tests;
