//! Capability-scoped view of a native host directory.
//!
//! `HostFileSystem` acquires ambient authority exactly once, when constructed.
//! All later namespace operations are relative to the open `cap_std::fs::Dir`.
//! `cap-std` provides a portable directory-capability model for Linux/macOS/Windows.
//! On Linux (kernel 5.6+ with `openat2` and procfs mounted), file opens pin the
//! target with `O_PATH` before opening it, so device nodes are rejected without
//! invoking a host driver. Xila paths and returned names are UTF-8;
//! unrepresentable host names are omitted from directory enumeration. Final
//! symlinks are rejected for file opens on Unix, while directory resolution may
//! follow links only within the root.

#[cfg(target_os = "linux")]
use std::os::fd::{AsFd, AsRawFd};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, Path as StdPath, PathBuf},
    sync::{Arc, Mutex},
};

#[cfg(any(unix, windows))]
use cap_std::fs::MetadataExt;
#[cfg(unix)]
use cap_std::fs::OpenOptionsExt;
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions, ReadDir},
    time::{SystemClock, SystemTime},
};
use file_system::{
    AccessFlags, AttributeFlags, AttributeOperations, Attributes, BaseOperations, Context,
    CreateFlags, DirectoryOperations, Entry, Error, FileOperations, FileSystemOperations, Flags,
    Kind, MountOperations, Path, Permissions, Position, Result, Size, Time,
};

/// Options controlling a host directory mount.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostFileSystemOptions {
    /// Reject all operations that can modify host state.
    pub read_only: bool,
    /// Synthetic Xila owner; never translated to a host UID.
    pub user: users::UserIdentifier,
    /// Synthetic Xila group; never translated to a host GID.
    pub group: users::GroupIdentifier,
    /// Synthetic permissions reported for regular files.
    pub file_permissions: Permissions,
    /// Synthetic permissions reported for directories.
    pub directory_permissions: Permissions,
}

impl Default for HostFileSystemOptions {
    fn default() -> Self {
        Self {
            read_only: false,
            user: users::UserIdentifier::ROOT,
            group: users::GroupIdentifier::ROOT,
            file_permissions: Permissions::FILE_DEFAULT,
            directory_permissions: Permissions::DIRECTORY_DEFAULT,
        }
    }
}

/// A filesystem view confined to one host directory capability.
pub struct HostFileSystem {
    root: Mutex<Option<Arc<Dir>>>,
    // Used only to reopen a verified O_PATH descriptor. The descriptor number
    // comes from our pinned file handle, never from an Xila path.
    #[cfg(target_os = "linux")]
    proc_fd: Dir,
    options: HostFileSystemOptions,
}

struct FileHandle(Arc<Mutex<cap_std::fs::File>>);

struct DirectoryHandle {
    directory: Dir,
    entries: ReadDir,
    position: Size,
}

impl HostFileSystem {
    /// Open a host directory and retain its capability for this filesystem's lifetime.
    pub fn open(path: impl AsRef<StdPath>, options: HostFileSystemOptions) -> Result<Self> {
        let root_path = path.as_ref().to_path_buf();
        let root = Dir::open_ambient_dir(&root_path, ambient_authority()).map_err(map_io_error)?;
        #[cfg(target_os = "linux")]
        let proc_fd =
            Dir::open_ambient_dir("/proc/self/fd", ambient_authority()).map_err(map_io_error)?;
        Ok(Self {
            root: Mutex::new(Some(Arc::new(root))),
            #[cfg(target_os = "linux")]
            proc_fd,
            options,
        })
    }

    fn root(&self) -> Result<Arc<Dir>> {
        self.root
            .lock()
            .map_err(|_| Error::InternalError)?
            .as_ref()
            .cloned()
            .ok_or(Error::NotMounted)
    }

    fn relative_path(path: &Path) -> Result<PathBuf> {
        let source = path.as_str();
        let mut result = PathBuf::new();
        for component in StdPath::new(source).components() {
            match component {
                Component::RootDir => {}
                Component::CurDir => {}
                Component::Normal(part) => result.push(part),
                Component::ParentDir | Component::Prefix(_) => return Err(Error::InvalidPath),
            }
        }
        Ok(if result.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            result
        })
    }

    fn writable(&self) -> Result<()> {
        if self.options.read_only {
            Err(Error::PermissionDenied)
        } else {
            Ok(())
        }
    }

    fn open_options(&self) -> OpenOptions {
        let mut options = OpenOptions::new();
        #[cfg(unix)]
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_NOCTTY);
        options
    }

    /// Open a verified regular file. On Linux, O_PATH pins the final object
    /// without invoking a device driver's open method. We then reopen only
    /// that pinned descriptor through procfs, avoiding a pathname race.
    #[cfg(target_os = "linux")]
    fn open_regular_file(
        &self,
        relative: &StdPath,
        read: bool,
        write: bool,
        truncate: bool,
    ) -> Result<cap_std::fs::File> {
        let root = self.root()?;
        let pinned = rustix::fs::openat2(
            root.as_fd(),
            relative,
            rustix::fs::OFlags::PATH | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
            rustix::fs::ResolveFlags::BENEATH | rustix::fs::ResolveFlags::NO_MAGICLINKS,
        )
        .map_err(|error| map_io_error(error.into()))?;
        let pinned = cap_std::fs::File::from_std(std::fs::File::from(pinned));
        let metadata = pinned.metadata().map_err(map_io_error)?;
        if !metadata.is_file() {
            return Err(if metadata.is_dir() {
                Error::IsDirectory
            } else {
                Error::UnsupportedOperation
            });
        }

        let access = match (read, write) {
            (true, true) => rustix::fs::OFlags::RDWR,
            (false, true) => rustix::fs::OFlags::WRONLY,
            (true, false) => rustix::fs::OFlags::RDONLY,
            (false, false) => return Err(Error::InvalidFlags),
        };
        let mut flags = access | rustix::fs::OFlags::CLOEXEC | rustix::fs::OFlags::NONBLOCK;
        if truncate {
            flags |= rustix::fs::OFlags::TRUNC;
        }
        let descriptor_name = pinned.as_fd().as_raw_fd().to_string();
        let descriptor = rustix::fs::openat(
            &self.proc_fd,
            descriptor_name.as_str(),
            flags,
            rustix::fs::Mode::empty(),
        )
        .map_err(|error| map_io_error(error.into()))?;
        Ok(cap_std::fs::File::from_std(std::fs::File::from(descriptor)))
    }

    #[cfg(target_os = "linux")]
    fn open_new_regular_file(
        &self,
        relative: &StdPath,
        read: bool,
        write: bool,
    ) -> Result<cap_std::fs::File> {
        let mut options = self.open_options();
        options.read(read).write(write).create_new(true);
        self.root()?
            .open_with(relative, &options)
            .map_err(map_io_error)
    }

    #[cfg(not(target_os = "linux"))]
    fn open_regular_file(
        &self,
        relative: &StdPath,
        read: bool,
        write: bool,
        truncate: bool,
    ) -> Result<cap_std::fs::File> {
        let mut options = self.open_options();
        options.read(read).write(write).truncate(truncate);
        let file = self
            .root()?
            .open_with(relative, &options)
            .map_err(map_io_error)?;
        let metadata = file.metadata().map_err(map_io_error)?;
        if !metadata.is_file() {
            return Err(if metadata.is_dir() {
                Error::IsDirectory
            } else {
                Error::UnsupportedOperation
            });
        }
        Ok(file)
    }

    fn metadata(&self, path: &Path) -> Result<cap_std::fs::Metadata> {
        let path = Self::relative_path(path)?;
        let metadata = self.root()?.symlink_metadata(&path).map_err(map_io_error)?;
        if !metadata.is_file() && !metadata.is_dir() {
            if !metadata.file_type().is_symlink() {
                return Err(Error::UnsupportedOperation);
            }
        }
        Ok(metadata)
    }

    fn populate_attributes(&self, metadata: &cap_std::fs::Metadata, attributes: &mut Attributes) {
        let requested = attributes.get_mask();
        let mut result = Attributes::new();
        let kind = if metadata.file_type().is_symlink() {
            Some(Kind::SymbolicLink)
        } else if metadata.is_file() {
            Some(Kind::File)
        } else if metadata.is_dir() {
            Some(Kind::Directory)
        } else {
            None
        };

        if requested.contains(AttributeFlags::Kind)
            && let Some(kind) = kind
        {
            result = result.set_kind(kind);
        }
        if requested.contains(AttributeFlags::Size) && metadata.is_file() {
            result = result.set_size(metadata.len());
        }
        if requested.contains(AttributeFlags::Inode)
            && let Some(inode) = stable_inode(metadata)
        {
            result = result.set_inode(inode);
        }
        if requested.contains(AttributeFlags::Links)
            && let Some(links) = link_count(metadata)
        {
            result = result.set_links(links);
        }
        if requested.contains(AttributeFlags::Permissions) {
            result = result.set_permissions(if metadata.is_dir() {
                self.options.directory_permissions
            } else {
                self.options.file_permissions
            });
        }
        if requested.contains(AttributeFlags::User) {
            result = result.set_user(self.options.user);
        }
        if requested.contains(AttributeFlags::Group) {
            result = result.set_group(self.options.group);
        }
        if requested.contains(AttributeFlags::ModificationTime)
            && let Some(time) = metadata.modified().ok().and_then(system_time)
        {
            result = result.set_modification(time);
        }
        if requested.contains(AttributeFlags::CreationTime)
            && let Some(time) = metadata.created().ok().and_then(system_time)
        {
            result = result.set_creation(time);
        }
        if requested.contains(AttributeFlags::AccessTime)
            && let Some(time) = metadata.accessed().ok().and_then(system_time)
        {
            result = result.set_access(time);
        }
        #[cfg(unix)]
        if requested.contains(AttributeFlags::StatusTime) {
            let seconds = MetadataExt::ctime(metadata);
            if let Ok(seconds) = u64::try_from(seconds) {
                result = result.set_status(Time::new(seconds));
            }
        }
        *attributes = result;
    }

    fn get_path_attributes(&self, path: &Path, attributes: &mut Attributes) -> Result<()> {
        if attributes.get_mask().is_empty() {
            return Ok(());
        }
        let metadata = self.metadata(path)?;
        self.populate_attributes(&metadata, attributes);
        Ok(())
    }
}

fn stable_inode(metadata: &cap_std::fs::Metadata) -> Option<u64> {
    #[cfg(unix)]
    {
        Some(MetadataExt::ino(metadata))
    }
    #[cfg(windows)]
    {
        MetadataExt::file_index(metadata)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = metadata;
        None
    }
}

fn link_count(metadata: &cap_std::fs::Metadata) -> Option<u64> {
    #[cfg(unix)]
    {
        Some(MetadataExt::nlink(metadata))
    }
    #[cfg(windows)]
    {
        MetadataExt::number_of_links(metadata).map(u64::from)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = metadata;
        None
    }
}

fn system_time(value: SystemTime) -> Option<Time> {
    value
        .duration_since(SystemClock::UNIX_EPOCH)
        .ok()
        .map(Time::from)
}

fn map_io_error(error: std::io::Error) -> Error {
    #[cfg(unix)]
    if let Some(code) = error.raw_os_error() {
        return match code {
            libc::ELOOP | libc::EXDEV => Error::UnsupportedOperation,
            libc::ENOSYS => Error::UnsupportedOperation,
            libc::EMFILE | libc::ENFILE => Error::TooManyOpenFiles,
            libc::ENAMETOOLONG => Error::NameTooLong,
            _ => map_io_error_kind(error.kind()),
        };
    }
    map_io_error_kind(error.kind())
}

fn map_io_error_kind(kind: std::io::ErrorKind) -> Error {
    use std::io::ErrorKind as Io;
    match kind {
        Io::NotFound => Error::NotFound,
        Io::AlreadyExists => Error::AlreadyExists,
        Io::PermissionDenied => Error::PermissionDenied,
        Io::NotADirectory => Error::NotDirectory,
        Io::IsADirectory => Error::IsDirectory,
        Io::DirectoryNotEmpty => Error::DirectoryNotEmpty,
        Io::InvalidInput => Error::InvalidPath,
        Io::WouldBlock => Error::RessourceBusy,
        Io::OutOfMemory => Error::NoMemory,
        Io::StorageFull => Error::NoSpaceLeft,
        _ => Error::InputOutput,
    }
}

impl FileSystemOperations for HostFileSystem {
    fn writable_attributes(&self) -> AttributeFlags {
        AttributeFlags::Size
    }

    fn create_symlink(&self, target: &Path, link: &Path) -> Result<()> {
        self.writable()?;
        if !target.is_valid() || target.is_empty() || target.get_length() > 4096 {
            return Err(Error::InvalidSymbolicLink);
        }
        let link = Self::relative_path(link)?;
        let target = if target.is_absolute() {
            // Host absolute symlinks name the host root. Store a relative path
            // from the link's parent instead, preserving Xila-root semantics.
            let parent_depth = link.parent().map_or(0, |parent| {
                parent
                    .components()
                    .filter(|component| matches!(component, Component::Normal(_)))
                    .count()
            });
            let mut relative_target = PathBuf::new();
            for _ in 0..parent_depth {
                relative_target.push("..");
            }
            for component in StdPath::new(target.as_str()).components() {
                if let Component::Normal(component) = component {
                    relative_target.push(component);
                }
            }
            relative_target
        } else {
            StdPath::new(target.as_str()).to_path_buf()
        };
        self.root()?.symlink(target, &link).map_err(map_io_error)
    }

    fn read_symlink(&self, link: &Path, buffer: &mut [u8]) -> Result<usize> {
        let link = Self::relative_path(link)?;
        let target = self.root()?.read_link(&link).map_err(map_io_error)?;
        if target.is_absolute() {
            // An absolute target in a pre-existing host tree belongs to the
            // host namespace and cannot safely be exposed as an Xila target.
            return Err(Error::InvalidSymbolicLink);
        }
        let target = target
            .to_str()
            .ok_or(Error::InvalidSymbolicLink)?
            .as_bytes();
        if target.len() > buffer.len() {
            return Err(Error::NameTooLong);
        }
        buffer[..target.len()].copy_from_slice(target);
        Ok(target.len())
    }

    fn lookup_directory(&self, context: &mut Context, path: &Path) -> Result<()> {
        let relative = Self::relative_path(path)?;
        let root = self.root()?;
        let directory = root.open_dir(&relative).map_err(map_io_error)?;
        let metadata = directory.dir_metadata().map_err(map_io_error)?;
        if !metadata.is_dir() {
            return Err(Error::NotDirectory);
        }
        let entries = directory.entries().map_err(map_io_error)?;
        context.set_private_data(Box::new(DirectoryHandle {
            directory,
            entries,
            position: 0,
        }));
        Ok(())
    }

    fn lookup_file(&self, context: &mut Context, path: &Path, flags: Flags) -> Result<()> {
        let relative = Self::relative_path(path)?;
        let access = flags.get_access();
        let create = flags.get_create();
        let write = access.contains(AccessFlags::Write);
        if !access.contains(AccessFlags::Read) && !write {
            return Err(Error::InvalidFlags);
        }
        if create.contains(CreateFlags::Exclusive) && !create.contains(CreateFlags::Create) {
            return Err(Error::InvalidFlags);
        }
        if create.contains(CreateFlags::Create) && !write {
            return Err(Error::InvalidFlags);
        }
        if write {
            self.writable()?;
        }
        let read = access.contains(AccessFlags::Read);
        let truncate = create.contains(CreateFlags::Truncate);
        if truncate && !write {
            return Err(Error::InvalidFlags);
        }
        #[cfg(target_os = "linux")]
        let file = if create.contains(CreateFlags::Exclusive) {
            self.open_new_regular_file(&relative, read, write)?
        } else {
            match self.open_regular_file(&relative, read, write, truncate) {
                Ok(file) => file,
                Err(Error::NotFound) if create.contains(CreateFlags::Create) => {
                    match self.open_new_regular_file(&relative, read, write) {
                        Err(Error::AlreadyExists) => {
                            self.open_regular_file(&relative, read, write, truncate)?
                        }
                        result => result?,
                    }
                }
                Err(error) => return Err(error),
            }
        };
        #[cfg(not(target_os = "linux"))]
        let file = {
            let mut options = self.open_options();
            options.read(read).write(write);
            options.create(create.contains(CreateFlags::Create));
            options.create_new(create.contains(CreateFlags::Exclusive));
            options.truncate(truncate);
            let file = self
                .root()?
                .open_with(&relative, &options)
                .map_err(map_io_error)?;
            let metadata = file.metadata().map_err(map_io_error)?;
            if !metadata.is_file() {
                return Err(if metadata.is_dir() {
                    Error::IsDirectory
                } else {
                    Error::UnsupportedOperation
                });
            }
            file
        };
        context.set_private_data(Box::new(FileHandle(Arc::new(Mutex::new(file)))));
        Ok(())
    }

    fn create_directory(&self, path: &Path) -> Result<()> {
        self.writable()?;
        let path = Self::relative_path(path)?;
        self.root()?.create_dir(&path).map_err(map_io_error)
    }

    fn create_file(&self, path: &Path) -> Result<()> {
        self.writable()?;
        let path = Self::relative_path(path)?;
        self.root()?
            .open_with(&path, self.open_options().write(true).create_new(true))
            .map(|_| ())
            .map_err(map_io_error)
    }

    fn remove(&self, path: &Path) -> Result<()> {
        self.writable()?;
        let path = Self::relative_path(path)?;
        let root = self.root()?;
        let metadata = root.symlink_metadata(&path).map_err(map_io_error)?;
        if !metadata.file_type().is_symlink() && !metadata.is_file() && !metadata.is_dir() {
            return Err(Error::UnsupportedOperation);
        }
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            root.remove_dir(&path).map_err(map_io_error)
        } else {
            root.remove_file(&path).map_err(map_io_error)
        }
    }

    fn rename(&self, source: &Path, destination: &Path) -> Result<()> {
        self.writable()?;
        let source = Self::relative_path(source)?;
        let destination = Self::relative_path(destination)?;
        let root = self.root()?;
        let metadata = root.symlink_metadata(&source).map_err(map_io_error)?;
        if !metadata.file_type().is_symlink() && !metadata.is_file() && !metadata.is_dir() {
            return Err(Error::UnsupportedOperation);
        }
        root.rename(&source, &root, &destination)
            .map_err(map_io_error)
    }

    fn get_attributes(&self, path: &Path, attributes: &mut Attributes) -> Result<()> {
        self.get_path_attributes(path, attributes)
    }

    fn set_attributes(&self, path: &Path, attributes: &Attributes) -> Result<()> {
        let mask = attributes.get_mask();
        if mask.is_empty() {
            return Ok(());
        }
        if mask != AttributeFlags::Size {
            return Err(Error::UnsupportedOperation);
        }
        self.writable()?;
        let size = *attributes.get_size().ok_or(Error::InvalidParameter)?;
        let path = Self::relative_path(path)?;
        let file = self.open_regular_file(&path, false, true, false)?;
        file.set_len(size).map_err(map_io_error)
    }
}

impl BaseOperations for HostFileSystem {
    fn read(
        &self,
        context: &mut Context,
        _cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        absolute_position: Size,
    ) -> core::task::Poll<Result<usize>> {
        file_system::ready(|| {
            let handle = context
                .get_private_data_of_type::<FileHandle>()
                .ok_or(Error::InvalidContext)?;
            let mut file = handle.0.lock().map_err(|_| Error::RessourceBusy)?;
            file.seek(SeekFrom::Start(absolute_position))
                .map_err(map_io_error)?;
            file.read(buffer).map_err(map_io_error)
        })
    }

    fn write(
        &self,
        context: &mut Context,
        _cx: &mut core::task::Context<'_>,
        buffer: &[u8],
        absolute_position: Size,
    ) -> core::task::Poll<Result<usize>> {
        file_system::ready(|| {
            self.writable()?;
            let handle = context
                .get_private_data_of_type::<FileHandle>()
                .ok_or(Error::InvalidContext)?;
            let mut file = handle.0.lock().map_err(|_| Error::RessourceBusy)?;
            file.seek(SeekFrom::Start(absolute_position))
                .map_err(map_io_error)?;
            file.write(buffer).map_err(map_io_error)
        })
    }

    fn set_position(
        &self,
        context: &mut Context,
        current: Size,
        position: &Position,
    ) -> Result<Size> {
        let file = context
            .get_private_data_of_type::<FileHandle>()
            .ok_or(Error::InvalidContext)?;
        let mut file = file.0.lock().map_err(|_| Error::RessourceBusy)?;
        let target = match position {
            Position::Start(value) => *value,
            Position::Current(offset) => current
                .checked_add_signed(*offset)
                .ok_or(Error::InvalidParameter)?,
            Position::End(offset) => file.seek(SeekFrom::End(*offset)).map_err(map_io_error)?,
        };
        if !matches!(position, Position::End(_)) {
            file.seek(SeekFrom::Start(target)).map_err(map_io_error)?;
        }
        Ok(target)
    }

    fn flush(
        &self,
        context: &mut Context,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Result<()>> {
        file_system::ready(|| {
            let file = context
                .get_private_data_of_type::<FileHandle>()
                .ok_or(Error::InvalidContext)?;
            file.0
                .lock()
                .map_err(|_| Error::RessourceBusy)?
                .sync_all()
                .map_err(map_io_error)
        })
    }

    fn clone_context(&self, context: &Context) -> Result<Context> {
        if let Some(handle) = context.get_private_data_of_type::<FileHandle>() {
            return Ok(Context::new(Some(FileHandle(handle.0.clone()))));
        }
        let handle = context
            .get_private_data_of_type::<DirectoryHandle>()
            .ok_or(Error::InvalidContext)?;
        let directory = handle.directory.try_clone().map_err(map_io_error)?;
        let mut entries = directory.entries().map_err(map_io_error)?;
        for _ in 0..handle.position {
            if entries.next().transpose().map_err(map_io_error)?.is_none() {
                break;
            }
        }
        Ok(Context::new(Some(DirectoryHandle {
            directory,
            entries,
            position: handle.position,
        })))
    }

    fn close(&self, context: &mut Context) -> Result<()> {
        if context.take_private_data_of_type::<FileHandle>().is_some() {
            Ok(())
        } else {
            Err(Error::InvalidContext)
        }
    }
}

impl FileOperations for HostFileSystem {}

impl AttributeOperations for HostFileSystem {
    fn get_attributes(&self, context: &mut Context, attributes: &mut Attributes) -> Result<()> {
        if attributes.get_mask().is_empty() {
            return Ok(());
        }
        let metadata = if let Some(file) = context.get_private_data_of_type::<FileHandle>() {
            file.0
                .lock()
                .map_err(|_| Error::RessourceBusy)?
                .metadata()
                .map_err(map_io_error)?
        } else if let Some(directory) = context.get_private_data_of_type::<DirectoryHandle>() {
            directory.directory.dir_metadata().map_err(map_io_error)?
        } else {
            return Err(Error::InvalidContext);
        };
        self.populate_attributes(&metadata, attributes);
        Ok(())
    }

    fn set_attributes(&self, context: &mut Context, attributes: &Attributes) -> Result<()> {
        let mask = attributes.get_mask();
        if mask.is_empty() {
            return Ok(());
        }
        if mask != AttributeFlags::Size {
            return Err(Error::UnsupportedOperation);
        }
        self.writable()?;
        let size = *attributes.get_size().ok_or(Error::InvalidParameter)?;
        let Some(file) = context.get_private_data_of_type::<FileHandle>() else {
            return if context
                .get_private_data_of_type::<DirectoryHandle>()
                .is_some()
            {
                Err(Error::IsDirectory)
            } else {
                Err(Error::InvalidContext)
            };
        };
        file.0
            .lock()
            .map_err(|_| Error::RessourceBusy)?
            .set_len(size)
            .map_err(map_io_error)
    }
}

impl DirectoryOperations for HostFileSystem {
    fn read(&self, context: &mut Context) -> Result<Option<Entry>> {
        loop {
            let (entry, position) = {
                let handle = context
                    .get_private_data_mutable_of_type::<DirectoryHandle>()
                    .ok_or(Error::InvalidContext)?;
                let entry = handle.entries.next().transpose().map_err(map_io_error)?;
                if entry.is_some() {
                    handle.position += 1;
                }
                (entry, handle.position)
            };
            let Some(entry) = entry else { return Ok(None) };
            let name = match entry.file_name().into_string() {
                Ok(name) => name,
                Err(_) => continue,
            };
            let kind = entry.file_type().map_err(map_io_error)?;
            let file_type = if kind.is_symlink() {
                Kind::SymbolicLink
            } else if kind.is_file() {
                Kind::File
            } else if kind.is_dir() {
                Kind::Directory
            } else {
                continue;
            };
            // DirEntry::metadata follows the target, which would hide dangling
            // links and make listing an escaping link fail. We do not expose a
            // fabricated target size for links.
            let (inode, size) = if kind.is_symlink() {
                (position, 0)
            } else {
                let metadata = entry.metadata().map_err(map_io_error)?;
                (stable_inode(&metadata).unwrap_or(position), metadata.len())
            };
            return Ok(Some(Entry::new(inode, name, file_type, size)));
        }
    }

    fn get_position(&self, context: &mut Context) -> Result<Size> {
        context
            .get_private_data_of_type::<DirectoryHandle>()
            .map(|handle| handle.position)
            .ok_or(Error::InvalidContext)
    }

    fn set_position(&self, context: &mut Context, position: Size) -> Result<()> {
        let handle = context
            .get_private_data_mutable_of_type::<DirectoryHandle>()
            .ok_or(Error::InvalidContext)?;
        handle.entries = handle.directory.entries().map_err(map_io_error)?;
        handle.position = 0;
        for _ in 0..position {
            if handle
                .entries
                .next()
                .transpose()
                .map_err(map_io_error)?
                .is_none()
            {
                break;
            }
            handle.position += 1;
        }
        Ok(())
    }

    fn rewind(&self, context: &mut Context) -> Result<()> {
        DirectoryOperations::set_position(self, context, 0)
    }

    fn close(&self, context: &mut Context) -> Result<()> {
        if context
            .take_private_data_of_type::<DirectoryHandle>()
            .is_some()
        {
            Ok(())
        } else {
            Err(Error::InvalidContext)
        }
    }
}

impl MountOperations for HostFileSystem {
    fn mount(&self) -> Result<()> {
        if self
            .root
            .lock()
            .map_err(|_| Error::InternalError)?
            .is_some()
        {
            Ok(())
        } else {
            Err(Error::NotMounted)
        }
    }

    fn unmount(&self) -> Result<()> {
        // Like a Linux mount, detaching the namespace root does not revoke
        // already-open file or directory descriptors. New path lookups fail,
        // while existing contexts remain usable until they are closed.
        self.root
            .lock()
            .map_err(|_| Error::InternalError)?
            .take()
            .map(drop)
            .ok_or(Error::NotMounted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use file_system::{AccessFlags, AttributeFlags, Flags};
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp_root() -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "xila-hostfs-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn confines_parent_absolute_and_symlink_paths() {
        let root = temp_root();
        let outside = root.with_extension("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret"), b"secret").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();
        let filesystem = HostFileSystem::open(&root, HostFileSystemOptions::default()).unwrap();
        assert_eq!(
            HostFileSystem::relative_path(Path::new("/../outside/secret")),
            Err(Error::InvalidPath)
        );
        let mut context = Context::new_empty();
        assert_eq!(
            filesystem.lookup_file(
                &mut context,
                Path::new("/../secret"),
                AccessFlags::Read.into()
            ),
            Err(Error::InvalidPath)
        );
        #[cfg(unix)]
        assert!(
            filesystem
                .lookup_file(
                    &mut context,
                    Path::new("/escape/secret"),
                    AccessFlags::Read.into()
                )
                .is_err()
        );
        assert_eq!(
            filesystem.rename(Path::new("/../secret"), Path::new("/inside")),
            Err(Error::InvalidPath)
        );
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn hostfs_round_trips_file_io_and_enforces_read_only() {
        let root = temp_root();
        let filesystem = HostFileSystem::open(&root, HostFileSystemOptions::default()).unwrap();
        filesystem.create_file(Path::new("/file")).unwrap();
        let mut context = Context::new_empty();
        filesystem
            .lookup_file(
                &mut context,
                Path::new("/file"),
                Flags::from(AccessFlags::READ_WRITE),
            )
            .unwrap();
        assert_eq!(
            BaseOperations::write_sync(&filesystem, &mut context, b"data", 0),
            Ok(4)
        );
        fs::write(root.join("file"), b"host").unwrap();
        let mut data = [0; 4];
        assert_eq!(
            BaseOperations::read_sync(&filesystem, &mut context, &mut data, 0),
            Ok(4)
        );
        assert_eq!(&data, b"host");
        BaseOperations::close(&filesystem, &mut context).unwrap();
        let readonly = HostFileSystem::open(
            &root,
            HostFileSystemOptions {
                read_only: true,
                ..HostFileSystemOptions::default()
            },
        )
        .unwrap();
        assert_eq!(
            readonly.create_file(Path::new("/blocked")),
            Err(Error::PermissionDenied)
        );
        let mut attrs = Attributes::new().set_mask(AttributeFlags::Size);
        FileSystemOperations::get_attributes(&readonly, Path::new("/file"), &mut attrs).unwrap();
        assert_eq!(attrs.get_size(), Some(&4));
        assert!(root.join("file").exists());
        assert_eq!(
            readonly.create_directory(Path::new("/blocked-dir")),
            Err(Error::PermissionDenied)
        );
        assert_eq!(
            readonly.remove(Path::new("/file")),
            Err(Error::PermissionDenied)
        );
        assert_eq!(
            readonly.rename(Path::new("/file"), Path::new("/renamed")),
            Err(Error::PermissionDenied)
        );
        assert_eq!(
            FileSystemOperations::set_attributes(
                &readonly,
                Path::new("/file"),
                &Attributes::new().set_size(1),
            ),
            Err(Error::PermissionDenied)
        );
        let mut read_write_context = Context::new_empty();
        filesystem
            .lookup_file(
                &mut read_write_context,
                Path::new("/file"),
                Flags::from(AccessFlags::READ_WRITE),
            )
            .unwrap();
        assert_eq!(
            BaseOperations::write_sync(&readonly, &mut read_write_context, b"x", 0),
            Err(Error::PermissionDenied)
        );
        assert_eq!(
            AttributeOperations::set_attributes(
                &readonly,
                &mut read_write_context,
                &Attributes::new().set_size(1),
            ),
            Err(Error::PermissionDenied)
        );
        BaseOperations::close(&filesystem, &mut read_write_context).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reports_stable_inode_and_synthetic_identity() {
        let root = temp_root();
        fs::write(root.join("file"), b"x").unwrap();
        let filesystem = HostFileSystem::open(
            &root,
            HostFileSystemOptions {
                user: users::UserIdentifier::new(42),
                group: users::GroupIdentifier::new(43),
                ..HostFileSystemOptions::default()
            },
        )
        .unwrap();
        let mut attrs = Attributes::new().set_mask(
            AttributeFlags::Kind
                | AttributeFlags::Inode
                | AttributeFlags::Links
                | AttributeFlags::User
                | AttributeFlags::Group
                | AttributeFlags::Permissions,
        );
        FileSystemOperations::get_attributes(&filesystem, Path::new("/file"), &mut attrs).unwrap();
        assert_eq!(attrs.get_kind(), Some(&Kind::File));
        assert!(attrs.get_inode().is_some());
        assert!(attrs.get_links().is_some());
        assert_eq!(attrs.get_user(), Some(&users::UserIdentifier::new(42)));
        assert_eq!(attrs.get_group(), Some(&users::GroupIdentifier::new(43)));
        assert_eq!(attrs.get_permissions(), Some(&Permissions::FILE_DEFAULT));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn satisfies_generic_filesystem_metadata_contract() {
        let root = temp_root();
        let filesystem = HostFileSystem::open(&root, HostFileSystemOptions::default()).unwrap();
        file_system::tests::test_attribute_operations_with_capabilities(
            &filesystem,
            file_system::tests::FileSystemTestCapabilities {
                readable: AttributeFlags::Inode
                    | AttributeFlags::Kind
                    | AttributeFlags::Size
                    | AttributeFlags::Links
                    | AttributeFlags::ModificationTime
                    | AttributeFlags::AccessTime
                    | AttributeFlags::Permissions
                    | AttributeFlags::User
                    | AttributeFlags::Group,
                writable: AttributeFlags::Size,
            },
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn mount_lifecycle_releases_the_root_capability() {
        let root = temp_root();
        let filesystem = HostFileSystem::open(&root, HostFileSystemOptions::default()).unwrap();
        MountOperations::mount(&filesystem).unwrap();
        MountOperations::unmount(&filesystem).unwrap();
        assert_eq!(
            filesystem.create_file(Path::new("/after-unmount")),
            Err(Error::NotMounted)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn open_file_context_survives_unmount_like_a_linux_descriptor() {
        let root = temp_root();
        let filesystem = HostFileSystem::open(&root, HostFileSystemOptions::default()).unwrap();
        filesystem.create_file(Path::new("/file")).unwrap();
        filesystem
            .create_directory(Path::new("/directory"))
            .unwrap();
        let mut context = Context::new_empty();
        let mut directory_context = Context::new_empty();
        filesystem
            .lookup_file(
                &mut context,
                Path::new("/file"),
                Flags::from(AccessFlags::READ_WRITE),
            )
            .unwrap();
        filesystem
            .lookup_directory(&mut directory_context, Path::new("/directory"))
            .unwrap();

        MountOperations::unmount(&filesystem).unwrap();
        assert_eq!(
            BaseOperations::write_sync(&filesystem, &mut context, b"still open", 0),
            Ok(10)
        );
        assert_eq!(
            filesystem.create_file(Path::new("/new-path")),
            Err(Error::NotMounted)
        );
        DirectoryOperations::set_position(&filesystem, &mut directory_context, 0).unwrap();
        let mut attributes = Attributes::new().set_mask(AttributeFlags::Kind);
        AttributeOperations::get_attributes(&filesystem, &mut directory_context, &mut attributes)
            .unwrap();
        assert_eq!(attributes.get_kind(), Some(&Kind::Directory));
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_host_special_files_without_blocking() {
        let root = temp_root();
        let fifo = root.join("pipe");
        let fifo_c = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) }, 0);
        let filesystem = HostFileSystem::open(&root, HostFileSystemOptions::default()).unwrap();
        let mut context = Context::new_empty();
        assert_eq!(
            filesystem.lookup_file(&mut context, Path::new("/pipe"), AccessFlags::Read.into()),
            Err(Error::UnsupportedOperation)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parent_replacement_cannot_redirect_creates_outside_root() {
        use std::thread;

        let root = temp_root();
        let outside = root.with_extension("outside");
        fs::create_dir(&outside).unwrap();
        fs::create_dir(root.join("parent")).unwrap();
        let filesystem =
            Arc::new(HostFileSystem::open(&root, HostFileSystemOptions::default()).unwrap());
        let replacing_root = root.clone();
        let replacing_outside = outside.clone();
        let replacer = thread::spawn(move || {
            for _ in 0..300 {
                let parent = replacing_root.join("parent");
                let moved = replacing_root.join("parent-moved");
                let _ = fs::rename(&parent, &moved);
                let _ = std::os::unix::fs::symlink(&replacing_outside, &parent);
                let _ = fs::remove_file(&parent);
                let _ = fs::rename(&moved, &parent);
            }
        });
        for _ in 0..300 {
            let _ = filesystem.create_file(Path::new("/parent/pwned"));
            let _ = fs::remove_file(root.join("parent/pwned"));
        }
        replacer.join().unwrap();
        assert!(!outside.join("pwned").exists());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }
}
