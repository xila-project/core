//! EXPERIMENTAL native reference format. Not ESP/MCUboot, XPK, or hardware trust.
use crate::nor::{Nor, PAGE, SECTOR};
use crate::{AUTHENTICATED_UPDATE, Capabilities, Error, FUNCTIONAL_REVERT, FirmwareBackend};
use verification::{ContentVerifier, PublicKey, Sha256Verifier};
pub const TARGET_SIZE: usize = 16;
pub const MANIFEST_SIZE: usize = 144;
pub const SLOT_SIZE: usize = 4096;
pub const FLASH_SIZE: usize = 2 * SECTOR + 2 * SLOT_SIZE;
pub const IMAGE_CAPACITY: usize = SLOT_SIZE - SECTOR;
const NONE: u8 = 255;
#[derive(Clone, Copy)]
struct Manifest {
    bytes: [u8; MANIFEST_SIZE],
    size: u64,
    version: u64,
    digest: [u8; 32],
}
impl Manifest {
    fn parse(bytes: &[u8], key: &[u8; 32], target: &[u8; 16]) -> Result<Self, Error> {
        let bytes: [u8; MANIFEST_SIZE] = bytes.try_into().map_err(|_| Error::Encoding)?;
        if &bytes[..16] != b"XILA-FW-SIM-v1\0\0" {
            return Err(Error::Encoding);
        }
        PublicKey::from_bytes(key)
            .and_then(|k| k.verify_slice(&bytes[..80], &bytes[80..]))
            .map_err(|_| Error::Authentication)?;
        if &bytes[16..32] != target {
            return Err(Error::Target);
        }
        let size = u64::from_le_bytes(bytes[32..40].try_into().unwrap());
        let version = u64::from_le_bytes(bytes[40..48].try_into().unwrap());
        if size == 0 || size > IMAGE_CAPACITY as u64 {
            return Err(Error::Length);
        }
        Ok(Self {
            bytes,
            size,
            version,
            digest: bytes[48..80].try_into().unwrap(),
        })
    }
}
#[derive(Clone, Copy)]
struct Record {
    generation: u64,
    confirmed: u8,
    pending: u8,
    trial: bool,
    floor: u64,
}
fn digest(bytes: &[u8]) -> Result<[u8; 32], Error> {
    let mut h = Sha256Verifier::new();
    h.update(bytes).map_err(|_| Error::Length)?;
    h.finalize().map_err(|_| Error::Length)
}
impl Record {
    fn encode(self) -> Result<[u8; 64], Error> {
        let mut b = [0; 64];
        b[..4].copy_from_slice(b"FWJ1");
        b[4..12].copy_from_slice(&self.generation.to_le_bytes());
        b[12] = self.confirmed;
        b[13] = self.pending;
        b[14] = u8::from(self.trial);
        b[16..24].copy_from_slice(&self.floor.to_le_bytes());
        let hash = digest(&b[..32])?;
        b[32..].copy_from_slice(&hash);
        Ok(b)
    }
    fn decode(b: &[u8; 64]) -> Option<Self> {
        if &b[..4] != b"FWJ1"
            || digest(&b[..32]).ok()?.as_slice() != &b[32..]
            || b[12] > 1
            || !(b[13] <= 1 || b[13] == NONE)
            || b[14] > 1
            || b[15] != 0
            || b[24..32] != [0; 8]
            || (b[13] == NONE && b[14] != 0)
            || b[12] == b[13]
        {
            return None;
        }
        let generation = u64::from_le_bytes(b[4..12].try_into().ok()?);
        if generation == 0 {
            return None;
        }
        Some(Self {
            generation,
            confirmed: b[12],
            pending: b[13],
            trial: b[14] == 1,
            floor: u64::from_le_bytes(b[16..24].try_into().ok()?),
        })
    }
}
/// Cannot be constructed by a caller; consumed on activation and bound to the
/// metadata generation, slot and signed manifest verified from flash.
pub struct Verified {
    manifest: Manifest,
    slot: u8,
    generation: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootInfo {
    pub slot: u8,
    pub trial: bool,
    pub security_version: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootStatus {
    pub confirmed: u8,
    pub pending: Option<u8>,
    pub trial: bool,
    pub security_floor: u64,
}
pub struct Simulator<F: Nor> {
    flash: F,
    key: [u8; 32],
    target: [u8; 16],
    record: Record,
    record_sector: usize,
    staging: Option<(u8, Manifest)>,
    received: usize,
    running: Option<BootInfo>,
}
fn slot_offset(slot: u8) -> usize {
    2 * SECTOR + slot as usize * SLOT_SIZE
}
fn program_all(flash: &mut impl Nor, mut offset: usize, mut bytes: &[u8]) -> Result<(), Error> {
    while !bytes.is_empty() {
        let n = bytes.len().min(PAGE - offset % PAGE);
        flash.program(offset, &bytes[..n])?;
        offset += n;
        bytes = &bytes[n..];
    }
    Ok(())
}
impl<F: Nor> Simulator<F> {
    pub fn new(mut flash: F, key: [u8; 32], target: [u8; 16]) -> Result<Self, Error> {
        if flash.capacity() != FLASH_SIZE {
            return Err(Error::Geometry);
        }
        let mut a = [0; 64];
        let mut b = [0; 64];
        flash.read(0, &mut a)?;
        flash.read(SECTOR, &mut b)?;
        let (record, record_sector) = match (Record::decode(&a), Record::decode(&b)) {
            (Some(a), Some(b)) if a.generation == b.generation => return Err(Error::Recovery),
            (Some(a), Some(b)) => {
                if a.generation > b.generation {
                    (a, 0)
                } else {
                    (b, SECTOR)
                }
            }
            (Some(a), None) => (a, 0),
            (None, Some(b)) => (b, SECTOR),
            (None, None) => return Err(Error::Recovery),
        };
        Ok(Self {
            flash,
            key,
            target,
            record,
            record_sector,
            staging: None,
            received: 0,
            running: None,
        })
    }
    /// Manufacturing/test setup only: refuses any non-erased medium. Private
    /// signing keys are never accepted. A torn provisioning needs explicit rescue.
    pub fn provision(
        mut flash: F,
        key: [u8; 32],
        target: [u8; 16],
        metadata: &[u8],
        image: &[u8],
    ) -> Result<Self, Error> {
        if flash.capacity() != FLASH_SIZE {
            return Err(Error::Geometry);
        }
        let manifest = Manifest::parse(metadata, &key, &target)?;
        let mut verifier =
            ContentVerifier::new(manifest.size, manifest.digest).map_err(|_| Error::Length)?;
        verifier
            .update(image)
            .and_then(|_| verifier.verify())
            .map_err(|_| Error::Authentication)?;
        let mut buf = [0; PAGE];
        for offset in (0..FLASH_SIZE).step_by(PAGE) {
            flash.read(offset, &mut buf)?;
            if buf != [255; PAGE] {
                return Err(Error::Recovery);
            }
        }
        program_all(&mut flash, slot_offset(0), metadata)?;
        program_all(&mut flash, slot_offset(0) + SECTOR, image)?;
        let record = Record {
            generation: 1,
            confirmed: 0,
            pending: NONE,
            trial: false,
            floor: manifest.version,
        };
        flash.program(0, &record.encode()?)?;
        Self::new(flash, key, target)
    }
    pub fn status(&self) -> BootStatus {
        BootStatus {
            confirmed: self.record.confirmed,
            pending: (self.record.pending != NONE).then_some(self.record.pending),
            trial: self.record.trial,
            security_floor: self.record.floor,
        }
    }
    pub fn into_flash(self) -> F {
        self.flash
    }
    fn commit(&mut self, mut record: Record) -> Result<(), Error> {
        record.generation = self
            .record
            .generation
            .checked_add(1)
            .ok_or(Error::Recovery)?;
        let next = SECTOR - self.record_sector;
        self.flash.erase(next)?;
        self.flash.program(next, &record.encode()?)?;
        // Detect readback failure before publishing the RAM view.
        let mut b = [0; 64];
        self.flash.read(next, &mut b)?;
        if b != record.encode()? {
            return Err(Error::Storage);
        }
        self.record = record;
        self.record_sector = next;
        Ok(())
    }
    fn authenticate(&mut self, slot: u8) -> Result<Manifest, Error> {
        let offset = slot_offset(slot);
        let mut bytes = [0; MANIFEST_SIZE];
        self.flash.read(offset, &mut bytes)?;
        let m = Manifest::parse(&bytes, &self.key, &self.target)?;
        if m.version < self.record.floor {
            return Err(Error::SecurityVersion);
        }
        let mut verifier = ContentVerifier::new(m.size, m.digest).map_err(|_| Error::Length)?;
        let mut buf = [0; PAGE];
        let mut done = 0;
        while done < m.size as usize {
            let n = PAGE.min(m.size as usize - done);
            self.flash.read(offset + SECTOR + done, &mut buf[..n])?;
            verifier
                .update(&buf[..n])
                .map_err(|_| Error::Authentication)?;
            done += n;
        }
        verifier.verify().map_err(|_| Error::Authentication)?;
        Ok(m)
    }
    /// Each new instance models a fresh bootloader. Trial intent is persisted
    /// before selection: power loss then conservatively consumes the one attempt.
    pub fn boot(&mut self) -> Result<BootInfo, Error> {
        if self.running.is_some() || self.staging.is_some() {
            return Err(Error::InvalidState);
        }
        let mut slot = self.record.confirmed;
        let mut trial = false;
        if self.record.pending != NONE {
            let pending = self.record.pending;
            if !self.record.trial && self.authenticate(pending).is_ok() {
                self.commit(Record {
                    trial: true,
                    ..self.record
                })?;
                slot = pending;
                trial = true;
            } else {
                self.commit(Record {
                    pending: NONE,
                    trial: false,
                    ..self.record
                })?;
            }
        }
        let manifest = self.authenticate(slot).map_err(|_| Error::Recovery)?;
        let info = BootInfo {
            slot,
            trial,
            security_version: manifest.version,
        };
        self.running = Some(info);
        Ok(info)
    }
}
impl<F: Nor> FirmwareBackend for Simulator<F> {
    type Verified = Verified;
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            max_image_size: IMAGE_CAPACITY as u64,
            max_chunk_size: PAGE as u32,
            guarantees: AUTHENTICATED_UPDATE | FUNCTIONAL_REVERT,
        }
    }
    fn begin(&mut self, size: u64, metadata: &[u8]) -> Result<(), Error> {
        if self.staging.is_some() || self.record.pending != NONE {
            return Err(Error::InvalidState);
        }
        let m = Manifest::parse(metadata, &self.key, &self.target)?;
        if m.size != size {
            return Err(Error::Length);
        }
        if m.version < self.record.floor {
            return Err(Error::SecurityVersion);
        }
        // Persist a fresh staging generation even for identical restaging.
        // Proofs from an aborted/reconstructed backend must never be reusable.
        self.commit(self.record)?;
        let slot = 1 - self.record.confirmed;
        for offset in (slot_offset(slot)..slot_offset(slot) + SLOT_SIZE).step_by(SECTOR) {
            self.flash.erase(offset)?;
        }
        program_all(&mut self.flash, slot_offset(slot), metadata)?;
        self.staging = Some((slot, m));
        self.received = 0;
        Ok(())
    }
    fn write(&mut self, bytes: &[u8]) -> Result<usize, Error> {
        let (slot, m) = self.staging.ok_or(Error::InvalidState)?;
        if bytes.len() as u64 > m.size - self.received as u64 {
            return Err(Error::Length);
        }
        let offset = slot_offset(slot) + SECTOR + self.received;
        let n = bytes.len().min(PAGE - offset % PAGE);
        self.flash.program(offset, &bytes[..n])?;
        self.received += n;
        Ok(n)
    }
    fn finish(&mut self) -> Result<Verified, Error> {
        let (slot, expected) = self.staging.ok_or(Error::InvalidState)?;
        if self.received as u64 != expected.size {
            return Err(Error::Length);
        }
        let manifest = self.authenticate(slot)?;
        if manifest.bytes != expected.bytes {
            return Err(Error::Authentication);
        }
        Ok(Verified {
            manifest,
            slot,
            generation: self.record.generation,
        })
    }
    fn activate(&mut self, verified: Verified) -> Result<(), Error> {
        let (slot, m) = self.staging.ok_or(Error::InvalidState)?;
        if verified.generation != self.record.generation
            || verified.slot != slot
            || verified.manifest.bytes != m.bytes
        {
            return Err(Error::InvalidState);
        }
        if self.authenticate(slot)?.bytes != verified.manifest.bytes {
            return Err(Error::Authentication);
        }
        self.commit(Record {
            pending: slot,
            trial: false,
            ..self.record
        })?;
        self.staging = None;
        Ok(())
    }
    fn abort(&mut self) -> Result<(), Error> {
        self.staging = None;
        self.received = 0;
        Ok(())
    }
    fn confirm_running(&mut self) -> Result<(), Error> {
        let running = self.running.ok_or(Error::InvalidState)?;
        self.authenticate(running.slot)?;
        if self.record.pending == NONE && self.record.confirmed == running.slot {
            return Ok(());
        }
        if self.record.pending != running.slot || !self.record.trial {
            return Err(Error::InvalidState);
        }
        self.commit(Record {
            confirmed: running.slot,
            pending: NONE,
            trial: false,
            floor: running.security_version.max(self.record.floor),
            ..self.record
        })
    }
}
