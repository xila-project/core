use crate::elf::{Abi, Elf, Native, SHF_ALLOC, section_header_size, subrange, u16_at, u32_at};

const WORD: usize = Native::WORD;
const HEADER: usize = Native::HEADER;
const SECTION: usize = section_header_size::<Native>();
const SYMBOL: usize = if WORD == 8 { 24 } else { 16 };

const SECTION_FLAGS: usize = 8;
const SECTION_ADDRESS: usize = 8 + WORD;
const SECTION_OFFSET: usize = 8 + 2 * WORD;
const SECTION_SIZE: usize = 8 + 3 * WORD;
const SECTION_LINK: usize = 8 + 4 * WORD;
const SECTION_ALIGNMENT: usize = SECTION_LINK + 8;
const SYMBOL_VALUE: usize = if WORD == 8 { 8 } else { 4 };
const SYMBOL_SECTION: usize = if WORD == 8 { 6 } else { 14 };
const HEADER_SIZES: usize = 28 + 3 * WORD;

const SHT_SYMTAB: u32 = 2;
const SHT_STRTAB: u32 = 3;
const SHT_NOBITS: u32 = 8;
const SHN_LORESERVE: usize = 0xff00;
const SHN_XINDEX: usize = 0xffff;

/// A view of the input tables. Their sizes determine the exact output allocation.
pub(super) struct SymbolFile<'a> {
    header: &'a [u8],
    sections: &'a [u8],
    symbols: (usize, &'a [u8]),
    symbol_names: (usize, &'a [u8]),
    section_names: (usize, &'a [u8]),
}

impl<'a> SymbolFile<'a> {
    pub(super) fn new(elf: &Elf<'a, Native>) -> Option<Self> {
        let (bytes, sections) = elf.symbol_file_input()?;
        let header = &bytes[..HEADER];
        let section_count = sections.len() / SECTION;
        // This symbol file uses ordinary section indices.
        if section_count == 0 || section_count >= SHN_LORESERVE {
            return None;
        }
        let section_data = |index, kind| {
            let section = section_at(sections, index)?;
            if u32_at(section, 4) != kind {
                return None;
            }
            subrange(
                bytes,
                Native::word_at(section, SECTION_OFFSET),
                Native::word_at(section, SECTION_SIZE),
            )
        };
        let names_index = usize::from(u16_at(header, HEADER_SIZES + 10));
        let section_names = (names_index, section_data(names_index, SHT_STRTAB)?);
        let (index, section) = sections
            .chunks_exact(SECTION)
            .enumerate()
            .find(|(_, section)| u32_at(section, 4) == SHT_SYMTAB)?;
        let symbols = section_data(index, SHT_SYMTAB)?;
        if symbols.len() <= SYMBOL
            || !symbols.len().is_multiple_of(SYMBOL)
            || Native::word_at(section, SECTION_ALIGNMENT + WORD) != SYMBOL
            || u32_at(section, SECTION_LINK + 4) as usize > symbols.len() / SYMBOL
        {
            return None;
        }
        let names_index = u32_at(section, SECTION_LINK) as usize;
        let symbol_names = (names_index, section_data(names_index, SHT_STRTAB)?);
        Some(Self {
            header,
            sections,
            symbols: (index, symbols),
            symbol_names,
            section_names,
        })
    }

    pub(super) fn size(&self) -> Option<usize> {
        (HEADER + self.sections.len())
            .checked_add(self.symbols.1.len())?
            .checked_add(self.symbol_names.1.len())?
            .checked_add(if self.symbol_names.0 == self.section_names.0 {
                0
            } else {
                self.section_names.1.len()
            })
    }

    pub(super) fn write(
        &self,
        output: &mut [u8],
        elf: &Elf<'_, Native>,
        data_base: usize,
        exec_base: usize,
    ) -> Option<()> {
        let (header, tail) = output.split_at_mut(HEADER);
        let (sections, contents) = tail.split_at_mut(self.sections.len());
        header.copy_from_slice(self.header);
        header[16..18].copy_from_slice(&2u16.to_ne_bytes()); // ET_EXEC, with absolute symbols
        header[24..24 + 2 * WORD].fill(0); // no entry point or program header table
        Native::put_word(header, 24 + 2 * WORD, HEADER);
        header[HEADER_SIZES + 2..HEADER_SIZES + 6].fill(0);
        header[HEADER_SIZES + 8..HEADER_SIZES + 10]
            .copy_from_slice(&((self.sections.len() / SECTION) as u16).to_ne_bytes());

        // Keep section indices so symbols and the string tables can use their
        // original indices. Omit all section contents except the three tables.
        let mut offset = HEADER + sections.len();
        for (index, bytes) in [self.symbols, self.symbol_names, self.section_names] {
            let target = &mut sections[index * SECTION..][..SECTION];
            if u32_at(target, 4) != 0 {
                continue; // The two string tables can share one section.
            }
            target.copy_from_slice(section_at(self.sections, index)?);
            Native::put_word(target, SECTION_OFFSET, offset);
            let start = offset - HEADER - self.sections.len();
            contents[start..start + bytes.len()].copy_from_slice(bytes);
            offset += bytes.len();
        }

        for (source, target) in self
            .sections
            .chunks_exact(SECTION)
            .zip(sections.chunks_exact_mut(SECTION))
            .skip(1)
        {
            let flags = Native::word_at(source, SECTION_FLAGS);
            if flags & SHF_ALLOC == 0 || u32_at(target, 4) != 0 {
                continue;
            }
            let address = Native::word_at(source, SECTION_ADDRESS);
            let size = Native::word_at(source, SECTION_SIZE);
            let Some(offset) = address.checked_sub(elf.start) else {
                continue;
            };
            let Some(region) = elf.regions().find(|region| {
                offset
                    .checked_sub(region.offset)
                    .and_then(|start| start.checked_add(size))
                    .is_some_and(|end| end <= region.size)
            }) else {
                continue;
            };
            let base = if region.executable {
                exec_base
            } else {
                data_base
            };
            target[..4].copy_from_slice(&source[..4]); // name
            target[4..8].copy_from_slice(&SHT_NOBITS.to_ne_bytes());
            // Only keep write, allocate and execute flags on placeholder sections.
            Native::put_word(target, SECTION_FLAGS, flags & 7);
            Native::put_word(target, SECTION_ADDRESS, base.checked_add(offset)?);
            Native::put_word(target, SECTION_SIZE, size);
            Native::put_word(
                target,
                SECTION_ALIGNMENT,
                Native::word_at(source, SECTION_ALIGNMENT),
            );
        }

        for symbol in contents[..self.symbols.1.len()].chunks_exact_mut(SYMBOL) {
            let index = usize::from(u16_at(symbol, SYMBOL_SECTION));
            if index == SHN_XINDEX {
                return None;
            }
            if index == 0 || index >= SHN_LORESERVE {
                continue; // Undefined and absolute symbols need no relocation.
            }
            let target = section_at(sections, index)?;
            if u32_at(target, 4) != SHT_NOBITS {
                // A definition in a discarded section has no runtime address.
                symbol[SYMBOL_SECTION..SYMBOL_SECTION + 2].fill(0);
                Native::put_word(symbol, SYMBOL_VALUE, 0);
                continue;
            }
            let source = section_at(self.sections, index)?;
            let relative = Native::word_at(symbol, SYMBOL_VALUE)
                .checked_sub(Native::word_at(source, SECTION_ADDRESS))?;
            let address = Native::word_at(target, SECTION_ADDRESS).checked_add(relative)?;
            Native::put_word(symbol, SYMBOL_VALUE, address);
        }
        Some(())
    }
}

fn section_at(sections: &[u8], index: usize) -> Option<&[u8]> {
    subrange(sections, index.checked_mul(SECTION)?, SECTION)
}
