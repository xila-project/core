#![no_std]

extern crate alloc;

#[cfg(test)]
extern crate std;

xila::internationalization::track_locales!();

#[cfg(test)]
mod appearance;
mod error;
mod settings;
mod tabs;

#[cfg(all(test, target_os = "linux"))]
drivers_std::memory::instantiate_global_allocator!();

use alloc::boxed::Box;
pub use error::*;
pub use settings::*;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::num::NonZeroUsize;
use xila::executable::{self, ExecutableTrait, Standard};
use xila::task::TaskIdentifier;
use xila::virtual_file_system::{File, VirtualFileSystem};

pub fn get_shortcut() -> alloc::string::String {
    application_metadata::shortcut("settings").unwrap()
}

pub struct SettingsExecutable;

impl SettingsExecutable {
    pub async fn new(
        virtual_file_system: &VirtualFileSystem,
        task: TaskIdentifier,
    ) -> core::result::Result<Self, String> {
        let _ = virtual_file_system
            .create_directory(task, &"/configuration/shared/applications/shortcuts")
            .await;

        File::write_to_path(
            virtual_file_system,
            task,
            "/configuration/shared/applications/shortcuts/settings.json",
            get_shortcut().as_bytes(),
        )
        .await
        .map_err(|error| error.to_string())?;

        Ok(Self)
    }
}

impl ExecutableTrait for SettingsExecutable {
    fn main(standard: Standard<'static>, arguments: Vec<String>) -> executable::MainFuture {
        Box::pin(async move { main(standard, arguments).await })
    }
}

pub async fn main(
    standard: Standard<'static>,
    _: Vec<String>,
) -> core::result::Result<(), NonZeroUsize> {
    let mut settings = Settings::new(standard.context)
        .await
        .map_err(|_| NonZeroUsize::new(1).unwrap())?;

    // Run the main loop
    while settings.handle_events().await {}

    Ok(())
}
