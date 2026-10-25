//! OS lifecycle control. The mount must be restricted to root; ioctl lacks
//! task/user identity, so VFS authorization happens at open time.
use core::task::{Context, Poll};
use file_system::{
    CharacterDevice, ControlCommand, ControlCommandIdentifier, DirectBaseOperations, Error,
    MountOperations, Result, Size,
};
use lifecycle::{Controller, GET_SHUTDOWN_STATE, REQUEST_SHUTDOWN};
use shared::AnyByLayout;

pub struct PowerControlDevice(&'static Controller);

impl PowerControlDevice {
    pub const fn new(controller: &'static Controller) -> Self {
        Self(controller)
    }
}

impl DirectBaseOperations for PowerControlDevice {
    fn read(&self, _cx: &mut Context<'_>, _buf: &mut [u8], _pos: Size) -> Poll<Result<usize>> {
        Poll::Ready(Err(Error::UnsupportedOperation))
    }

    fn write(&self, _cx: &mut Context<'_>, _buf: &[u8], _pos: Size) -> Poll<Result<usize>> {
        Poll::Ready(Err(Error::UnsupportedOperation))
    }

    fn control(
        &self,
        _cx: &mut Context<'_>,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Poll<Result<()>> {
        file_system::ready(|| match command {
            GET_SHUTDOWN_STATE::IDENTIFIER => {
                *GET_SHUTDOWN_STATE::cast_output(output)? = self.0.state() as u8;
                Ok(())
            }
            REQUEST_SHUTDOWN::IDENTIFIER => {
                REQUEST_SHUTDOWN::cast_input(input)?;
                self.0.request_shutdown().map_err(|_| Error::RessourceBusy)
            }
            _ => Err(Error::UnsupportedOperation),
        })
    }
}

impl MountOperations for PowerControlDevice {}
impl CharacterDevice for PowerControlDevice {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_requests_shutdown_once_and_reads_state() {
        static CONTROLLER: Controller = Controller::new();
        let device = PowerControlDevice::new(&CONTROLLER);
        let mut cx = Context::from_waker(core::task::Waker::noop());
        let mut empty = ();
        let requested = device.control(
            &mut cx,
            REQUEST_SHUTDOWN::IDENTIFIER,
            AnyByLayout::from(&()),
            AnyByLayout::from_mutable(&mut empty),
        );
        assert!(matches!(requested, Poll::Ready(Ok(()))));
        assert_eq!(CONTROLLER.state(), lifecycle::State::Requested);

        let mut phase = 255_u8;
        let status = device.control(
            &mut cx,
            GET_SHUTDOWN_STATE::IDENTIFIER,
            AnyByLayout::from(&()),
            AnyByLayout::from_mutable(&mut phase),
        );
        assert!(matches!(status, Poll::Ready(Ok(()))));
        assert_eq!(phase, lifecycle::STATE_REQUESTED);

        let duplicate = device.control(
            &mut cx,
            REQUEST_SHUTDOWN::IDENTIFIER,
            AnyByLayout::from(&()),
            AnyByLayout::from_mutable(&mut empty),
        );
        assert!(matches!(
            duplicate,
            Poll::Ready(Err(Error::RessourceBusy))
        ));
    }

    #[test]
    fn arbitrary_bytes_cannot_request_shutdown() {
        static CONTROLLER: Controller = Controller::new();
        let device = PowerControlDevice::new(&CONTROLLER);
        let mut cx = Context::from_waker(core::task::Waker::noop());
        assert!(matches!(
            device.write(&mut cx, b"shutdown", 0),
            Poll::Ready(Err(Error::UnsupportedOperation))
        ));
        assert_eq!(CONTROLLER.state(), lifecycle::State::Running);
    }
}
