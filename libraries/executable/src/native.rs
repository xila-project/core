use abi_application::{HomeGrant, ProcessHost, ProcessLaunch, SpawnFuture, SpawnRequest, Status};
use alloc::{boxed::Box, vec::Vec};
use file_system::IntoIoErrorKind;
use native_loader::{ExecutableMemory, LoadedImage, Startup};
use stackful::{MIN_STACK_SIZE, Stack};
use virtual_file_system::VirtualFileSystem;

use crate::{
    Error, ExecutableContext, ExecutableFormat, FormatHandler, FormatLaunch, FormatRequest, Home,
    Launch, NetworkRights, Result, Standard, execute_inner,
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
        Error::NativeFileTooLarge => Status::OutOfMemory,
        Error::Native(
            native_loader::Error::AllocationFailed | native_loader::Error::ImageTooLarge,
        ) => Status::OutOfMemory,
        Error::Native(native_loader::Error::FinalizationFailed) => Status::InputOutput,
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
                path,
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
            let launch = Launch {
                environment,
                home: home.map(|grant| Home {
                    path: grant.path,
                    rights: grant.rights,
                }),
                identity: None,
                // Checked against the parent's rights by `xila_process_spawn`.
                network: u8::try_from(network)
                    .ok()
                    .and_then(NetworkRights::from_bits)
                    .ok_or(Status::InvalidArgument)?,
            };
            // The child's coroutine must stay on the spawner's executor.
            execute_inner(self, &path, arguments, standard, None, launch)
                .await
                .map_err(status)
        })
    }
}

/// Developer-only handler for raw ELF files on a trusted filesystem.
pub struct NativeFormat {
    memory: &'static dyn ExecutableMemory,
    stack_size: usize,
    file_limit: usize,
    image_limit: usize,
}

impl NativeFormat {
    /// Configure executable memory and budgets for unrelocated file bytes,
    /// installed image bytes and usable coroutine stack bytes.
    ///
    /// # Safety
    /// Every ELF reachable through this handler must be trusted to run in
    /// firmware, implement the current startup ABI, and fit on the chosen
    /// stack. The platform must satisfy the executor and single-core
    /// restrictions of `LoadedImage::spawn`. Files must be protected against
    /// untrusted modification for the entire load.
    pub const unsafe fn new(
        memory: &'static dyn ExecutableMemory,
        stack_size: usize,
        file_limit: usize,
        image_limit: usize,
    ) -> Self {
        Self {
            memory,
            stack_size,
            file_limit,
            image_limit,
        }
    }

    pub const fn registration(&'static self) -> ExecutableFormat {
        ExecutableFormat::new(*native_loader::ELF_MAGIC, self)
    }

    async fn load(&self, request: &mut FormatRequest) -> Result<LoadedImage> {
        let size = usize::try_from(request.file_size)
            .ok()
            .filter(|size| *size <= self.file_limit)
            .ok_or(Error::NativeFileTooLarge)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(size)
            .map_err(|_| Error::Native(native_loader::Error::AllocationFailed))?;
        bytes.resize(size, 0);
        let mut unread = bytes.as_mut_slice();
        while !unread.is_empty() {
            let count = request.file.read(unread).await?;
            if count == 0 {
                return Err(Error::Native(native_loader::Error::Truncated));
            }
            unread = &mut unread[count..];
        }
        if request.file.read(&mut [0]).await? != 0 {
            return Err(Error::NativeFileTooLarge);
        }
        LoadedImage::load(&bytes, self.memory, self.image_limit).map_err(Error::Native)
    }
}

impl FormatHandler for NativeFormat {
    fn launch(&self, mut request: FormatRequest) -> FormatLaunch<'_> {
        Box::pin(async move {
            if self.stack_size < MIN_STACK_SIZE {
                return Err(Error::InvalidStackSize);
            }
            let context = request.standard.context;
            let manager = context.task_manager;
            if let Some(spawner) = request.spawner
                && spawner != manager.get_spawner(request.parent).await?
            {
                // Native coroutines must stay on the caller's executor.
                return Err(task::Error::InvalidSpawnerIdentifier.into());
            }
            abi_application::initialize(context.time_manager);
            abi_application::initialize_random(context.virtual_file_system).await;

            // Allocate the stack, fallibly, before the executable image: running out of
            // stack memory must not leave an installed image to unwind.
            let stack = Stack::try_with_min_size(self.stack_size, MIN_STACK_SIZE)
                .ok_or(Error::Native(native_loader::Error::AllocationFailed))?;
            let image = self.load(&mut request).await?;
            let (input, output, error) = request.standard.split();
            let wrap = |file: virtual_file_system::File<'static>| {
                Some(abi_application::File::new(file.into_synchronous_file()))
            };
            let launch = request.launch;
            let startup = Startup {
                arguments: request.arguments,
                standard_input: wrap(input),
                standard_output: wrap(output),
                standard_error: wrap(error),
                process: Some(ProcessLaunch {
                    host: context,
                    environment: launch.environment,
                    home: launch.home.map(|home| HomeGrant {
                        path: home.path,
                        rights: home.rights,
                    }),
                    network: u32::from(launch.network.bits()),
                }),
            };
            let spawned = unsafe {
                image.spawn(
                    manager,
                    request.parent,
                    &request.name,
                    stack,
                    startup,
                    |status| status as isize,
                )
            };
            Ok(spawned.map_err(Error::Native)?.await?)
        })
    }
}
