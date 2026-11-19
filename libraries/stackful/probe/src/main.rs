#![no_std]
#![no_main]

extern crate alloc;
use alloc::boxed::Box;
#[cfg(any(target_arch = "arm", feature = "fp-register"))]
use core::arch::asm;
use core::{
    alloc::{GlobalAlloc, Layout},
    arch::global_asm,
    cell::UnsafeCell,
    ptr::{addr_of_mut, null_mut},
};

#[cfg(target_arch = "arm")]
global_asm!(
    ".section .vectors, \"a\"",
    ".word _stack_top, boot, 0, fault",
);
#[cfg(target_arch = "riscv32")]
global_asm!(
    ".section .text.reset,\"ax\"",
    ".global _start",
    "_start:",
    "la sp, _stack_top",
    "li t0, 0x6000",
    "csrs mstatus, t0",
    "tail boot",
);
#[cfg(target_arch = "arm")]
#[unsafe(no_mangle)]
extern "C" fn fault() -> ! {
    report(b"fault\n\0");
    halt(3)
}

#[repr(align(16))]
struct Arena([u8; 24 * 1024]);
static mut ARENA: Arena = Arena([0; 24 * 1024]);
struct Bump(UnsafeCell<usize>);
// Probe is single-threaded and keeps interrupts disabled.
unsafe impl Sync for Bump {}
static mut STACK_USED: usize = 0;
const USABLE: usize = stackful::MIN_STACK_SIZE;
const GUARD: usize = 64;

unsafe impl GlobalAlloc for Bump {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let cursor = unsafe { &mut *self.0.get() };
        let offset = (*cursor + layout.align() - 1) & !(layout.align() - 1);
        if offset + layout.size() > 24 * 1024 || layout.align() > 16 {
            return null_mut();
        }
        *cursor = offset + layout.size();
        unsafe { addr_of_mut!(ARENA.0).cast::<u8>().add(offset) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if layout.size() == USABLE + GUARD && layout.align() == 16 {
            for offset in GUARD..layout.size() {
                if unsafe { ptr.add(offset).read_volatile() } != 0xa5 {
                    unsafe { STACK_USED = layout.size() - offset };
                    break;
                }
            }
        }
    }
}
#[global_allocator]
static ALLOC: Bump = Bump(UnsafeCell::new(0));

struct SingleThread;
critical_section::set_impl!(SingleThread);
unsafe impl critical_section::Impl for SingleThread {
    unsafe fn acquire() -> critical_section::RawRestoreState {}
    unsafe fn release(_: critical_section::RawRestoreState) {}
}

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

#[unsafe(no_mangle)]
extern "C" fn boot() -> ! {
    #[cfg(target_arch = "arm")]
    unsafe {
        let cpacr = 0xe000_ed88 as *mut u32;
        cpacr.write_volatile(cpacr.read_volatile() | (0xf << 20));
        asm!("dsb", "isb", options(nostack));
    }
    #[cfg(feature = "fp-register")]
    verify_fp_register();
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
            let used = unsafe { STACK_USED };
            assert!(used > 0 && used < USABLE);
            number(b"control: ", stackful::control_allocation_size());
            number(b"painted high-water: ", used);
            halt(0);
        },
    ))
    .unwrap();
    loop {
        unsafe { executor.poll() };
    }
}

fn number(label: &[u8], mut value: usize) {
    let mut message = [0; 64];
    message[..label.len()].copy_from_slice(label);
    let mut end = label.len() + 8;
    message[end] = b'\n';
    while end > label.len() {
        end -= 1;
        message[end] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    report(&message);
}

fn report(message: &[u8]) {
    #[cfg(target_arch = "arm")]
    unsafe {
        asm!("bkpt 0xab", inout("r0") 4usize => _, inout("r1") message.as_ptr() => _, options(nostack));
    }
    #[cfg(target_arch = "riscv32")]
    for &byte in message.iter().take_while(|&&byte| byte != 0) {
        unsafe { (0x1000_0000 as *mut u8).write_volatile(byte) }
    }
}

fn halt(status: u32) -> ! {
    #[cfg(target_arch = "arm")]
    unsafe {
        let block = [0x20026, status];
        asm!("bkpt 0xab", inout("r0") 0x20usize => _, inout("r1") block.as_ptr() => _, options(nostack));
    }
    #[cfg(target_arch = "riscv32")]
    unsafe {
        (0x10_0000 as *mut u32).write_volatile(if status == 0 {
            0x5555
        } else {
            (status << 16) | 0x3333
        });
    }
    loop {
        core::hint::spin_loop()
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    report(b"panic\n\0");
    halt(1)
}

#[cfg(feature = "fp-register")]
#[inline(never)]
fn verify_fp_register() {
    use core::task::{Context, Poll, Waker};

    #[inline(always)]
    fn write(value: u32) {
        unsafe {
            #[cfg(target_arch = "arm")]
            asm!("vmov d8, {word}, {word}", word = in(reg) value, lateout("d8") _, options(nostack));
            #[cfg(target_arch = "riscv32")]
            asm!("fmv.w.x fs0, {word}", word = in(reg) value, lateout("fs0") _, options(nostack));
        }
    }

    fn read() -> u32 {
        let value: u32;
        unsafe {
            #[cfg(target_arch = "arm")]
            asm!("vmov {word}, d8[0]", word = out(reg) value, options(nostack));
            #[cfg(target_arch = "riscv32")]
            asm!("fmv.x.w {word}, fs0", word = out(reg) value, options(nostack));
        }
        value
    }

    write(0x1357);
    let mut coro = unsafe {
        stackful::Coroutine::new(stackful::Stack::new(0), || {
            write(0x2468);
            stackful::suspend_on(task::yield_now());
            if read() != 0x2468 {
                report(b"native FP register clobbered\n\0");
                halt(4);
            }
            42
        })
    };
    let mut cx = Context::from_waker(Waker::noop());
    assert_eq!(coro.resume(&mut cx), Poll::Pending);
    if read() != 0x1357 {
        report(b"parent FP register clobbered\n\0");
        halt(5);
    }
    assert_eq!(coro.resume(&mut cx), Poll::Ready(42));
}
