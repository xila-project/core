#[path = "fp.rs"]
mod fp;
#[path = "stack.rs"]
mod stack;
use stack::{Backing, check_guard_at};
pub use stack::{
    DEFAULT_MIN_STACK_SIZE, MIN_STACK_SIZE, Stack, StackAllocateFn, StackAllocator,
    StackDeallocateFn, set_stack_allocator,
};

use alloc::rc::Rc;
use core::{
    cell::{Cell, RefCell, UnsafeCell},
    future::Future,
    pin::Pin,
    ptr::NonNull,
    task::{Context, Poll, Waker},
};

type Native = corosensei::Coroutine<(), (), (), Backing>;
type Yielder = corosensei::Yielder<(), ()>;

/// Bytes corosensei reserves at the top of the stack, beyond the entry, on every target.
const INITIAL_FRAME: usize = 128;
/// Usable bytes that must remain for the entry's own frames.
const MIN_ENTRY_STACK: usize = 1024;

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
#[derive(Clone, Copy)]
struct Pending {
    future: ErasedFuture,
    cancel: bool,
}
struct Control {
    /// Owns the stack and the entry. Only the driver touches it, around each switch.
    native: UnsafeCell<Native>,
    /// The stack's painted guard, which `native` owns.
    guard: NonNull<u8>,
    /// Set when the entry starts; stays valid, on the coroutine stack, until it ends.
    yielder: Cell<*const Yielder>,
    pending: Cell<Option<Pending>>,
    started: Cell<bool>,
    result: Cell<Option<Result<i32, Cancelled>>>,
    waker: RefCell<Option<Waker>>,
    tls: [Cell<usize>; TLS_SLOTS],
    native_context: Cell<*const ()>,
}
/// Maximum number of bare-metal CPU cores tracked by the per-core current-coroutine table.
pub const MAX_CORES: usize = 8;

/// Platform hook returning the calling CPU core's index in `0..MAX_CORES`.
pub type CoreIdProvider = fn() -> usize;

#[cfg(any(not(target_os = "linux"), test))]
fn single_core_id() -> usize {
    0
}

#[cfg(any(not(target_os = "linux"), test))]
struct PerCoreCurrent {
    provider: Cell<CoreIdProvider>,
    slots: [Cell<usize>; MAX_CORES],
}

#[cfg(any(not(target_os = "linux"), test))]
impl PerCoreCurrent {
    const fn new() -> Self {
        Self {
            provider: Cell::new(single_core_id),
            slots: [const { Cell::new(0) }; MAX_CORES],
        }
    }

    fn core_index(&self) -> usize {
        let core = (self.provider.get())();
        assert!(
            core < MAX_CORES,
            "stackful core id {core} exceeds MAX_CORES ({MAX_CORES})"
        );
        core
    }

    fn get(&self) -> *const Control {
        let core = self.core_index();
        self.slots[core].get() as *const Control
    }

    fn set(&self, value: *const Control) {
        let core = self.core_index();
        self.slots[core].set(value as usize);
    }

    fn set_provider(&self, provider: CoreIdProvider) {
        assert!(
            self.slots.iter().all(|slot| slot.get() == 0),
            "cannot change stackful core id provider while a coroutine is active"
        );
        self.provider.set(provider);
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
static CURRENT: critical_section::Mutex<PerCoreCurrent> =
    critical_section::Mutex::new(PerCoreCurrent::new());
#[cfg(not(target_os = "linux"))]
fn current() -> *const Control {
    critical_section::with(|cs| CURRENT.borrow(cs).get())
}
#[cfg(not(target_os = "linux"))]
fn set_current(value: *const Control) {
    critical_section::with(|cs| CURRENT.borrow(cs).set(value));
}

/// Register the platform hook that identifies the calling CPU core on bare metal.
///
/// By default, bare-metal targets assume a single core (index `0`). Multi-core bare-metal
/// firmware (such as dual-core ESP32-S3 or ESP32-P4) must register a provider once at
/// start-up before any coroutine runs on a secondary core.
///
/// On Linux each OS thread already has its own thread-local current-coroutine pointer, so
/// this function is a no-op there.
///
/// Panics on bare metal if any coroutine is currently active when called.
///
/// # Safety
/// `provider` must return a unique index in `0..MAX_CORES` for each physical CPU core that
/// can execute concurrently, and must be safe to call inside a `critical_section` without
/// blocking, allocating, or panicking on valid cores.
pub unsafe fn set_core_id_provider(provider: CoreIdProvider) {
    #[cfg(not(target_os = "linux"))]
    critical_section::with(|cs| CURRENT.borrow(cs).set_provider(provider));
    #[cfg(target_os = "linux")]
    let _ = provider;
}

/// A native coroutine. Neither Send nor Sync: poll it on its creating executor thread.
pub struct Coroutine {
    control: Rc<Control>,
}
/// The driver was dropped before completion: the coroutine was cancelled, and its suspended
/// frames were discarded without running anything on them.
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
    /// Dropping the coroutine after it started and before it finished cancels it: the
    /// suspended frames are discarded and the stack is freed, without running a destructor
    /// on them (see [`Cancelled`]). The entry and everything it calls must therefore not
    /// leave an object that owns a resource (an open file, an allocation, a lock guard)
    /// only in a frame across a suspension, because nothing would ever release it: keep
    /// it in a place the host owns and releases itself. A future passed to
    /// [`suspend_on`] counts as such a place only if it is passed to
    /// [`suspend_on_cancellable`], which the cancellation drops in place. Nothing outside
    /// the coroutine may keep a pointer into its stack beyond the call that is suspended:
    /// the stack is freed and reused once the coroutine is dropped.
    ///
    /// On bare metal, the default current-coroutine slot assumes a single core (`0`).
    /// Multi-core firmware must register a per-core ID hook with [`set_core_id_provider`]
    /// before starting coroutines on secondary cores; without one, another core calling
    /// an API that reaches [`suspend_on`] (the native application ABI or synchronous VFS
    /// files and directories) while a coroutine is live would see that coroutine as its
    /// own and switch onto its stack.
    pub unsafe fn new<F: FnOnce() -> i32 + 'static>(stack: Stack, entry: F) -> Self {
        // The entry and corosensei's initial frame sit at the top of the stack.
        assert!(
            stack.usable_size()
                >= size_of::<F>() + align_of::<F>() + INITIAL_FRAME + MIN_ENTRY_STACK,
            "entry leaves too little stack"
        );
        let guard = stack.guard();
        let native = Native::with_stack(Backing(stack), move |yielder, ()| {
            // SAFETY: the driver set the current coroutine before entering.
            let control = unsafe { &*current() };
            control.yielder.set(yielder);
            fp::Environment::INITIAL.write();
            let status = entry();
            // SAFETY: this is the running coroutine's control block.
            unsafe { finish(control, status) }
        });
        Self {
            control: Rc::new(Control {
                native: UnsafeCell::new(native),
                guard,
                yielder: Cell::new(core::ptr::null()),
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
    /// A coroutine that was [stopped](Self::stop) stays pending for good.
    pub fn resume(&mut self, cx: &mut Context<'_>) -> Poll<i32> {
        let c = Rc::as_ptr(&self.control);
        loop {
            match self.control.result.get() {
                Some(Ok(result)) => return Poll::Ready(result),
                Some(Err(Cancelled)) => return Poll::Pending,
                None => {}
            }
            if let Some(Pending { future, .. }) = self.control.pending.get() {
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
            self.control.check_guard();
            let previous = current();
            set_current(c);
            self.control.started.set(true);
            let environment = fp::Environment::read();
            // SAFETY: the driver has exclusive access to `native`: the coroutine side
            // reaches only the other fields of the control block, through `current`.
            unsafe {
                let native = &mut *self.control.native.get();
                native.resume(());
                if matches!(self.control.result.get(), Some(Ok(_))) {
                    // `finish` left the coroutine suspended for good, abandoning
                    // its frames as `exit` documents. Nothing on them is dropped.
                    native.force_reset();
                }
            }
            environment.write();
            set_current(previous);
            self.control.check_guard();
        }
    }
}
impl Future for Coroutine {
    type Output = i32;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<i32> {
        self.resume(cx)
    }
}
impl Coroutine {
    /// Cancel a suspended coroutine for good, but keep its stack until it is dropped.
    ///
    /// This does what dropping it would do to its pending operation: an operation passed to
    /// [`suspend_on_cancellable`] is dropped in place, so it gives back what it holds (a
    /// lock, a file), and the join observer sees [`Cancelled`]. The frames stay where they
    /// are and are never resumed: [`Self::resume`] stays pending from now on. A host that
    /// must let other code run before it can drop the coroutine (another coroutine of the
    /// same program that still reads its stack, say) stops it first.
    ///
    /// Stopping never suspends, even when called from another coroutine: the operation's
    /// destructor runs as if no coroutine were running, so a wait inside it (closing a file
    /// it opened, say) blocks the caller as [`task::block_on`] does, and runs to its end.
    /// Nothing can therefore cancel the caller halfway through a destructor and leave what
    /// the operation held, or this coroutine, behind.
    ///
    /// Stopping a coroutine that has not started, has finished or is already stopped does
    /// nothing. Do not call it from the coroutine itself.
    pub fn stop(&mut self) {
        if !self.control.started.get() || self.control.result.get().is_some() {
            return;
        }
        self.control.result.set(Some(Err(Cancelled)));
        if let Some(w) = self.control.waker.borrow_mut().take() {
            w.wake();
        }
        // The last suspension checked the guard on the way out, but an overflow in the
        // frames that suspended can only be seen now: stop before running a destructor on
        // a stack that may be damaged, as `resume` does.
        self.control.check_guard();
        // Opted-in bridge futures can release their state on cancellation, while the stack
        // they are pinned on is intact.
        if let Some(Pending {
            future,
            cancel: true,
        }) = self.control.pending.take()
        {
            /// Restores the running coroutine, if any, once the destructor has run.
            struct Restore(*const Control);
            impl Drop for Restore {
                fn drop(&mut self) {
                    set_current(self.0);
                }
            }
            let _restore = Restore(current());
            set_current(core::ptr::null());
            // SAFETY: the wrapper is initialized and pinned on the suspended stack. The
            // coroutine will never resume, so it cannot drop the wrapper again.
            unsafe { core::ptr::drop_in_place(future) };
        }
    }
}
impl Drop for Coroutine {
    fn drop(&mut self) {
        self.control.native_context.set(core::ptr::null());
        if self.control.started.get() && !matches!(self.control.result.get(), Some(Ok(_))) {
            // Cancels it now, or did when it was stopped.
            self.stop();
            // SAFETY: the driver is the only user of `native`, and it is being dropped,
            // so nothing resumes the coroutine again. The frames are discarded, never
            // unwound: `Coroutine::new` makes the caller keep every owner of a resource
            // out of them and every pointer to them out of the rest of the system. This
            // keeps corosensei's own drop, which would try to unwind a suspended
            // coroutine and abort without its `unwind` feature, from running on them.
            unsafe { (*self.control.native.get()).force_reset() };
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
/// Do not call it while holding a critical section: the interrupt mask is not
/// switched with the stack, so the executor would run with interrupts masked.
pub fn suspend_on<F: Future>(future: F) -> F::Output {
    suspend(future, false)
}

/// Like [`suspend_on`], but drops this future in place if the driver cancels
/// the coroutine. The future's destructor runs on the executor's stack.
pub fn suspend_on_cancellable<F: Future>(future: F) -> F::Output {
    suspend(future, true)
}

fn suspend<F: Future>(future: F, cancel: bool) -> F::Output {
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
        // occurs once polling completes. Dropping the driver discards the stack, and runs
        // this future's destructor only if it was opted in with `suspend_on_cancellable`.
        let erased: ErasedFuture = unsafe {
            core::mem::transmute::<*mut dyn Future<Output = ()>, ErasedFuture>(
                wrapper.as_mut().get_unchecked_mut(),
            )
        };
        unsafe {
            suspend_pending(
                c,
                Pending {
                    future: erased,
                    cancel,
                },
            )
        };
        // End the future's mutable borrow before reading its result.
    }
    output.expect("coroutine resumed before future completed")
}

#[inline(never)]
unsafe fn suspend_pending(control: *const Control, pending: Pending) {
    // SAFETY: the caller is on this control's coroutine stack and the pending
    // future remains pinned until completion or cancellation of that stack.
    unsafe {
        (*control).check_guard();
        (*control).pending.set(Some(pending));
        let environment = fp::Environment::read();
        (*(*control).yielder.get()).suspend(());
        environment.write();
        (*control).check_guard();
    }
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

impl Control {
    fn check_guard(&self) {
        // SAFETY: `native` owns the stack containing the guard for our lifetime.
        unsafe { check_guard_at(self.guard) };
    }
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
        (*c).check_guard();
        // The driver discards the abandoned coroutine with `force_reset`.
        (*(*c).yielder.get()).suspend(());
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

#[cfg(test)]
mod per_core_tests {
    use super::*;

    std::thread_local! {
        static SIMULATED_CORE: Cell<usize> = const { Cell::new(0) };
    }

    fn simulated_core_id() -> usize {
        SIMULATED_CORE.with(Cell::get)
    }

    fn out_of_range_core_id() -> usize {
        MAX_CORES
    }

    #[test]
    fn default_provider_uses_core_zero() {
        let table = PerCoreCurrent::new();
        assert!(table.get().is_null());
        let sentinel = 0x1000usize as *const Control;
        table.set(sentinel);
        assert_eq!(table.get(), sentinel);
        table.set(core::ptr::null());
        assert!(table.get().is_null());
    }

    #[test]
    fn isolates_current_pointer_per_core() {
        let table = PerCoreCurrent::new();
        table.set_provider(simulated_core_id);

        let core0_ptr = 0x2000usize as *const Control;
        let core1_ptr = 0x3000usize as *const Control;
        let last_core_ptr = 0x4000usize as *const Control;

        SIMULATED_CORE.with(|c| c.set(0));
        table.set(core0_ptr);

        SIMULATED_CORE.with(|c| c.set(1));
        assert!(table.get().is_null());
        table.set(core1_ptr);

        SIMULATED_CORE.with(|c| c.set(MAX_CORES - 1));
        assert!(table.get().is_null());
        table.set(last_core_ptr);

        SIMULATED_CORE.with(|c| c.set(0));
        assert_eq!(table.get(), core0_ptr);
        SIMULATED_CORE.with(|c| c.set(1));
        assert_eq!(table.get(), core1_ptr);
        SIMULATED_CORE.with(|c| c.set(MAX_CORES - 1));
        assert_eq!(table.get(), last_core_ptr);

        // Clearing all slots allows changing the provider again.
        table.set(core::ptr::null());
        SIMULATED_CORE.with(|c| c.set(1));
        table.set(core::ptr::null());
        SIMULATED_CORE.with(|c| c.set(0));
        table.set(core::ptr::null());
        table.set_provider(single_core_id);
    }

    #[test]
    #[should_panic(expected = "while a coroutine is active")]
    fn rejects_provider_change_while_coroutine_active() {
        let table = PerCoreCurrent::new();
        table.set(0x5000usize as *const Control);
        table.set_provider(simulated_core_id);
    }

    #[test]
    #[should_panic(expected = "exceeds MAX_CORES")]
    fn rejects_out_of_range_core_id() {
        let table = PerCoreCurrent::new();
        table.set_provider(out_of_range_core_id);
        let _ = table.get();
    }
}
