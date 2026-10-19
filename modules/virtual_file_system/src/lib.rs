#![no_std]

extern crate alloc;

embedded_i18n::track_locales!();

mod cursor;
mod directory;
mod error;
mod file;
mod file_system;
mod hierarchy;
mod home;
mod item;
mod r#macro;
mod operation;
mod pipe;
mod synchronous_directory;
mod synchronous_file;

pub use directory::*;
pub use error::*;
use exported_file_system::{Flags, StateFlags};
pub use file::*;
pub use file_system::*;
pub use hierarchy::*;
pub use home::user_home;
pub use item::*;
pub use synchronous_directory::*;
pub use synchronous_file::*;

pub extern crate file_system as exported_file_system;

pub async fn poll<O>(mut operation: impl FnMut() -> Result<O>) -> Result<O> {
    loop {
        match operation() {
            Err(Error::FileSystem(::file_system::Error::RessourceBusy))
            | Err(Error::RessourceBusy) => {
                task::yield_now().await;
            }
            other => return other,
        }
    }
}

pub fn blocking_operation<O>(flags: Flags, mut operation: impl FnMut() -> Result<O>) -> Result<O> {
    let non_blocking = flags.get_state().contains(StateFlags::NonBlocking);

    loop {
        match operation() {
            Err(Error::FileSystem(::file_system::Error::RessourceBusy))
            | Err(Error::RessourceBusy) => {
                if non_blocking {
                    return Err(Error::FileSystem(::file_system::Error::RessourceBusy));
                }
            }
            other => return other,
        }
    }
}
