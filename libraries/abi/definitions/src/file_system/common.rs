use core::{
    ffi::{CStr, c_char},
    num::NonZeroU32,
};
use virtual_file_system::Error;

use crate::XilaFileSystemResult;

/// This function is used to convert a function returning a Result into a u32.
pub fn into_result<F>(function: F) -> XilaFileSystemResult
where
    F: FnOnce() -> Result<(), virtual_file_system::Error>,
{
    match function() {
        Ok(()) => 0,
        Err(error) => {
            let non_zero: NonZeroU32 = error.into();

            if matches!(
                error,
                Error::RessourceBusy | Error::FileSystem(file_system::Error::RessourceBusy)
            ) {
                log::debug!(
                    "File system busy (expected while polling): {:?} ({})",
                    error,
                    non_zero
                );
            } else {
                log::error!("File system error: {:?} ({})", error, non_zero);
            }

            //panic!("File system error: {:?} ({})", error, non_zero.get());

            non_zero.get()
        }
    }
}

/// # Safety
/// The caller must ensure that the provided pointer is a vlid null-terminated C string.
#[inline]
pub unsafe fn parse_c_str(path: *const c_char) -> Result<&'static str, Error> {
    if path.is_null() {
        return Err(Error::InvalidParameter);
    }
    unsafe {
        CStr::from_ptr(path)
            .to_str()
            .map_err(|_| Error::InvalidParameter)
    }
}

#[macro_export]
macro_rules! abi_unsafe_function {
    (
        $(#[$meta:meta])*
        fn $name:ident ( $($arg:ident : $ty:ty),* $(,)? ) -> XilaFileSystemResult $body:block
    ) => {
        $(#[$meta])*
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name ( $($arg : $ty),* ) -> $crate::XilaFileSystemResult {
            log::debug!("Calling function {}", stringify!($name));

            // We use an immediately invoked anonymous closure (|| { ... })()
            // This captures variables naturally and gives the `?` operator a target scope to return from.
            #[allow(unused_unsafe)]
            #[allow(clippy::macro_metavars_in_unsafe)]
            let f = || unsafe {
                $body
            };

            let result: Result<(), virtual_file_system::Error> = f();

            match result {
                Ok(()) => 0,
                Err(virtual_file_system::Error::RessourceBusy | virtual_file_system::Error::FileSystem(file_system::Error::RessourceBusy)) => {
                    log::debug!(
                        "File system busy (expected while polling) in function {}: {:?}",
                        stringify!($name),
                        result
                    );
                    1 // Specific code for resource busy, if needed
                },
                Err(error) => {
                    let non_zero: core::num::NonZeroU32 = error.into();

                    log::error!(
                        "File system error when calling {}: {:?} ({})",
                        stringify!($name),
                        error,
                        non_zero
                    );


                    non_zero.get()
                }
            }
        }
    };
}
