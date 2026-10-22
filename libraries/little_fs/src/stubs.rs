use core::{
    ffi::{c_char, c_void},
    ptr::null_mut,
    slice,
};

#[unsafe(no_mangle)]
pub extern "C" fn lfs_crc(mut crc: u32, data: *const c_void, size: usize) -> u32 {
    const RTABLE: [u32; 16] = [
        0x00000000, 0x1db71064, 0x3b6e20c8, 0x26d930ac, 0x76dc4190, 0x6b6b51f4, 0x4db26158,
        0x5005713c, 0xedb88320, 0xf00f9344, 0xd6d6a3e8, 0xcb61b38c, 0x9b64c2b0, 0x86d3d2d4,
        0xa00ae278, 0xbdbdf21c,
    ];

    if data.is_null() {
        return crc;
    }

    let data = unsafe { slice::from_raw_parts(data as *const u8, size) };

    for &byte in data {
        let byte = byte as u32;
        crc = (crc >> 4) ^ RTABLE[((crc ^ byte) & 0xf) as usize];
        crc = (crc >> 4) ^ RTABLE[((crc ^ (byte >> 4)) & 0xf) as usize];
    }

    crc
}

#[unsafe(no_mangle)]
pub extern "C" fn strcpy(destination: *mut c_char, source: *const c_char) -> *mut c_char {
    if destination.is_null() || source.is_null() {
        return destination;
    }

    let mut dst_ptr = destination;
    let mut src_ptr = source;

    unsafe {
        // Copy characters until null terminator
        while *src_ptr != 0 {
            *dst_ptr = *src_ptr;
            dst_ptr = dst_ptr.add(1);
            src_ptr = src_ptr.add(1);
        }

        // Null-terminate the destination string
        *dst_ptr = 0;
    }

    destination
}

/// Allocates memory of the given size.
///
/// # Safety
///
/// The caller must ensure that the allocated memory is properly freed
/// using `lfs_free` to avoid memory leaks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lfs_malloc(size: usize) -> *mut c_void {
    unsafe {
        abi_declarations::xila_memory_allocate(
            null_mut(),
            size,
            1,
            abi_declarations::XILA_MEMORY_CAPABILITIES_NONE,
        )
    }
}

/// Frees memory allocated with lfs_malloc
///
/// # Safety
///
/// The caller must ensure that the pointer was allocated with lfs_malloc
/// and has not already been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lfs_free(p: *mut c_void) {
    unsafe {
        abi_declarations::xila_memory_deallocate(p);
    }
}
