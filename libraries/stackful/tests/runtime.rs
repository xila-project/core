#![allow(
    clippy::disallowed_types,
    reason = "host-only test implements std::task::Wake with std::sync::Arc and std atomics"
)]

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
fn cancellation_drops_an_opted_in_future_once() {
    struct DropCount(Rc<Cell<usize>>);

    impl Drop for DropCount {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    let drops = Rc::new(Cell::new(0));
    let resource = DropCount(drops.clone());
    let mut driver = coroutine(move || {
        stackful::suspend_on_cancellable(async move {
            core::future::pending::<()>().await;
            drop(resource);
        });
        0
    });
    assert_eq!(
        driver.resume(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    );
    assert_eq!(drops.get(), 0);
    drop(driver);
    assert_eq!(drops.get(), 1);
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

#[test]
fn floating_point_values_survive_stack_switches() {
    let mut drivers = Vec::new();
    for index in 0..8usize {
        drivers.push(coroutine(move || {
            let base = (index + 1) as f32;
            let first = core::hint::black_box(base * 1.125 + 0.0625);
            let second = core::hint::black_box(-base * 3.75 + 0.25);
            let third = core::hint::black_box(base / 7.0);
            let fourth = core::hint::black_box((index as f64 + 1.5) * 1.333_333_333_333);
            let fifth = core::hint::black_box(-((index as f64) + 2.25) * 42.125);

            suspend_on(embassy_futures::yield_now());

            let scratch_single = core::hint::black_box(first * 2.0 + second - third);
            let scratch_double = core::hint::black_box(fourth * 3.0 - fifth);

            suspend_on(embassy_futures::yield_now());

            assert_eq!(first.to_bits(), (base * 1.125 + 0.0625).to_bits());
            assert_eq!(second.to_bits(), (-base * 3.75 + 0.25).to_bits());
            assert_eq!(third.to_bits(), (base / 7.0).to_bits());
            assert_eq!(
                fourth.to_bits(),
                ((index as f64 + 1.5) * 1.333_333_333_333).to_bits()
            );
            assert_eq!(
                fifth.to_bits(),
                (-((index as f64) + 2.25) * 42.125).to_bits()
            );
            assert_eq!(
                scratch_single.to_bits(),
                (first * 2.0 + second - third).to_bits()
            );
            assert_eq!(scratch_double.to_bits(), (fourth * 3.0 - fifth).to_bits());
            index as i32
        }));
    }

    let mut context = Context::from_waker(Waker::noop());
    for round in 0..2 {
        for driver in &mut drivers {
            assert_eq!(
                std::pin::Pin::new(driver).poll(&mut context),
                Poll::Pending,
                "expected coroutine to suspend on round {round}"
            );
        }
    }
    for (index, driver) in drivers.iter_mut().enumerate() {
        assert_eq!(
            std::pin::Pin::new(driver).poll(&mut context),
            Poll::Ready(index as i32)
        );
    }
}

#[test]
fn tls_slots_are_isolated_and_zero_allocation() {
    use stackful::{TLS_SLOTS, TlsError, tls_get, tls_set};

    // Outside a coroutine: valid slots report NotInCoroutine, out-of-range report InvalidSlot.
    for slot in 0..TLS_SLOTS {
        assert_eq!(tls_get(slot), Err(TlsError::NotInCoroutine));
        assert_eq!(tls_set(slot, 123), Err(TlsError::NotInCoroutine));
    }
    assert_eq!(tls_get(TLS_SLOTS), Err(TlsError::InvalidSlot));
    assert_eq!(tls_set(TLS_SLOTS, 123), Err(TlsError::InvalidSlot));

    // Multiple interleaved coroutines on the same executor keep independent TLS slots.
    let mut drivers = Vec::new();
    for id in 1..=8usize {
        drivers.push(coroutine(move || {
            for slot in 0..TLS_SLOTS {
                assert_eq!(tls_get(slot), Ok(0));
            }
            assert_eq!(tls_get(TLS_SLOTS), Err(TlsError::InvalidSlot));
            assert_eq!(tls_set(TLS_SLOTS, 99), Err(TlsError::InvalidSlot));

            for slot in 0..TLS_SLOTS {
                tls_set(slot, id * 100 + slot).unwrap();
            }

            // Polling a future on the executor stack must not see any coroutine's TLS.
            let poll_saw_not_in_coroutine = suspend_on(core::future::poll_fn({
                let mut yielded = false;
                move |cx| {
                    assert_eq!(tls_get(0), Err(TlsError::NotInCoroutine));
                    assert_eq!(tls_set(0, 0xdead), Err(TlsError::NotInCoroutine));
                    if !yielded {
                        yielded = true;
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    } else {
                        Poll::Ready(true)
                    }
                }
            }));
            assert!(poll_saw_not_in_coroutine);

            // Values survive suspension and other coroutines running in between.
            for slot in 0..TLS_SLOTS {
                assert_eq!(tls_get(slot), Ok(id * 100 + slot));
            }
            id as i32
        }));
    }

    let before = ALLOCATIONS.with(Cell::get);
    let mut cx = Context::from_waker(Waker::noop());
    for c in &mut drivers {
        assert_eq!(std::pin::Pin::new(c).poll(&mut cx), Poll::Pending);
    }
    for (index, c) in drivers.iter_mut().enumerate() {
        assert_eq!(
            std::pin::Pin::new(c).poll(&mut cx),
            Poll::Ready((index + 1) as i32)
        );
    }
    assert_eq!(ALLOCATIONS.with(Cell::get), before);
}

#[test]
fn two_kib_stack_runs_simple_call_chain() {
    #[inline(never)]
    fn leaf(x: i32) -> i32 {
        let mut buf = [0u8; 64];
        for (i, byte) in buf.iter_mut().enumerate() {
            *byte = (x as u8).wrapping_add(i as u8);
        }
        let acc = i32::from(buf[0]) + i32::from(buf[63]);
        stackful::tls_set(0, acc as usize).unwrap();
        let resumed = suspend_on(async move { acc + 1 });
        resumed + stackful::tls_get(0).unwrap() as i32
    }

    #[inline(never)]
    fn middle(x: i32) -> i32 {
        leaf(x + 3) + 10
    }

    #[inline(never)]
    fn outer(x: i32) -> i32 {
        middle(x * 2) - 1
    }

    let stack = Stack::with_min_size(stackful::MIN_STACK_SIZE, stackful::MIN_STACK_SIZE);
    assert_eq!(stack.usable_size(), 2048);
    assert!(stack.guard_intact());

    // SAFETY: bounded small call chain fits comfortably inside 2 KiB and never unwinds.
    let c = unsafe { Coroutine::new(stack, || outer(5)) };
    // outer(5) -> middle(10) -> leaf(13): buf[0]=13, buf[63]=76 -> acc=89 -> resumed=90 -> leaf=179 -> middle=189 -> outer=188
    assert_eq!(embassy_futures::block_on(c), 188);
}

#[test]
fn exit_leaves_from_a_nested_frame_without_running_destructors() {
    let reached_after = Rc::new(Cell::new(false));
    // The entry's capture is moved into its frame, so exit abandons it: its
    // destructor never runs and the count below stays raised. This is the leak
    // the safety contract of `exit` describes.
    let captured = Rc::new(());
    let (after, held) = (reached_after.clone(), captured.clone());
    let mut c = pin!(coroutine(move || {
        fn nested(depth: i32) -> i32 {
            if depth == 0 {
                // SAFETY: the frames above own no resource that matters here.
                unsafe { stackful::exit(7) }
            }
            nested(depth - 1) + 1
        }
        let _held = held;
        let result = nested(3);
        after.set(true);
        result
    }));
    let join = c.join_handle();
    let mut join = pin!(join);
    let wake = Arc::new(Wake(AtomicUsize::new(0)));
    let waker = Waker::from(wake.clone());
    let mut cx = Context::from_waker(&waker);
    assert_eq!(join.as_mut().poll(&mut cx), Poll::Pending);
    assert_eq!(c.as_mut().poll(&mut cx), Poll::Ready(7));
    assert!(!reached_after.get());
    assert_eq!(Rc::strong_count(&captured), 2);
    // The join observer saw the status and was woken once.
    assert_eq!(wake.0.load(Ordering::Relaxed), 1);
    assert_eq!(join.as_mut().poll(&mut cx), Poll::Ready(Ok(7)));
    // Polling a finished coroutine again repeats its status.
    assert_eq!(c.as_mut().poll(&mut cx), Poll::Ready(7));
}

#[test]
fn exit_after_a_suspension_and_outside_a_coroutine() {
    let signal = Rc::new(Signal::<NoopRawMutex, i32>::new());
    let s = signal.clone();
    let mut c = pin!(coroutine(move || {
        let value = suspend_on(s.wait());
        // SAFETY: nothing owning a resource is live below the entry.
        unsafe { stackful::exit(value + 1) }
    }));
    let wake = Arc::new(Wake(AtomicUsize::new(0)));
    let waker = Waker::from(wake.clone());
    let mut cx = Context::from_waker(&waker);
    assert_eq!(c.as_mut().poll(&mut cx), Poll::Pending);
    signal.signal(41);
    assert_eq!(c.as_mut().poll(&mut cx), Poll::Ready(42));

    let outside = std::panic::catch_unwind(|| {
        // SAFETY: the call panics before it abandons any frame.
        unsafe { stackful::exit(0) }
    });
    assert!(outside.is_err());
}
