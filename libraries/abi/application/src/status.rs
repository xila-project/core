/// Stable error values; output arguments are written only on success.
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
    TimedOut = 20,
    Deadlock = 21,
    Busy = 22,
}

impl From<file_system::Error> for Status {
    fn from(error: file_system::Error) -> Self {
        use file_system::Error::*;
        match error {
            PermissionDenied | InvalidMode => Self::PermissionDenied,
            NotFound => Self::NotFound,
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
