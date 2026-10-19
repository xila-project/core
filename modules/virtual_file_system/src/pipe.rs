use alloc::collections::VecDeque;
use core::{
    cell::RefCell,
    task::{Context as TaskContext, Poll, Waker},
};
use file_system::{
    AccessFlags, BaseOperations, Context, Error, MountOperations, Operation, Result, Size,
};
use synchronization::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};

/// Maximum simultaneous openings (including duplicates) of one pipe.
/// Slots are reserved during open, never allocated on the I/O path.
pub const MAX_PIPE_OPENINGS: usize = 16;
type WakeBatch = [Option<Waker>; MAX_PIPE_OPENINGS];

#[derive(Debug)]
struct Slot {
    access: Option<AccessFlags>,
    waiter: Option<(Operation, Waker)>,
}
#[derive(Debug)]
struct State {
    buffer: VecDeque<u8>,
    readers: usize,
    writers: usize,
    slots: [Slot; MAX_PIPE_OPENINGS],
}
impl State {
    fn notify(&mut self, operation: Operation, wake: &mut WakeBatch) {
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if slot
                .waiter
                .as_ref()
                .is_some_and(|(kind, _)| *kind == operation)
            {
                wake[index] = slot.waiter.take().map(|(_, waker)| waker);
            }
        }
    }
}
struct Opening {
    access: AccessFlags,
    slot: usize,
}

/// Bounded FIFO. Each opening owns one independent waiter registration.
/// The final writer closing makes an empty read return EOF.
#[derive(Debug)]
pub struct Pipe {
    state: Mutex<CriticalSectionRawMutex, RefCell<State>>,
    capacity: usize,
}
impl Pipe {
    pub fn new(capacity: usize) -> Self {
        Self {
            state: Mutex::new(RefCell::new(State {
                buffer: VecDeque::with_capacity(capacity),
                readers: 0,
                writers: 0,
                slots: core::array::from_fn(|_| Slot {
                    access: None,
                    waiter: None,
                }),
            })),
            capacity,
        }
    }

    pub(crate) fn open_context(&self, access: AccessFlags) -> Result<Context> {
        // Allocate the device-owned context before entering the critical section.
        let mut context = Context::new(Some(Opening { access, slot: 0 }));
        let slot = self.state.lock(|state| {
            let mut state = state.borrow_mut();
            let slot = state
                .slots
                .iter()
                .position(|slot| slot.access.is_none())
                .ok_or(Error::TooManyOpenFiles)?;
            state.slots[slot].access = Some(access);
            state.readers += usize::from(access.contains(AccessFlags::Read));
            state.writers += usize::from(access.contains(AccessFlags::Write));
            Ok::<_, Error>(slot)
        })?;
        context
            .get_private_data_mutable_of_type::<Opening>()
            .unwrap()
            .slot = slot;
        Ok(context)
    }

    fn opening(context: &Context, required: AccessFlags) -> Result<&Opening> {
        let opening = context
            .get_private_data_of_type::<Opening>()
            .ok_or(Error::InvalidContext)?;
        if !opening.access.contains(required) {
            return Err(Error::InvalidMode);
        }
        Ok(opening)
    }

    fn transact(
        &self,
        kind: Operation,
        registration: Option<(usize, &Waker)>,
        operation: impl FnOnce(&mut State) -> Result<usize>,
    ) -> Poll<Result<usize>> {
        // Waker vtables are arbitrary code. Clone/drop/wake outside the lock.
        let mut registration = registration.map(|(slot, waker)| (slot, waker.clone()));
        let mut retired = None;
        let mut wake: WakeBatch = core::array::from_fn(|_| None);
        let result = self.state.lock(|state| {
            let mut state = state.borrow_mut();
            let result = operation(&mut state);
            if result == Err(Error::RessourceBusy) && registration.is_some() {
                let (slot, waker) = registration.take().unwrap();
                retired = state.slots[slot].waiter.replace((kind, waker));
                return Poll::Pending;
            }
            if let Some((slot, _)) = registration.as_ref() {
                retired = state.slots[*slot].waiter.take();
            }
            if result.as_ref().is_ok_and(|count| *count > 0) {
                state.notify(
                    if kind == Operation::Read {
                        Operation::Write
                    } else {
                        Operation::Read
                    },
                    &mut wake,
                );
            }
            Poll::Ready(result)
        });
        drop(retired);
        drop(registration);
        for waker in wake.into_iter().flatten() {
            waker.wake();
        }
        result
    }

    fn read_inner(
        &self,
        data: &mut [u8],
        delimiter: Option<&[u8]>,
        registration: Option<(usize, &Waker)>,
    ) -> Poll<Result<usize>> {
        self.transact(Operation::Read, registration, |state| {
            if data.is_empty() {
                return Ok(0);
            }
            if state.buffer.is_empty() && state.writers > 0 {
                return Err(Error::RessourceBusy);
            }
            let mut length = 0;
            while length < data.len() {
                let Some(byte) = state.buffer.pop_front() else {
                    break;
                };
                data[length] = byte;
                length += 1;
                if delimiter.is_some_and(|delimiter| data[..length].ends_with(delimiter)) {
                    break;
                }
            }
            Ok(length)
        })
    }

    fn write_inner(
        &self,
        data: &[u8],
        registration: Option<(usize, &Waker)>,
    ) -> Poll<Result<usize>> {
        self.transact(Operation::Write, registration, |state| {
            if data.is_empty() {
                return Ok(0);
            }
            if state.readers == 0 {
                return Err(Error::InputOutput);
            }
            let length = data.len().min(self.capacity - state.buffer.len());
            if length == 0 {
                return Err(Error::RessourceBusy);
            }
            state.buffer.extend(&data[..length]);
            Ok(length)
        })
    }

    // These helpers preserve the original single-attempt API.
    pub async fn write(&self, data: &[u8]) -> Result<usize> {
        match self.write_inner(data, None) {
            Poll::Ready(result) => result,
            Poll::Pending => unreachable!(),
        }
    }
    pub async fn read(&self, data: &mut [u8]) -> Result<usize> {
        match self.read_inner(data, None, None) {
            Poll::Ready(result) => result,
            Poll::Pending => unreachable!(),
        }
    }
}
impl BaseOperations for Pipe {
    fn supports_polling(&self) -> bool {
        true
    }
    fn close(&self, context: &mut Context) -> Result<()> {
        let opening = context
            .take_private_data_of_type::<Opening>()
            .ok_or(Error::InvalidContext)?;
        let mut retired = None;
        let mut wake: WakeBatch = core::array::from_fn(|_| None);
        self.state.lock(|state| {
            let mut state = state.borrow_mut();
            let slot = &mut state.slots[opening.slot];
            slot.access = None;
            retired = slot.waiter.take();
            state.readers -= usize::from(opening.access.contains(AccessFlags::Read));
            state.writers -= usize::from(opening.access.contains(AccessFlags::Write));
            if state.readers == 0 {
                state.notify(Operation::Write, &mut wake);
            }
            if state.writers == 0 {
                state.notify(Operation::Read, &mut wake);
            }
        });
        drop(retired);
        for waker in wake.into_iter().flatten() {
            waker.wake();
        }
        Ok(())
    }
    fn clone_context(&self, context: &Context) -> Result<Context> {
        self.open_context(
            context
                .get_private_data_of_type::<Opening>()
                .ok_or(Error::InvalidContext)?
                .access,
        )
    }
    fn cancel_operation(&self, context: &mut Context, operation: Operation) {
        let Some(opening) = context.get_private_data_of_type::<Opening>() else {
            return;
        };
        let retired = self.state.lock(|state| {
            let mut state = state.borrow_mut();
            let slot = &mut state.slots[opening.slot];
            if slot
                .waiter
                .as_ref()
                .is_some_and(|(kind, _)| *kind == operation)
            {
                slot.waiter.take()
            } else {
                None
            }
        });
        drop(retired);
    }
    fn read(&self, context: &mut Context, data: &mut [u8], _: Size) -> Result<usize> {
        Self::opening(context, AccessFlags::Read)?;
        match self.read_inner(data, None, None) {
            Poll::Ready(result) => result,
            Poll::Pending => unreachable!(),
        }
    }
    fn poll_read(
        &self,
        context: &mut Context,
        cx: &mut TaskContext<'_>,
        data: &mut [u8],
        _: Size,
    ) -> Poll<Result<usize>> {
        let opening = Self::opening(context, AccessFlags::Read)?;
        self.read_inner(data, None, Some((opening.slot, cx.waker())))
    }
    fn read_until(
        &self,
        context: &mut Context,
        data: &mut [u8],
        _: Size,
        delimiter: &[u8],
    ) -> Result<usize> {
        Self::opening(context, AccessFlags::Read)?;
        if delimiter.is_empty() {
            return Err(Error::InvalidParameter);
        }
        match self.read_inner(data, Some(delimiter), None) {
            Poll::Ready(result) => result,
            Poll::Pending => unreachable!(),
        }
    }
    fn write(&self, context: &mut Context, data: &[u8], _: Size) -> Result<usize> {
        Self::opening(context, AccessFlags::Write)?;
        match self.write_inner(data, None) {
            Poll::Ready(result) => result,
            Poll::Pending => unreachable!(),
        }
    }
    fn poll_write(
        &self,
        context: &mut Context,
        cx: &mut TaskContext<'_>,
        data: &[u8],
        _: Size,
    ) -> Poll<Result<usize>> {
        let opening = Self::opening(context, AccessFlags::Write)?;
        self.write_inner(data, Some((opening.slot, cx.waker())))
    }
}
impl MountOperations for Pipe {}

#[cfg(test)]
mod regression_tests {
    extern crate std;
    use super::*;
    #[task::test]
    async fn short_read_does_not_pop_beyond_available_bytes() {
        let pipe = Pipe::new(4);
        let _reader = pipe.open_context(AccessFlags::Read).unwrap();
        assert_eq!(pipe.write(b"hi").await.unwrap(), 2);
        let mut output = [0; 8];
        assert_eq!(pipe.read(&mut output).await.unwrap(), 2);
        assert_eq!(&output[..2], b"hi");
    }
    #[task::test]
    async fn writes_are_bounded_by_the_configured_capacity() {
        let pipe = Pipe::new(4);
        let _reader = pipe.open_context(AccessFlags::Read).unwrap();
        assert_eq!(pipe.write(b"abcdef").await.unwrap(), 4);
        assert!(matches!(pipe.write(b"x").await, Err(Error::RessourceBusy)));
    }
    #[task::test]
    async fn eof_waits_for_every_writer_and_preserves_buffered_data() {
        let pipe = Pipe::new(4);
        let mut reader = pipe.open_context(AccessFlags::Read).unwrap();
        let mut writer = pipe.open_context(AccessFlags::Write).unwrap();
        let mut duplicate = pipe.clone_context(&writer).unwrap();
        assert!(matches!(
            pipe.read(&mut [0; 1]).await,
            Err(Error::RessourceBusy)
        ));
        pipe.write(b"hi").await.unwrap();
        pipe.close(&mut writer).unwrap();
        let mut output = [0; 8];
        assert_eq!(pipe.read(&mut output).await.unwrap(), 2);
        assert!(matches!(
            pipe.read(&mut output).await,
            Err(Error::RessourceBusy)
        ));
        pipe.close(&mut duplicate).unwrap();
        assert_eq!(pipe.read(&mut output).await.unwrap(), 0);
        pipe.close(&mut reader).unwrap();
        assert!(matches!(pipe.write(b"x").await, Err(Error::InputOutput)));
    }
}

#[cfg(test)]
#[path = "pipe_tests.rs"]
mod polling_tests;
