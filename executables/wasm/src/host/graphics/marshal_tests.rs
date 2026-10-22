use super::{
    error::GraphicsError,
    marshal::{
        ButtonMap, GridTracks, read_c_string, read_textarea_string, validate_style_selector,
    },
};
use crate::host::translation::{GuestMemory, GuestPointer, GuestValue, WasmUsize};
use alloc::{vec, vec::Vec};
use core::ffi::CStr;

#[test]
fn style_selectors_allow_known_parts_and_states_only() {
    for selector in [0, 0x1_0000, 0x2_0000 | 0x80, 0x6_0000 | 0xF3FD] {
        assert_eq!(validate_style_selector(selector), Ok(()));
    }
    for selector in [0x7_0000, 0x1_0000 | 0x2, 0x10_0000] {
        assert_eq!(
            validate_style_selector(selector),
            Err(GraphicsError::InvalidValue)
        );
    }
}

#[test]
fn grid_track_arrays_are_bounded_terminated_and_copied() {
    const GRID_LAST: i32 = 536_870_911;
    const GRID_CONTENT: i32 = 536_870_810;
    let mut bytes = [0; 160];
    let mut memory = GuestMemory::new(&mut bytes);
    for (offset, value) in [(32, 48), (36, GRID_CONTENT), (40, GRID_LAST), (64, 24), (68, GRID_LAST)] {
        memory.write(GuestPointer::new(offset), value).unwrap();
    }
    let grid = GridTracks::decode(&memory, 32, 64, GRID_LAST, GRID_CONTENT).unwrap();
    drop(memory);
    bytes.fill(0xff);
    assert_eq!(grid.columns(), &[48, GRID_CONTENT, GRID_LAST]);
    assert_eq!(grid.rows(), &[24, GRID_LAST]);
}

#[test]
fn grid_track_arrays_reject_missing_terminators_and_unknown_descriptors() {
    const GRID_LAST: i32 = 536_870_911;
    const GRID_CONTENT: i32 = 536_870_810;
    let mut bytes = [0; 256];
    let mut memory = GuestMemory::new(&mut bytes);
    for index in 0..32 {
        memory
            .write(GuestPointer::new(32 + index * 4), 16i32)
            .unwrap();
    }
    memory
        .write(GuestPointer::new(160), GRID_LAST)
        .unwrap();
    assert_eq!(
        GridTracks::decode(&memory, 32, 160, GRID_LAST, GRID_CONTENT),
        Err(GraphicsError::ResourceLimit)
    );
    memory.write(GuestPointer::new(32), -1i32).unwrap();
    memory
        .write(GuestPointer::new(36), GRID_LAST)
        .unwrap();
    assert_eq!(
        GridTracks::decode(&memory, 32, 160, GRID_LAST, GRID_CONTENT),
        Err(GraphicsError::InvalidValue)
    );
}

fn table(memory: &mut GuestMemory<'_>, pointers: &[WasmUsize]) {
    for (index, pointer) in pointers.iter().enumerate() {
        memory
            .write(
                GuestPointer::new(1 + (index * WasmUsize::SIZE) as WasmUsize),
                *pointer,
            )
            .unwrap();
    }
}

#[test]
fn c_strings_are_bounded_and_never_accept_null_or_out_of_range_offsets() {
    let mut bytes = [0; 1030];
    bytes[1..1025].fill(b'a');
    let memory = GuestMemory::new(&mut bytes);
    assert_eq!(read_c_string(&memory, 0), Err(GraphicsError::InvalidMemory));
    assert_eq!(read_c_string(&memory, 1), Err(GraphicsError::ResourceLimit));
    assert_eq!(read_c_string(&memory, 2).unwrap().to_bytes().len(), 1023);
    assert_eq!(read_c_string(&memory, 1029).unwrap().to_bytes(), b"");
    assert_eq!(
        read_c_string(&memory, 1030),
        Err(GraphicsError::InvalidMemory)
    );
    assert_eq!(
        read_c_string(&memory, WasmUsize::MAX),
        Err(GraphicsError::InvalidMemory)
    );
    let mut no_nul = [1, b'x'];
    assert_eq!(
        read_c_string(&GuestMemory::new(&mut no_nul), 1),
        Err(GraphicsError::InvalidMemory)
    );
}

#[test]
fn textarea_strings_allow_a_full_page_and_remain_bounded() {
    let mut bytes = vec![b'a'; 4099];
    bytes[4097] = 0;
    let mut memory = GuestMemory::new(&mut bytes);
    assert_eq!(read_textarea_string(&memory, 1).unwrap().to_bytes().len(), 4096);
    memory.write(GuestPointer::new(4097), b'a').unwrap();
    assert_eq!(
        read_textarea_string(&memory, 1),
        Err(GraphicsError::ResourceLimit)
    );
}

#[test]
fn maps_own_native_strings_and_pointer_tables_after_guest_memory_disappears() {
    let map = {
        let mut bytes = [0; 320];
        bytes[300..309].copy_from_slice(b"1\0\n\0two\0\0");
        let mut memory = GuestMemory::new(&mut bytes);
        table(&mut memory, &[300, 302, 304, 0]);
        let map = ButtonMap::decode(&memory, 1).unwrap();
        bytes.fill(0xff);
        map
    };
    let pointer = map.as_ptr();
    let moved = alloc::boxed::Box::new(map);
    assert_eq!(moved.as_ptr(), pointer);
    for (index, expected) in [b"1".as_slice(), b"\n", b"two", b""].iter().enumerate() {
        assert_eq!(
            unsafe { CStr::from_ptr(*pointer.add(index)) }.to_bytes(),
            *expected
        );
    }
}

#[test]
fn empty_string_sentinels_and_null_sentinels_produce_lvgl_empty_string_terminators() {
    let mut bytes = [0; 300];
    bytes[280..284].copy_from_slice(b"x\0\0\0");
    let mut memory = GuestMemory::new(&mut bytes);
    for pointers in [&[280, 282][..], &[280, 0][..], &[0][..], &[282][..]] {
        table(&mut memory, pointers);
        let map = ButtonMap::decode(&memory, 1).unwrap();
        let last = if pointers.len() == 1 { 0 } else { 1 };
        assert_eq!(
            unsafe { CStr::from_ptr(*map.as_ptr().add(last)) }.to_bytes(),
            b""
        );
    }
}

#[test]
fn maps_reject_incomplete_tables_and_invalid_string_pointers() {
    let mut bytes = [0; 12];
    let mut memory = GuestMemory::new(&mut bytes);
    assert!(matches!(
        ButtonMap::decode(&memory, 0),
        Err(GraphicsError::InvalidMemory)
    ));
    assert!(matches!(
        ButtonMap::decode(&memory, WasmUsize::MAX),
        Err(GraphicsError::InvalidMemory)
    ));
    table(&mut memory, &[100]);
    assert!(matches!(
        ButtonMap::decode(&memory, 1),
        Err(GraphicsError::InvalidMemory)
    ));
    let mut bytes = [b'a'; 80];
    let mut memory = GuestMemory::new(&mut bytes);
    table(&mut memory, &[79]);
    assert!(matches!(
        ButtonMap::decode(&memory, 1),
        Err(GraphicsError::InvalidMemory)
    ));
    let mut bytes = [0; 2];
    assert!(matches!(
        ButtonMap::decode(&GuestMemory::new(&mut bytes), 1),
        Err(GraphicsError::InvalidMemory)
    ));
}

#[test]
fn maps_enforce_entry_and_retained_byte_limits() {
    let mut bytes = vec![0; 2048];
    bytes[1800..1802].copy_from_slice(b"x\0");
    let mut memory = GuestMemory::new(&mut bytes);
    let mut pointers = vec![1800; 63];
    pointers.push(0);
    table(&mut memory, &pointers);
    assert!(ButtonMap::decode(&memory, 1).is_ok());
    pointers[63] = 1800;
    table(&mut memory, &pointers);
    assert!(matches!(
        ButtonMap::decode(&memory, 1),
        Err(GraphicsError::ResourceLimit)
    ));

    let mut bytes = vec![b'x'; 9000];
    bytes[2047] = 0; // 1024-byte string, including NUL.
    bytes[3070] = 0; // 1023-byte string, including NUL.
    let mut memory = GuestMemory::new(&mut bytes);
    let mut pointers: Vec<_> = vec![1024; 7];
    pointers.extend([2048, 0]);
    table(&mut memory, &pointers);
    assert!(ButtonMap::decode(&memory, 1).is_ok()); // 7*1024 + 1023 + terminator = 8192.
    pointers[7] = 1024;
    table(&mut memory, &pointers);
    assert!(matches!(
        ButtonMap::decode(&memory, 1),
        Err(GraphicsError::ResourceLimit)
    ));
}

#[test]
fn errors_never_encode_success() {
    for error in [
        GraphicsError::InvalidArgumentsCount,
        GraphicsError::InvalidMemory,
        GraphicsError::Unavailable,
        GraphicsError::Unsupported,
        GraphicsError::InvalidHandle,
        GraphicsError::ResourceLimit,
        GraphicsError::InvalidValue,
        GraphicsError::NativeFailure,
    ] {
        assert_ne!(error as i32, 0);
    }
}
