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
use exported_file_system::Flags;
pub use file::*;
pub use file_system::*;
pub use hierarchy::*;
pub use home::user_home;
pub use item::*;
pub use synchronous_directory::*;
pub use synchronous_file::*;

pub extern crate file_system as exported_file_system;

/// Run synchronous metadata once. Busy is an error, not an implicit wait.
pub async fn poll<O>(operation: impl FnOnce() -> Result<O>) -> Result<O> {
    operation()
}

pub fn blocking_operation<O>(_flags: Flags, operation: impl FnOnce() -> Result<O>) -> Result<O> {
    operation()
}
