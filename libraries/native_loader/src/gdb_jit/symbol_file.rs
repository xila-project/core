use crate::elf::{Abi, Arch, Elf, SHF_ALLOC, section_header_size, subrange, u16_at, u32_at};

const SHT_SYMTAB: u32 = 2;
const SHT_STRTAB: u32 = 3;
const SHT_NOBITS: u32 = 8;
const SHN_LORESERVE: usize = 0xff00;
const TABLE_NAMES: &[u8] = b".symtab\0.strtab\0.shstrtab\0";

struct Symbol<'a> {
    name: &'a [u8],
    section: u16,
    offset: usize,
    local: bool,
}

/// Counts the output tables before the registration allocates them.
pub(crate) struct SymbolFile<'a, A: Abi> {
    elf: &'a Elf<'a, A>,
    header: &'a [u8],
    sections: &'a [u8],
    symbols: &'a [u8],
    symbol_names: &'a [u8],
    section_names: &'a [u8],
    mapped_sections: usize,
    symbol_count: usize,
    local_count: usize,
    name_bytes: usize,
}

impl<'a, A: Abi> SymbolFile<'a, A> {
    const SECTION: usize = section_header_size::<A>();
    const SYMBOL: usize = if A::WORD == 8 { 24 } else { 16 };
    const SECTION_ADDRESS: usize = 8 + A::WORD;
    const SECTION_OFFSET: usize = 8 + 2 * A::WORD;
    const SECTION_SIZE: usize = 8 + 3 * A::WORD;
    const SECTION_LINK: usize = 8 + 4 * A::WORD;
    const SECTION_INFO: usize = Self::SECTION_LINK + 4;
    const SECTION_ALIGNMENT: usize = Self::SECTION_LINK + 8;
    const SECTION_ENTRY_SIZE: usize = Self::SECTION_ALIGNMENT + A::WORD;
    const SYMBOL_VALUE: usize = if A::WORD == 8 { 8 } else { 4 };
    const SYMBOL_SIZE: usize = if A::WORD == 8 { 16 } else { 8 };
    const SYMBOL_INFO: usize = if A::WORD == 8 { 4 } else { 12 };
    const SYMBOL_SECTION: usize = if A::WORD == 8 { 6 } else { 14 };
    const HEADER_SIZES: usize = 28 + 3 * A::WORD;

    pub(crate) fn new(elf: &'a Elf<'a, A>) -> Option<Self> {
        let (bytes, sections) = elf.symbol_file_input()?;
        let header = &bytes[..A::HEADER];
        let section_count = sections.len() / Self::SECTION;
        if section_count == 0 || section_count >= SHN_LORESERVE {
            return None;
        }
        let section_data = |section: &[u8], kind| {
            if u32_at(section, 4) != kind {
                return None;
            }
            subrange(
                bytes,
                A::word_at(section, Self::SECTION_OFFSET),
                A::word_at(section, Self::SECTION_SIZE),
            )
        };
        let names_index = u16_at(header, Self::HEADER_SIZES + 10);
        let section_names = section_data(Self::section(sections, names_index)?, SHT_STRTAB)?;
        let symbol_section = sections
            .chunks_exact(Self::SECTION)
            .find(|section| u32_at(section, 4) == SHT_SYMTAB)?;
        let symbols = section_data(symbol_section, SHT_SYMTAB)?;
        if symbols.len() <= Self::SYMBOL
            || !symbols.len().is_multiple_of(Self::SYMBOL)
            || A::word_at(symbol_section, Self::SECTION_ENTRY_SIZE) != Self::SYMBOL
        {
            return None;
        }
        let names_index = u16::try_from(u32_at(symbol_section, Self::SECTION_LINK)).ok()?;
        let symbol_names = section_data(Self::section(sections, names_index)?, SHT_STRTAB)?;
        if section_names.first() != Some(&0) {
            return None;
        }
        let mut file = Self {
            elf,
            header,
            sections,
            symbols,
            symbol_names,
            section_names,
            mapped_sections: 0,
            symbol_count: 1, // The mandatory null symbol is local.
            local_count: 1,
            name_bytes: 1, // String offset zero holds the empty name.
        };
        file.mapped_sections = sections
            .chunks_exact(Self::SECTION)
            .skip(1)
            .filter(|section| file.mapped_section(section).is_some())
            .count();
        if file.mapped_sections + 4 >= SHN_LORESERVE {
            return None;
        }
        for source in symbols.chunks_exact(Self::SYMBOL).skip(1) {
            if let Some(symbol) = file.symbol(source) {
                file.symbol_count += 1;
                file.local_count += usize::from(symbol.local);
                file.name_bytes = file.name_bytes.checked_add(symbol.name.len())?;
            }
        }
        (file.symbol_count > 1).then_some(file)
    }

    pub(crate) fn size(&self) -> Option<usize> {
        let size = A::HEADER
            .checked_add((self.mapped_sections + 4) * Self::SECTION)?
            .checked_add(self.symbol_count * Self::SYMBOL)?
            .checked_add(self.name_bytes)?
            .checked_add(self.section_names.len())?
            .checked_add(TABLE_NAMES.len())?;
        (u32::try_from(size).is_ok()).then_some(size)
    }

    /// Fill a zeroed buffer of the size returned by `size`.
    pub(crate) fn write(
        &self,
        output: &mut [u8],
        data_base: usize,
        exec_base: usize,
    ) -> Option<()> {
        let (header, tail) = output.split_at_mut(A::HEADER);
        let (sections, contents) = tail.split_at_mut((self.mapped_sections + 4) * Self::SECTION);
        let (symbols, names) = contents.split_at_mut(self.symbol_count * Self::SYMBOL);
        let (symbol_names, section_names) = names.split_at_mut(self.name_bytes);
        let symbol_index = self.mapped_sections + 1;
        let symbol_names_index = symbol_index + 1;
        let section_names_index = symbol_index + 2;

        header.copy_from_slice(self.header);
        put_u16(header, 16, 2); // ET_EXEC stores absolute symbol addresses.
        header[24..24 + 2 * A::WORD].fill(0); // No entry point or program header table.
        A::put_word(header, 24 + 2 * A::WORD, A::HEADER);
        header[Self::HEADER_SIZES + 2..Self::HEADER_SIZES + 6].fill(0);
        put_u16(
            header,
            Self::HEADER_SIZES + 8,
            (section_names_index + 1) as u16,
        );
        put_u16(header, Self::HEADER_SIZES + 10, section_names_index as u16);

        let mapped = &mut sections[Self::SECTION..symbol_index * Self::SECTION];
        let mut targets = mapped.chunks_exact_mut(Self::SECTION);
        for (index, source) in self
            .sections
            .chunks_exact(Self::SECTION)
            .enumerate()
            .skip(1)
        {
            let Some((offset, executable)) = self.mapped_section(source) else {
                continue;
            };
            let target = targets.next()?;
            let base = if executable { exec_base } else { data_base };
            target[..4].copy_from_slice(&source[..4]);
            put_u32(target, 4, SHT_NOBITS);
            A::put_word(target, 8, A::word_at(source, 8) & 7);
            A::put_word(target, Self::SECTION_ADDRESS, base.checked_add(offset)?);
            A::put_word(
                target,
                Self::SECTION_SIZE,
                A::word_at(source, Self::SECTION_SIZE),
            );
            A::put_word(
                target,
                Self::SECTION_ALIGNMENT,
                A::word_at(source, Self::SECTION_ALIGNMENT),
            );
            // Save the input index in the output header until symbols are remapped.
            put_u32(target, Self::SECTION_INFO, index as u32);
        }

        let (mut local, mut global, mut name_offset) = (1, self.local_count, 1);
        for source in self.symbols.chunks_exact(Self::SYMBOL).skip(1) {
            let Some(symbol) = self.symbol(source) else {
                continue;
            };
            let index = mapped.chunks_exact(Self::SECTION).position(|section| {
                u32_at(section, Self::SECTION_INFO) == u32::from(symbol.section)
            })?;
            let section = &mapped[index * Self::SECTION..][..Self::SECTION];
            let address = A::word_at(section, Self::SECTION_ADDRESS).checked_add(symbol.offset)?;
            let cursor = if symbol.local {
                &mut local
            } else {
                &mut global
            };
            let target = &mut symbols[*cursor * Self::SYMBOL..][..Self::SYMBOL];
            target.copy_from_slice(source);
            put_u32(target, 0, name_offset as u32);
            put_u16(target, Self::SYMBOL_SECTION, (index + 1) as u16);
            A::put_word(target, Self::SYMBOL_VALUE, address);
            symbol_names[name_offset..][..symbol.name.len()].copy_from_slice(symbol.name);
            *cursor += 1;
            name_offset += symbol.name.len();
        }
        for section in mapped.chunks_exact_mut(Self::SECTION) {
            put_u32(section, Self::SECTION_INFO, 0);
        }

        let mut offset = A::HEADER + sections.len();
        let mut name = self.section_names.len();
        for (index, kind, size, name_size) in [
            (symbol_index, SHT_SYMTAB, symbols.len(), b".symtab\0".len()),
            (
                symbol_names_index,
                SHT_STRTAB,
                symbol_names.len(),
                b".strtab\0".len(),
            ),
            (
                section_names_index,
                SHT_STRTAB,
                section_names.len(),
                b".shstrtab\0".len(),
            ),
        ] {
            let section = &mut sections[index * Self::SECTION..][..Self::SECTION];
            put_u32(section, 0, name as u32);
            put_u32(section, 4, kind);
            A::put_word(section, Self::SECTION_OFFSET, offset);
            A::put_word(section, Self::SECTION_SIZE, size);
            A::put_word(
                section,
                Self::SECTION_ALIGNMENT,
                if kind == SHT_SYMTAB { A::WORD } else { 1 },
            );
            offset += size;
            name += name_size;
        }
        let symtab = &mut sections[symbol_index * Self::SECTION..][..Self::SECTION];
        put_u32(symtab, Self::SECTION_LINK, symbol_names_index as u32);
        put_u32(symtab, Self::SECTION_INFO, self.local_count as u32);
        A::put_word(symtab, Self::SECTION_ENTRY_SIZE, Self::SYMBOL);
        let (original, added) = section_names.split_at_mut(self.section_names.len());
        original.copy_from_slice(self.section_names);
        added.copy_from_slice(TABLE_NAMES);
        Some(())
    }

    fn section(sections: &[u8], index: u16) -> Option<&[u8]> {
        subrange(sections, usize::from(index) * Self::SECTION, Self::SECTION)
    }

    fn mapped_section(&self, section: &[u8]) -> Option<(usize, bool)> {
        if A::word_at(section, 8) & SHF_ALLOC == 0 {
            return None;
        }
        let offset = A::word_at(section, Self::SECTION_ADDRESS).checked_sub(self.elf.start)?;
        let size = A::word_at(section, Self::SECTION_SIZE);
        let region = self.elf.regions().find(|region| {
            offset
                .checked_sub(region.offset)
                .and_then(|start| start.checked_add(size))
                .is_some_and(|end| end <= region.size)
        })?;
        Some((offset, region.executable))
    }

    fn symbol(&self, source: &[u8]) -> Option<Symbol<'a>> {
        let info = source[Self::SYMBOL_INFO];
        let kind = info & 15;
        let size = A::word_at(source, Self::SYMBOL_SIZE);
        if !matches!(kind, 1 | 2) || size == 0 {
            return None; // Keep only sized object and function symbols.
        }
        let index = u16_at(source, Self::SYMBOL_SECTION);
        if index == 0 || usize::from(index) >= SHN_LORESERVE {
            return None;
        }
        let section = Self::section(self.sections, index)?;
        self.mapped_section(section)?;
        let value = A::word_at(source, Self::SYMBOL_VALUE);
        let section_address = A::word_at(section, Self::SECTION_ADDRESS);
        let offset = value.checked_sub(section_address)?;
        // Ignore the Thumb selector for the bounds check. Preserve it in st_value.
        let address = if A::ARCH == Arch::Arm && kind == 2 {
            value & !1
        } else {
            value
        };
        if address.checked_sub(section_address)?.checked_add(size)?
            > A::word_at(section, Self::SECTION_SIZE)
        {
            return None;
        }
        let name = self.symbol_names.get(u32_at(source, 0) as usize..)?;
        let length = name.iter().position(|byte| *byte == 0)?;
        (length != 0).then_some(Symbol {
            name: &name[..=length],
            section: index,
            offset,
            local: info >> 4 == 0,
        })
    }
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_ne_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_ne_bytes());
}
