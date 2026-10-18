#![no_std]

extern crate alloc;

xila::internationalization::track_locales!();

mod error;
mod file_manager;

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use core::num::NonZeroUsize;
use xila::executable::{self, ExecutableTrait, Standard};
use xila::task::TaskIdentifier;
use xila::virtual_file_system::{File, VirtualFileSystem};

pub use error::*;
pub use file_manager::*;

pub const SHORTCUT: &str = r#"
{
    "name": "File manager",
    "command": "/binaries/file_manager",
    "arguments": [],
    "terminal": false,
    "icon_string": "Fm",
    "icon_color": [0, 188, 212]
}"#;

pub struct FileManagerExecutable;

impl FileManagerExecutable {
    pub async fn new(
        virtual_file_system: &VirtualFileSystem,
        task: TaskIdentifier,
    ) -> core::result::Result<Self, String> {
        let _ = virtual_file_system
            .create_directory(task, &"/configuration/shared/shortcuts")
            .await;

        File::write_to_path(
            virtual_file_system,
            task,
            "/configuration/shared/shortcuts/file_manager.json",
            application_metadata::shortcut("file_manager")
                .unwrap()
                .as_bytes(),
        )
        .await
        .map_err(|error| error.to_string())?;

        Ok(Self)
    }
}

impl ExecutableTrait for FileManagerExecutable {
    fn main(standard: Standard<'static>, arguments: Vec<String>) -> executable::MainFuture {
        Box::pin(async move { main(standard, arguments).await })
    }
}

pub async fn main(
    standard: Standard<'static>,
    _: Vec<String>,
) -> core::result::Result<(), NonZeroUsize> {
    let mut file_manager = FileManager::new(standard.context)
        .await
        .map_err(|_| NonZeroUsize::new(1).unwrap())?;

    while file_manager.handle_events().await {}

    Ok(())
}
