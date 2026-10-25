//! Reference NOR geometry, deterministic torn operations, and bounded storage.
use crate::Error;
pub const SECTOR: usize = 256;
pub const PAGE: usize = 64;
/// Before/after a single erase/program, or after a prefix of its bytes. Operation
/// numbers start at zero. Once triggered, the medium requires a simulated reboot.
#[derive(Clone, Copy, Debug)]
pub struct Fault {
    pub operation: u64,
    pub after_bytes: usize,
}
#[derive(Default)]
pub struct Faults {
    fault: Option<Fault>,
    operations: u64,
    failed: bool,
}
impl Faults {
    pub fn new(fault: Option<Fault>) -> Self {
        Self {
            fault,
            ..Self::default()
        }
    }
    pub fn operations(&self) -> u64 {
        self.operations
    }
    fn limit(&mut self, size: usize) -> Result<(usize, bool), Error> {
        if self.failed {
            return Err(Error::Storage);
        }
        let fault = self.fault.filter(|f| f.operation == self.operations);
        self.operations += 1;
        if let Some(f) = fault {
            self.failed = true;
            Ok((f.after_bytes.min(size), true))
        } else {
            Ok((size, false))
        }
    }
}
/// All operations complete synchronously. `program` never spans a page and only
/// clears bits. Storage implementations must persist a torn prefix before error.
/// This reference geometry has byte writes; hardware adapters use their own trait.
pub trait Nor {
    fn capacity(&self) -> usize;
    fn read(&mut self, offset: usize, bytes: &mut [u8]) -> Result<(), Error>;
    fn erase(&mut self, offset: usize) -> Result<(), Error>;
    fn program(&mut self, offset: usize, bytes: &[u8]) -> Result<(), Error>;
}
fn bounds(capacity: usize, offset: usize, len: usize) -> Result<(), Error> {
    if offset.checked_add(len).is_none_or(|end| end > capacity) {
        Err(Error::Geometry)
    } else {
        Ok(())
    }
}
fn program_bounds(capacity: usize, offset: usize, len: usize) -> Result<(), Error> {
    bounds(capacity, offset, len)?;
    if len > PAGE || offset % PAGE + len > PAGE {
        return Err(Error::Geometry);
    }
    Ok(())
}
pub struct MemoryNor<'a> {
    bytes: &'a mut [u8],
    faults: Faults,
}
impl<'a> MemoryNor<'a> {
    pub fn new(bytes: &'a mut [u8]) -> Result<Self, Error> {
        Self::construct(bytes, None)
    }
    pub fn with_fault(bytes: &'a mut [u8], fault: Fault) -> Result<Self, Error> {
        Self::construct(bytes, Some(fault))
    }
    fn construct(bytes: &'a mut [u8], fault: Option<Fault>) -> Result<Self, Error> {
        if bytes.is_empty() || !bytes.len().is_multiple_of(SECTOR) {
            return Err(Error::Geometry);
        }
        Ok(Self {
            bytes,
            faults: Faults::new(fault),
        })
    }
    pub fn operations(&self) -> u64 {
        self.faults.operations()
    }
}
impl Nor for MemoryNor<'_> {
    fn capacity(&self) -> usize {
        self.bytes.len()
    }
    fn read(&mut self, offset: usize, bytes: &mut [u8]) -> Result<(), Error> {
        bounds(self.capacity(), offset, bytes.len())?;
        bytes.copy_from_slice(&self.bytes[offset..offset + bytes.len()]);
        Ok(())
    }
    fn erase(&mut self, offset: usize) -> Result<(), Error> {
        bounds(self.capacity(), offset, SECTOR)?;
        if !offset.is_multiple_of(SECTOR) {
            return Err(Error::Geometry);
        }
        let (n, failed) = self.faults.limit(SECTOR)?;
        self.bytes[offset..offset + n].fill(255);
        if failed { Err(Error::Storage) } else { Ok(()) }
    }
    fn program(&mut self, offset: usize, bytes: &[u8]) -> Result<(), Error> {
        program_bounds(self.capacity(), offset, bytes.len())?;
        if self.bytes[offset..offset + bytes.len()]
            .iter()
            .zip(bytes)
            .any(|(old, new)| old & new != *new)
        {
            return Err(Error::Geometry);
        }
        let (n, failed) = self.faults.limit(bytes.len())?;
        self.bytes[offset..offset + n].copy_from_slice(&bytes[..n]);
        if failed { Err(Error::Storage) } else { Ok(()) }
    }
}

#[cfg(feature = "std")]
mod file {
    use super::*;
    use std::{
        fs::{File, OpenOptions},
        io::{Read, Seek, SeekFrom, Write},
        path::Path,
    };
    /// Raw NOR image: no rename, filesystem journal or whole-image allocation.
    /// sync_data makes each simulated durable prefix survive process exit; this
    /// does not model physical storage/cache failure beyond the injected NOR cut.
    pub struct FileNor {
        file: File,
        capacity: usize,
        faults: Faults,
    }
    impl FileNor {
        pub fn create(path: &Path, capacity: usize) -> Result<Self, Error> {
            if capacity == 0 || !capacity.is_multiple_of(SECTOR) {
                return Err(Error::Geometry);
            }
            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(|_| Error::Storage)?;
            file.try_lock().map_err(|_| Error::Storage)?;
            for _ in 0..capacity / PAGE {
                file.write_all(&[255; PAGE]).map_err(|_| Error::Storage)?;
            }
            file.sync_data().map_err(|_| Error::Storage)?;
            Ok(Self {
                file,
                capacity,
                faults: Faults::default(),
            })
        }
        pub fn open(path: &Path, fault: Option<Fault>) -> Result<Self, Error> {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .map_err(|_| Error::Storage)?;
            file.try_lock().map_err(|_| Error::Storage)?;
            let capacity = usize::try_from(file.metadata().map_err(|_| Error::Storage)?.len())
                .map_err(|_| Error::Geometry)?;
            if capacity == 0 || !capacity.is_multiple_of(SECTOR) {
                return Err(Error::Geometry);
            }
            Ok(Self {
                file,
                capacity,
                faults: Faults::new(fault),
            })
        }
        fn persist(&mut self, offset: usize, bytes: &[u8]) -> Result<(), Error> {
            self.file
                .seek(SeekFrom::Start(offset as u64))
                .map_err(|_| Error::Storage)?;
            self.file.write_all(bytes).map_err(|_| Error::Storage)?;
            self.file.sync_data().map_err(|_| Error::Storage)
        }
    }
    impl Nor for FileNor {
        fn capacity(&self) -> usize {
            self.capacity
        }
        fn read(&mut self, offset: usize, bytes: &mut [u8]) -> Result<(), Error> {
            bounds(self.capacity, offset, bytes.len())?;
            self.file
                .seek(SeekFrom::Start(offset as u64))
                .map_err(|_| Error::Storage)?;
            self.file.read_exact(bytes).map_err(|_| Error::Storage)
        }
        fn erase(&mut self, offset: usize) -> Result<(), Error> {
            bounds(self.capacity, offset, SECTOR)?;
            if !offset.is_multiple_of(SECTOR) {
                return Err(Error::Geometry);
            }
            let (n, failed) = self.faults.limit(SECTOR)?;
            self.persist(offset, &[255; SECTOR][..n])?;
            if failed { Err(Error::Storage) } else { Ok(()) }
        }
        fn program(&mut self, offset: usize, bytes: &[u8]) -> Result<(), Error> {
            program_bounds(self.capacity, offset, bytes.len())?;
            let mut old = [0; PAGE];
            self.read(offset, &mut old[..bytes.len()])?;
            if old.iter().zip(bytes).any(|(a, b)| a & b != *b) {
                return Err(Error::Geometry);
            }
            let (n, failed) = self.faults.limit(bytes.len())?;
            self.persist(offset, &bytes[..n])?;
            if failed { Err(Error::Storage) } else { Ok(()) }
        }
    }
}
#[cfg(feature = "std")]
pub use file::FileNor;
