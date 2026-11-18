use alloc::collections::BTreeMap;
use synchronization::Arc;

mod condition_variable;
mod mutex;
mod rwlock;
mod semaphore;

pub use condition_variable::*;
pub use mutex::*;
pub use rwlock::*;
pub use semaphore::*;

pub type XilaTaskIdentifier = usize;

struct HandleRegistry<Value> {
    values: BTreeMap<usize, Arc<Value>>,
    next_handle: usize,
}

impl<Value> HandleRegistry<Value> {
    const fn new() -> Self {
        Self {
            values: BTreeMap::new(),
            next_handle: 1,
        }
    }

    fn insert(&mut self, value: Arc<Value>) -> Option<usize> {
        let handle = self.next_handle;
        self.next_handle = handle.checked_add(1)?;
        self.values.insert(handle, value);
        Some(handle)
    }

    fn get(&self, handle: usize) -> Option<Arc<Value>> {
        self.values.get(&handle).cloned()
    }

    fn remove(&mut self, handle: usize) -> Option<Arc<Value>> {
        self.values.remove(&handle)
    }
}
