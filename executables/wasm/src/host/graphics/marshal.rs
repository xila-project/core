//! VM-independent graphics memory adapters.

use super::error::{GraphicsError, Result};
use crate::host::translation::{GuestMemory, GuestPointer, GuestSlice, GuestValue, WasmUsize};
use alloc::vec::Vec;
use core::ffi::{CStr, c_char};

pub const MAX_STRING_BYTES: usize = 1024;
pub const MAX_TEXTAREA_BYTES: usize = 4097;
/// Includes the sentinel. Calculator uses 48 entries.
pub const MAX_MAP_ENTRIES: usize = 64;
/// Includes every string's NUL and the native empty-string sentinel.
pub const MAX_MAP_BYTES: usize = 8192;

pub fn read_c_string<'a>(memory: &'a GuestMemory<'_>, offset: WasmUsize) -> Result<&'a CStr> {
    read_c_string_with_limit(memory, offset, MAX_STRING_BYTES)
}

pub fn read_textarea_string<'a>(
    memory: &'a GuestMemory<'_>,
    offset: WasmUsize,
) -> Result<&'a CStr> {
    read_c_string_with_limit(memory, offset, MAX_TEXTAREA_BYTES)
}

fn read_c_string_with_limit<'a>(
    memory: &'a GuestMemory<'_>,
    offset: WasmUsize,
    limit: usize,
) -> Result<&'a CStr> {
    for index in 0..limit {
        let address = offset
            .checked_add(index as WasmUsize)
            .ok_or(GraphicsError::InvalidMemory)?;
        let byte = memory
            .read(GuestPointer::<u8>::new(address))
            .ok_or(GraphicsError::InvalidMemory)?;
        if byte == 0 {
            let bytes = memory
                .bytes(GuestSlice::new(offset, (index + 1) as WasmUsize))
                .expect("the scanned string range is valid");
            return Ok(CStr::from_bytes_with_nul(bytes).expect("first NUL terminates the string"));
        }
    }
    Err(GraphicsError::ResourceLimit)
}

/// LVGL retains this table and its strings. Both allocations have stable addresses;
/// moving the wrapper or growing/mutating guest memory cannot invalidate them.
pub struct ButtonMap {
    // Vec owns allocations through raw pointers: unlike Box, moving the owner
    // does not uniquely retag the pointee and invalidate these cached pointers.
    _strings: Vec<u8>,
    pointers: Vec<*const c_char>,
}

impl ButtonMap {
    pub fn decode(memory: &GuestMemory<'_>, table: WasmUsize) -> Result<Self> {
        let mut ranges = [GuestSlice::<u8>::new(0, 0); MAX_MAP_ENTRIES];
        let mut total = 1; // Native sentinel, even when the guest uses a null pointer.
        let mut count = 0;
        let mut terminated = false;
        for (index, range) in ranges.iter_mut().enumerate() {
            let address = table
                .checked_add((index * WasmUsize::SIZE) as WasmUsize)
                .ok_or(GraphicsError::InvalidMemory)?;
            let pointer = memory
                .read(GuestPointer::<WasmUsize>::new(address))
                .ok_or(GraphicsError::InvalidMemory)?;
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
                return Err(GraphicsError::ResourceLimit);
            }
            *range = GuestSlice::new(pointer, length as WasmUsize);
            count += 1;
        }
        if !terminated {
            return Err(GraphicsError::ResourceLimit);
        }

        // All guest input is validated before allocating retained native storage.
        let mut strings = Vec::with_capacity(total);
        for range in &ranges[..count] {
            strings.extend_from_slice(memory.bytes(*range).expect("validated map range"));
        }
        strings.push(0);
        let mut pointers = Vec::with_capacity(count + 1);
        let mut start = 0;
        for range in &ranges[..count] {
            pointers.push(strings[start..].as_ptr().cast());
            start += range.size as usize;
        }
        pointers.push(strings[start..].as_ptr().cast());
        Ok(Self {
            _strings: strings,
            pointers,
        })
    }

    pub fn as_ptr(&self) -> *const *const c_char {
        self.pointers.as_ptr()
    }
}
