extern crate std;
use super::*;
use core::{
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context as TaskContext, Poll, Waker},
};
use std::sync::Arc;
use std::task::Wake;

#[derive(Default)]
struct Counter(AtomicUsize);
impl Wake for Counter {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
fn counter() -> (Arc<Counter>, Waker) {
    let count = Arc::new(Counter::default());
    let waker = Waker::from(count.clone());
    (count, waker)
}

#[test]
fn empty_read_waits_for_write_without_self_waking() {
    let pipe = Pipe::new(1);
    let mut reader = pipe.open_context(AccessFlags::Read).unwrap();
    let mut writer = pipe.open_context(AccessFlags::Write).unwrap();
    let (count, waker) = counter();
    let mut cx = TaskContext::from_waker(&waker);
    for _ in 0..3 {
        assert_eq!(
            BaseOperations::poll_read(&pipe, &mut reader, &mut cx, &mut [0], 0),
            Poll::Pending
        );
    }
    assert_eq!(count.0.load(Ordering::SeqCst), 0);
    assert_eq!(BaseOperations::write(&pipe, &mut writer, b"a", 0), Ok(1));
    assert_eq!(count.0.load(Ordering::SeqCst), 1);
    let mut out = [0];
    assert_eq!(
        BaseOperations::poll_read(&pipe, &mut reader, &mut cx, &mut out, 0),
        Poll::Ready(Ok(1))
    );
    assert_eq!(&out, b"a");
}

#[test]
fn full_write_waits_for_space_and_last_reader_close() {
    let pipe = Pipe::new(1);
    let mut reader = pipe.open_context(AccessFlags::Read).unwrap();
    let mut writer = pipe.open_context(AccessFlags::Write).unwrap();
    let (count, waker) = counter();
    let mut cx = TaskContext::from_waker(&waker);
    assert_eq!(BaseOperations::write(&pipe, &mut writer, b"a", 0), Ok(1));
    assert_eq!(
        BaseOperations::poll_write(&pipe, &mut writer, &mut cx, b"b", 0),
        Poll::Pending
    );
    assert_eq!(BaseOperations::read(&pipe, &mut reader, &mut [0], 0), Ok(1));
    assert_eq!(count.0.load(Ordering::SeqCst), 1);
    assert_eq!(
        BaseOperations::poll_write(&pipe, &mut writer, &mut cx, b"b", 0),
        Poll::Ready(Ok(1))
    );
    assert_eq!(
        BaseOperations::poll_write(&pipe, &mut writer, &mut cx, b"c", 0),
        Poll::Pending
    );
    pipe.close(&mut reader).unwrap();
    assert_eq!(count.0.load(Ordering::SeqCst), 2);
    assert_eq!(
        BaseOperations::poll_write(&pipe, &mut writer, &mut cx, b"c", 0),
        Poll::Ready(Err(Error::InputOutput))
    );
}

#[test]
fn independent_waiters_replacement_cancellation_and_eof() {
    let pipe = Pipe::new(1);
    let mut first = pipe.open_context(AccessFlags::Read).unwrap();
    let mut second = pipe.clone_context(&first).unwrap();
    let mut writer = pipe.open_context(AccessFlags::Write).unwrap();
    let (old, old_waker) = counter();
    let (new, new_waker) = counter();
    let mut old_cx = TaskContext::from_waker(&old_waker);
    let mut new_cx = TaskContext::from_waker(&new_waker);
    assert!(BaseOperations::poll_read(&pipe, &mut first, &mut old_cx, &mut [0], 0).is_pending());
    assert!(BaseOperations::poll_read(&pipe, &mut first, &mut new_cx, &mut [0], 0).is_pending());
    assert!(BaseOperations::poll_read(&pipe, &mut second, &mut old_cx, &mut [0], 0).is_pending());
    pipe.cancel_operation(&mut second, file_system::Operation::Read);
    pipe.close(&mut second).unwrap();
    let mut reused = pipe.open_context(AccessFlags::Read).unwrap();
    assert_eq!(BaseOperations::write(&pipe, &mut writer, b"x", 0), Ok(1));
    assert_eq!(old.0.load(Ordering::SeqCst), 0);
    assert_eq!(new.0.load(Ordering::SeqCst), 1);
    pipe.close(&mut writer).unwrap();
    assert_eq!(BaseOperations::read(&pipe, &mut reused, &mut [0], 0), Ok(1));
    assert_eq!(
        BaseOperations::poll_read(&pipe, &mut first, &mut new_cx, &mut [0], 0),
        Poll::Ready(Ok(0))
    );
}

#[test]
fn opening_capacity_is_explicit_and_slots_are_reused() {
    let pipe = Pipe::new(1);
    let mut contexts = std::vec::Vec::new();
    for _ in 0..MAX_PIPE_OPENINGS {
        contexts.push(pipe.open_context(AccessFlags::Read).unwrap());
    }
    assert!(matches!(
        pipe.open_context(AccessFlags::Write),
        Err(Error::TooManyOpenFiles)
    ));
    assert!(matches!(
        pipe.clone_context(&contexts[0]),
        Err(Error::TooManyOpenFiles)
    ));
    pipe.close(&mut contexts.pop().unwrap()).unwrap();
    let mut replacement = pipe.open_context(AccessFlags::Write).unwrap();
    pipe.close(&mut replacement).unwrap();
    for context in &mut contexts {
        pipe.close(context).unwrap();
    }
}

#[test]
fn data_before_registration_and_zero_length_io_complete_immediately() {
    let pipe = Pipe::new(1);
    let mut reader = pipe.open_context(AccessFlags::Read).unwrap();
    let mut writer = pipe.open_context(AccessFlags::Write).unwrap();
    let (count, waker) = counter();
    let mut cx = TaskContext::from_waker(&waker);
    assert_eq!(
        BaseOperations::poll_read(&pipe, &mut reader, &mut cx, &mut [], 0),
        Poll::Ready(Ok(0))
    );
    assert_eq!(BaseOperations::write(&pipe, &mut writer, b"x", 0), Ok(1));
    assert_eq!(
        BaseOperations::poll_read(&pipe, &mut reader, &mut cx, &mut [0], 0),
        Poll::Ready(Ok(1))
    );
    assert_eq!(count.0.load(Ordering::SeqCst), 0);
}

struct Reentrant {
    pipe: Arc<Pipe>,
    calls: Arc<Counter>,
}
impl Reentrant {
    fn check_unlocked(&self) {
        self.pipe
            .state
            .lock(|state| assert!(state.try_borrow_mut().is_ok()));
        self.calls.0.fetch_add(1, Ordering::SeqCst);
    }
}
impl Wake for Reentrant {
    fn wake(self: Arc<Self>) {
        self.check_unlocked();
    }
}
impl Drop for Reentrant {
    fn drop(&mut self) {
        self.check_unlocked();
    }
}

#[test]
fn wake_and_replaced_cancelled_waker_drop_run_outside_lock() {
    let pipe = Arc::new(Pipe::new(1));
    let mut reader = pipe.open_context(AccessFlags::Read).unwrap();
    let mut writer = pipe.open_context(AccessFlags::Write).unwrap();
    let calls = Arc::new(Counter::default());
    let make_waker = || {
        Waker::from(Arc::new(Reentrant {
            pipe: pipe.clone(),
            calls: calls.clone(),
        }))
    };
    {
        let waker = make_waker();
        let mut cx = TaskContext::from_waker(&waker);
        assert!(BaseOperations::poll_read(&*pipe, &mut reader, &mut cx, &mut [0], 0).is_pending());
    } // The registration now owns the last reference to this waker.
    {
        let waker = make_waker();
        let mut cx = TaskContext::from_waker(&waker);
        assert!(BaseOperations::poll_read(&*pipe, &mut reader, &mut cx, &mut [0], 0).is_pending());
    }
    assert_eq!(calls.0.load(Ordering::SeqCst), 1); // Replacement dropped old waker.
    BaseOperations::write(&*pipe, &mut writer, b"x", 0).unwrap();
    assert_eq!(calls.0.load(Ordering::SeqCst), 3); // Wake and final drop.
    BaseOperations::read(&*pipe, &mut reader, &mut [0], 0).unwrap();
    {
        let waker = make_waker();
        let mut cx = TaskContext::from_waker(&waker);
        assert!(BaseOperations::poll_read(&*pipe, &mut reader, &mut cx, &mut [0], 0).is_pending());
    }
    pipe.cancel_operation(&mut reader, file_system::Operation::Read);
    assert_eq!(calls.0.load(Ordering::SeqCst), 4);
    pipe.close(&mut reader).unwrap();
    pipe.close(&mut writer).unwrap();
}
