#[cfg(test)]
use super::http_common::compute_request_length;
use super::{
    http_common::{TcpSocketAdapter, create_tcp_connection, send_request, split_host_port},
    http_exchange::{ClientContext, Exchange, IO_TIMEOUT_SECONDS, read_response},
};
use alloc::{boxed::Box, vec, vec::Vec};
use core::time::Duration;
use embassy_futures::select::{Either, select};
use embedded_tls::{Aes128GcmSha256, TlsConfig, TlsConnection, TlsContext, UnsecureProvider};
use file_system::{
    BaseOperations, CharacterDevice, Context, DirectCharacterDevice, Error, MountOperations,
    Result, Size,
};
use rand_core::{TryCryptoRng, TryRng};
use shared::HttpRequestParser;
use synchronization::Arc;
const DEFAULT_HTTPS_PORT: u16 = 443;
const TLS_RECORD_BUFFER_SIZE: usize = 16384;
// TLS 1.3 ciphertext can exceed the plaintext record limit by 256 bytes
// (RFC 8446 section 5.2; embedded-tls RecordReader requires this capacity).
const TLS_READ_RECORD_BUFFER_SIZE: usize = TLS_RECORD_BUFFER_SIZE + 256;
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

struct ResponseTls<'a, 'b>(&'a mut TlsConnection<'b, TcpSocketAdapter, Aes128GcmSha256>);
impl embedded_io::ErrorType for ResponseTls<'_, '_> {
    type Error = embedded_tls::TlsError;
}
impl embedded_io_async::Read for ResponseTls<'_, '_> {
    async fn read(&mut self, buffer: &mut [u8]) -> core::result::Result<usize, Self::Error> {
        match self.0.read(buffer).await {
            Err(embedded_tls::TlsError::ConnectionClosed) => Ok(0),
            result => result,
        }
    }
}
async fn run_request<T: DirectCharacterDevice + 'static>(
    manager: &'static network::Manager,
    exchange: Arc<Exchange>,
    request: Vec<u8>,
    random: &'static T,
    generation: u64,
) {
    let mut buffer = match exchange.take_buffer(generation).await {
        Ok(buffer) => buffer,
        Err(error) => {
            exchange.finish(generation, None, Err(error)).await;
            return;
        }
    };
    let operation = async {
        let parser = HttpRequestParser::from_buffer(&request);
        let (method, _) = parser.get_request().ok_or(Error::InvalidParameter)?;
        let head_request = method == "HEAD";
        let host = parser
            .get_headers()
            .find(|(n, _)| n.eq_ignore_ascii_case("Host"))
            .map(|(_, v)| v)
            .ok_or(Error::InvalidParameter)?;
        let (host, port) = split_host_port(host, DEFAULT_HTTPS_PORT);
        let socket = create_tcp_connection(manager, host, port).await?;
        // Allocate directly on the heap; no 16 KiB arrays in the worker future.
        let mut read_record = vec![0; TLS_READ_RECORD_BUFFER_SIZE].into_boxed_slice();
        let mut write_record = vec![0; TLS_RECORD_BUFFER_SIZE].into_boxed_slice();
        let mut tls = TlsConnection::new(socket, &mut read_record, &mut write_record);
        let configuration = TlsConfig::new().with_server_name(host);
        let provider = UnsecureProvider::new::<Aes128GcmSha256>(TlsRandomNumberGenerator(
            RandomNumberGenerator(random),
        ));
        match select(
            tls.open(TlsContext::new(&configuration, provider)),
            task::sleep(Duration::from_secs(IO_TIMEOUT_SECONDS)),
        )
        .await
        {
            Either::First(Ok(())) => {}
            _ => return Err(Error::InputOutput),
        }
        match select(
            send_request(&mut tls, &request),
            task::sleep(Duration::from_secs(IO_TIMEOUT_SECONDS)),
        )
        .await
        {
            Either::First(result) => result?,
            Either::Second(_) => return Err(Error::InputOutput),
        }
        drop(configuration);
        drop(request);
        read_response(
            &mut ResponseTls(&mut tls),
            &exchange,
            generation,
            &mut buffer,
            head_request,
        )
        .await
    };
    let result = match select(operation, exchange.cancelled.wait()).await {
        Either::First(result) => result,
        Either::Second(_) => Err(Error::InvalidContext),
    };
    exchange.finish(generation, buffer, result).await;
}

pub struct HttpsClientDevice<T: DirectCharacterDevice + 'static> {
    network_manager: &'static network::Manager,
    task_manager: &'static task::Manager,
    random_device: &'static T,
}
impl<T: DirectCharacterDevice + 'static> HttpsClientDevice<T> {
    pub const fn new(
        random_device: &'static T,
        network_manager: &'static network::Manager,
        task_manager: &'static task::Manager,
    ) -> Self {
        Self {
            network_manager,
            task_manager,
            random_device,
        }
    }
}
impl<T: DirectCharacterDevice + 'static> BaseOperations for HttpsClientDevice<T> {
    fn open(&self, context: &mut Context) -> Result<()> {
        context.set_private_data(Box::new(ClientContext::new()));
        Ok(())
    }
    fn close(&self, context: &mut Context) -> Result<()> {
        if let Some(mut client) = context.take_private_data_of_type::<ClientContext>() {
            client.cancel_read();
            task::block_on(client.exchange.close());
        }
        Ok(())
    }
    fn read(
        &self,
        context: &mut Context,
        cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        _: Size,
    ) -> core::task::Poll<Result<usize>> {
        context
            .get_private_data_mutable_of_type::<ClientContext>()
            .ok_or(Error::InvalidContext)?
            .poll_read(cx, buffer)
    }
    fn write(
        &self,
        context: &mut Context,
        _: &mut core::task::Context<'_>,
        buffer: &[u8],
        _: Size,
    ) -> core::task::Poll<Result<usize>> {
        file_system::ready(|| {
            let client = context
                .get_private_data_mutable_of_type::<ClientContext>()
                .ok_or(Error::InvalidContext)?;
            let generation = client.exchange.begin()?;
            let request = buffer.to_vec();
            let exchange = client.exchange.clone();
            let manager = self.network_manager;
            let random = self.random_device;
            if task::block_on(self.task_manager.spawn(
                task::Manager::ROOT_TASK_IDENTIFIER,
                "HTTP request worker",
                None,
                move |_| async move {
                    run_request(manager, exchange, request, random, generation).await;
                },
            ))
            .is_err()
            {
                task::block_on(
                    client
                        .exchange
                        .finish(generation, None, Err(Error::RessourceBusy)),
                );
                return Err(Error::RessourceBusy);
            }
            Ok(buffer.len())
        })
    }
    fn clone_context(&self, context: &Context) -> Result<Context> {
        let client = context
            .get_private_data_of_type::<ClientContext>()
            .ok_or(Error::InvalidContext)?;
        Ok(Context::new(Some(client.clone_context())))
    }
    fn cancel_operation(&self, context: &mut Context, operation: file_system::Operation) {
        if operation == file_system::Operation::Read
            && let Some(client) = context.get_private_data_mutable_of_type::<ClientContext>()
        {
            client.cancel_read();
        }
    }
}
impl<T: DirectCharacterDevice + 'static> MountOperations for HttpsClientDevice<T> {}
impl<T: DirectCharacterDevice + 'static> CharacterDevice for HttpsClientDevice<T> {}
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
}
