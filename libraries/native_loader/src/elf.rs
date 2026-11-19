use crate::{ELF_MAGIC, Error, Region};
use core::{alloc::Layout, marker::PhantomData};

const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_NOTE: u32 = 4;
const PF_X: u32 = 1;
const PF_W: u32 = 2;
/// Largest accepted program header table. Real static PIEs carry fewer than ten.
pub(crate) const MAX_PROGRAM_HEADERS: usize = 16;
/// Largest accepted segment alignment, a power of two. It bounds the padding
/// requested from the platform allocator when an image is linked with the
/// default (megabyte-sized) page alignment by mistake.
pub(crate) const MAX_ALIGNMENT: usize = 64 * 1024;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum Arch {
    X86_64,
    Aarch64,
    Arm,
    Riscv32,
    Xtensa,
    Unsupported,
}

/// ELF class, machine and ABI constants of one target. [`Native`] describes the
/// running target; the tests instantiate the other class on the host.
pub(crate) trait Abi: 'static {
    /// Bytes per ELF word: 4 for ELF32, 8 for ELF64.
    const WORD: usize;
    const MACHINE: u16;
    /// The `R_*_RELATIVE` relocation type.
    const RELATIVE: u32;
    const ARCH: Arch;
    /// Bare-metal Arm cores run Thumb only, so the entry must select Thumb.
    const THUMB_ONLY: bool;
    const HEADER: usize = if Self::WORD == 8 { 64 } else { 52 };
    const PROGRAM_HEADER: usize = if Self::WORD == 8 { 56 } else { 32 };

    /// Whether `e_flags` describes code this target can run.
    fn flags_supported(flags: u32) -> bool;

    fn word_at(bytes: &[u8], offset: usize) -> usize {
        let bytes = &bytes[offset..offset + Self::WORD];
        if Self::WORD == 8 {
            u64::from_ne_bytes(bytes.try_into().unwrap()) as usize
        } else {
            u32::from_ne_bytes(bytes.try_into().unwrap()) as usize
        }
    }

    fn put_word(bytes: &mut [u8], offset: usize, value: usize) {
        let bytes = &mut bytes[offset..offset + Self::WORD];
        if Self::WORD == 8 {
            bytes.copy_from_slice(&(value as u64).to_ne_bytes());
        } else {
            bytes.copy_from_slice(&(value as u32).to_ne_bytes());
        }
    }
}

/// `e_flags` of an Arm EABI image: EABI version 5, little-endian code and a
/// float ABI the target can call. An image that sets neither float ABI flag
/// (for example one whose VFP-args attribute is "compatible") runs on both.
pub(crate) const fn arm_flags_supported(flags: u32, hard_float: bool) -> bool {
    const EF_ARM_BE8: u32 = 0x0080_0000;
    const EF_ARM_ABI_FLOAT_SOFT: u32 = 0x200;
    const EF_ARM_ABI_FLOAT_HARD: u32 = 0x400;
    let float = flags & (EF_ARM_ABI_FLOAT_SOFT | EF_ARM_ABI_FLOAT_HARD);
    flags >> 24 == 5
        && flags & EF_ARM_BE8 == 0
        && (float == 0
            || float
                == if hard_float {
                    EF_ARM_ABI_FLOAT_HARD
                } else {
                    EF_ARM_ABI_FLOAT_SOFT
                })
}

/// The `EF_RISCV_FLOAT_ABI` value of the running target, from the float ABI
/// that `build.rs` detects. `cfg!(target_feature = "f")` is false on stable
/// Rust even for `riscv32imafc`, so it cannot be used here.
const RISCV_FLOAT_ABI: u32 = if cfg!(native_loader_float_abi = "double") {
    4
} else if cfg!(native_loader_float_abi = "single") {
    2
} else {
    0
};

/// Rust targets whose `EF_RISCV_FLOAT_ABI` value is known: 0 soft, 2 single.
#[cfg(target_arch = "riscv32")]
const RISCV_TARGET_FLOAT_ABIS: &[(&str, u32)] = &[
    ("riscv32i-unknown-none-elf", 0),
    ("riscv32im-unknown-none-elf", 0),
    ("riscv32imc-unknown-none-elf", 0),
    ("riscv32ima-unknown-none-elf", 0),
    ("riscv32imac-unknown-none-elf", 0),
    ("riscv32imac-esp-espidf", 0),
    ("riscv32imc-esp-espidf", 0),
    ("riscv32imafc-unknown-none-elf", 2),
];

/// The `EF_RISCV_FLOAT_ABI` value of the Rust target `target`, or `None` for a
/// target the table does not list.
#[cfg(target_arch = "riscv32")]
const fn riscv_target_float_abi(target: &str) -> Option<u32> {
    let mut index = 0;
    while index < RISCV_TARGET_FLOAT_ABIS.len() {
        let (name, abi) = RISCV_TARGET_FLOAT_ABIS[index];
        if str_eq(target, name) {
            return Some(abi);
        }
        index += 1;
    }
    None
}

#[cfg(target_arch = "riscv32")]
const fn str_eq(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}

// Pin the detection of every known RISC-V target, so a wrong float ABI fails
// the build of that target instead of silently rejecting correct images. An
// explicit `-C target-feature` float flag overrides the target.
#[cfg(all(target_arch = "riscv32", not(native_loader_float_flags)))]
const _: () = {
    if let Some(expected) = riscv_target_float_abi(env!("NATIVE_LOADER_TARGET")) {
        assert!(
            RISCV_FLOAT_ABI == expected,
            "the detected RISC-V float ABI does not match the target"
        );
    }
};

/// Whether the target is RV32E. `rustc` reports it as `target_abi = "ilp32e"`,
/// while `cfg!(target_feature = "e")` is always false.
const RISCV_EMBEDDED: bool = cfg!(target_abi = "ilp32e");

/// `e_flags` of a RISC-V image. `float_abi` uses the `EF_RISCV_FLOAT_ABI`
/// encoding: 0 soft, 2 single, 4 double. Compressed code needs a target with
/// the C extension. RVE and TSO images do not run on a plain target.
pub(crate) const fn riscv_flags_supported(
    flags: u32,
    compressed: bool,
    float_abi: u32,
    embedded: bool,
) -> bool {
    const EF_RISCV_RVC: u32 = 0x1;
    const EF_RISCV_FLOAT_ABI: u32 = 0x6;
    const EF_RISCV_RVE: u32 = 0x8;
    flags & !(EF_RISCV_RVC | EF_RISCV_FLOAT_ABI | EF_RISCV_RVE) == 0
        && (compressed || flags & EF_RISCV_RVC == 0)
        && flags & EF_RISCV_FLOAT_ABI == float_abi
        && (flags & EF_RISCV_RVE != 0) == embedded
}

pub(crate) struct Native;

impl Abi for Native {
    const WORD: usize = size_of::<usize>();
    const MACHINE: u16 = if cfg!(target_arch = "x86_64") {
        62
    } else if cfg!(target_arch = "aarch64") {
        183
    } else if cfg!(target_arch = "arm") {
        40
    } else if cfg!(target_arch = "riscv32") {
        243
    } else if cfg!(target_arch = "xtensa") {
        94
    } else {
        0
    };
    const RELATIVE: u32 = if cfg!(target_arch = "x86_64") {
        8
    } else if cfg!(target_arch = "aarch64") {
        1027
    } else if cfg!(target_arch = "arm") {
        23
    } else if cfg!(target_arch = "riscv32") {
        3
    } else {
        5
    };
    const ARCH: Arch = if cfg!(target_arch = "x86_64") {
        Arch::X86_64
    } else if cfg!(target_arch = "aarch64") {
        Arch::Aarch64
    } else if cfg!(target_arch = "arm") {
        Arch::Arm
    } else if cfg!(target_arch = "riscv32") {
        Arch::Riscv32
    } else if cfg!(target_arch = "xtensa") {
        Arch::Xtensa
    } else {
        Arch::Unsupported
    };
    const THUMB_ONLY: bool = cfg!(all(target_arch = "arm", target_os = "none"));

    fn flags_supported(flags: u32) -> bool {
        match Self::ARCH {
            Arch::Arm => arm_flags_supported(flags, cfg!(target_abi = "eabihf")),
            Arch::Riscv32 => riscv_flags_supported(
                flags,
                cfg!(target_feature = "c"),
                RISCV_FLOAT_ABI,
                RISCV_EMBEDDED,
            ),
            // Xtensa flags describe the core configuration, which the linker
            // already checked against the toolchain.
            Arch::Xtensa => true,
            _ => flags == 0,
        }
    }
}

#[derive(Clone, Copy)]
struct Program {
    kind: u32,
    flags: u32,
    offset: usize,
    address: usize,
    file_size: usize,
    memory_size: usize,
    align: usize,
}

impl Program {
    fn read<A: Abi>(bytes: &[u8]) -> Self {
        let (flags, offset, address, file_size, memory_size, align) = if A::WORD == 8 {
            (4, 8, 16, 32, 40, 48)
        } else {
            (24, 4, 8, 16, 20, 28)
        };
        Self {
            kind: u32_at(bytes, 0),
            flags: u32_at(bytes, flags),
            offset: A::word_at(bytes, offset),
            address: A::word_at(bytes, address),
            file_size: A::word_at(bytes, file_size),
            memory_size: A::word_at(bytes, memory_size),
            align: A::word_at(bytes, align),
        }
    }

    fn contains(self, address: usize, size: usize, limit: usize) -> bool {
        address
            .checked_sub(self.address)
            .and_then(|offset| offset.checked_add(size))
            .is_some_and(|end| end <= limit)
    }
}

pub(crate) struct Elf<'a, A: Abi> {
    bytes: &'a [u8],
    headers: &'a [u8],
    /// `DT_REL` and `DT_RELA` tables, empty when absent.
    rel: &'a [u8],
    rela: &'a [u8],
    pub(crate) start: usize,
    pub(crate) layout: Layout,
    pub(crate) entry_offset: usize,
    abi: PhantomData<A>,
}

impl<'a, A: Abi> Elf<'a, A> {
    pub(crate) fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        if !bytes.starts_with(ELF_MAGIC) {
            return Err(Error::InvalidHeader);
        }
        let ident = bytes.get(..16).ok_or(Error::Truncated)?;
        if ident[4] != if A::WORD == 8 { 2 } else { 1 } {
            return Err(Error::WrongClass);
        }
        if ident[5] != if cfg!(target_endian = "little") { 1 } else { 2 } {
            return Err(Error::WrongEndian);
        }
        let header = bytes.get(..A::HEADER).ok_or(Error::Truncated)?;
        if ident[6] != 1 || !matches!(ident[7], 0 | 3) || ident[8] != 0 || u32_at(header, 20) != 1 {
            return Err(Error::InvalidHeader);
        }
        if u16_at(header, 16) != 3 {
            return Err(Error::NotPositionIndependent);
        }
        if A::MACHINE == 0 || u16_at(header, 18) != A::MACHINE {
            return Err(Error::WrongMachine);
        }
        let flags_offset = 24 + 3 * A::WORD;
        let flags = u32_at(header, flags_offset);
        if !A::flags_supported(flags) {
            return Err(Error::IncompatibleFlags(flags));
        }
        let sizes = flags_offset + 4;
        let phoff = A::word_at(header, 24 + A::WORD);
        let count = usize::from(u16_at(header, sizes + 4));
        if usize::from(u16_at(header, sizes)) != A::HEADER
            || usize::from(u16_at(header, sizes + 2)) != A::PROGRAM_HEADER
            || phoff < A::HEADER
            || count == 0
            || count > MAX_PROGRAM_HEADERS
        {
            return Err(Error::InvalidHeader);
        }
        let headers = subrange(bytes, phoff, count * A::PROGRAM_HEADER).ok_or(Error::Truncated)?;
        let entry = A::word_at(header, 24);
        // The ARM ELF entry carries the instruction-set selector in bit zero.
        let code = if A::ARCH == Arch::Arm {
            entry & !1
        } else {
            entry
        };
        if A::THUMB_ONLY && entry == code {
            return Err(Error::InvalidEntry);
        }
        let instruction_alignment =
            if A::ARCH == Arch::Aarch64 || A::ARCH == Arch::Arm && entry == code {
                4
            } else if A::ARCH == Arch::Arm || A::ARCH == Arch::Riscv32 {
                2
            } else {
                1
            };

        let mut start = usize::MAX;
        let mut end = 0;
        let mut align = instruction_alignment;
        let mut entry_found = false;
        let mut dynamic = None;
        for (index, program) in programs::<A>(headers).enumerate() {
            match program.kind {
                3 => return Err(Error::Interpreter),
                7 => return Err(Error::ThreadLocalStorage),
                PT_DYNAMIC => {
                    if dynamic.replace(program).is_some() {
                        return Err(Error::InvalidDynamic);
                    }
                }
                PT_LOAD => {
                    if program.align > 1
                        && (!program.align.is_power_of_two()
                            || program.align > MAX_ALIGNMENT
                            || program.offset % program.align != program.address % program.align)
                    {
                        return Err(Error::InvalidAlignment);
                    }
                    if program.file_size > program.memory_size
                        || subrange(bytes, program.offset, program.file_size).is_none()
                    {
                        return Err(Error::InvalidSegment);
                    }
                    let limit = program
                        .address
                        .checked_add(program.memory_size)
                        .ok_or(Error::InvalidSegment)?;
                    if program.memory_size == 0 {
                        continue;
                    }
                    for other in programs::<A>(&headers[..index * A::PROGRAM_HEADER])
                        .filter(|other| other.kind == PT_LOAD && other.memory_size != 0)
                    {
                        if program.address < other.address + other.memory_size
                            && other.address < limit
                        {
                            return Err(Error::OverlappingSegments);
                        }
                    }
                    start = start.min(program.address);
                    end = end.max(limit);
                    align = align.max(program.align);
                    entry_found |= program.flags & PF_X != 0
                        && program.contains(code, instruction_alignment, program.file_size);
                }
                _ => {}
            }
        }
        if !entry_found || !code.is_multiple_of(instruction_alignment) {
            return Err(Error::InvalidEntry);
        }
        // Preserve the congruence of every segment and Thumb entry after rebasing.
        start &= !(align - 1);
        let layout =
            Layout::from_size_align(end - start, align).map_err(|_| Error::InvalidSegment)?;
        let mut elf = Self {
            bytes,
            headers,
            rel: &[],
            rela: &[],
            start,
            layout,
            entry_offset: entry - start,
            abi: PhantomData,
        };
        // A static PIE always has a dynamic section, even without relocations.
        let program = dynamic.ok_or(Error::InvalidDynamic)?;
        if program.file_size == 0 || program.file_size > program.memory_size {
            return Err(Error::InvalidDynamic);
        }
        let dynamic = elf
            .file_range(program.address, program.file_size)
            .ok_or(Error::InvalidDynamic)?;
        if subrange(bytes, program.offset, program.file_size).map(<[u8]>::as_ptr)
            != Some(dynamic.as_ptr())
        {
            return Err(Error::InvalidDynamic);
        }
        elf.read_dynamic(dynamic)?;
        Ok(elf)
    }

    /// Locate the relocation tables and reject everything the loader would
    /// otherwise silently skip, before any memory is allocated.
    fn read_dynamic(&mut self, dynamic: &'a [u8]) -> Result<(), Error> {
        let mut rel = Table::default();
        let mut rela = Table::default();
        let mut terminated = false;
        for entry in dynamic.chunks_exact(2 * A::WORD) {
            let tag = A::word_at(entry, 0);
            let value = A::word_at(entry, A::WORD);
            let field = match tag {
                0 => {
                    terminated = true;
                    break;
                }
                1 => return Err(Error::DynamicDependency),
                7 => &mut rela.address,
                8 => &mut rela.size,
                9 => &mut rela.entry_size,
                17 => &mut rel.address,
                18 => &mut rel.size,
                19 => &mut rel.entry_size,
                // DT_INIT, DT_FINI and the init, fini and preinit arrays: the loader runs
                // no constructors or destructors, so reject them instead of skipping them.
                12 | 13 | 27 | 28 | 33 if value != 0 => return Err(Error::Constructors),
                // Hash/string/symbol metadata, version definitions, relative counts, flags,
                // empty constructor metadata and the debugger rendezvous slot are not consumed.
                4..=6
                | 10..=16
                | 21
                | 22
                | 24..=30
                | 32
                | 33
                | 0x6fff_fef5
                | 0x6fff_fff0
                | 0x6fff_fff9..=0x6fff_fffd => continue,
                _ => return Err(Error::UnsupportedDynamicTag(tag)),
            };
            if field.replace(value).is_some() {
                return Err(Error::InvalidDynamic);
            }
        }
        if !terminated || !dynamic.len().is_multiple_of(2 * A::WORD) {
            return Err(Error::InvalidDynamic);
        }
        self.rel = rel.bytes(self, 2 * A::WORD)?;
        self.rela = rela.bytes(self, 3 * A::WORD)?;
        Ok(())
    }

    pub(crate) fn check_abi(&self) -> Result<(), Error> {
        let mut version = None;
        for note in programs::<A>(self.headers).filter(|program| program.kind == PT_NOTE) {
            let mut bytes =
                subrange(self.bytes, note.offset, note.file_size).ok_or(Error::InvalidAbiNote)?;
            while !bytes.is_empty() {
                let header = bytes.get(..12).ok_or(Error::InvalidAbiNote)?;
                let name_size = u32_at(header, 0) as usize;
                let desc_size = u32_at(header, 4) as usize;
                let name_end = name_size
                    .checked_next_multiple_of(4)
                    .ok_or(Error::InvalidAbiNote)?;
                let desc_end = desc_size
                    .checked_next_multiple_of(4)
                    .ok_or(Error::InvalidAbiNote)?;
                let (name, rest) = bytes[12..]
                    .split_at_checked(name_end)
                    .ok_or(Error::InvalidAbiNote)?;
                let (desc, rest) = rest
                    .split_at_checked(desc_end)
                    .ok_or(Error::InvalidAbiNote)?;
                if u32_at(header, 8) == 1
                    && &name[..name_size] == b"XILA\0"
                    && (desc_size != 4 || version.replace(u32_at(desc, 0)).is_some())
                {
                    return Err(Error::InvalidAbiNote);
                }
                bytes = rest;
            }
        }
        let version = version.ok_or(Error::MissingAbiNote)?;
        let (major, minor) = (version >> 16, version & 0xffff);
        let host_major = abi_application::XILA_ABI_VERSION_MAJOR;
        let host_minor = abi_application::XILA_ABI_VERSION_MINOR;
        if major != host_major || minor > host_minor || (major == 0 && minor != host_minor) {
            return Err(Error::IncompatibleAbi(version));
        }
        Ok(())
    }

    fn loads(&self) -> impl Iterator<Item = Program> + 'a {
        programs::<A>(self.headers)
            .filter(|program| program.kind == PT_LOAD && program.memory_size != 0)
    }

    fn file_range(&self, address: usize, size: usize) -> Option<&'a [u8]> {
        let program = self
            .loads()
            .find(|program| program.contains(address, size, program.file_size))?;
        subrange(
            self.bytes,
            program.offset + (address - program.address),
            size,
        )
    }

    pub(crate) fn regions(&self) -> impl Iterator<Item = Region> + 'a {
        let start = self.start;
        self.loads().map(move |program| Region {
            offset: program.address - start,
            size: program.memory_size,
            executable: program.flags & PF_X != 0,
            writable: program.flags & PF_W != 0,
        })
    }

    pub(crate) fn copy_to(&self, image: &mut [u8]) {
        for program in self.loads() {
            let offset = program.address - self.start;
            image[offset..offset + program.file_size]
                .copy_from_slice(&self.bytes[program.offset..program.offset + program.file_size]);
        }
    }

    pub(crate) fn relocate(&self, image: &mut [u8], bias: usize) -> Result<(), Error> {
        for (table, explicit) in [(self.rel, false), (self.rela, true)] {
            let stride = if explicit { 3 * A::WORD } else { 2 * A::WORD };
            for entry in table.chunks_exact(stride) {
                let target = A::word_at(entry, 0);
                let info = A::word_at(entry, A::WORD);
                let (symbol, kind) = if A::WORD == 8 {
                    (info >> (usize::BITS - 32), info as u32)
                } else {
                    (info >> 8, (info & 0xff) as u32)
                };
                if kind != A::RELATIVE {
                    return Err(Error::UnsupportedRelocation(kind));
                }
                if symbol != 0 {
                    return Err(Error::SymbolRelocation);
                }
                if !self
                    .loads()
                    .any(|program| program.contains(target, A::WORD, program.memory_size))
                {
                    return Err(Error::InvalidRelocationTarget);
                }
                let offset = target - self.start;
                // Xtensa uses the literal's in-place addend even for RELA.
                let addend = if explicit {
                    let addend = A::word_at(entry, 2 * A::WORD);
                    if A::ARCH == Arch::Xtensa {
                        if addend != 0 {
                            return Err(Error::InvalidRelocationAddend);
                        }
                        A::word_at(image, offset)
                    } else {
                        addend
                    }
                } else {
                    A::word_at(image, offset)
                };
                A::put_word(image, offset, bias.wrapping_add(addend));
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct Table {
    address: Option<usize>,
    size: Option<usize>,
    entry_size: Option<usize>,
}

impl Table {
    fn bytes<'a, A: Abi>(&self, elf: &Elf<'a, A>, stride: usize) -> Result<&'a [u8], Error> {
        match (self.address, self.size, self.entry_size) {
            (None, None, None) => Ok(&[]),
            (Some(address), Some(size), Some(entry_size))
                if entry_size == stride && size.is_multiple_of(stride) =>
            {
                if size == 0 {
                    return Ok(&[]);
                }
                elf.file_range(address, size)
                    .ok_or(Error::InvalidRelocationTable)
            }
            _ => Err(Error::InvalidRelocationTable),
        }
    }
}

fn programs<A: Abi>(headers: &[u8]) -> impl Iterator<Item = Program> {
    headers
        .chunks_exact(A::PROGRAM_HEADER)
        .map(Program::read::<A>)
}

fn subrange(bytes: &[u8], offset: usize, size: usize) -> Option<&[u8]> {
    bytes.get(offset..offset.checked_add(size)?)
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_ne_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_ne_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
