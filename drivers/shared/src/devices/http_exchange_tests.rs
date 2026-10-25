extern crate std;
use super::*;
use core::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};
use std::vec::Vec;

struct Input<'a> {
    wire: &'a [u8],
    fragment: usize,
}
impl embedded_io::ErrorType for Input<'_> {
    type Error = embedded_io::ErrorKind;
}
impl embedded_io_async::Read for Input<'_> {
    async fn read(&mut self, out: &mut [u8]) -> core::result::Result<usize, Self::Error> {
        let n = out.len().min(self.wire.len()).min(self.fragment);
        out[..n].copy_from_slice(&self.wire[..n]);
        self.wire = &self.wire[n..];
        Ok(n)
    }
}

fn replay(wire: &[u8], fragment: usize, read_size: usize) -> Result<Vec<u8>> {
    let mut context = ClientContext::new();
    let generation = context.exchange.begin().unwrap();
    let exchange = context.exchange.clone();
    let mut input = Input { wire, fragment };
    let producer = async {
        let mut buffer = exchange.take_buffer(generation).await.unwrap();
        let result = read_response(&mut input, &exchange, generation, &mut buffer, false).await;
        exchange.finish(generation, buffer, result).await;
    };
    let mut producer = pin!(producer);
    let mut cx = Context::from_waker(Waker::noop());
    let mut result = Vec::new();
    let mut out = std::vec![0; read_size];
    let mut done = false;
    for _ in 0..wire.len() * 4 + 100 {
        if !done {
            done = producer.as_mut().poll(&mut cx).is_ready();
        }
        match context.poll_read(&mut cx, &mut out) {
            Poll::Ready(Ok(0)) if done => return Ok(result),
            Poll::Ready(Ok(n)) => result.extend_from_slice(&out[..n]),
            Poll::Ready(Err(e)) => return Err(e),
            Poll::Pending => {}
        }
    }
    panic!("producer/reader stalled")
}

#[test]
fn fragmented_stream_and_small_reads() {
    let head = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
    let wire = [head.as_slice(), b"5\r\nhello\r\n0\r\n\r\n"].concat();
    for fragment in 1..=wire.len() {
        assert_eq!(
            replay(&wire, fragment, 3).unwrap(),
            [head.as_slice(), b"hello"].concat()
        );
    }
}
#[test]
fn premature_eof_and_header_limit() {
    assert_eq!(
        replay(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\na", 10, 8),
        Err(Error::InputOutput)
    );
    let wire = [
        b"HTTP/1.1 200 OK\r\nX: ".as_slice(),
        &std::vec![b'x'; RESPONSE_BUFFER_SIZE],
        b"\r\n\r\n",
    ]
    .concat();
    assert_eq!(replay(&wire, 10, 8), Err(Error::FileTooLarge));
}
#[test]
fn close_retires_generation_and_wakes_producer() {
    let mut context = ClientContext::new();
    let old = context.exchange.begin().unwrap();
    task::block_on(context.exchange.close());
    assert_eq!(context.exchange.begin(), Err(Error::RessourceBusy));
    task::block_on(
        context
            .exchange
            .finish(old, None, Err(Error::InvalidContext)),
    );
    let new = context.exchange.begin().unwrap();
    assert_ne!(old, new);
    task::block_on(context.exchange.finish(old, None, Err(Error::InputOutput)));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(context.poll_read(&mut cx, &mut [0; 8]).is_pending());
    task::block_on(context.exchange.close());
}

#[test]
fn producer_is_woken_by_drain_and_pending_reader_by_completion() {
    use std::sync::{
        Arc as StdArc,
        atomic::{AtomicUsize, Ordering},
    };
    struct Count(AtomicUsize);
    impl std::task::Wake for Count {
        fn wake(self: StdArc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    let mut client = ClientContext::new();
    let generation = client.exchange.begin().unwrap();
    let wakes = StdArc::new(Count(AtomicUsize::new(0)));
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    assert!(client.poll_read(&mut cx, &mut [0; 8]).is_pending());
    task::block_on(
        client
            .exchange
            .finish(generation, None, Err(Error::RessourceBusy)),
    );
    assert!(wakes.0.load(Ordering::Relaxed) > 0);
    assert_eq!(
        client.poll_read(&mut cx, &mut [0; 8]),
        Poll::Ready(Err(Error::RessourceBusy))
    );
}

mod memory {
    use super::*;
    use std::{
        alloc::{GlobalAlloc, Layout, System},
        cell::Cell,
    };
    thread_local! { static COUNTERS: Cell<Option<(usize, usize)>> = const { Cell::new(None) }; }
    struct Allocator;
    unsafe impl GlobalAlloc for Allocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let _ = COUNTERS.try_with(|c| {
                if let Some((live, peak)) = c.get() {
                    let live = live + layout.size();
                    c.set(Some((live, peak.max(live))));
                }
            });
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            let _ = COUNTERS.try_with(|c| {
                if let Some((live, peak)) = c.get() {
                    c.set(Some((live.saturating_sub(layout.size()), peak)));
                }
            });
            unsafe { System.dealloc(ptr, layout) }
        }
    }
    #[global_allocator]
    static ALLOCATOR: Allocator = Allocator;
    struct Generated {
        header: &'static [u8],
        remaining: usize,
    }
    impl embedded_io::ErrorType for Generated {
        type Error = embedded_io::ErrorKind;
    }
    impl embedded_io_async::Read for Generated {
        async fn read(&mut self, out: &mut [u8]) -> core::result::Result<usize, Self::Error> {
            if !self.header.is_empty() {
                let n = self.header.len().min(out.len());
                out[..n].copy_from_slice(&self.header[..n]);
                self.header = &self.header[n..];
                return Ok(n);
            }
            let n = self.remaining.min(out.len());
            out[..n].fill(b'x');
            self.remaining -= n;
            Ok(n)
        }
    }
    fn download(size: usize) -> usize {
        COUNTERS.set(Some((0, 0)));
        let mut client = ClientContext::new();
        let generation = client.exchange.begin().unwrap();
        let exchange = client.exchange.clone();
        let mut input = Generated {
            header: b"HTTP/1.0 200 OK\r\n\r\n",
            remaining: size,
        };
        let producer = async {
            let mut storage = exchange.take_buffer(generation).await.unwrap();
            let result =
                read_response(&mut input, &exchange, generation, &mut storage, false).await;
            exchange.finish(generation, storage, result).await;
        };
        let mut producer = pin!(producer);
        let mut cx = Context::from_waker(Waker::noop());
        let mut bytes = 0;
        let mut done = false;
        let mut out = [0; 127];
        loop {
            if !done {
                done = producer.as_mut().poll(&mut cx).is_ready();
            }
            if let Poll::Ready(result) = client.poll_read(&mut cx, &mut out) {
                let n = result.unwrap();
                if n == 0 && done {
                    break;
                }
                bytes += n;
            }
        }
        assert_eq!(bytes, size + 19);
        COUNTERS.replace(None).unwrap().1
    }
    #[test]
    fn large_download_has_constant_peak_heap() {
        let small = download(4096);
        let large = download(4 * 1024 * 1024);
        assert_eq!(small, large);
        assert!(large < 4096, "exchange peak: {large}");
        std::println!("HTTP exchange heap peak: {small} bytes (4 KiB and 4 MiB bodies)");
    }
}

#[test]
fn ready_result_does_not_require_a_waiter_slot() {
    let mut client = ClientContext::new();
    let generation = client.exchange.begin().unwrap();
    let mut clones: Vec<_> = (0..16).map(|_| client.clone_context()).collect();
    let mut cx = Context::from_waker(Waker::noop());
    for clone in &mut clones {
        assert!(clone.poll_read(&mut cx, &mut [0; 8]).is_pending());
    }
    task::block_on(
        client
            .exchange
            .finish(generation, None, Err(Error::InputOutput)),
    );
    assert_eq!(
        client.poll_read(&mut cx, &mut [0; 8]),
        Poll::Ready(Err(Error::InputOutput))
    );
    for clone in &mut clones {
        clone.cancel_read();
    }
}

#[test]
fn cancelling_receive_or_publication_returns_the_same_buffer() {
    struct Pending;
    impl embedded_io::ErrorType for Pending {
        type Error = embedded_io::ErrorKind;
    }
    impl embedded_io_async::Read for Pending {
        async fn read(&mut self, _: &mut [u8]) -> core::result::Result<usize, Self::Error> {
            core::future::pending().await
        }
    }
    fn cancel<R: embedded_io_async::Read>(mut input: R) {
        let client = ClientContext::new();
        let original = task::block_on(client.exchange.inner.lock())
            .buffer
            .as_ref()
            .unwrap()
            .as_ptr();
        let generation = client.exchange.begin().unwrap();
        let producer = async {
            let mut buffer = client.exchange.take_buffer(generation).await.unwrap();
            let result = match select(
                read_response(&mut input, &client.exchange, generation, &mut buffer, false),
                client.exchange.cancelled.wait(),
            )
            .await
            {
                Either::First(result) => result,
                Either::Second(()) => Err(Error::InvalidContext),
            };
            client.exchange.finish(generation, buffer, result).await;
        };
        let mut producer = pin!(producer);
        let mut cx = Context::from_waker(Waker::noop());
        assert!(producer.as_mut().poll(&mut cx).is_pending());
        task::block_on(client.exchange.close());
        assert!(producer.as_mut().poll(&mut cx).is_ready());
        assert_eq!(
            task::block_on(client.exchange.inner.lock())
                .buffer
                .as_ref()
                .unwrap()
                .as_ptr(),
            original
        );
        assert!(client.exchange.begin().is_ok());
    }
    cancel(Pending);
    cancel(Input {
        wire: b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nbody",
        fragment: 128,
    });
}
