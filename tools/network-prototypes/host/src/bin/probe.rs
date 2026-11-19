//! Client-only observation probe. Start local fixtures with ../fixtures.py.
use futures_lite::future::block_on;
use network::{IpAddress, Port};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};
use xila_host_network_prototype::{HostProvider, Resolver};
struct Tracking;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Tracking {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe {
            System.dealloc(p, layout);
        }
    }
}
#[global_allocator]
static ALLOCATOR: Tracking = Tracking;
fn threads() -> usize {
    std::fs::read_dir("/proc/self/task")
        .map(|d| d.count())
        .unwrap_or(0)
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: probe TCP_PORT UDP_PORT (localhost fixtures)");
        std::process::exit(2);
    }
    let tcp = args[1].parse().unwrap();
    let udp = args[2].parse().unwrap();
    let baseline = LIVE.load(Ordering::Relaxed);
    let before_threads = threads();
    let started = Instant::now();
    block_on(async {
        let provider = HostProvider::new(8).unwrap();
        let address = IpAddress::new_ipv4([127, 0, 0, 1]);
        let mut stream = provider
            .connect(address.clone(), Port::from_inner(tcp))
            .await
            .unwrap();
        let datagram = provider
            .bind_udp(address.clone(), Port::from_inner(0))
            .unwrap();
        let payload = b"Xila network probe!!!";
        let mut bytes = [0; 21];
        // Warm up lazy reactor storage before the measured steady-state run.
        stream.write_all(payload).await.unwrap();
        let mut n = 0;
        while n < bytes.len() {
            let got = stream.read(&mut bytes[n..]).await.unwrap();
            assert!(got > 0, "peer closed during warm-up");
            n += got;
        }
        let allocs = ALLOCS.load(Ordering::Relaxed);
        let live = LIVE.load(Ordering::Relaxed);
        PEAK.store(live, Ordering::Relaxed);
        let operations = Instant::now();
        for _ in 0..100 {
            stream.write_all(payload).await.unwrap();
            let mut n = 0;
            while n < bytes.len() {
                let got = stream.read(&mut bytes[n..]).await.unwrap();
                assert!(got > 0);
                n += got;
            }
            assert_eq!(&bytes, payload);
            datagram
                .write_to(payload, address.clone(), Port::from_inner(udp))
                .await
                .unwrap();
            let (n, _, _) = datagram.read_from(&mut bytes).await.unwrap();
            assert_eq!(&bytes[..n], payload);
        }
        let elapsed = operations.elapsed();
        let operation_allocs = ALLOCS.load(Ordering::Relaxed) - allocs;
        let operation_peak = PEAK.load(Ordering::Relaxed).saturating_sub(live);
        let provider_live = LIVE.load(Ordering::Relaxed).saturating_sub(baseline);
        println!(
            "TCP+UDP: 100 pairs x21 bytes, {elapsed:?}; steady allocations={operation_allocs}; extra peak={operation_peak}; client Rust live delta={provider_live}; reactor threads delta={}",
            threads().saturating_sub(before_threads)
        );
        drop(stream);
        drop(datagram);
        assert_eq!(provider.active_sockets(), 0);
        let resolver = Resolver::new(2).unwrap();
        let dns = resolver.lookup("localhost").await.unwrap();
        assert!(!dns.is_empty());
        println!(
            "DNS localhost: {} addresses; optional resolver stack reservation=131072 bytes; threads now={}; total elapsed={:?}",
            dns.len(),
            threads(),
            started.elapsed()
        );
    });
}
