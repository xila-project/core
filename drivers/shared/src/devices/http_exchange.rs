//! One allocation per opening, reused for response head and body. The worker
//! waits for readers to drain each publication before receiving more data.
use alloc::{boxed::Box, vec};
use core::{
    task::{Context, Poll},
    time::Duration,
};
use embassy_futures::select::{Either, select};
use file_system::{Error, Result};
use synchronization::{
    Arc, blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex, signal::Signal,
};
use xila_http::{BodyDecoder, parse_response_head};

pub(super) const RESPONSE_BUFFER_SIZE: usize = 2048;
const MAX_HEADERS: usize = 32;
pub(super) const IO_TIMEOUT_SECONDS: u64 = 15;
type Buffer = Box<[u8; RESPONSE_BUFFER_SIZE]>;

#[derive(Clone, Copy)]
enum State {
    Idle,
    Pending,
    Ready,
    Complete,
    Failed(Error),
}
struct Inner {
    generation: u64,
    active: Option<u64>,
    state: State,
    buffer: Option<Buffer>,
    cursor: usize,
    end: usize,
}
pub(super) struct Exchange {
    inner: Mutex<CriticalSectionRawMutex, Inner>,
    waiters: file_system::WaitQueue<16>,
    consumed: Signal<CriticalSectionRawMutex, ()>,
    pub cancelled: Signal<CriticalSectionRawMutex, ()>,
}
pub(super) struct ClientContext {
    pub exchange: Arc<Exchange>,
    waiter: file_system::Waiter,
}
impl ClientContext {
    pub fn new() -> Self {
        Self {
            exchange: Arc::new(Exchange {
                inner: Mutex::new(Inner {
                    generation: 0,
                    active: None,
                    state: State::Idle,
                    buffer: Some(
                        vec![0; RESPONSE_BUFFER_SIZE]
                            .into_boxed_slice()
                            .try_into()
                            .ok()
                            .unwrap(),
                    ),
                    cursor: 0,
                    end: 0,
                }),
                waiters: file_system::WaitQueue::new(),
                consumed: Signal::new(),
                cancelled: Signal::new(),
            }),
            waiter: file_system::Waiter::new(),
        }
    }
    pub fn clone_context(&self) -> Self {
        Self {
            exchange: self.exchange.clone(),
            waiter: file_system::Waiter::new(),
        }
    }
    pub fn cancel_read(&mut self) {
        self.exchange.waiters.cancel(&mut self.waiter);
    }
    pub fn poll_read(&mut self, cx: &mut Context<'_>, buffer: &mut [u8]) -> Poll<Result<usize>> {
        if buffer.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let result = self.read_buffer(buffer);
        if result.is_ready() {
            self.cancel_read();
            return result;
        }
        self.exchange
            .waiters
            .register(&mut self.waiter, cx.waker())?;
        // Recheck after registration: publication may have raced the first read.
        let result = self.read_buffer(buffer);
        if result.is_ready() {
            self.cancel_read();
        }
        result
    }
    fn read_buffer(&self, buffer: &mut [u8]) -> Poll<Result<usize>> {
        {
            let mut inner = self
                .exchange
                .inner
                .try_lock()
                .map_err(|_| Error::RessourceBusy)?;
            match inner.state {
                State::Idle => Poll::Ready(Ok(0)),
                State::Pending => Poll::Pending,
                State::Failed(error) => {
                    inner.state = State::Idle;
                    Poll::Ready(Err(error))
                }
                State::Complete => {
                    inner.state = State::Idle;
                    Poll::Ready(Ok(0))
                }
                State::Ready => {
                    let n = buffer.len().min(inner.end - inner.cursor);
                    buffer[..n].copy_from_slice(
                        &inner.buffer.as_ref().unwrap()[inner.cursor..inner.cursor + n],
                    );
                    inner.cursor += n;
                    if inner.cursor == inner.end {
                        inner.state = State::Pending;
                        self.exchange.consumed.signal(());
                    }
                    Poll::Ready(Ok(n))
                }
            }
        }
    }
}
impl Exchange {
    pub fn begin(&self) -> Result<u64> {
        let mut inner = self.inner.try_lock().map_err(|_| Error::RessourceBusy)?;
        if inner.active.is_some()
            || !matches!(inner.state, State::Idle | State::Failed(_))
            || inner.buffer.is_none()
        {
            return Err(Error::RessourceBusy);
        }
        inner.generation = inner.generation.wrapping_add(1);
        inner.state = State::Pending;
        inner.active = Some(inner.generation);
        self.cancelled.reset();
        self.consumed.reset();
        Ok(inner.generation)
    }
    pub async fn close(&self) {
        let mut inner = self.inner.lock().await;
        inner.generation = inner.generation.wrapping_add(1);
        inner.state = State::Idle;
        inner.cursor = 0;
        inner.end = 0;
        drop(inner);
        self.cancelled.signal(());
        self.consumed.signal(());
        self.waiters.wake_all();
    }
    pub async fn take_buffer(&self, generation: u64) -> Result<Option<Buffer>> {
        let mut inner = self.inner.lock().await;
        if inner.generation != generation {
            return Err(Error::InvalidContext);
        }
        Ok(inner.buffer.take())
    }
    pub async fn finish(&self, generation: u64, buffer: Option<Buffer>, result: Result<()>) {
        let mut inner = self.inner.lock().await;
        if inner.active != Some(generation) {
            return;
        }
        inner.active = None;
        if inner.buffer.is_none() {
            inner.buffer = buffer;
        }
        if inner.generation == generation {
            inner.state = match result {
                Ok(()) => State::Complete,
                Err(e) => State::Failed(e),
            };
        }
        drop(inner);
        self.waiters.wake_all();
    }
    async fn publish(
        &self,
        generation: u64,
        buffer: &mut Option<Buffer>,
        start: usize,
        end: usize,
    ) -> Result<()> {
        {
            let mut inner = self.inner.lock().await;
            if inner.generation != generation {
                return Err(Error::InvalidContext);
            }
            inner.buffer = buffer.take();
            inner.cursor = start;
            inner.end = end;
            inner.state = State::Ready;
        }
        self.waiters.wake_all();
        loop {
            self.consumed.wait().await;
            let mut inner = self.inner.lock().await;
            if inner.generation != generation {
                return Err(Error::InvalidContext);
            }
            if inner.cursor == inner.end {
                *buffer = inner.buffer.take();
                return Ok(());
            }
        }
    }
}

fn framing_error(error: xila_http::Error) -> Error {
    match error {
        xila_http::Error::HeadersTooLarge => Error::FileTooLarge,
        _ => Error::InputOutput,
    }
}
async fn receive<R: embedded_io_async::Read>(io: &mut R, buffer: &mut [u8]) -> Result<usize> {
    match select(
        io.read(buffer),
        task::sleep(Duration::from_secs(IO_TIMEOUT_SECONDS)),
    )
    .await
    {
        Either::First(result) => result.map_err(|_| Error::InputOutput),
        Either::Second(_) => Err(Error::InputOutput),
    }
}
pub(super) async fn read_response<R: embedded_io_async::Read>(
    io: &mut R,
    exchange: &Exchange,
    generation: u64,
    storage: &mut Option<Buffer>,
    head_request: bool,
) -> Result<()> {
    let mut filled = 0;
    let (head, prefix) = loop {
        let buffer = storage.as_mut().ok_or(Error::InvalidContext)?;
        if let Some(head) = parse_response_head::<MAX_HEADERS>(&buffer[..filled], head_request)
            .map_err(framing_error)?
        {
            if (100..200).contains(&head.status) {
                buffer.copy_within(head.length..filled, 0);
                filled -= head.length;
                continue;
            }
            break (head, filled);
        }
        if filled == buffer.len() {
            return Err(Error::FileTooLarge);
        }
        let n = receive(io, &mut buffer[filled..]).await?;
        if n == 0 {
            return Err(Error::InputOutput);
        }
        filled += n;
    };
    exchange
        .publish(generation, storage, 0, head.length)
        .await?;
    let mut decoder = BodyDecoder::new(head.body);
    let mut start = head.length;
    let mut end = prefix;
    while !decoder.is_complete() {
        let buffer = storage.as_mut().ok_or(Error::InvalidContext)?;
        if start == end {
            start = 0;
            end = receive(io, buffer.as_mut()).await?;
            if end == 0 {
                return decoder.finish().map_err(framing_error);
            }
        }
        let n = decoder
            .decode(&mut buffer[start..end])
            .map_err(framing_error)?;
        if n != 0 {
            exchange
                .publish(generation, storage, start, start + n)
                .await?;
        }
        start = 0;
        end = 0;
    }
    Ok(())
}

#[cfg(test)]
#[path = "http_exchange_tests.rs"]
mod tests;
