// Explicit drop boundaries release borrowed NOR state to model a reboot.
#![allow(clippy::drop_non_drop)]
#![cfg(feature = "simulator")]
mod support;
use firmware_update::nor::{Fault, MemoryNor, Nor};
use firmware_update::simulator::{FLASH_SIZE, Simulator};
use firmware_update::{Error, FirmwareBackend, Update};
use support::*;
const OLD: &[u8] = b"confirmed old firmware";
const NEW: &[u8] = &[0x5a; 137];
fn provision(bytes: &mut [u8]) {
    Simulator::provision(
        MemoryNor::new(bytes).unwrap(),
        key().verifying_key().to_bytes(),
        TARGET,
        &manifest(OLD, 1),
        OLD,
    )
    .unwrap();
}
fn stage(bytes: &mut [u8]) {
    let sim = Simulator::new(
        MemoryNor::new(bytes).unwrap(),
        key().verifying_key().to_bytes(),
        TARGET,
    )
    .unwrap();
    let mut u = Update::new(sim);
    u.begin(NEW.len() as u64, &manifest(NEW, 2)).unwrap();
    let mut off = 0;
    while off < NEW.len() {
        off += u.write(&NEW[off..]).unwrap();
    }
    u.finish().unwrap();
    u.activate().unwrap();
}
#[test]
fn nor_rejects_geometry_bounds_and_zero_to_one() {
    let mut bytes = [255; FLASH_SIZE];
    let mut f = MemoryNor::new(&mut bytes).unwrap();
    assert_eq!(f.erase(1), Err(Error::Geometry));
    assert_eq!(f.program(63, &[0, 0]), Err(Error::Geometry));
    assert_eq!(f.program(FLASH_SIZE, &[0]), Err(Error::Geometry));
    f.program(0, &[0]).unwrap();
    assert_eq!(f.program(0, &[255]), Err(Error::Geometry));
    f.erase(0).unwrap();
    f.program(0, &[255]).unwrap();
}
#[test]
fn persistent_trial_reverts_or_confirms() {
    let mut bytes = [255; FLASH_SIZE];
    provision(&mut bytes);
    stage(&mut bytes);
    {
        let mut s = Simulator::new(
            MemoryNor::new(&mut bytes).unwrap(),
            key().verifying_key().to_bytes(),
            TARGET,
        )
        .unwrap();
        assert_eq!(s.boot().unwrap().slot, 1);
        s.confirm_running().unwrap();
        s.confirm_running().unwrap();
    }
    let mut s = Simulator::new(
        MemoryNor::new(&mut bytes).unwrap(),
        key().verifying_key().to_bytes(),
        TARGET,
    )
    .unwrap();
    assert_eq!(s.boot().unwrap().slot, 1);
    drop(s);
    let mut bytes = [255; FLASH_SIZE];
    provision(&mut bytes);
    stage(&mut bytes);
    for expected in [1, 0, 0] {
        let mut s = Simulator::new(
            MemoryNor::new(&mut bytes).unwrap(),
            key().verifying_key().to_bytes(),
            TARGET,
        )
        .unwrap();
        assert_eq!(s.boot().unwrap().slot, expected);
    }
}
#[test]
fn invalid_metadata_and_corruption_never_activate() {
    let mut bytes = [255; FLASH_SIZE];
    provision(&mut bytes);
    for (offset, expected) in [
        (0, Error::Encoding),
        (16, Error::Authentication),
        (80, Error::Authentication),
    ] {
        let mut m = manifest(NEW, 2);
        m[offset] ^= 1;
        let mut u = Update::new(
            Simulator::new(
                MemoryNor::new(&mut bytes).unwrap(),
                key().verifying_key().to_bytes(),
                TARGET,
            )
            .unwrap(),
        );
        assert_eq!(u.begin(NEW.len() as u64, &m), Err(expected));
        assert_eq!(u.activate(), Err(Error::InvalidState));
    }
    let mut u = Update::new(
        Simulator::new(
            MemoryNor::new(&mut bytes).unwrap(),
            key().verifying_key().to_bytes(),
            TARGET,
        )
        .unwrap(),
    );
    assert_eq!(
        u.begin(NEW.len() as u64, &manifest(NEW, 0)),
        Err(Error::SecurityVersion)
    );
    drop(u);
    let mut u = Update::new(
        Simulator::new(
            MemoryNor::new(&mut bytes).unwrap(),
            key().verifying_key().to_bytes(),
            TARGET,
        )
        .unwrap(),
    );
    u.begin(NEW.len() as u64, &manifest(NEW, 2)).unwrap();
    let mut off = 0;
    let bad = [0x33; 137];
    while off < bad.len() {
        off += u.write(&bad[off..]).unwrap();
    }
    assert_eq!(u.finish(), Err(Error::Authentication));
    assert_eq!(u.activate(), Err(Error::InvalidState));
}
#[test]
fn every_update_operation_and_torn_byte_preserves_confirmed_boot() {
    let mut original = [255; FLASH_SIZE];
    provision(&mut original);
    // Begin journal erase/program + 16 erases + 3 header + 3 body + activation journal.
    for operation in 0..26 {
        for byte in [0, 1, 31, 63, 64, 127, 255, 256] {
            let mut bytes = original;
            let flash = MemoryNor::with_fault(
                &mut bytes,
                Fault {
                    operation,
                    after_bytes: byte,
                },
            )
            .unwrap();
            let mut u = Update::new(
                Simulator::new(flash, key().verifying_key().to_bytes(), TARGET).unwrap(),
            );
            let result = (|| {
                u.begin(NEW.len() as u64, &manifest(NEW, 2))?;
                let mut off = 0;
                while off < NEW.len() {
                    off += u.write(&NEW[off..])?;
                }
                u.finish()?;
                u.activate()
            })();
            drop(u);
            let mut s = Simulator::new(
                MemoryNor::new(&mut bytes).unwrap(),
                key().verifying_key().to_bytes(),
                TARGET,
            )
            .unwrap();
            let boot = s.boot().unwrap_or_else(|e| {
                panic!("operation={operation} byte={byte} result={result:?}: {e:?}")
            });
            assert!(boot.slot <= 1);
            drop(s);
            // An unconfirmed trial always falls back to the original A.
            let mut s = Simulator::new(
                MemoryNor::new(&mut bytes).unwrap(),
                key().verifying_key().to_bytes(),
                TARGET,
            )
            .unwrap();
            assert_eq!(
                s.boot().unwrap().slot,
                0,
                "operation={operation} byte={byte}"
            );
        }
    }
}
#[test]
fn boot_and_confirm_torn_journals_recover_a_valid_image() {
    let mut pending = [255; FLASH_SIZE];
    provision(&mut pending);
    stage(&mut pending);
    for operation in 0..2 {
        for after_bytes in [0, 1, 31, 63, 64, 127, 255, 256] {
            let mut bytes = pending;
            let mut s = Simulator::new(
                MemoryNor::with_fault(
                    &mut bytes,
                    Fault {
                        operation,
                        after_bytes,
                    },
                )
                .unwrap(),
                key().verifying_key().to_bytes(),
                TARGET,
            )
            .unwrap();
            assert!(s.boot().is_err());
            drop(s);
            let mut s = Simulator::new(
                MemoryNor::new(&mut bytes).unwrap(),
                key().verifying_key().to_bytes(),
                TARGET,
            )
            .unwrap();
            let boot = s.boot().unwrap();
            assert!(boot.slot <= 1);
            drop(s);
            let mut s = Simulator::new(
                MemoryNor::new(&mut bytes).unwrap(),
                key().verifying_key().to_bytes(),
                TARGET,
            )
            .unwrap();
            assert_eq!(s.boot().unwrap().slot, 0);
        }
    }
    // Boot consumes two journal operations, so cut either confirm operation.
    for operation in 2..4 {
        for after_bytes in [0, 1, 31, 63, 64, 127, 255, 256] {
            let mut bytes = pending;
            let mut s = Simulator::new(
                MemoryNor::with_fault(
                    &mut bytes,
                    Fault {
                        operation,
                        after_bytes,
                    },
                )
                .unwrap(),
                key().verifying_key().to_bytes(),
                TARGET,
            )
            .unwrap();
            assert_eq!(s.boot().unwrap().slot, 1);
            assert!(s.confirm_running().is_err());
            drop(s);
            let mut s = Simulator::new(
                MemoryNor::new(&mut bytes).unwrap(),
                key().verifying_key().to_bytes(),
                TARGET,
            )
            .unwrap();
            let boot = s.boot().unwrap();
            assert!(!boot.trial);
            assert_eq!(boot.security_version, if boot.slot == 0 { 1 } else { 2 });
        }
    }
}
#[test]
fn corruption_between_finish_and_activate_and_at_boot_is_rejected() {
    use std::{cell::RefCell, rc::Rc};
    struct Shared(Rc<RefCell<[u8; FLASH_SIZE]>>);
    impl Nor for Shared {
        fn capacity(&self) -> usize {
            FLASH_SIZE
        }
        fn read(&mut self, o: usize, b: &mut [u8]) -> Result<(), Error> {
            MemoryNor::new(&mut *self.0.borrow_mut())
                .unwrap()
                .read(o, b)
        }
        fn program(&mut self, o: usize, b: &[u8]) -> Result<(), Error> {
            MemoryNor::new(&mut *self.0.borrow_mut())
                .unwrap()
                .program(o, b)
        }
        fn erase(&mut self, o: usize) -> Result<(), Error> {
            MemoryNor::new(&mut *self.0.borrow_mut()).unwrap().erase(o)
        }
    }
    let bytes = Rc::new(RefCell::new([255; FLASH_SIZE]));
    provision(&mut *bytes.borrow_mut());
    let sim = Simulator::new(
        Shared(bytes.clone()),
        key().verifying_key().to_bytes(),
        TARGET,
    )
    .unwrap();
    let mut u = Update::new(sim);
    u.begin(NEW.len() as u64, &manifest(NEW, 2)).unwrap();
    let mut off = 0;
    while off < NEW.len() {
        off += u.write(&NEW[off..]).unwrap();
    }
    u.finish().unwrap();
    bytes.borrow_mut()[512 + 4096 + 256] ^= 1;
    assert_eq!(u.activate(), Err(Error::Authentication));
    drop(u);
    stage(&mut *bytes.borrow_mut());
    bytes.borrow_mut()[512 + 4096 + 256] ^= 1;
    let mut s = Simulator::new(
        Shared(bytes.clone()),
        key().verifying_key().to_bytes(),
        TARGET,
    )
    .unwrap();
    assert_eq!(s.boot().unwrap().slot, 0);
    drop(s);
    bytes.borrow_mut()[512 + 256] ^= 1;
    let mut s = Simulator::new(
        Shared(bytes.clone()),
        key().verifying_key().to_bytes(),
        TARGET,
    )
    .unwrap();
    assert_eq!(s.boot(), Err(Error::Recovery));
}
#[test]
fn signed_wrong_target_size_and_untrusted_key_fail_closed() {
    use ed25519_dalek::Signer;
    let mut bytes = [255; FLASH_SIZE];
    provision(&mut bytes);
    for mode in 0..4 {
        let mut m = manifest(NEW, 2);
        if mode == 0 {
            m[16] ^= 1;
            let sig = key().sign(&m[..80]).to_bytes();
            m[80..].copy_from_slice(&sig);
        }
        if mode == 1 {
            m[32..40].copy_from_slice(&0u64.to_le_bytes());
            let sig = key().sign(&m[..80]).to_bytes();
            m[80..].copy_from_slice(&sig);
        }
        let trust = if mode == 2 {
            [0; 32]
        } else {
            key().verifying_key().to_bytes()
        };
        let mut u = Update::new(
            Simulator::new(MemoryNor::new(&mut bytes).unwrap(), trust, TARGET).unwrap(),
        );
        assert_eq!(
            u.begin(if mode == 3 { 136 } else { 137 }, &m),
            Err(match mode {
                0 => Error::Target,
                1 | 3 => Error::Length,
                _ => Error::Authentication,
            })
        );
    }
}
#[test]
fn seeded_fault_schedules_are_reproducible() {
    const SEED: u64 = 0x526_f1a5;
    let mut rng = SEED;
    let mut original = [255; FLASH_SIZE];
    provision(&mut original);
    for schedule in 0..32 {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        let operation = (rng >> 32) % 26;
        let after_bytes = (rng & 255) as usize;
        let mut bytes = original;
        let mut u = Update::new(
            Simulator::new(
                MemoryNor::with_fault(
                    &mut bytes,
                    Fault {
                        operation,
                        after_bytes,
                    },
                )
                .unwrap(),
                key().verifying_key().to_bytes(),
                TARGET,
            )
            .unwrap(),
        );
        let _ = (|| {
            u.begin(NEW.len() as u64, &manifest(NEW, 2))?;
            let mut off = 0;
            while off < NEW.len() {
                off += u.write(&NEW[off..])?;
            }
            u.finish()?;
            u.activate()
        })();
        drop(u);
        let mut s = Simulator::new(
            MemoryNor::new(&mut bytes).unwrap(),
            key().verifying_key().to_bytes(),
            TARGET,
        )
        .unwrap();
        assert!(s.boot().unwrap_or_else(|e|panic!("seed={SEED:#x} schedule={schedule} op={operation} byte={after_bytes} error={e:?}")).slot<=1);
    }
}

#[test]
fn proof_cannot_cross_abort_or_backend_reconstruction() {
    let mut bytes = [255; FLASH_SIZE];
    provision(&mut bytes);
    let mut sim = Simulator::new(
        MemoryNor::new(&mut bytes).unwrap(),
        key().verifying_key().to_bytes(),
        TARGET,
    )
    .unwrap();
    sim.begin(3, &manifest(b"new", 2)).unwrap();
    sim.write(b"new").unwrap();
    let proof = sim.finish().unwrap();
    sim.abort().unwrap();
    sim.begin(3, &manifest(b"new", 2)).unwrap();
    sim.write(b"new").unwrap();
    assert_eq!(sim.activate(proof), Err(Error::InvalidState));
    let proof = sim.finish().unwrap();
    drop(sim);
    let mut sim = Simulator::new(
        MemoryNor::new(&mut bytes).unwrap(),
        key().verifying_key().to_bytes(),
        TARGET,
    )
    .unwrap();
    sim.begin(3, &manifest(b"new", 2)).unwrap();
    sim.write(b"new").unwrap();
    assert_eq!(sim.activate(proof), Err(Error::InvalidState));
}

#[test]
fn readback_io_failure_blocks_finish_and_activation() {
    use std::{cell::Cell, rc::Rc};
    struct ReadFault<'a> {
        flash: MemoryNor<'a>,
        failed: Rc<Cell<bool>>,
    }
    impl Nor for ReadFault<'_> {
        fn capacity(&self) -> usize {
            self.flash.capacity()
        }
        fn read(&mut self, o: usize, b: &mut [u8]) -> Result<(), Error> {
            if self.failed.get() {
                Err(Error::Storage)
            } else {
                self.flash.read(o, b)
            }
        }
        fn erase(&mut self, o: usize) -> Result<(), Error> {
            self.flash.erase(o)
        }
        fn program(&mut self, o: usize, b: &[u8]) -> Result<(), Error> {
            self.flash.program(o, b)
        }
    }
    for at_activation in [false, true] {
        let mut bytes = [255; FLASH_SIZE];
        provision(&mut bytes);
        let failed = Rc::new(Cell::new(false));
        let flash = ReadFault {
            flash: MemoryNor::new(&mut bytes).unwrap(),
            failed: failed.clone(),
        };
        let mut u =
            Update::new(Simulator::new(flash, key().verifying_key().to_bytes(), TARGET).unwrap());
        u.begin(3, &manifest(b"new", 2)).unwrap();
        u.write(b"new").unwrap();
        if at_activation {
            u.finish().unwrap();
            failed.set(true);
            assert_eq!(u.activate(), Err(Error::Storage));
        } else {
            failed.set(true);
            assert_eq!(u.finish(), Err(Error::Storage));
            assert_eq!(u.activate(), Err(Error::InvalidState));
        }
    }
}

#[test]
fn interrupted_revert_retries_without_booting_the_failed_trial() {
    let mut trial = [255; FLASH_SIZE];
    provision(&mut trial);
    stage(&mut trial);
    {
        let mut s = Simulator::new(
            MemoryNor::new(&mut trial).unwrap(),
            key().verifying_key().to_bytes(),
            TARGET,
        )
        .unwrap();
        assert_eq!(s.boot().unwrap().slot, 1);
    }
    for operation in 0..2 {
        for after_bytes in [0, 1, 31, 63, 64, 127, 255, 256] {
            let mut bytes = trial;
            let mut s = Simulator::new(
                MemoryNor::with_fault(
                    &mut bytes,
                    Fault {
                        operation,
                        after_bytes,
                    },
                )
                .unwrap(),
                key().verifying_key().to_bytes(),
                TARGET,
            )
            .unwrap();
            assert!(s.boot().is_err());
            drop(s);
            let mut s = Simulator::new(
                MemoryNor::new(&mut bytes).unwrap(),
                key().verifying_key().to_bytes(),
                TARGET,
            )
            .unwrap();
            assert_eq!(
                s.boot().unwrap().slot,
                0,
                "operation={operation} after_bytes={after_bytes}"
            );
        }
    }
}
#[test]
fn bounded_manifest_mutations_never_panic_or_prepare_an_update() {
    let mut bytes = [255; FLASH_SIZE];
    provision(&mut bytes);
    for len in 0..=256 {
        let mut s = Simulator::new(
            MemoryNor::new(&mut bytes).unwrap(),
            key().verifying_key().to_bytes(),
            TARGET,
        )
        .unwrap();
        assert!(s.begin(3, &[0; 256][..len]).is_err());
    }
    let valid = manifest(b"new", 2);
    for offset in 0..valid.len() {
        let mut changed = valid;
        changed[offset] ^= 1;
        let mut s = Simulator::new(
            MemoryNor::new(&mut bytes).unwrap(),
            key().verifying_key().to_bytes(),
            TARGET,
        )
        .unwrap();
        assert!(
            s.begin(3, &changed).is_err(),
            "manifest mutation offset={offset}"
        );
    }
}
