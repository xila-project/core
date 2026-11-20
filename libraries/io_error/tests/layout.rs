use core::mem::size_of;
use io_error::{Kind, ResolveError, UnknownKind};

#[test]
fn values_stay_two_bytes_on_every_target() {
    assert_eq!(size_of::<Kind>(), 2);
    assert_eq!(size_of::<ResolveError>(), 2);
    assert_eq!(size_of::<Option<Kind>>(), 2);
    assert_eq!(size_of::<Result<(), Kind>>(), 2);
    assert_eq!(size_of::<Result<(), ResolveError>>(), 2);
    assert_eq!(size_of::<UnknownKind>(), 2);
}

#[test]
fn ids_stay_inside_their_bands() {
    for kind in Kind::ALL {
        let id = kind.id();
        assert_ne!(id, 0, "zero is success");
        // Bands 0x01 to 0x05 are assigned; 0x06 is reserved for future kinds
        // and 0x07 for native-ABI-only outcomes, which are never kinds.
        assert!((0x0101..=0x05FF).contains(&id), "{id:#06x}");
    }
    for id in (0x0600..=0x06FF).chain(0x0700..=0x07FF) {
        assert!(Kind::try_from(id).is_err(), "{id:#06x}");
    }
}

#[test]
fn ids_are_listed_in_ascending_order() {
    assert!(Kind::ALL.windows(2).all(|pair| pair[0].id() < pair[1].id()));
}

#[test]
fn unknown_ids_are_reported_not_reinterpreted() {
    assert_eq!(Kind::try_from(0), Err(UnknownKind(0)));
    assert_eq!(Kind::try_from(u16::MAX), Err(UnknownKind(u16::MAX)));
    assert_eq!(
        UnknownKind(0x0abc).to_string(),
        "Unknown I/O error ID 0x0abc"
    );
}
