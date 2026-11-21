#![allow(
    clippy::disallowed_types,
    reason = "host-only test counts allocations in a thread-local over the system allocator"
)]

//! Cancelling a started coroutine frees its stack, and only runs the destructors
//! it opted in to. The allocator counts the live bytes of the calling thread, so the
//! test harness's other threads cannot disturb the counts.

use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};
use stackful::{Coroutine, Stack, suspend_on, suspend_on_cancellable};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    rc::Rc,
};

struct Counting;

std::thread_local! {
    /// Bytes allocated and not yet freed by this thread, as far as it has seen. Constant
    /// initialization means reading it never allocates.
    static LIVE: Cell<isize> = const { Cell::new(0) };
}

// SAFETY: every call forwards to the system allocator with the same arguments.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.with(|live| live.set(live.get() + layout.size() as isize));
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE.with(|live| live.set(live.get() - layout.size() as isize));
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

struct DropCount(Rc<Cell<usize>>);

impl Drop for DropCount {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

fn live() -> isize {
    LIVE.with(Cell::get)
}

const STACK: usize = 64 * 1024;

/// A coroutine suspended in `suspend`, with the join observer and the allocation count
/// measured before it existed.
fn suspended(suspend: fn(DropCount), drops: &Rc<Cell<usize>>) -> (Coroutine, isize) {
    let before = live();
    let resource = DropCount(drops.clone());
    // SAFETY: the bounded entry fits the stack and never unwinds.
    let mut coroutine = unsafe {
        Coroutine::new(Stack::new(STACK), move || {
            suspend(resource);
            0
        })
    };
    let mut context = Context::from_waker(Waker::noop());
    assert_eq!(coroutine.resume(&mut context), Poll::Pending);
    assert!(live() - before >= STACK as isize, "the stack is allocated");
    (coroutine, before)
}

#[test]
fn cancelling_a_started_coroutine_frees_its_stack() {
    // A coroutine whose pending future is opted in: the future's destructor runs once,
    // on the executor, and the stack goes with it.
    let drops = Rc::new(Cell::new(0));
    let (coroutine, before) = suspended(
        |resource| {
            suspend_on_cancellable(async move {
                core::future::pending::<()>().await;
                drop(resource);
            });
        },
        &drops,
    );
    drop(coroutine);
    assert_eq!(drops.get(), 1, "the opted-in future was dropped in place");
    assert_eq!(live(), before, "the stack and the control block were freed");

    // A pending future that is not opted in is discarded with the stack, without running
    // its destructor: the frames are never unwound. The memory it owns is the caller's to
    // keep elsewhere, as `Coroutine::new` documents.
    let drops = Rc::new(Cell::new(0));
    let (coroutine, before) = suspended(
        |resource| {
            suspend_on(async move {
                core::future::pending::<()>().await;
                drop(resource);
            });
        },
        &drops,
    );
    drop(coroutine);
    assert_eq!(drops.get(), 0, "no destructor runs on the discarded frames");
    assert_eq!(
        Rc::strong_count(&drops),
        2,
        "the resource in the frames is leaked, not released"
    );
    assert_eq!(live(), before, "the stack was freed all the same");

    // The join observer is woken with `Cancelled`, and the allocation lives until the
    // observer is gone.
    let drops = Rc::new(Cell::new(0));
    let (coroutine, before) = suspended(
        |resource| {
            suspend_on(async move {
                core::future::pending::<()>().await;
                drop(resource);
            });
        },
        &drops,
    );
    let mut join = Box::pin(coroutine.join_handle());
    let mut context = Context::from_waker(Waker::noop());
    assert_eq!(Pin::new(&mut join).poll(&mut context), Poll::Pending);
    drop(coroutine);
    assert_eq!(
        Pin::new(&mut join).poll(&mut context),
        Poll::Ready(Err(stackful::Cancelled))
    );
    drop(join);
    assert_eq!(live(), before, "the last observer released the allocation");
}
