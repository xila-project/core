use alloc::boxed::Box;
use alloc::vec::Vec;
use core::time::Duration;

use embassy_futures::select::{Either, select};
use file_system::{BaseOperations, CharacterDevice, Context, Error, MountOperations, Result, Size};
use network::{DnsQueryKind, Duration as NetworkDuration, Port, TcpSocket};
use shared::HttpRequestParser;
use synchronization::{Arc, blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};

use super::http_common::{
    build_serialized_response_headers, compute_request_length, map_network_error, split_host_port,
};

const RESPONSE_SCAN_BUFFER_SIZE: usize = 3072;
const RESPONSE_SERIALIZED_HEADER_SIZE: usize = 2048;
const TCP_READ_CHUNK_SIZE: usize = 512;
const DEFAULT_HTTP_PORT: u16 = 80;
const IO_TIMEOUT_SECONDS: u64 = 15;

enum State {
    Idle,
    InFlight,
    HeadersReady,
    BodyStreaming,
    Failed(Error),
}

struct HttpClientContext {
    waiters: Arc<file_system::WaitQueue<16>>,
    waiter: file_system::Waiter,
    inner: Arc<Mutex<CriticalSectionRawMutex, HttpClientInner>>,
}

struct HttpClientInner {
    generation: u64,
    state: State,
    response_headers: [u8; RESPONSE_SERIALIZED_HEADER_SIZE],
    response_headers_len: usize,
    response_headers_cursor: usize,
    response_body: Vec<u8>,
    response_body_cursor: usize,
}

impl HttpClientContext {
    fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HttpClientInner::new())),
            waiters: Arc::new(file_system::WaitQueue::new()),
            waiter: file_system::Waiter::new(),
        }
    }
}

impl HttpClientInner {
    fn new() -> Self {
        Self {
            state: State::Idle,
            generation: 0,
            response_headers: [0; RESPONSE_SERIALIZED_HEADER_SIZE],
            response_headers_len: 0,
            response_headers_cursor: 0,
            response_body: Vec::new(),
            response_body_cursor: 0,
        }
    }

    fn reset_buffers(&mut self) {
        self.response_headers_len = 0;
        self.response_headers_cursor = 0;
        self.response_body.clear();
        self.response_body_cursor = 0;
    }

    fn reset(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.reset_buffers();
        self.state = State::Idle;
    }

    fn set_failed(&mut self, error: Error) {
        self.reset_buffers();
        self.state = State::Failed(error);
    }
}

unsafe impl Send for HttpClientContext {}
unsafe impl Sync for HttpClientContext {}

async fn read_response_headers_and_body(
    socket: &mut TcpSocket,
    raw_headers: &mut [u8; RESPONSE_SCAN_BUFFER_SIZE],
) -> Result<(usize, Vec<u8>)> {
    let mut filled = 0usize;
    let mut chunk = [0u8; TCP_READ_CHUNK_SIZE];

    loop {
        let bytes_read = match select(
            socket.read(&mut chunk),
            task::sleep(Duration::from_secs(IO_TIMEOUT_SECONDS)),
        )
        .await
        {
            Either::First(result) => result.map_err(map_network_error)?,
            Either::Second(_) => {
                log::warning!("http_client: timeout waiting for response header bytes");
                return Err(Error::InputOutput);
            }
        };

        if bytes_read == 0 {
            return Err(Error::InputOutput);
        }

        let destination = raw_headers
            .get_mut(filled..filled + bytes_read)
            .ok_or(Error::FileTooLarge)?;
        destination.copy_from_slice(&chunk[..bytes_read]);
        filled += bytes_read;

        if let Some(headers_end) = raw_headers[..filled]
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|position| position + 4)
        {
            let mut body = Vec::new();
            if filled > headers_end {
                body.extend_from_slice(&raw_headers[headers_end..filled]);
            }

            loop {
                let bytes_read = match select(
                    socket.read(&mut chunk),
                    task::sleep(Duration::from_secs(IO_TIMEOUT_SECONDS)),
                )
                .await
                {
                    Either::First(result) => result.map_err(map_network_error)?,
                    Either::Second(_) => {
                        log::warning!("http_client: timeout waiting for response body bytes");
                        return Err(Error::InputOutput);
                    }
                };

                if bytes_read == 0 {
                    break;
                }

                body.extend_from_slice(&chunk[..bytes_read]);
            }

            return Ok((headers_end, body));
        }
    }
}

async fn write_tcp_all(socket: &mut TcpSocket, mut payload: &[u8]) -> Result<()> {
    while !payload.is_empty() {
        let bytes_written = socket.write(payload).await.map_err(map_network_error)?;
        if bytes_written == 0 {
            return Err(Error::InputOutput);
        }
        payload = &payload[bytes_written..];
    }

    Ok(())
}

async fn create_tcp_connection(
    manager: &'static network::Manager,
    host: &str,
    port: u16,
) -> Result<TcpSocket> {
    log::information!("http_client: create session host='{}' port={}", host, port);

    let address = manager
        .resolve(host, DnsQueryKind::A | DnsQueryKind::Aaaa, true, None)
        .await
        .map_err(map_network_error)?
        .first()
        .cloned()
        .ok_or(Error::NotFound)?;

    let mut socket = manager
        .new_tcp_socket(4096, 4096, None)
        .await
        .map_err(map_network_error)?;
    socket
        .set_timeout(Some(NetworkDuration::from_seconds(IO_TIMEOUT_SECONDS)))
        .await;
    socket
        .connect(address, Port::from_inner(port))
        .await
        .map_err(map_network_error)?;
    socket
        .set_timeout(Some(NetworkDuration::from_seconds(IO_TIMEOUT_SECONDS)))
        .await;

    Ok(socket)
}

async fn run_request(
    network_manager: &'static network::Manager,
    inner: Arc<Mutex<CriticalSectionRawMutex, HttpClientInner>>,
    request: Vec<u8>,
    waiters: Arc<file_system::WaitQueue<16>>,
    generation: u64,
) {
    let result = async {
        let parser = HttpRequestParser::from_buffer(&request);
        let _ = parser.get_request().ok_or(Error::InvalidParameter)?;

        let host_header = parser
            .get_headers()
            .find(|(name, _)| *name == HttpRequestParser::HOST_HEADER)
            .map(|(_, value)| value)
            .ok_or(Error::InvalidParameter)?;

        let (host, port) = split_host_port(host_header, DEFAULT_HTTP_PORT);

        let mut socket = create_tcp_connection(network_manager, host, port).await?;

        let request_length = compute_request_length(&request, parser)?;
        let payload = &request[..request_length];

        write_tcp_all(&mut socket, payload).await?;

        let has_header_terminator = payload.windows(4).any(|window| window == b"\r\n\r\n");
        if !has_header_terminator {
            let suffix = if payload.ends_with(b"\r\n") {
                b"\r\n".as_slice()
            } else {
                b"\r\n\r\n".as_slice()
            };

            write_tcp_all(&mut socket, suffix).await?;
        }

        socket.flush().await.map_err(map_network_error)?;

        let mut raw_headers = [0u8; RESPONSE_SCAN_BUFFER_SIZE];
        let (raw_headers_end, response_body) =
            read_response_headers_and_body(&mut socket, &mut raw_headers).await?;

        let mut response_headers = [0u8; RESPONSE_SERIALIZED_HEADER_SIZE];
        let serialized_headers_len = build_serialized_response_headers(
            &raw_headers[..raw_headers_end],
            &mut response_headers,
        )?;

        socket.close().await;

        Ok::<(usize, [u8; RESPONSE_SERIALIZED_HEADER_SIZE], Vec<u8>), Error>((
            serialized_headers_len,
            response_headers,
            response_body,
        ))
    }
    .await;

    complete_request(&inner, &waiters, generation, result).await;
}
async fn complete_request(
    inner: &Mutex<CriticalSectionRawMutex, HttpClientInner>,
    waiters: &file_system::WaitQueue<16>,
    generation: u64,
    result: Result<(usize, [u8; RESPONSE_SERIALIZED_HEADER_SIZE], Vec<u8>)>,
) {
    let mut guard = inner.lock().await;
    if guard.generation != generation || !matches!(guard.state, State::InFlight) {
        return;
    }

    match result {
        Ok((serialized_headers_len, response_headers, response_body)) => {
            if !matches!(guard.state, State::InFlight) {
                return;
            }

            guard.response_headers[..serialized_headers_len]
                .copy_from_slice(&response_headers[..serialized_headers_len]);
            guard.response_headers_len = serialized_headers_len;
            guard.response_headers_cursor = 0;

            guard.response_body = response_body;
            guard.response_body_cursor = 0;

            guard.state = State::HeadersReady;
        }
        Err(error) => {
            if matches!(guard.state, State::InFlight) {
                guard.set_failed(error);
            }
        }
    }

    drop(guard);
    waiters.wake_all();
}

pub struct HttpClientDevice {
    network_manager: &'static network::Manager,
    task_manager: &'static task::Manager,
}

impl HttpClientDevice {
    pub const fn new(
        network_manager: &'static network::Manager,
        task_manager: &'static task::Manager,
    ) -> Self {
        Self {
            network_manager,
            task_manager,
        }
    }
}

impl BaseOperations for HttpClientDevice {
    fn open(&self, context: &mut Context) -> Result<()> {
        context.set_private_data(Box::new(HttpClientContext::new()));
        Ok(())
    }

    fn close(&self, context: &mut Context) -> Result<()> {
        if let Some(mut client_context) = context.take_private_data_of_type::<HttpClientContext>() {
            let waiters = client_context.waiters.clone();
            waiters.cancel(&mut client_context.waiter);
            let mut inner = task::block_on(client_context.inner.lock());
            inner.reset();
            drop(inner);
            client_context.waiters.wake_all();
        }

        Ok(())
    }

    fn read(
        &self,
        context: &mut Context,
        _cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        _: Size,
    ) -> core::task::Poll<Result<usize>> {
        let context = context
            .get_private_data_mutable_of_type::<HttpClientContext>()
            .ok_or(Error::InvalidContext)?;
        poll_read_state(context, _cx, buffer)
    }

    fn write(
        &self,
        context: &mut Context,
        _cx: &mut core::task::Context<'_>,
        buffer: &[u8],
        _: Size,
    ) -> core::task::Poll<Result<usize>> {
        file_system::ready(|| {
            let context = context
                .get_private_data_mutable_of_type::<HttpClientContext>()
                .ok_or(Error::InvalidParameter)?;

            let generation = write_state_gate(context)?;

            let request = buffer.to_vec();
            let inner = context.inner.clone();
            let waiters = context.waiters.clone();

            let task_manager = self.task_manager;
            let network_manager = self.network_manager;
            let parent = task::Manager::ROOT_TASK_IDENTIFIER;

            if task::block_on(
                task_manager.spawn(parent, "HTTP request worker", None, move |_| {
                    let inner_clone = inner.clone();
                    let request_owned = request;
                    async move {
                        run_request(
                            network_manager,
                            inner_clone,
                            request_owned,
                            waiters,
                            generation,
                        )
                        .await
                    }
                }),
            )
            .is_err()
            {
                task::block_on(complete_request(
                    &context.inner,
                    &context.waiters,
                    generation,
                    Err(Error::RessourceBusy),
                ));
                return Err(Error::RessourceBusy);
            }

            Ok(buffer.len())
        })
    }

    fn clone_context(&self, context: &Context) -> Result<Context> {
        let source = context
            .get_private_data_of_type::<HttpClientContext>()
            .ok_or(Error::InvalidParameter)?;

        Ok(Context::new(Some(HttpClientContext {
            inner: source.inner.clone(),
            waiters: source.waiters.clone(),
            waiter: file_system::Waiter::new(),
        })))
    }
    fn cancel_operation(&self, context: &mut Context, operation: file_system::Operation) {
        if operation == file_system::Operation::Read
            && let Some(context) = context.get_private_data_mutable_of_type::<HttpClientContext>()
        {
            context.waiters.cancel(&mut context.waiter);
        }
    }
}

impl MountOperations for HttpClientDevice {}

impl CharacterDevice for HttpClientDevice {}

fn write_state_gate(context: &mut HttpClientContext) -> Result<u64> {
    let mut inner = task::block_on(context.inner.lock());

    match inner.state {
        State::Idle | State::Failed(_) => {
            inner.reset();
            inner.state = State::InFlight;
            Ok(inner.generation)
        }
        State::InFlight | State::HeadersReady | State::BodyStreaming => Err(Error::RessourceBusy),
    }
}

fn read_transition_inner(inner: &mut HttpClientInner, buffer: &mut [u8]) -> Result<usize> {
    match inner.state {
        State::Idle => Ok(0),
        State::InFlight => Err(Error::RessourceBusy),
        State::Failed(error) => {
            inner.reset();
            Err(error)
        }
        State::HeadersReady => {
            let remaining = inner
                .response_headers_len
                .saturating_sub(inner.response_headers_cursor);
            let bytes_to_copy = remaining.min(buffer.len());

            buffer[..bytes_to_copy].copy_from_slice(
                &inner.response_headers
                    [inner.response_headers_cursor..inner.response_headers_cursor + bytes_to_copy],
            );

            inner.response_headers_cursor += bytes_to_copy;

            if inner.response_headers_cursor >= inner.response_headers_len {
                inner.state = State::BodyStreaming;
            }

            Ok(bytes_to_copy)
        }
        State::BodyStreaming => {
            if inner.response_body_cursor >= inner.response_body.len() {
                inner.reset();
                return Ok(0);
            }

            let remaining = inner
                .response_body
                .len()
                .saturating_sub(inner.response_body_cursor);
            let bytes_to_copy = remaining.min(buffer.len());
            buffer[..bytes_to_copy].copy_from_slice(
                &inner.response_body
                    [inner.response_body_cursor..inner.response_body_cursor + bytes_to_copy],
            );
            inner.response_body_cursor += bytes_to_copy;
            Ok(bytes_to_copy)
        }
    }
}
#[cfg(test)]
fn read_state_transition(context: &mut HttpClientContext, buffer: &mut [u8]) -> Result<usize> {
    let mut inner = task::block_on(context.inner.lock());
    read_transition_inner(&mut inner, buffer)
}
fn poll_read_state(
    context: &mut HttpClientContext,
    cx: &mut core::task::Context<'_>,
    buffer: &mut [u8],
) -> core::task::Poll<Result<usize>> {
    use core::task::Poll;
    if buffer.is_empty() {
        return Poll::Ready(Ok(0));
    }
    {
        let mut inner = context.inner.try_lock().map_err(|_| Error::RessourceBusy)?;
        if !matches!(inner.state, State::InFlight) {
            return Poll::Ready(read_transition_inner(&mut inner, buffer));
        }
    }
    context.waiters.register(&mut context.waiter, cx.waker())?;
    let mut inner = context.inner.try_lock().map_err(|_| Error::RessourceBusy)?;
    if matches!(inner.state, State::InFlight) {
        Poll::Pending
    } else {
        Poll::Ready(read_transition_inner(&mut inner, buffer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_host_port_parses_default_port() {
        let (host, port) = split_host_port("example.com", DEFAULT_HTTP_PORT);
        assert_eq!(host, "example.com");
        assert_eq!(port, 80);
    }

    #[test]
    fn split_host_port_parses_explicit_port() {
        let (host, port) = split_host_port("example.com:8080", DEFAULT_HTTP_PORT);
        assert_eq!(host, "example.com");
        assert_eq!(port, 8080);
    }

    #[test]
    fn write_rejected_when_request_in_flight() {
        let mut context = HttpClientContext::new();
        {
            let mut inner = task::block_on(context.inner.lock());
            inner.state = State::InFlight;
        }

        assert_eq!(write_state_gate(&mut context), Err(Error::RessourceBusy));
    }

    #[test]
    fn read_returns_resource_busy_while_in_flight() {
        let mut context = HttpClientContext::new();
        let mut buffer = [0u8; 16];
        {
            let mut inner = task::block_on(context.inner.lock());
            inner.state = State::InFlight;
        }

        assert_eq!(
            read_state_transition(&mut context, &mut buffer),
            Err(Error::RessourceBusy)
        );
    }
}

#[cfg(test)]
mod polling_regression {
    extern crate std;
    use super::*;
    use core::task::{Context as TaskContext, Poll, Waker};
    #[test]
    fn in_flight_read_is_pending_but_a_failed_busy_result_stays_an_error() {
        let mut context = HttpClientContext::new();
        let mut output = [0; 16];
        let mut cx = TaskContext::from_waker(Waker::noop());
        task::block_on(context.inner.lock()).state = State::InFlight;
        assert_eq!(
            poll_read_state(&mut context, &mut cx, &mut output),
            Poll::Pending
        );
        task::block_on(context.inner.lock()).set_failed(Error::RessourceBusy);
        assert_eq!(
            poll_read_state(&mut context, &mut cx, &mut output),
            Poll::Ready(Err(Error::RessourceBusy))
        );
    }
}

#[cfg(test)]
mod completion_regression {
    extern crate std;
    use super::*;
    use core::task::{Context as TaskContext, Poll, Waker};
    use std::{
        sync::{
            Arc as StdArc,
            atomic::{AtomicUsize, Ordering},
        },
        task::Wake,
    };
    #[derive(Default)]
    struct Count(AtomicUsize);
    impl Wake for Count {
        fn wake(self: StdArc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    fn response() -> Result<(usize, [u8; RESPONSE_SERIALIZED_HEADER_SIZE], Vec<u8>)> {
        let mut headers = [0; RESPONSE_SERIALIZED_HEADER_SIZE];
        headers[..2].copy_from_slice(b"OK");
        Ok((2, headers, Vec::new()))
    }
    #[test]
    fn completion_notifies_the_pending_opening() {
        let mut context = HttpClientContext::new();
        let generation = write_state_gate(&mut context).unwrap();
        let count = StdArc::new(Count::default());
        let waker = Waker::from(count.clone());
        let mut cx = TaskContext::from_waker(&waker);
        let mut output = [0; 8];
        assert!(poll_read_state(&mut context, &mut cx, &mut output).is_pending());
        assert_eq!(count.0.load(Ordering::Relaxed), 0);
        task::block_on(complete_request(
            &context.inner,
            &context.waiters,
            generation,
            response(),
        ));
        assert_eq!(count.0.load(Ordering::Relaxed), 1);
        assert_eq!(
            poll_read_state(&mut context, &mut cx, &mut output),
            Poll::Ready(Ok(2))
        );
        assert_eq!(&output[..2], b"OK");
        context.waiters.cancel(&mut context.waiter);
    }
    #[test]
    fn an_old_request_cannot_publish_into_a_new_request_after_close() {
        let mut context = HttpClientContext::new();
        let old = write_state_gate(&mut context).unwrap();
        task::block_on(context.inner.lock()).reset();
        let new = write_state_gate(&mut context).unwrap();
        task::block_on(complete_request(
            &context.inner,
            &context.waiters,
            old,
            response(),
        ));
        let mut cx = TaskContext::from_waker(Waker::noop());
        task::block_on(complete_request(
            &context.inner,
            &context.waiters,
            old,
            Err(Error::RessourceBusy),
        ));
        let mut output = [0; 8];
        assert!(poll_read_state(&mut context, &mut cx, &mut output).is_pending());
        task::block_on(complete_request(
            &context.inner,
            &context.waiters,
            new,
            response(),
        ));
        assert_eq!(
            poll_read_state(&mut context, &mut cx, &mut output),
            Poll::Ready(Ok(2))
        );
        context.waiters.cancel(&mut context.waiter);
    }
}
