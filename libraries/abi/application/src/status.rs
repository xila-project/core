/// Stable error values; outputs change only on success except the documented
/// BufferTooSmall required listing length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Status {
    Ok = 0,
    InvalidArgument = 1,
    NotFound = 2,
    PermissionDenied = 3,
    AlreadyExists = 4,
    NotADirectory = 5,
    IsADirectory = 6,
    NoSpace = 7,
    OutOfMemory = 8,
    WouldBlock = 9,
    Unsupported = 10,
    Overflow = 11,
    Unavailable = 12,
    InputOutput = 13,
    End = 14,
    BufferTooSmall = 15,
    DirectoryNotEmpty = 16,
    ConnectionRefused = 26,
    ConnectionReset = 27,
    TimedOut = 28,
    HostUnreachable = 29,
    NetworkUnreachable = 30,
    AddressInUse = 31,
    AddressNotAvailable = 32,
    NotConnected = 33,
    ConnectionAborted = 34,
    BrokenPipe = 35,
}

impl From<file_system::Error> for Status {
    fn from(error: file_system::Error) -> Self {
        use file_system::Error::*;
        match error {
            PermissionDenied | InvalidMode => Self::PermissionDenied,
            NotFound => Self::NotFound,
            DirectoryNotEmpty => Self::DirectoryNotEmpty,
            AlreadyExists | DirectoryAlreadyExists => Self::AlreadyExists,
            NotDirectory => Self::NotADirectory,
            IsDirectory => Self::IsADirectory,
            FileSystemFull | NoSpaceLeft => Self::NoSpace,
            NoMemory => Self::OutOfMemory,
            RessourceBusy => Self::WouldBlock,
            UnsupportedOperation => Self::Unsupported,
            FileTooLarge | NameTooLong => Self::Overflow,
            InvalidPath | InvalidParameter | InvalidFlags | InvalidIdentifier | InvalidContext => {
                Self::InvalidArgument
            }
            NotInitialized
            | NotMounted
            | FailedToGetTaskManagerInstance
            | FailedToGetUsersManagerInstance => Self::Unavailable,
            _ => Self::InputOutput,
        }
    }
}
impl From<virtual_file_system::Error> for Status {
    fn from(error: virtual_file_system::Error) -> Self {
        use virtual_file_system::Error::*;
        match error {
            FileSystem(error) => error.into(),
            PermissionDenied | InvalidMode => Self::PermissionDenied,
            AlreadyExists => Self::AlreadyExists,
            NotADirectory => Self::NotADirectory,
            NotAFile => Self::IsADirectory,
            TooManyOpenFiles | TooManyInodes => Self::NoSpace,
            RessourceBusy => Self::WouldBlock,
            UnsupportedOperation => Self::Unsupported,
            InvalidPath | InvalidParameter | InvalidIdentifier | InvalidOpen => {
                Self::InvalidArgument
            }
            UnavailableDriver | Orphaned => Self::Unavailable,
            _ => Self::InputOutput,
        }
    }
}

#[cfg(feature = "network")]
impl From<::network::Error> for Status {
    fn from(error: ::network::Error) -> Self {
        use ::network::Error::*;
        match error {
            PermissionDenied => Self::PermissionDenied,
            ConnectionRefused => Self::ConnectionRefused,
            ConnectionReset => Self::ConnectionReset,
            TimedOut => Self::TimedOut,
            HostUnreachable | NoRoute => Self::HostUnreachable,
            NetworkUnreachable => Self::NetworkUnreachable,
            AddressInUse => Self::AddressInUse,
            AddressNotAvailable => Self::AddressNotAvailable,
            NotConnected | SocketNotBound => Self::NotConnected,
            ConnectionAborted => Self::ConnectionAborted,
            BrokenPipe => Self::BrokenPipe,
            NotFound => Self::NotFound,
            WouldBlock | Pending | ResourceBusy => Self::WouldBlock,
            InvalidInput | InvalidData | InvalidName | InvalidPort | InvalidEndpoint
            | InvalidState => Self::InvalidArgument,
            Unsupported | UnsupportedProtocol => Self::Unsupported,
            OutOfMemory => Self::OutOfMemory,
            StorageFull | NoFreeSlot => Self::NoSpace,
            PacketTooLarge | Truncated | NameTooLong => Self::Overflow,
            NetworkDown => Self::Unavailable,
            _ => Self::InputOutput,
        }
    }
}
