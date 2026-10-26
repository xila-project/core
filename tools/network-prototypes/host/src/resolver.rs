//! Optional bounded host DNS worker. libc/NSS lookup never runs in poll().
//! Cancelling drops the task's waker but cannot interrupt an in-flight libc
//! resolver call. The worker may finish after Resolver is dropped; production
//! shutdown/deadlines still require a decision before adopting this approach.
use crate::{Lease, Limits, map_io, reserve};
use network::{Error, IpAddress, Result};
use std::{
    future::Future,
    net::{IpAddr, ToSocketAddrs},
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::AtomicUsize,
        mpsc::{self, SyncSender},
    },
    task::{Context, Poll, Waker},
};
const DNS_STACK: usize = 128 * 1024;
const MAX_ANSWERS: usize = 16;
struct State {
    result: Option<Result<Vec<IpAddress>>>,
    waker: Option<Waker>,
    cancelled: bool,
}
struct Job {
    name: String,
    state: Arc<Mutex<State>>,
    _lease: Lease,
}

pub struct Resolver {
    sender: SyncSender<Job>,
    limits: Arc<Limits>,
}
impl Resolver {
    pub fn new(maximum_pending: usize) -> Result<Self> {
        if maximum_pending == 0 {
            return Err(Error::InvalidInput);
        }
        let (sender, receiver) = mpsc::sync_channel::<Job>(maximum_pending);
        std::thread::Builder::new()
            .name("xila-probe-dns".into())
            .stack_size(DNS_STACK)
            .spawn(move || {
                while let Ok(job) = receiver.recv() {
                    if job.state.lock().unwrap().cancelled {
                        continue;
                    }
                    let result = (job.name.as_str(), 0)
                        .to_socket_addrs()
                        .map(|addresses| {
                            addresses
                                .take(MAX_ANSWERS)
                                .map(|a| match a.ip() {
                                    IpAddr::V4(ip) => IpAddress::from(ip.octets()),
                                    IpAddr::V6(ip) => IpAddress::from(ip.octets()),
                                })
                                .collect()
                        })
                        .map_err(map_io);
                    // Release the pending-job slot before waking the consumer,
                    // which may immediately submit its next lookup.
                    drop(job._lease);
                    let waker = {
                        let mut state = job.state.lock().unwrap();
                        if state.cancelled {
                            continue;
                        }
                        state.result = Some(result);
                        state.waker.take()
                    };
                    if let Some(waker) = waker {
                        waker.wake();
                    }
                }
            })
            .map_err(map_io)?;
        Ok(Self {
            sender,
            limits: Arc::new(Limits {
                maximum: maximum_pending,
                active: AtomicUsize::new(0),
            }),
        })
    }
    pub async fn lookup(&self, name: &str) -> Result<Vec<IpAddress>> {
        if name.is_empty() || name.len() > 253 || name.contains('\0') {
            return Err(Error::InvalidInput);
        }
        let lease = reserve(&self.limits)?;
        let state = Arc::new(Mutex::new(State {
            result: None,
            waker: None,
            cancelled: false,
        }));
        self.sender
            .try_send(Job {
                name: name.into(),
                state: state.clone(),
                _lease: lease,
            })
            .map_err(|_| Error::ResourceBusy)?;
        Query(state).await
    }
}
struct Query(Arc<Mutex<State>>);
impl Future for Query {
    type Output = Result<Vec<IpAddress>>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.0.lock().unwrap();
        if let Some(result) = state.result.take() {
            return Poll::Ready(result);
        }
        if !state
            .waker
            .as_ref()
            .is_some_and(|w| w.will_wake(cx.waker()))
        {
            state.waker = Some(cx.waker().clone());
        }
        Poll::Pending
    }
}
impl Drop for Query {
    fn drop(&mut self) {
        let mut state = self.0.lock().unwrap();
        state.cancelled = true;
        state.waker = None;
    }
}
