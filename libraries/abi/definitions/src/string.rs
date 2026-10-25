use core::{
    cmp::{Ordering, min},
    ffi::{c_char, c_int},
    ptr::null_mut,
    slice,
};

use alloc::str;

use crate::memory::xila_memory_allocate_core;

/// Helper function to convert C string to Rust str for parsing
unsafe fn c_str_to_str(ptr: *const c_char) -> Result<&'static str, ()> {
    unsafe {
        if ptr.is_null() {
            return Err(());
        }

        let len = xila_string_get_length(ptr);
        let slice = slice::from_raw_parts(ptr as *const u8, len);
        core::str::from_utf8(slice).map_err(|_| ())
    }
}

/// Get the length of a null-terminated string
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_string_get_length(str: *const c_char) -> usize {
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_string_get_length_bounded(
    str: *const c_char,
    maxlen: usize,
) -> usize {
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_string_compare(str1: *const c_char, str2: *const c_char) -> c_int {
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

        let len1 = xila_string_get_length(str1);
        let len2 = xila_string_get_length(str2);

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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_string_compare_bounded(
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

        let len1 = min(xila_string_get_length(str1), num);
        let len2 = min(xila_string_get_length(str2), num);
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_string_copy(
    destination: *mut c_char,
    source: *const c_char,
) -> *mut c_char {
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_string_copy_bounded(
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

/// Tokenize a string using delimiters
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_string_tokenize(
    _string: *mut c_char,
    delimiters: *const c_char,
) -> *mut c_char {
    // Note: This is a simplified implementation
    // A full strtok implementation requires static state management
    if delimiters.is_null() {
        return null_mut();
    }

    // This is a basic implementation that doesn't maintain state
    // In a real implementation, you'd need to track the current position
    null_mut()
}

/// Find substring in a string
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_string_find_substring(
    haystack: *const c_char,
    needle: *const c_char,
) -> *mut c_char {
    unsafe {
        if haystack.is_null() || needle.is_null() {
            return null_mut();
        }

        let needle_len = xila_string_get_length(needle);
        if needle_len == 0 {
            return haystack as *mut c_char;
        }

        let haystack_len = xila_string_get_length(haystack);
        if needle_len > haystack_len {
            return null_mut();
        }

        // Use slice operations for efficient searching
        let haystack_slice = slice::from_raw_parts(haystack as *const u8, haystack_len);
        let needle_slice = slice::from_raw_parts(needle as *const u8, needle_len);

        // Use Rust's windows iterator for efficient substring search
        for (i, window) in haystack_slice.windows(needle_len).enumerate() {
            if window == needle_slice {
                return haystack.add(i) as *mut c_char;
            }
        }

        null_mut()
    }
}

/// Convert string to double
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_string_to_double(
    nptr: *const c_char,
    endptr: *mut *mut c_char,
) -> f64 {
    unsafe {
        if nptr.is_null() {
            if !endptr.is_null() {
                *endptr = nptr as *mut c_char;
            }
            return 0.0;
        }

        // Try to parse using Rust's str parsing
        if let Ok(s) = c_str_to_str(nptr) {
            let trimmed = s.trim_start();
            if let Ok(value) = trimmed.parse::<f64>() {
                if !endptr.is_null() {
                    // Calculate how many characters were consumed
                    let consumed = s.len() - trimmed.len()
                        + trimmed.chars().take_while(|c| !c.is_whitespace()).count();
                    *endptr = nptr.add(consumed) as *mut c_char;
                }
                return value;
            }
        }

        if !endptr.is_null() {
            *endptr = nptr as *mut c_char;
        }
        0.0
    }
}

/// Case-insensitive string comparison up to n characters
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_string_compare_case_insensitive_bounded(
    str1: *const c_char,
    str2: *const c_char,
    num: usize,
) -> c_int {
    unsafe {
        if str1.is_null() || str2.is_null() || num == 0 {
            return 0;
        }

        let len1 = min(xila_string_get_length(str1), num);
        let len2 = min(xila_string_get_length(str2), num);
        let min_len = min(len1, len2);

        // Convert both slices to lowercase and compare
        let slice1 = slice::from_raw_parts(str1 as *const u8, min_len);
        let slice2 = slice::from_raw_parts(str2 as *const u8, min_len);

        for (a, b) in slice1.iter().zip(slice2.iter()) {
            let lower_a = a.to_ascii_lowercase();
            let lower_b = b.to_ascii_lowercase();
            if lower_a != lower_b {
                return if lower_a < lower_b { -1 } else { 1 };
            }
        }

        // If compared parts are equal, compare lengths
        use core::cmp::Ordering;
        match len1.cmp(&len2) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        }
    }
}

/// Convert string to unsigned long
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_string_to_unsigned_long(
    nptr: *const c_char,
    endptr: *mut *mut c_char,
    base: c_int,
) -> u64 {
    unsafe {
        if nptr.is_null() {
            if !endptr.is_null() {
                *endptr = nptr as *mut c_char;
            }
            return 0;
        }

        // Try to parse using Rust's str parsing
        if let Ok(s) = c_str_to_str(nptr) {
            let trimmed = s.trim_start();
            let radix = if base == 0 {
                // Auto-detect base like C strtoul
                if trimmed.starts_with("0x") || trimmed.starts_with("0X") {
                    16
                } else if trimmed.starts_with("0") && trimmed.len() > 1 {
                    8
                } else {
                    10
                }
            } else {
                base as u32
            };

            if (2..=36).contains(&radix) {
                let parse_str =
                    if radix == 16 && (trimmed.starts_with("0x") || trimmed.starts_with("0X")) {
                        &trimmed[2..]
                    } else {
                        trimmed
                    };

                if let Ok(value) = u64::from_str_radix(parse_str, radix) {
                    if !endptr.is_null() {
                        let consumed = s.len() - trimmed.len()
                            + if radix == 16 && parse_str != trimmed {
                                2
                            } else {
                                0
                            }
                            + parse_str
                                .chars()
                                .take_while(|c| c.is_ascii_alphanumeric())
                                .count();
                        *endptr = nptr.add(consumed) as *mut c_char;
                    }
                    return value;
                }
            }
        }

        if !endptr.is_null() {
            *endptr = nptr as *mut c_char;
        }
        0
    }
}

/// Find character in string
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_string_find_character(s: *const c_char, c: c_int) -> *mut c_char {
    unsafe {
        if s.is_null() {
            return null_mut();
        }

        let target = c as u8;
        let len = xila_string_get_length(s);
        let slice = slice::from_raw_parts(s as *const u8, len + 1); // +1 to include null terminator

        // Use Rust's efficient position finding
        if let Some(pos) = slice.iter().position(|&byte| byte == target) {
            return s.add(pos) as *mut c_char;
        }

        null_mut()
    }
}

/// Convert string to float
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_string_to_float(
    nptr: *const c_char,
    endptr: *mut *mut c_char,
) -> f32 {
    unsafe {
        if nptr.is_null() {
            if !endptr.is_null() {
                *endptr = nptr as *mut c_char;
            }
            return 0.0;
        }

        // Try to parse using Rust's str parsing
        if let Ok(s) = c_str_to_str(nptr) {
            let trimmed = s.trim_start();
            if let Ok(value) = trimmed.parse::<f32>() {
                if !endptr.is_null() {
                    // Calculate how many characters were consumed
                    let consumed = s.len() - trimmed.len()
                        + trimmed.chars().take_while(|c| !c.is_whitespace()).count();
                    *endptr = nptr.add(consumed) as *mut c_char;
                }
                return value;
            }
        }

        if !endptr.is_null() {
            *endptr = nptr as *mut c_char;
        }
        0.0
    }
}

/// Get length of initial segment of string that consists of reject characters
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_string_span_complement(
    s: *const c_char,
    reject: *const c_char,
) -> usize {
    unsafe {
        if s.is_null() || reject.is_null() {
            return 0;
        }

        let s_len = xila_string_get_length(s);
        let reject_len = xila_string_get_length(reject);

        let s_slice = slice::from_raw_parts(s as *const u8, s_len);
        let reject_slice = slice::from_raw_parts(reject as *const u8, reject_len);

        // Use Rust's iterator methods for efficient searching
        s_slice
            .iter()
            .position(|&byte| reject_slice.contains(&byte))
            .unwrap_or(s_len)
    }
}

/// Get length of initial segment of string that consists of accept characters
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_string_span(s: *const c_char, accept: *const c_char) -> usize {
    unsafe {
        if s.is_null() || accept.is_null() {
            return 0;
        }

        let s_len = xila_string_get_length(s);
        let accept_len = xila_string_get_length(accept);

        let s_slice = slice::from_raw_parts(s as *const u8, s_len);
        let accept_slice = slice::from_raw_parts(accept as *const u8, accept_len);

        // Use Rust's iterator methods for efficient searching
        s_slice
            .iter()
            .position(|&byte| !accept_slice.contains(&byte))
            .unwrap_or(s_len)
    }
}

/// Convert string to unsigned long long
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_string_to_unsigned_long_long(
    nptr: *const c_char,
    endptr: *mut *mut c_char,
    base: c_int,
) -> u64 {
    unsafe {
        // strtoull and strtoul have the same implementation for u64
        xila_string_to_unsigned_long(nptr, endptr, base)
    }
}

/// Duplicate a string
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_string_duplicate(string: *const c_char) -> *mut c_char {
    if string.is_null() {
        return null_mut();
    }

    let new_string = unsafe {
        let length = xila_string_get_length(string);
        xila_memory_allocate_core(length + 1)
    };

    if new_string.is_null() {
        return null_mut();
    }

    // Copy the string into the newly allocated memory
    unsafe {
        xila_string_copy(new_string as *mut c_char, string);
    }

    new_string as *mut c_char
}

/// Duplicate a string up to a maximum length
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_string_duplicate_bounded(
    string: *const c_char,
    max_length: usize,
) -> *mut c_char {
    if string.is_null() || max_length == 0 {
        return null_mut();
    }

    let length = unsafe { xila_string_get_length_bounded(string, max_length) };
    let new_string = unsafe { xila_memory_allocate_core(length + 1) };

    if new_string.is_null() {
        return null_mut();
    }

    // Copy the string into the newly allocated memory
    unsafe {
        xila_string_copy_bounded(new_string as *mut c_char, string, length);
    }

    new_string as *mut c_char
}

/// Parse integer from string
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_string_parse_integer(string: *const c_char) -> c_int {
    unsafe {
        if string.is_null() {
            return 0;
        }

        // Convert C string to Rust str
        if let Ok(s) = c_str_to_str(string) {
            // Parse the string as an integer
            if let Ok(value) = s.trim().parse::<c_int>() {
                return value;
            }
        }

        0 // Return 0 if parsing fails
    }
}

/// Concatenate source string to destination string
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_string_concatenate(
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
