#[path = "arch.rs"]
mod arch;
#[path = "stack.rs"]
mod stack;
pub use stack::{DEFAULT_MIN_STACK_SIZE, MIN_STACK_SIZE, Stack};

use alloc::rc::Rc;
use core::{
    cell::{Cell, RefCell, UnsafeCell},
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

/// Number of coroutine-local storage slots stored inline in each coroutine control block.
pub const TLS_SLOTS: usize = 4;

/// Failure accessing coroutine-local storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TlsError {
    /// Called outside a running coroutine.
    NotInCoroutine,
    /// Requested slot index is greater than or equal to [`TLS_SLOTS`].
    InvalidSlot,
}

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
    tls: [Cell<usize>; TLS_SLOTS],
    native_context: Cell<*const ()>,
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
    ///
    /// On bare metal the current coroutine is one global, so while any coroutine is
    /// live, no other core may call an API that reaches [`suspend_on`]: the native
    /// application ABI and the synchronous VFS files and directories. That core would
    /// see the live coroutine as its own and switch onto its stack. A per-core current
    /// pointer (xila-project/core#682) lifts this restriction.
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
                tls: [const { Cell::new(0) }; TLS_SLOTS],
                native_context: Cell::new(core::ptr::null()),
            }),
        }
    }
    /// Install the native ABI's private context for this coroutine.
    /// # Safety
    /// A non-null value must identify a live native ABI context for each poll.
    pub unsafe fn set_native_context(&mut self, value: *const ()) {
        self.control.native_context.set(value);
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
        self.control.native_context.set(core::ptr::null());
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

/// Whether execution is currently on a native coroutine stack.
pub fn is_in_coroutine() -> bool {
    !current().is_null()
}

/// Await from native code. Outside a coroutine this preserves task::block_on behavior.
/// No heap allocation occurs here. The future and output stay on the native stack.
/// Do not call this from a future's poll method: poll methods must return Pending.
pub fn suspend_on<F: Future>(future: F) -> F::Output {
    let c = current();
    if c.is_null() {
        return task::block_on(future);
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

/// Pointer privately installed by the native ABI; null on other coroutines.
pub fn native_context() -> *const () {
    unsafe { current().as_ref() }.map_or(core::ptr::null(), |control| control.native_context.get())
}

/// Install the native ABI's private context on the running coroutine.
/// # Safety
/// Must be in a coroutine. A non-null value must remain a live native ABI
/// context while execution can access it.
pub unsafe fn set_native_context(value: *const ()) {
    if let Some(control) = unsafe { current().as_ref() } {
        control.native_context.set(value);
    }
}

/// Read a coroutine-local storage slot for the currently running coroutine.
/// Slots start at `0`. No heap allocation occurs.
/// Returns [`TlsError::InvalidSlot`] if `slot >= TLS_SLOTS`, or
/// [`TlsError::NotInCoroutine`] outside a running coroutine.
pub fn tls_get(slot: usize) -> Result<usize, TlsError> {
    if slot >= TLS_SLOTS {
        return Err(TlsError::InvalidSlot);
    }
    let c = current();
    if c.is_null() {
        return Err(TlsError::NotInCoroutine);
    }
    // SAFETY: `c` points to the live control block of the currently executing coroutine.
    let tls = unsafe { &(*c).tls };
    Ok(tls[slot].get())
}

/// Write a coroutine-local storage slot for the currently running coroutine.
/// No heap allocation occurs.
/// Returns [`TlsError::InvalidSlot`] if `slot >= TLS_SLOTS`, or
/// [`TlsError::NotInCoroutine`] outside a running coroutine.
pub fn tls_set(slot: usize, value: usize) -> Result<(), TlsError> {
    if slot >= TLS_SLOTS {
        return Err(TlsError::InvalidSlot);
    }
    let c = current();
    if c.is_null() {
        return Err(TlsError::NotInCoroutine);
    }
    // SAFETY: `c` points to the live control block of the currently executing coroutine.
    let tls = unsafe { &(*c).tls };
    tls[slot].set(value);
    Ok(())
}

/// Publish `status`, wake the join observer and leave the coroutine for good.
///
/// # Safety
/// `c` must be the control block of the running coroutine. Nothing may resume it.
unsafe fn finish(c: *const Control, status: i32) -> ! {
    unsafe {
        (*c).native_context.set(core::ptr::null());
        (*c).result.set(Some(Ok(status)));
        let waker = (*c).waker.borrow_mut().take();
        if let Some(w) = waker {
            w.wake();
        }
        (*c).stack.check_guard();
        arch::switch((*c).coroutine_sp.get(), (*c).caller_sp.get());
    }
    panic!("completed coroutine resumed");
}

/// End the running coroutine at once with `status`, as if its entry had returned it.
///
/// The frames between this call and the entry are abandoned: no destructor runs and
/// nothing unwinds, like a `longjmp` that never returns. The driver then observes
/// `status`, and the stack and control block are released as for a normal return.
/// Panics outside a coroutine.
///
/// # Safety
/// Every object that owns a resource (an allocation, an open file, a lock guard) and
/// is live in an abandoned frame is leaked. The caller must have released or must
/// not need such resources, including the entry closure's own captures: move owned
/// data out of the coroutine stack, or free it before calling. Frames that
/// borrowed data from the caller's executor side stay valid, as the stack is only
/// dropped by the driver after this call.
pub unsafe fn exit(status: i32) -> ! {
    let c = current();
    assert!(!c.is_null(), "exit outside a coroutine");
    unsafe { finish(c, status) }
}

// C ABI makes escaping panics abort on hosts even when the application uses unwind.
extern "C" fn trampoline() -> ! {
    let c = current();
    unsafe {
        let result = ((*c).run)((*c).entry.get());
        finish(c, result)
    }
}

/// Spawn on the calling task's executor, preserving stack/thread affinity.
/// The manager additionally allocates its existing task pool, metadata and join signal.
///
/// # Safety
/// Same contract as [`Coroutine::new`]. The manager must map the current task to
/// its actual executor; registering or moving local Spawners across threads is invalid.
pub async unsafe fn spawn<F: FnOnce() -> i32 + 'static>(
    manager: &'static task::Manager,
    parent: task::TaskIdentifier,
    name: &str,
    stack: Stack,
    entry: F,
) -> task::Result<(task::JoinHandle<i32>, task::TaskIdentifier)> {
    unsafe { spawn_with_result(manager, parent, name, stack, entry, core::convert::identity).await }
}

/// Like [`spawn`], converting the exit status before signaling the task join handle.
///
/// # Safety
/// Same contract as [`spawn`].
pub async unsafe fn spawn_with_result<F, Map, T>(
    manager: &'static task::Manager,
    parent: task::TaskIdentifier,
    name: &str,
    stack: Stack,
    entry: F,
    map: Map,
) -> task::Result<(task::JoinHandle<T>, task::TaskIdentifier)>
where
    F: FnOnce() -> i32 + 'static,
    Map: FnOnce(i32) -> T + 'static,
    T: 'static,
{
    unsafe { spawn_with_driver(manager, parent, name, stack, entry, drive, map).await }
}

/// Like [`spawn_with_result`], with a local future that drives the entry coroutine.
/// # Safety
/// Same contract as [`spawn`]. The driver must keep all native execution on the task's
/// executor and stop it before releasing the resources borrowed by the entry.
pub async unsafe fn spawn_with_driver<F, Driver, Run, Map, T>(
    manager: &'static task::Manager,
    parent: task::TaskIdentifier,
    name: &str,
    stack: Stack,
    entry: F,
    driver: Run,
    map: Map,
) -> task::Result<(task::JoinHandle<T>, task::TaskIdentifier)>
where
    F: FnOnce() -> i32 + 'static,
    Driver: Future<Output = i32> + 'static,
    Run: FnOnce(Coroutine) -> Driver + 'static,
    Map: FnOnce(i32) -> T + 'static,
    T: 'static,
{
    let caller = manager.get_current_task_identifier().await;
    let spawner = manager.get_spawner(caller).await?;
    let coroutine = unsafe { Coroutine::new(stack, entry) };
    manager
        .spawn(parent, name, Some(spawner), move |_| async move {
            map(driver(coroutine).await)
        })
        .await
}

/// Bytes in the shared control allocation, including the two Rc reference counts.
pub const fn control_allocation_size() -> usize {
    core::mem::size_of::<Control>() + 2 * core::mem::size_of::<usize>()
}
