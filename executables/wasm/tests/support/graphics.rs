use xila::{
    file_system::{
        self, ControlCommand, ControlCommandIdentifier, DirectBaseOperations,
        DirectCharacterDevice, MountOperations, Size,
    },
    graphics,
    shared::AnyByLayout,
};

struct HeadlessDevice;
impl MountOperations for HeadlessDevice {}
impl DirectCharacterDevice for HeadlessDevice {}
impl DirectBaseOperations for HeadlessDevice {
    fn read(
        &self,
        _cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        _: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        xila::file_system::ready(|| {
            let mut input = graphics::InputData::default();
            buffer.copy_from_slice(input.as_mut());
            Ok(buffer.len())
        })
    }
    fn write(
        &self,
        _cx: &mut core::task::Context<'_>,
        buffer: &[u8],
        _: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        xila::file_system::ready(|| Ok(buffer.len()))
    }
    fn control(
        &self,
        _cx: &mut core::task::Context<'_>,
        command: ControlCommandIdentifier,
        _: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> core::task::Poll<file_system::Result<()>> {
        xila::file_system::ready(|| {
            match command {
                graphics::GET_RESOLUTION::IDENTIFIER => {
                    *graphics::GET_RESOLUTION::cast_output(output)? =
                        graphics::Point::new(320, 240);
                }
                graphics::WAS_RESIZED::IDENTIFIER => {
                    *graphics::WAS_RESIZED::cast_output(output)? = false
                }
                graphics::SET_DRAWING_AREA::IDENTIFIER => (),
                _ => return Err(file_system::Error::UnsupportedOperation),
            }
            Ok(())
        })
    }
}

pub async fn manager() -> &'static graphics::Manager {
    let time = Box::leak(Box::new(
        xila::time::Manager::new(&drivers_std::devices::TimeDevice).unwrap(),
    ));
    Box::leak(Box::new(
        graphics::initialize(
            time,
            &HeadlessDevice,
            &HeadlessDevice,
            graphics::InputKind::Pointer,
            320 * 16 * 4,
            false,
        )
        .await,
    ))
}

#[allow(dead_code)] // Shared by the adapter suite, not the direct-core suite.
pub fn context(
    standard: &xila::executable::Standard<'static>,
    manager: Option<&'static graphics::Manager>,
) -> &'static xila::executable::ExecutableContext {
    let source = standard.context;
    Box::leak(Box::new(xila::executable::ExecutableContext {
        task_manager: source.task_manager,
        users_manager: source.users_manager,
        virtual_file_system: source.virtual_file_system,
        graphics_manager: manager,
        network_manager: source.network_manager,
        time_manager: source.time_manager,
    }))
}
