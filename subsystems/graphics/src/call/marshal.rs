//! VM-independent graphics memory adapters.

use super::{
    Word,
    error::{Error, Result},
    transport::Transport,
};
use alloc::vec::Vec;
use core::ffi::{CStr, c_char};

pub const MAX_STRING_BYTES: usize = 1024;
pub const MAX_TEXTAREA_BYTES: usize = 4097;
pub const MAX_WIDGET_OPTIONS_BYTES: usize = 1024;
pub const MAX_WIDGET_OPTIONS: usize = 64;
pub const MAX_ROLLER_INFINITE_OPTIONS_BYTES: usize = 512;
pub const MAX_WIDGET_OPTIONS_BYTES_PER_VM: usize = 8192;
/// Includes the sentinel. Calculator uses 48 entries.
pub const MAX_MAP_ENTRIES: usize = 64;
/// Includes every string's NUL and the native empty-string sentinel.
pub const MAX_MAP_BYTES: usize = 8192;
/// Includes the LVGL sentinel for each axis; each axis may have at most 31 tracks.
pub const MAX_GRID_TRACKS_PER_AXIS: usize = 32;
const VALID_STYLE_STATES: u32 = 0xF3FD;

/// LVGL's void setters may retain the old value on allocation failure, so
/// replacements conservatively retain the per-object high-water charge.
pub fn updated_widget_options_usage(total: usize, previous: usize, new: usize) -> Result<usize> {
    let used = total
        .checked_sub(previous)
        .ok_or(Error::InvalidValue)?
        .checked_add(previous.max(new))
        .ok_or(Error::ResourceLimit)?;
    if used > MAX_WIDGET_OPTIONS_BYTES_PER_VM {
        Err(Error::ResourceLimit)
    } else {
        Ok(used)
    }
}

/// Restrict style selectors to known LVGL parts and state bits.
pub fn validate_style_selector(selector: u32) -> Result<()> {
    let part = selector & !0xffff;
    let state = selector & 0xffff;
    if matches!(
        part,
        0 | 0x1_0000 | 0x2_0000 | 0x3_0000 | 0x4_0000 | 0x5_0000 | 0x6_0000 | 0xf_0000
    ) && state & !VALID_STYLE_STATES == 0
    {
        Ok(())
    } else {
        Err(Error::InvalidValue)
    }
}

/// Stable host-owned LVGL grid descriptors. LVGL retains both pointers until
/// the grid layout is replaced or its object is deleted.
/// Layout: `[columns_len, columns..., rows...]`.
#[derive(Debug, PartialEq, Eq)]
pub struct GridTracks(Vec<i32>);

impl GridTracks {
    pub fn decode(
        memory: &dyn Transport,
        columns: Word,
        rows: Word,
        template_last: i32,
        content: i32,
    ) -> Result<Self> {
        fn scan_axis(
            memory: &dyn Transport,
            address: Word,
            template_last: i32,
            content: i32,
        ) -> Result<([i32; MAX_GRID_TRACKS_PER_AXIS], usize)> {
            let mut values = [0; MAX_GRID_TRACKS_PER_AXIS];
            let fr_min = template_last - 100;
            for (index, value) in values.iter_mut().enumerate() {
                let descriptor = memory.read_at::<i32>(address, index)?;
                if descriptor == template_last {
                    *value = descriptor;
                    return Ok((values, index + 1));
                }
                // Accept bounded pixel tracks, LV_GRID_CONTENT, and valid
                // LV_GRID_FR weights; reject arbitrary flag/coordinate values.
                if !(0..=16_384).contains(&descriptor)
                    && descriptor != content
                    && !(fr_min..template_last).contains(&descriptor)
                {
                    return Err(Error::InvalidValue);
                }
                *value = descriptor;
            }
            Err(Error::ResourceLimit)
        }

        let (columns, columns_len) = scan_axis(memory, columns, template_last, content)?;
        let (rows, rows_len) = scan_axis(memory, rows, template_last, content)?;
        let mut tracks = Vec::with_capacity(1 + columns_len + rows_len);
        tracks.push(columns_len as i32);
        tracks.extend_from_slice(&columns[..columns_len]);
        tracks.extend_from_slice(&rows[..rows_len]);
        Ok(Self(tracks))
    }

    fn columns_len(&self) -> usize {
        self.0[0] as usize
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub fn columns(&self) -> &[i32] {
        &self.0[1..1 + self.columns_len()]
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub fn rows(&self) -> &[i32] {
        &self.0[1 + self.columns_len()..]
    }

    pub fn track_count(&self) -> usize {
        self.0.len() - 1
    }

    pub fn columns_ptr(&self) -> *const i32 {
        self.0.as_ptr().wrapping_add(1)
    }

    pub fn rows_ptr(&self) -> *const i32 {
        self.0.as_ptr().wrapping_add(1 + self.columns_len())
    }
}

pub fn read_c_string(memory: &dyn Transport, address: Word) -> Result<&CStr> {
    memory.c_string(address, MAX_STRING_BYTES)
}

/// Reads a bounded, NUL-terminated UTF-8 string for LVGL text APIs.
pub fn read_utf8_c_string(memory: &dyn Transport, address: Word) -> Result<&str> {
    read_c_string(memory, address)?
        .to_str()
        .map_err(|_| Error::InvalidValue)
}

/// Reads newline-separated widget options. LVGL's dynamic setters copy this
/// string, so the returned borrow is only valid for the current call.
pub fn read_widget_options(memory: &dyn Transport, address: Word) -> Result<&CStr> {
    let options = memory.c_string(address, MAX_WIDGET_OPTIONS_BYTES)?;
    let mut count = 0;
    for entry in options.to_bytes().split(|byte| *byte == b'\n') {
        count += 1;
        if entry.len() >= MAX_STRING_BYTES {
            return Err(Error::ResourceLimit);
        }
    }
    if count > MAX_WIDGET_OPTIONS {
        return Err(Error::ResourceLimit);
    }
    Ok(options)
}

/// One aligned allocation: native pointer array followed by NUL-terminated
/// strings. Moving the Vec owner never moves or retags its allocation.
pub struct ButtonMap {
    storage: Vec<usize>,
}

impl ButtonMap {
    pub fn decode(memory: &dyn Transport, table: Word) -> Result<Self> {
        // (address, length including NUL) of each string.
        let mut ranges = [(0, 0); MAX_MAP_ENTRIES];
        let mut total = 1; // Native sentinel, even when the caller uses a null pointer.
        let mut count = 0;
        let mut terminated = false;
        for (index, range) in ranges.iter_mut().enumerate() {
            let address = table
                .checked_add((index * memory.word_size()) as Word)
                .ok_or(Error::InvalidMemory)?;
            let pointer = memory.read_word(address)?;
            if pointer == 0 {
                terminated = true;
                break;
            }
            let text = read_c_string(memory, pointer)?;
            if text.is_empty() {
                terminated = true;
                break;
            }
            let length = text.to_bytes_with_nul().len();
            total += length;
            if total > MAX_MAP_BYTES {
                return Err(Error::ResourceLimit);
            }
            *range = (pointer, length);
            count += 1;
        }
        if !terminated {
            return Err(Error::ResourceLimit);
        }

        // All guest input is validated before allocating retained native storage.
        let pointer_bytes = (count + 1) * core::mem::size_of::<usize>();
        let words = (pointer_bytes + total).div_ceil(core::mem::size_of::<usize>());
        let mut storage = alloc::vec![0usize; words];
        let base = storage.as_mut_ptr().cast::<u8>();
        let pointers = base.cast::<*const c_char>();
        let mut start = pointer_bytes;
        for (index, &(address, length)) in ranges[..count].iter().enumerate() {
            let text = memory.bytes(address, length).expect("validated map range");
            // SAFETY: allocation includes the pointer prefix and all validated
            // strings; regions do not overlap and the prefix is pointer-aligned.
            unsafe {
                pointers.add(index).write(base.add(start).cast());
                core::ptr::copy_nonoverlapping(text.as_ptr(), base.add(start), text.len());
            }
            start += text.len();
        }
        // The zero-filled final byte is the native empty-string sentinel.
        unsafe {
            pointers.add(count).write(base.add(start).cast());
        }
        Ok(Self { storage })
    }

    pub fn as_ptr(&self) -> *const *const c_char {
        self.storage.as_ptr().cast()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call::{LinearMemory, Transport};
    use alloc::{vec, vec::Vec};

    /// Write through the executor's typed accessors.
    fn write<T: crate::call::transport::Scalar>(
        memory: &mut LinearMemory<'_>,
        address: Word,
        value: T,
    ) {
        (memory as &mut dyn Transport)
            .write(address, value)
            .unwrap();
    }

    #[test]
    fn widget_option_usage_charges_replacement_high_water_and_caps_vm_budget() {
        assert_eq!(updated_widget_options_usage(700, 100, 200), Ok(800));
        assert_eq!(updated_widget_options_usage(700, 200, 100), Ok(700));
        assert_eq!(
            updated_widget_options_usage(8192, 1024, 1025),
            Err(Error::ResourceLimit)
        );
        assert_eq!(
            updated_widget_options_usage(100, 101, 50),
            Err(Error::InvalidValue)
        );
    }

    #[test]
    fn utf8_c_strings_reject_invalid_sequences_before_native_copying() {
        let mut bytes = [0; 8];
        bytes[1..4].copy_from_slice(&[0xc3, 0x28, 0]);
        let memory = LinearMemory::new(&mut bytes, 32);
        assert_eq!(read_utf8_c_string(&memory, 1), Err(Error::InvalidValue));
    }

    #[test]
    fn widget_options_are_bounded_for_guest_to_lvgl_copying() {
        let mut bytes = [0; 9000];
        bytes[64..77].copy_from_slice(b"One\nDeux\n\xe4\xb8\x89\0");
        let memory = LinearMemory::new(&mut bytes, 32);
        assert_eq!(
            read_widget_options(&memory, 64).unwrap().to_bytes(),
            "One\nDeux\n三".as_bytes()
        );
    }

    #[test]
    fn widget_options_reject_excess_entries_and_oversized_option_text() {
        let mut bytes = [0; 9000];
        bytes[64..64 + 1100].fill(b'a');
        bytes[64 + 1100] = 0;
        let memory = LinearMemory::new(&mut bytes, 32);
        assert_eq!(read_widget_options(&memory, 64), Err(Error::ResourceLimit));

        bytes.fill(0);
        for index in 0..65 {
            bytes[64 + index * 2] = b'x';
            bytes[65 + index * 2] = b'\n';
        }
        bytes[64 + 130] = 0;
        let memory = LinearMemory::new(&mut bytes, 32);
        assert_eq!(read_widget_options(&memory, 64), Err(Error::ResourceLimit));
    }

    #[test]
    fn style_selectors_allow_known_parts_and_states_only() {
        for selector in [0, 0x1_0000, 0x2_0000 | 0x80, 0x6_0000 | 0xF3FD] {
            assert_eq!(validate_style_selector(selector), Ok(()));
        }
        for selector in [0x7_0000, 0x1_0000 | 0x2, 0x10_0000] {
            assert_eq!(validate_style_selector(selector), Err(Error::InvalidValue));
        }
    }

    #[test]
    fn grid_track_arrays_are_bounded_terminated_and_copied() {
        const GRID_LAST: i32 = 536_870_911;
        const GRID_CONTENT: i32 = 536_870_810;
        let mut bytes = [0; 160];
        let mut memory = LinearMemory::new(&mut bytes, 32);
        for (offset, value) in [
            (32, 48),
            (36, GRID_CONTENT),
            (40, GRID_LAST),
            (64, 24),
            (68, GRID_LAST),
        ] {
            write(&mut memory, offset, value);
        }
        let grid = GridTracks::decode(&memory, 32, 64, GRID_LAST, GRID_CONTENT).unwrap();
        bytes.fill(0xff);
        assert_eq!(grid.columns(), &[48, GRID_CONTENT, GRID_LAST]);
        assert_eq!(grid.rows(), &[24, GRID_LAST]);
    }

    #[test]
    fn grid_track_arrays_reject_missing_terminators_and_unknown_descriptors() {
        const GRID_LAST: i32 = 536_870_911;
        const GRID_CONTENT: i32 = 536_870_810;
        let mut bytes = [0; 256];
        let mut memory = LinearMemory::new(&mut bytes, 32);
        for index in 0..32 {
            write(&mut memory, 32 + index * 4, 16i32);
        }
        write(&mut memory, 160, GRID_LAST);
        assert_eq!(
            GridTracks::decode(&memory, 32, 160, GRID_LAST, GRID_CONTENT),
            Err(Error::ResourceLimit)
        );
        write(&mut memory, 32, -1i32);
        write(&mut memory, 36, GRID_LAST);
        assert_eq!(
            GridTracks::decode(&memory, 32, 160, GRID_LAST, GRID_CONTENT),
            Err(Error::InvalidValue)
        );
    }

    fn table(memory: &mut LinearMemory<'_>, pointers: &[Word]) {
        for (index, pointer) in pointers.iter().enumerate() {
            (memory as &mut dyn Transport).put_word(1 + (index * 4) as Word, *pointer);
        }
    }

    #[test]
    fn c_strings_are_bounded_and_never_accept_null_or_out_of_range_offsets() {
        let mut bytes = [0; 1030];
        bytes[1..1025].fill(b'a');
        let base = bytes.as_ptr() as usize;
        let memory = LinearMemory::new(&mut bytes, 32);
        assert_eq!(read_c_string(&memory, 0), Err(Error::InvalidMemory));
        assert_eq!(read_c_string(&memory, 1), Err(Error::ResourceLimit));
        let borrowed = read_c_string(&memory, 2).unwrap();
        assert_eq!(borrowed.to_bytes().len(), 1023);
        assert_eq!(borrowed.as_ptr() as usize, base + 2);
        assert_eq!(read_c_string(&memory, 1029).unwrap().to_bytes(), b"");
        assert_eq!(read_c_string(&memory, 1030), Err(Error::InvalidMemory));
        assert_eq!(read_c_string(&memory, Word::MAX), Err(Error::InvalidMemory));
        let mut no_nul = [1, b'x'];
        assert_eq!(
            read_c_string(&LinearMemory::new(&mut no_nul, 32), 1),
            Err(Error::InvalidMemory)
        );
    }

    #[test]
    fn textarea_strings_allow_a_full_page_and_remain_bounded() {
        let mut bytes = vec![b'a'; 4099];
        bytes[4097] = 0;
        let base = bytes.as_ptr() as usize;
        let mut memory = LinearMemory::new(&mut bytes, 32);
        let borrowed = memory.c_string(1, MAX_TEXTAREA_BYTES).unwrap();
        assert_eq!(borrowed.to_bytes().len(), 4096);
        assert_eq!(borrowed.as_ptr() as usize, base + 1);
        write(&mut memory, 4097, b'a');
        assert_eq!(
            memory.c_string(1, MAX_TEXTAREA_BYTES),
            Err(Error::ResourceLimit)
        );
    }

    #[test]
    fn maps_own_native_strings_and_pointer_tables_after_guest_memory_disappears() {
        let map = {
            let mut bytes = [0; 320];
            bytes[300..309].copy_from_slice(b"1\0\n\0two\0\0");
            let mut memory = LinearMemory::new(&mut bytes, 32);
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
        let mut memory = LinearMemory::new(&mut bytes, 32);
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
        let mut memory = LinearMemory::new(&mut bytes, 32);
        assert!(matches!(
            ButtonMap::decode(&memory, 0),
            Err(Error::InvalidMemory)
        ));
        assert!(matches!(
            ButtonMap::decode(&memory, Word::MAX),
            Err(Error::InvalidMemory)
        ));
        table(&mut memory, &[100]);
        assert!(matches!(
            ButtonMap::decode(&memory, 1),
            Err(Error::InvalidMemory)
        ));
        let mut bytes = [b'a'; 80];
        let mut memory = LinearMemory::new(&mut bytes, 32);
        table(&mut memory, &[79]);
        assert!(matches!(
            ButtonMap::decode(&memory, 1),
            Err(Error::InvalidMemory)
        ));
        let mut bytes = [0; 2];
        assert!(matches!(
            ButtonMap::decode(&LinearMemory::new(&mut bytes, 32), 1),
            Err(Error::InvalidMemory)
        ));
    }

    #[test]
    fn maps_enforce_entry_and_retained_byte_limits() {
        let mut bytes = vec![0; 2048];
        bytes[1800..1802].copy_from_slice(b"x\0");
        let mut memory = LinearMemory::new(&mut bytes, 32);
        let mut pointers = vec![1800; 63];
        pointers.push(0);
        table(&mut memory, &pointers);
        assert!(ButtonMap::decode(&memory, 1).is_ok());
        pointers[63] = 1800;
        table(&mut memory, &pointers);
        assert!(matches!(
            ButtonMap::decode(&memory, 1),
            Err(Error::ResourceLimit)
        ));

        let mut bytes = vec![b'x'; 9000];
        bytes[2047] = 0; // 1024-byte string, including NUL.
        bytes[3070] = 0; // 1023-byte string, including NUL.
        let mut memory = LinearMemory::new(&mut bytes, 32);
        let mut pointers: Vec<_> = vec![1024; 7];
        pointers.extend([2048, 0]);
        table(&mut memory, &pointers);
        assert!(ButtonMap::decode(&memory, 1).is_ok()); // 7*1024 + 1023 + terminator = 8192.
        pointers[7] = 1024;
        table(&mut memory, &pointers);
        assert!(matches!(
            ButtonMap::decode(&memory, 1),
            Err(Error::ResourceLimit)
        ));
    }

    #[test]
    fn errors_never_encode_success() {
        for error in [
            Error::InvalidArgumentsCount,
            Error::InvalidMemory,
            Error::Unavailable,
            Error::Unsupported,
            Error::InvalidHandle,
            Error::ResourceLimit,
            Error::InvalidValue,
            Error::NativeFailure,
        ] {
            assert_ne!(error as i32, 0);
        }
    }
}
