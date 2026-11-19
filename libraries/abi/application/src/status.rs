/// Native status values mirror the unified error kinds one-to-one. Zero is success.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Status {
    Ok = 0,

    InvalidArgument = 0x0101,
    InvalidData = 0x0102,
    PermissionDenied = 0x0103,
    OperationNotPermitted = 0x0104,
    InputOutput = 0x0105,
    OutOfMemory = 0x0106,
    WouldBlock = 0x0107,
    ResourceBusy = 0x0108,
    Interrupted = 0x0109,
    TimedOut = 0x010A,
    Unsupported = 0x010B,
    Overflow = 0x010C,
    WriteZero = 0x010D,
    NoBuffers = 0x010E,
    NotFound = 0x010F,
    AlreadyExists = 0x0110,
    InvalidFlags = 0x0111,
    InvalidState = 0x0112,
    NotImplemented = 0x0113,
    // Reserved for ENOEXEC, ECHILD, E2BIG and EDEADLK respectively.
    ExecFormat = 0x0114,
    NoChild = 0x0115,
    ArgumentListTooLong = 0x0116,
    Deadlock = 0x0117,

    BadDescriptor = 0x0201,
    InvalidPath = 0x0202,
    MissingPathComponent = 0x0203,
    MissingFinalEntry = 0x0204,
    NameTooLong = 0x0205,
    SymlinkLoop = 0x0206,
    NotADirectory = 0x0207,
    IsADirectory = 0x0208,
    DirectoryNotEmpty = 0x0209,
    NoSpace = 0x020A,
    FileTooLarge = 0x020B,
    ProcessFileLimit = 0x020C,
    SystemFileLimit = 0x020D,
    CrossDevice = 0x020E,
    PathEscapesRoot = 0x020F,
    CorruptData = 0x0210,
    MissingAttribute = 0x0211,
    StaleHandle = 0x0212,
    ReadOnlyFileSystem = 0x0213,
    InvalidSeek = 0x0214,

    Unavailable = 0x0301,
    NoSuchTask = 0x0302,
    // Reserved for ENOTTY.
    InappropriateIoControl = 0x0303,

    ConnectionRefused = 0x0401,
    ConnectionReset = 0x0402,
    ConnectionAborted = 0x0403,
    NotConnected = 0x0404,
    AlreadyConnected = 0x0405,
    AddressInUse = 0x0406,
    AddressNotAvailable = 0x0407,
    DestinationRequired = 0x0408,
    HostUnreachable = 0x0409,
    NetworkUnreachable = 0x040A,
    NetworkDown = 0x040B,
    BrokenPipe = 0x040C,
    MessageTooLarge = 0x040D,
    DatagramTruncated = 0x040E,
    SocketOperationUnsupported = 0x040F,
    ProtocolUnsupported = 0x0410,
    AddressFamilyUnsupported = 0x0411,
    InProgress = 0x0412,
    AlreadyInProgress = 0x0413,
    ProtocolError = 0x0414,
    NotASocket = 0x0415,

    InvalidDnsName = 0x0501,
    UnsupportedDnsQuery = 0x0502,
    NameNotResolved = 0x0503,
    ResolverTemporaryFailure = 0x0504,
    ResolverFailure = 0x0505,

    // 0x0600–0x06FF: reserved for future unified kinds.
    // 0x0700–0x07FF: native ABI-only outcomes.
    End = 0x0701,
    BufferTooSmall = 0x0702,
}

impl From<file_system::Error> for Status {
    fn from(error: file_system::Error) -> Self {
        use file_system::Error::*;
        match error {
            PermissionDenied => Self::PermissionDenied,
            InvalidMode | InvalidIdentifier => Self::BadDescriptor,
            NotFound => Self::NotFound,
            DirectoryNotEmpty => Self::DirectoryNotEmpty,
            AlreadyExists | DirectoryAlreadyExists => Self::AlreadyExists,
            NotDirectory => Self::NotADirectory,
            IsDirectory => Self::IsADirectory,
            FileSystemFull | NoSpaceLeft => Self::NoSpace,
            NoMemory => Self::OutOfMemory,
            RessourceBusy => Self::ResourceBusy,
            UnsupportedOperation => Self::Unsupported,
            FileTooLarge => Self::FileTooLarge,
            NameTooLong => Self::NameTooLong,
            InvalidPath => Self::InvalidPath,
            InvalidFlags => Self::InvalidFlags,
            InvalidParameter | InvalidContext => Self::InvalidArgument,
            NoAttribute => Self::MissingAttribute,
            InvalidFile | InvalidDirectory | Corrupted => Self::CorruptData,
            InvalidInode => Self::StaleHandle,
            // Exhaustion of shared poll or pipe slots maps to ENFILE.
            TooManyOpenFiles => Self::SystemFileLimit,
            NotInitialized
            | NotMounted
            | FailedToGetTaskManagerInstance
            | FailedToGetUsersManagerInstance => Self::Unavailable,
            FailedToInitializeFileSystem
            | FileSystemError
            | InvalidSymbolicLink
            | Unknown
            | FailedToGetTaskInformations
            | FailedToGetUsersInformations
            | TooManyMountedFileSystems
            | InternalError
            | AlreadyInitialized
            | InputOutput
            | TimeError
            | AlreadyMounted
            | Other => Self::InputOutput,
        }
    }
}
impl From<task::Error> for Status {
    fn from(error: task::Error) -> Self {
        use task::Error::*;
        match error {
            InvalidTaskIdentifier => Self::NoSuchTask,
            InvalidEnvironmentVariable => Self::NotFound,
            // The task table is full: retry after another task exits.
            TooManyTasks | TooManySpawners | NoSpawnerAvailable => Self::ResourceBusy,
            InvalidSpawnerIdentifier
            | ThreadNotRegistered
            | ThreadAlreadyRegistered
            | NoThreadForTask
            | AlreadyInitialized
            | AlreadySet => Self::InvalidState,
            NotInitialized => Self::Unavailable,
            FailedToCreateThread | FailedToSpawnThread => Self::InputOutput,
        }
    }
}
impl From<virtual_file_system::Error> for Status {
    fn from(error: virtual_file_system::Error) -> Self {
        use virtual_file_system::Error::*;
        match error {
            FileSystem(error) => error.into(),
            PermissionDenied => Self::PermissionDenied,
            InvalidMode | InvalidIdentifier => Self::BadDescriptor,
            AlreadyExists => Self::AlreadyExists,
            NotADirectory => Self::NotADirectory,
            NotAFile => Self::IsADirectory,
            // Exhaustion of shared poll or pipe slots maps to ENFILE.
            TooManyOpenFiles => Self::SystemFileLimit,
            TooManyInodes => Self::NoSpace,
            RessourceBusy => Self::ResourceBusy,
            UnsupportedOperation => Self::Unsupported,
            InvalidPath => Self::InvalidPath,
            InvalidParameter | InvalidOpen => Self::InvalidArgument,
            InvalidFileSystem => Self::InvalidData,
            Orphaned | InvalidInode => Self::StaleHandle,
            UnavailableDriver => Self::Unavailable,
            MissingAttribute => Self::MissingAttribute,
            PathEscapesRoot => Self::PathEscapesRoot,
            CrossDevice => Self::CrossDevice,
            AlreadyInitialized
            | FailedToGetTaskInformations
            | Time(_)
            | Users(_)
            | Task(_)
            | FailedToWrite
            | DelimiterNotFound => Self::InputOutput,
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
            WouldBlock => Self::WouldBlock,
            Pending => Self::InProgress,
            ResourceBusy => Self::ResourceBusy,
            InvalidInput | InvalidPort | InvalidEndpoint => Self::InvalidArgument,
            InvalidData => Self::InvalidData,
            InvalidName => Self::InvalidDnsName,
            InvalidState => Self::InvalidState,
            Unsupported => Self::Unsupported,
            UnsupportedProtocol => Self::ProtocolUnsupported,
            OutOfMemory => Self::OutOfMemory,
            StorageFull | NoFreeSlot => Self::NoBuffers,
            PacketTooLarge => Self::MessageTooLarge,
            Truncated => Self::DatagramTruncated,
            NameTooLong => Self::NameTooLong,
            NetworkDown => Self::NetworkDown,
            WriteZero => Self::WriteZero,
            Interrupted => Self::Interrupted,
            Deadlock => Self::Deadlock,
            Failed => Self::ResolverFailure,
            AlreadyExists | DuplicateIdentifier => Self::AlreadyExists,
            InvalidIdentifier => Self::BadDescriptor,
            FailedToGenerateSeed(error) => error.into(),
            FailedToMountDevice(error) => error.into(),
            UnexpectedEndOfFile | FailedToSpawnNetworkTask(_) | Other => Self::InputOutput,
        }
    }
}
