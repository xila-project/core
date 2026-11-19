use crate::{ELF_MAGIC, Error, Region};
use core::alloc::Layout;

const ELF64: bool = usize::BITS == 64;
const WORD: usize = size_of::<usize>();
const HEADER: usize = if ELF64 { 64 } else { 52 };
const PROGRAM_HEADER: usize = if ELF64 { 56 } else { 32 };
const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_NOTE: u32 = 4;
const PF_X: u32 = 1;
const PF_W: u32 = 2;

const TARGET: (u16, u32) = if cfg!(target_arch = "x86_64") {
    (62, 8)
} else if cfg!(target_arch = "aarch64") {
    (183, 1027)
} else if cfg!(target_arch = "arm") {
    (40, 23)
} else if cfg!(target_arch = "riscv32") {
    (243, 3)
} else if cfg!(target_arch = "xtensa") {
    (94, 5)
} else {
    (0, 0)
};

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
    fn read(bytes: &[u8]) -> Self {
        let (flags, offset, address, file_size, memory_size, align) = if ELF64 {
            (4, 8, 16, 32, 40, 48)
        } else {
            (24, 4, 8, 16, 20, 28)
        };
        Self {
            kind: u32_at(bytes, 0),
            flags: u32_at(bytes, flags),
            offset: word_at(bytes, offset),
            address: word_at(bytes, address),
            file_size: word_at(bytes, file_size),
            memory_size: word_at(bytes, memory_size),
            align: word_at(bytes, align),
        }
    }

    fn contains(self, address: usize, size: usize, limit: usize) -> bool {
        address
            .checked_sub(self.address)
            .and_then(|offset| offset.checked_add(size))
            .is_some_and(|end| end <= limit)
    }
}

pub(crate) struct Elf<'a> {
    bytes: &'a [u8],
    headers: &'a [u8],
    dynamic: &'a [u8],
    pub(crate) start: usize,
    pub(crate) layout: Layout,
    pub(crate) entry_offset: usize,
}

impl<'a> Elf<'a> {
    pub(crate) fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        if !bytes.starts_with(ELF_MAGIC) {
            return Err(Error::InvalidHeader);
        }
        let ident = bytes.get(..16).ok_or(Error::Truncated)?;
        if ident[4] != if ELF64 { 2 } else { 1 } {
            return Err(Error::WrongClass);
        }
        if ident[5] != if cfg!(target_endian = "little") { 1 } else { 2 } {
            return Err(Error::WrongEndian);
        }
        let header = bytes.get(..HEADER).ok_or(Error::Truncated)?;
        if ident[6] != 1 || !matches!(ident[7], 0 | 3) || ident[8] != 0 || u32_at(header, 20) != 1 {
            return Err(Error::InvalidHeader);
        }
        if u16_at(header, 16) != 3 {
            return Err(Error::NotPositionIndependent);
        }
        if TARGET.0 == 0 || u16_at(header, 18) != TARGET.0 {
            return Err(Error::WrongMachine);
        }
        let sizes = if ELF64 { 52 } else { 40 };
        let phoff = word_at(header, 24 + WORD);
        let count = usize::from(u16_at(header, sizes + 4));
        if usize::from(u16_at(header, sizes)) != HEADER
            || usize::from(u16_at(header, sizes + 2)) != PROGRAM_HEADER
            || phoff < HEADER
            || count == 0
            || count == usize::from(u16::MAX)
        {
            return Err(Error::InvalidHeader);
        }
        let headers = subrange(bytes, phoff, count * PROGRAM_HEADER).ok_or(Error::Truncated)?;
        let entry = word_at(header, 24);
        // The ARM ELF entry carries the instruction-set selector in bit zero.
        let code = if cfg!(target_arch = "arm") {
            entry & !1
        } else {
            entry
        };
        if cfg!(all(target_arch = "arm", target_os = "none")) && entry == code {
            return Err(Error::InvalidEntry);
        }
        let instruction_alignment =
            if cfg!(target_arch = "aarch64") || cfg!(target_arch = "arm") && entry == code {
                4
            } else if cfg!(any(target_arch = "arm", target_arch = "riscv32")) {
                2
            } else {
                1
            };

        let mut start = usize::MAX;
        let mut end = 0;
        let mut align = instruction_alignment;
        let mut entry_found = false;
        let mut dynamic = None;
        for (index, program) in programs(headers).enumerate() {
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
                    for other in programs(&headers[..index * PROGRAM_HEADER])
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
            dynamic: &[],
            start,
            layout,
            entry_offset: entry - start,
        };
        if let Some(program) = dynamic {
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
            elf.dynamic = dynamic;
        }
        Ok(elf)
    }

    pub(crate) fn check_abi(&self) -> Result<(), Error> {
        let mut version = None;
        for note in programs(self.headers).filter(|program| program.kind == PT_NOTE) {
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
        programs(self.headers).filter(|program| program.kind == PT_LOAD && program.memory_size != 0)
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
        if self.dynamic.is_empty() {
            return Ok(());
        }
        let mut rel = Relocations::default();
        let mut rela = Relocations::default();
        let mut terminated = false;
        for entry in self.dynamic.chunks_exact(2 * WORD) {
            let tag = word_at(entry, 0);
            let value = word_at(entry, WORD);
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
                // Hash/string/symbol metadata, version definitions, relative counts, flags,
                // constructor metadata and the debugger rendezvous slot are not consumed.
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
        if !terminated || !self.dynamic.len().is_multiple_of(2 * WORD) {
            return Err(Error::InvalidDynamic);
        }
        for (table, explicit) in [(rel, false), (rela, true)] {
            let stride = if explicit { 3 * WORD } else { 2 * WORD };
            for entry in table.bytes(self, stride)?.chunks_exact(stride) {
                let target = word_at(entry, 0);
                let info = word_at(entry, WORD);
                let (symbol, kind) = if ELF64 {
                    (info >> (usize::BITS - 32), info as u32)
                } else {
                    (info >> 8, (info & 0xff) as u32)
                };
                if kind != TARGET.1 {
                    return Err(Error::UnsupportedRelocation(kind));
                }
                if symbol != 0 {
                    return Err(Error::SymbolRelocation);
                }
                if !self
                    .loads()
                    .any(|program| program.contains(target, WORD, program.memory_size))
                {
                    return Err(Error::InvalidRelocationTarget);
                }
                let offset = target - self.start;
                // Xtensa uses the literal's in-place addend even for RELA.
                let addend = if explicit {
                    let addend = word_at(entry, 2 * WORD);
                    if cfg!(target_arch = "xtensa") {
                        if addend != 0 {
                            return Err(Error::InvalidRelocationAddend);
                        }
                        word_at(image, offset)
                    } else {
                        addend
                    }
                } else {
                    word_at(image, offset)
                };
                image[offset..offset + WORD]
                    .copy_from_slice(&bias.wrapping_add(addend).to_ne_bytes());
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct Relocations {
    address: Option<usize>,
    size: Option<usize>,
    entry_size: Option<usize>,
}

impl Relocations {
    fn bytes<'a>(&self, elf: &Elf<'a>, stride: usize) -> Result<&'a [u8], Error> {
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

fn programs(headers: &[u8]) -> impl Iterator<Item = Program> {
    headers.chunks_exact(PROGRAM_HEADER).map(Program::read)
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

fn word_at(bytes: &[u8], offset: usize) -> usize {
    usize::from_ne_bytes(bytes[offset..offset + WORD].try_into().unwrap())
}
