use crate::main;
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use xila::executable::ExecutableTrait;
use xila::task::TaskIdentifier;
use xila::virtual_file_system::{File, VirtualFileSystem};

pub struct TerminalExecutable;

impl TerminalExecutable {
    pub async fn new(
        virtual_file_system: &VirtualFileSystem,
        task: TaskIdentifier,
    ) -> Result<Self, String> {
        let _ = virtual_file_system
            .create_directory(task, &"/configuration/shared/shortcuts")
            .await;

        File::write_to_path(
            virtual_file_system,
            task,
            "/configuration/shared/shortcuts/terminal.json",
            application_metadata::shortcut("terminal")
                .unwrap()
                .as_bytes(),
        )
        .await
        .map_err(|error| error.to_string())?;

        Ok(Self)
    }
}

impl ExecutableTrait for TerminalExecutable {
    fn main(
        standard: xila::executable::Standard<'static>,
        arguments: alloc::vec::Vec<alloc::string::String>,
    ) -> xila::executable::MainFuture {
        Box::pin(async move { main(standard, arguments).await })
    }
}
