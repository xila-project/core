//! Version 1 byte-array ABI for `/devices/system/firmware`.
//! Mount root-owned with 0600 permissions atomically before publishing. All
//! openings are exclusive, including status readers. Native Rust is trusted;
//! untrusted callers must pass through the VFS and its Wasm pointer checks.
use crate::{Error as FirmwareError, FirmwareBackend, Update};
use alloc::boxed::Box;
use file_system::{
    BaseOperations, CharacterDevice, Context, ControlCommand, ControlCommandIdentifier, Error,
    MountOperations, Position, Result, Size, define_command,
};
use shared::AnyByLayout;
use synchronization::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
pub const DEVICE_PATH: &str = "/devices/system/firmware";
pub const ABI_VERSION: u16 = 1;
pub const MAX_METADATA: usize = 256;
pub const BEGIN_SIZE: usize = 280;
define_command!(GET_CAPABILITIES, Read, b'F', 0, [u8; 8], [u8; 32]);
define_command!(GET_STATUS, Read, b'F', 1, [u8; 8], [u8; 24]);
define_command!(BEGIN, Write, b'F', 2, [u8; BEGIN_SIZE], ());
define_command!(FINISH, Write, b'F', 3, [u8; 8], ());
define_command!(ACTIVATE, Write, b'F', 4, [u8; 8], ());
define_command!(ABORT, Write, b'F', 5, [u8; 8], ());
define_command!(CONFIRM, Write, b'F', 6, [u8; 8], ());
pub fn header() -> [u8; 8] {
    let mut h = [0; 8];
    h[..2].copy_from_slice(&ABI_VERSION.to_le_bytes());
    h
}
pub fn begin_request(size: u64, metadata: &[u8]) -> Result<[u8; BEGIN_SIZE]> {
    if metadata.len() > MAX_METADATA {
        return Err(Error::InvalidParameter);
    }
    let mut bytes = [0; BEGIN_SIZE];
    bytes[..8].copy_from_slice(&header());
    bytes[8..16].copy_from_slice(&size.to_le_bytes());
    bytes[16..18].copy_from_slice(&(metadata.len() as u16).to_le_bytes());
    bytes[24..24 + metadata.len()].copy_from_slice(metadata);
    Ok(bytes)
}
fn validate(bytes: &[u8]) -> Result<()> {
    if bytes == header() {
        Ok(())
    } else {
        Err(Error::InvalidParameter)
    }
}
/// Stable mapping into existing Xila file-system errors.
fn error(error: FirmwareError) -> Error {
    match error {
        FirmwareError::Unsupported => Error::UnsupportedOperation,
        FirmwareError::Storage | FirmwareError::Recovery => Error::InputOutput,
        FirmwareError::Authentication | FirmwareError::Target | FirmwareError::SecurityVersion => {
            Error::PermissionDenied
        }
        _ => Error::InvalidParameter,
    }
}
struct Inner<B: FirmwareBackend> {
    update: Update<B>,
    opened: bool,
}
struct Opening;
pub struct FirmwareDevice<B: FirmwareBackend> {
    inner: Mutex<CriticalSectionRawMutex, Inner<B>>,
}
impl<B: FirmwareBackend> FirmwareDevice<B> {
    pub fn new(backend: B) -> Self {
        Self {
            inner: Mutex::new(Inner {
                update: Update::new(backend),
                opened: false,
            }),
        }
    }
    fn opening(context: &Context) -> Result<()> {
        context
            .get_private_data_of_type::<Opening>()
            .ok_or(Error::InvalidIdentifier)
            .map(|_| ())
    }
}
impl<B> BaseOperations for FirmwareDevice<B>
where
    B: FirmwareBackend + Send,
    B::Verified: Send,
{
    fn open(&self, context: &mut Context) -> Result<()> {
        let mut inner = self.inner.try_lock().map_err(|_| Error::RessourceBusy)?;
        if inner.opened {
            return Err(Error::RessourceBusy);
        }
        context.set_private_data(Box::new(Opening));
        inner.opened = true;
        Ok(())
    }
    fn close(&self, context: &mut Context) -> Result<()> {
        Self::opening(context)?;
        let mut inner = self.inner.try_lock().map_err(|_| Error::RessourceBusy)?;
        // Keep exclusivity after abort failure; reopening an uncertain backend
        // requires reconstruction/recovery by the platform owner.
        inner.update.abort().map_err(error)?;
        inner.opened = false;
        context.take_private_data_of_type::<Opening>();
        Ok(())
    }
    fn read(
        &self,
        _: &mut Context,
        _: &mut core::task::Context<'_>,
        _: &mut [u8],
        _: Size,
    ) -> core::task::Poll<Result<usize>> {
        file_system::ready(|| Err(Error::UnsupportedOperation))
    }
    fn write(
        &self,
        context: &mut Context,
        _: &mut core::task::Context<'_>,
        bytes: &[u8],
        _: Size,
    ) -> core::task::Poll<Result<usize>> {
        file_system::ready(|| {
            Self::opening(context)?;
            self.inner
                .try_lock()
                .map_err(|_| Error::RessourceBusy)?
                .update
                .write(bytes)
                .map_err(error)
        })
    }
    fn control(
        &self,
        context: &mut Context,
        _: &mut core::task::Context<'_>,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> core::task::Poll<Result<()>> {
        file_system::ready(|| {
            Self::opening(context)?;
            let mut inner = self.inner.try_lock().map_err(|_| Error::RessourceBusy)?;
            let update = &mut inner.update;
            let expected = match command {
                GET_CAPABILITIES::IDENTIFIER => (8, 32),
                GET_STATUS::IDENTIFIER => (8, 24),
                BEGIN::IDENTIFIER => (BEGIN_SIZE, 0),
                FINISH::IDENTIFIER
                | ACTIVATE::IDENTIFIER
                | ABORT::IDENTIFIER
                | CONFIRM::IDENTIFIER => (8, 0),
                _ => return Err(Error::UnsupportedOperation),
            };
            if input.get_size() != expected.0 || output.get_size() != expected.1 {
                return Err(Error::InvalidParameter);
            }
            match command {
                GET_CAPABILITIES::IDENTIFIER => {
                    let (i, o) = GET_CAPABILITIES::cast(input, output)?;
                    validate(i)?;
                    o.fill(0);
                    o[..8].copy_from_slice(&header());
                    let c = update.capabilities();
                    o[8..16].copy_from_slice(&c.max_image_size.to_le_bytes());
                    o[16..20].copy_from_slice(&c.max_chunk_size.to_le_bytes());
                    o[20..24].copy_from_slice(&c.guarantees.to_le_bytes());
                    Ok(())
                }
                GET_STATUS::IDENTIFIER => {
                    let (i, o) = GET_STATUS::cast(input, output)?;
                    validate(i)?;
                    o.fill(0);
                    o[..8].copy_from_slice(&header());
                    o[8] = update.state() as u8;
                    o[16..24].copy_from_slice(&update.received().to_le_bytes());
                    Ok(())
                }
                BEGIN::IDENTIFIER => {
                    let i = BEGIN::cast_input(input)?;
                    validate(&i[..8])?;
                    let n = u16::from_le_bytes(i[16..18].try_into().unwrap()) as usize;
                    if n > MAX_METADATA
                        || i[18..24] != [0; 6]
                        || i[24 + n..].iter().any(|b| *b != 0)
                    {
                        return Err(Error::InvalidParameter);
                    }
                    update
                        .begin(
                            u64::from_le_bytes(i[8..16].try_into().unwrap()),
                            &i[24..24 + n],
                        )
                        .map_err(error)
                }
                FINISH::IDENTIFIER
                | ACTIVATE::IDENTIFIER
                | ABORT::IDENTIFIER
                | CONFIRM::IDENTIFIER => {
                    let i = FINISH::cast_input(input)?;
                    validate(i)?;
                    match command {
                        FINISH::IDENTIFIER => update.finish(),
                        ACTIVATE::IDENTIFIER => update.activate(),
                        ABORT::IDENTIFIER => update.abort(),
                        _ => update.confirm_running(),
                    }
                    .map_err(error)
                }
                _ => Err(Error::UnsupportedOperation),
            }
        })
    }
    fn set_position(&self, _: &mut Context, _: Size, _: &Position) -> Result<Size> {
        Err(Error::UnsupportedOperation)
    }
    fn clone_context(&self, _: &Context) -> Result<Context> {
        Err(Error::UnsupportedOperation)
    }
}
impl<B: FirmwareBackend> MountOperations for FirmwareDevice<B> {}
impl<B> CharacterDevice for FirmwareDevice<B>
where
    B: FirmwareBackend + Send,
    B::Verified: Send,
{
}
