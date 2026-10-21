//! VM-independent graphics memory adapters.

use super::error::{GraphicsError, Result};
use crate::host::translation::{GuestMemory, GuestPointer, GuestSlice, GuestValue, WasmUsize};
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
        .ok_or(GraphicsError::InvalidValue)?
        .checked_add(previous.max(new))
        .ok_or(GraphicsError::ResourceLimit)?;
    if used > MAX_WIDGET_OPTIONS_BYTES_PER_VM {
        Err(GraphicsError::ResourceLimit)
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
        Err(GraphicsError::InvalidValue)
    }
}

/// Stable host-owned LVGL grid descriptors. LVGL retains both pointers until
/// the grid layout is replaced or its object is deleted.
#[derive(Debug, PartialEq, Eq)]
pub struct GridTracks {
    tracks: Vec<i32>,
    columns_len: usize,
}

impl GridTracks {
    pub fn decode(
        memory: &GuestMemory<'_>,
        columns: WasmUsize,
        rows: WasmUsize,
        template_last: i32,
        content: i32,
    ) -> Result<Self> {
        fn scan_axis(
            memory: &GuestMemory<'_>,
            offset: WasmUsize,
            template_last: i32,
            content: i32,
        ) -> Result<([i32; MAX_GRID_TRACKS_PER_AXIS], usize)> {
            let mut values = [0; MAX_GRID_TRACKS_PER_AXIS];
            let fr_min = template_last - 100;
            for (index, value) in values.iter_mut().enumerate() {
                let address = offset
                    .checked_add((index * i32::SIZE) as WasmUsize)
                    .ok_or(GraphicsError::InvalidMemory)?;
                let descriptor = memory
                    .read(GuestPointer::<i32>::new(address))
                    .ok_or(GraphicsError::InvalidMemory)?;
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
                    return Err(GraphicsError::InvalidValue);
                }
                *value = descriptor;
            }
            Err(GraphicsError::ResourceLimit)
        }

        let (columns, columns_len) = scan_axis(memory, columns, template_last, content)?;
        let (rows, rows_len) = scan_axis(memory, rows, template_last, content)?;
        let mut tracks = Vec::with_capacity(columns_len + rows_len);
        tracks.extend_from_slice(&columns[..columns_len]);
        tracks.extend_from_slice(&rows[..rows_len]);
        Ok(Self {
            tracks,
            columns_len,
        })
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub fn columns(&self) -> &[i32] {
        &self.tracks[..self.columns_len]
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub fn rows(&self) -> &[i32] {
        &self.tracks[self.columns_len..]
    }

    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }

    pub fn columns_ptr(&self) -> *const i32 {
        self.tracks.as_ptr()
    }

    pub fn rows_ptr(&self) -> *const i32 {
        self.tracks.as_ptr().wrapping_add(self.columns_len)
    }
}

pub fn read_c_string<'a>(memory: &'a GuestMemory<'_>, offset: WasmUsize) -> Result<&'a CStr> {
    read_c_string_with_limit(memory, offset, MAX_STRING_BYTES)
}

pub fn read_textarea_string<'a>(
    memory: &'a GuestMemory<'_>,
    offset: WasmUsize,
) -> Result<&'a CStr> {
    read_c_string_with_limit(memory, offset, MAX_TEXTAREA_BYTES)
}

/// Reads newline-separated widget options. LVGL's dynamic setters copy this
/// string, so the returned borrow is only valid for the current host call.
pub fn read_widget_options<'a>(memory: &'a GuestMemory<'_>, offset: WasmUsize) -> Result<&'a CStr> {
    let options = read_c_string_with_limit(memory, offset, MAX_WIDGET_OPTIONS_BYTES)?;
    let entries = options.to_bytes().split(|byte| *byte == b'\n');
    let mut count = 0;
    for entry in entries {
        count += 1;
        if entry.len() >= MAX_STRING_BYTES {
            return Err(GraphicsError::ResourceLimit);
        }
    }
    if count > MAX_WIDGET_OPTIONS {
        return Err(GraphicsError::ResourceLimit);
    }
    Ok(options)
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
