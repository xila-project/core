use core::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, signal::Signal};
use stackful::{Coroutine, Stack, suspend_on};
use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct Wake(AtomicUsize);
impl std::task::Wake for Wake {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}
fn coroutine(entry: impl FnOnce() -> i32 + 'static) -> Coroutine {
    // These bounded test entries fit well within 64 KiB and never unwind.
    unsafe { Coroutine::new(Stack::new(64 * 1024), entry) }
}
#[test]
fn signal_uses_real_waker_and_does_not_spin() {
    let signal = Rc::new(Signal::<NoopRawMutex, i32>::new());
    let polls = Rc::new(Cell::new(0));
    let (s, p) = (signal.clone(), polls.clone());
    let mut c = pin!(coroutine(move || {
        let mut wait = pin!(s.wait());
        suspend_on(core::future::poll_fn(|cx| {
            p.set(p.get() + 1);
            wait.as_mut().poll(cx)
        }))
    }));
    let wake = Arc::new(Wake(AtomicUsize::new(0)));
    let waker = Waker::from(wake.clone());
    let mut cx = Context::from_waker(&waker);
    assert_eq!(c.as_mut().poll(&mut cx), Poll::Pending);
    assert_eq!(polls.get(), 1);
    assert_eq!(wake.0.load(Ordering::Relaxed), 0);
    signal.signal(42);
    assert_eq!(wake.0.load(Ordering::Relaxed), 1);
    assert_eq!(c.as_mut().poll(&mut cx), Poll::Ready(42));
    assert_eq!(polls.get(), 2);
}
#[test]
fn interleaving_nested_calls_and_join() {
    let mut drivers = Vec::new();
    let mut joins = Vec::new();
    for n in 0..128 {
        let c = coroutine(move || {
            fn nested(n: i32) -> i32 {
                suspend_on(embassy_futures::yield_now());
                suspend_on(async { n })
            }
            nested(n)
        });
        joins.push(c.join_handle());
        drivers.push(c);
    }
    let mut cx = Context::from_waker(Waker::noop());
    for c in &mut drivers {
        assert_eq!(std::pin::Pin::new(c).poll(&mut cx), Poll::Pending);
    }
    for (n, c) in drivers.iter_mut().enumerate() {
        assert_eq!(std::pin::Pin::new(c).poll(&mut cx), Poll::Ready(n as i32));
    }
    for (n, join) in joins.into_iter().enumerate() {
        assert_eq!(embassy_futures::block_on(join), Ok(n as i32));
    }
    let child = coroutine(|| {
        suspend_on(embassy_futures::yield_now());
        73
    });
    let join = child.join_handle();
    let parent = coroutine(move || suspend_on(join).unwrap());
    let (a, b) = embassy_futures::block_on(embassy_futures::join::join(child, parent));
    assert_eq!((a, b), (73, 73));
}
#[test]
fn fallback_and_thread_isolation() {
    assert_eq!(suspend_on(async { 11 }), 11);
    let threads: Vec<_> = (0..4)
        .map(|n| {
            std::thread::spawn(move || {
                let c = coroutine(move || {
                    suspend_on(embassy_futures::yield_now());
                    n
                });
                assert_eq!(embassy_futures::block_on(c), n);
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
}

#[test]
fn cancellation_and_unstarted_drop() {
    let drops = Rc::new(Cell::new(0));
    struct DropCount(Rc<Cell<usize>>);
    impl Drop for DropCount {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let d = DropCount(drops.clone());
    drop(coroutine(move || {
        drop(d);
        0
    }));
    assert_eq!(drops.get(), 1);
    let mut c = coroutine(|| {
        suspend_on(core::future::pending::<()>());
        0
    });
    let join = c.join_handle();
    let mut cx = Context::from_waker(Waker::noop());
    assert_eq!(std::pin::Pin::new(&mut c).poll(&mut cx), Poll::Pending);
    drop(c);
    assert_eq!(embassy_futures::block_on(join), Err(stackful::Cancelled));
}

#[test]
fn footprint_and_switch_timing() {
    const N: usize = 100_000;
    let c = coroutine(|| {
        for _ in 0..N {
            suspend_on(embassy_futures::yield_now());
        }
        0
    });
    let start = std::time::Instant::now();
    assert_eq!(embassy_futures::block_on(c), 0);
    println!(
        "Coroutine={} JoinHandle={} Stack={} control allocation={} bytes; {:.1} ns per suspend/resume round trip (driver, TLS and guard checks included)",
        size_of::<Coroutine>(),
        size_of::<stackful::JoinHandle>(),
        size_of::<Stack>(),
        stackful::control_allocation_size(),
        start.elapsed().as_nanos() as f64 / N as f64
    );
}

#[task::test]
async fn embassy_tasks_wake_native_waiters() {
    let manager = task::test_manager();
    let parent = manager.get_current_task_identifier().await;
    let spawner = manager.get_spawner(parent).await.unwrap();
    let mut handles = Vec::new();
    let signal = Rc::new(Signal::<NoopRawMutex, i32>::new());
    let received = Rc::new(Cell::new(false));
    let (s, r) = (signal.clone(), received.clone());
    let (waiter, _) = unsafe {
        stackful::spawn(
            manager,
            parent,
            "native waiter",
            Stack::new(64 * 1024),
            move || {
                let result = suspend_on(s.wait());
                r.set(true);
                result
            },
        )
        .await
        .unwrap()
    };
    for n in 0..64 {
        handles.push(unsafe {
            stackful::spawn(
                manager,
                parent,
                "interleaved",
                Stack::new(64 * 1024),
                move || {
                    suspend_on(task::yield_now());
                    n
                },
            )
            .await
            .unwrap()
            .0
        });
    }
    let (sender, _) = manager
        .spawn(parent, "sender", Some(spawner), move |_| async move {
            task::yield_now().await;
            assert!(!received.get());
            signal.signal(91);
        })
        .await
        .unwrap();
    assert_eq!(waiter.join().await, 91);
    sender.join().await;
    for (n, h) in handles.into_iter().enumerate() {
        assert_eq!(h.join().await, n as i32);
    }
}

struct CountingAllocator;
std::thread_local! { static ALLOCATIONS: Cell<usize> = const { Cell::new(0) }; }
unsafe impl std::alloc::GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        ALLOCATIONS.with(|n| n.set(n.get() + 1));
        unsafe { std::alloc::System.alloc(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: std::alloc::Layout) {
        unsafe { std::alloc::System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;
#[test]
fn allocation_budget() {
    let before = ALLOCATIONS.with(Cell::get);
    let mut c = coroutine(|| {
        for _ in 0..100 {
            suspend_on(embassy_futures::yield_now());
        }
        17
    });
    let join = c.join_handle();
    assert_eq!(ALLOCATIONS.with(Cell::get) - before, 2);
    let before = ALLOCATIONS.with(Cell::get);
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..100 {
        assert_eq!(std::pin::Pin::new(&mut c).poll(&mut cx), Poll::Pending);
    }
    assert_eq!(std::pin::Pin::new(&mut c).poll(&mut cx), Poll::Ready(17));
    assert_eq!(embassy_futures::block_on(join), Ok(17));
    assert_eq!(ALLOCATIONS.with(Cell::get), before);
}

#[cfg(target_arch = "x86_64")]
#[test]
fn floating_point_control_is_local() {
    fn read() -> u32 {
        let mut value = 0;
        unsafe {
            core::arch::asm!("stmxcsr [{p}]", p = in(reg) &mut value, options(nostack));
        }
        value
    }
    fn write(value: u32) {
        unsafe {
            core::arch::asm!("ldmxcsr [{p}]", p = in(reg) &value, options(nostack));
        }
    }
    let original = read();
    let mode = (original & !0x6000) | 0x2000;
    let mut c = coroutine(move || {
        write(mode);
        suspend_on(embassy_futures::yield_now());
        i32::from(read() == mode)
    });
    let mut cx = Context::from_waker(Waker::noop());
    assert_eq!(std::pin::Pin::new(&mut c).poll(&mut cx), Poll::Pending);
    assert_eq!(read(), original);
    assert_eq!(std::pin::Pin::new(&mut c).poll(&mut cx), Poll::Ready(1));
    assert_eq!(read(), original);
}
