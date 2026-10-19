use core::ffi::c_int;

use crate::{XilaTime, XilaTimeClockIdentifier};

use super::RawMutex;

pub type XilaConditionVariable = usize;
pub type XilaConditionVariableAttribute = usize;

/// This function is used to create a condition variable.
///
/// # Safety
///
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condition_variable_new(
    _condition_variable: *mut XilaConditionVariable,
) -> u32 {
    todo!()
}

/// This function is used to initialize a condition variable.
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condition_variable_initialize(
    _condition_variable: *mut XilaConditionVariable,
    _attribute: *const XilaConditionVariableAttribute,
) -> u32 {
    todo!()
}

/// This function is used to delete a condition variable.
///
/// # Safety
///
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condition_variable_remove(
    _condition_variable: *mut XilaConditionVariable,
) -> u32 {
    todo!()
}

/// This function is used to wait a condition variable.
///
/// # Safety
///
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condition_variable_wait(
    _condition_variable: *mut XilaConditionVariable,
    _mutex: *mut RawMutex,
) -> u32 {
    todo!()
}

/// This function is used to try wait a condition variable.
///
/// # Safety
///
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condition_variable_try_wait(
    _condition_variable: *mut XilaConditionVariable,
    _mutex: *mut RawMutex,
    _time: XilaTime,
) -> u32 {
    todo!()
}

/// This function is used to signal a condition variable.
///
/// # Safety
///
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condition_variable_signal(
    _condition_variable: *mut XilaConditionVariable,
) -> u32 {
    todo!()
}

/// This function is used to broadcast a condition variable.
///
/// # Safety
///
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condition_variable_broadcast(
    _condition_variable: *mut XilaConditionVariable,
) -> u32 {
    todo!()
}

/// This function is used to create a condition variable attribute.
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condition_variable_attribute_initialize(
    _attribute: *mut XilaConditionVariableAttribute,
) -> c_int {
    todo!()
}

/// This function is used to set the clock of a condition variable attribute.
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condition_variable_attribute_set_clock(
    _attribute: *mut XilaConditionVariableAttribute,
    _clock: XilaTimeClockIdentifier,
) -> c_int {
    todo!()
}

/// This function is used to destroy a condition variable attribute.
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condition_variable_attribute_destroy(
    _attribute: *mut XilaConditionVariableAttribute,
) -> c_int {
    todo!()
}
