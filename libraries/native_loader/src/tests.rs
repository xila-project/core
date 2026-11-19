use super::*;

const PH: usize = 64;
const DYNAMIC: usize = 0x120;
const RELOC: usize = 0x190;
const SLOT: usize = 0x1b0;

fn write<const N: usize>(elf: &mut [u8], offset: usize, bytes: [u8; N]) {
    elf[offset..offset + N].copy_from_slice(&bytes);
}

fn fixture(rela: bool) -> [u8; 0x200] {
    let mut elf = [0; 0x200];
    write(&mut elf, 0, *b"\x7fELF\x02\x01\x01");
    write(&mut elf, 16, 3u16.to_le_bytes());
    write(&mut elf, 18, 62u16.to_le_bytes());
    write(&mut elf, 20, 1u32.to_le_bytes());
    write(&mut elf, 24, 0x100u64.to_le_bytes());
    write(&mut elf, 32, (PH as u64).to_le_bytes());
    write(&mut elf, 52, 64u16.to_le_bytes());
    write(&mut elf, 54, 56u16.to_le_bytes());
    write(&mut elf, 56, 3u16.to_le_bytes());

    for (index, (kind, flags, offset, size, memory)) in [
        (1u32, 5u32, 0, 0x110, 0x110),
        (1, 6, DYNAMIC, 0xc0, 0xe0),
        (2, 6, DYNAMIC, 64, 64),
    ]
    .into_iter()
    .enumerate()
    {
        let header = PH + index * 56;
        write(&mut elf, header, kind.to_le_bytes());
        write(&mut elf, header + 4, flags.to_le_bytes());
        for (field, value) in [
            (8, offset),
            (16, offset),
            (32, size),
            (40, memory),
            (48, 16),
        ] {
            write(&mut elf, header + field, (value as u64).to_le_bytes());
        }
    }

    let entry_size = if rela { 24u64 } else { 16 };
    let tag = if rela { 7u64 } else { 17 };
    for (index, (tag, value)) in [
        (tag, RELOC as u64),
        (tag + 1, entry_size),
        (tag + 2, entry_size),
    ]
    .into_iter()
    .enumerate()
    {
        write(&mut elf, DYNAMIC + index * 16, tag.to_le_bytes());
        write(&mut elf, DYNAMIC + index * 16 + 8, value.to_le_bytes());
    }
    write(&mut elf, RELOC, (SLOT as u64).to_le_bytes());
    write(&mut elf, RELOC + 8, 8u64.to_le_bytes());
    write(
        &mut elf,
        if rela { RELOC + 16 } else { SLOT },
        0x100u64.to_le_bytes(),
    );
    elf
}

#[test]
fn rejects_malformed_elfs() {
    let good = fixture(true);
    for end in 0..0x1e0 {
        assert!(
            Elf::parse(&good[..end]).is_err(),
            "accepted truncation {end:#x}"
        );
    }
    let cases: &[(usize, &[u8], Error)] = &[
        (4, &[1], Error::WrongClass),
        (5, &[2], Error::WrongEndian),
        (18, &[40], Error::WrongMachine),
        (
            PH + 56 + 16,
            &0x100u64.to_le_bytes(),
            Error::OverlappingSegments,
        ),
        (PH + 56 + 32, &0xffu64.to_le_bytes(), Error::InvalidSegment),
        (PH + 56 + 48, &[3], Error::InvalidAlignment),
        (RELOC + 8, &[1], Error::UnsupportedRelocation(1)),
        (RELOC + 12, &[1], Error::SymbolRelocation),
        (
            RELOC,
            &0x110u64.to_le_bytes(),
            Error::InvalidRelocationTarget,
        ),
        (DYNAMIC, &[1], Error::DynamicDependency),
    ];
    for &(offset, bytes, expected) in cases {
        let mut bad = good;
        bad[offset..offset + bytes.len()].copy_from_slice(bytes);
        assert_eq!(
            Elf::parse(&bad).and_then(|elf| elf.relocate(&mut [0; 0x200], 0)),
            Err(expected)
        );
    }
}

#[test]
fn copies_and_relocates_both_formats() {
    for rela in [true, false] {
        let file = fixture(rela);
        let elf = Elf::parse(&file).unwrap();
        let mut image = [0; 0x200];
        elf.copy_to(&mut image);
        elf.relocate(&mut image, 0x5000).unwrap();
        assert_eq!(&image[SLOT..SLOT + 8], &0x5100u64.to_le_bytes());
        assert_eq!(&image[0x110..0x120], &[0; 16]);
        assert_eq!(&image[0x1e0..], &[0; 32]);
    }
}

#[test]
fn byte_mutations_do_not_panic() {
    let file = fixture(true);
    for index in 0..file.len() {
        for byte in [0, 0x55, 0xff] {
            let mut mutated = file;
            mutated[index] = byte;
            if let Ok(elf) = Elf::parse(&mutated) {
                let _ = elf.check_abi();
                if elf.layout.size() <= file.len() {
                    let mut image = [0; 0x200];
                    elf.copy_to(&mut image);
                    let _ = elf.relocate(&mut image, 0x5000);
                }
            }
        }
    }
}
