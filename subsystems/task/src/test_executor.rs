//! Single-threaded std executor for tests (`test_harness` feature).
//!
//! Lets `#[task::test]` and test helpers run without any platform crate.
//! Embassy's `platform-std` provides the pender that wakes [`Executor`].

extern crate std;

use alloc::boxed::Box;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_executor::{Spawner, raw};
use std::sync::{Condvar, Mutex};
use synchronization::blocking_mutex::raw::CriticalSectionRawMutex;
use synchronization::signal::Signal;

use crate::{Manager, SpawnerIdentifier};

/// A std executor that [`Executor::run`] until [`Executor::stop`] is called.
pub struct Executor {
    inner: raw::Executor,
    not_send: PhantomData<*mut ()>,
    signaler: &'static Signaler,
    stop: AtomicBool,
}

impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

impl Executor {
    pub fn new() -> Self {
        let signaler = Box::leak(Box::new(Signaler::new()));
        Self {
            inner: raw::Executor::new(signaler as *mut Signaler as *mut ()),
            not_send: PhantomData,
            signaler,
            stop: AtomicBool::new(false),
        }
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.signaler.signal();
    }

    pub fn spawner(&'static self) -> Spawner {
        self.inner.spawner()
    }

    /// Call `init` with a spawner, then poll tasks until [`Self::stop`].
    pub fn run(&'static self, init: impl FnOnce(Spawner, &'static Self)) {
        init(self.inner.spawner(), self);
        while !self.stop.load(Ordering::SeqCst) {
            self.poll_and_wait();
        }
    }

    /// Call `init` with a spawner, then poll tasks forever.
    pub fn start(&'static self, init: impl FnOnce(Spawner)) -> ! {
        init(self.inner.spawner());
        loop {
            self.poll_and_wait();
        }
    }

    fn poll_and_wait(&'static self) {
        unsafe { self.inner.poll() };
        self.signaler.wait();
    }
}

// Layout shared with embassy's `platform-std` pender.
struct Signaler {
    mutex: Mutex<bool>,
    condvar: Condvar,
}

impl Signaler {
    fn new() -> Self {
        Self {
            mutex: Mutex::new(false),
            condvar: Condvar::new(),
        }
    }

    fn wait(&self) {
        let mut signaled = self.mutex.lock().unwrap();
        while !*signaled {
            signaled = self.condvar.wait(signaled).unwrap();
        }
        *signaled = false;
    }

    fn signal(&self) {
        let mut signaled = self.mutex.lock().unwrap();
        *signaled = true;
        self.condvar.notify_one();
    }
}

/// A per-call-site static [`Executor`], as used by `#[task::test]`.
#[macro_export]
macro_rules! instantiate_test_executor {
    () => {{
        static mut __EXECUTOR: Option<$crate::test_executor::Executor> = None;
        // Callers such as `#[task::test]` may already be in an unsafe block.
        #[allow(static_mut_refs, unused_unsafe)]
        unsafe {
            if __EXECUTOR.is_none() {
                __EXECUTOR = Some($crate::test_executor::Executor::new());
            }
            __EXECUTOR.as_mut().expect("executor is initialized")
        }
    }};
}

/// Start an executor on a new thread and register it with `manager`.
pub async fn new_thread_executor(manager: &'static Manager) -> SpawnerIdentifier {
    let signal: &'static Signal<CriticalSectionRawMutex, SpawnerIdentifier> =
        Box::leak(Box::new(Signal::new()));
    std::thread::spawn(move || {
        let executor: &'static Executor = Box::leak(Box::new(Executor::new()));
        executor.start(move |spawner| {
            signal.signal(manager.register_spawner(spawner).unwrap());
        });
    });
    signal.wait().await
}
