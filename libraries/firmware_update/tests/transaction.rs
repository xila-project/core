use firmware_update::{Capabilities, Error, FirmwareBackend, State, Update};

#[derive(Default)]
struct Backend {
    accepted: usize,
    activated: bool,
    fail: bool,
    overcount: bool,
}
impl FirmwareBackend for Backend {
    type Verified = ();
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            max_image_size: 100,
            max_chunk_size: 3,
            guarantees: 0,
        }
    }
    fn begin(&mut self, _: u64, _: &[u8]) -> Result<(), Error> {
        Ok(())
    }
    fn write(&mut self, bytes: &[u8]) -> Result<usize, Error> {
        if self.fail {
            return Err(Error::Storage);
        }
        if self.overcount {
            return Ok(bytes.len() + 1);
        }
        let n = bytes.len().min(3);
        self.accepted += n;
        Ok(n)
    }
    fn finish(&mut self) -> Result<(), Error> {
        Ok(())
    }
    fn activate(&mut self, _: ()) -> Result<(), Error> {
        self.activated = true;
        Ok(())
    }
    fn abort(&mut self) -> Result<(), Error> {
        Ok(())
    }
    fn confirm_running(&mut self) -> Result<(), Error> {
        Ok(())
    }
}
#[test]
fn activation_requires_complete_explicit_verification() {
    let mut update = Update::new(Backend::default());
    assert_eq!(update.activate(), Err(Error::InvalidState));
    update.begin(5, &[]).unwrap();
    assert_eq!(update.write(b"abcde"), Ok(3));
    assert_eq!(update.received(), 3);
    assert_eq!(update.finish(), Err(Error::Length));
    assert_eq!(update.state(), State::Failed);
    assert_eq!(update.activate(), Err(Error::InvalidState));
    update.abort().unwrap();
    update.begin(5, &[]).unwrap();
    assert_eq!(update.write(b"abcde"), Ok(3));
    assert_eq!(update.write(b"de"), Ok(2));
    update.finish().unwrap();
    assert_eq!(update.state(), State::Verified);
    assert_eq!(update.confirm_running(), Err(Error::InvalidState));
    update.activate().unwrap();
    assert_eq!(update.state(), State::PendingBoot);
    assert_eq!(update.activate(), Err(Error::InvalidState));
}
#[test]
fn excess_and_backend_errors_poison_transaction() {
    for backend in [
        Backend::default(),
        Backend {
            fail: true,
            ..Default::default()
        },
        Backend {
            overcount: true,
            ..Default::default()
        },
    ] {
        let mut u = Update::new(backend);
        u.begin(1, &[]).unwrap();
        assert!(u.write(b"ab").is_err());
        assert_eq!(u.state(), State::Failed);
        assert_eq!(u.finish(), Err(Error::InvalidState));
    }
    for backend in [
        Backend {
            fail: true,
            ..Default::default()
        },
        Backend {
            overcount: true,
            ..Default::default()
        },
    ] {
        let mut u = Update::new(backend);
        u.begin(2, &[]).unwrap();
        assert!(u.write(b"ab").is_err());
        assert_eq!(u.state(), State::Failed);
    }
}
#[test]
fn no_implicit_activation_on_drop() {
    let mut u = Update::new(Backend::default());
    assert_eq!(u.begin(101, &[]), Err(Error::Length));
    assert_eq!(u.begin(0, &[]), Err(Error::Length));
    u.begin(1, &[]).unwrap();
    u.write(b"a").unwrap();
    u.finish().unwrap();
    let backend = u.into_backend();
    assert!(!backend.activated);
}

#[test]
fn rejected_extra_write_after_finish_revokes_activation_proof() {
    let mut u = Update::new(Backend::default());
    u.begin(1, &[]).unwrap();
    u.write(b"a").unwrap();
    u.finish().unwrap();
    assert_eq!(u.write(b"extra"), Err(Error::InvalidState));
    assert_eq!(u.activate(), Err(Error::InvalidState));
    assert_eq!(u.state(), State::Failed);
}
