use abi_application::{ProcessHost, SpawnFuture, SpawnRequest, Status};
use alloc::boxed::Box;
use file_system::IntoIoErrorKind;
use virtual_file_system::VirtualFileSystem;

use crate::{
    Error, ExecutableContext, ExecutableSource, Launch, NetworkRights, Standard, execute_inner,
};

/// The status a failed spawn reports to the native caller.
fn status(error: Error) -> Status {
    match error {
        Error::FileSystem(kind) => kind.into(),
        Error::Task(error) => error.into_io_error_kind().into(),
        Error::PermissionDenied => Status::PermissionDenied,
        // The file is not an image this firmware can run.
        Error::FailedToGetMainFunction => Status::ExecFormat,
        Error::InvalidStackSize => Status::InvalidState,
        #[cfg(feature = "native-dev")]
        Error::NativeFileTooLarge => Status::OutOfMemory,
        #[cfg(feature = "native-dev")]
        Error::Native(
            native_loader::Error::AllocationFailed | native_loader::Error::ImageTooLarge,
        ) => Status::OutOfMemory,
        #[cfg(feature = "native-dev")]
        Error::Native(native_loader::Error::FinalizationFailed) => Status::InputOutput,
        #[cfg(feature = "native-dev")]
        Error::Native(_) => Status::ExecFormat,
    }
}

/// A native process spawns children through the executable context it was launched
/// from: the same permission checks, formats and task tree as any other launch.
impl ProcessHost for ExecutableContext {
    fn task_manager(&self) -> &'static task::Manager {
        self.task_manager
    }

    fn virtual_file_system(&self) -> &'static VirtualFileSystem {
        self.virtual_file_system
    }

    #[cfg(feature = "graphics")]
    fn graphics_manager(&self) -> Option<&'static graphics::Manager> {
        self.graphics_manager
    }

    #[cfg(feature = "network")]
    fn network_manager(&self) -> Option<&'static network::Manager> {
        self.network_manager
    }

    fn spawn(&'static self, request: SpawnRequest) -> SpawnFuture {
        Box::pin(async move {
            let SpawnRequest {
                name,
                executable,
                statistics,
                arguments,
                environment,
                standard_input,
                standard_output,
                standard_error,
                home,
                network,
                ..
            } = request;
            let standard = Standard::new(self, standard_input, standard_output, standard_error);
            #[cfg(not(feature = "native-dev"))]
            let _ = home;
            let launch = Launch {
                environment,
                home: None,
                #[cfg(feature = "native-dev")]
                pinned_home: home,
                identity: None,
                // Checked against the parent's rights by `xila_process_spawn`.
                network: u8::try_from(network)
                    .ok()
                    .and_then(NetworkRights::from_bits)
                    .ok_or(Status::InvalidArgument)?,
            };
            // The child's coroutine must stay on the spawner's executor.
            execute_inner(
                self,
                ExecutableSource::Opened {
                    name: &name,
                    file: executable,
                    statistics,
                },
                arguments,
                standard,
                None,
                launch,
            )
            .await
            .map_err(status)
        })
    }
}
