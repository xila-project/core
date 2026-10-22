use crate::{GET_KIND, InterfaceKind};
use file_system::{
    ControlCommand, ControlCommandIdentifier, DirectBaseOperations, DirectCharacterDevice, Error,
    MountOperations,
};
use shared::AnyByLayout;
pub use smoltcp::phy::Loopback;
use smoltcp::phy::Medium;

pub struct LoopbackControllerDevice;

impl DirectBaseOperations for LoopbackControllerDevice {
    fn read(
        &self,
        _cx: &mut core::task::Context<'_>,
        _: &mut [u8],
        _: file_system::Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| Err(Error::UnsupportedOperation))
    }

    fn write(
        &self,
        _cx: &mut core::task::Context<'_>,
        _: &[u8],
        _: file_system::Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| Err(Error::UnsupportedOperation))
    }

    fn control(
        &self,
        _cx: &mut core::task::Context<'_>,
        command: ControlCommandIdentifier,
        _: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> core::task::Poll<file_system::Result<()>> {
        file_system::ready(|| match command {
            GET_KIND::IDENTIFIER => {
                let kind = GET_KIND::cast_output(output)?;
                *kind = InterfaceKind::Ethernet;
                Ok(())
            }
            _ => Err(Error::UnsupportedOperation),
        })
    }
}

impl MountOperations for LoopbackControllerDevice {}

impl DirectCharacterDevice for LoopbackControllerDevice {}

pub fn create_loopback_device() -> (Loopback, LoopbackControllerDevice) {
    (Loopback::new(Medium::Ethernet), LoopbackControllerDevice)
}
