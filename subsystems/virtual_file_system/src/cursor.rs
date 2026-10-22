use crate::{Error, Result};
use core::cell::RefCell;
use file_system::Size;
use synchronization::{
    Arc,
    blocking_mutex::{Mutex, raw::CriticalSectionRawMutex},
    mutex::Mutex as AsyncMutex,
};

type SharedPosition = Arc<AsyncMutex<CriticalSectionRawMutex, Size>>;
#[derive(Clone)]
enum Position {
    Exclusive(Size),
    Shared(SharedPosition),
}

/// Allocate shared position storage only when a file is actually duplicated.
pub(crate) struct Cursor(Mutex<CriticalSectionRawMutex, RefCell<Position>>);
impl Cursor {
    pub const fn new(position: Size) -> Self {
        Self(Mutex::new(RefCell::new(Position::Exclusive(position))))
    }
    pub fn duplicate(&self) -> Self {
        self.0.lock(|position| {
            let mut position = position.borrow_mut();
            if let Position::Exclusive(value) = *position {
                *position = Position::Shared(Arc::new(AsyncMutex::new(value)));
            }
            Self(Mutex::new(RefCell::new(position.clone())))
        })
    }
    /// Commit position only after completed I/O, releasing the lock on Pending.
    pub fn poll_update<T>(
        &self,
        operation: impl FnOnce(Size) -> core::task::Poll<Result<(T, Size)>>,
    ) -> core::task::Poll<Result<T>> {
        use core::task::Poll;
        let position = self.0.lock(|position| position.borrow().clone());
        match position {
            Position::Exclusive(value) => match operation(value) {
                Poll::Ready(Ok((result, next))) => {
                    self.0
                        .lock(|position| *position.borrow_mut() = Position::Exclusive(next));
                    Poll::Ready(Ok(result))
                }
                Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
                Poll::Pending => Poll::Pending,
            },
            Position::Shared(shared) => {
                let mut value = shared.try_lock().map_err(|_| Error::RessourceBusy)?;
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
        // duplicated streams return ResourceBusy without changing the cursor.
        let position = self.0.lock(|position| position.borrow().clone());
        match position {
            Position::Exclusive(value) => {
                let (result, next) = operation(value)?;
                self.0
                    .lock(|position| *position.borrow_mut() = Position::Exclusive(next));
                Ok(result)
            }
            Position::Shared(shared) => {
                let mut value = shared.try_lock().map_err(|_| Error::RessourceBusy)?;
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
        let cursor = Cursor::new(7);
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
                    Poll::Ready(Err(Error::RessourceBusy))
                );
                Ok(((), position))
            })
            .unwrap();
    }

    #[test]
    fn io_callbacks_do_not_hold_the_global_critical_section() {
        let cursor = Cursor::new(0);
        let duplicate = cursor.duplicate();
        cursor
            .update(|position| {
                std::thread::scope(|scope| {
                    let (sender, receiver) = std::sync::mpsc::channel();
                    scope.spawn(move || {
                        let other = Cursor::new(0);
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
                    Err(Error::RessourceBusy)
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
