use alloc::boxed::Box;
use alloc::vec::Vec;
use core::time::Duration;
use embassy_futures::select::{Either, select};
use embedded_io;
use embedded_io_async;
use embedded_tls::{Aes128GcmSha256, TlsConfig, TlsConnection, TlsContext, UnsecureProvider};
use file_system::{
    BaseOperations, CharacterDevice, Context, DirectCharacterDevice, Error, MountOperations,
    Result, Size,
};
use network::{DnsQueryKind, Duration as NetworkDuration, Port, TcpSocket};
use rand_core::{TryCryptoRng, TryRng};
use shared::HttpRequestParser;
use synchronization::{Arc, blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};

use super::http_common::{
    build_serialized_response_headers, compute_request_length, map_network_error, split_host_port,
};

const TLS_RECORD_BUFFER_SIZE: usize = 16384; // TLS 1.2 max record size (2^14 bytes)
const RESPONSE_SCAN_BUFFER_SIZE: usize = 4096; // HTTP headers buffer (typically 2-5 KB)
const RESPONSE_SERIALIZED_HEADER_SIZE: usize = 2048; // Parsed header output buffer
const TLS_READ_CHUNK_SIZE: usize = 512;
const DEFAULT_HTTPS_PORT: u16 = 443;
const IO_TIMEOUT_SECONDS: u64 = 15;

struct RandomNumberGenerator<T: DirectCharacterDevice + 'static>(&'static T);

impl<T: DirectCharacterDevice + 'static> TryCryptoRng for RandomNumberGenerator<T> {}

impl<T: DirectCharacterDevice + 'static> TryRng for RandomNumberGenerator<T> {
    type Error = Error;

    fn try_next_u32(&mut self) -> Result<u32> {
        let mut bytes = [0u8; 4];
        self.try_fill_bytes(&mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn try_next_u64(&mut self) -> Result<u64> {
        let mut bytes = [0u8; 8];
        self.try_fill_bytes(&mut bytes)?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<()> {
        if dest.is_empty() {
            return Ok(());
        }
        if self.0.read_sync(dest, 0)? != dest.len() {
            // Never report success with missing entropy from a short read.
            return Err(Error::InputOutput);
        }
        Ok(())
    }
}

// embedded-tls 0.19 still consumes rand_core 0.6. Keep its legacy traits at
// this boundary while the device RNG implements the fallible 0.10 API.
struct TlsRandomNumberGenerator<T: DirectCharacterDevice + 'static>(RandomNumberGenerator<T>);

impl<T: DirectCharacterDevice + 'static> rand_core_06::CryptoRng for TlsRandomNumberGenerator<T> {}

impl<T: DirectCharacterDevice + 'static> rand_core_06::RngCore for TlsRandomNumberGenerator<T> {
    fn next_u32(&mut self) -> u32 {
        self.0.try_next_u32().expect("Random device read failed")
    }

    fn next_u64(&mut self) -> u64 {
        self.0.try_next_u64().expect("Random device read failed")
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.0
            .try_fill_bytes(dest)
            .expect("Random device read failed");
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> core::result::Result<(), rand_core_06::Error> {
        self.0.try_fill_bytes(dest).map_err(|e| {
            log::error!("Random device read failed: {:?}", e);
            rand_core_06::Error::from(e.get_discriminant())
        })
    }
}

#[derive(Debug, Copy, Clone)]
enum IoError {
    FileSystem(Error),
}

impl embedded_io::Error for IoError {
    fn kind(&self) -> embedded_io::ErrorKind {
        match self {
            IoError::FileSystem(Error::NotFound) => embedded_io::ErrorKind::NotFound,
            IoError::FileSystem(Error::PermissionDenied) => {
                embedded_io::ErrorKind::PermissionDenied
            }
            _ => embedded_io::ErrorKind::Other,
        }
    }
}

impl core::fmt::Display for IoError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            IoError::FileSystem(error) => write!(formatter, "filesystem error: {:?}", error),
        }
    }
}

impl core::error::Error for IoError {}

struct TcpSocketAdapter {
    socket: TcpSocket,
}

impl embedded_io::ErrorType for TcpSocketAdapter {
    type Error = IoError;
}

impl embedded_io_async::Read for TcpSocketAdapter {
    async fn read(&mut self, buffer: &mut [u8]) -> core::result::Result<usize, Self::Error> {
        self.socket
            .read(buffer)
            .await
            .map_err(map_network_error)
            .map_err(IoError::FileSystem)
    }
}

impl embedded_io_async::Write for TcpSocketAdapter {
    async fn write(&mut self, buffer: &[u8]) -> core::result::Result<usize, Self::Error> {
        self.socket
            .write(buffer)
            .await
            .map_err(map_network_error)
            .map_err(IoError::FileSystem)
    }

    async fn flush(&mut self) -> core::result::Result<(), Self::Error> {
        self.socket
            .flush()
            .await
            .map_err(map_network_error)
            .map_err(IoError::FileSystem)
    }
}

enum State {
    Idle,
    InFlight,
    HeadersReady,
    BodyStreaming,
    Failed(Error),
}

struct HttpsClientContext {
    waiters: Arc<file_system::WaitQueue<16>>,
    waiter: file_system::Waiter,
    inner: Arc<Mutex<CriticalSectionRawMutex, HttpsClientInner>>,
}

struct HttpsClientInner {
    generation: u64,
    state: State,
    response_headers: [u8; RESPONSE_SERIALIZED_HEADER_SIZE],
    response_headers_len: usize,
    response_headers_cursor: usize,
    response_body: Vec<u8>,
    response_body_cursor: usize,
}

impl HttpsClientContext {
    fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HttpsClientInner::new())),
            waiters: Arc::new(file_system::WaitQueue::new()),
            waiter: file_system::Waiter::new(),
        }
    }
}

impl HttpsClientInner {
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

unsafe impl Send for HttpsClientContext {}
unsafe impl Sync for HttpsClientContext {}

fn map_tls_error(error: embedded_tls::TlsError) -> Error {
    log::error!("https_client: tls error: {:?}", error);

    match error {
        embedded_tls::TlsError::ConnectionClosed => Error::InputOutput,
        embedded_tls::TlsError::Io(embedded_io::ErrorKind::TimedOut) => Error::InputOutput,
        _ => Error::InputOutput,
    }
}

async fn read_response_headers_and_body(
    tls: &mut TlsConnection<'_, TcpSocketAdapter, Aes128GcmSha256>,
    raw_headers: &mut [u8; RESPONSE_SCAN_BUFFER_SIZE],
) -> Result<(usize, Vec<u8>)> {
    let mut filled = 0usize;
    let mut chunk = [0u8; TLS_READ_CHUNK_SIZE];

    loop {
        let bytes_read = match select(
            tls.read(&mut chunk),
            task::sleep(Duration::from_secs(IO_TIMEOUT_SECONDS)),
        )
        .await
        {
            Either::First(result) => match result {
                Ok(size) => size,
                Err(embedded_tls::TlsError::ConnectionClosed) => 0,
                Err(error) => return Err(map_tls_error(error)),
            },
            Either::Second(_) => {
                log::warning!("https_client: timeout waiting for response header bytes");
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
                    tls.read(&mut chunk),
                    task::sleep(Duration::from_secs(IO_TIMEOUT_SECONDS)),
                )
                .await
                {
                    Either::First(result) => match result {
                        Ok(size) => size,
                        Err(embedded_tls::TlsError::ConnectionClosed) => 0,
                        Err(error) => return Err(map_tls_error(error)),
                    },
                    Either::Second(_) => {
                        log::warning!("https_client: timeout waiting for response body bytes");
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

async fn write_tls_all(
    tls: &mut TlsConnection<'_, TcpSocketAdapter, Aes128GcmSha256>,
    mut payload: &[u8],
) -> Result<()> {
    while !payload.is_empty() {
        let bytes_written = tls.write(payload).await.map_err(map_tls_error)?;
        if bytes_written == 0 {
            return Err(Error::InputOutput);
        }
        payload = &payload[bytes_written..];
    }

    Ok(())
}

async fn create_tls_connection<'a, T: DirectCharacterDevice + 'static>(
    manager: &'static network::Manager,
    host: &str,
    port: u16,
    read_record: &'a mut [u8; TLS_RECORD_BUFFER_SIZE],
    write_record: &'a mut [u8; TLS_RECORD_BUFFER_SIZE],
    random_device: &'static T,
) -> Result<TlsConnection<'a, TcpSocketAdapter, Aes128GcmSha256>> {
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

    let mut tls = TlsConnection::new(TcpSocketAdapter { socket }, read_record, write_record);

    let configuration = TlsConfig::new().with_server_name(host);
    let provider = UnsecureProvider::new::<Aes128GcmSha256>(TlsRandomNumberGenerator(
        RandomNumberGenerator(random_device),
    ));
    let context = TlsContext::new(&configuration, provider);

    tls.open(context).await.map_err(map_tls_error)?;

    Ok(tls)
}

async fn run_request<T: DirectCharacterDevice + 'static>(
    network_manager: &'static network::Manager,
    inner: Arc<Mutex<CriticalSectionRawMutex, HttpsClientInner>>,
    request: Vec<u8>,
    random_device: &'static T,
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

        let (host, port) = split_host_port(host_header, DEFAULT_HTTPS_PORT);

        let mut read_record = [0u8; TLS_RECORD_BUFFER_SIZE];
        let mut write_record = [0u8; TLS_RECORD_BUFFER_SIZE];
        let mut tls = create_tls_connection(
            network_manager,
            host,
            port,
            &mut read_record,
            &mut write_record,
            random_device,
        )
        .await?;

        let request_length = compute_request_length(&request, parser)?;
        let payload = &request[..request_length];

        write_tls_all(&mut tls, payload).await?;

        let has_header_terminator = payload.windows(4).any(|window| window == b"\r\n\r\n");
        if !has_header_terminator {
            let suffix = if payload.ends_with(b"\r\n") {
                b"\r\n".as_slice()
            } else {
                b"\r\n\r\n".as_slice()
            };

            log::warning!(
                "https_client: request missing header terminator, appending {} bytes",
                suffix.len()
            );

            write_tls_all(&mut tls, suffix).await?;
        }

        tls.flush().await.map_err(map_tls_error)?;

        let mut raw_headers = [0u8; RESPONSE_SCAN_BUFFER_SIZE];

        let (raw_headers_end, response_body) =
            read_response_headers_and_body(&mut tls, &mut raw_headers).await?;

        let mut response_headers = [0u8; RESPONSE_SERIALIZED_HEADER_SIZE];
        let serialized_headers_len = build_serialized_response_headers(
            &raw_headers[..raw_headers_end],
            &mut response_headers,
        )?;

        // Skip explicit TLS close as it may block indefinitely.
        // The TLS connection and socket will be dropped naturally when this scope ends.
        // This avoids deadlock while still cleaning up resources.
        drop(tls);

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
    inner: &Mutex<CriticalSectionRawMutex, HttpsClientInner>,
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
                log::warning!("https_client: state is not InFlight, returning early");
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
                log::error!("https_client: run_request failed: {:?}", error);
            }
        }
    }

    drop(guard);
    waiters.wake_all();
}

pub struct HttpsClientDevice<T: DirectCharacterDevice + 'static> {
    random_device: &'static T,
    network_manager: &'static network::Manager,
    task_manager: &'static task::Manager,
}

impl<T: DirectCharacterDevice + 'static> HttpsClientDevice<T> {
    pub const fn new(
        random_device: &'static T,
        network_manager: &'static network::Manager,
        task_manager: &'static task::Manager,
    ) -> Self {
        Self {
            random_device,
            network_manager,
            task_manager,
        }
    }
}

impl<T: DirectCharacterDevice + 'static> BaseOperations for HttpsClientDevice<T> {
    fn open(&self, context: &mut Context) -> Result<()> {
        context.set_private_data(Box::new(HttpsClientContext::new()));
        Ok(())
    }

    fn close(&self, context: &mut Context) -> Result<()> {
        if let Some(mut client_context) = context.take_private_data_of_type::<HttpsClientContext>()
        {
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
            .get_private_data_mutable_of_type::<HttpsClientContext>()
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
                .get_private_data_mutable_of_type::<HttpsClientContext>()
                .ok_or(Error::InvalidParameter)?;

            let generation = write_state_gate(context)?;

            let request = buffer.to_vec();
            let inner = context.inner.clone();
            let waiters = context.waiters.clone();

            let task_manager = self.task_manager;
            let network_manager = self.network_manager;
            let parent = task::Manager::ROOT_TASK_IDENTIFIER;

            let random_device = self.random_device;

            if let Err(spawn_error) = task::block_on(task_manager.spawn(
                parent,
                "HTTPS request worker",
                None,
                move |_| {
                    let inner_clone = inner.clone();
                    let request_owned = request;
                    async move {
                        run_request(
                            network_manager,
                            inner_clone,
                            request_owned,
                            random_device,
                            waiters,
                            generation,
                        )
                        .await
                    }
                },
            )) {
                task::block_on(complete_request(
                    &context.inner,
                    &context.waiters,
                    generation,
                    Err(Error::RessourceBusy),
                ));
                log::error!(
                    "https_client: failed to spawn request worker: {:?}",
                    spawn_error
                );
                return Err(Error::RessourceBusy);
            }

            Ok(buffer.len())
        })
    }

    fn clone_context(&self, context: &Context) -> Result<Context> {
        let source = context
            .get_private_data_of_type::<HttpsClientContext>()
            .ok_or(Error::InvalidParameter)?;

        Ok(Context::new(Some(HttpsClientContext {
            inner: source.inner.clone(),
            waiters: source.waiters.clone(),
            waiter: file_system::Waiter::new(),
        })))
    }
    fn cancel_operation(&self, context: &mut Context, operation: file_system::Operation) {
        if operation == file_system::Operation::Read
            && let Some(context) = context.get_private_data_mutable_of_type::<HttpsClientContext>()
        {
            context.waiters.cancel(&mut context.waiter);
        }
    }
}

impl<T: DirectCharacterDevice + 'static> MountOperations for HttpsClientDevice<T> {}

impl<T: DirectCharacterDevice + 'static> CharacterDevice for HttpsClientDevice<T> {}

fn write_state_gate(context: &mut HttpsClientContext) -> Result<u64> {
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

fn read_transition_inner(inner: &mut HttpsClientInner, buffer: &mut [u8]) -> Result<usize> {
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
                // Only reset if we actually had a body (headers_len > 0).
                // If headers_len is 0, we're being called prematurely during request setup.
                if inner.response_headers_len > 0 {
                    inner.reset();
                }
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
fn read_state_transition(context: &mut HttpsClientContext, buffer: &mut [u8]) -> Result<usize> {
    let mut inner = task::block_on(context.inner.lock());
    read_transition_inner(&mut inner, buffer)
}
fn poll_read_state(
    context: &mut HttpsClientContext,
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
    use file_system::DirectBaseOperations;

    struct TestRandomDevice {
        limit: usize,
        fail: bool,
    }

    impl DirectBaseOperations for TestRandomDevice {
        fn read(
            &self,
            _cx: &mut core::task::Context<'_>,
            buffer: &mut [u8],
            _: Size,
        ) -> core::task::Poll<Result<usize>> {
            file_system::ready(|| {
                if self.fail {
                    return Err(Error::InputOutput);
                }
                let count = buffer.len().min(self.limit);
                for (index, byte) in buffer[..count].iter_mut().enumerate() {
                    *byte = (index + 1) as u8;
                }
                Ok(count)
            })
        }

        fn write(
            &self,
            _cx: &mut core::task::Context<'_>,
            _: &[u8],
            _: Size,
        ) -> core::task::Poll<Result<usize>> {
            file_system::ready(|| Err(Error::UnsupportedOperation))
        }

        fn set_position(&self, _: Size, _: &file_system::Position) -> Result<Size> {
            Err(Error::UnsupportedOperation)
        }
    }

    impl MountOperations for TestRandomDevice {}
    impl DirectCharacterDevice for TestRandomDevice {}

    static COMPLETE_RANDOM: TestRandomDevice = TestRandomDevice {
        limit: usize::MAX,
        fail: false,
    };
    static SHORT_RANDOM: TestRandomDevice = TestRandomDevice {
        limit: 2,
        fail: false,
    };
    static FAILED_RANDOM: TestRandomDevice = TestRandomDevice {
        limit: 0,
        fail: true,
    };

    #[test]
    fn random_generator_provides_fallible_rand_core_010_values() {
        use rand_core::TryRng;
        fn accepts_crypto_rng(_: impl rand_core::TryCryptoRng<Error = Error>) {}
        accepts_crypto_rng(RandomNumberGenerator(&COMPLETE_RANDOM));
        let mut rng = RandomNumberGenerator(&COMPLETE_RANDOM);
        assert_eq!(rng.try_next_u32(), Ok(0x0403_0201));
        assert_eq!(rng.try_next_u64(), Ok(0x0807_0605_0403_0201));
        let mut bytes = [0; 3];
        rng.try_fill_bytes(&mut bytes).unwrap();
        assert_eq!(bytes, [1, 2, 3]);
    }

    #[test]
    fn random_generator_propagates_device_errors_and_rejects_short_reads() {
        use rand_core::TryRng;
        for device in [&SHORT_RANDOM, &FAILED_RANDOM] {
            let mut rng = RandomNumberGenerator(device);
            assert_eq!(rng.try_next_u32(), Err(Error::InputOutput));
            assert_eq!(rng.try_next_u64(), Err(Error::InputOutput));
            assert_eq!(rng.try_fill_bytes(&mut [0; 3]), Err(Error::InputOutput));
        }
        // Empty requests do not need entropy and must not touch a failing device.
        assert_eq!(
            RandomNumberGenerator(&FAILED_RANDOM).try_fill_bytes(&mut []),
            Ok(())
        );
    }

    #[test]
    fn tls_rng_adapter_preserves_values_and_legacy_error_codes() {
        use rand_core_06::RngCore;
        fn accepts_tls_rng(_: impl embedded_tls::CryptoRngCore) {}
        accepts_tls_rng(TlsRandomNumberGenerator(RandomNumberGenerator(
            &COMPLETE_RANDOM,
        )));
        let mut rng = TlsRandomNumberGenerator(RandomNumberGenerator(&COMPLETE_RANDOM));
        assert_eq!(rng.next_u32(), 0x0403_0201);
        assert_eq!(rng.next_u64(), 0x0807_0605_0403_0201);
        let mut bytes = [0; 3];
        rng.fill_bytes(&mut bytes);
        assert_eq!(bytes, [1, 2, 3]);
        let mut rng = TlsRandomNumberGenerator(RandomNumberGenerator(&FAILED_RANDOM));
        assert_eq!(
            rng.try_fill_bytes(&mut [0; 3]).unwrap_err().code(),
            Some(Error::InputOutput.get_discriminant())
        );
    }

    #[test]
    #[should_panic(expected = "Random device read failed")]
    fn tls_rng_adapter_panics_when_infallible_entropy_requests_fail() {
        use rand_core_06::RngCore;
        TlsRandomNumberGenerator(RandomNumberGenerator(&FAILED_RANDOM)).next_u64();
    }

    #[test]
    fn split_host_port_parses_default_port() {
        let (host, port) = split_host_port("example.com", DEFAULT_HTTPS_PORT);
        assert_eq!(host, "example.com");
        assert_eq!(port, 443);
    }

    #[test]
    fn split_host_port_parses_explicit_port() {
        let (host, port) = split_host_port("example.com:8443", DEFAULT_HTTPS_PORT);
        assert_eq!(host, "example.com");
        assert_eq!(port, 8443);
    }

    #[test]
    fn compute_request_length_ignores_trailing_zeroes() {
        let request = b"GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n";
        let mut request_buffer = [0u8; 256];
        request_buffer[..request.len()].copy_from_slice(request);
        let parser = HttpRequestParser::from_buffer(&request_buffer);

        let length = compute_request_length(&request_buffer, parser).unwrap();
        assert_eq!(length, request.len());
    }

    #[test]
    fn compute_request_length_accepts_no_header_terminator() {
        let request = b"GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n";
        let mut request_buffer = [0u8; 256];
        request_buffer[..request.len()].copy_from_slice(request);
        let parser = HttpRequestParser::from_buffer(&request_buffer);

        let length = compute_request_length(&request_buffer, parser).unwrap();
        assert_eq!(length, request.len());
    }

    #[test]
    fn write_rejected_when_request_in_flight() {
        let mut context = HttpsClientContext::new();
        {
            let mut inner = task::block_on(context.inner.lock());
            inner.state = State::InFlight;
        }

        assert_eq!(write_state_gate(&mut context), Err(Error::RessourceBusy));
    }

    #[test]
    fn read_returns_resource_busy_while_in_flight() {
        let mut context = HttpsClientContext::new();
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
        let mut context = HttpsClientContext::new();
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
        let mut context = HttpsClientContext::new();
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
        let mut context = HttpsClientContext::new();
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
