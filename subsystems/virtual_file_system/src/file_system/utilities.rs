use super::DirectoryCapability;
use crate::{AccessTimePolicy, IoErrorKind, Result, ResultExt, VirtualFileSystem, pipe::Pipe};
use alloc::{borrow::ToOwned, string::String, vec::Vec};
use exported_file_system::{
    AccessFlags, AttributeFlags, AttributeOperations, Attributes, BaseOperations, BlockDevice,
    CharacterDevice, Context, DirectoryOperations, FileSystemOperations, Flags, Inode, Kind, Path,
    PathOwned, Permission, Permissions, Time,
};
use task::TaskIdentifier;
use users::{GroupIdentifier, UserIdentifier};

const MAX_PATH_LENGTH: usize = 4096;
const MAX_SYMLINKS: usize = 40;

/// Registry of mounted devices or pipes, sorted by inode.
///
/// A device mounts once and its entry stays until it unmounts, so a sorted
/// vector is sufficient. A `BTreeMap` for each of the three value types costs
/// about 6 KB of flash on a Cortex-M4 target, because each map instantiates
/// the node split, merge and rebalance code.
pub(super) struct InodeMap<T>(Vec<(Inode, T)>);

impl<T> InodeMap<T> {
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    fn position(&self, inode: Inode) -> core::result::Result<usize, usize> {
        self.0.binary_search_by_key(&inode, |&(key, _)| key)
    }

    /// Return the lowest inode above 0 that has no entry.
    pub fn allocate_inode(&self) -> Option<Inode> {
        let mut candidate: Inode = 1;
        for &(inode, _) in &self.0 {
            if inode > candidate {
                break;
            }
            if inode == candidate {
                candidate = candidate.checked_add(1)?;
            }
        }
        (candidate < Inode::MAX).then_some(candidate)
    }

    /// Add the entry, or replace the value of an entry with the same inode.
    pub fn insert(&mut self, inode: Inode, value: T) {
        match self.position(inode) {
            Ok(index) => self.0[index].1 = value,
            Err(index) => self.0.insert(index, (inode, value)),
        }
    }

    pub fn get_mut(&mut self, inode: &Inode) -> Option<&mut T> {
        let index = self.position(*inode).ok()?;
        Some(&mut self.0[index].1)
    }

    pub fn remove(&mut self, inode: &Inode) -> Option<T> {
        let index = self.position(*inode).ok()?;
        Some(self.0.remove(index).1)
    }

    /// Iterate over the values in ascending inode order.
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.0.iter_mut().map(|(_, value)| value)
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

pub(super) struct InternalFileSystem {
    pub reference_count: usize,
    /// Whether this filesystem participates in path resolution. Inactive
    /// entries remain here while open contexts still hold its static handle.
    pub is_mounted: bool,
    pub mount_point: PathOwned,
    pub access_time_policy: AccessTimePolicy,
    pub file_system: &'static dyn FileSystemOperations,
}

#[derive(Clone, Copy)]
pub(super) enum LookupRoot<'a> {
    Path(&'a Path),
    Pinned(&'a DirectoryCapability<'a>),
}

impl<'a> LookupRoot<'a> {
    pub fn is_handle_relative(self) -> bool {
        matches!(self, Self::Pinned(directory) if !directory.pins_path)
    }

    pub fn path(self) -> &'a Path {
        match self {
            Self::Path(path) => path,
            Self::Pinned(directory) => &directory.path,
        }
    }
}

pub(super) struct InternalBlockDevice {
    pub reference_count: usize,
    pub device: &'static dyn BlockDevice,
}

pub(super) struct InternalCharacterDevice {
    pub reference_count: usize,
    pub device: &'static dyn CharacterDevice,
}

pub(super) struct InternalPipe {
    pub pipe: synchronization::Arc<Pipe>,
}

pub(super) type FileSystemsArray = Vec<InternalFileSystem>;
pub(super) type CharacterDevicesMap = InodeMap<InternalCharacterDevice>;
pub(super) type BlockDevicesMap = InodeMap<InternalBlockDevice>;
pub(super) type PipeMap = InodeMap<InternalPipe>;

pub(super) enum ResolvedPath<'a> {
    Borrowed(&'a Path),
    Owned(PathOwned),
}

/// Pinned directory handle used during component-wise traversal on backends
/// that perform entry operations relative to an opened directory [`Context`].
pub(super) struct OpenDirectoryHandle {
    file_system: &'static dyn FileSystemOperations,
    context: Context,
    closed: bool,
}

impl OpenDirectoryHandle {
    pub fn open_root(file_system: &'static dyn FileSystemOperations) -> Result<Self> {
        let mut context = Context::new_empty();
        file_system.lookup_directory(&mut context, Path::ROOT)?;
        Ok(Self {
            file_system,
            context,
            closed: false,
        })
    }

    pub fn open_child(&self, name: &str) -> Result<Self> {
        let mut context = Context::new_empty();
        self.file_system
            .lookup_directory_entry(&self.context, name, &mut context)?;
        Ok(Self {
            file_system: self.file_system,
            context,
            closed: false,
        })
    }

    pub fn duplicate(&self) -> Result<Self> {
        let context = self.file_system.clone_context(&self.context)?;
        Ok(Self {
            file_system: self.file_system,
            context,
            closed: false,
        })
    }

    pub fn context(&self) -> &Context {
        &self.context
    }

    pub fn context_mutable(&mut self) -> &mut Context {
        &mut self.context
    }

    pub fn into_context(mut self) -> Context {
        self.closed = true;
        core::mem::replace(&mut self.context, Context::new_empty())
    }
}

impl Drop for OpenDirectoryHandle {
    fn drop(&mut self) {
        if !self.closed {
            let _ = DirectoryOperations::close(self.file_system, &mut self.context);
        }
    }
}

pub(super) struct ResolvedTarget<'a> {
    pub path: ResolvedPath<'a>,
    pub mount_index: usize,
    pub current_directory: Option<OpenDirectoryHandle>,
    pub parent_directory: Option<OpenDirectoryHandle>,
}

impl<'a> ResolvedTarget<'a> {
    pub fn relative_path<'b>(&'b self, file_systems: &[InternalFileSystem]) -> Result<&'b Path>
    where
        'a: 'b,
    {
        self.path
            .strip_prefix_absolute(&file_systems[self.mount_index].mount_point)
            .ok_or(IoErrorKind::InvalidPath)
    }

    pub fn entry_name<'b>(&'b self, file_systems: &[InternalFileSystem]) -> Result<&'b str>
    where
        'a: 'b,
    {
        self.relative_path(file_systems)?
            .get_file_name()
            .ok_or(IoErrorKind::InvalidPath)
    }
}

#[derive(Clone, Copy)]
struct ResolutionOptions<'a> {
    follow_final: bool,
    allow_missing_final: bool,
    user: UserIdentifier,
    task: TaskIdentifier,
    root: Option<&'a Path>,
    pinned_mount: Option<usize>,
}

pub(super) struct ConfinedRoot {
    path: PathOwned,
    mount_index: usize,
    directory: Option<OpenDirectoryHandle>,
}

struct ResolutionState {
    resolved: String,
    candidate: String,
    pending: String,
    links_followed: usize,
    target_buffer: Vec<u8>,
    mount_index: usize,
    current_directory: Option<OpenDirectoryHandle>,
    parent_directory: Option<OpenDirectoryHandle>,
    root_mount_index: usize,
    root_directory: Option<OpenDirectoryHandle>,
}

impl core::ops::Deref for ResolvedPath<'_> {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Borrowed(path) => path,
            Self::Owned(path) => path,
        }
    }
}

impl AsRef<Path> for ResolvedPath<'_> {
    fn as_ref(&self) -> &Path {
        self
    }
}

impl VirtualFileSystem {
    /// Read persisted link payloads through ordinary file operations, falling
    /// back to a backend adapter only for native host links.
    pub(super) fn read_link_payload(
        file_system: &dyn FileSystemOperations,
        path: &Path,
        buffer: &mut Vec<u8>,
    ) -> Result<usize> {
        let mut attributes = Attributes::new().set_mask(AttributeFlags::Size);
        Self::get_attributes(file_system, path, &mut attributes)?;
        let size = *attributes.get_size().ok_or(IoErrorKind::InputOutput)? as usize;
        if size == 0 || size > MAX_PATH_LENGTH {
            return Err(IoErrorKind::CorruptData);
        }

        buffer.resize(size, 0);
        match file_system.read_symlink(path, buffer.as_mut_slice()) {
            Ok(read) => {
                if read != size {
                    return Err(IoErrorKind::CorruptData);
                }
                return Ok(read);
            }
            Err(IoErrorKind::Unsupported) => {}
            Err(error) => return Err(error),
        }

        let mut context = Context::new_empty();
        file_system.lookup_file(&mut context, path, Flags::from(AccessFlags::Read))?;
        let read = file_system.read_sync(&mut context, buffer.as_mut_slice(), 0);
        let close = BaseOperations::close(file_system, &mut context);
        let read = read?;
        close?;
        if read != size {
            return Err(IoErrorKind::CorruptData);
        }
        Ok(read)
    }

    pub(super) fn read_entry_link_payload(
        file_system: &dyn FileSystemOperations,
        parent: &Context,
        name: &str,
        buffer: &mut Vec<u8>,
    ) -> Result<usize> {
        let mut attributes = Attributes::new().set_mask(AttributeFlags::Size);
        file_system.get_entry_attributes(parent, name, &mut attributes)?;
        let size = *attributes.get_size().ok_or(IoErrorKind::InputOutput)? as usize;
        if size == 0 || size > MAX_PATH_LENGTH {
            return Err(IoErrorKind::CorruptData);
        }

        buffer.resize(size, 0);
        match file_system.read_symlink_entry(parent, name, buffer.as_mut_slice()) {
            Ok(read) => {
                if read != size {
                    return Err(IoErrorKind::CorruptData);
                }
                return Ok(read);
            }
            Err(IoErrorKind::Unsupported) => {}
            Err(error) => return Err(error),
        }

        let mut context = Context::new_empty();
        file_system.lookup_file_entry(
            parent,
            name,
            &mut context,
            Flags::from(AccessFlags::Read),
        )?;
        let read = file_system.read_sync(&mut context, buffer.as_mut_slice(), 0);
        let close = BaseOperations::close(file_system, &mut context);
        let read = read?;
        close?;
        if read != size {
            return Err(IoErrorKind::CorruptData);
        }
        Ok(read)
    }

    /// Resolve symlinks component by component. Canonical paths without links
    /// stay borrowed on path-based backends; scratch storage is created only
    /// after normalization or a link.
    pub(super) async fn resolve_path<'a>(
        &self,
        file_systems: &FileSystemsArray,
        path: &'a Path,
        follow_final: bool,
        allow_missing_final: bool,
        user: UserIdentifier,
        task: TaskIdentifier,
    ) -> Result<ResolvedTarget<'a>> {
        let require_directory = path.as_str().len() > 1 && path.as_str().ends_with('/');
        let mut resolved = if require_directory {
            let normalized = PathOwned::try_from(path.as_str().trim_end_matches('/'))
                .map_err(|_| IoErrorKind::InvalidPath)?;
            let ResolvedTarget {
                path: resolved_path,
                mount_index,
                current_directory,
                parent_directory,
            } = self
                .resolve_path_inner(
                    file_systems,
                    &normalized,
                    ResolutionOptions {
                        follow_final,
                        allow_missing_final,
                        user,
                        task,
                        root: None,
                        pinned_mount: None,
                    },
                    None,
                )
                .await?;
            let owned = PathOwned::try_from(resolved_path.as_str())
                .map_err(|_| IoErrorKind::InvalidPath)?;
            ResolvedTarget {
                path: ResolvedPath::Owned(owned),
                mount_index,
                current_directory,
                parent_directory,
            }
        } else {
            self.resolve_path_inner(
                file_systems,
                path,
                ResolutionOptions {
                    follow_final,
                    allow_missing_final,
                    user,
                    task,
                    root: None,
                    pinned_mount: None,
                },
                None,
            )
            .await?
        };
        if require_directory {
            Self::verify_resolved_directory(file_systems, &mut resolved, allow_missing_final)?;
        }
        Ok(resolved)
    }

    fn verify_resolved_directory(
        file_systems: &FileSystemsArray,
        resolved: &mut ResolvedTarget<'_>,
        allow_missing_final: bool,
    ) -> Result<()> {
        let mut attributes = Attributes::new().set_mask(AttributeFlags::Kind);
        match Self::target_attributes(file_systems, resolved, &mut attributes) {
            Ok(()) => {}
            Err(IoErrorKind::NotFound) if allow_missing_final => {
                return Ok(());
            }
            Err(error) => return Err(error),
        }
        if attributes.get_kind() != Some(&Kind::Directory) {
            return Err(IoErrorKind::NotADirectory);
        }
        Ok(())
    }

    /// Revalidate a pre-resolved confinement root without following any
    /// symbolic links, verifying mount namespace guarantees along the path.
    pub(super) async fn validate_confined_root(
        &self,
        file_systems: &FileSystemsArray,
        root: &Path,
        user: UserIdentifier,
        task: TaskIdentifier,
    ) -> Result<ConfinedRoot> {
        if !root.is_valid() || !root.is_absolute() || root.get_length() > MAX_PATH_LENGTH {
            return Err(IoErrorKind::InvalidPath);
        }

        let (root_file_system, _, mut mount_index) =
            Self::get_file_system_from_path(file_systems, Path::ROOT)?;
        if !root_file_system.file_system.supports_confined_namespace() {
            return Err(IoErrorKind::PathEscapesRoot);
        }
        let mut current_directory = if root_file_system
            .file_system
            .supports_directory_entry_operations()
        {
            Some(OpenDirectoryHandle::open_root(
                root_file_system.file_system,
            )?)
        } else {
            None
        };

        if root.is_root() {
            let mut attributes = Attributes::new().set_mask(AttributeFlags::Kind);
            if let Some(directory) = current_directory.as_mut() {
                AttributeOperations::get_attributes(
                    root_file_system.file_system,
                    directory.context_mutable(),
                    &mut attributes,
                )?;
            } else {
                Self::get_attributes(root_file_system.file_system, Path::ROOT, &mut attributes)?;
            }
            if attributes.get_kind() != Some(&Kind::Directory) {
                return Err(IoErrorKind::NotADirectory);
            }
            return Ok(ConfinedRoot {
                path: PathOwned::root(),
                mount_index,
                directory: current_directory,
            });
        }

        let text = root.as_str();
        let mut parent_end = 1usize;
        let mut component_start = 1usize;
        while component_start < text.len() {
            let component_end = text[component_start..]
                .find('/')
                .map_or(text.len(), |offset| component_start + offset);
            let component = &text[component_start..component_end];
            if component.is_empty() || component == "." || component == ".." {
                return Err(IoErrorKind::InvalidPath);
            }

            let parent_path = Path::from_str(&text[..parent_end]);
            self.check_directory_execute_permission(
                file_systems,
                mount_index,
                parent_path,
                current_directory.as_mut(),
                user,
                task,
            )
            .await?;

            let candidate_path = Path::from_str(&text[..component_end]);
            let (candidate_file_system, candidate_relative, candidate_mount_index) =
                Self::get_file_system_from_path(file_systems, candidate_path)?;
            if !candidate_file_system
                .file_system
                .supports_confined_namespace()
            {
                return Err(IoErrorKind::PathEscapesRoot);
            }

            let mut attributes = Attributes::new().set_mask(AttributeFlags::Kind);
            if candidate_mount_index != mount_index {
                let mut new_directory = if candidate_file_system
                    .file_system
                    .supports_directory_entry_operations()
                {
                    Some(OpenDirectoryHandle::open_root(
                        candidate_file_system.file_system,
                    )?)
                } else {
                    None
                };
                if let Some(directory) = new_directory.as_mut() {
                    AttributeOperations::get_attributes(
                        candidate_file_system.file_system,
                        directory.context_mutable(),
                        &mut attributes,
                    )?;
                } else {
                    Self::get_attributes(
                        candidate_file_system.file_system,
                        Path::ROOT,
                        &mut attributes,
                    )?;
                }
                let kind = *attributes.get_kind().ok_or(IoErrorKind::InputOutput)?;
                if kind == Kind::SymbolicLink {
                    return Err(IoErrorKind::PathEscapesRoot);
                }
                if kind != Kind::Directory {
                    return Err(IoErrorKind::NotADirectory);
                }
                current_directory = new_directory;
                mount_index = candidate_mount_index;
            } else {
                if let Some(directory) = current_directory.as_ref() {
                    candidate_file_system.file_system.get_entry_attributes(
                        directory.context(),
                        component,
                        &mut attributes,
                    )?;
                } else {
                    Self::get_attributes(
                        candidate_file_system.file_system,
                        candidate_relative,
                        &mut attributes,
                    )?;
                }
                let kind = *attributes.get_kind().ok_or(IoErrorKind::InputOutput)?;
                if kind == Kind::SymbolicLink {
                    return Err(IoErrorKind::PathEscapesRoot);
                }
                if kind != Kind::Directory {
                    return Err(IoErrorKind::NotADirectory);
                }
                if let Some(directory) = current_directory.as_ref() {
                    current_directory = Some(directory.open_child(component)?);
                }
            }

            parent_end = component_end;
            component_start = component_end + 1;
        }

        Ok(ConfinedRoot {
            path: root.to_owned(),
            mount_index,
            directory: current_directory,
        })
    }

    /// Resolve a relative path beneath a trusted directory while the caller holds
    /// the mount table guard. Absolute link targets never acquire ambient authority,
    /// and revalidating `root` never follows symbolic links.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn resolve_path_with_root<'a>(
        &self,
        file_systems: &FileSystemsArray,
        path: &'a Path,
        follow_final: bool,
        allow_missing_final: bool,
        user: UserIdentifier,
        task: TaskIdentifier,
        root: Option<LookupRoot<'_>>,
    ) -> Result<ResolvedTarget<'a>> {
        let Some(root) = root else {
            return self
                .resolve_path(
                    file_systems,
                    path,
                    follow_final,
                    allow_missing_final,
                    user,
                    task,
                )
                .await;
        };
        if path.is_absolute() {
            return Err(IoErrorKind::PathEscapesRoot);
        }
        let mut confined_root = match root {
            LookupRoot::Path(path) => {
                self.validate_confined_root(file_systems, path, user, task)
                    .await?
            }
            LookupRoot::Pinned(directory) => {
                if !core::ptr::eq(self, directory.virtual_file_system) {
                    return Err(IoErrorKind::InvalidArgument);
                }
                let (mount, _, mount_index) =
                    Self::get_file_system_from_path(file_systems, &directory.path)?;
                let handle = &directory.directory;
                if !core::ptr::eq(mount.file_system, handle.directory) {
                    return Err(IoErrorKind::StaleHandle);
                }
                let current = if mount.file_system.supports_directory_entry_operations() {
                    Some(OpenDirectoryHandle {
                        file_system: mount.file_system,
                        context: mount.file_system.clone_context(&handle.context)?,
                        closed: false,
                    })
                } else {
                    None
                };
                ConfinedRoot {
                    path: directory.path.as_ref().clone(),
                    mount_index,
                    directory: current,
                }
            }
        };
        if matches!(root, LookupRoot::Pinned(_)) {
            self.check_directory_execute_permission(
                file_systems,
                confined_root.mount_index,
                &confined_root.path,
                confined_root.directory.as_mut(),
                user,
                task,
            )
            .await?;
        }

        let require_directory = path.as_str().ends_with('/');
        let trimmed_path = path.as_str().trim_end_matches('/');
        let mut joined =
            String::with_capacity(confined_root.path.get_length() + 1 + trimmed_path.len());
        joined.push_str(confined_root.path.as_str().trim_end_matches('/'));
        if !trimmed_path.is_empty() {
            joined.push('/');
            joined.push_str(trimmed_path);
        } else if joined.is_empty() {
            joined.push('/');
        }
        let joined = PathOwned::try_from(joined).map_err(|_| IoErrorKind::InvalidPath)?;
        let root_path = confined_root.path.clone();
        let pinned_mount = root
            .is_handle_relative()
            .then_some(confined_root.mount_index);
        let target = self
            .resolve_path_inner(
                file_systems,
                &joined,
                ResolutionOptions {
                    follow_final,
                    allow_missing_final,
                    user,
                    task,
                    root: Some(&root_path),
                    pinned_mount,
                },
                Some(confined_root),
            )
            .await?;
        let ResolvedTarget {
            path: target_path,
            mount_index,
            current_directory,
            parent_directory,
        } = target;
        let owned = match target_path {
            ResolvedPath::Borrowed(borrowed) => {
                PathOwned::try_from(borrowed.as_str()).map_err(|_| IoErrorKind::InvalidPath)?
            }
            ResolvedPath::Owned(path) => path,
        };
        let mut owned_target = ResolvedTarget {
            path: ResolvedPath::Owned(owned),
            mount_index,
            current_directory,
            parent_directory,
        };
        if require_directory {
            Self::verify_resolved_directory(file_systems, &mut owned_target, allow_missing_final)?;
        }
        Ok(owned_target)
    }

    async fn check_directory_execute_permission(
        &self,
        file_systems: &FileSystemsArray,
        mount_index: usize,
        path: &Path,
        directory_handle: Option<&mut OpenDirectoryHandle>,
        user: UserIdentifier,
        task: TaskIdentifier,
    ) -> Result<()> {
        let internal_file_system = &file_systems[mount_index];
        if let Some(directory) = directory_handle {
            let mut attributes = Attributes::default().set_mask(
                AttributeFlags::User | AttributeFlags::Group | AttributeFlags::Permissions,
            );
            AttributeOperations::get_attributes(
                internal_file_system.file_system,
                directory.context_mutable(),
                &mut attributes,
            )?;
            return self
                .check_attributes_permissions(&attributes, Permission::Execute, user)
                .await;
        }
        let relative = path
            .strip_prefix_absolute(&internal_file_system.mount_point)
            .ok_or(IoErrorKind::InvalidPath)?;
        self.check_permissions(
            internal_file_system.file_system,
            relative,
            Permission::Execute,
            user,
            task,
        )
        .await
    }

    fn get_lookup_mount<'mount, 'path>(
        file_systems: &'mount FileSystemsArray,
        path: &'path Path,
        pinned_mount: Option<usize>,
    ) -> Result<(&'mount InternalFileSystem, &'path Path, usize)> {
        let selected = Self::get_file_system_from_path(file_systems, path)?;
        // The mount table identifies entries by path. An open directory can move,
        // so the table cannot establish that a mount lies under its handle.
        if pinned_mount.is_some_and(|pinned| pinned != selected.2) {
            return Err(IoErrorKind::PathEscapesRoot);
        }
        Ok(selected)
    }

    async fn resolve_path_inner<'a>(
        &self,
        file_systems: &FileSystemsArray,
        path: &'a Path,
        options: ResolutionOptions<'_>,
        confined_root: Option<ConfinedRoot>,
    ) -> Result<ResolvedTarget<'a>> {
        if !path.is_valid() || !path.is_absolute() || path.get_length() > MAX_PATH_LENGTH {
            return Err(IoErrorKind::InvalidPath);
        }

        let text = path.as_str();
        let (
            mut parent_end,
            mut mount_index,
            mut current_directory,
            root_mount_index,
            root_directory,
        ) = if let Some(confined) = confined_root {
            let root_length = confined.path.as_str().len().max(1);
            let root_mount_index = confined.mount_index;
            let current_directory = if text.len() > root_length {
                confined
                    .directory
                    .as_ref()
                    .map(OpenDirectoryHandle::duplicate)
                    .transpose()?
            } else {
                None
            };
            let (current_directory, root_directory) = if text.len() > root_length {
                (current_directory, confined.directory)
            } else {
                (confined.directory, None)
            };
            (
                root_length,
                root_mount_index,
                current_directory,
                root_mount_index,
                root_directory,
            )
        } else {
            let (root_file_system, _, root_mount_index) =
                Self::get_file_system_from_path(file_systems, Path::ROOT)?;
            let current_directory = if root_file_system
                .file_system
                .supports_directory_entry_operations()
            {
                Some(OpenDirectoryHandle::open_root(
                    root_file_system.file_system,
                )?)
            } else {
                None
            };
            (
                1usize,
                root_mount_index,
                current_directory,
                root_mount_index,
                None,
            )
        };

        let mut parent_directory: Option<OpenDirectoryHandle> = None;
        let mut component_start = if parent_end > 1 { parent_end + 1 } else { 1 };
        let mut target_buffer = Vec::new();

        // Fast path: walk borrowed canonical prefixes first. This avoids
        // allocating working Strings or a target buffer for ordinary paths.
        while component_start < text.len() {
            let component_end = text[component_start..]
                .find('/')
                .map_or(text.len(), |offset| component_start + offset);
            let component = &text[component_start..component_end];
            if component.is_empty() || component == "." || component == ".." {
                break;
            }
            let has_remaining = component_end < text.len();
            let parent_path = Path::from_str(&text[..parent_end]);
            self.check_directory_execute_permission(
                file_systems,
                mount_index,
                parent_path,
                current_directory.as_mut(),
                options.user,
                options.task,
            )
            .await?;

            let candidate_path = Path::from_str(&text[..component_end]);
            let (candidate_file_system, candidate_relative, candidate_mount_index) =
                Self::get_lookup_mount(file_systems, candidate_path, options.pinned_mount)?;
            if options.root.is_some()
                && !candidate_file_system
                    .file_system
                    .supports_confined_namespace()
            {
                return Err(IoErrorKind::PathEscapesRoot);
            }

            let mut attributes = Attributes::new().set_mask(AttributeFlags::Kind);
            let crossed_mount = candidate_mount_index != mount_index;
            let mut entered_mount_directory = None;
            let found = if crossed_mount {
                if candidate_file_system
                    .file_system
                    .supports_directory_entry_operations()
                {
                    let mut new_directory =
                        OpenDirectoryHandle::open_root(candidate_file_system.file_system)?;
                    let result = AttributeOperations::get_attributes(
                        candidate_file_system.file_system,
                        new_directory.context_mutable(),
                        &mut attributes,
                    );
                    entered_mount_directory = Some(new_directory);
                    result
                } else {
                    Self::get_attributes(
                        candidate_file_system.file_system,
                        Path::ROOT,
                        &mut attributes,
                    )
                }
            } else if let Some(directory) = current_directory.as_ref() {
                candidate_file_system.file_system.get_entry_attributes(
                    directory.context(),
                    component,
                    &mut attributes,
                )
            } else {
                Self::get_attributes(
                    candidate_file_system.file_system,
                    candidate_relative,
                    &mut attributes,
                )
            };

            match found {
                Ok(()) => {}
                Err(IoErrorKind::NotFound) if !has_remaining && options.allow_missing_final => {
                    return Ok(ResolvedTarget {
                        path: ResolvedPath::Borrowed(path),
                        mount_index: if crossed_mount {
                            candidate_mount_index
                        } else {
                            mount_index
                        },
                        current_directory: None,
                        parent_directory: if crossed_mount {
                            None
                        } else {
                            current_directory
                        },
                    });
                }
                Err(error) => return Err(error),
            }

            let kind = *attributes.get_kind().ok_or(IoErrorKind::InputOutput)?;
            if kind == Kind::SymbolicLink && (has_remaining || options.follow_final) {
                let length = if let Some(directory) = current_directory.as_ref()
                    && !crossed_mount
                {
                    Self::read_entry_link_payload(
                        candidate_file_system.file_system,
                        directory.context(),
                        component,
                        &mut target_buffer,
                    )?
                } else {
                    Self::read_link_payload(
                        candidate_file_system.file_system,
                        candidate_relative,
                        &mut target_buffer,
                    )?
                };
                let target = core::str::from_utf8(&target_buffer[..length])
                    .map_err(|_| IoErrorKind::CorruptData)?;
                if !Path::new(target).is_valid() || target.is_empty() {
                    return Err(IoErrorKind::CorruptData);
                }
                if options.root.is_some() && target.starts_with('/') {
                    return Err(IoErrorKind::PathEscapesRoot);
                }

                // A relative target continues from the link's containing directory,
                // so keep its open handle and parent handle: a later permission
                // check must not re-resolve the recorded path by name, which can
                // name a different directory after a rename.
                let (resolved, mount_index, current_directory, parent_directory) =
                    if target.starts_with('/') {
                        let (root_file_system, _, root_mount) =
                            Self::get_file_system_from_path(file_systems, Path::ROOT)?;
                        let root_directory = if root_file_system
                            .file_system
                            .supports_directory_entry_operations()
                        {
                            Some(OpenDirectoryHandle::open_root(
                                root_file_system.file_system,
                            )?)
                        } else {
                            None
                        };
                        (String::from("/"), root_mount, root_directory, None)
                    } else {
                        (
                            String::from(&text[..parent_end]),
                            mount_index,
                            current_directory,
                            parent_directory,
                        )
                    };

                let target_trimmed = target.trim_start_matches('/');
                let suffix = text[component_end..].trim_start_matches('/');
                let needs_separator = !target_trimmed.is_empty() && !suffix.is_empty();
                let pending_length =
                    target_trimmed.len() + usize::from(needs_separator) + suffix.len();
                if resolved.len().saturating_add(pending_length) > MAX_PATH_LENGTH {
                    return Err(IoErrorKind::NameTooLong);
                }
                let mut pending = String::with_capacity(pending_length);
                pending.push_str(target_trimmed);
                if needs_separator {
                    pending.push('/');
                }
                pending.push_str(suffix);

                return self
                    .resolve_pending(
                        file_systems,
                        ResolutionState {
                            resolved,
                            candidate: String::new(),
                            pending,
                            links_followed: 1,
                            target_buffer,
                            mount_index,
                            current_directory,
                            parent_directory,
                            root_mount_index,
                            root_directory,
                        },
                        options,
                    )
                    .await;
            }

            if has_remaining && kind != Kind::Directory {
                return Err(IoErrorKind::NotADirectory);
            }

            if crossed_mount {
                mount_index = candidate_mount_index;
                parent_directory = None;
                current_directory = entered_mount_directory;
            } else if candidate_file_system
                .file_system
                .supports_directory_entry_operations()
            {
                if has_remaining {
                    let next_directory = current_directory
                        .as_ref()
                        .ok_or(IoErrorKind::InvalidState)?
                        .open_child(component)?;
                    parent_directory = current_directory.take();
                    current_directory = Some(next_directory);
                } else {
                    parent_directory = current_directory.take();
                    current_directory = None;
                }
            }

            parent_end = component_end;
            component_start = component_end + 1;
        }

        if component_start >= text.len() {
            return Ok(ResolvedTarget {
                path: ResolvedPath::Borrowed(path),
                mount_index,
                current_directory,
                parent_directory,
            });
        }

        // Dot components and repeated separators need a normalized working path.
        // Resume from the already-validated prefix instead of re-walking it.
        let pending = String::from(text[component_start..].trim_start_matches('/'));
        self.resolve_pending(
            file_systems,
            ResolutionState {
                resolved: String::from(&text[..parent_end]),
                candidate: String::new(),
                pending,
                links_followed: 0,
                target_buffer,
                mount_index,
                current_directory,
                parent_directory,
                root_mount_index,
                root_directory,
            },
            options,
        )
        .await
    }

    fn rewalk_handles_to_resolved(
        file_systems: &FileSystemsArray,
        state: &mut ResolutionState,
        options: ResolutionOptions<'_>,
    ) -> Result<()> {
        let resolved_path = Path::from_str(&state.resolved);
        let (target_file_system, relative_path, mount_index) =
            Self::get_lookup_mount(file_systems, resolved_path, options.pinned_mount)?;
        state.mount_index = mount_index;
        if !target_file_system
            .file_system
            .supports_directory_entry_operations()
        {
            state.parent_directory = None;
            state.current_directory = None;
            return Ok(());
        }

        let (mut current, walk_relative) = if let Some(root_directory) =
            state.root_directory.as_ref()
            && mount_index == state.root_mount_index
            && let Some(root_path) = options.root
        {
            let suffix = resolved_path.strip_prefix(root_path).unwrap_or(Path::EMPTY);
            (root_directory.duplicate()?, suffix)
        } else {
            (
                OpenDirectoryHandle::open_root(target_file_system.file_system)?,
                relative_path,
            )
        };

        let mut parent = None;
        for part in walk_relative
            .as_str()
            .split('/')
            .filter(|part| !part.is_empty())
        {
            let next = current.open_child(part)?;
            parent = Some(current);
            current = next;
        }
        state.parent_directory = parent;
        state.current_directory = Some(current);
        Ok(())
    }

    async fn resolve_pending<'a>(
        &self,
        file_systems: &FileSystemsArray,
        mut state: ResolutionState,
        options: ResolutionOptions<'_>,
    ) -> Result<ResolvedTarget<'a>> {
        while !state.pending.is_empty() {
            let separator = state.pending.find('/').unwrap_or(state.pending.len());
            let mut consumed = separator;
            while consumed < state.pending.len() && state.pending.as_bytes()[consumed] == b'/' {
                consumed += 1;
            }
            let has_remaining = consumed < state.pending.len();
            let component_length = separator;

            if component_length == 0 || (component_length == 1 && &state.pending[..1] == ".") {
                state.pending.drain(..consumed.max(1));
                continue;
            }

            self.check_directory_execute_permission(
                file_systems,
                state.mount_index,
                Path::from_str(&state.resolved),
                state.current_directory.as_mut(),
                options.user,
                options.task,
            )
            .await?;

            if component_length == 2 && &state.pending[..2] == ".." {
                if options
                    .root
                    .is_some_and(|root| state.resolved.as_str() == root.as_str())
                {
                    return Err(IoErrorKind::PathEscapesRoot);
                }
                if let Some(last_separator) = state.resolved.rfind('/') {
                    if last_separator == 0 {
                        state.resolved.truncate(1);
                    } else {
                        state.resolved.truncate(last_separator);
                    }
                }
                state.pending.drain(..consumed.max(1));
                Self::rewalk_handles_to_resolved(file_systems, &mut state, options)?;
                continue;
            }

            let component = &state.pending[..component_length];
            state.candidate.clear();
            state.candidate.push_str(&state.resolved);
            if !state.resolved.ends_with('/') {
                state.candidate.push('/');
            }
            state.candidate.push_str(component);
            if !Path::new(&state.candidate).is_valid() || state.candidate.len() > MAX_PATH_LENGTH {
                return Err(IoErrorKind::InvalidPath);
            }

            let (file_system, relative_path, candidate_mount_index) = Self::get_lookup_mount(
                file_systems,
                Path::from_str(&state.candidate),
                options.pinned_mount,
            )?;
            if options.root.is_some() && !file_system.file_system.supports_confined_namespace() {
                return Err(IoErrorKind::PathEscapesRoot);
            }

            let crossed_mount = candidate_mount_index != state.mount_index;
            let mut entered_mount_directory = None;
            let mut attributes = Attributes::new().set_mask(AttributeFlags::Kind);
            let found = if crossed_mount {
                if file_system
                    .file_system
                    .supports_directory_entry_operations()
                {
                    let mut new_directory =
                        OpenDirectoryHandle::open_root(file_system.file_system)?;
                    let result = AttributeOperations::get_attributes(
                        file_system.file_system,
                        new_directory.context_mutable(),
                        &mut attributes,
                    );
                    entered_mount_directory = Some(new_directory);
                    result
                } else {
                    Self::get_attributes(file_system.file_system, Path::ROOT, &mut attributes)
                }
            } else if let Some(directory) = state.current_directory.as_ref() {
                file_system.file_system.get_entry_attributes(
                    directory.context(),
                    component,
                    &mut attributes,
                )
            } else {
                Self::get_attributes(file_system.file_system, relative_path, &mut attributes)
            };

            match found {
                Ok(()) => {}
                Err(IoErrorKind::NotFound) if !has_remaining && options.allow_missing_final => {
                    let owned = PathOwned::try_from(core::mem::take(&mut state.candidate))
                        .map_err(|_| IoErrorKind::InvalidPath)?;
                    return Ok(ResolvedTarget {
                        path: ResolvedPath::Owned(owned),
                        mount_index: if crossed_mount {
                            candidate_mount_index
                        } else {
                            state.mount_index
                        },
                        current_directory: None,
                        parent_directory: if crossed_mount {
                            None
                        } else {
                            state.current_directory
                        },
                    });
                }
                Err(error) => return Err(error),
            }

            let kind = *attributes.get_kind().ok_or(IoErrorKind::InputOutput)?;
            if kind == Kind::SymbolicLink && (has_remaining || options.follow_final) {
                state.links_followed += 1;
                if state.links_followed > MAX_SYMLINKS {
                    return Err(IoErrorKind::SymlinkLoop);
                }
                let target_length = if let Some(directory) = state.current_directory.as_ref()
                    && !crossed_mount
                {
                    Self::read_entry_link_payload(
                        file_system.file_system,
                        directory.context(),
                        component,
                        &mut state.target_buffer,
                    )?
                } else {
                    Self::read_link_payload(
                        file_system.file_system,
                        relative_path,
                        &mut state.target_buffer,
                    )?
                };
                let target = core::str::from_utf8(&state.target_buffer[..target_length])
                    .map_err(|_| IoErrorKind::CorruptData)?;
                if target.is_empty()
                    || target_length > MAX_PATH_LENGTH
                    || !Path::new(target).is_valid()
                {
                    return Err(IoErrorKind::CorruptData);
                }

                if options.root.is_some() && target.starts_with('/') {
                    return Err(IoErrorKind::PathEscapesRoot);
                }
                if target.starts_with('/') {
                    state.resolved.clear();
                    state.resolved.push('/');
                    state.parent_directory = None;
                    let (root_file_system, _, root_mount) =
                        Self::get_file_system_from_path(file_systems, Path::ROOT)?;
                    state.mount_index = root_mount;
                    state.current_directory = if root_file_system
                        .file_system
                        .supports_directory_entry_operations()
                    {
                        Some(OpenDirectoryHandle::open_root(
                            root_file_system.file_system,
                        )?)
                    } else {
                        None
                    };
                }

                let target_trimmed = target.trim_start_matches('/');
                let remaining_length = state.pending.len() - consumed;
                let needs_separator = !target_trimmed.is_empty() && remaining_length > 0;
                let new_pending_length =
                    target_trimmed.len() + usize::from(needs_separator) + remaining_length;
                if state.resolved.len().saturating_add(new_pending_length) > MAX_PATH_LENGTH {
                    return Err(IoErrorKind::NameTooLong);
                }

                state.pending.drain(..consumed);
                if needs_separator {
                    state.pending.insert(0, '/');
                }
                state.pending.insert_str(0, target_trimmed);
                continue;
            }

            if has_remaining && kind != Kind::Directory {
                return Err(IoErrorKind::NotADirectory);
            }

            if crossed_mount {
                state.mount_index = candidate_mount_index;
                state.parent_directory = None;
                state.current_directory = entered_mount_directory;
            } else if file_system
                .file_system
                .supports_directory_entry_operations()
            {
                if has_remaining {
                    let next_directory = state
                        .current_directory
                        .as_ref()
                        .ok_or(IoErrorKind::InvalidState)?
                        .open_child(component)?;
                    state.parent_directory = state.current_directory.take();
                    state.current_directory = Some(next_directory);
                } else {
                    state.parent_directory = state.current_directory.take();
                    state.current_directory = None;
                }
            }

            core::mem::swap(&mut state.resolved, &mut state.candidate);
            state.pending.drain(..consumed);
        }

        let owned = PathOwned::try_from(core::mem::take(&mut state.resolved))
            .map_err(|_| IoErrorKind::InvalidPath)?;
        Ok(ResolvedTarget {
            path: ResolvedPath::Owned(owned),
            mount_index: state.mount_index,
            current_directory: state.current_directory,
            parent_directory: state.parent_directory,
        })
    }

    pub(super) fn target_attributes(
        file_systems: &FileSystemsArray,
        target: &mut ResolvedTarget<'_>,
        attributes: &mut Attributes,
    ) -> Result<()> {
        if attributes.get_mask().is_empty() {
            return Ok(());
        }
        let file_system = file_systems[target.mount_index].file_system;
        if let Some(directory) = target.current_directory.as_mut() {
            AttributeOperations::get_attributes(
                file_system,
                directory.context_mutable(),
                attributes,
            )
        } else if let Some(parent) = target.parent_directory.as_ref() {
            let entry_name = target.entry_name(file_systems)?;
            file_system.get_entry_attributes(parent.context(), entry_name, attributes)
        } else {
            let relative_path = target.relative_path(file_systems)?;
            Self::get_attributes(file_system, relative_path, attributes)
        }
    }

    pub(super) fn set_target_attributes(
        file_systems: &FileSystemsArray,
        target: &mut ResolvedTarget<'_>,
        attributes: &Attributes,
    ) -> Result<()> {
        let mask = attributes.get_mask();
        if mask.is_empty() {
            return Ok(());
        }
        let mut current = Attributes::new().set_mask(mask);
        Self::target_attributes(file_systems, target, &mut current)?;
        if attributes_equal(&current, attributes) {
            return Ok(());
        }
        let file_system = file_systems[target.mount_index].file_system;
        if let Some(directory) = target.current_directory.as_mut() {
            AttributeOperations::set_attributes(
                file_system,
                directory.context_mutable(),
                attributes,
            )
        } else if let Some(parent) = target.parent_directory.as_ref() {
            let entry_name = target.entry_name(file_systems)?;
            file_system.set_entry_attributes(parent.context(), entry_name, attributes)
        } else {
            let relative_path = target.relative_path(file_systems)?;
            FileSystemOperations::set_attributes(file_system, relative_path, attributes)
        }
    }

    pub(super) fn update_target_read_access_time(
        file_systems: &FileSystemsArray,
        target: &mut ResolvedTarget<'_>,
        time: Time,
    ) -> Result<()> {
        const SECONDS_PER_DAY: u64 = 24 * 60 * 60;
        let internal_file_system = &file_systems[target.mount_index];
        if !internal_file_system
            .file_system
            .writable_attributes()
            .contains(AttributeFlags::AccessTime)
        {
            return Ok(());
        }

        let should_update = match internal_file_system.access_time_policy {
            AccessTimePolicy::Never => false,
            AccessTimePolicy::Always => true,
            AccessTimePolicy::Relative => {
                let mut current = Attributes::new().set_mask(
                    AttributeFlags::AccessTime
                        | AttributeFlags::ModificationTime
                        | AttributeFlags::StatusTime,
                );
                Self::target_attributes(file_systems, target, &mut current)?;
                if let Some(&access) = current.get_access() {
                    let older_than_modification = current
                        .get_modification()
                        .is_some_and(|modification| access < *modification);
                    let older_than_status =
                        current.get_status().is_some_and(|status| access < *status);
                    let older_than_one_day =
                        time.as_u64().saturating_sub(access.as_u64()) >= SECONDS_PER_DAY;
                    older_than_modification || older_than_status || older_than_one_day
                } else {
                    false
                }
            }
        };

        if should_update {
            let attributes = Attributes::new().set_access(time);
            Self::set_target_attributes(file_systems, target, &attributes)?;
        }
        Ok(())
    }

    pub(super) async fn check_attributes_permissions(
        &self,
        attributes: &Attributes,
        asked_permissions: Permission,
        current_user: UserIdentifier,
    ) -> Result<()> {
        if asked_permissions.contains(Permission::Write)
            && self.writes_frozen.load(portable_atomic::Ordering::Acquire)
        {
            return Err(IoErrorKind::ResourceBusy);
        }

        let owner_user = *attributes.get_user().ok_or(IoErrorKind::InputOutput)?;
        let owner_group = *attributes.get_group().ok_or(IoErrorKind::InputOutput)?;
        let owner_permissions = *attributes
            .get_permissions()
            .ok_or(IoErrorKind::InputOutput)?;

        if !Self::has_permissions(
            self.users_manager,
            current_user,
            asked_permissions,
            owner_user,
            owner_group,
            owner_permissions,
        )
        .await
        {
            log::error!(
                "Asked permissions {:?} not granted for user {:?} (owner: {:?}, group: {:?}, permissions: {:?})",
                asked_permissions,
                current_user,
                owner_user,
                owner_group,
                owner_permissions,
            );
            return Err(IoErrorKind::PermissionDenied);
        }

        Ok(())
    }

    pub(super) async fn check_target_parent_permissions(
        &self,
        file_systems: &FileSystemsArray,
        target: &mut ResolvedTarget<'_>,
        asked_permissions: Permission,
        user: UserIdentifier,
        task: TaskIdentifier,
    ) -> Result<()> {
        if let Some(parent) = target.parent_directory.as_mut() {
            let file_system = file_systems[target.mount_index].file_system;
            let mut attributes = Attributes::default().set_mask(
                AttributeFlags::User | AttributeFlags::Group | AttributeFlags::Permissions,
            );
            AttributeOperations::get_attributes(
                file_system,
                parent.context_mutable(),
                &mut attributes,
            )?;
            return self
                .check_attributes_permissions(&attributes, asked_permissions, user)
                .await;
        }
        let parent_path = target.path.go_parent().ok_or(IoErrorKind::InvalidPath)?;
        let (parent_file_system, relative_path, _) =
            Self::get_file_system_from_path(file_systems, parent_path)?;
        if parent_file_system
            .file_system
            .supports_directory_entry_operations()
        {
            let mut directory = OpenDirectoryHandle::open_root(parent_file_system.file_system)?;
            for component in relative_path
                .as_str()
                .split('/')
                .filter(|component| !component.is_empty())
            {
                directory = directory.open_child(component)?;
            }
            let mut attributes = Attributes::default().set_mask(
                AttributeFlags::User | AttributeFlags::Group | AttributeFlags::Permissions,
            );
            AttributeOperations::get_attributes(
                parent_file_system.file_system,
                directory.context_mutable(),
                &mut attributes,
            )?;
            return self
                .check_attributes_permissions(&attributes, asked_permissions, user)
                .await;
        }
        self.check_permissions(
            parent_file_system.file_system,
            relative_path,
            asked_permissions,
            user,
            task,
        )
        .await
    }

    pub(super) async fn check_target_permissions_with_parent(
        &self,
        file_systems: &FileSystemsArray,
        target: &mut ResolvedTarget<'_>,
        current_permission: Permission,
        parent_permission: Permission,
        user: UserIdentifier,
        task: TaskIdentifier,
    ) -> Result<()> {
        if !target.path.is_root() {
            self.check_target_parent_permissions(
                file_systems,
                target,
                parent_permission,
                user,
                task,
            )
            .await?;
        }
        let mut attributes = Attributes::default()
            .set_mask(AttributeFlags::User | AttributeFlags::Group | AttributeFlags::Permissions);
        Self::target_attributes(file_systems, target, &mut attributes)?;
        self.check_attributes_permissions(&attributes, current_permission, user)
            .await
    }

    pub(super) fn check_target_sticky_permissions(
        &self,
        file_systems: &FileSystemsArray,
        target: &mut ResolvedTarget<'_>,
        current_user: UserIdentifier,
    ) -> Result<()> {
        if current_user == UserIdentifier::ROOT {
            return Ok(());
        }
        let file_system = file_systems[target.mount_index].file_system;
        if let Some(parent) = target.parent_directory.as_mut() {
            let mut parent_attributes = Attributes::default().set_mask(
                AttributeFlags::User | AttributeFlags::Group | AttributeFlags::Permissions,
            );
            AttributeOperations::get_attributes(
                file_system,
                parent.context_mutable(),
                &mut parent_attributes,
            )?;
            let parent_owner = *parent_attributes
                .get_user()
                .ok_or(IoErrorKind::InputOutput)?;
            let parent_permissions = *parent_attributes
                .get_permissions()
                .ok_or(IoErrorKind::InputOutput)?;
            if !parent_permissions.get_special().get_sticky() || parent_owner == current_user {
                return Ok(());
            }
            let entry_name = target
                .path
                .strip_prefix_absolute(&file_systems[target.mount_index].mount_point)
                .and_then(|relative| relative.get_file_name())
                .ok_or(IoErrorKind::InvalidPath)?;
            let mut entry_attributes = Attributes::default().set_mask(AttributeFlags::User);
            file_system.get_entry_attributes(
                parent.context(),
                entry_name,
                &mut entry_attributes,
            )?;
            let entry_owner = *entry_attributes
                .get_user()
                .ok_or(IoErrorKind::InputOutput)?;
            if entry_owner != current_user {
                return Err(IoErrorKind::PermissionDenied);
            }
            return Ok(());
        }
        let relative_path = target.relative_path(file_systems)?;
        let parent_path = relative_path.go_parent().ok_or(IoErrorKind::InvalidPath)?;
        self.check_sticky_permissions(file_system, parent_path, relative_path, current_user)
    }

    pub(super) async fn has_permissions(
        users_manager: &users::Manager,
        current_user: UserIdentifier,
        necessary_permissions: Permission,
        owner_user: UserIdentifier,
        owner_group: GroupIdentifier,
        permissions: Permissions,
    ) -> bool {
        if current_user == UserIdentifier::ROOT {
            return true;
        }

        // POSIX selects exactly one class; denied owner/group permissions must
        // never fall through to a more permissive class.
        if current_user == owner_user {
            return permissions.get_user().contains(necessary_permissions);
        }
        if users_manager.is_in_group(current_user, owner_group).await {
            return permissions.get_group().contains(necessary_permissions);
        }
        permissions.get_others().contains(necessary_permissions)
    }

    /// Metadata policy shared by namespace and retained handle contexts.
    /// Ownership changes are root-only; chmod permits the owner or root.
    pub(crate) fn authorize_metadata_change(
        current_user: UserIdentifier,
        owner: UserIdentifier,
        changes: &Attributes,
    ) -> Result<()> {
        if current_user == UserIdentifier::ROOT || changes.get_mask().is_empty() {
            return Ok(());
        }
        if changes
            .get_mask()
            .intersects(AttributeFlags::User | AttributeFlags::Group)
            || current_user != owner
        {
            return Err(IoErrorKind::PermissionDenied);
        }
        Ok(())
    }

    /// Namespace mutations may not invalidate a visible mount or its ancestors.
    /// Call while holding the mount table write guard (before backend mutation).
    pub(super) fn protects_mount(
        file_systems: &FileSystemsArray,
        path: &Path,
        include_self: bool,
    ) -> bool {
        file_systems.iter().any(|fs| {
            fs.is_mounted
                && (include_self || fs.mount_point.as_str() != path.as_str())
                && fs
                    .mount_point
                    .get_components()
                    .strip_prefix(&path.get_components())
                    .is_some()
        })
    }

    pub(super) fn get_file_system_index_from_path(
        file_systems: &FileSystemsArray,
        path: &Path,
    ) -> usize {
        let mut result_score = 0;
        let mut result: usize = 0;

        let path_components = path.get_components();

        for (i, file_system) in file_systems.iter().enumerate() {
            if !file_system.is_mounted {
                continue;
            }
            let mount_point: &Path = file_system.mount_point.as_ref();
            let mount_point_components = mount_point.get_components();

            let striped_components = path_components
                .clone()
                .strip_prefix(&mount_point_components);

            if striped_components.is_none() {
                continue;
            }

            let score = mount_point_components.count();

            // Only consider this file system if all mount point components match
            if result_score < score {
                result_score = score;
                result = i;
            }
        }

        result
    }

    pub(super) fn get_mutable_file_system_from_path<'b, 'c>(
        file_systems: &'b mut FileSystemsArray,
        path: &'c Path,
    ) -> Result<(&'b mut InternalFileSystem, &'c Path, usize)> {
        let i = Self::get_file_system_index_from_path(file_systems, path);

        let internal_file_system = &mut file_systems[i];

        let relative_path = path
            .strip_prefix_absolute(&internal_file_system.mount_point)
            .ok_or_else(|| {
                log::error!(
                    "Error stripping prefix {:?} from path {:?}",
                    internal_file_system.mount_point,
                    path
                );

                IoErrorKind::InvalidPath
            })?;

        Ok((internal_file_system, relative_path, i))
    }

    pub(super) fn get_file_system_from_path<'b, 'c>(
        file_systems: &'b FileSystemsArray,
        path: &'c Path,
    ) -> Result<(&'b InternalFileSystem, &'c Path, usize)> {
        let i = Self::get_file_system_index_from_path(file_systems, path);

        let internal_file_system = &file_systems[i];

        let relative_path = path
            .strip_prefix_absolute(&internal_file_system.mount_point)
            .ok_or(IoErrorKind::InvalidPath)?;

        Ok((internal_file_system, relative_path, i))
    }

    /// UTC representable by the filesystem's unsigned seconds format.
    pub(super) fn get_time(&self) -> Result<Time> {
        Ok(self
            .time_manager
            .realtime_now()
            .map_io_error()?
            .try_into_duration()
            .map_io_error()?
            .into())
    }

    pub(super) async fn get_time_user_group(
        &self,
        task: TaskIdentifier,
    ) -> Result<(Time, UserIdentifier, GroupIdentifier)> {
        let time = self.get_time()?;

        let user = self.task_manager.get_user(task).await.map_io_error()?;

        let group = self
            .users_manager
            .get_user_primary_group(user)
            .await
            .map_io_error()?;

        Ok((time, user, group))
    }

    pub(super) fn get_attributes(
        file_system: &dyn FileSystemOperations,
        path: &Path,
        attributes: &mut Attributes,
    ) -> Result<()> {
        FileSystemOperations::get_attributes(file_system, path, attributes)
    }

    pub(super) async fn check_permissions(
        &self,
        file_system: &dyn FileSystemOperations,
        path: &Path,
        asked_permissions: Permission,
        current_user: UserIdentifier,
        _task: TaskIdentifier,
    ) -> Result<()> {
        if asked_permissions.contains(Permission::Write)
            && self.writes_frozen.load(portable_atomic::Ordering::Acquire)
        {
            return Err(IoErrorKind::ResourceBusy);
        }
        let mut attributes = Attributes::default()
            .set_mask(AttributeFlags::User | AttributeFlags::Group | AttributeFlags::Permissions);

        Self::get_attributes(file_system, path, &mut attributes)?;

        let owner_user = *attributes.get_user().ok_or(IoErrorKind::InputOutput)?;
        let owner_group = *attributes.get_group().ok_or(IoErrorKind::InputOutput)?;
        let owner_permissions = *attributes
            .get_permissions()
            .ok_or(IoErrorKind::InputOutput)?;

        if !Self::has_permissions(
            self.users_manager,
            current_user,
            asked_permissions,
            owner_user,
            owner_group,
            owner_permissions,
        )
        .await
        {
            log::error!(
                "Asked permissions {:?} not granted for user {:?} (owner: {:?}, group: {:?}, permissions: {:?})",
                asked_permissions,
                current_user,
                owner_user,
                owner_group,
                owner_permissions,
            );
            return Err(IoErrorKind::PermissionDenied);
        }

        Ok(())
    }

    pub(super) fn check_sticky_permissions(
        &self,
        file_system: &dyn FileSystemOperations,
        parent_path: &Path,
        entry_path: &Path,
        current_user: UserIdentifier,
    ) -> Result<()> {
        if current_user == UserIdentifier::ROOT {
            return Ok(());
        }

        let mut parent_attributes = Attributes::default()
            .set_mask(AttributeFlags::User | AttributeFlags::Group | AttributeFlags::Permissions);
        Self::get_attributes(file_system, parent_path, &mut parent_attributes)?;

        let parent_owner = *parent_attributes
            .get_user()
            .ok_or(IoErrorKind::InputOutput)?;
        let parent_permissions = *parent_attributes
            .get_permissions()
            .ok_or(IoErrorKind::InputOutput)?;

        if !parent_permissions.get_special().get_sticky() || parent_owner == current_user {
            return Ok(());
        }

        let mut entry_attributes = Attributes::default().set_mask(AttributeFlags::User);
        Self::get_attributes(file_system, entry_path, &mut entry_attributes)?;
        let entry_owner = *entry_attributes
            .get_user()
            .ok_or(IoErrorKind::InputOutput)?;

        if entry_owner != current_user {
            return Err(IoErrorKind::PermissionDenied);
        }

        Ok(())
    }
}

pub(super) fn attributes_equal(current: &Attributes, desired: &Attributes) -> bool {
    let mask = desired.get_mask();
    if !mask.difference(AttributeFlags::all()).is_empty() || current.get_mask() != mask {
        return false;
    }

    macro_rules! check_field {
        ($flag:expr, $getter:ident) => {
            if mask.contains($flag) && current.$getter() != desired.$getter() {
                return false;
            }
        };
    }

    check_field!(AttributeFlags::Inode, get_inode);
    check_field!(AttributeFlags::Kind, get_kind);
    check_field!(AttributeFlags::Size, get_size);
    check_field!(AttributeFlags::Links, get_links);
    check_field!(AttributeFlags::CreationTime, get_creation);
    check_field!(AttributeFlags::ModificationTime, get_modification);
    check_field!(AttributeFlags::AccessTime, get_access);
    check_field!(AttributeFlags::StatusTime, get_status);
    check_field!(AttributeFlags::Permissions, get_permissions);
    check_field!(AttributeFlags::User, get_user);
    check_field!(AttributeFlags::Group, get_group);

    true
}

#[cfg(test)]
mod tests {

    use alloc::{boxed::Box, string::ToString, vec};
    use file_system::{DummyFileSystem, PathOwned};

    use super::*;

    #[test]
    fn inode_map_allocates_the_lowest_free_inode() {
        let mut map = InodeMap::new();
        assert_eq!(map.allocate_inode(), Some(1));
        for inode in [3, 1, 2] {
            map.insert(inode, inode * 10);
        }
        assert_eq!(map.allocate_inode(), Some(4));

        assert_eq!(map.remove(&2), Some(20));
        assert_eq!(map.remove(&2), None);
        assert_eq!(map.allocate_inode(), Some(2));

        // Inode 0 is never allocated and does not hide inode 1.
        map.insert(0, 0);
        assert_eq!(map.remove(&1), Some(10));
        assert_eq!(map.allocate_inode(), Some(1));
    }

    #[test]
    fn inode_map_keeps_one_sorted_entry_for_each_inode() {
        let mut map = InodeMap::new();
        for inode in [5, 2, 9, 2] {
            map.insert(inode, inode);
        }
        map.insert(9, 90);
        assert_eq!(
            map.values_mut().map(|value| *value).collect::<Vec<_>>(),
            [2, 5, 90]
        );

        *map.get_mut(&5).unwrap() = 50;
        assert_eq!(map.get_mut(&5), Some(&mut 50));
        assert_eq!(map.get_mut(&4), None);

        map.clear();
        assert_eq!(map.values_mut().count(), 0);
        assert_eq!(map.allocate_inode(), Some(1));
    }

    #[test]
    fn pinned_lookup_rejects_submounts() {
        let file_systems = ["/", "/host", "/host/grant/submount"]
            .into_iter()
            .map(|path| InternalFileSystem {
                reference_count: 1,
                is_mounted: true,
                mount_point: PathOwned::try_from(path).unwrap(),
                access_time_policy: AccessTimePolicy::default(),
                file_system: Box::leak(Box::new(DummyFileSystem)),
            })
            .collect();
        let path = Path::new("/host/grant/submount");
        assert_eq!(
            VirtualFileSystem::get_lookup_mount(&file_systems, path, None)
                .unwrap()
                .2,
            2
        );
        assert_eq!(
            VirtualFileSystem::get_lookup_mount(&file_systems, path, Some(1)).err(),
            Some(IoErrorKind::PathEscapesRoot)
        );

        let path = Path::new("/host/grant/ordinary");
        assert_eq!(
            VirtualFileSystem::get_lookup_mount(&file_systems, path, Some(1))
                .unwrap()
                .2,
            1
        );
    }

    #[test]
    fn test_get_file_system_from_path() {
        let file_systems: FileSystemsArray = vec![
            InternalFileSystem {
                reference_count: 1,
                is_mounted: true,
                mount_point: PathOwned::new("/".to_string()).unwrap(),
                access_time_policy: AccessTimePolicy::default(),
                file_system: Box::leak(Box::new(DummyFileSystem)),
            },
            InternalFileSystem {
                reference_count: 1,
                is_mounted: true,
                mount_point: PathOwned::new("/Foo".to_string()).unwrap(),
                access_time_policy: AccessTimePolicy::default(),
                file_system: Box::leak(Box::new(DummyFileSystem)),
            },
            InternalFileSystem {
                reference_count: 1,
                is_mounted: true,
                mount_point: PathOwned::new("/Foo/Bar".to_string()).unwrap(),
                access_time_policy: AccessTimePolicy::default(),
                file_system: Box::leak(Box::new(DummyFileSystem)),
            },
        ];

        let (_, relative_path, i) =
            VirtualFileSystem::get_file_system_from_path(&file_systems, "/".as_ref()).unwrap();

        assert_eq!(i, 0);
        assert_eq!(relative_path, Path::ROOT);

        let (_, relative_path, i) =
            VirtualFileSystem::get_file_system_from_path(&file_systems, "/Foo/Bar".as_ref())
                .unwrap();

        assert_eq!(i, 2);
        assert_eq!(relative_path, Path::ROOT);

        let (_, relative_path, i) =
            VirtualFileSystem::get_file_system_from_path(&file_systems, "/Foo/Bar/Baz".as_ref())
                .unwrap();

        assert_eq!(i, 2);
        assert_eq!(relative_path, "/Baz".as_ref());

        let (_, relative_path, i) =
            VirtualFileSystem::get_file_system_from_path(&file_systems, "/Foo".as_ref()).unwrap();

        assert_eq!(i, 1);
        assert_eq!(relative_path, Path::ROOT);
    }

    #[test]
    fn path_resolution_skips_detached_mounts_after_remount() {
        let old = Box::leak(Box::new(DummyFileSystem));
        let replacement = Box::leak(Box::new(DummyFileSystem));
        let file_systems: FileSystemsArray = vec![
            InternalFileSystem {
                reference_count: 1,
                is_mounted: true,
                mount_point: PathOwned::new("/".to_string()).unwrap(),
                access_time_policy: AccessTimePolicy::default(),
                file_system: Box::leak(Box::new(DummyFileSystem)),
            },
            InternalFileSystem {
                reference_count: 2,
                is_mounted: false,
                mount_point: PathOwned::new("/Foo".to_string()).unwrap(),
                access_time_policy: AccessTimePolicy::default(),
                file_system: old,
            },
            InternalFileSystem {
                reference_count: 1,
                is_mounted: true,
                mount_point: PathOwned::new("/Foo".to_string()).unwrap(),
                access_time_policy: AccessTimePolicy::default(),
                file_system: replacement,
            },
        ];

        let (file_system, relative_path, index) =
            VirtualFileSystem::get_file_system_from_path(&file_systems, "/Foo/app".as_ref())
                .unwrap();
        assert_eq!(index, 2);
        assert!(core::ptr::eq(file_system.file_system, replacement));
        assert_eq!(relative_path.as_str(), "/app");
    }
}
