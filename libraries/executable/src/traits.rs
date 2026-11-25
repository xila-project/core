use crate::Standard;
use alloc::{boxed::Box, string::String, vec::Vec};
use core::{num::NonZeroUsize, pin::Pin};
use file_system::{
    ControlCommand, ControlCommandIdentifier, DirectBaseOperations, DirectCharacterDevice,
    MountOperations, define_command,
};
use shared::AnyByLayout;

pub type MainFunction = Option<
    Box<dyn Fn(&'static ExecutableContext, Standard<'static>, Vec<String>) -> MainFuture + 'static>,
>;

file_system::plain_data! {
    #[repr(C)]
    /// Plain-data payload of [`GET_MAIN_FUNCTION`]: the two words of a
    /// [`MainFunction`] fat pointer, so the payload size and layout are
    /// unchanged. A function pointer is not plain data, so the payload type
    /// is this mirror; the host converts it back where the value is read.
    #[derive(Clone, Copy, Debug, Default)]
    pub struct MainFunctionAbi {
        pub data: usize,
        pub vtable: usize,
    }
}

const _: () = assert!(
    core::mem::size_of::<MainFunctionAbi>() == core::mem::size_of::<MainFunction>()
        && core::mem::align_of::<MainFunctionAbi>() == core::mem::align_of::<MainFunction>(),
    "MainFunctionAbi must mirror the MainFunction fat pointer"
);

impl From<MainFunction> for MainFunctionAbi {
    fn from(value: MainFunction) -> Self {
        // SAFETY: the const assertion above proves the sizes and alignments
        // match, and a fat pointer is exactly two words.
        unsafe { core::mem::transmute(value) }
    }
}

impl MainFunctionAbi {
    /// Reinterprets a payload written by the host device as the function
    /// pointer it carries.
    ///
    /// # Safety
    ///
    /// The value must come from a [`MainFunction`] produced by the host:
    /// arbitrary bytes are not a valid function pointer.
    pub unsafe fn into_main_function(self) -> MainFunction {
        // SAFETY: the caller guarantees the payload holds a real
        // `MainFunction`; sizes and alignments match (asserted above).
        unsafe { core::mem::transmute(self) }
    }
}

define_command!(
    GET_MAIN_FUNCTION,
    Read,
    device::kind::EXECUTABLE,
    1,
    (),
    MainFunctionAbi
);

pub trait ExecutableTrait: 'static + Send + Sync {
    fn main(standard: Standard<'static>, arguments: Vec<String>) -> MainFuture;
}

pub struct ExecutableContext {
    pub task_manager: &'static task::Manager,
    pub users_manager: &'static users::Manager,
    pub virtual_file_system: &'static virtual_file_system::VirtualFileSystem,
    #[cfg(feature = "graphics")]
    pub graphics_manager: Option<&'static graphics::Manager>,
    #[cfg(feature = "network")]
    pub network_manager: Option<&'static network::Manager>,
    pub formats: &'static [crate::ExecutableFormat],
    pub time_manager: &'static time::Manager<'static>,
}

impl ExecutableContext {
    /// A context without a graphics or network manager, whichever features are
    /// enabled: callers need not know whether those fields exist.
    pub fn new(
        task_manager: &'static task::Manager,
        users_manager: &'static users::Manager,
        virtual_file_system: &'static virtual_file_system::VirtualFileSystem,
        time_manager: &'static time::Manager<'static>,
    ) -> Self {
        Self {
            task_manager,
            users_manager,
            virtual_file_system,
            #[cfg(feature = "graphics")]
            graphics_manager: None,
            #[cfg(feature = "network")]
            network_manager: None,
            formats: &[],
            time_manager,
        }
    }
}

pub type MainFuture =
    Pin<Box<dyn Future<Output = core::result::Result<(), NonZeroUsize>> + 'static>>;

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
        file_system::ready(|| Err(file_system::IoErrorKind::Unsupported))
    }

    fn write(
        &self,
        _cx: &mut core::task::Context<'_>,
        _buffer: &[u8],
        _absolute_position: file_system::Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| Err(file_system::IoErrorKind::Unsupported))
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
                    let main: MainFunction = Some(Box::new(move |context, standard, arguments| {
                        let mut standard = standard;
                        standard.context = context;
                        debug_assert!(core::ptr::eq(context, executable_context));
                        T::main(standard, arguments)
                    }));
                    *output = MainFunctionAbi::from(main);
                    Ok(())
                }
                _ => Err(file_system::IoErrorKind::Unsupported),
            }
        })
    }
}

impl<T: ExecutableTrait> DirectCharacterDevice for ExecutableWrapper<T> {}

#[macro_export]
macro_rules! mount_executables {

    ( $context:expr, $virtual_file_system:expr, $task:expr, &[ $( $(#[$attribute:meta])* ($path:expr, $executable:expr) ),* $(,)? ] ) => {

    async || -> $crate::exported_virtual_file_system::Result<()>
    {
        use $crate::exported_file_system::{Permissions};
        use $crate::exported_virtual_file_system::ItemStatic;

        $(
            $(#[$attribute])*
            {
                let __executable = $crate::ExecutableWrapper::new($executable, $context);

                let _ = $virtual_file_system.remove($task, $path).await;
                $virtual_file_system.mount_character_device($task, $path, __executable).await?;
                $virtual_file_system.set_permissions($task, $path, Permissions::EXECUTABLE).await?;
            }
        )*

        Ok(())
    }()
};

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_function_abi_round_trips_a_function_pointer() {
        let main: MainFunction = Some(Box::new(|_, _, _| Box::pin(async { Ok(()) })));
        let abi = MainFunctionAbi::from(main);
        // SAFETY: the payload was produced from a real `MainFunction`.
        let recovered = unsafe { abi.into_main_function() };
        assert!(recovered.is_some());
        let abi = MainFunctionAbi::from(MainFunction::None);
        // SAFETY: the payload was produced from a real `MainFunction`.
        assert!(unsafe { abi.into_main_function() }.is_none());
    }
}
