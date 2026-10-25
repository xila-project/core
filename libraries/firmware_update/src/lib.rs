#![no_std]
//! Allocation-free transaction ownership. Platform boot policy belongs to backends.

#[cfg(feature = "device")]
extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "device")]
pub mod device;
#[cfg(feature = "simulator")]
pub mod nor;
#[cfg(feature = "simulator")]
pub mod simulator;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidState,
    Length,
    Encoding,
    Authentication,
    Target,
    SecurityVersion,
    Storage,
    Geometry,
    Unsupported,
    Recovery,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum State {
    Idle,
    Receiving,
    Verified,
    PendingBoot,
    Failed,
}

/// Guarantees are independent: authenticated update does not imply verified boot
/// or a hardware root. Unknown bits must not be interpreted as stronger trust.
pub const AUTHENTICATED_UPDATE: u32 = 1;
pub const FUNCTIONAL_REVERT: u32 = 2;
pub const VERIFIED_BOOT: u32 = 4;
pub const HARDWARE_ROOT: u32 = 8;
#[derive(Debug, Clone, Copy)]
pub struct Capabilities {
    pub max_image_size: u64,
    pub max_chunk_size: u32,
    pub guarantees: u32,
}

/// Implementations own image format, trust roots, staging geometry and persistent
/// boot state. Never return success before accepted bytes are durable according
/// to the platform contract. `finish` authenticates readback; its nonforgeable
/// proof must bind the exact staging generation. `activate` must reject stale
/// proofs and mutations since verification. Abort must never clear pending boot.
///
/// No VFS, allocator, transport, scheduler or signing key is required here.
pub trait FirmwareBackend {
    type Verified;
    fn capabilities(&self) -> Capabilities;
    fn begin(&mut self, size: u64, metadata: &[u8]) -> Result<(), Error>;
    fn write(&mut self, bytes: &[u8]) -> Result<usize, Error>;
    fn finish(&mut self) -> Result<Self::Verified, Error>;
    fn activate(&mut self, verified: Self::Verified) -> Result<(), Error>;
    fn abort(&mut self) -> Result<(), Error>;
    fn confirm_running(&mut self) -> Result<(), Error>;
}

/// Exclusive ownership prevents a second writer between verification/activation.
/// Device adapters must also make opening/duplication exclusive. Dropping or
/// recovering the backend never invokes activation. Backends reconstruct boot
/// state themselves; this RAM state describes only the current stream.
pub struct Update<B: FirmwareBackend> {
    backend: B,
    state: State,
    expected: u64,
    received: u64,
    verified: Option<B::Verified>,
}
impl<B: FirmwareBackend> Update<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            state: State::Idle,
            expected: 0,
            received: 0,
            verified: None,
        }
    }
    pub fn capabilities(&self) -> Capabilities {
        self.backend.capabilities()
    }
    pub fn state(&self) -> State {
        self.state
    }
    pub fn received(&self) -> u64 {
        self.received
    }
    pub fn into_backend(self) -> B {
        self.backend
    }
    fn failed<T>(&mut self, error: Error) -> Result<T, Error> {
        self.verified = None;
        self.state = State::Failed;
        Err(error)
    }
    pub fn begin(&mut self, size: u64, metadata: &[u8]) -> Result<(), Error> {
        if self.state != State::Idle {
            return Err(Error::InvalidState);
        }
        if size == 0 || size > self.capabilities().max_image_size {
            return Err(Error::Length);
        }
        if let Err(e) = self.backend.begin(size, metadata) {
            return self.failed(e);
        }
        self.expected = size;
        self.received = 0;
        self.state = State::Receiving;
        Ok(())
    }
    pub fn write(&mut self, bytes: &[u8]) -> Result<usize, Error> {
        if self.state == State::Verified && !bytes.is_empty() {
            return self.failed(Error::InvalidState);
        }
        if self.state != State::Receiving {
            return Err(Error::InvalidState);
        }
        if bytes.len() as u64 > self.expected - self.received {
            return self.failed(Error::Length);
        }
        if bytes.is_empty() {
            return Ok(0);
        }
        let limit = bytes.len().min(self.capabilities().max_chunk_size as usize);
        if limit == 0 {
            return self.failed(Error::Geometry);
        }
        let count = match self.backend.write(&bytes[..limit]) {
            Ok(n) => n,
            Err(e) => return self.failed(e),
        };
        if count == 0 || count > limit {
            return self.failed(Error::Storage);
        }
        self.received += count as u64;
        Ok(count)
    }
    pub fn finish(&mut self) -> Result<(), Error> {
        if self.state != State::Receiving {
            return Err(Error::InvalidState);
        }
        if self.received != self.expected {
            return self.failed(Error::Length);
        }
        self.verified = Some(match self.backend.finish() {
            Ok(v) => v,
            Err(e) => return self.failed(e),
        });
        self.state = State::Verified;
        Ok(())
    }
    pub fn activate(&mut self) -> Result<(), Error> {
        if self.state != State::Verified {
            return Err(Error::InvalidState);
        }
        let proof = self.verified.take().ok_or(Error::InvalidState)?;
        if let Err(e) = self.backend.activate(proof) {
            return self.failed(e);
        }
        self.state = State::PendingBoot;
        Ok(())
    }
    pub fn abort(&mut self) -> Result<(), Error> {
        self.verified = None;
        if let Err(e) = self.backend.abort() {
            return self.failed(e);
        }
        self.state = State::Idle;
        self.expected = 0;
        self.received = 0;
        Ok(())
    }
    pub fn confirm_running(&mut self) -> Result<(), Error> {
        if self.state != State::Idle {
            return Err(Error::InvalidState);
        }
        self.backend.confirm_running()
    }
}
