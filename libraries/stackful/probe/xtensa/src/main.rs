//! Xtensa firmware probe: the Xila stackful runtime on the corosensei backend.
//!
//! Boots under Espressif's QEMU (ESP32, ESP32-S3), runs the runtime scenarios
//! (feature `scenarios`) and the same one-coroutine footprint workload as the
//! Cortex-M and RISC-V probes. Results go to UART0, the run ends with
//! `PROBE OK` or `PROBE FAILED`.
#![no_std]
#![no_main]
#![feature(asm_experimental_arch)]

extern crate alloc;

use core::{
    alloc::{GlobalAlloc, Layout},
    cell::UnsafeCell,
    ptr::{addr_of_mut, null_mut},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
use esp_println::println;

esp_bootloader_esp_idf::esp_app_desc!();

const ARENA_SIZE: usize = 96 * 1024;
#[repr(align(16))]
struct Arena([u8; ARENA_SIZE]);
static mut ARENA: Arena = Arena([0; ARENA_SIZE]);
struct Bump(UnsafeCell<usize>);
// The probe is single-threaded.
unsafe impl Sync for Bump {}
const USABLE: usize = stackful::MIN_STACK_SIZE;
const GUARD: usize = 64;
static STACK_USED: AtomicUsize = AtomicUsize::new(0);
static LAST_STACK_BASE: AtomicUsize = AtomicUsize::new(0);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
// Freed blocks are reused for later allocations of the same size.
static mut FREE: [(usize, usize); 64] = [(0, 0); 64];

unsafe impl GlobalAlloc for Bump {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let free = unsafe { &mut *addr_of_mut!(FREE) };
        if let Some(slot) = free
            .iter_mut()
            .find(|(p, size)| *p != 0 && *size == layout.size() && *p % layout.align() == 0)
        {
            let ptr = slot.0 as *mut u8;
            slot.0 = 0;
            ALLOCATIONS.store(ALLOCATIONS.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
            if layout.size() == USABLE + GUARD && layout.align() == 16 {
                LAST_STACK_BASE.store(ptr as usize, Ordering::Relaxed);
            }
            return ptr;
        }
        let cursor = unsafe { &mut *self.0.get() };
        let offset = (*cursor + layout.align() - 1) & !(layout.align() - 1);
        if offset + layout.size() > ARENA_SIZE || layout.align() > 16 {
            return null_mut();
        }
        *cursor = offset + layout.size();
        ALLOCATIONS.store(ALLOCATIONS.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        let ptr = unsafe { addr_of_mut!(ARENA.0).cast::<u8>().add(offset) };
        if layout.size() == USABLE + GUARD && layout.align() == 16 {
            LAST_STACK_BASE.store(ptr as usize, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // Painted high-water mark of the audited 2 KiB stack, as in the other probes.
        if layout.size() == USABLE + GUARD && layout.align() == 16 {
            for offset in GUARD..layout.size() {
                if unsafe { ptr.add(offset).read_volatile() } != 0xa5 {
                    STACK_USED.store(layout.size() - offset, Ordering::Relaxed);
                    break;
                }
            }
        }
        let free = unsafe { &mut *addr_of_mut!(FREE) };
        if let Some(slot) = free.iter_mut().find(|(p, _)| *p == 0) {
            *slot = (ptr as usize, layout.size());
        }
    }
}
#[global_allocator]
static ALLOC: Bump = Bump(UnsafeCell::new(0));

struct Clock;
embassy_time_driver::time_driver_impl!(static CLOCK: Clock = Clock);
impl embassy_time_driver::Driver for Clock {
    fn now(&self) -> u64 {
        0
    }
    fn schedule_wake(&self, _: u64, _: &core::task::Waker) {}
}

#[unsafe(no_mangle)]
extern "Rust" fn __pender(_: *mut ()) {}

static MANAGER: task::Manager = task::Manager::with_maximum_tasks(2);
static EXPECT_GUARD_PANIC: AtomicBool = AtomicBool::new(false);

fn cycles() -> u32 {
    let value: u32;
    unsafe { core::arch::asm!("rsr.ccount {0}", out(reg) value, options(nostack)) };
    value
}

#[esp_hal::main]
fn main() -> ! {
    // esp-hal saves the FPU registers on every interrupt; Espressif's QEMU boots
    // with the coprocessor disabled.
    #[cfg(any(feature = "esp32", feature = "esp32s3"))]
    unsafe {
        core::arch::asm!("movi {0}, 1", "wsr.cpenable {0}", "rsync", out(reg) _);
    }
    let _peripherals = esp_hal::init(esp_hal::Config::default());
    println!(
        "stackful xtensa probe: fp={} baseline={} scenarios={}",
        cfg!(target_feature = "fp"),
        cfg!(feature = "baseline"),
        cfg!(feature = "scenarios")
    );
    #[cfg(feature = "baseline")]
    {
        println!("PROBE OK");
        loop {
            core::hint::spin_loop();
        }
    }
    #[cfg(not(feature = "baseline"))]
    {
        #[cfg(feature = "scenarios")]
        scenarios::run();
        footprint()
    }
}

#[cfg(not(feature = "baseline"))]
fn footprint() -> ! {
    use alloc::boxed::Box;
    let executor = Box::leak(Box::new(task::embassy_executor::raw::Executor::new(
        null_mut(),
    )));
    let spawner = executor.spawner();
    let registered = MANAGER.register_spawner(spawner).unwrap();
    task::block_on(MANAGER.spawn_on(
        spawner,
        task::Manager::ROOT_TASK_IDENTIFIER,
        "root",
        Some(registered),
        |parent| async move {
            let (join, _) = unsafe {
                stackful::spawn(
                    &MANAGER,
                    parent,
                    "one",
                    stackful::Stack::with_min_size(USABLE, USABLE),
                    || {
                        stackful::suspend_on(task::yield_now());
                        42
                    },
                )
                .await
                .unwrap()
            };
            assert_eq!(join.join().await, 42);
            let used = STACK_USED.load(Ordering::Relaxed);
            assert!(used > 0 && used < USABLE);
            println!("control: {}", stackful::control_allocation_size());
            println!("painted high-water: {}", used);
            println!("PROBE OK");
            loop {
                core::hint::spin_loop();
            }
        },
    ))
    .unwrap();
    loop {
        unsafe { executor.poll() };
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    if EXPECT_GUARD_PANIC.load(Ordering::Relaxed) {
        println!(
            "scenario canary_detection ... ok (panic: {})",
            info.message()
        );
        #[cfg(feature = "scenarios")]
        scenarios::after_canary();
    }
    println!("PANIC: {}", info);
    println!("PROBE FAILED");
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(feature = "scenarios")]
mod scenarios {
    use super::*;
    use alloc::{rc::Rc, vec::Vec};
    use core::{
        cell::Cell,
        hint::black_box,
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    use stackful::{Coroutine, Stack, suspend_on};

    fn coroutine(size: usize, entry: impl FnOnce() -> i32 + 'static) -> Coroutine {
        // These bounded scenario entries fit well within their stacks and never unwind.
        unsafe { Coroutine::new(Stack::new(size), entry) }
    }
    fn poll(c: &mut Coroutine) -> Poll<i32> {
        Pin::new(c).poll(&mut Context::from_waker(Waker::noop()))
    }
    macro_rules! scenario {
        ($name:ident) => {{
            println!("scenario {} ...", stringify!($name));
            $name();
            println!("scenario {} ... ok", stringify!($name));
        }};
    }

    pub fn run() {
        scenario!(interleaving_nested_calls_and_join);
        scenario!(deep_window_chain);
        scenario!(cancellation_and_unstarted_drop);
        scenario!(allocation_budget);
        scenario!(two_kib_call_chain);
        scenario!(tls_slots_are_isolated);
        scenario!(fp_state_is_local);
        scenario!(switch_round_trip_cycles);
        scenario!(nested_coroutines);
        canary_detection();
    }

    fn interleaving_nested_calls_and_join() {
        let mut drivers = Vec::new();
        let mut joins = Vec::new();
        for n in 0..16 {
            let c = coroutine(4096, move || {
                #[inline(never)]
                fn nested(n: i32) -> i32 {
                    suspend_on(embassy_futures::yield_now());
                    suspend_on(async { n })
                }
                nested(n)
            });
            joins.push(c.join_handle());
            drivers.push(c);
        }
        for c in &mut drivers {
            assert_eq!(poll(c), Poll::Pending);
        }
        for (n, c) in drivers.iter_mut().enumerate() {
            assert_eq!(poll(c), Poll::Ready(n as i32));
        }
        for (n, join) in joins.into_iter().enumerate() {
            assert_eq!(embassy_futures::block_on(join), Ok(n as i32));
        }
        let child = coroutine(4096, || {
            suspend_on(embassy_futures::yield_now());
            73
        });
        let join = child.join_handle();
        let parent = coroutine(4096, move || suspend_on(join).unwrap());
        let (a, b) = embassy_futures::block_on(embassy_futures::join::join(child, parent));
        assert_eq!((a, b), (73, 73));
    }

    /// Suspends at the bottom of a call chain much deeper than the 16 register windows.
    fn deep_window_chain() {
        #[inline(never)]
        fn deep(n: u32) -> u32 {
            if n == 0 {
                suspend_on(embassy_futures::yield_now());
                return 1;
            }
            black_box(deep(n - 1)) + black_box(n)
        }
        let mut c = coroutine(8192, || {
            let a = deep(45);
            suspend_on(embassy_futures::yield_now());
            let b = deep(3);
            (a + b) as i32
        });
        // The parent also has live frames across each switch.
        #[inline(never)]
        fn parent_depth(n: u32, c: &mut Coroutine, out: &mut Poll<i32>) -> u32 {
            if n == 0 {
                *out = poll(c);
                return 0;
            }
            black_box(parent_depth(n - 1, c, out)) + black_box(n)
        }
        let mut out = Poll::Pending;
        // deep(45) = 1 + sum 1..=45 = 1036, deep(3) = 7.
        parent_depth(30, &mut c, &mut out);
        assert_eq!(out, Poll::Pending);
        parent_depth(17, &mut c, &mut out);
        assert_eq!(out, Poll::Pending);
        parent_depth(3, &mut c, &mut out);
        assert_eq!(out, Poll::Pending);
        parent_depth(40, &mut c, &mut out);
        assert_eq!(out, Poll::Ready(1036 + 7));
    }

    fn cancellation_and_unstarted_drop() {
        struct DropCount(Rc<Cell<usize>>);
        impl Drop for DropCount {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }
        let drops = Rc::new(Cell::new(0));
        let d = DropCount(drops.clone());
        drop(coroutine(4096, move || {
            drop(d);
            0
        }));
        assert_eq!(drops.get(), 1);
        let mut c = coroutine(4096, || {
            suspend_on(core::future::pending::<()>());
            0
        });
        let join = c.join_handle();
        assert_eq!(poll(&mut c), Poll::Pending);
        drop(c);
        assert_eq!(embassy_futures::block_on(join), Err(stackful::Cancelled));

        let resource = DropCount(drops.clone());
        let mut driver = coroutine(4096, move || {
            stackful::suspend_on_cancellable(async move {
                core::future::pending::<()>().await;
                drop(resource);
            });
            0
        });
        assert_eq!(poll(&mut driver), Poll::Pending);
        assert_eq!(drops.get(), 1);
        drop(driver);
        assert_eq!(drops.get(), 2);
    }

    fn allocation_budget() {
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        let mut c = coroutine(4096, || {
            for _ in 0..100 {
                suspend_on(embassy_futures::yield_now());
            }
            17
        });
        let join = c.join_handle();
        assert_eq!(ALLOCATIONS.load(Ordering::Relaxed) - before, 2);
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        for _ in 0..100 {
            assert_eq!(poll(&mut c), Poll::Pending);
        }
        assert_eq!(poll(&mut c), Poll::Ready(17));
        assert_eq!(embassy_futures::block_on(join), Ok(17));
        assert_eq!(ALLOCATIONS.load(Ordering::Relaxed), before);
    }

    fn two_kib_call_chain() {
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
        let mut c = unsafe { Coroutine::new(stack, || outer(5)) };
        assert_eq!(poll(&mut c), Poll::Ready(188));
    }

    fn tls_slots_are_isolated() {
        use stackful::{TLS_SLOTS, TlsError, tls_get, tls_set};
        assert_eq!(tls_get(0), Err(TlsError::NotInCoroutine));
        assert_eq!(tls_get(TLS_SLOTS), Err(TlsError::InvalidSlot));
        let mut drivers = Vec::new();
        for id in 1..=8usize {
            drivers.push(coroutine(4096, move || {
                for slot in 0..TLS_SLOTS {
                    tls_set(slot, id * 100 + slot).unwrap();
                }
                suspend_on(embassy_futures::yield_now());
                for slot in 0..TLS_SLOTS {
                    assert_eq!(tls_get(slot), Ok(id * 100 + slot));
                }
                id as i32
            }));
        }
        for c in &mut drivers {
            assert_eq!(poll(c), Poll::Pending);
        }
        for (index, c) in drivers.iter_mut().enumerate() {
            assert_eq!(poll(c), Poll::Ready((index + 1) as i32));
        }
    }

    /// FP values live across switches on both sides, and FCR (rounding mode) local to each side.
    fn fp_state_is_local() {
        #[cfg(target_feature = "fp")]
        {
            fn read_fcr() -> u32 {
                let v: u32;
                unsafe { core::arch::asm!("rur.fcr {0}", out(reg) v, options(nostack)) };
                v
            }
            fn write_fcr(v: u32) {
                unsafe { core::arch::asm!("wur.fcr {0}", in(reg) v, options(nostack)) };
            }
            let original = read_fcr();
            let mode = (original & !3) | 1;
            let mut c = coroutine(4096, move || {
                let mut acc = black_box(0.5f32);
                write_fcr(mode);
                for i in 0..4 {
                    suspend_on(embassy_futures::yield_now());
                    acc = black_box(acc * 2.0 + black_box(i as f32));
                }
                i32::from(read_fcr() == mode) + black_box(acc) as i32 * 10
            });
            let mut parent = black_box(1.25f32);
            for _ in 0..4 {
                assert_eq!(poll(&mut c), Poll::Pending);
                assert_eq!(read_fcr(), original, "coroutine FCR leaked into the parent");
                parent = black_box(parent * 3.0);
            }
            // acc: 0.5 -> 1 -> 3 -> 8 -> 19.
            let mut expected = 0.5f32;
            for i in 0..4 {
                expected = expected * 2.0 + i as f32;
            }
            assert_eq!(poll(&mut c), Poll::Ready(1 + expected as i32 * 10));
            assert_eq!(parent, 1.25 * 81.0);
            assert_eq!(read_fcr(), original);
        }
        #[cfg(not(target_feature = "fp"))]
        println!("  (no FPU on this chip, FCR not exercised)");
    }

    fn switch_round_trip_cycles() {
        const N: u32 = 20_000;
        let mut c = coroutine(4096, || {
            for _ in 0..N {
                suspend_on(embassy_futures::yield_now());
            }
            0
        });
        let start = cycles();
        for _ in 0..N {
            assert_eq!(poll(&mut c), Poll::Pending);
        }
        let elapsed = cycles().wrapping_sub(start);
        assert_eq!(poll(&mut c), Poll::Ready(0));
        println!(
            "  {} CCOUNT ticks per suspend/resume round trip (emulated, driver and guard checks included)",
            elapsed / N
        );
    }

    fn nested_coroutines() {
        let mut outer = coroutine(8192, || {
            let mut inner = coroutine(4096, || {
                suspend_on(embassy_futures::yield_now());
                5
            });
            let mut total = 0;
            loop {
                match Pin::new(&mut inner).poll(&mut Context::from_waker(Waker::noop())) {
                    Poll::Pending => suspend_on(embassy_futures::yield_now()),
                    Poll::Ready(v) => {
                        total += v;
                        break;
                    }
                }
            }
            total
        });
        let mut polls = 0;
        loop {
            polls += 1;
            match poll(&mut outer) {
                Poll::Pending => continue,
                Poll::Ready(v) => {
                    assert_eq!(v, 5);
                    break;
                }
            }
        }
        assert!(polls >= 2);
    }

    /// The guard is a painted canary: damage must be detected at the next switch.
    fn canary_detection() {
        println!("scenario canary_detection ...");
        let mut c = unsafe {
            Coroutine::new(Stack::with_min_size(USABLE, USABLE), || {
                let base = LAST_STACK_BASE.load(Ordering::Relaxed) as *mut u8;
                base.add(8).write_volatile(0);
                suspend_on(embassy_futures::yield_now());
                0
            })
        };
        EXPECT_GUARD_PANIC.store(true, Ordering::Relaxed);
        let _ = poll(&mut c);
        // The driver's guard check after the switch must have panicked.
        EXPECT_GUARD_PANIC.store(false, Ordering::Relaxed);
        println!("PANIC: canary damage was not detected");
        println!("PROBE FAILED");
        loop {}
    }

    /// Reached from the panic handler once the canary panic is observed.
    pub fn after_canary() -> ! {
        EXPECT_GUARD_PANIC.store(false, Ordering::Relaxed);
        println!("SCENARIOS OK");
        // Everything after the panic is unreachable from the aborted coroutine
        // frames, so continue with the footprint workload on a fresh state.
        super::footprint()
    }
}
