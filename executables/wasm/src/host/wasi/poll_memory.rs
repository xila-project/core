//! Portable clock subscription/event marshalling.

use crate::host::translation::{
    GuestMemory, GuestPointer, GuestSlice, GuestValue, WasmUsize, sealed::Sealed,
};
use alloc::vec::Vec;
use core::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum PollError {
    Fault = 21,
    Invalid = 28,
    Io = 29,
    NotSupported = 58,
    Overflow = 61,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Subscription {
    pub userdata: u64,
    pub tag: u8,
    pub clock: u32,
    pub timeout: u64,
    pub precision: u64,
    pub flags: u16,
}
impl Sealed for Subscription {}
impl GuestValue for Subscription {
    const SIZE: usize = 48;
    fn decode(bytes: &[u8]) -> Self {
        Self {
            userdata: u64::decode(&bytes[..8]),
            tag: bytes[8],
            clock: u32::decode(&bytes[16..20]),
            timeout: u64::decode(&bytes[24..32]),
            precision: u64::decode(&bytes[32..40]),
            flags: u16::decode(&bytes[40..42]),
        }
    }
    fn encode(self, bytes: &mut [u8]) {
        bytes.fill(0);
        self.userdata.encode(&mut bytes[..8]);
        bytes[8] = self.tag;
        self.clock.encode(&mut bytes[16..20]);
        self.timeout.encode(&mut bytes[24..32]);
        self.precision.encode(&mut bytes[32..40]);
        self.flags.encode(&mut bytes[40..42]);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PollEvent {
    pub userdata: u64,
}
impl Sealed for PollEvent {}
impl GuestValue for PollEvent {
    const SIZE: usize = 32;
    fn decode(bytes: &[u8]) -> Self {
        Self {
            userdata: u64::decode(&bytes[..8]),
        }
    }
    fn encode(self, bytes: &mut [u8]) {
        bytes.fill(0);
        self.userdata.encode(&mut bytes[..8]);
    }
}

#[derive(Debug)]
struct Clock {
    userdata: u64,
    id: usize,
    deadline: u64,
}

#[derive(Debug)]
pub struct ClockPoll {
    clocks: Vec<Clock>,
    events: GuestSlice<PollEvent>,
    written: GuestPointer<WasmUsize>,
}

impl ClockPoll {
    pub fn prepare(
        memory: &GuestMemory<'_>,
        input: WasmUsize,
        events: WasmUsize,
        count: WasmUsize,
        written: WasmUsize,
        mut now: impl FnMut(usize) -> Result<u64, PollError>,
    ) -> Result<Self, PollError> {
        if count == 0 || count > 64 {
            return Err(PollError::Invalid);
        }
        let events = GuestSlice::new(events, count);
        let written = GuestPointer::new(written);
        memory.validate_slice(events).ok_or(PollError::Fault)?;
        memory.validate_pointer(written).ok_or(PollError::Fault)?;
        let subscriptions = memory
            .values(GuestSlice::<Subscription>::new(input, count))
            .ok_or(PollError::Fault)?;
        let mut clocks = Vec::with_capacity(count as usize);
        let mut readings = [None; 2];
        for subscription in subscriptions {
            if subscription.tag != 0 || subscription.clock > 1 {
                return Err(PollError::NotSupported);
            }
            if subscription.flags > 1 {
                return Err(PollError::Invalid);
            }
            let id = subscription.clock as usize;
            let deadline = if subscription.flags == 1 {
                subscription.timeout
            } else {
                let current = match readings[id] {
                    Some(current) => current,
                    None => {
                        let current = now(id)?;
                        readings[id] = Some(current);
                        current
                    }
                };
                current
                    .checked_add(subscription.timeout)
                    .ok_or(PollError::Overflow)?
            };
            clocks.push(Clock {
                userdata: subscription.userdata,
                id,
                deadline,
            });
        }
        Ok(Self {
            clocks,
            events,
            written,
        })
    }

    /// Clocks used by this poll, including absolute subscriptions.
    pub fn required_clocks(&self) -> [bool; 2] {
        let mut required = [false; 2];
        for clock in &self.clocks {
            required[clock.id] = true;
        }
        required
    }

    pub fn delay(&self, now: [u64; 2]) -> Duration {
        Duration::from_nanos(
            self.clocks
                .iter()
                .map(|clock| clock.deadline.saturating_sub(now[clock.id]))
                .min()
                .expect("at least one subscription"),
        )
    }

    pub fn complete(
        &self,
        memory: &mut GuestMemory<'_>,
        now: [u64; 2],
    ) -> Result<usize, PollError> {
        // Revalidate after suspension; memory may have been replaced by an embedder.
        memory.validate_slice(self.events).ok_or(PollError::Fault)?;
        memory
            .validate_pointer(self.written)
            .ok_or(PollError::Fault)?;
        let mut count = 0;
        for clock in &self.clocks {
            if clock.deadline > now[clock.id] {
                continue;
            }
            let pointer = self
                .events
                .pointer_at(count as WasmUsize)
                .expect("bounded ready count");
            memory
                .write(
                    pointer,
                    PollEvent {
                        userdata: clock.userdata,
                    },
                )
                .expect("validated event range");
            count += 1;
        }
        memory
            .write(self.written, count as WasmUsize)
            .expect("validated count range");
        Ok(count)
    }
}
