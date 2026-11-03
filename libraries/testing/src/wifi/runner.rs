use super::controller::{Pending, Shared};
use embassy_futures::select::{Either, select};
use network::WifiConnectionErrorKind;
use synchronization::Arc;

/// Consumed by run, so cancelling its task also marks the controller unavailable.
pub struct MockWifiRunner {
    pub(super) shared: Arc<Shared>,
}
impl MockWifiRunner {
    pub async fn run(self) -> ! {
        loop {
            let pending = self.shared.state.lock(|state| {
                let state = state.borrow();
                state.pending.as_ref().map(|pending| {
                    (
                        state.revision,
                        match pending {
                            Pending::Scan(_) => state.configuration.scan_latency,
                            Pending::Connect(_) | Pending::Disconnect => {
                                state.configuration.connect_latency
                            }
                        },
                    )
                })
            });
            if let Some((revision, delay)) = pending {
                if let Either::First(()) = select(task::sleep(delay), self.shared.wake.wait()).await
                {
                    self.shared.state.lock(|state| {
                        let mut state = state.borrow_mut();
                        if state.revision == revision {
                            state.complete();
                        }
                    });
                }
            } else {
                self.shared.wake.wait().await;
            }
        }
    }
}
impl Drop for MockWifiRunner {
    fn drop(&mut self) {
        self.shared.state.lock(|state| {
            let mut state = state.borrow_mut();
            state.cancel();
            state.alive = false;
            state.error = WifiConnectionErrorKind::DRIVER;
        });
    }
}
