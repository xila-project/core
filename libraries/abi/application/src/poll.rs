use crate::{File, Status};
use alloc::vec::Vec;
use core::{
    ffi::c_void,
    future::{Future, poll_fn},
    pin::{Pin, pin},
    slice,
    task::{Context, Poll, Waker},
};
use embassy_time::{Instant, Timer};
use file_system::{IoErrorKind, ReadinessWaiter, ReadyEvents};
use virtual_file_system::FileReadiness;

pub const XILA_POLL_IGNORE: u32 = 0;
pub const XILA_POLL_FILE: u32 = 1;
pub const XILA_POLL_DIRECTORY: u32 = 2;
pub const XILA_POLL_TCP: u32 = 3;
pub const XILA_POLL_TCP_LISTENER: u32 = 4;
pub const XILA_POLL_UDP: u32 = 5;

pub const XILA_POLL_IN: u16 = 1;
pub const XILA_POLL_OUT: u16 = 2;
pub const XILA_POLL_ERR: u16 = 4;
pub const XILA_POLL_HUP: u16 = 8;
pub const XILA_POLL_NVAL: u16 = 16;

/// One poll entry. Only IN and OUT are requested. ERR, HUP and NVAL are
/// reported regardless of the request. IGNORE entries always return zero.
/// A null file/directory/socket or unknown kind reports NVAL.
#[repr(C)]
pub struct PollEntry {
    /// Opaque object of the type named by kind.
    pub handle: *const c_void,
    pub kind: u32,
    pub events: u16,
    pub revents: u16,
}

enum Source<'a> {
    Ignore,
    Invalid,
    File(FileReadiness),
    Directory,
    #[cfg(feature = "network")]
    Tcp(&'a ::network::TcpSocket, bool),
    #[cfg(feature = "network")]
    Udp(&'a crate::Udp),
    // Keep the same shape in network-off builds without retaining socket data.
    #[cfg(not(feature = "network"))]
    Disabled(core::marker::PhantomData<&'a ()>),
}

impl<'a> Source<'a> {
    // SAFETY: callers supply the correct live object for the tag.
    unsafe fn from_entry(entry: &PollEntry) -> Self {
        if entry.kind == XILA_POLL_IGNORE {
            return Self::Ignore;
        }
        if entry.handle.is_null() {
            return Self::Invalid;
        }
        match entry.kind {
            XILA_POLL_FILE => {
                Self::File(unsafe { &(*entry.handle.cast::<File>()).readiness }.clone())
            }
            XILA_POLL_DIRECTORY => Self::Directory,
            #[cfg(feature = "network")]
            XILA_POLL_TCP => Self::Tcp(
                unsafe { &(*entry.handle.cast::<crate::Tcp>()).inner },
                false,
            ),
            #[cfg(feature = "network")]
            XILA_POLL_TCP_LISTENER => Self::Tcp(
                unsafe { &(*entry.handle.cast::<crate::TcpListener>()).inner },
                true,
            ),
            #[cfg(feature = "network")]
            XILA_POLL_UDP => Self::Udp(unsafe { &*entry.handle.cast() }),
            #[cfg(not(feature = "network"))]
            XILA_POLL_TCP | XILA_POLL_TCP_LISTENER | XILA_POLL_UDP => {
                Self::Disabled(core::marker::PhantomData)
            }
            _ => Self::Invalid,
        }
    }

    fn events(&self, _cx: &mut Context<'_>, wanted: u16) -> u16 {
        let interests = ReadyEvents::from_bits_truncate(wanted & (XILA_POLL_IN | XILA_POLL_OUT));
        let result = match self {
            Self::Ignore => return 0,
            Self::Invalid => return XILA_POLL_NVAL,
            Self::Directory => return wanted & XILA_POLL_IN,
            Self::File(file) => file.events(interests),
            #[cfg(not(feature = "network"))]
            Self::Disabled(_) => return XILA_POLL_NVAL,
            #[cfg(feature = "network")]
            Self::Tcp(socket, listener) => {
                return crate::network::tcp_readiness(socket, _cx, *listener, wanted);
            }
            #[cfg(feature = "network")]
            Self::Udp(socket) => return crate::network::udp_readiness(socket, _cx, wanted),
        };
        match result {
            Ok(events) => events.bits() & (wanted | XILA_POLL_ERR | XILA_POLL_HUP),
            Err(IoErrorKind::BadDescriptor) => XILA_POLL_NVAL,
            Err(_) => XILA_POLL_ERR,
        }
    }
}

/// Wait for the first ready handle, returning every ready entry in that scan.
/// Timeout is in milliseconds, negative means infinite, and zero is a probe.
/// A blocking wait outside a stackful coroutine returns WouldBlock when no
/// handles are immediately ready.
/// # Safety
/// Entries must be aligned, writable and disjoint from ready_count, which
/// must be aligned and writable. Non-null handles must have the indicated type.
/// Files and directories must be live on entry. Sockets must stay live and
/// concurrent calls on them must be limited to other poll waits.
pub unsafe extern "C" fn xila_poll_wait(
    entries: *mut PollEntry,
    count: usize,
    timeout_ms: i32,
    ready_count: *mut usize,
) -> Status {
    if ready_count.is_null()
        || (count != 0 && entries.is_null())
        || count > isize::MAX as usize / size_of::<PollEntry>()
    {
        return Status::InvalidArgument;
    }
    let entries = if count == 0 {
        &mut []
    } else {
        unsafe { slice::from_raw_parts_mut(entries, count) }
    };

    // No snapshot or waker allocation for the immediate probe.
    let mut ready = 0;
    if timeout_ms == 0 {
        let mut cx = Context::from_waker(Waker::noop());
        for entry in entries {
            entry.revents = unsafe { Source::from_entry(entry) }.events(&mut cx, entry.events);
            ready += usize::from(entry.revents != 0);
        }
    } else {
        let mut sources = Vec::new();
        if sources.try_reserve_exact(count).is_err() {
            return Status::OutOfMemory;
        }
        for entry in entries.iter() {
            sources.push((unsafe { Source::from_entry(entry) }, 0));
        }
        let deadline = (timeout_ms > 0)
            .then(|| task::deadline_after_nanoseconds(timeout_ms as u64 * 1_000_000));

        let result = if crate::is_in_coroutine() {
            crate::suspend_on_cancellable(wait(entries, sources, deadline))
        } else {
            let ready = scan(
                entries,
                &mut sources,
                &mut Context::from_waker(Waker::noop()),
            );
            if ready == 0 {
                Err(Status::WouldBlock)
            } else {
                publish(entries, &sources);
                Ok(ready)
            }
        };
        match result {
            Ok(count) => ready = count,
            Err(error) => return error,
        }
    }
    unsafe { ready_count.write(ready) };
    Status::Ok
}

fn scan(entries: &[PollEntry], sources: &mut [(Source<'_>, u16)], cx: &mut Context<'_>) -> usize {
    let mut ready = 0;
    for (entry, (source, events)) in entries.iter().zip(sources.iter_mut()) {
        *events = source.events(cx, entry.events);
        ready += usize::from(*events != 0);
    }
    ready
}

fn publish(entries: &mut [PollEntry], sources: &[(Source<'_>, u16)]) {
    for (entry, (_, events)) in entries.iter_mut().zip(sources) {
        entry.revents = *events;
    }
}

async fn wait(
    entries: &mut [PollEntry],
    mut sources: Vec<(Source<'_>, u16)>,
    deadline: Option<Instant>,
) -> Result<usize, Status> {
    let waiter = pin!(ReadinessWaiter::default());
    let mut timer = deadline.map(Timer::at);
    poll_fn(|cx| {
        if !sources.is_empty() {
            waiter.as_ref().register(cx.waker())?;
        }
        let ready = scan(entries, &mut sources, cx);
        if ready != 0
            || timer
                .as_mut()
                .is_some_and(|timer| Pin::new(timer).poll(cx).is_ready())
        {
            publish(entries, &sources);
            Poll::Ready(Ok(ready))
        } else {
            Poll::Pending
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr::null_mut;

    #[test]
    fn poll_distinguishes_invalid_and_ignored_entries() {
        let mut entries = [
            PollEntry {
                handle: null_mut(),
                kind: XILA_POLL_FILE,
                events: 0,
                revents: 0,
            },
            PollEntry {
                handle: null_mut(),
                kind: XILA_POLL_IGNORE,
                events: XILA_POLL_IN,
                revents: u16::MAX,
            },
        ];
        let mut ready = 17;
        assert_eq!(
            unsafe { xila_poll_wait(entries.as_mut_ptr(), entries.len(), 0, &mut ready) },
            Status::Ok,
        );
        assert_eq!(ready, 1);
        assert_eq!(entries[0].revents, XILA_POLL_NVAL);
        assert_eq!(entries[1].revents, 0);
    }

    #[test]
    fn synchronous_wait_has_no_side_effects_when_it_would_block() {
        let mut entry = PollEntry {
            handle: null_mut(),
            kind: XILA_POLL_IGNORE,
            events: 0,
            revents: u16::MAX,
        };
        let mut ready = 17;
        assert_eq!(
            unsafe { xila_poll_wait(&mut entry, 1, -1, &mut ready) },
            Status::WouldBlock
        );
        assert_eq!(ready, 17);
        assert_eq!(entry.revents, u16::MAX);
        assert_eq!(
            unsafe { xila_poll_wait(null_mut(), 1, 0, &mut ready) },
            Status::InvalidArgument
        );
        assert_eq!(ready, 17);
    }
}
