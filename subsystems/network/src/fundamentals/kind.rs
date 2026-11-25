use file_system::IoErrorKind;

#[repr(u8)]
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterfaceKind {
    #[default]
    Unknown,
    Ethernet,
    WiFi,
}

impl TryFrom<u8> for InterfaceKind {
    type Error = IoErrorKind;

    fn try_from(value: u8) -> file_system::Result<Self> {
        match value {
            0 => Ok(Self::Unknown),
            1 => Ok(Self::Ethernet),
            2 => Ok(Self::WiFi),
            _ => Err(IoErrorKind::InvalidArgument),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_kind_try_from_rejects_unknown_bytes() {
        assert_eq!(InterfaceKind::try_from(0).unwrap(), InterfaceKind::Unknown);
        assert_eq!(InterfaceKind::try_from(1).unwrap(), InterfaceKind::Ethernet);
        assert_eq!(InterfaceKind::try_from(2).unwrap(), InterfaceKind::WiFi);
        assert_eq!(
            InterfaceKind::try_from(3),
            Err(IoErrorKind::InvalidArgument)
        );
        assert_eq!(
            InterfaceKind::try_from(255),
            Err(IoErrorKind::InvalidArgument)
        );
    }
}
