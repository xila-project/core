use core::task::{Poll, Waker};
use synchronization::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    mutex::{Mutex, MutexGuard},
};

use super::{Capabilities, Config, Error, UartControl};

/// Fixed-capacity byte FIFO. Storage lives inline; nothing is allocated.
struct Ring<const N: usize> {
    bytes: [u8; N],
    head: usize,
    len: usize,
}

impl<const N: usize> Ring<N> {
    const fn new() -> Self {
        Self {
            bytes: [0; N],
            head: 0,
            len: 0,
        }
    }

    const fn free(&self) -> usize {
        N - self.len
    }

    fn push(&mut self, byte: u8) -> bool {
        if self.len == N {
            return false;
        }
        self.bytes[(self.head + self.len) % N] = byte;
        self.len += 1;
        true
    }

    fn pop(&mut self) -> Option<u8> {
        if self.len == 0 {
            return None;
        }
        let byte = self.bytes[self.head];
        self.head = (self.head + 1) % N;
        self.len -= 1;
        Some(byte)
    }

    fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
    }
}

/// Moves bytes from `from` to `to` while `to` has room and returns the count.
/// Bytes that do not fit stay queued in `from`, which models back-pressure
/// rather than loss.
fn move_bytes<const N: usize>(from: &mut Ring<N>, to: &mut Ring<N>) -> usize {
    let mut moved = 0;
    while to.free() > 0 {
        let Some(byte) = from.pop() else { break };
        to.push(byte);
        moved += 1;
    }
    moved
}

/// Each direction holds one waker, so one task at a time may wait on it. A
/// writer and a flusher on the same endpoint must not be pending together.
struct Port<const N: usize> {
    tx: Ring<N>,
    rx: Ring<N>,
    pending_error: Option<Error>,
    /// Task waiting for received data or a receive error on this port.
    rx_waker: Option<Waker>,
    /// Task waiting for space in this port's TX queue.
    tx_waker: Option<Waker>,
}

impl<const N: usize> Port<N> {
    const fn new() -> Self {
        Self {
            tx: Ring::new(),
            rx: Ring::new(),
            pending_error: None,
            rx_waker: None,
            tx_waker: None,
        }
    }
}

/// Two UART endpoints wired back to back, with `N`-byte TX and RX queues per
/// endpoint. Bytes move from a TX queue to the peer's RX queue whenever an
/// endpoint operation runs, so the model is deterministic and needs no threads.
pub struct LoopbackUart<const N: usize> {
    capabilities: Capabilities,
    ports: Mutex<CriticalSectionRawMutex, [Port<N>; 2]>,
}

impl<const N: usize> LoopbackUart<N> {
    fn ports(&self) -> MutexGuard<'_, CriticalSectionRawMutex, [Port<N>; 2]> {
        self.ports
            .try_lock()
            .expect("UART loopback ports are already locked")
    }

    pub const fn new(capabilities: Capabilities) -> Self {
        const { assert!(N > 0, "loopback queues need a non-zero capacity") };
        Self {
            capabilities,
            ports: Mutex::new([const { Port::new() }; 2]),
        }
    }

    pub fn endpoints(&self) -> [LoopbackEndpoint<'_, N>; 2] {
        [0, 1].map(|side| LoopbackEndpoint {
            pair: self,
            side,
            config: Config::default(),
            nonblocking: false,
        })
    }

    /// Moves queued bytes across the wire and wakes any task that was waiting
    /// on a queue that changed. Wakers are taken out before waking so that no
    /// lock is held while user code runs.
    fn transfer(&self) {
        let wakers = {
            let mut ports = self.ports();
            let [first, second] = &mut *ports;
            let forward = move_bytes(&mut first.tx, &mut second.rx);
            let backward = move_bytes(&mut second.tx, &mut first.rx);
            [
                (forward > 0).then(|| second.rx_waker.take()).flatten(),
                (forward > 0).then(|| first.tx_waker.take()).flatten(),
                (backward > 0).then(|| first.rx_waker.take()).flatten(),
                (backward > 0).then(|| second.tx_waker.take()).flatten(),
            ]
        };
        for waker in wakers.into_iter().flatten() {
            waker.wake();
        }
    }
}

pub struct LoopbackEndpoint<'a, const N: usize> {
    pair: &'a LoopbackUart<N>,
    side: usize,
    config: Config,
    nonblocking: bool,
}

impl<const N: usize> LoopbackEndpoint<'_, N> {
    /// Queues a receive error for this endpoint. It is reported by the next
    /// read, before any buffered data. A second injection replaces the first.
    pub fn inject_rx_error(&mut self, error: Error) {
        let waker = {
            let mut ports = self.pair.ports();
            let port = &mut ports[self.side];
            port.pending_error = Some(error);
            port.rx_waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    fn try_read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        self.pair.transfer();
        let count = {
            let mut ports = self.pair.ports();
            let port = &mut ports[self.side];
            if let Some(error) = port.pending_error.take() {
                return Err(error);
            }
            let mut count = 0;
            while let Some(slot) = buf.get_mut(count) {
                let Some(byte) = port.rx.pop() else { break };
                *slot = byte;
                count += 1;
            }
            count
        };
        if count == 0 {
            return Err(Error::WouldBlock);
        }
        // Freed RX space lets the peer's queued bytes move and wakes its writer.
        self.pair.transfer();
        Ok(count)
    }

    fn try_write(&mut self, buf: &[u8]) -> Result<usize, Error> {
        self.pair.transfer();
        let count = {
            let mut ports = self.pair.ports();
            let tx = &mut ports[self.side].tx;
            let mut count = 0;
            for &byte in buf {
                if !tx.push(byte) {
                    break;
                }
                count += 1;
            }
            count
        };
        if count == 0 {
            return Err(Error::WouldBlock);
        }
        self.pair.transfer();
        Ok(count)
    }

    fn is_drained(&self) -> bool {
        self.pair.ports()[self.side].tx.len == 0
    }

    fn register_rx_waker(&self, waker: &Waker) {
        self.pair.ports()[self.side].rx_waker = Some(waker.clone());
    }

    fn register_tx_waker(&self, waker: &Waker) {
        self.pair.ports()[self.side].tx_waker = Some(waker.clone());
    }
}

impl<const N: usize> embedded_io::ErrorType for LoopbackEndpoint<'_, N> {
    type Error = Error;
}

impl<const N: usize> UartControl for LoopbackEndpoint<'_, N> {
    fn capabilities(&self) -> Capabilities {
        self.pair.capabilities
    }

    fn config(&self) -> Config {
        self.config
    }

    fn configure(&mut self, config: Config) -> Result<(), Error> {
        self.pair.capabilities.check(&config)?;
        self.config = config;
        Ok(())
    }

    fn is_nonblocking(&self) -> bool {
        self.nonblocking
    }

    /// The loopback has no time source, so an empty read or full write reports
    /// `WouldBlock` in both modes instead of waiting. Async methods wait.
    fn set_nonblocking(&mut self, nonblocking: bool) -> Result<(), Error> {
        self.nonblocking = nonblocking;
        Ok(())
    }

    fn flush_rx(&mut self) -> Result<(), Error> {
        {
            let mut ports = self.pair.ports();
            let port = &mut ports[self.side];
            port.rx.clear();
            port.pending_error = None;
        }
        self.pair.transfer();
        Ok(())
    }

    fn flush_tx(&mut self) -> Result<(), Error> {
        let waker = {
            let mut ports = self.pair.ports();
            let port = &mut ports[self.side];
            port.tx.clear();
            port.tx_waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
        Ok(())
    }
}

impl<const N: usize> embedded_io::Read for LoopbackEndpoint<'_, N> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.try_read(buf)
    }
}

impl<const N: usize> embedded_io::Write for LoopbackEndpoint<'_, N> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.try_write(buf)
    }

    /// Waits for transmission: fails with `WouldBlock` while bytes remain queued.
    fn flush(&mut self) -> Result<(), Self::Error> {
        self.pair.transfer();
        if self.is_drained() {
            Ok(())
        } else {
            Err(Error::WouldBlock)
        }
    }
}

impl<const N: usize> embedded_io::ReadReady for LoopbackEndpoint<'_, N> {
    fn read_ready(&mut self) -> Result<bool, Self::Error> {
        self.pair.transfer();
        let ports = self.pair.ports();
        let port = &ports[self.side];
        Ok(port.rx.len > 0 || port.pending_error.is_some())
    }
}

impl<const N: usize> embedded_io::WriteReady for LoopbackEndpoint<'_, N> {
    fn write_ready(&mut self) -> Result<bool, Self::Error> {
        self.pair.transfer();
        Ok(self.pair.ports()[self.side].tx.free() > 0)
    }
}

/// Async operations complete when the peer moves bytes or an error is
/// injected. The task waker is stored in the port, one per direction.
impl<const N: usize> embedded_io_async::Read for LoopbackEndpoint<'_, N> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        core::future::poll_fn(|cx| match self.try_read(buf) {
            Err(Error::WouldBlock) => {
                self.register_rx_waker(cx.waker());
                Poll::Pending
            }
            result => Poll::Ready(result),
        })
        .await
    }
}

impl<const N: usize> embedded_io_async::Write for LoopbackEndpoint<'_, N> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        core::future::poll_fn(|cx| match self.try_write(buf) {
            Err(Error::WouldBlock) => {
                self.register_tx_waker(cx.waker());
                Poll::Pending
            }
            result => Poll::Ready(result),
        })
        .await
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        core::future::poll_fn(|cx| {
            self.pair.transfer();
            if self.is_drained() {
                Poll::Ready(Ok(()))
            } else {
                self.register_tx_waker(cx.waker());
                Poll::Pending
            }
        })
        .await
    }
}

/// Simulated AT-command peer driven through embedded-io. It answers
/// CR- or LF-terminated lines, and only touches the port when the readiness
/// queries say an operation will not wait. Work happens only in `poll`, so a
/// test drives it step by step. Lines longer than `LINE` bytes are discarded
/// and answered with `ERROR`.
pub struct AtResponder<U, const LINE: usize> {
    port: U,
    line: [u8; LINE],
    line_len: usize,
    overflow: bool,
    response: Option<&'static [u8]>,
    sent: usize,
}

impl<U, const LINE: usize> AtResponder<U, LINE>
where
    U: embedded_io::Read + embedded_io::Write + embedded_io::ReadReady + embedded_io::WriteReady,
{
    pub const fn new(port: U) -> Self {
        const { assert!(LINE > 0, "AT line buffer needs a non-zero capacity") };
        Self {
            port,
            line: [0; LINE],
            line_len: 0,
            overflow: false,
            response: None,
            sent: 0,
        }
    }

    /// Makes as much progress as the port allows: finishes a pending response,
    /// then consumes input until the port has none or a response is pending.
    /// Errors reported by the port are returned to the caller.
    pub fn poll(&mut self) -> Result<(), U::Error> {
        loop {
            self.flush_response()?;
            if self.response.is_some() || !self.port.read_ready()? {
                return Ok(());
            }
            let mut byte = [0u8; 1];
            self.port.read(&mut byte)?;
            self.handle_byte(byte[0]);
        }
    }

    fn flush_response(&mut self) -> Result<(), U::Error> {
        let Some(response) = self.response else {
            return Ok(());
        };
        if !self.port.write_ready()? {
            return Ok(());
        }
        self.sent += self.port.write(&response[self.sent..])?;
        if self.sent == response.len() {
            self.response = None;
            self.sent = 0;
        }
        Ok(())
    }

    fn handle_byte(&mut self, byte: u8) {
        match byte {
            b'\r' | b'\n' => {
                if self.overflow {
                    self.overflow = false;
                    self.respond(b"ERROR\r\n");
                } else if self.line_len > 0 {
                    let response = dispatch(&self.line[..self.line_len]);
                    self.respond(response);
                }
                self.line_len = 0;
            }
            _ if self.overflow => {}
            _ if self.line_len == LINE => {
                self.overflow = true;
                self.line_len = 0;
            }
            _ => {
                self.line[self.line_len] = byte;
                self.line_len += 1;
            }
        }
    }

    fn respond(&mut self, response: &'static [u8]) {
        self.response = Some(response);
        self.sent = 0;
    }
}

fn dispatch(line: &[u8]) -> &'static [u8] {
    match line {
        b"AT" => b"OK\r\n",
        b"AT+VERSION?" => b"+VERSION:1\r\nOK\r\n",
        _ => b"ERROR\r\n",
    }
}

#[cfg(all(test, feature = "mock"))]
mod tests {
    use core::cell::Cell;
    use core::pin::pin;
    use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    use embedded_io::{Read as _, ReadReady as _, Write as _, WriteReady as _};

    use crate::uart::{
        AtResponder, Capabilities, Config, DataBits, Error, FlowControl, LoopbackEndpoint,
        LoopbackUart, Parity, StopBits, UartControl,
    };

    const CAPABILITIES: Capabilities = Capabilities {
        min_baud_rate: 9_600,
        max_baud_rate: 921_600,
        data_five: true,
        data_six: true,
        data_seven: true,
        data_eight: true,
        stop_two: true,
        parity_even: true,
        parity_odd: true,
        rts_cts: true,
        xon_xoff: false,
    };

    fn pair() -> LoopbackUart<4> {
        LoopbackUart::new(CAPABILITIES)
    }

    /// Feeds `request` to `peer` and collects `reply` from `host`, polling the
    /// peer between chunks. The queues are four bytes, so a single burst of
    /// either side cannot fit. Only ready operations are attempted.
    fn exchange<const LINE: usize>(
        host: &mut LoopbackEndpoint<'_, 4>,
        peer: &mut AtResponder<LoopbackEndpoint<'_, 4>, LINE>,
        request: &[u8],
        reply: &mut [u8],
    ) {
        let mut sent = 0;
        let mut received = 0;
        for _ in 0..1_000 {
            if sent < request.len() && host.write_ready().unwrap() {
                sent += host.write(&request[sent..]).unwrap();
            }
            peer.poll().unwrap();
            if received < reply.len() && host.read_ready().unwrap() {
                received += host.read(&mut reply[received..]).unwrap();
            }
            if sent == request.len() && received == reply.len() {
                return;
            }
        }
        panic!("exchange did not complete");
    }

    #[test]
    fn write_on_one_endpoint_is_read_on_its_pair() {
        let pair = pair();
        let [mut a, mut b] = pair.endpoints();
        assert_eq!(a.write(b"hi").unwrap(), 2);
        let mut buf = [0u8; 8];
        assert_eq!(b.read(&mut buf).unwrap(), 2);
        assert_eq!(&buf[..2], b"hi");
    }

    #[test]
    fn nonblocking_read_on_empty_buffer_reports_would_block() {
        let pair = pair();
        let [mut a, _b] = pair.endpoints();
        a.set_nonblocking(true).unwrap();
        assert!(a.is_nonblocking());
        let mut buf = [0u8; 4];
        assert_eq!(a.read(&mut buf), Err(Error::WouldBlock));
    }

    #[test]
    fn readiness_follows_queues() {
        let pair = pair();
        let [mut a, mut b] = pair.endpoints();
        assert!(!b.read_ready().unwrap());
        assert!(a.write_ready().unwrap());
        a.write(b"x").unwrap();
        assert!(b.read_ready().unwrap());
        assert!(!a.read_ready().unwrap());
    }

    #[test]
    fn full_peer_queue_stops_writes_until_it_is_read() {
        let pair = pair();
        let [mut a, mut b] = pair.endpoints();
        // Peer RX (4) plus local TX (4) hold eight bytes in total.
        assert_eq!(a.write(b"abcd").unwrap(), 4);
        assert_eq!(a.write(b"efgh").unwrap(), 4);
        assert!(!a.write_ready().unwrap());
        assert_eq!(a.write(b"i"), Err(Error::WouldBlock));
        let mut buf = [0u8; 2];
        assert_eq!(b.read(&mut buf).unwrap(), 2);
        assert!(a.write_ready().unwrap());
    }

    #[test]
    fn flush_rx_discards_pending_input() {
        let pair = pair();
        let [mut a, mut b] = pair.endpoints();
        a.write(b"abc").unwrap();
        assert!(b.read_ready().unwrap());
        b.flush_rx().unwrap();
        assert!(!b.read_ready().unwrap());
        let mut buf = [0u8; 4];
        assert_eq!(b.read(&mut buf), Err(Error::WouldBlock));
    }

    #[test]
    fn flush_tx_discards_queued_output() {
        let pair = pair();
        let [mut a, mut b] = pair.endpoints();
        a.write(b"abcd").unwrap();
        assert_eq!(a.write(b"efgh").unwrap(), 4);
        a.flush_tx().unwrap();
        assert!(a.flush().is_ok());
        let mut buf = [0u8; 8];
        assert_eq!(b.read(&mut buf).unwrap(), 4);
        assert_eq!(&buf[..4], b"abcd");
    }

    #[test]
    fn flush_waits_until_queued_output_is_transmitted() {
        let pair = pair();
        let [mut a, mut b] = pair.endpoints();
        a.write(b"abcd").unwrap();
        assert_eq!(a.write(b"efgh").unwrap(), 4);
        assert_eq!(a.flush(), Err(Error::WouldBlock));
        let mut buf = [0u8; 4];
        assert_eq!(b.read(&mut buf).unwrap(), 4);
        assert!(a.flush().is_ok());
        assert_eq!(b.read(&mut buf).unwrap(), 4);
        assert_eq!(&buf, b"efgh");
    }

    #[test]
    fn unsupported_settings_are_rejected_and_config_is_kept() {
        let pair = LoopbackUart::<4>::new(Capabilities {
            data_seven: false,
            parity_odd: false,
            rts_cts: false,
            ..CAPABILITIES
        });
        let [mut a, _b] = pair.endpoints();
        let base = a.config();

        let seven = Config {
            data_bits: DataBits::Seven,
            ..base
        };
        assert_eq!(a.configure(seven), Err(Error::UnsupportedDataBits));

        let odd = Config {
            parity: Parity::Odd,
            ..base
        };
        assert_eq!(a.configure(odd), Err(Error::UnsupportedParity));

        let flow = Config {
            flow_control: FlowControl::RtsCts,
            ..base
        };
        assert_eq!(a.configure(flow), Err(Error::UnsupportedFlowControl));

        let xon = Config {
            flow_control: FlowControl::XonXoff,
            ..base
        };
        assert_eq!(a.configure(xon), Err(Error::UnsupportedFlowControl));

        let slow = Config {
            baud_rate: 300,
            ..base
        };
        assert_eq!(a.configure(slow), Err(Error::UnsupportedBaudRate));

        assert_eq!(a.config(), base);

        let supported = Config::new(
            57_600,
            DataBits::Eight,
            StopBits::Two,
            Parity::Even,
            FlowControl::None,
        );
        a.configure(supported).unwrap();
        assert_eq!(a.config(), supported);
    }

    #[test]
    fn injected_framing_and_overrun_reach_the_reader() {
        let pair = pair();
        let [mut a, mut b] = pair.endpoints();
        a.write(b"x").unwrap();
        b.inject_rx_error(Error::Framing);
        let mut buf = [0u8; 4];
        assert_eq!(b.read(&mut buf), Err(Error::Framing));
        // The error is consumed; the byte received after it is still readable.
        assert_eq!(b.read(&mut buf).unwrap(), 1);
        b.inject_rx_error(Error::Overrun);
        assert!(b.read_ready().unwrap());
        assert_eq!(b.read(&mut buf), Err(Error::Overrun));
    }

    #[test]
    fn embedded_io_write_all_and_read_exact_round_trip() {
        let pair = pair();
        let [mut a, mut b] = pair.endpoints();
        a.write_all(b"abcd").unwrap();
        a.flush().unwrap();
        let mut buf = [0u8; 4];
        b.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"abcd");
        assert!(!b.read_ready().unwrap());
        assert!(a.write_ready().unwrap());
    }

    #[test]
    fn embedded_io_surfaces_line_errors_with_kind() {
        let pair = pair();
        let [mut a, mut b] = pair.endpoints();
        a.write_all(b"z").unwrap();
        b.inject_rx_error(Error::ParityError);
        let mut buf = [0u8; 1];
        match b.read_exact(&mut buf) {
            Err(embedded_io::ReadExactError::Other(error)) => {
                assert_eq!(error, Error::ParityError);
                assert_eq!(
                    embedded_io::Error::kind(&error),
                    embedded_io::ErrorKind::InvalidData
                );
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn at_responder_answers_lines_through_embedded_io() {
        let pair = pair();
        let [mut host, peer] = pair.endpoints();
        let mut peer = AtResponder::<_, 16>::new(peer);

        let mut reply = [0u8; 4];
        exchange(&mut host, &mut peer, b"AT\r\n", &mut reply);
        assert_eq!(&reply, b"OK\r\n");

        let mut reply = [0u8; 7];
        exchange(&mut host, &mut peer, b"AT+NOPE\n", &mut reply);
        assert_eq!(&reply, b"ERROR\r\n");
    }

    #[test]
    fn at_responder_answers_multi_line_reply_across_polls() {
        let pair = pair();
        let [mut host, peer] = pair.endpoints();
        let mut peer = AtResponder::<_, 16>::new(peer);

        // The 16-byte reply is four times the queue size, so it needs several polls.
        let mut reply = [0u8; 16];
        exchange(&mut host, &mut peer, b"AT+VERSION?\r\n", &mut reply);
        assert_eq!(&reply, b"+VERSION:1\r\nOK\r\n");
    }

    #[test]
    fn at_responder_rejects_overlong_lines() {
        let pair = pair();
        let [mut host, peer] = pair.endpoints();
        let mut peer = AtResponder::<_, 4>::new(peer);

        let mut reply = [0u8; 7];
        exchange(&mut host, &mut peer, b"AT+VERSION?\r\n", &mut reply);
        assert_eq!(&reply, b"ERROR\r\n");
    }

    #[test]
    fn at_responder_surfaces_injected_peer_errors() {
        let pair = pair();
        let [_host, mut peer_end] = pair.endpoints();
        peer_end.inject_rx_error(Error::Overrun);
        let mut peer = AtResponder::<_, 8>::new(peer_end);
        assert_eq!(peer.poll(), Err(Error::Overrun));
    }

    /// Counts wake-ups so that the async test can check that data arrival
    /// wakes a pending reader.
    fn counting_waker(counter: &Cell<usize>) -> Waker {
        fn clone(data: *const ()) -> RawWaker {
            RawWaker::new(data, &VTABLE)
        }
        fn wake(data: *const ()) {
            let counter = unsafe { &*data.cast::<Cell<usize>>() };
            counter.set(counter.get() + 1);
        }
        fn noop(_: *const ()) {}
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake, noop);
        let data = core::ptr::from_ref(counter).cast::<()>();
        // SAFETY: the counter outlives every use of the waker in these tests,
        // and all vtable functions only read the counter.
        unsafe { Waker::from_raw(RawWaker::new(data, &VTABLE)) }
    }

    #[test]
    fn async_read_waits_for_peer_data_and_is_woken() {
        use embedded_io_async::{Read as AsyncRead, Write as AsyncWrite};

        let wakes = Cell::new(0);
        let waker = counting_waker(&wakes);
        let mut cx = Context::from_waker(&waker);

        let pair = pair();
        let [mut a, mut b] = pair.endpoints();
        let mut buf = [0u8; 4];
        let received = {
            let mut read = pin!(AsyncRead::read(&mut b, &mut buf));
            assert!(read.as_mut().poll(&mut cx).is_pending());

            let mut write = pin!(AsyncWrite::write(&mut a, b"ok"));
            match write.as_mut().poll(&mut cx) {
                Poll::Ready(result) => assert_eq!(result.unwrap(), 2),
                Poll::Pending => panic!("write should not wait"),
            }
            assert_eq!(wakes.get(), 1);

            match read.as_mut().poll(&mut cx) {
                Poll::Ready(result) => result.unwrap(),
                Poll::Pending => panic!("read should complete"),
            }
        };
        assert_eq!(received, 2);
        assert_eq!(&buf[..2], b"ok");
    }

    #[test]
    fn async_write_waits_for_space_and_is_woken_by_peer_read() {
        use embedded_io_async::Write as AsyncWrite;

        let wakes = Cell::new(0);
        let waker = counting_waker(&wakes);
        let mut cx = Context::from_waker(&waker);

        let pair = pair();
        let [mut a, mut b] = pair.endpoints();
        embedded_io::Write::write(&mut a, b"abcd").unwrap();
        embedded_io::Write::write(&mut a, b"efgh").unwrap();

        let mut write = pin!(AsyncWrite::write(&mut a, b"i"));
        assert!(write.as_mut().poll(&mut cx).is_pending());

        let mut buf = [0u8; 4];
        assert_eq!(embedded_io::Read::read(&mut b, &mut buf).unwrap(), 4);
        assert_eq!(wakes.get(), 1);
        match write.as_mut().poll(&mut cx) {
            Poll::Ready(result) => assert_eq!(result.unwrap(), 1),
            Poll::Pending => panic!("write should complete after the peer read"),
        }
    }

    #[test]
    fn async_flush_waits_for_drain_and_is_woken_by_peer_read() {
        use embedded_io_async::Write as AsyncWrite;

        let wakes = Cell::new(0);
        let waker = counting_waker(&wakes);
        let mut cx = Context::from_waker(&waker);

        let pair = pair();
        let [mut a, mut b] = pair.endpoints();
        embedded_io::Write::write(&mut a, b"abcd").unwrap();
        embedded_io::Write::write(&mut a, b"efgh").unwrap();

        let mut flush = pin!(AsyncWrite::flush(&mut a));
        assert!(flush.as_mut().poll(&mut cx).is_pending());

        let mut buf = [0u8; 4];
        assert_eq!(embedded_io::Read::read(&mut b, &mut buf).unwrap(), 4);
        assert_eq!(wakes.get(), 1);
        assert!(flush.as_mut().poll(&mut cx).is_ready());
    }
}
