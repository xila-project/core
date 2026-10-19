use core::{
    cmp::Ordering,
    ffi::{c_int, c_void},
    ptr::{copy, copy_nonoverlapping, write_bytes},
    slice,
};

/// Copy memory from source to destination
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_memory_copy(
    destination: *mut c_void,
    source: *const c_void,
    size: usize,
) -> *mut c_void {
    if destination.is_null() || source.is_null() || size == 0 {
        return destination;
    }

    // Use Rust's efficient copy_nonoverlapping
    unsafe {
        copy_nonoverlapping(source as *const u8, destination as *mut u8, size);
    }
    destination
}

/// Compare memory blocks
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_memory_compare(
    ptr1: *const c_void,
    ptr2: *const c_void,
    num: usize,
) -> c_int {
    if ptr1.is_null() || ptr2.is_null() || num == 0 {
        return 0;
    }

    // Use Rust's slice comparison which is optimized
    let bytes1 = unsafe { slice::from_raw_parts(ptr1 as *const u8, num) };
    let bytes2 = unsafe { slice::from_raw_parts(ptr2 as *const u8, num) };

    // Use Rust's cmp which returns Ordering
    match bytes1.cmp(bytes2) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// Move memory from source to destination (handles overlapping regions)
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_memory_move(
    destination: *mut c_void,
    source: *const c_void,
    size: usize,
) -> *mut c_void {
    if destination.is_null() || source.is_null() || size == 0 {
        return destination;
    }

    // Use Rust's copy which handles overlapping memory correctly
    unsafe {
        copy(source as *const u8, destination as *mut u8, size);
    }
    destination
}

/// Set memory to a specific value
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_memory_set(
    ptr: *mut c_void,
    value: c_int,
    num: usize,
) -> *mut c_void {
    if ptr.is_null() || num == 0 {
        return ptr;
    }

    // Use Rust's efficient write_bytes
    unsafe {
        write_bytes(ptr as *mut u8, value as u8, num);
    }
    ptr
}
