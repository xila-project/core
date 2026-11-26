use super::*;
use crate::elf::{
    Abi, Arch, MAX_ALIGNMENT, MAX_PROGRAM_HEADERS, Native, arm_flags_supported,
    riscv_flags_supported,
};

const FILE: usize = 0x200;
const DYNAMIC: usize = 0x120;
const RELOC: usize = 0x190;
const SLOT: usize = 0x1b0;

/// The 32-bit parser runs on the host through these two stand-in targets.
struct Riscv32;
impl Abi for Riscv32 {
    const WORD: usize = 4;
    const MACHINE: u16 = 243;
    const RELATIVE: u32 = 3;
    const ARCH: Arch = Arch::Riscv32;
    const THUMB_ONLY: bool = false;
    fn flags_supported(flags: u32) -> bool {
        riscv_flags_supported(flags, true, 0, false)
    }
}

struct Arm32;
impl Abi for Arm32 {
    const WORD: usize = 4;
    const MACHINE: u16 = 40;
    const RELATIVE: u32 = 23;
    const ARCH: Arch = Arch::Arm;
    const THUMB_ONLY: bool = true;
    fn flags_supported(flags: u32) -> bool {
        arm_flags_supported(flags, true)
    }
}

struct Xtensa32;
impl Abi for Xtensa32 {
    const WORD: usize = 4;
    const MACHINE: u16 = 94;
    const RELATIVE: u32 = 5;
    const ARCH: Arch = Arch::Xtensa;
    const THUMB_ONLY: bool = false;
    fn flags_supported(_flags: u32) -> bool {
        true
    }
}

/// Byte offsets of the fields the tests patch, for one ELF class.
struct Fields {
    word: usize,
    header: usize,
    program: usize,
    flags: usize,
    // Inside a program header.
    offset: usize,
    size: usize,
    memory: usize,
    align: usize,
}

impl Fields {
    fn of<A: Abi>() -> Self {
        let wide = A::WORD == 8;
        Self {
            word: A::WORD,
            header: A::HEADER,
            program: A::PROGRAM_HEADER,
            flags: 24 + 3 * A::WORD,
            offset: if wide { 8 } else { 4 },
            size: if wide { 32 } else { 16 },
            memory: if wide { 40 } else { 20 },
            align: if wide { 48 } else { 28 },
        }
    }

    /// Offset of field `field` of program header `index`.
    fn program_field(&self, index: usize, field: usize) -> usize {
        self.header + index * self.program + field
    }
}

fn write(elf: &mut [u8], offset: usize, bytes: &[u8]) {
    elf[offset..offset + bytes.len()].copy_from_slice(bytes);
}

fn word<A: Abi>(value: usize) -> [u8; 8] {
    // Little-endian: the first `A::WORD` bytes are the word.
    (value as u64).to_le_bytes()
}

fn put<A: Abi>(elf: &mut [u8], offset: usize, value: usize) {
    write(elf, offset, &word::<A>(value)[..A::WORD]);
}

/// A text segment, a data segment and a dynamic section whose single relative
/// relocation (explicit or in-place addend) targets `SLOT`.
fn fixture<A: Abi>(rela: bool) -> [u8; FILE] {
    let fields = Fields::of::<A>();
    let mut elf = [0; FILE];
    write(&mut elf, 0, b"\x7fELF");
    elf[4] = if A::WORD == 8 { 2 } else { 1 };
    elf[5] = 1;
    elf[6] = 1;
    write(&mut elf, 16, &3u16.to_le_bytes());
    write(&mut elf, 18, &A::MACHINE.to_le_bytes());
    write(&mut elf, 20, &1u32.to_le_bytes());
    // Thumb entries carry the instruction-set selector in bit zero.
    put::<A>(&mut elf, 24, 0x100 | usize::from(A::ARCH == Arch::Arm));
    put::<A>(&mut elf, 24 + A::WORD, fields.header);
    let flags = match A::ARCH {
        Arch::Arm => 0x0500_0400,
        _ => 0,
    };
    write(&mut elf, fields.flags, &u32::to_le_bytes(flags));
    write(
        &mut elf,
        fields.flags + 4,
        &(fields.header as u16).to_le_bytes(),
    );
    write(
        &mut elf,
        fields.flags + 6,
        &(fields.program as u16).to_le_bytes(),
    );
    write(&mut elf, fields.flags + 8, &3u16.to_le_bytes());

    let dynamic_size = 5 * 2 * A::WORD;
    for (index, (kind, program_flags, offset, size, memory)) in [
        (1u32, 5u32, 0, 0x110, 0x110),
        (1, 6, DYNAMIC, 0xc0, 0xe0),
        (2, 6, DYNAMIC, dynamic_size, dynamic_size),
    ]
    .into_iter()
    .enumerate()
    {
        let header = fields.program_field(index, 0);
        write(&mut elf, header, &kind.to_le_bytes());
        let flags_at = if A::WORD == 8 { 4 } else { 24 };
        write(&mut elf, header + flags_at, &program_flags.to_le_bytes());
        for (field, value) in [
            (fields.offset, offset),
            (fields.offset + A::WORD, offset),
            (fields.size, size),
            (fields.memory, memory),
            (fields.align, 16),
        ] {
            put::<A>(&mut elf, header + field, value);
        }
    }

    let entry_size = if rela { 3 * A::WORD } else { 2 * A::WORD };
    let tag = if rela { 7 } else { 17 };
    for (index, (tag, value)) in [(tag, RELOC), (tag + 1, entry_size), (tag + 2, entry_size)]
        .into_iter()
        .enumerate()
    {
        put::<A>(&mut elf, DYNAMIC + index * 2 * A::WORD, tag);
        put::<A>(&mut elf, DYNAMIC + index * 2 * A::WORD + A::WORD, value);
    }
    put::<A>(&mut elf, RELOC, SLOT);
    put::<A>(&mut elf, RELOC + A::WORD, A::RELATIVE as usize);
    put::<A>(
        &mut elf,
        if rela { RELOC + 2 * A::WORD } else { SLOT },
        0x100,
    );
    elf
}

fn check<A: Abi>(bytes: &[u8]) -> Result<(), Error> {
    let elf = Elf::<A>::parse(bytes)?;
    elf.relocate(&mut [0; FILE], 0, 0)
}

fn rejects_malformed_elfs<A: Abi>() {
    let fields = Fields::of::<A>();
    let good = fixture::<A>(true);
    check::<A>(&good).unwrap();
    for end in 0..0x1e0 {
        assert!(
            Elf::<A>::parse(&good[..end]).is_err(),
            "accepted truncation {end:#x}"
        );
    }
    let w = fields.word;
    let dynamic_entry = |index: usize| DYNAMIC + index * 2 * w;
    let cases: &[(usize, &[u8], Error)] = &[
        (4, &[if w == 8 { 1 } else { 2 }], Error::WrongClass),
        (5, &[2], Error::WrongEndian),
        (18, &[1], Error::WrongMachine),
        (16, &[2], Error::NotPositionIndependent),
        (
            fields.program_field(1, fields.offset + w),
            &word::<A>(0x100)[..w],
            Error::OverlappingSegments,
        ),
        (
            fields.program_field(1, fields.size),
            &word::<A>(0xff)[..w],
            Error::InvalidSegment,
        ),
        (
            fields.program_field(1, fields.align),
            &[3],
            Error::InvalidAlignment,
        ),
        (
            // A power of two above the cap, congruent with the segment's address.
            fields.program_field(0, fields.align),
            &word::<A>(MAX_ALIGNMENT * 2)[..w],
            Error::InvalidAlignment,
        ),
        // The ELF type, the program header count and an absent dynamic section.
        (fields.flags + 8, &[17], Error::InvalidHeader),
        (fields.flags + 8, &[0], Error::InvalidHeader),
        (fields.program_field(2, 0), &[0], Error::InvalidDynamic),
        (RELOC + w, &[1], Error::UnsupportedRelocation(1)),
        // The symbol index is the high half of `r_info` (ELF64) or its upper 24 bits (ELF32).
        (
            RELOC + w + if w == 8 { 4 } else { 1 },
            &[1],
            Error::SymbolRelocation,
        ),
        (
            RELOC,
            &word::<A>(0x110)[..w],
            Error::InvalidRelocationTarget,
        ),
        (DYNAMIC, &[1], Error::DynamicDependency),
        (DYNAMIC, &[99], Error::UnsupportedDynamicTag(99)),
        // A second table address, and a size that is not a whole entry.
        (dynamic_entry(3), &[7], Error::InvalidDynamic),
        (dynamic_entry(1) + w, &[1], Error::InvalidRelocationTable),
    ];
    for &(offset, bytes, expected) in cases {
        let mut bad = good;
        bad[offset..offset + bytes.len()].copy_from_slice(bytes);
        if offset == dynamic_entry(3) {
            // A repeated DT_RELA address needs its value too.
            bad[offset + w..offset + 2 * w].fill(0);
        }
        assert_eq!(check::<A>(&bad), Err(expected), "case at {offset:#x}");
    }
}

#[test]
fn rejects_malformed_elfs_for_every_class() {
    rejects_malformed_elfs::<Native>();
    rejects_malformed_elfs::<Riscv32>();
    rejects_malformed_elfs::<Arm32>();
}

fn alignment_is_bounded<A: Abi>() {
    let fields = Fields::of::<A>();
    let mut file = fixture::<A>(true);
    let align = fields.program_field(0, fields.align);
    for valid in [0usize, 1, 16, 4096, MAX_ALIGNMENT] {
        put::<A>(&mut file, align, valid);
        put::<A>(&mut file, fields.program_field(1, fields.align), valid);
        Elf::<A>::parse(&file).unwrap();
    }
    for invalid in [3usize, 0x30, MAX_ALIGNMENT * 2, 1 << 21] {
        put::<A>(&mut file, align, invalid);
        assert!(matches!(
            Elf::<A>::parse(&file),
            Err(Error::InvalidAlignment)
        ));
    }
}

#[test]
fn bounds_segment_alignment() {
    alignment_is_bounded::<Native>();
    alignment_is_bounded::<Riscv32>();
}

fn rejects_constructors<A: Abi>() {
    let w = A::WORD;
    let file = fixture::<A>(true);
    // DT_INIT, DT_FINI, DT_INIT_ARRAYSZ, DT_FINI_ARRAYSZ and DT_PREINIT_ARRAYSZ.
    for tag in [12, 13, 27, 28, 33] {
        let mut image = file;
        put::<A>(&mut image, DYNAMIC + 3 * 2 * w, tag);
        put::<A>(&mut image, DYNAMIC + 3 * 2 * w + w, 0x100);
        assert!(
            matches!(check::<A>(&image), Err(Error::Constructors)),
            "tag {tag}"
        );
        // An empty array is harmless; so is the array address alone.
        put::<A>(&mut image, DYNAMIC + 3 * 2 * w + w, 0);
        check::<A>(&image).unwrap();
    }
    for tag in [25, 26, 32] {
        let mut image = file;
        put::<A>(&mut image, DYNAMIC + 3 * 2 * w, tag);
        put::<A>(&mut image, DYNAMIC + 3 * 2 * w + w, 0x100);
        check::<A>(&image).unwrap();
    }
}

#[test]
fn rejects_images_with_constructors_or_destructors() {
    rejects_constructors::<Native>();
    rejects_constructors::<Riscv32>();
    rejects_constructors::<Arm32>();
}

#[test]
fn requires_a_dynamic_section_for_static_pies() {
    let fields = Fields::of::<Native>();
    let mut file = fixture::<Native>(true);
    // Turn PT_DYNAMIC into PT_NOTE: the image still has no dynamic section.
    file[fields.program_field(2, 0)] = 4;
    assert!(matches!(
        Elf::<Native>::parse(&file),
        Err(Error::InvalidDynamic)
    ));
}

fn bounds_the_program_header_count<A: Abi>() {
    let fields = Fields::of::<A>();
    let mut file = fixture::<A>(true);
    // A trailing PT_NULL header is accepted.
    file[fields.flags + 8] = 4;
    Elf::<A>::parse(&file).unwrap();
    for count in [0, MAX_PROGRAM_HEADERS as u8 + 1, u8::MAX] {
        file[fields.flags + 8] = count;
        assert!(matches!(Elf::<A>::parse(&file), Err(Error::InvalidHeader)));
    }
}

#[test]
fn bounds_the_program_header_count_in_either_class() {
    bounds_the_program_header_count::<Native>();
    bounds_the_program_header_count::<Riscv32>();
}

#[test]
fn checks_arm_flags() {
    // EABI version 5 with hard float, like thumbv7em-none-eabihf.
    assert!(arm_flags_supported(0x0500_0400, true));
    // Soft-float images cannot call hard-float code, and the reverse.
    assert!(!arm_flags_supported(0x0500_0200, true));
    assert!(arm_flags_supported(0x0500_0200, false));
    assert!(!arm_flags_supported(0x0500_0400, false));
    // An image that sets neither float ABI flag ("compatible" VFP args) runs on both.
    assert!(arm_flags_supported(0x0500_0000, true));
    assert!(arm_flags_supported(0x0500_0000, false));
    assert!(!arm_flags_supported(0x0500_0600, true));
    assert!(!arm_flags_supported(0x0500_0600, false));
    // Other EABI versions and big-endian BE8 images.
    assert!(!arm_flags_supported(0x0400_0400, true));
    assert!(!arm_flags_supported(0x0000_0400, true));
    assert!(!arm_flags_supported(0x0580_0400, true));

    let mut file = fixture::<Arm32>(true);
    let fields = Fields::of::<Arm32>();
    check::<Arm32>(&file).unwrap();
    write(&mut file, fields.flags, &0x0500_0200u32.to_le_bytes());
    assert_eq!(
        check::<Arm32>(&file),
        Err(Error::IncompatibleFlags(0x0500_0200))
    );
}

#[test]
fn checks_riscv_flags() {
    // RV32IMAC soft float: RVC allowed, no float ABI, no RVE.
    assert!(riscv_flags_supported(0, true, 0, false));
    assert!(riscv_flags_supported(1, true, 0, false));
    // Compressed code needs the C extension.
    assert!(!riscv_flags_supported(1, false, 0, false));
    // Single, double and quad float ABIs must match the target exactly.
    assert!(!riscv_flags_supported(2, true, 0, false));
    assert!(!riscv_flags_supported(4, true, 0, false));
    assert!(!riscv_flags_supported(0, true, 2, false));
    assert!(!riscv_flags_supported(1, true, 2, false));
    assert!(riscv_flags_supported(1 | 2, true, 2, false));
    assert!(!riscv_flags_supported(1 | 4, true, 2, false));
    assert!(!riscv_flags_supported(6, true, 4, false));
    // RVE and TSO.
    assert!(!riscv_flags_supported(8, true, 0, false));
    assert!(riscv_flags_supported(8, true, 0, true));
    assert!(!riscv_flags_supported(0x10, true, 0, false));

    let mut file = fixture::<Riscv32>(true);
    let fields = Fields::of::<Riscv32>();
    check::<Riscv32>(&file).unwrap();
    write(&mut file, fields.flags, &4u32.to_le_bytes());
    assert_eq!(check::<Riscv32>(&file), Err(Error::IncompatibleFlags(4)));

    // Hard-float single-precision RISC-V rejects soft-float and double-float images.
    struct Riscv32SingleFloat;
    impl Abi for Riscv32SingleFloat {
        const WORD: usize = 4;
        const MACHINE: u16 = 243;
        const RELATIVE: u32 = 3;
        const ARCH: Arch = Arch::Riscv32;
        const THUMB_ONLY: bool = false;
        fn flags_supported(flags: u32) -> bool {
            riscv_flags_supported(flags, true, 2, false)
        }
    }

    let mut hard_float = fixture::<Riscv32SingleFloat>(true);
    assert_eq!(
        check::<Riscv32SingleFloat>(&hard_float),
        Err(Error::IncompatibleFlags(0))
    );
    write(&mut hard_float, fields.flags, &1u32.to_le_bytes());
    assert_eq!(
        check::<Riscv32SingleFloat>(&hard_float),
        Err(Error::IncompatibleFlags(1))
    );
    write(&mut hard_float, fields.flags, &(1u32 | 2).to_le_bytes());
    check::<Riscv32SingleFloat>(&hard_float).unwrap();
    write(&mut hard_float, fields.flags, &(1u32 | 4).to_le_bytes());
    assert_eq!(
        check::<Riscv32SingleFloat>(&hard_float),
        Err(Error::IncompatibleFlags(5))
    );
}

#[test]
fn checks_native_flags() {
    let mut file = fixture::<Native>(true);
    let fields = Fields::of::<Native>();
    write(&mut file, fields.flags, &1u32.to_le_bytes());
    assert_eq!(check::<Native>(&file), Err(Error::IncompatibleFlags(1)));
}

#[test]
fn thumb_entry_selects_instruction_alignment() {
    // Arm bare metal runs Thumb only: the entry's bit zero must be set.
    let mut file = fixture::<Arm32>(true);
    check::<Arm32>(&file).unwrap();
    put::<Arm32>(&mut file, 24, 0x100);
    assert_eq!(check::<Arm32>(&file), Err(Error::InvalidEntry));
    // Elsewhere an odd entry is not even halfword aligned.
    let mut file = fixture::<Riscv32>(true);
    put::<Riscv32>(&mut file, 24, 0x101);
    assert_eq!(
        Elf::<Riscv32>::parse(&file).map(|_| ()),
        Err(Error::InvalidEntry)
    );
}

fn copies_and_relocates<A: Abi>(slot: impl Fn(&[u8], usize) -> usize) {
    for rela in [true, false] {
        let file = fixture::<A>(rela);
        let elf = Elf::<A>::parse(&file).unwrap();
        let mut image = [0; FILE];
        elf.copy_to(&mut image);
        elf.relocate(&mut image, 0x5000, 0x5000).unwrap();
        assert_eq!(slot(&image, SLOT), 0x5100);
        assert_eq!(&image[0x110..0x120], &[0; 16]);
        assert_eq!(&image[0x1e0..], &[0; 32]);
        // Only the relocated word changes next to it.
        assert_eq!(slot(&image, SLOT + A::WORD), 0);
    }
}

#[test]
fn copies_and_relocates_both_formats() {
    copies_and_relocates::<Native>(|image, at| {
        usize::from_le_bytes(image[at..at + 8].try_into().unwrap())
    });
}

#[test]
fn copies_and_relocates_32_bit_images() {
    for_32_bit(|image, at| u32::from_le_bytes(image[at..at + 4].try_into().unwrap()) as usize);
}

fn for_32_bit(slot: impl Fn(&[u8], usize) -> usize + Copy) {
    copies_and_relocates::<Riscv32>(slot);
    copies_and_relocates::<Arm32>(slot);
}

fn byte_mutations_do_not_panic<A: Abi>() {
    let file = fixture::<A>(true);
    for index in 0..file.len() {
        for byte in [0, 0x55, 0xff] {
            let mut mutated = file;
            mutated[index] = byte;
            if let Ok(elf) = Elf::<A>::parse(&mutated) {
                let _ = elf.check_abi();
                if elf.layout.size() <= file.len() {
                    let mut image = [0; FILE];
                    elf.copy_to(&mut image);
                    let _ = elf.relocate(&mut image, 0x5000, 0x5000);
                }
            }
        }
    }
}

#[test]
fn byte_mutations_do_not_panic_in_either_class() {
    byte_mutations_do_not_panic::<Native>();
    byte_mutations_do_not_panic::<Riscv32>();
    byte_mutations_do_not_panic::<Arm32>();
}

const NOTE: usize = 0x1c0;

/// The fixture with a fourth program header: a `XILA` ABI note recording `version`,
/// packed as `(major << 16) | minor`.
fn with_note<A: Abi>(version: u32) -> [u8; FILE] {
    let fields = Fields::of::<A>();
    let mut elf = fixture::<A>(true);
    write(&mut elf, fields.flags + 8, &4u16.to_le_bytes());
    let header = fields.program_field(3, 0);
    write(&mut elf, header, &4u32.to_le_bytes());
    for (field, value) in [
        (fields.offset, NOTE),
        (fields.offset + A::WORD, NOTE),
        (fields.size, 24),
        (fields.memory, 24),
        (fields.align, 4),
    ] {
        put::<A>(&mut elf, header + field, value);
    }
    for (index, value) in [5u32, 4, 1].into_iter().enumerate() {
        write(&mut elf, NOTE + 4 * index, &value.to_le_bytes());
    }
    write(&mut elf, NOTE + 12, b"XILA\0\0\0\0");
    write(&mut elf, NOTE + 20, &version.to_le_bytes());
    elf
}

fn checks_the_xila_version<A: Abi>() {
    let running = xila_version::RUNNING;
    let check = |declared: Version| {
        let file = with_note::<A>(declared.packed());
        Elf::<A>::parse(&file)?.check_abi()
    };
    assert_eq!(check(running), Ok(()));
    // Before 1.0 an older minor is refused as well; from 1.0 it runs.
    let older = running
        .minor
        .checked_sub(1)
        .map(|minor| Version::new(running.major, minor));
    if let Some(older) = older
        && running.major != 0
    {
        assert_eq!(check(older), Ok(()));
    }
    let older = older.filter(|_| running.major == 0);
    for declared in [
        Some(Version::new(running.major, running.minor.wrapping_add(1))),
        Some(Version::new(running.major.wrapping_add(1), running.minor)),
        Some(Version::new(running.major.wrapping_add(1), 0)),
        older,
    ]
    .into_iter()
    .flatten()
    {
        assert_eq!(
            check(declared),
            Err(Error::IncompatibleXilaVersion { declared, running }),
            "accepted an image built for {declared} on {running}"
        );
    }
    let missing = fixture::<A>(true);
    assert_eq!(
        Elf::<A>::parse(&missing).unwrap().check_abi(),
        Err(Error::MissingAbiNote)
    );
}

#[test]
fn checks_the_xila_version_in_either_class() {
    checks_the_xila_version::<Native>();
    checks_the_xila_version::<Riscv32>();
    checks_the_xila_version::<Arm32>();
    checks_the_xila_version::<Xtensa32>();
}

#[test]
fn xtensa_accepts_gnu_ld_dynamic_tags_only_on_xtensa() {
    for tag in [3usize, 0x7000_0000, 0x7000_0001] {
        // Put the tag at dynamic entry 3 (before the trailing DT_NULL at entry 4).
        let mut xtensa = fixture::<Xtensa32>(true);
        put::<Xtensa32>(&mut xtensa, DYNAMIC + 3 * 8, tag);
        put::<Xtensa32>(&mut xtensa, DYNAMIC + 3 * 8 + 4, 0x100);
        Elf::<Xtensa32>::parse(&xtensa).unwrap();

        // The same tags stay rejected on non-Xtensa targets.
        let mut riscv = fixture::<Riscv32>(true);
        put::<Riscv32>(&mut riscv, DYNAMIC + 3 * 8, tag);
        put::<Riscv32>(&mut riscv, DYNAMIC + 3 * 8 + 4, 0x100);
        assert_eq!(
            Elf::<Riscv32>::parse(&riscv).map(|_| ()),
            Err(Error::UnsupportedDynamicTag(tag))
        );
    }
}

#[test]
fn split_bus_relocates_xtensa_by_segment_and_rejects_other_arches() {
    let data_bias = 0x3FC8_8000usize;
    let exec_bias = 0x4037_8000usize;

    // Non-Xtensa architectures must reject a split address space.
    let riscv_file = fixture::<Riscv32>(true);
    let riscv = Elf::<Riscv32>::parse(&riscv_file).unwrap();
    let mut image = [0; FILE];
    riscv.copy_to(&mut image);
    assert_eq!(
        riscv.relocate(&mut image, data_bias, exec_bias),
        Err(Error::UnsupportedSplitAddressSpace)
    );

    // Build an Xtensa fixture with two RELA entries:
    // 1. SLOT      -> in-place addend 0x100 (inside PF_X text segment 0..0x110) -> exec_bias
    // 2. SLOT + 4  -> in-place addend 0x140 (inside PF_RW data segment 0x120..0x200) -> data_bias
    let mut file = fixture::<Xtensa32>(true);
    // Clear the RELA explicit addend (Xtensa reads in-place) and set 2 RELA entries (24 bytes).
    put::<Xtensa32>(&mut file, DYNAMIC + 8 + 4, 24); // DT_RELASZ = 24
    put::<Xtensa32>(&mut file, RELOC, SLOT);
    put::<Xtensa32>(&mut file, RELOC + 4, Xtensa32::RELATIVE as usize);
    put::<Xtensa32>(&mut file, RELOC + 8, 0);
    put::<Xtensa32>(&mut file, RELOC + 12, SLOT + 4);
    put::<Xtensa32>(&mut file, RELOC + 16, Xtensa32::RELATIVE as usize);
    put::<Xtensa32>(&mut file, RELOC + 20, 0);
    // In-place addends in the image.
    put::<Xtensa32>(&mut file, SLOT, 0x100);
    put::<Xtensa32>(&mut file, SLOT + 4, 0x140);

    let elf = Elf::<Xtensa32>::parse(&file).unwrap();
    let mut image = [0; FILE];
    elf.copy_to(&mut image);
    elf.relocate(&mut image, data_bias, exec_bias).unwrap();
    let read32 =
        |buf: &[u8], at: usize| u32::from_le_bytes(buf[at..at + 4].try_into().unwrap()) as usize;
    assert_eq!(read32(&image, SLOT), exec_bias + 0x100);
    assert_eq!(read32(&image, SLOT + 4), data_bias + 0x140);

    // One-past-the-end data pointer (0x120 + 0xe0 = 0x200) resolves against data_bias.
    put::<Xtensa32>(&mut file, SLOT + 4, 0x200);
    let elf = Elf::<Xtensa32>::parse(&file).unwrap();
    let mut image = [0; FILE];
    elf.copy_to(&mut image);
    elf.relocate(&mut image, data_bias, exec_bias).unwrap();
    assert_eq!(read32(&image, SLOT + 4), data_bias + 0x200);

    // Out-of-segment addend is rejected on split bus.
    put::<Xtensa32>(&mut file, SLOT + 4, 0x9999);
    let elf = Elf::<Xtensa32>::parse(&file).unwrap();
    let mut image = [0; FILE];
    elf.copy_to(&mut image);
    assert_eq!(
        elf.relocate(&mut image, data_bias, exec_bias),
        Err(Error::InvalidRelocationAddend)
    );
}
