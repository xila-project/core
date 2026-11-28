//! Load trusted, statically linked position-independent native applications.
#![no_std]

mod elf;
mod lifecycle;
#[cfg(any(target_arch = "riscv32", test))]
mod riscv;

use core::{alloc::Layout, ptr::NonNull};
use elf::{Elf, Native};
pub use xila_version::Version;

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
    /// The image was built for a Xila version that the running one does not run
    /// (see [`xila_version::compatible`]).
    IncompatibleXilaVersion {
        declared: Version,
        running: Version,
    },
    InvalidDynamic,
    InvalidSectionHeaders,
    InvalidRiscvAttributes,
    IncompatibleRiscvArchitecture,
    InvalidLifecycleArray,
    InvalidLifecycleEntry,
    /// Legacy DT_INIT or DT_FINI function.
    Constructors,
    DynamicDependency,
    UnsupportedDynamicTag(usize),
    InvalidRelocationTable,
    InvalidRelocationTarget,
    InvalidRelocationAddend,
    UnsupportedRelocation(u32),
    UnsupportedSplitAddressSpace,
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
/// layout. On unified-address platforms, the same virtual addresses are used
/// for data access and execution ([`Self::execution_base`] returns
/// `allocation`). On split-bus (Harvard) platforms where instruction fetch
/// uses a separate address alias of the same physical memory,
/// [`Self::execution_base`] returns that alias with at least the same
/// alignment and size as `allocation`, valid until [`Self::free`].
/// Finalization must make written instructions visible to instruction fetch,
/// apply permissions supported by the platform and preserve region contents.
/// Memory must remain mapped until freed, including on finalization failure.
pub unsafe trait ExecutableMemory: Sync {
    fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, Error>;

    /// Instruction-fetch base address aliasing `allocation`.
    ///
    /// Unified-address platforms inherit the default, which returns
    /// `allocation`. Split-bus platforms (such as ESP32-S2/S3 internal D/IRAM)
    /// override this to return the instruction-bus alias of the same physical
    /// bytes.
    fn execution_base(&self, allocation: NonNull<u8>) -> NonNull<u8> {
        allocation
    }

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
    execution_base: NonNull<u8>,
    layout: Layout,
    entry_offset: usize,
    lifecycle: lifecycle::Lifecycle,
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
        let execution_base = memory.execution_base(allocation);
        let loaded = Self {
            memory,
            allocation,
            execution_base,
            layout: elf.layout,
            entry_offset: elf.entry_offset,
            lifecycle: lifecycle::Lifecycle::new(allocation, &elf.arrays),
        };
        let data_addr = allocation.as_ptr().addr();
        let exec_addr = execution_base.as_ptr().addr();
        if exec_addr != data_addr
            && (!exec_addr.is_multiple_of(elf.layout.align())
                || exec_addr.checked_add(elf.layout.size()).is_none())
        {
            return Err(Error::AllocationFailed);
        }
        let data_bias = data_addr.wrapping_sub(elf.start);
        let execution_bias = exec_addr.wrapping_sub(elf.start);
        // Initialize raw memory before constructing a byte slice.
        unsafe { allocation.as_ptr().write_bytes(0, elf.layout.size()) };
        let image =
            unsafe { core::slice::from_raw_parts_mut(allocation.as_ptr(), elf.layout.size()) };
        elf.copy_to(image);
        elf.relocate(image, data_bias, execution_bias)?;
        elf.check_array_entries(image, exec_addr)?;
        unsafe { memory.finish_writes(allocation, elf.layout, &mut elf.regions()) }?;
        Ok(loaded)
    }

    pub fn base(&self) -> NonNull<u8> {
        self.allocation
    }

    pub fn execution_base(&self) -> NonNull<u8> {
        self.execution_base
    }

    pub fn entry_address(&self) -> NonNull<u8> {
        // SAFETY: the ELF parser proved the entry lies in the allocation, and
        // `execution_base` aliases the same layout on the instruction bus.
        unsafe { self.execution_base.add(self.entry_offset) }
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
            target_arch = "riscv32",
            target_arch = "xtensa"
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
            target_arch = "riscv32",
            target_arch = "xtensa"
        ),
        target_os = "none"
    )
))]
pub use run::Startup;

#[cfg(all(test, target_arch = "x86_64", target_endian = "little"))]
mod tests;
