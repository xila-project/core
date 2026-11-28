use abi_application::{HomeGrant, ProcessLaunch};
use alloc::{boxed::Box, vec::Vec};
use native_loader::{ExecutableMemory, LoadedImage, Startup};
use stackful::{MIN_STACK_SIZE, Stack};

use crate::{Error, ExecutableFormat, FormatHandler, FormatLaunch, FormatRequest, Result};

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
                    home: launch.pinned_home.or_else(|| {
                        launch.home.map(|home| HomeGrant::Path {
                            path: home.path,
                            rights: home.rights,
                        })
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
