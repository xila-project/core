use super::*;
use core::{
    future::Future,
    pin::pin,
    task::{Context as TaskContext, Poll, Waker},
};
use file_system::{
    BaseOperations, CharacterDevice, Context as DeviceContext, ControlCommand,
    ControlCommandIdentifier, MountOperations, Operation, Result, Size,
};
use shared::AnyByLayout;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

file_system::define_command!(WAIT_COMMAND, Read, b'P', 1, usize, usize);
#[derive(Default)]
struct State {
    mode: AtomicUsize,
    waker: Mutex<Option<Waker>>,
    cleanup: [AtomicUsize; 4],
}
impl State {
    fn signal(&self, mode: usize) {
        self.mode.store(mode, Ordering::SeqCst);
        let waker = self.waker.lock().unwrap().take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }
    fn attempt(&self) -> Result<()> {
        match self.mode.load(Ordering::SeqCst) {
            0 => Err(file_system::Error::RessourceBusy),
            1 => Ok(()),
            _ => Err(file_system::Error::InputOutput),
        }
    }
    fn poll(&self, cx: &mut TaskContext<'_>) -> Poll<Result<()>> {
        if self.mode.load(Ordering::SeqCst) == 0 {
            *self.waker.lock().unwrap() = Some(cx.waker().clone());
            Poll::Pending
        } else {
            Poll::Ready(self.attempt())
        }
    }
}
struct Opening {
    polls: usize,
}
struct Device {
    state: Arc<State>,
    polling: bool,
}
impl BaseOperations for Device {
    fn supports_polling(&self) -> bool {
        self.polling
    }
    fn open(&self, context: &mut DeviceContext) -> Result<()> {
        context.set_private_data(Box::new(Opening { polls: 0 }));
        Ok(())
    }
    fn read(&self, _: &mut DeviceContext, buffer: &mut [u8], _: Size) -> Result<usize> {
        self.state.attempt()?;
        if let Some(byte) = buffer.first_mut() {
            *byte = b'x';
            Ok(1)
        } else {
            Ok(0)
        }
    }
    fn write(&self, _: &mut DeviceContext, buffer: &[u8], _: Size) -> Result<usize> {
        self.state.attempt()?;
        Ok(buffer.len())
    }
    fn flush(&self, _: &mut DeviceContext) -> Result<()> {
        self.state.attempt()
    }
    fn control(
        &self,
        _: &mut DeviceContext,
        _: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Result<()> {
        self.state.attempt()?;
        let (input, output) = WAIT_COMMAND::cast(input, output)?;
        *output = *input;
        Ok(())
    }
    fn poll_flush(
        &self,
        context: &mut DeviceContext,
        cx: &mut TaskContext<'_>,
    ) -> Poll<Result<()>> {
        context
            .get_private_data_mutable_of_type::<Opening>()
            .unwrap()
            .polls += 1;
        self.state.poll(cx)
    }
    fn poll_control(
        &self,
        context: &mut DeviceContext,
        cx: &mut TaskContext<'_>,
        _: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Poll<Result<()>> {
        let opening = context
            .get_private_data_mutable_of_type::<Opening>()
            .unwrap();
        opening.polls += 1;
        let (input, output) = WAIT_COMMAND::cast(input, output)?;
        // The test proves caller-owned output is preserved across Pending.
        *output += input;
        self.state.poll(cx)
    }
    fn cancel_operation(&self, context: &mut DeviceContext, operation: Operation) {
        assert!(context.get_private_data_of_type::<Opening>().is_some());
        self.state.cleanup[operation as usize].fetch_add(1, Ordering::SeqCst);
        let retired = self.state.waker.lock().unwrap().take();
        drop(retired);
    }
    fn clone_context(&self, _: &DeviceContext) -> Result<DeviceContext> {
        Ok(DeviceContext::new(Some(Opening { polls: 0 })))
    }
}
impl MountOperations for Device {}
impl CharacterDevice for Device {}

async fn device(polling: bool, flags: StateFlags) -> (Arc<State>, File<'static>) {
    let (task, vfs) = initialize().await;
    let state = Arc::new(State::default());
    vfs.mount_character_device(
        task,
        "/poll-device",
        Device {
            state: state.clone(),
            polling,
        },
    )
    .await
    .unwrap();
    let file = File::open(
        vfs,
        task,
        "/poll-device",
        Flags::new(AccessFlags::READ_WRITE, None, Some(flags)),
    )
    .await
    .unwrap();
    (state, file)
}

#[task::test]
async fn flush_and_control_preserve_private_state_and_cleanup_on_all_exits() {
    let (state, mut file) = device(true, StateFlags::None).await;
    let mut cx = TaskContext::from_waker(Waker::noop());
    {
        let mut flush = pin!(file.flush());
        assert!(flush.as_mut().poll(&mut cx).is_pending());
    }
    assert!(state.waker.lock().unwrap().is_none());
    assert_eq!(
        state.cleanup[Operation::Flush as usize].load(Ordering::SeqCst),
        1
    );
    {
        let mut control = pin!(file.control(WAIT_COMMAND, &5));
        assert!(control.as_mut().poll(&mut cx).is_pending());
        state.signal(1);
        assert_eq!(control.as_mut().poll(&mut cx), Poll::Ready(Ok(10)));
    }
    assert_eq!(
        state.cleanup[Operation::Control as usize].load(Ordering::SeqCst),
        1
    );
    state.signal(0);
    {
        let mut control = pin!(file.control(WAIT_COMMAND, &5));
        assert!(control.as_mut().poll(&mut cx).is_pending());
    }
    assert!(state.waker.lock().unwrap().is_none());
    assert_eq!(
        state.cleanup[Operation::Control as usize].load(Ordering::SeqCst),
        2
    );
    state.signal(2); // Model a terminal device error/removal notification.
    assert_eq!(
        file.flush().await,
        Err(virtual_file_system::Error::FileSystem(
            file_system::Error::InputOutput
        ))
    );
    assert_eq!(
        state.cleanup[Operation::Flush as usize].load(Ordering::SeqCst),
        2
    );
}

#[task::test]
async fn nonblocking_flush_and_control_never_register_or_retry() {
    let (state, mut file) = device(true, StateFlags::NonBlocking).await;
    let busy = Err(virtual_file_system::Error::FileSystem(
        file_system::Error::RessourceBusy,
    ));
    assert_eq!(file.flush().await, busy);
    assert_eq!(file.control(WAIT_COMMAND, &5).await, busy.map(|_| 0));
    assert!(state.waker.lock().unwrap().is_none());
    assert!(
        state
            .cleanup
            .iter()
            .all(|count| count.load(Ordering::SeqCst) == 0)
    );
}

#[task::test]
async fn legacy_device_keeps_cooperative_retry_and_explicit_busy_is_not_pending() {
    let (state, mut legacy) = device(false, StateFlags::None).await;
    let mut cx = TaskContext::from_waker(Waker::noop());
    let mut byte = [0];
    {
        let mut read = pin!(legacy.read(&mut byte));
        assert!(read.as_mut().poll(&mut cx).is_pending());
        state.signal(1);
        assert_eq!(read.as_mut().poll(&mut cx), Poll::Ready(Ok(1)));
    }
    assert_eq!(&byte, b"x");
    assert!(
        state
            .cleanup
            .iter()
            .all(|count| count.load(Ordering::SeqCst) == 0)
    );
    let (state, mut migrated) = device(true, StateFlags::None).await;
    let mut read = pin!(migrated.read(&mut byte)); // Default hook reports explicit busy.
    assert_eq!(
        read.as_mut().poll(&mut cx),
        Poll::Ready(Err(virtual_file_system::Error::FileSystem(
            file_system::Error::RessourceBusy
        )))
    );
    assert_eq!(
        state.cleanup[Operation::Read as usize].load(Ordering::SeqCst),
        1
    );
}
