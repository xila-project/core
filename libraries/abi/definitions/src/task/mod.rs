mod condition_variable;
mod mutex;
mod rwlock;
mod semaphore;

pub use condition_variable::*;
pub use mutex::*;
pub use rwlock::*;
pub use semaphore::*;

pub type XilaTaskIdentifier = usize;
