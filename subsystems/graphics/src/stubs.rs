use core::ffi::{c_char, c_int, c_void};

use abi_declarations::{
    xila_memory_allocate_core, xila_memory_compare, xila_memory_copy, xila_memory_deallocate,
    xila_memory_move, xila_memory_reallocate, xila_memory_set, xila_string_compare,
    xila_string_compare_bounded, xila_string_concatenate, xila_string_copy,
    xila_string_copy_bounded, xila_string_duplicate, xila_string_duplicate_bounded,
    xila_string_get_length,
};

/// Set a block of memory to a specific value
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_memset(destination: *mut c_void, value: u8, size: usize) {
    unsafe { xila_memory_set(destination, value as c_int, size) };
}

/// Deinitialize the memory manager
///
/// # Safety
/// This function is unsafe because it may involve low-level memory operations.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_mem_deinit() {
    log::information!("Deinitializing memory manager");
    // This function is a no-op in this stub.
    // In a real implementation, you would clean up memory resources here.
}

/// Initialize the memory manager
///
/// # Safety
/// This function is unsafe because it may involve low-level memory operations.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_mem_init() {
    log::information!("Initializing memory manager");
    // This function is a no-op in this stub.
    // In a real implementation, you would set up memory resources here.
}

/// Allocate a block of memory
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_malloc_core(size: usize) -> *mut c_void {
    unsafe { xila_memory_allocate_core(size) }
}

/// Free a block of memory
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_free_core(pointer: *mut c_void) {
    unsafe { xila_memory_deallocate(pointer) }
}

/// Reallocate a block of memory
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_realloc_core(pointer: *mut c_void, new_size: usize) -> *mut c_void {
    unsafe { xila_memory_reallocate(pointer, new_size) }
}

/// Copy a block of memory from source to destination
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_memcpy(
    destination: *mut c_void,
    source: *const c_void,
    size: usize,
) -> *mut c_void {
    unsafe { xila_memory_copy(destination, source, size) }
}

/// Compare two strings
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_strcmp(str1: *const c_char, str2: *const c_char) -> c_int {
    unsafe { xila_string_compare(str1, str2) }
}

/// Get the length of a string
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_strlen(string: *const c_char) -> usize {
    unsafe { xila_string_get_length(string) }
}

/// Format a string
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_snprintf(
    _: *mut c_char,
    _: usize,
    _: *const c_char,
    _: *mut c_void,
) -> c_int {
    log::information!("Formatting string (not implemented in this stub)");
    // This function is not implemented in this stub.
    // In a real implementation, you would handle formatted output here.
    0
}

/// Format a string with variable arguments
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_vsnprintf(
    _: *mut c_char,
    _: usize,
    _: *const c_char,
    _: *mut c_void,
) -> c_int {
    log::information!("Formatting string with variable arguments (not implemented in this stub)");
    // This function is not implemented in this stub.
    // In a real implementation, you would handle formatted output here.
    0
}

/// Copy a string from source to destination
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_strcpy(destination: *mut c_char, source: *const c_char) -> *mut c_char {
    unsafe { xila_string_copy(destination, source) }
}

/// Copy a string up to a given size
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_strncpy(
    destination: *mut c_char,
    source: *const c_char,
    size: usize,
) -> *mut c_char {
    unsafe { xila_string_copy_bounded(destination, source, size) }
}

/// Duplicate a string
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_strdup(string: *const c_char) -> *mut c_char {
    unsafe { xila_string_duplicate(string) }
}

/// Move a block of memory from source to destination
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_memmove(
    destination: *mut c_void,
    source: *const c_void,
    size: usize,
) -> *mut c_void {
    unsafe { xila_memory_move(destination, source, size) }
}

/// Duplicate a string up to a given size
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_strndup(string: *const c_char, size: usize) -> *mut c_char {
    unsafe { xila_string_duplicate_bounded(string, size) }
}

/// Compare two memory areas up to a given size
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_memcmp(ptr1: *const c_void, ptr2: *const c_void, size: usize) -> c_int {
    unsafe { xila_memory_compare(ptr1, ptr2, size) }
}

/// Concatenate two strings
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_strcat(destination: *mut c_char, source: *const c_char) -> *mut c_char {
    unsafe { xila_string_concatenate(destination, source) }
}

/// Compare two strings up to a given size
///
/// # Safety
///
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_strncmp(
    str1: *const c_char,
    str2: *const c_char,
    size: usize,
) -> c_int {
    unsafe { xila_string_compare_bounded(str1, str2, size) }
}
