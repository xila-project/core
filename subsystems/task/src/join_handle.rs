use synchronization::{Arc, blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};

pub struct JoinHandle<T>(Arc<Signal<CriticalSectionRawMutex, T>>);

unsafe impl<T> Send for JoinHandle<T> {}
unsafe impl<T> Sync for JoinHandle<T> {}

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
