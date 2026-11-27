use alloc::boxed::Box;
use core::cell::RefCell;
use core::task::{Context as TaskContext, Poll, Waker};

use file_system::{
    BaseOperations, CharacterDevice, Context, ControlCommand, ControlCommandIdentifier,
    IoErrorKind, MountOperations, Operation, ReadyEvents, Result, Size, notify_readiness, ready,
};
use heapless::Deque;
use shared::AnyByLayout;
use synchronization::{
    blocking_mutex::{Mutex, raw::CriticalSectionRawMutex},
    signal::Signal,
};

use crate::*;

const EVENT_CAPACITY: usize = 16;
type Lock = CriticalSectionRawMutex;

#[derive(Clone, Copy)]
pub(crate) enum Request {
    StartScan(ScanConfiguration),
    StopScan,
    StartAdvertising(AdvertisingData),
    StopAdvertising,
    Connect(ConnectionRequest),
    Disconnect,
    Services,
    Characteristics(u16),
    Descriptors(u16),
    Read(u16),
    Write(AttributeValue),
    Notifications(NotificationRequest),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Session {
    Idle,
    Open,
    Closing,
}

struct Opening;

struct Inner {
    online: bool,
    session: Session,
    state: State,
    next_handle: u32,
    work: Option<(u8, Request)>,
    busy: bool,
    events: Deque<Event, EVENT_CAPACITY>,
    dropped: u32,
    reader: Option<Waker>,
}

/// The upper half of the Bluetooth device. Start its runner on a local task
/// before opening the device. Each successful opening owns the controller
/// until its work has been cancelled and its connection released.
pub struct BluetoothDevice {
    inner: Mutex<Lock, RefCell<Inner>>,
    work: Signal<Lock, ()>,
    closed: Signal<Lock, ()>,
}

impl Default for BluetoothDevice {
    fn default() -> Self {
        Self::new()
    }
}

impl BluetoothDevice {
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(RefCell::new(Inner {
                online: false,
                session: Session::Idle,
                state: State {
                    connection: ConnectionHandle::NONE,
                    scanning: 0,
                    advertising: 0,
                    reserved: [0; 2],
                },
                next_handle: 0,
                work: None,
                busy: false,
                events: Deque::new(),
                dropped: 0,
                reader: None,
            })),
            work: Signal::new(),
            closed: Signal::new(),
        }
    }

    fn with<T>(&self, operation: impl FnOnce(&mut Inner) -> T) -> T {
        self.inner.lock(|inner| operation(&mut inner.borrow_mut()))
    }

    pub(crate) fn begin_run(&self) -> Result<Running<'_>> {
        self.with(|inner| {
            if inner.online || inner.session != Session::Idle {
                return Err(IoErrorKind::ResourceBusy);
            }
            self.closed.reset();
            self.work.reset();
            inner.online = true;
            Ok(Running(self))
        })
    }

    pub(crate) async fn wait_open(&self) {
        while self.with(|inner| inner.session == Session::Idle) {
            self.work.wait().await;
        }
    }

    pub(crate) async fn wait_closed(&self) {
        self.closed.wait().await;
    }

    pub(crate) fn finish_close(&self) {
        self.with(|inner| {
            inner.reset_session();
            inner.session = Session::Idle;
        });
    }

    pub(crate) async fn next_request(&self) -> (u8, Request) {
        loop {
            if let Some(request) = self.with(|inner| inner.work.take()) {
                return request;
            }
            self.work.wait().await;
        }
    }

    pub(crate) fn set_scanning(&self, scanning: bool) {
        self.with(|inner| inner.state.scanning = u8::from(scanning));
    }

    pub(crate) fn set_advertising(&self, advertising: bool) {
        self.with(|inner| inner.state.advertising = u8::from(advertising));
    }

    pub(crate) fn connected(&self) -> ConnectionHandle {
        self.with(|inner| {
            inner.next_handle = inner.next_handle.wrapping_add(1).max(1);
            let handle = ConnectionHandle(inner.next_handle);
            inner.state.connection = handle;
            handle
        })
    }

    pub(crate) fn disconnected(&self) {
        self.with(|inner| inner.state.connection = ConnectionHandle::NONE);
    }

    pub(crate) fn complete(&self, command: u8, handle: ConnectionHandle, result: Result<()>) {
        let mut event = Event::new(Event::COMPLETION, command, handle);
        event.status = result.err().map_or(0, IoErrorKind::id);
        self.emit(event);
    }

    pub(crate) fn emit(&self, event: Event) {
        let (changed, reader) = self.with(|inner| {
            if inner.session != Session::Open
                || (event.kind == Event::SCAN_RESULT && inner.state.scanning == 0)
            {
                return (false, None);
            }

            // Keep one slot for the accepted command's completion. The next
            // command cannot enter until the opening reads that completion.
            let limit = if event.kind != Event::COMPLETION {
                EVENT_CAPACITY - 1
            } else {
                EVENT_CAPACITY
            };
            if inner.events.len() >= limit {
                inner.dropped = inner.dropped.saturating_add(1);
            } else {
                let _ = inner.events.push_back(event);
            }
            (true, inner.reader.take())
        });
        if let Some(reader) = reader {
            reader.wake();
        }
        if changed {
            notify_readiness();
        }
    }

    fn submit(
        &self,
        command: u8,
        request: Request,
        handle: Option<ConnectionHandle>,
    ) -> Result<()> {
        self.with(|inner| {
            if !inner.online {
                return Err(IoErrorKind::DeviceUnavailable);
            }
            if let Some(handle) = handle
                && (handle == ConnectionHandle::NONE || handle != inner.state.connection)
            {
                return Err(IoErrorKind::NotConnected);
            }
            if inner.busy {
                return Err(IoErrorKind::ResourceBusy);
            }
            inner.busy = true;
            inner.work = Some((command, request));
            Ok(())
        })?;
        self.work.signal(());
        Ok(())
    }

    fn dispatch(
        &self,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Result<()> {
        let (request, handle) = match command {
            GET_CAPABILITIES::IDENTIFIER => {
                *GET_CAPABILITIES::cast_output(output)? = Capabilities {
                    features: Capabilities::CENTRAL
                        | Capabilities::OBSERVER
                        | Capabilities::BROADCASTER
                        | Capabilities::GATT_CLIENT,
                    connection_limit: 1,
                    value_capacity: VALUE_CAPACITY as u8,
                };
                return Ok(());
            }
            GET_STATE::IDENTIFIER => {
                *GET_STATE::cast_output(output)? = self.with(|inner| inner.state);
                return Ok(());
            }
            START_SCAN::IDENTIFIER => {
                let configuration = *START_SCAN::cast_input(input)?;
                if configuration.active > 1 || configuration.filter_duplicates > 1 {
                    return Err(IoErrorKind::InvalidArgument);
                }
                (Request::StartScan(configuration), None)
            }
            STOP_SCAN::IDENTIFIER => (Request::StopScan, None),
            START_ADVERTISING::IDENTIFIER => {
                let data = *START_ADVERTISING::cast_input(input)?;
                if data.as_slice().is_none_or(<[u8]>::is_empty) {
                    return Err(IoErrorKind::InvalidArgument);
                }
                (Request::StartAdvertising(data), None)
            }
            STOP_ADVERTISING::IDENTIFIER => (Request::StopAdvertising, None),
            CONNECT::IDENTIFIER => {
                let request = *CONNECT::cast_input(input)?;
                if request.peer.kind > Address::RANDOM
                    || request.timeout_milliseconds == 0
                    || request.reserved != 0
                {
                    return Err(IoErrorKind::InvalidArgument);
                }
                (Request::Connect(request), None)
            }
            DISCONNECT::IDENTIFIER => (Request::Disconnect, Some(*DISCONNECT::cast_input(input)?)),
            DISCOVER_SERVICES::IDENTIFIER => (
                Request::Services,
                Some(*DISCOVER_SERVICES::cast_input(input)?),
            ),
            DISCOVER_CHARACTERISTICS::IDENTIFIER
            | DISCOVER_DESCRIPTORS::IDENTIFIER
            | READ_ATTRIBUTE::IDENTIFIER => {
                let request = match command {
                    DISCOVER_CHARACTERISTICS::IDENTIFIER => {
                        DISCOVER_CHARACTERISTICS::cast_input(input)?
                    }
                    DISCOVER_DESCRIPTORS::IDENTIFIER => DISCOVER_DESCRIPTORS::cast_input(input)?,
                    _ => READ_ATTRIBUTE::cast_input(input)?,
                };
                if request.reserved != 0 || request.handle == 0 {
                    return Err(IoErrorKind::InvalidArgument);
                }
                let operation = match command {
                    DISCOVER_CHARACTERISTICS::IDENTIFIER => {
                        Request::Characteristics(request.handle)
                    }
                    DISCOVER_DESCRIPTORS::IDENTIFIER => Request::Descriptors(request.handle),
                    _ => Request::Read(request.handle),
                };
                (operation, Some(request.connection))
            }
            WRITE_ATTRIBUTE::IDENTIFIER => {
                let request = WRITE_ATTRIBUTE::cast_input(input)?;
                if request.value.handle == 0 || request.value.as_slice().is_none() {
                    return Err(IoErrorKind::InvalidArgument);
                }
                (Request::Write(request.value), Some(request.connection))
            }
            SET_NOTIFICATIONS::IDENTIFIER => {
                let request = *SET_NOTIFICATIONS::cast_input(input)?;
                if request.enabled > 1
                    || request.characteristic.handle == 0
                    || request.characteristic.reserved != 0
                    || request.reserved != [0; 3]
                {
                    return Err(IoErrorKind::InvalidArgument);
                }
                (
                    Request::Notifications(request),
                    Some(request.characteristic.connection),
                )
            }
            _ => return Err(IoErrorKind::Unsupported),
        };
        self.submit(command.get_number(), request, handle)
    }
}

impl Inner {
    fn reset_session(&mut self) {
        self.state = State::default();
        self.work = None;
        self.events.clear();
        self.busy = false;
        self.dropped = 0;
        self.reader = None;
    }

    fn pop_event(&mut self) -> Option<Event> {
        if let Some(event) = self.events.pop_front() {
            if event.kind == Event::COMPLETION {
                self.busy = false;
            }
            return Some(event);
        }
        (self.dropped > 0).then(|| {
            Event::new(Event::OVERFLOW, 0, ConnectionHandle::NONE)
                .with_payload(core::mem::take(&mut self.dropped))
        })
    }
}

pub(crate) struct Running<'a>(&'a BluetoothDevice);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        let reader = self.0.with(|inner| {
            inner.online = false;
            let reader = inner.reader.take();
            inner.reset_session();
            if inner.session == Session::Closing {
                inner.session = Session::Idle;
            }
            reader
        });
        if let Some(reader) = reader {
            reader.wake();
        }
        notify_readiness();
    }
}

impl BaseOperations for BluetoothDevice {
    fn open(&self, context: &mut Context) -> Result<()> {
        self.with(|inner| {
            if !inner.online {
                return Err(IoErrorKind::DeviceUnavailable);
            }
            if inner.session != Session::Idle {
                return Err(IoErrorKind::ResourceBusy);
            }
            inner.session = Session::Open;
            Ok(())
        })?;
        context.set_private_data(Box::new(Opening));
        self.work.signal(());
        Ok(())
    }

    fn close(&self, context: &mut Context) -> Result<()> {
        if context.take_private_data_of_type::<Opening>().is_some() {
            self.with(|inner| {
                if inner.online {
                    inner.session = Session::Closing;
                    // Signal under the same lock used to publish the session state.
                    self.closed.signal(());
                } else {
                    inner.session = Session::Idle;
                }
            });
        }
        Ok(())
    }

    fn clone_context(&self, _: &Context) -> Result<Context> {
        Err(IoErrorKind::Unsupported)
    }

    fn read(
        &self,
        _: &mut Context,
        context: &mut TaskContext<'_>,
        buffer: &mut [u8],
        _: Size,
    ) -> Poll<Result<usize>> {
        if buffer.len() < size_of::<Event>() {
            return Poll::Ready(Err(IoErrorKind::MessageTooLarge));
        }
        self.with(|inner| {
            if !inner.online {
                return Poll::Ready(Err(IoErrorKind::DeviceUnavailable));
            }
            if let Some(mut event) = inner.pop_event() {
                let bytes = event.as_mutable_bytes();
                buffer[..bytes.len()].copy_from_slice(bytes);
                return Poll::Ready(Ok(bytes.len()));
            }
            inner.reader = Some(context.waker().clone());
            Poll::Pending
        })
    }

    fn cancel_operation(&self, _: &mut Context, operation: Operation) {
        if operation == Operation::Read {
            self.with(|inner| inner.reader = None);
        }
    }

    fn write(
        &self,
        _: &mut Context,
        _: &mut TaskContext<'_>,
        _: &[u8],
        _: Size,
    ) -> Poll<Result<usize>> {
        Poll::Ready(Err(IoErrorKind::Unsupported))
    }

    fn control(
        &self,
        _: &mut Context,
        _: &mut TaskContext<'_>,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Poll<Result<()>> {
        ready(|| self.dispatch(command, input, output))
    }
}

impl MountOperations for BluetoothDevice {}

impl CharacterDevice for BluetoothDevice {
    fn readiness(&self, interests: ReadyEvents) -> Result<Option<ReadyEvents>> {
        let events = self.with(|inner| {
            if !inner.online {
                return ReadyEvents::ERROR;
            }
            if inner.events.is_empty() && inner.dropped == 0 {
                ReadyEvents::empty()
            } else {
                ReadyEvents::READ
            }
        });
        Ok(Some(events & interests))
    }
}
