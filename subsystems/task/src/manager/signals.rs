// Signals module - handles task signal management

use super::*;

impl Manager {
    pub async fn send_signal(&self, task_identifier: TaskIdentifier, signal: Signal) -> Result<()> {
        Self::get_task_mutable(&mut *self.0.write().await, task_identifier)
            .map(|task| task.signals.send(signal))
    }

    pub async fn pop_signal(&self, task_identifier: TaskIdentifier) -> Result<Option<Signal>> {
        Self::get_task_mutable(&mut *self.0.write().await, task_identifier)
            .map(|task| task.signals.pop())
    }

    pub async fn peek_signal(&self, task_identifier: TaskIdentifier) -> Result<Option<Signal>> {
        Self::get_task(&*self.0.read().await, task_identifier).map(|task| task.signals.peek())
    }
}
