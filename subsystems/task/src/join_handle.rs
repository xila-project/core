use synchronization::{Arc, blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};

/// A result can cross threads only when it is Send.
/// ```compile_fail
/// fn require_send<T: Send>() {}
/// require_send::<task::JoinHandle<std::rc::Rc<()>>>();
/// ```
/// ```compile_fail
/// fn require_sync<T: Sync>() {}
/// require_sync::<task::JoinHandle<std::rc::Rc<()>>>();
/// ```
pub struct JoinHandle<T>(Arc<Signal<CriticalSectionRawMutex, T>>);

unsafe impl<T: Send> Send for JoinHandle<T> {}
unsafe impl<T: Send> Sync for JoinHandle<T> {}

impl<T> JoinHandle<T> {
    pub fn new() -> (Self, Self) {
        let signal = Signal::<CriticalSectionRawMutex, T>::new();

        let arc = Arc::new(signal);

        (Self(arc.clone()), Self(arc))
    }

    pub(crate) fn signal(&self, value: T) {
        self.0.signal(value);
    }

    pub async fn join(self) -> T {
        self.0.wait().await
    }

    /// Observe completion without consuming the result or waiting.
    pub fn is_finished(&self) -> bool {
        self.0.signaled()
    }
}
