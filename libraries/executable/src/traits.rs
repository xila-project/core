use crate::Standard;
use alloc::{boxed::Box, string::String, vec::Vec};
use core::{num::NonZeroUsize, pin::Pin};
use file_system::{
    ControlCommand, ControlCommandIdentifier, DirectBaseOperations, DirectCharacterDevice,
    MountOperations, define_command,
};
use shared::AnyByLayout;

define_command!(GET_MAIN_FUNCTION, Read, b'E', 1, (), MainFunction);

pub trait ExecutableTrait: 'static + Send + Sync {
    fn main(standard: Standard<'static>, arguments: Vec<String>) -> MainFuture;
}

pub struct ExecutableContext {
    pub task_manager: &'static task::Manager,
    pub users_manager: &'static users::Manager,
    pub virtual_file_system: &'static virtual_file_system::VirtualFileSystem,
    #[cfg(feature = "graphics")]
    pub graphics_manager: Option<&'static graphics::Manager>,
    pub network_manager: Option<&'static network::Manager>,
    pub time_manager: &'static time::Manager<'static>,
}

pub type MainFuture =
    Pin<Box<dyn Future<Output = core::result::Result<(), NonZeroUsize>> + 'static>>;

pub type MainFunction = Option<
    Box<dyn Fn(&'static ExecutableContext, Standard<'static>, Vec<String>) -> MainFuture + 'static>,
>;

pub struct ExecutableWrapper<T: ExecutableTrait>(T, &'static ExecutableContext);

impl<T: ExecutableTrait> ExecutableWrapper<T> {
    pub fn new(executable: T, context: &'static ExecutableContext) -> Self {
        Self(executable, context)
    }
}

impl<T: ExecutableTrait> MountOperations for ExecutableWrapper<T> {}

impl<T: ExecutableTrait> DirectBaseOperations for ExecutableWrapper<T> {
    fn read(
        &self,
        _cx: &mut core::task::Context<'_>,
        _buffer: &mut [u8],
        _absolute_position: file_system::Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| Err(file_system::Error::UnsupportedOperation))
    }

    fn write(
        &self,
        _cx: &mut core::task::Context<'_>,
        _buffer: &[u8],
        _absolute_position: file_system::Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| Err(file_system::Error::UnsupportedOperation))
    }

    fn control(
        &self,
        _cx: &mut core::task::Context<'_>,
        command: ControlCommandIdentifier,
        _: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> core::task::Poll<file_system::Result<()>> {
        file_system::ready(|| {
            log::debug!("ExecutableWrapper control command: {:?}", command);

            match command {
                GET_MAIN_FUNCTION::IDENTIFIER => {
                    let output = GET_MAIN_FUNCTION::cast_output(output)?;
                    let executable_context = self.1;
                    *output = Some(Box::new(move |context, standard, arguments| {
                        let mut standard = standard;
                        standard.context = context;
                        debug_assert!(core::ptr::eq(context, executable_context));
                        T::main(standard, arguments)
                    }));
                    Ok(())
                }
                _ => Err(file_system::Error::UnsupportedOperation),
            }
        })
    }
}

impl<T: ExecutableTrait> DirectCharacterDevice for ExecutableWrapper<T> {}

#[macro_export]
macro_rules! mount_executables {

    ( $context:expr, $virtual_file_system:expr, $task:expr, &[ $( ($path:expr, $executable:expr) ),* $(,)? ] ) => {

    async || -> $crate::exported_virtual_file_system::Result<()>
    {
        use $crate::exported_file_system::{Permissions};
        use $crate::exported_virtual_file_system::ItemStatic;

        $(
            let __executable = $crate::ExecutableWrapper::new($executable, $context);

            let _ = $virtual_file_system.remove($task, $path).await;
            $virtual_file_system.mount_character_device($task, $path, __executable).await?;
            $virtual_file_system.set_permissions($task, $path, Permissions::EXECUTABLE).await?;
        )*

        Ok(())
    }()
};

}
