use core::ffi::{c_char, c_int, c_void};

#[cfg(lvgl_system_heap)]
use abi_declarations::{xila_memory_allocate_core, xila_memory_deallocate, xila_memory_reallocate};
use abi_declarations::{
    xila_memory_compare, xila_memory_copy, xila_memory_move, xila_memory_set, xila_string_compare,
    xila_string_compare_bounded, xila_string_concatenate, xila_string_copy,
    xila_string_copy_bounded, xila_string_get_length,
};
#[cfg(lvgl_system_heap)]
use abi_declarations::{xila_string_duplicate, xila_string_duplicate_bounded};

/// Set a block of memory to a specific value
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_memset(destination: *mut c_void, value: u8, size: usize) {
    unsafe { xila_memory_set(destination, value as c_int, size) };
}

// LVGL's heap. With `LV_STDLIB_CUSTOM` (hosts, `lvgl_system_heap`) every LVGL
// allocation goes to Xila's system allocator and the hooks below implement it.
// With `LV_STDLIB_BUILTIN` (microcontrollers) LVGL manages its own fixed pool
// (see `include/lv_conf.h` and the README) and provides all of these itself,
// so they must not be defined here.
//
// `build.rs` queries the selected C header; this guards against it
// disagreeing with `lv_conf.h` (which would otherwise surface as duplicate or
// undefined `lv_mem_*` symbols at link time).
const _: () = assert!(
    (crate::lvgl::LV_USE_STDLIB_MALLOC == crate::lvgl::LV_STDLIB_CUSTOM) == cfg!(lvgl_system_heap),
    "lv_conf.h and build.rs disagree on LVGL's allocator (XILA_LVGL_MEM_POOL)"
);

/// Deinitialize the memory manager
///
/// # Safety
/// This function is unsafe because it may involve low-level memory operations.
#[cfg(lvgl_system_heap)]
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
#[cfg(lvgl_system_heap)]
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
#[cfg(lvgl_system_heap)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_malloc_core(size: usize) -> *mut c_void {
    unsafe { xila_memory_allocate_core(size) }
}

/// Free a block of memory
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[cfg(lvgl_system_heap)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_free_core(pointer: *mut c_void) {
    unsafe { xila_memory_deallocate(pointer) }
}

/// Reallocate a block of memory
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[cfg(lvgl_system_heap)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_realloc_core(pointer: *mut c_void, new_size: usize) -> *mut c_void {
    unsafe { xila_memory_reallocate(pointer, new_size) }
}

/// Report the memory usage through `lv_mem_monitor()`.
///
/// LVGL shares Xila's system heap in this configuration, so this reports the
/// system heap as a whole, not LVGL's own share: `max_used`, the block counts
/// and the biggest free block are not tracked. Use the built-in pool (see the
/// README) to measure LVGL's own peak usage.
///
/// # Safety
/// `monitor` must be null or point to a writable `lv_mem_monitor_t`.
#[cfg(lvgl_system_heap)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_mem_monitor_core(monitor: *mut crate::lvgl::lv_mem_monitor_t) {
    let Some(monitor) = (unsafe { monitor.as_mut() }) else {
        return;
    };
    let manager = memory::global_manager();
    let free = manager.get_free();
    let total = manager.get_total_size().max(free);
    monitor.total_size = total;
    monitor.free_size = free;
    monitor.used_pct = ((total - free) as u64 * 100 / total.max(1) as u64) as u8;
}

/// Check the allocator's integrity. The system allocator has nothing to check.
///
/// # Safety
/// Always safe to call.
#[cfg(lvgl_system_heap)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_mem_test_core() -> crate::lvgl::lv_result_t {
    crate::lvgl::lv_result_t_LV_RESULT_OK
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

/// Duplicate a string.
///
/// The copy is released with `lv_free()`, so it comes from LVGL's heap.
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[cfg(lvgl_system_heap)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_strdup(string: *const c_char) -> *mut c_char {
    unsafe { xila_string_duplicate(string) }
}

/// Duplicate a string.
///
/// The copy is released with `lv_free()`, so it must come from LVGL's own pool,
/// not from the system allocator.
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[cfg(not(lvgl_system_heap))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_strdup(string: *const c_char) -> *mut c_char {
    unsafe { duplicate_into_pool(string, xila_string_get_length(string)) }
}

/// Copy `length` bytes of `string` plus a terminator into a new LVGL allocation.
///
/// # Safety
/// `string` must be readable for `length` bytes.
#[cfg(not(lvgl_system_heap))]
unsafe fn duplicate_into_pool(string: *const c_char, length: usize) -> *mut c_char {
    let copy = unsafe { crate::lvgl::lv_malloc(length + 1) } as *mut c_char;
    if !copy.is_null() {
        unsafe {
            xila_memory_copy(copy.cast(), string.cast(), length);
            *copy.add(length) = 0;
        }
    }
    copy
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
/// The copy is released with `lv_free()`, so it comes from LVGL's heap.
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[cfg(lvgl_system_heap)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_strndup(string: *const c_char, size: usize) -> *mut c_char {
    unsafe { xila_string_duplicate_bounded(string, size) }
}

/// Duplicate a string up to a given size.
///
/// The copy is released with `lv_free()`, so it must come from LVGL's own pool,
/// not from the system allocator.
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[cfg(not(lvgl_system_heap))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv_strndup(string: *const c_char, size: usize) -> *mut c_char {
    let mut length = 0;
    while length < size && unsafe { *string.add(length) } != 0 {
        length += 1;
    }
    unsafe { duplicate_into_pool(string, length) }
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
