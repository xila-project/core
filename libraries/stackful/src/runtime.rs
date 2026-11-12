#[path = "arch.rs"]
mod arch;
#[path = "stack.rs"]
mod stack;
pub use stack::Stack;

use alloc::rc::Rc;
use core::{
    cell::{Cell, RefCell, UnsafeCell},
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

type ErasedFuture = *mut (dyn Future<Output = ()> + 'static);
struct Control {
    stack: Stack,
    coroutine_sp: UnsafeCell<usize>,
    caller_sp: UnsafeCell<usize>,
    entry: Cell<*mut u8>,
    run: unsafe fn(*mut u8) -> i32,
    destroy: unsafe fn(*mut u8),
    pending: Cell<Option<ErasedFuture>>,
    started: Cell<bool>,
    result: Cell<Option<Result<i32, Cancelled>>>,
    waker: RefCell<Option<Waker>>,
}
impl Drop for Control {
    fn drop(&mut self) {
        if !self.started.get() {
            // SAFETY: the unstarted entry is still initialized on our owned stack.
            unsafe { (self.destroy)(self.entry.get()) };
        }
    }
}

#[cfg(target_os = "linux")]
std::thread_local! { static CURRENT: Cell<*const Control> = const { Cell::new(core::ptr::null()) }; }
#[cfg(target_os = "linux")]
fn current() -> *const Control {
    CURRENT.with(Cell::get)
}
#[cfg(target_os = "linux")]
fn set_current(value: *const Control) {
    CURRENT.with(|c| c.set(value));
}

#[cfg(not(target_os = "linux"))]
static CURRENT: critical_section::Mutex<Cell<usize>> = critical_section::Mutex::new(Cell::new(0));
#[cfg(not(target_os = "linux"))]
fn current() -> *const Control {
    critical_section::with(|cs| CURRENT.borrow(cs).get() as *const Control)
}
#[cfg(not(target_os = "linux"))]
fn set_current(value: *const Control) {
    critical_section::with(|cs| CURRENT.borrow(cs).set(value as usize));
}

/// A native coroutine. Neither Send nor Sync: poll it on its creating executor thread.
pub struct Coroutine {
    control: Rc<Control>,
}
/// The driver was dropped before completion; its suspended stack is intentionally leaked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cancelled;
/// One completion observer, sharing the coroutine's control allocation.
pub struct JoinHandle {
    control: Rc<Control>,
}

impl Coroutine {
    /// Create a coroutine, allocating only its shared control block.
    ///
    /// # Safety
    /// The stack must accommodate the entry and all callees, including interrupt frames
    /// if the platform uses the same SP. Canary checks cannot prevent memory corruption.
    /// Entry must never unwind. On bare metal, all coroutines must run on a single core
    /// and must never be entered or suspended from an interrupt. Do not migrate a live
    /// coroutine or its handles to another OS thread, including via foreign code.
    pub unsafe fn new<F: FnOnce() -> i32 + 'static>(mut stack: Stack, entry: F) -> Self {
        unsafe fn run<F: FnOnce() -> i32>(p: *mut u8) -> i32 {
            unsafe { p.cast::<F>().read()() }
        }
        unsafe fn destroy<F>(p: *mut u8) {
            unsafe { p.cast::<F>().drop_in_place() };
        }
        let top = stack.top();
        let entry_address = (top
            .checked_sub(core::mem::size_of::<F>())
            .expect("entry too large"))
            & !(core::mem::align_of::<F>() - 1);
        assert!(
            entry_address >= stack.usable_limit() + 1024,
            "entry leaves too little stack"
        );
        let sp = unsafe { arch::initialize(entry_address & !15, trampoline as *const () as usize) };
        unsafe { (entry_address as *mut F).write(entry) };
        Self {
            control: Rc::new(Control {
                stack,
                coroutine_sp: UnsafeCell::new(sp),
                caller_sp: UnsafeCell::new(0),
                entry: Cell::new(entry_address as *mut u8),
                run: run::<F>,
                destroy: destroy::<F>,
                pending: Cell::new(None),
                started: Cell::new(false),
                result: Cell::new(None),
                waker: RefCell::new(None),
            }),
        }
    }
    /// Create the sole join observer. Multiple observers are rejected to bound waker storage.
    pub fn join_handle(&self) -> JoinHandle {
        // Rc count also includes an earlier observer, if any.
        assert_eq!(
            Rc::strong_count(&self.control),
            1,
            "join handle already exists"
        );
        JoinHandle {
            control: self.control.clone(),
        }
    }
    /// Poll the suspended operation with the executor's real waker, then resume native code.
    pub fn resume(&mut self, cx: &mut Context<'_>) -> Poll<i32> {
        let c = Rc::as_ptr(&self.control);
        loop {
            if let Some(Ok(result)) = self.control.result.get() {
                return Poll::Ready(result);
            }
            if let Some(future) = self.control.pending.get() {
                // SAFETY: the future remains pinned on the suspended native stack. No
                // coroutine runs while the driver polls it; it is cleared before resume.
                if unsafe { Pin::new_unchecked(&mut *future) }
                    .poll(cx)
                    .is_pending()
                {
                    return Poll::Pending;
                }
                self.control.pending.set(None);
            }
            self.control.stack.check_guard();
            let previous = current();
            set_current(c);
            self.control.started.set(true);
            unsafe {
                arch::switch(
                    self.control.caller_sp.get(),
                    self.control.coroutine_sp.get(),
                )
            };
            set_current(previous);
            self.control.stack.check_guard();
        }
    }
}
impl Future for Coroutine {
    type Output = i32;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<i32> {
        self.resume(cx)
    }
}
impl Drop for Coroutine {
    fn drop(&mut self) {
        if self.control.started.get() && self.control.result.get().is_none() {
            self.control.result.set(Some(Err(Cancelled)));
            if let Some(w) = self.control.waker.borrow_mut().take() {
                w.wake();
            }
            // Never free a suspended stack containing live objects. In particular,
            // destructors may hold pointers into that stack. Cancellation leaks it.
            core::mem::forget(self.control.clone());
        }
    }
}
impl Future for JoinHandle {
    type Output = Result<i32, Cancelled>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(result) = self.control.result.get() {
            Poll::Ready(result)
        } else {
            let mut w = self.control.waker.borrow_mut();
            if w.as_ref().is_none_or(|w| !w.will_wake(cx.waker())) {
                *w = Some(cx.waker().clone());
            }
            Poll::Pending
        }
    }
}
/// Drive a coroutine as a normal local Embassy future.
pub async fn drive(coroutine: Coroutine) -> i32 {
    coroutine.await
}

/// Await from native code. Outside a coroutine this preserves task::block_on behavior.
/// No heap allocation occurs here. The future and output stay on the native stack.
/// Do not call this from a future's poll method: poll methods must return Pending.
pub fn suspend_on<F: Future>(future: F) -> F::Output {
    let c = current();
    if c.is_null() {
        #[cfg(target_has_atomic = "ptr")]
        return task::block_on(future);
        // Identical implementation on targets Core's task manager cannot build yet.
        #[cfg(not(target_has_atomic = "ptr"))]
        return embassy_futures::block_on(future);
    }
    let mut output = None;
    {
        let mut wrapper = core::pin::pin!(async {
            output = Some(future.await);
        });
        // SAFETY: erased lifetime is bounded by the suspended call frame; resume only
        // occurs once polling completes. Dropping the driver leaks the entire stack.
        let erased: ErasedFuture = unsafe {
            core::mem::transmute::<*mut dyn Future<Output = ()>, ErasedFuture>(
                wrapper.as_mut().get_unchecked_mut(),
            )
        };
        unsafe {
            (*c).stack.check_guard();
            (*c).pending.set(Some(erased));
            arch::switch((*c).coroutine_sp.get(), (*c).caller_sp.get());
            (*c).stack.check_guard();
        }
        // End the future's mutable borrow before reading its result.
    }
    output.expect("coroutine resumed before future completed")
}

// C ABI makes escaping panics abort on hosts even when the application uses unwind.
extern "C" fn trampoline() -> ! {
    let c = current();
    unsafe {
        let result = ((*c).run)((*c).entry.get());
        (*c).result.set(Some(Ok(result)));
        if let Some(w) = (*c).waker.borrow_mut().take() {
            w.wake();
        }
        (*c).stack.check_guard();
        arch::switch((*c).coroutine_sp.get(), (*c).caller_sp.get());
    }
    panic!("completed coroutine resumed");
}

/// Spawn on the calling task's executor, preserving stack/thread affinity.
/// The manager additionally allocates its existing task pool, metadata and join signal.
///
/// # Safety
/// Same contract as [`Coroutine::new`]. The manager must map the current task to
/// its actual executor; registering or moving local Spawners across threads is invalid.
#[cfg(target_has_atomic = "ptr")]
pub async unsafe fn spawn<F: FnOnce() -> i32 + 'static>(
    manager: &'static task::Manager,
    parent: task::TaskIdentifier,
    name: &str,
    stack: Stack,
    entry: F,
) -> task::Result<(task::JoinHandle<i32>, task::TaskIdentifier)> {
    let caller = manager.get_current_task_identifier().await;
    let spawner = manager.get_spawner(caller).await?;
    let coroutine = unsafe { Coroutine::new(stack, entry) };
    manager
        .spawn(parent, name, Some(spawner), move |_| drive(coroutine))
        .await
}

/// Bytes in the shared control allocation, including the two Rc reference counts.
pub const fn control_allocation_size() -> usize {
    core::mem::size_of::<Control>() + 2 * core::mem::size_of::<usize>()
}
