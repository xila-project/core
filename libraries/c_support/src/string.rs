use core::{
    cmp::{Ordering, min},
    ffi::{c_char, c_int},
    ptr::null_mut,
    slice,
};

use crate::memory::memory_allocate_core;

/// Get the length of a null-terminated string
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
pub unsafe fn string_get_length(str: *const c_char) -> usize {
    unsafe {
        if str.is_null() {
            return 0;
        }

        let mut len = 0;
        let mut ptr = str;
        while *ptr != 0 {
            len += 1;
            ptr = ptr.add(1);
        }
        len
    }
}

/// Get the length of a null-terminated string with maximum length
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
pub unsafe fn string_get_length_bounded(str: *const c_char, maxlen: usize) -> usize {
    unsafe {
        if str.is_null() {
            return 0;
        }

        let mut len = 0;
        let mut ptr = str;
        while len < maxlen && *ptr != 0 {
            len += 1;
            ptr = ptr.add(1);
        }
        len
    }
}

/// Compare two null-terminated strings
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
pub unsafe fn string_compare(str1: *const c_char, str2: *const c_char) -> c_int {
    unsafe {
        if str1.is_null() || str2.is_null() {
            return if str1.is_null() && str2.is_null() {
                0
            } else if str1.is_null() {
                -1
            } else {
                1
            };
        }

        let len1 = string_get_length(str1);
        let len2 = string_get_length(str2);

        // Use Rust's slice comparison for efficiency
        let slice1 = slice::from_raw_parts(str1 as *const u8, len1);
        let slice2 = slice::from_raw_parts(str2 as *const u8, len2);

        match slice1.cmp(slice2) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        }
    }
}

/// Compare two strings up to n characters
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
pub unsafe fn string_compare_bounded(
    str1: *const c_char,
    str2: *const c_char,
    num: usize,
) -> c_int {
    unsafe {
        if str1.is_null() || str2.is_null() || num == 0 {
            return if str1.is_null() && str2.is_null() {
                0
            } else if str1.is_null() {
                -1
            } else {
                1
            };
        }

        let len1 = min(string_get_length(str1), num);
        let len2 = min(string_get_length(str2), num);
        let min_len = min(len1, len2);

        // Use Rust's slice comparison for the overlapping part
        let slice1 = slice::from_raw_parts(str1 as *const u8, min_len);
        let slice2 = slice::from_raw_parts(str2 as *const u8, min_len);

        use core::cmp::Ordering;
        match slice1.cmp(slice2) {
            Ordering::Less => -1,
            Ordering::Greater => 1,
            Ordering::Equal => {
                // If the compared parts are equal, compare lengths
                match len1.cmp(&len2) {
                    Ordering::Less => -1,
                    Ordering::Equal => 0,
                    Ordering::Greater => 1,
                }
            }
        }
    }
}

/// Copy string from source to destination
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
pub unsafe fn string_copy(destination: *mut c_char, source: *const c_char) -> *mut c_char {
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

/// Copy string from source to destination with maximum length
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
pub unsafe fn string_copy_bounded(
    destination: *mut c_char,
    source: *const c_char,
    num: usize,
) -> *mut c_char {
    unsafe {
        if destination.is_null() || source.is_null() || num == 0 {
            return destination;
        }

        let mut dst_ptr = destination;
        let mut src_ptr = source;
        let mut count = 0;

        // Copy characters until null terminator or num reached
        while count < num && *src_ptr != 0 {
            *dst_ptr = *src_ptr;
            dst_ptr = dst_ptr.add(1);
            src_ptr = src_ptr.add(1);
            count += 1;
        }

        // Pad with null bytes if necessary
        while count < num {
            *dst_ptr = 0;
            dst_ptr = dst_ptr.add(1);
            count += 1;
        }

        destination
    }
}

/// Duplicate a string
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
pub unsafe fn string_duplicate(string: *const c_char) -> *mut c_char {
    if string.is_null() {
        return null_mut();
    }

    let new_string = unsafe {
        let length = string_get_length(string);
        memory_allocate_core(length + 1)
    };

    if new_string.is_null() {
        return null_mut();
    }

    // Copy the string into the newly allocated memory
    unsafe {
        string_copy(new_string as *mut c_char, string);
    }

    new_string as *mut c_char
}

/// Duplicate a string up to a maximum length
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
pub unsafe fn string_duplicate_bounded(string: *const c_char, max_length: usize) -> *mut c_char {
    if string.is_null() {
        return null_mut();
    }

    // Like `strndup`, a zero bound yields an empty string, not a null pointer.
    let length = unsafe { string_get_length_bounded(string, max_length) };
    let new_string = unsafe { memory_allocate_core(length + 1) } as *mut c_char;

    if new_string.is_null() {
        return null_mut();
    }

    // The bounded copy stops after `length` bytes without a terminator when
    // the source is longer, so the terminator is always written here.
    unsafe {
        string_copy_bounded(new_string, string, length);
        *new_string.add(length) = 0;
    }

    new_string
}

/// Concatenate source string to destination string
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
pub unsafe fn string_concatenate(
    destination: *mut c_char,
    mut source: *const c_char,
) -> *mut c_char {
    if destination.is_null() || source.is_null() {
        return destination;
    }

    // Find the end of dest string
    let mut destination_end = destination;
    unsafe {
        while *destination_end != 0 {
            destination_end = destination_end.add(1);
        }
    }

    unsafe {
        // Copy src to the end of dest (including null terminator)
        while *source != 0 {
            *destination_end = *source;
            destination_end = destination_end.add(1);
            source = source.add(1);
        }
        // Add null terminator
        *destination_end = 0;
    }

    destination
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_deallocate;
    use core::ffi::CStr;

    #[test]
    fn bounded_duplicate_is_always_terminated() {
        // Poison the heap so that a missing terminator cannot hide behind
        // freshly zeroed memory.
        unsafe {
            for _ in 0..8 {
                let poison = memory_allocate_core(8) as *mut u8;
                if !poison.is_null() {
                    poison.write_bytes(b'X', 8);
                    memory_deallocate(poison as *mut _);
                }
            }
            for (source, bound, expected) in [
                (c"abcdefg", 3, c"abc"),
                (c"abcdefg", 7, c"abcdefg"),
                (c"abc", 16, c"abc"),
                (c"abc", 0, c""),
            ] {
                let copy = string_duplicate_bounded(source.as_ptr(), bound);
                if copy.is_null() {
                    panic!("duplicate of {source:?} bounded to {bound} is null");
                }
                assert_eq!(CStr::from_ptr(copy), expected);
                memory_deallocate(copy as *mut _);
            }
            assert!(string_duplicate_bounded(core::ptr::null(), 4).is_null());
        }
    }
}
