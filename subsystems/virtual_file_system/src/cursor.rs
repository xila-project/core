use crate::{IoErrorKind, Result};
use core::cell::RefCell;
use file_system::{Size, StateFlags};
use portable_atomic::{AtomicU8, Ordering};
use synchronization::{
    Arc,
    blocking_mutex::{Mutex, raw::CriticalSectionRawMutex},
    mutex::Mutex as AsyncMutex,
};

struct SharedDescription {
    position: AsyncMutex<CriticalSectionRawMutex, Size>,
    state: AtomicU8,
}

enum Description {
    Exclusive { position: Size, state: StateFlags },
    Shared(Arc<SharedDescription>),
}

/// Store the cursor and status flags inline until an opening is duplicated.
pub(crate) struct Cursor(Mutex<CriticalSectionRawMutex, RefCell<Description>>);
impl Cursor {
    pub const fn new(position: Size, state: StateFlags) -> Self {
        Self(Mutex::new(RefCell::new(Description::Exclusive {
            position,
            state,
        })))
    }

    pub fn duplicate(&self) -> Self {
        self.0.lock(|description| {
            let mut description = description.borrow_mut();
            let shared = match &*description {
                Description::Exclusive { position, state } => {
                    let shared = Arc::new(SharedDescription {
                        position: AsyncMutex::new(*position),
                        state: AtomicU8::new(state.bits()),
                    });
                    *description = Description::Shared(shared.clone());
                    shared
                }
                Description::Shared(shared) => shared.clone(),
            };
            Self(Mutex::new(RefCell::new(Description::Shared(shared))))
        })
    }

    pub fn state(&self) -> StateFlags {
        self.0.lock(|description| match &*description.borrow() {
            Description::Exclusive { state, .. } => *state,
            Description::Shared(shared) => {
                StateFlags::from_bits_retain(shared.state.load(Ordering::Relaxed))
            }
        })
    }

    pub fn set_state(&mut self, flags: StateFlags) {
        match self.0.get_mut().get_mut() {
            Description::Exclusive { state, .. } => *state = flags,
            Description::Shared(shared) => shared.state.store(flags.bits(), Ordering::Relaxed),
        }
    }

    fn current_position(&self) -> core::result::Result<Size, Arc<SharedDescription>> {
        self.0.lock(|description| match &*description.borrow() {
            Description::Exclusive { position, .. } => Ok(*position),
            Description::Shared(shared) => Err(shared.clone()),
        })
    }

    pub fn position(&self) -> Result<Size> {
        match self.current_position() {
            Ok(position) => Ok(position),
            Err(shared) => shared
                .position
                .try_lock()
                .map(|position| *position)
                .map_err(|_| IoErrorKind::WouldBlock),
        }
    }

    fn set_exclusive_position(&self, next: Size) {
        self.0.lock(|description| {
            // I/O operations hold the opening exclusively, including during a
            // callback, so it cannot become shared between snapshot and commit.
            let Description::Exclusive { position, .. } = &mut *description.borrow_mut() else {
                unreachable!("exclusive operation cannot overlap a duplication");
            };
            *position = next;
        });
    }

    /// Commit position only after completed I/O, releasing the lock on Pending.
    pub fn poll_update<T>(
        &self,
        operation: impl FnOnce(Size) -> core::task::Poll<Result<(T, Size)>>,
    ) -> core::task::Poll<Result<T>> {
        use core::task::Poll;
        match self.current_position() {
            Ok(value) => match operation(value) {
                Poll::Ready(Ok((result, next))) => {
                    self.set_exclusive_position(next);
                    Poll::Ready(Ok(result))
                }
                Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
                Poll::Pending => Poll::Pending,
            },
            Err(shared) => {
                let mut value = shared
                    .position
                    .try_lock()
                    .map_err(|_| IoErrorKind::WouldBlock)?;
                match operation(*value) {
                    Poll::Ready(Ok((result, next))) => {
                        *value = next;
                        Poll::Ready(Ok(result))
                    }
                    Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
                    Poll::Pending => Poll::Pending,
                }
            }
        }
    }

    pub fn update<T>(&self, operation: impl FnOnce(Size) -> Result<(T, Size)>) -> Result<T> {
        // Never hold the global critical section across device I/O. Contended
        // duplicated streams return WouldBlock without changing the cursor.
        match self.current_position() {
            Ok(value) => {
                let (result, next) = operation(value)?;
                self.set_exclusive_position(next);
                Ok(result)
            }
            Err(shared) => {
                let mut value = shared
                    .position
                    .try_lock()
                    .map_err(|_| IoErrorKind::WouldBlock)?;
                let (result, next) = operation(*value)?;
                *value = next;
                Ok(result)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    #[test]
    fn pending_does_not_commit_or_retain_shared_cursor_lock() {
        use core::task::Poll;
        let cursor = Cursor::new(7, StateFlags::empty());
        let duplicate = cursor.duplicate();
        assert_eq!(cursor.poll_update::<()>(|_| Poll::Pending), Poll::Pending);
        assert_eq!(
            duplicate
                .update(|position| Ok((position, position)))
                .unwrap(),
            7
        );
        assert_eq!(
            cursor.poll_update(|position| Poll::Ready(Ok((2, position + 2)))),
            Poll::Ready(Ok(2))
        );
        assert_eq!(
            duplicate
                .update(|position| Ok((position, position)))
                .unwrap(),
            9
        );
        duplicate
            .update(|position| {
                assert_eq!(
                    cursor.poll_update::<()>(|_| panic!("contended callback must not run")),
                    Poll::Ready(Err(IoErrorKind::WouldBlock))
                );
                Ok(((), position))
            })
            .unwrap();
    }

    #[test]
    fn io_callbacks_do_not_hold_the_global_critical_section() {
        let cursor = Cursor::new(0, StateFlags::empty());
        let duplicate = cursor.duplicate();
        cursor
            .update(|position| {
                std::thread::scope(|scope| {
                    let (sender, receiver) = std::sync::mpsc::channel();
                    scope.spawn(move || {
                        let other = Cursor::new(0, StateFlags::empty());
                        sender.send(other.update(|_| Ok(((), 1)))).unwrap();
                    });
                    assert!(
                        receiver
                            .recv_timeout(std::time::Duration::from_secs(1))
                            .unwrap()
                            .is_ok()
                    );
                });
                assert!(matches!(
                    duplicate.update(|_| Ok(((), 1))),
                    Err(IoErrorKind::WouldBlock)
                ));
                Ok(((), position + 1))
            })
            .unwrap();
        assert_eq!(
            duplicate
                .update(|position| Ok((position, position)))
                .unwrap(),
            1
        );
    }
}
