use io_error::{GetAddrInfoError, Kind, ResolveError};

#[test]
fn every_resolver_error_has_an_explicit_row() {
    use GetAddrInfoError::*;
    let rows = [
        (ResolveError::InvalidName, Kind::InvalidDnsName, EAI_NONAME),
        (ResolveError::NameTooLong, Kind::NameTooLong, EAI_NONAME),
        (
            ResolveError::UnsupportedQuery,
            Kind::UnsupportedDnsQuery,
            EAI_FAIL,
        ),
        (
            ResolveError::UnsupportedFamily,
            Kind::AddressFamilyUnsupported,
            EAI_FAMILY,
        ),
        (ResolveError::NoAnswer, Kind::NameNotResolved, EAI_NONAME),
        (
            ResolveError::TemporaryFailure,
            Kind::ResolverTemporaryFailure,
            EAI_AGAIN,
        ),
        (ResolveError::Failure, Kind::ResolverFailure, EAI_FAIL),
        (ResolveError::OutOfMemory, Kind::OutOfMemory, EAI_MEMORY),
        (ResolveError::NoQuerySlots, Kind::NoBuffers, EAI_AGAIN),
        (
            ResolveError::NoInterface,
            Kind::DeviceUnavailable,
            EAI_SYSTEM,
        ),
        (ResolveError::NetworkDown, Kind::NetworkDown, EAI_SYSTEM),
        (
            ResolveError::NetworkUnreachable,
            Kind::NetworkUnreachable,
            EAI_SYSTEM,
        ),
        (ResolveError::TimedOut, Kind::TimedOut, EAI_AGAIN),
        (ResolveError::SystemIo, Kind::InputOutput, EAI_SYSTEM),
    ];
    assert_eq!(rows.len(), ResolveError::ALL.len());
    for (error, kind, getaddrinfo) in rows {
        assert_eq!(error.kind(), kind);
        assert_eq!(Kind::from(error), kind);
        // A resolver error carries the stable ID of the kind it projects to.
        assert_eq!(error.id(), kind.id());
        assert_eq!(ResolveError::try_from(error.id()), Ok(error));
        assert_eq!(error.getaddrinfo(), getaddrinfo);
        assert_eq!(error.to_string(), kind.to_string());
    }
}

#[test]
fn only_resolver_ids_decode_to_resolver_errors() {
    let known: Vec<u16> = ResolveError::ALL.iter().map(|e| e.id()).collect();
    for id in 0..=u16::MAX {
        assert_eq!(
            ResolveError::try_from(id).is_ok(),
            known.contains(&id),
            "{id:#06x}"
        );
    }
    // A kind that is not a resolver outcome is not decodable as one.
    assert!(ResolveError::try_from(Kind::PermissionDenied.id()).is_err());
}

#[test]
fn a_negative_answer_is_distinct_from_an_unknown_failure() {
    assert_ne!(ResolveError::NoAnswer, ResolveError::Failure);
    assert_ne!(
        ResolveError::NoAnswer.to_string(),
        ResolveError::Failure.to_string()
    );
}
