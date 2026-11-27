mod utilities;

use crate::pipe::Pipe;
use crate::{Directory, File, IoErrorKind, ItemStatic, Result, ResultExt, poll};
use alloc::borrow::ToOwned;
use alloc::vec;
use alloc::{boxed::Box, collections::BTreeMap};
use core::cell::RefCell;
use core::ptr;
use exported_file_system::{BlockDevice, CharacterDevice, CreateFlags, Permission, Permissions};
use file_system::{
    AccessFlags, AttributeFlags, AttributeOperations, Attributes, BaseOperations, Context,
    FileSystemOperations, Flags, Inode, Kind, Path, PathOwned, StateFlags, Statistics,
};
use synchronization::{
    blocking_mutex::{Mutex as BlockingMutex, raw::CriticalSectionRawMutex},
    rwlock::RwLock,
};
use task::TaskIdentifier;
use users::{GroupIdentifier, UserIdentifier};
use utilities::*;

/// Maximum bytes stored in a symbolic-link target.
const MAX_SYMLINK_TARGET_LENGTH: usize = 4096;

/// Policy controlling when read operations update a mounted file system's access time.
///
/// The policy applies to read-only opens: reads through a read-write handle
/// do not update the access time, so a read-write open that writes nothing
/// costs no metadata commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AccessTimePolicy {
    /// Never update the access time on read. Writes update the modification
    /// and status times on the first write of more than zero bytes after an
    /// open or a flush.
    Never,
    /// Update the access time on read only when it is older than the modification or status time, or older than 24 hours.
    #[default]
    Relative,
    /// Always update the access time on read, including directory opens.
    Always,
}

/// Mount-time options for a file system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MountOptions {
    pub access_time_policy: AccessTimePolicy,
}

impl MountOptions {
    pub const fn new() -> Self {
        Self {
            access_time_policy: AccessTimePolicy::Relative,
        }
    }

    pub const fn with_access_time_policy(mut self, access_time_policy: AccessTimePolicy) -> Self {
        self.access_time_policy = access_time_policy;
        self
    }
}

pub fn initialize(
    task_manager: &'static task::Manager,
    users_manager: &'static users::Manager,
    time_manager: &'static time::Manager<'static>,
    root_file_system: impl FileSystemOperations + 'static,
) -> Result<VirtualFileSystem> {
    initialize_with_options(
        task_manager,
        users_manager,
        time_manager,
        root_file_system,
        MountOptions::default(),
    )
}

pub fn initialize_with_options(
    task_manager: &'static task::Manager,
    users_manager: &'static users::Manager,
    time_manager: &'static time::Manager<'static>,
    root_file_system: impl FileSystemOperations + 'static,
    options: MountOptions,
) -> Result<VirtualFileSystem> {
    Ok(VirtualFileSystem::new_with_options(
        task_manager,
        users_manager,
        time_manager,
        root_file_system,
        options,
    ))
}

/// The virtual file system.
///
pub struct VirtualFileSystem {
    task_manager: &'static task::Manager,
    users_manager: &'static users::Manager,
    time_manager: &'static time::Manager<'static>,
    /// Mounted file systems.
    file_systems: RwLock<CriticalSectionRawMutex, FileSystemsArray>,
    /// Character devices.
    character_device: RwLock<CriticalSectionRawMutex, CharacterDevicesMap>,
    /// Block devices.
    block_device: RwLock<CriticalSectionRawMutex, BlockDevicesMap>,
    /// Pipes.
    pipes: RwLock<CriticalSectionRawMutex, PipeMap>,
    /// Set by [`Self::shutdown_storage`]; never cleared.
    writes_frozen: portable_atomic::AtomicBool,
    /// References that closes which could not wait for their lock still owe (see
    /// [`Self::close_without_waiting`]). Given back by whoever takes that lock next to
    /// look at the counts. The mutex is never held across a wait.
    owed: BlockingMutex<CriticalSectionRawMutex, RefCell<alloc::vec::Vec<Owed>>>,
}

/// One reference a close gave back without its lock: a file system by address, as
/// `ptr::eq` on the registered object compares it, or a device by the inode of the entry
/// its opening took the reference on (two mounted devices may share a pointer).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Owed {
    FileSystem(usize),
    BlockDevice(Inode),
    CharacterDevice(Inode),
}

impl Owed {
    /// What closing `item` gives back, if anything (a pipe owes nothing). A device needs
    /// the inode its opening took the reference on.
    fn of(item: &ItemStatic, device_inode: Option<Inode>) -> Result<Option<Self>> {
        Ok(match item {
            ItemStatic::File(file_system) | ItemStatic::Directory(file_system) => Some(
                Self::FileSystem(ptr::from_ref(*file_system).cast::<()>().addr()),
            ),
            ItemStatic::BlockDevice(_) => Some(Self::BlockDevice(
                device_inode.ok_or(IoErrorKind::BadDescriptor)?,
            )),
            ItemStatic::CharacterDevice(_) => Some(Self::CharacterDevice(
                device_inode.ok_or(IoErrorKind::BadDescriptor)?,
            )),
            ItemStatic::Pipe(_) => None,
            ItemStatic::FileSystem(_) => return Err(IoErrorKind::BadDescriptor),
        })
    }
}

/// Give back one reference to the file system at `address`. False if it is not mounted.
fn release_file_system(file_systems: &mut FileSystemsArray, address: usize) -> bool {
    file_systems
        .iter_mut()
        .find(|file_system| ptr::from_ref(file_system.file_system).cast::<()>().addr() == address)
        .map(|file_system| {
            file_system.reference_count = file_system.reference_count.saturating_sub(1)
        })
        .is_some()
}

fn release_block_device(devices: &mut BlockDevicesMap, inode: Inode) -> bool {
    devices
        .get_mut(&inode)
        .map(|device| device.reference_count = device.reference_count.saturating_sub(1))
        .is_some()
}

fn release_character_device(devices: &mut CharacterDevicesMap, inode: Inode) -> bool {
    devices
        .get_mut(&inode)
        .map(|device| device.reference_count = device.reference_count.saturating_sub(1))
        .is_some()
}

impl VirtualFileSystem {
    pub fn task_manager(&self) -> &'static task::Manager {
        self.task_manager
    }

    pub fn users_manager(&self) -> &'static users::Manager {
        self.users_manager
    }

    pub fn new(
        task_manager: &'static task::Manager,
        users_manager: &'static users::Manager,
        time_manager: &'static time::Manager<'static>,
        root_file_system: impl FileSystemOperations + 'static,
    ) -> Self {
        Self::new_with_options(
            task_manager,
            users_manager,
            time_manager,
            root_file_system,
            MountOptions::default(),
        )
    }

    pub fn new_with_options(
        task_manager: &'static task::Manager,
        users_manager: &'static users::Manager,
        time_manager: &'static time::Manager<'static>,
        root_file_system: impl FileSystemOperations + 'static,
        options: MountOptions,
    ) -> Self {
        let file_systems = vec![InternalFileSystem {
            reference_count: 1,
            is_mounted: true,
            mount_point: Path::ROOT.to_owned(),
            access_time_policy: options.access_time_policy,
            file_system: Box::leak(Box::new(root_file_system)),
        }];

        Self {
            task_manager,
            users_manager,
            time_manager,
            file_systems: RwLock::new(file_systems),
            character_device: RwLock::new(BTreeMap::new()),
            block_device: RwLock::new(BTreeMap::new()),
            pipes: RwLock::new(BTreeMap::new()),
            writes_frozen: portable_atomic::AtomicBool::new(false),
            owed: BlockingMutex::new(RefCell::new(alloc::vec::Vec::new())),
        }
    }

    fn settle_file_systems(&self, file_systems: &mut FileSystemsArray) {
        self.settle_kind(|entry| match entry {
            Owed::FileSystem(address) => Some(release_file_system(file_systems, address)),
            _ => None,
        });
    }

    fn settle_block_devices(&self, devices: &mut BlockDevicesMap) {
        self.settle_kind(|entry| match entry {
            Owed::BlockDevice(address) => Some(release_block_device(devices, address)),
            _ => None,
        });
    }

    fn settle_character_devices(&self, devices: &mut CharacterDevicesMap) {
        self.settle_kind(|entry| match entry {
            Owed::CharacterDevice(address) => Some(release_character_device(devices, address)),
            _ => None,
        });
    }

    /// Give back the owed references `release` handles (it returns `Some` for those), keep
    /// the others, and free the queue once it is empty.
    fn settle_kind(&self, mut release: impl FnMut(Owed) -> Option<bool>) {
        self.owed.lock(|owed| {
            let mut owed = owed.borrow_mut();
            if owed.is_empty() {
                return;
            }
            // A reference whose object was unmounted meanwhile has nothing left to give.
            owed.retain(|entry| release(*entry).is_none());
            if owed.is_empty() {
                *owed = alloc::vec::Vec::new();
            }
        });
    }

    /// Hold the namespace lock, the one path-based calls take, until `release` completes,
    /// and return its output. Tests use it to suspend a task with the lock held, as an open
    /// does while it waits for the task or user managers.
    #[cfg(feature = "test_harness")]
    pub async fn hold_namespace_lock<F: core::future::Future>(&self, release: F) -> F::Output {
        let _file_systems = self.file_systems.write().await;
        release.await
    }

    /// Hold the character device lock until `release` completes, and return its output.
    /// An open of a character device takes it while it holds the namespace lock, so tests
    /// use it to suspend a real open with the namespace lock held.
    #[cfg(feature = "test_harness")]
    pub async fn hold_character_device_lock<F: core::future::Future>(
        &self,
        release: F,
    ) -> F::Output {
        let _character_devices = self.character_device.write().await;
        release.await
    }

    pub async fn uninitialize(&self) {
        let mut file_systems = self.file_systems.write().await;

        for internal_file_system in file_systems.drain(..) {
            let _: Box<dyn FileSystemOperations> =
                unsafe { Box::from_raw(internal_file_system.file_system as *const _ as *mut _) };
        }
    }

    /// Commit and unmount every mounted filesystem for a system shutdown.
    ///
    /// Requires quiescence: if any filesystem still has an open file or
    /// directory, nothing changes and `ResourceBusy` is returned. Otherwise
    /// new writes are frozen for good, then mounts are unmounted deepest first
    /// with the root last; each filesystem flushes its backing device. The
    /// first failure is returned and the remaining mounts stay mounted (and
    /// readable) for diagnosis. Nothing is deallocated: tasks that still hold
    /// static references keep valid memory.
    pub async fn shutdown_storage(&self) -> Result<()> {
        let mut file_systems = self.file_systems.write().await;
        self.settle_file_systems(&mut file_systems);
        if file_systems
            .iter()
            .any(|file_system| file_system.reference_count > 1)
        {
            return Err(IoErrorKind::ResourceBusy);
        }
        self.writes_frozen
            .store(true, portable_atomic::Ordering::Release);
        // A nested mount point is always longer than the mount containing it.
        let mut order: alloc::vec::Vec<usize> = (0..file_systems.len()).collect();
        // Keep mount order for equal lengths so a failure leaves the same mounts readable.
        order.sort_unstable_by_key(|&index| {
            (
                core::cmp::Reverse(file_systems[index].mount_point.as_str().len()),
                index,
            )
        });
        for index in order {
            let file_system = &mut file_systems[index];
            if file_system.is_mounted {
                file_system.file_system.unmount()?;
                file_system.is_mounted = false;
            }
        }
        Ok(())
    }

    /// Mount a file system at a given mount point.
    pub async fn mount_file_system(
        &'static self,
        file_system: impl FileSystemOperations + 'static,
        path: impl AsRef<Path>,
        task: TaskIdentifier,
    ) -> Result<()> {
        self.mount_file_system_with_options(file_system, path, task, MountOptions::default())
            .await
    }

    /// Mount a file system at a given mount point with explicit mount options.
    pub async fn mount_file_system_with_options(
        &self,
        file_system: impl FileSystemOperations + 'static,
        path: impl AsRef<Path>,
        task: TaskIdentifier,
        options: MountOptions,
    ) -> Result<()> {
        let item = ItemStatic::FileSystem(Box::leak(Box::new(file_system)));
        self.mount_static_with_permissions_and_options(
            task,
            path.as_ref(),
            item,
            Permissions::DEVICE_DEFAULT,
            options,
        )
        .await
    }

    pub async fn unmount(&self, path: impl AsRef<Path>, force: bool) -> Result<()> {
        self.unmount_inner(path.as_ref(), force).await
    }

    async fn unmount_inner(&self, path: &Path, force: bool) -> Result<()> {
        if !path.is_valid() || !path.is_absolute() || path.is_root() {
            return Err(IoErrorKind::InvalidPath);
        }

        let mut file_systems = self.file_systems.write().await;
        self.settle_file_systems(&mut file_systems);

        if Self::protects_mount(&file_systems, path, false) {
            return Err(IoErrorKind::ResourceBusy);
        }

        let parent_path = path.go_parent().ok_or(IoErrorKind::InvalidPath)?;

        let (underlying_file_system, _, _) =
            Self::get_file_system_from_path(&file_systems, parent_path)?; // Get the file system identifier and the relative path
        let relative_path = path
            .strip_prefix_absolute(&underlying_file_system.mount_point)
            .ok_or(IoErrorKind::InvalidPath)?;

        let mut attributes =
            Attributes::new().set_mask(AttributeFlags::Kind | AttributeFlags::Inode);
        Self::get_attributes(
            underlying_file_system.file_system,
            relative_path,
            &mut attributes,
        )
        .await?;

        let kind = attributes.get_kind().ok_or(IoErrorKind::InputOutput)?;
        let inode = *attributes.get_inode().ok_or(IoErrorKind::InputOutput)?;

        match kind {
            Kind::Directory => {
                let mountpoint_index = file_systems
                    .iter()
                    .position(|file_system| {
                        let mount_point: &Path = file_system.mount_point.as_ref();
                        file_system.is_mounted && mount_point == path
                    })
                    .ok_or(IoErrorKind::InvalidPath)?;
                let file_system = &file_systems[mountpoint_index];

                if file_system.reference_count > 1 && !force {
                    return Err(IoErrorKind::ResourceBusy);
                }

                // Unmount only removes the overlay; the original directory
                // (including hidden contents) belongs to the underlying namespace.
                file_system.file_system.unmount()?;
                file_systems[mountpoint_index].is_mounted = false;
            }
            Kind::BlockDevice => {
                let mut block_devices = self.block_device.write().await;
                self.settle_block_devices(&mut block_devices);
                let device = block_devices
                    .get_mut(&inode)
                    .ok_or(IoErrorKind::StaleHandle)?;

                if device.reference_count > 1 && !force {
                    return Err(IoErrorKind::ResourceBusy);
                }

                underlying_file_system.file_system.remove(relative_path)?;
                device.device.unmount()?;
            }
            Kind::CharacterDevice => {
                let mut character_devices = self.character_device.write().await;
                self.settle_character_devices(&mut character_devices);
                let device = character_devices
                    .get_mut(&inode)
                    .ok_or(IoErrorKind::StaleHandle)?;

                if device.reference_count > 1 && !force {
                    return Err(IoErrorKind::ResourceBusy);
                }

                underlying_file_system.file_system.remove(relative_path)?;
                device.device.unmount()?;
            }
            _ => {
                return Err(IoErrorKind::Unsupported);
            }
        }

        Ok(())
    }

    pub async fn unmount_all(&self) -> Result<()> {
        let mut file_systems = self.file_systems.write().await;
        let mut block_devices = self.block_device.write().await;
        let mut character_devices = self.character_device.write().await;

        for file_system in file_systems.drain(..) {
            if file_system.is_mounted {
                file_system.file_system.unmount()?;
            }
            let _: Box<dyn FileSystemOperations> =
                unsafe { Box::from_raw(file_system.file_system as *const _ as *mut _) };
        }

        for block_device in block_devices.values_mut() {
            block_device.device.unmount()?;
            let _: Box<dyn BlockDevice> =
                unsafe { Box::from_raw(block_device.device as *const _ as *mut _) };
        }
        block_devices.clear();

        for character_device in character_devices.values_mut() {
            character_device.device.unmount()?;
            let _: Box<dyn CharacterDevice> =
                unsafe { Box::from_raw(character_device.device as *const _ as *mut _) };
        }
        character_devices.clear();

        Ok(())
    }

    /// Resolve a directory path once, following symbolic links and verifying
    /// that it names an accessible directory on a mount with confined namespace
    /// guarantees.
    ///
    /// Callers can keep the returned canonical path as the `root` argument to
    /// `*_beneath` operations; subsequent revalidation of that `root` does not
    /// follow symbolic links.
    pub async fn resolve_directory_root(
        &self,
        task: TaskIdentifier,
        path: impl AsRef<Path>,
    ) -> Result<PathOwned> {
        let file_systems = self.file_systems.read().await;
        let user = self.task_manager.get_user(task).await.map_io_error()?;
        let mut resolved = self
            .resolve_path(&file_systems, path.as_ref(), true, false, user, task)
            .await?;
        if !file_systems[resolved.mount_index]
            .file_system
            .supports_confined_namespace()
        {
            return Err(IoErrorKind::PathEscapesRoot);
        }
        let mut attributes = Attributes::new().set_mask(
            AttributeFlags::Kind
                | AttributeFlags::User
                | AttributeFlags::Group
                | AttributeFlags::Permissions,
        );
        Self::target_attributes(&file_systems, &mut resolved, &mut attributes).await?;
        if attributes.get_kind() != Some(&Kind::Directory) {
            return Err(IoErrorKind::NotADirectory);
        }
        self.check_attributes_permissions(&attributes, Permission::Execute, user)
            .await?;
        self.validate_confined_root(&file_systems, &resolved.path, user, task)
            .await?;
        Ok(resolved.path.as_ref().to_owned())
    }

    pub async fn open_directory<'a>(
        &'a self,
        task: TaskIdentifier,
        path: &impl AsRef<Path>,
    ) -> Result<Directory<'a>> {
        self.open_directory_with_root(task, path.as_ref(), None, true)
            .await
    }

    /// Open a directory, controlling whether a final symbolic link is followed.
    pub async fn open_directory_with_lookup<'a>(
        &'a self,
        task: TaskIdentifier,
        path: &impl AsRef<Path>,
        follow_final: bool,
    ) -> Result<Directory<'a>> {
        self.open_directory_with_root(task, path.as_ref(), None, follow_final)
            .await
    }

    /// Resolve relative paths beneath the supplied root directory.
    pub async fn open_directory_beneath<'a>(
        &'a self,
        task: TaskIdentifier,
        path: &impl AsRef<Path>,
        root: &Path,
    ) -> Result<Directory<'a>> {
        self.open_directory_with_root(task, path.as_ref(), Some(root), true)
            .await
    }

    /// Resolve relative paths beneath the supplied root directory, controlling
    /// whether a final symbolic link is followed.
    pub async fn open_directory_beneath_with_lookup<'a>(
        &'a self,
        task: TaskIdentifier,
        path: &impl AsRef<Path>,
        root: &Path,
        follow_final: bool,
    ) -> Result<Directory<'a>> {
        self.open_directory_with_root(task, path.as_ref(), Some(root), follow_final)
            .await
    }

    async fn open_directory_with_root<'a>(
        &'a self,
        task: TaskIdentifier,
        path: &Path,
        root: Option<&Path>,
        follow_final: bool,
    ) -> Result<Directory<'a>> {
        let mut file_systems = self.file_systems.write().await;

        let user = self.task_manager.get_user(task).await.map_io_error()?;
        let mut resolved = self
            .resolve_path_with_root(&file_systems, path, follow_final, false, user, task, root)
            .await?;

        let mut attributes = Attributes::new().set_mask(
            AttributeFlags::Kind
                | AttributeFlags::User
                | AttributeFlags::Group
                | AttributeFlags::Permissions,
        );
        Self::target_attributes(&file_systems, &mut resolved, &mut attributes).await?;
        let kind = *attributes.get_kind().ok_or(IoErrorKind::InputOutput)?;

        if kind == Kind::SymbolicLink {
            return Err(IoErrorKind::SymlinkLoop);
        }
        if kind != Kind::Directory {
            return Err(IoErrorKind::NotADirectory);
        }

        if !resolved.path.is_root() {
            self.check_target_parent_permissions(
                &file_systems,
                &mut resolved,
                Permission::Execute,
                user,
                task,
            )
            .await?;
        }
        self.check_attributes_permissions(&attributes, Permission::Read, user)
            .await?;

        if file_systems[resolved.mount_index].access_time_policy == AccessTimePolicy::Always {
            // An unsynchronized or unrepresentable UTC must not prevent reading
            // existing storage. Keep its previous access timestamp in that case.
            if let Ok(time) = self.get_time() {
                let attributes = Attributes::new().set_access(time);
                let mask = attributes.get_mask().intersection(
                    file_systems[resolved.mount_index]
                        .file_system
                        .writable_attributes(),
                );
                Self::set_target_attributes(
                    &file_systems,
                    &mut resolved,
                    &attributes.set_mask(mask),
                )
                .await?;
            }
        }

        let context = if let Some(directory_handle) = resolved.current_directory.take() {
            directory_handle.into_context()
        } else if let Some(parent) = resolved.parent_directory.as_ref() {
            let entry_name = resolved.entry_name(&file_systems)?;
            let mut context = Context::new_empty();
            poll(|| {
                file_systems[resolved.mount_index]
                    .file_system
                    .lookup_directory_entry(parent.context(), entry_name, &mut context)
            })
            .await?;
            context
        } else {
            let mut context = Context::new_empty();
            let relative_path = resolved.relative_path(&file_systems)?;
            poll(|| {
                file_systems[resolved.mount_index]
                    .file_system
                    .lookup_directory(&mut context, relative_path)
            })
            .await?;
            context
        };

        let internal_file_system = &mut file_systems[resolved.mount_index];
        internal_file_system.reference_count += 1;

        Ok(Directory::new(
            self,
            internal_file_system.file_system,
            Flags::new(AccessFlags::Read, None, None),
            context,
        ))
    }

    /// Check the caller's parent-directory authorization for creating a file.
    /// Resolves paths exactly as a nonexclusive create would, without creating
    /// or opening a file. This is a UI preflight, not a reservation: the actual
    /// create must still enforce access when it executes.
    pub async fn check_create_file_access(
        &self,
        task: TaskIdentifier,
        path: &impl AsRef<Path>,
    ) -> Result<()> {
        self.check_create_file_access_inner(task, path.as_ref())
            .await
    }

    async fn check_create_file_access_inner(
        &self,
        task: TaskIdentifier,
        path: &Path,
    ) -> Result<()> {
        let user = self.task_manager.get_user(task).await.map_io_error()?;
        let file_systems = self.file_systems.read().await;
        let mut resolved = self
            .resolve_path(&file_systems, path, true, true, user, task)
            .await?;
        if resolved.path.is_root() {
            return Err(IoErrorKind::InvalidPath);
        }
        self.check_target_parent_permissions(
            &file_systems,
            &mut resolved,
            Permission::Write | Permission::Execute,
            user,
            task,
        )
        .await
    }

    pub async fn open<'a>(
        &'a self,
        path: &impl AsRef<Path>,
        flags: Flags,
        task: TaskIdentifier,
    ) -> Result<File<'a>> {
        self.open_with_root(
            path.as_ref(),
            flags,
            task,
            None,
            true,
            Permissions::FILE_DEFAULT,
        )
        .await
    }

    /// Open a file; a file that the call creates gets `permissions` instead of
    /// `Permissions::FILE_DEFAULT`, from its creation on. An existing file
    /// keeps its permissions.
    pub async fn open_with_permissions<'a>(
        &'a self,
        path: &impl AsRef<Path>,
        flags: Flags,
        task: TaskIdentifier,
        permissions: Permissions,
    ) -> Result<File<'a>> {
        self.open_with_root(path.as_ref(), flags, task, None, true, permissions)
            .await
    }

    /// Open a file, controlling whether a final symbolic link is followed.
    pub async fn open_with_lookup<'a>(
        &'a self,
        path: &impl AsRef<Path>,
        flags: Flags,
        task: TaskIdentifier,
        follow_final: bool,
    ) -> Result<File<'a>> {
        self.open_with_root(
            path.as_ref(),
            flags,
            task,
            None,
            follow_final,
            Permissions::FILE_DEFAULT,
        )
        .await
    }

    /// Resolve relative paths beneath the supplied root directory.
    pub async fn open_beneath<'a>(
        &'a self,
        path: &impl AsRef<Path>,
        flags: Flags,
        task: TaskIdentifier,
        root: &Path,
    ) -> Result<File<'a>> {
        self.open_with_root(
            path.as_ref(),
            flags,
            task,
            Some(root),
            true,
            Permissions::FILE_DEFAULT,
        )
        .await
    }

    /// Resolve relative paths beneath the supplied root directory, atomically
    /// enforcing `follow_final` for the final component under the namespace lock.
    pub async fn open_beneath_with_lookup<'a>(
        &'a self,
        path: &impl AsRef<Path>,
        flags: Flags,
        task: TaskIdentifier,
        root: &Path,
        follow_final: bool,
    ) -> Result<File<'a>> {
        self.open_with_root(
            path.as_ref(),
            flags,
            task,
            Some(root),
            follow_final,
            Permissions::FILE_DEFAULT,
        )
        .await
    }

    async fn open_with_root<'a>(
        &'a self,
        requested_path: &Path,
        flags: Flags,
        task: TaskIdentifier,
        root: Option<&Path>,
        follow_final: bool,
        create_permissions: Permissions,
    ) -> Result<File<'a>> {
        let authority = self.task_manager.authority(task).await.map_io_error()?;

        let mut file_systems = self.file_systems.write().await;

        let user = self.task_manager.get_user(task).await.map_io_error()?;
        let group = self
            .users_manager
            .get_user_primary_group(user)
            .await
            .map_io_error()?;
        let time = self.get_time();
        let (mode, open, _) = flags.split();

        if (open.contains(CreateFlags::Truncate) || flags.get_state().contains(StateFlags::Append))
            && !mode.contains(AccessFlags::Write)
        {
            return Err(IoErrorKind::InvalidFlags);
        }
        let exclusive_create =
            open.contains(CreateFlags::Create) && open.contains(CreateFlags::Exclusive);
        let mut resolved = self
            .resolve_path_with_root(
                &file_systems,
                requested_path,
                follow_final && !exclusive_create,
                open.contains(CreateFlags::Create),
                user,
                task,
                root,
            )
            .await?;

        let mut attributes = Attributes::new().set_mask(AttributeFlags::Kind);
        // An entry left without attributes by a power loss during creation is
        // replaced by a create, after the parent write permission check below,
        // instead of failing the name until a reformat.
        let mut replace_attribute_less = false;
        let mut exists =
            match Self::target_attributes(&file_systems, &mut resolved, &mut attributes).await {
                Ok(()) => true,
                Err(IoErrorKind::NotFound) => false,
                Err(IoErrorKind::MissingAttribute) if open.contains(CreateFlags::Create) => {
                    replace_attribute_less = true;
                    false
                }
                Err(error) => return Err(error),
            };
        if exists && exclusive_create {
            // Distinguish a complete entry from an attribute-less one (whose
            // kind the backend can still report from the directory entry):
            // the former is reported as existing, the latter is replaced.
            let mut probe = Attributes::new().set_mask(AttributeFlags::User);
            match Self::target_attributes(&file_systems, &mut resolved, &mut probe).await {
                Ok(()) => return Err(IoErrorKind::AlreadyExists),
                Err(IoErrorKind::MissingAttribute) => {
                    replace_attribute_less = true;
                    exists = false;
                }
                Err(error) => return Err(error),
            }
        }
        if exists {
            if attributes.get_kind() == Some(&Kind::SymbolicLink) {
                return Err(IoErrorKind::SymlinkLoop);
            }
            self.check_target_permissions_with_parent(
                &file_systems,
                &mut resolved,
                mode.into_permission(),
                Permission::Execute,
                user,
                task,
            )
            .await?;
        } else if open.contains(CreateFlags::Create) {
            if requested_path.as_str().len() > 1 && requested_path.as_str().ends_with('/') {
                return Err(IoErrorKind::NotADirectory);
            }
            // Authorize before mutating the filesystem, including failed redirections.
            self.check_target_parent_permissions(
                &file_systems,
                &mut resolved,
                Permission::Write | Permission::Execute,
                user,
                task,
            )
            .await?;
            if replace_attribute_less {
                // Remove the attribute-less entry left by the interrupted
                // creation; the create below replaces it. The sticky-bit check
                // of a plain remove does not apply here: an attribute-less
                // entry has no readable owner, so on a sticky parent the
                // replace is allowed for anyone with write permission on the
                // parent, exactly like the remove path for root and the
                // parent owner.
                let target_file_system = file_systems[resolved.mount_index].file_system;
                if let Some(parent) = resolved.parent_directory.as_ref() {
                    let entry_name = resolved.entry_name(&file_systems)?;
                    poll(|| target_file_system.remove_entry(parent.context(), entry_name)).await?;
                } else {
                    let relative_path = resolved.relative_path(&file_systems)?;
                    poll(|| target_file_system.remove(relative_path)).await?;
                }
            }
            let time = time?;
            // The backend persists these attributes with the new entry itself
            // (LittleFS commits them atomically with the file), so no separate
            // metadata commit follows the creation.
            let attributes = Attributes::new()
                .set_user(user)
                .set_group(group)
                .set_creation(time)
                .set_modification(time)
                .set_status(time)
                .set_access(time)
                .set_permissions(create_permissions)
                .set_kind(Kind::File);
            let mask = attributes.get_mask().intersection(
                file_systems[resolved.mount_index]
                    .file_system
                    .writable_attributes(),
            );
            let attributes = attributes.set_mask(mask);
            if let Some(parent) = resolved.parent_directory.as_ref() {
                let entry_name = resolved.entry_name(&file_systems)?;
                let target_file_system = file_systems[resolved.mount_index].file_system;
                poll(|| {
                    target_file_system.create_file_entry(parent.context(), entry_name, &attributes)
                })
                .await?;
            } else {
                let target_file_system = file_systems[resolved.mount_index].file_system;
                let relative_path = resolved.relative_path(&file_systems)?;
                poll(|| target_file_system.create_file(relative_path, &attributes)).await?;
            }
        } else {
            return Err(IoErrorKind::NotFound);
        }

        // Preserve previous timestamps while UTC is unavailable; never publish
        // uptime or wrap a signed epoch into unsigned filesystem metadata.
        // Opening for writing changes no time by itself: the modification and
        // status times follow the first write of more than zero bytes and any
        // truncate, and the access time follows reads per the mount policy.
        // Only read-only opens update the access time: reads through a
        // read-write handle that writes nothing keep the previous access time,
        // so an idle read-write open costs no metadata commit.
        if exists
            && !mode.contains(AccessFlags::Write)
            && let Ok(time) = time
        {
            Self::update_target_read_access_time(&file_systems, &mut resolved, time).await?;
        }

        let mut attributes =
            Attributes::new().set_mask(AttributeFlags::Inode | AttributeFlags::Kind);
        Self::target_attributes(&file_systems, &mut resolved, &mut attributes).await?;

        let kind = attributes.get_kind().ok_or(IoErrorKind::InputOutput)?;
        let inode = *attributes.get_inode().ok_or(IoErrorKind::InputOutput)?;

        let mut context = Context::new_empty();

        let file = match kind {
            Kind::CharacterDevice => {
                let mut devices = self.character_device.write().await;

                let device = devices.get_mut(&inode).ok_or(IoErrorKind::StaleHandle)?;

                poll(|| device.device.open(&mut context)).await?;
                device.reference_count += 1;

                File::new(
                    self,
                    ItemStatic::CharacterDevice(device.device),
                    flags,
                    context,
                )
                .with_device_inode(inode)
            }
            Kind::BlockDevice => {
                let mut devices = self.block_device.write().await;

                let device = devices.get_mut(&inode).ok_or(IoErrorKind::StaleHandle)?;

                poll(|| device.device.open(&mut context)).await?;
                device.reference_count += 1;

                File::new(self, ItemStatic::BlockDevice(device.device), flags, context)
                    .with_device_inode(inode)
            }
            Kind::Pipe => {
                let mut pipes = self.pipes.write().await;
                let pipe = pipes.get_mut(&inode).ok_or(IoErrorKind::StaleHandle)?;

                context = pipe.pipe.open_context(flags.get_access())?;
                File::new(self, ItemStatic::Pipe(pipe.pipe.clone()), flags, context)
            }
            Kind::File => {
                // A file just created by this open is already empty: passing
                // Truncate again would only cost an extra commit on close.
                let truncate = flags.get_create().contains(CreateFlags::Truncate) && exists;
                let lookup_flags = Flags::new(
                    flags.get_access(),
                    Some(if truncate {
                        CreateFlags::Truncate
                    } else {
                        CreateFlags::from_bits_truncate(0)
                    }),
                    Some(flags.get_state()),
                );
                if let Some(parent) = resolved.parent_directory.as_ref() {
                    let entry_name = resolved.entry_name(&file_systems)?;
                    let target_file_system = file_systems[resolved.mount_index].file_system;
                    poll(|| {
                        target_file_system.lookup_file_entry(
                            parent.context(),
                            entry_name,
                            &mut context,
                            lookup_flags,
                        )
                    })
                    .await?;
                } else {
                    let target_file_system = file_systems[resolved.mount_index].file_system;
                    let relative_path = resolved.relative_path(&file_systems)?;
                    poll(|| {
                        target_file_system.lookup_file(&mut context, relative_path, lookup_flags)
                    })
                    .await?;
                }

                // O_TRUNC mutates the file: record the modification and status
                // times on the handle. The backend commits them atomically with
                // the truncation on sync or close.
                if truncate && mode.contains(AccessFlags::Write) {
                    let target_file_system = file_systems[resolved.mount_index].file_system;
                    if let Ok(time) = time {
                        Self::set_handle_times(target_file_system, &mut context, time)?;
                    }
                }

                let internal_file_system = &mut file_systems[resolved.mount_index];
                internal_file_system.reference_count += 1;

                File::new(
                    self,
                    ItemStatic::File(internal_file_system.file_system),
                    flags,
                    context,
                )
            }
            Kind::SymbolicLink => return Err(IoErrorKind::SymlinkLoop),
            _ => Err(IoErrorKind::Unsupported)?,
        };

        Ok(file.with_authority(authority))
    }

    /// Drop the device or pipe registration taken for a mount whose backing
    /// entry creation failed. File system mounts register nothing.
    async fn unregister_mounted_item(&self, item: &ItemStatic, inode: Inode) {
        match item {
            ItemStatic::BlockDevice(_) => {
                self.block_device.write().await.remove(&inode);
            }
            ItemStatic::CharacterDevice(_) => {
                self.character_device.write().await.remove(&inode);
            }
            ItemStatic::Pipe(_) => {
                self.pipes.write().await.remove(&inode);
            }
            _ => {}
        }
    }

    pub async fn mount_static(
        &self,
        task: TaskIdentifier,
        path: impl AsRef<Path>,
        item: ItemStatic,
    ) -> Result<()> {
        self.mount_static_with_permissions(task, path, item, Permissions::DEVICE_DEFAULT)
            .await
    }

    /// Publish a mount with its final permissions under the namespace lock.
    /// Sensitive devices must not become briefly accessible with default modes.
    pub async fn mount_static_with_permissions(
        &self,
        task: TaskIdentifier,
        path: impl AsRef<Path>,
        item: ItemStatic,
        permissions: Permissions,
    ) -> Result<()> {
        self.mount_static_with_permissions_and_options(
            task,
            path.as_ref(),
            item,
            permissions,
            MountOptions::default(),
        )
        .await
    }

    async fn mount_static_with_permissions_and_options(
        &self,
        task: TaskIdentifier,
        path: &Path,
        item: ItemStatic,
        permissions: Permissions,
        options: MountOptions,
    ) -> Result<()> {
        if !path.is_valid() || !path.is_absolute() || path.is_root() {
            return Err(IoErrorKind::InvalidPath);
        }

        let mut file_systems = self.file_systems.write().await; // Get the file systems

        if file_systems.iter().any(|file_system| {
            let mount_point: &Path = file_system.mount_point.as_ref();
            file_system.is_mounted && mount_point == path
        }) {
            return Err(IoErrorKind::AlreadyExists);
        }

        let (parent_file_system, relative_path, _) =
            Self::get_mutable_file_system_from_path(&mut file_systems, path)?; // Get the file system identifier and the relative path

        let (time, user, group) = self.get_time_user_group(task).await?;

        self.check_permissions(
            parent_file_system.file_system,
            relative_path.go_parent().ok_or(IoErrorKind::InvalidPath)?,
            Permission::Write,
            user,
            task,
        )
        .await?;

        // The inode and kind are known before the entry is created, so the
        // backend can persist the full attribute set with the new entry in
        // one commit instead of rewriting it afterwards.
        let inode = match item {
            ItemStatic::BlockDevice(device) => {
                let mut block_devices = self.block_device.write().await;
                let inode = Self::get_new_inode(&*block_devices).ok_or(IoErrorKind::NoSpace)?;
                block_devices.insert(
                    inode,
                    InternalBlockDevice {
                        reference_count: 1,
                        device,
                    },
                );
                inode
            }
            ItemStatic::CharacterDevice(device) => {
                let mut character_devices = self.character_device.write().await;
                let inode = Self::get_new_inode(&*character_devices).ok_or(IoErrorKind::NoSpace)?;
                character_devices.insert(
                    inode,
                    InternalCharacterDevice {
                        reference_count: 1,
                        device,
                    },
                );
                inode
            }
            ItemStatic::FileSystem(_) => 0,
            ItemStatic::Pipe(ref pipe) => {
                let mut pipes = self.pipes.write().await;
                let inode = Self::get_new_inode(&*pipes).ok_or(IoErrorKind::NoSpace)?;
                pipes.insert(inode, InternalPipe { pipe: pipe.clone() });
                inode
            }
            _ => {
                return Err(IoErrorKind::BadDescriptor);
            }
        };

        let underlying_kind = match &item {
            ItemStatic::FileSystem(_) => Kind::Directory,
            ItemStatic::BlockDevice(_) => Kind::BlockDevice,
            ItemStatic::CharacterDevice(_) => Kind::CharacterDevice,
            ItemStatic::Pipe(_) => Kind::Pipe,
            _ => {
                return Err(IoErrorKind::BadDescriptor);
            }
        };

        let attributes = Attributes::new()
            .set_user(user)
            .set_group(group)
            .set_creation(time)
            .set_modification(time)
            .set_access(time)
            .set_status(time)
            .set_kind(underlying_kind)
            .set_permissions(permissions)
            .set_inode(inode);
        let mask = attributes
            .get_mask()
            .intersection(parent_file_system.file_system.writable_attributes());
        let attributes = attributes.set_mask(mask);

        // An existing mount point directory keeps its attributes, as on Linux:
        // rewriting them at every boot would wear flash for nothing. New
        // entries are created with their final attributes in one commit.
        if let ItemStatic::FileSystem(_) = item {
            match parent_file_system
                .file_system
                .create_directory(relative_path, &attributes)
            {
                Ok(()) => {}
                Err(IoErrorKind::AlreadyExists) => {
                    let mut kind_attributes = Attributes::new().set_mask(AttributeFlags::Kind);
                    Self::get_attributes(
                        parent_file_system.file_system,
                        relative_path,
                        &mut kind_attributes,
                    )
                    .await?;
                    if kind_attributes.get_kind() != Some(&Kind::Directory) {
                        return Err(IoErrorKind::AlreadyExists);
                    }
                }
                Err(error) => {
                    self.unregister_mounted_item(&item, inode).await;
                    return Err(error);
                }
            }
        } else {
            if let Err(error) = parent_file_system
                .file_system
                .create_file(relative_path, &attributes)
            {
                self.unregister_mounted_item(&item, inode).await;
                return Err(error);
            }
        }

        poll(|| {
            item.as_mount_operations()
                .ok_or(IoErrorKind::Unsupported)?
                .mount()
        })
        .await?;

        if let ItemStatic::FileSystem(file_system) = item {
            file_systems.push(InternalFileSystem {
                reference_count: 1,
                is_mounted: true,
                mount_point: path.to_owned(),
                access_time_policy: options.access_time_policy,
                file_system,
            });
        }

        Ok(())
    }

    pub async fn mount_block_device(
        &self,
        task: TaskIdentifier,
        path: impl AsRef<Path>,
        device: impl BlockDevice + 'static,
    ) -> Result<()> {
        self.mount_static(
            task,
            path,
            ItemStatic::BlockDevice(Box::leak(Box::new(device))),
        )
        .await
    }

    pub async fn mount_character_device(
        &self,
        task: TaskIdentifier,
        path: impl AsRef<Path>,
        device: impl CharacterDevice + 'static,
    ) -> Result<()> {
        self.mount_static(
            task,
            path,
            ItemStatic::CharacterDevice(Box::leak(Box::new(device))),
        )
        .await
    }

    pub async fn create_named_pipe(
        &self,
        path: &impl AsRef<Path>,
        size: usize,
        task: TaskIdentifier,
    ) -> Result<()> {
        self.create_named_pipe_inner(path.as_ref(), size, task)
            .await
    }

    async fn create_named_pipe_inner(
        &self,
        path: &Path,
        size: usize,
        task: TaskIdentifier,
    ) -> Result<()> {
        if size == 0 {
            return Err(IoErrorKind::InvalidArgument);
        }
        self.mount_static(
            task,
            path,
            ItemStatic::Pipe(synchronization::Arc::new(Pipe::new(size))),
        )
        .await
    }

    pub async fn create_unnamed_pipe<'a>(
        &'a self,
        size: usize,
        status: StateFlags,
    ) -> Result<(File<'a>, File<'a>)> {
        if size == 0 {
            return Err(IoErrorKind::InvalidArgument);
        }
        let pipe = synchronization::Arc::new(Pipe::new(size));

        let writer = File::new(
            self,
            ItemStatic::Pipe(pipe.clone()),
            Flags::new(AccessFlags::Write, None, Some(status)),
            pipe.open_context(AccessFlags::Write)?,
        );

        let reader = File::new(
            self,
            ItemStatic::Pipe(pipe.clone()),
            Flags::new(AccessFlags::Read, None, Some(status)),
            pipe.open_context(AccessFlags::Read)?,
        );

        Ok((reader, writer))
    }

    pub async fn remove(&self, task: TaskIdentifier, path: impl AsRef<Path>) -> Result<()> {
        self.remove_with_root(task, path.as_ref(), None).await
    }

    /// Resolve relative paths beneath the supplied root directory.
    pub async fn remove_beneath(
        &self,
        task: TaskIdentifier,
        path: impl AsRef<Path>,
        root: &Path,
    ) -> Result<()> {
        self.remove_with_root(task, path.as_ref(), Some(root)).await
    }

    async fn remove_with_root(
        &self,
        task: TaskIdentifier,
        path: &Path,
        root: Option<&Path>,
    ) -> Result<()> {
        let file_systems = self.file_systems.write().await; // Serialize path resolution with removal.
        let user = self.task_manager.get_user(task).await.map_io_error()?;
        let mut resolved = self
            .resolve_path_with_root(&file_systems, path, false, false, user, task, root)
            .await?;
        if root.is_some_and(|root| resolved.path.as_str() == root.as_str())
            || Self::protects_mount(&file_systems, &resolved.path, true)
        {
            return Err(IoErrorKind::ResourceBusy);
        }

        self.check_target_parent_permissions(
            &file_systems,
            &mut resolved,
            Permission::Write | Permission::Execute,
            user,
            task,
        )
        .await?;

        self.check_target_sticky_permissions(&file_systems, &mut resolved, user)
            .await?;

        let mut attributes =
            Attributes::new().set_mask(AttributeFlags::Kind | AttributeFlags::Inode);
        // An entry left without attributes by a power loss during creation is
        // not a pipe (those are VFS-created and always carry attributes), so
        // removal proceeds with a regular-file kind and inode 0. Backends that
        // report the kind from the directory entry itself (LittleFS) answer
        // this request directly.
        let (inode, kind) =
            match Self::target_attributes(&file_systems, &mut resolved, &mut attributes).await {
                Ok(()) => (
                    *attributes.get_inode().ok_or(IoErrorKind::InputOutput)?,
                    *attributes.get_kind().ok_or(IoErrorKind::InputOutput)?,
                ),
                Err(IoErrorKind::MissingAttribute) => (0, Kind::File),
                Err(error) => return Err(error),
            };

        if kind == Kind::Pipe {
            let mut named_pipes = self.pipes.write().await;

            named_pipes.remove(&inode);
        }

        let target_file_system = file_systems[resolved.mount_index].file_system;
        if let Some(parent) = resolved.parent_directory.as_ref() {
            let entry_name = resolved.entry_name(&file_systems)?;
            poll(|| target_file_system.remove_entry(parent.context(), entry_name)).await?;
        } else {
            let relative_path = resolved.relative_path(&file_systems)?;
            poll(|| target_file_system.remove(relative_path)).await?;
        }

        Ok(())
    }

    pub async fn create_directory(
        &self,
        task: TaskIdentifier,
        path: &impl AsRef<Path>,
    ) -> Result<()> {
        self.create_directory_with_permissions(task, path, Permissions::DIRECTORY_DEFAULT)
            .await
    }

    /// Create a directory with the specified permissions from the start.
    pub async fn create_directory_with_permissions(
        &self,
        task: TaskIdentifier,
        path: &impl AsRef<Path>,
        permissions: Permissions,
    ) -> Result<()> {
        self.create_directory_with_root(task, path.as_ref(), None, permissions)
            .await
    }

    /// Resolve relative paths beneath the supplied root directory.
    pub async fn create_directory_beneath(
        &self,
        task: TaskIdentifier,
        path: &impl AsRef<Path>,
        root: &Path,
    ) -> Result<()> {
        self.create_directory_with_root(
            task,
            path.as_ref(),
            Some(root),
            Permissions::DIRECTORY_DEFAULT,
        )
        .await
    }

    async fn create_directory_with_root(
        &self,
        task: TaskIdentifier,
        path: &Path,
        root: Option<&Path>,
        permissions: Permissions,
    ) -> Result<()> {
        let file_systems = self.file_systems.write().await; // Serialize path resolution with creation.
        let (time, user, group) = self.get_time_user_group(task).await?;
        let mut resolved = self
            .resolve_path_with_root(&file_systems, path, false, true, user, task, root)
            .await?;

        // A mount point or confinement root always exists.
        if resolved.path.is_root()
            || root.is_some_and(|root| resolved.path.as_str() == root.as_str())
            || resolved.relative_path(&file_systems)?.is_root()
        {
            return Err(IoErrorKind::AlreadyExists);
        }

        // Like POSIX mkdir, an existing entry is reported before the parent
        // write check: resolution already required search permission on the
        // parent, so this reveals nothing a lookup would not. An entry left
        // without attributes by a power loss during creation is replaced
        // after the parent write check instead of failing the name.
        let mut attributes = Attributes::new().set_mask(AttributeFlags::Kind);
        let mut replace_attribute_less = false;
        match Self::target_attributes(&file_systems, &mut resolved, &mut attributes).await {
            Ok(()) => {
                // Distinguish a complete entry from an attribute-less one:
                // the former is reported as existing, the latter is replaced.
                let mut probe = Attributes::new().set_mask(AttributeFlags::User);
                match Self::target_attributes(&file_systems, &mut resolved, &mut probe).await {
                    Ok(()) => return Err(IoErrorKind::AlreadyExists),
                    Err(IoErrorKind::MissingAttribute) => replace_attribute_less = true,
                    Err(error) => return Err(error),
                }
            }
            Err(IoErrorKind::NotFound) => {}
            Err(IoErrorKind::MissingAttribute) => replace_attribute_less = true,
            Err(error) => return Err(error),
        }

        self.check_target_parent_permissions(
            &file_systems,
            &mut resolved,
            Permission::Write | Permission::Execute,
            user,
            task,
        )
        .await?;

        if replace_attribute_less {
            // Remove the attribute-less entry left by the interrupted
            // creation; the create below replaces it. The sticky-bit check of
            // a plain remove does not apply here: an attribute-less entry has
            // no readable owner, so on a sticky parent the replace is allowed
            // for anyone with write permission on the parent, exactly like
            // the remove path for root and the parent owner.
            let target_file_system = file_systems[resolved.mount_index].file_system;
            if let Some(parent) = resolved.parent_directory.as_ref() {
                let entry_name = resolved.entry_name(&file_systems)?;
                poll(|| target_file_system.remove_entry(parent.context(), entry_name)).await?;
            } else {
                let relative_path = resolved.relative_path(&file_systems)?;
                poll(|| target_file_system.remove(relative_path)).await?;
            }
        }

        let target_file_system = file_systems[resolved.mount_index].file_system;
        // The backend persists these attributes with the new directory entry
        // itself, so no separate metadata commit follows the creation.
        let attributes = Attributes::new()
            .set_inode(0)
            .set_links(1)
            .set_user(user)
            .set_group(group)
            .set_creation(time)
            .set_modification(time)
            .set_access(time)
            .set_status(time)
            .set_kind(Kind::Directory)
            .set_permissions(permissions);
        let mask = attributes
            .get_mask()
            .intersection(target_file_system.writable_attributes());
        let attributes = attributes.set_mask(mask);

        if let Some(parent) = resolved.parent_directory.as_ref() {
            let entry_name = resolved.entry_name(&file_systems)?;
            poll(|| {
                target_file_system.create_directory_entry(parent.context(), entry_name, &attributes)
            })
            .await?;
        } else {
            let relative_path = resolved.relative_path(&file_systems)?;
            poll(|| target_file_system.create_directory(relative_path, &attributes)).await?;
        }

        Ok(())
    }

    /// Create a symbolic link at `link` whose target is interpreted in the
    /// caller's virtual namespace. The target may be dangling or cross mounts.
    ///
    /// Invalid caller input is reported as `InvalidPath` (unusable target or
    /// link path), `NameTooLong` (target over the link limit) and `NotFound`
    /// for an empty target, which is POSIX `symlink("")` failing with ENOENT.
    pub async fn create_symlink(
        &self,
        task: TaskIdentifier,
        target: &Path,
        link: &Path,
    ) -> Result<()> {
        self.create_symlink_with_root(task, target, link, None)
            .await
    }

    /// Resolve relative paths beneath the supplied root directory.
    pub async fn create_symlink_beneath(
        &self,
        task: TaskIdentifier,
        target: &Path,
        link: &Path,
        root: &Path,
    ) -> Result<()> {
        self.create_symlink_with_root(task, target, link, Some(root))
            .await
    }

    async fn create_symlink_with_root(
        &self,
        task: TaskIdentifier,
        target: &Path,
        link: &Path,
        root: Option<&Path>,
    ) -> Result<()> {
        // Caller input is validated first and reported with the kind that
        // describes the argument; `CorruptData` is reserved for stored link
        // targets that cannot be used.
        if target.is_empty() {
            // POSIX `symlink("", link)` fails with ENOENT.
            return Err(IoErrorKind::NotFound);
        }
        if target.get_length() > MAX_SYMLINK_TARGET_LENGTH {
            return Err(IoErrorKind::NameTooLong);
        }
        if !target.is_valid()
            || !link.is_valid()
            || (root.is_none() && !link.is_absolute())
            || link.is_root()
            || link.as_str().ends_with('/')
        {
            return Err(IoErrorKind::InvalidPath);
        }

        let user = self.task_manager.get_user(task).await.map_io_error()?;
        // Serialize staging and publication against other VFS namespace
        // mutations, so temporary entries are never opened through the VFS.
        let file_systems = self.file_systems.write().await;
        let mut resolved_link = self
            .resolve_path_with_root(&file_systems, link, false, true, user, task, root)
            .await?;
        if resolved_link.path.is_root() || resolved_link.relative_path(&file_systems)?.is_root() {
            return Err(IoErrorKind::InvalidPath);
        }
        if root.is_some_and(|root| resolved_link.path.as_str() == root.as_str()) {
            return Err(IoErrorKind::AlreadyExists);
        }
        self.check_target_parent_permissions(
            &file_systems,
            &mut resolved_link,
            Permission::Write | Permission::Execute,
            user,
            task,
        )
        .await?;

        let target_file_system = file_systems[resolved_link.mount_index].file_system;
        if target_file_system
            .persistent_attributes()
            .contains(AttributeFlags::Kind)
        {
            let mut existing = Attributes::new().set_mask(AttributeFlags::Kind);
            match Self::target_attributes(&file_systems, &mut resolved_link, &mut existing).await {
                Ok(()) => return Err(IoErrorKind::AlreadyExists),
                Err(IoErrorKind::NotFound) => {}
                Err(error) => return Err(error),
            }

            let relative_link = resolved_link.relative_path(&file_systems)?;
            let stage_parent = relative_link.go_parent().ok_or(IoErrorKind::InvalidPath)?;
            let (time, _, _) = self.get_time_user_group(task).await?;
            let attributes = Attributes::new()
                .set_kind(Kind::SymbolicLink)
                .set_user(user)
                .set_group(self.task_manager.get_group(task).await.map_io_error()?)
                .set_permissions(Permissions::ALL_FULL)
                .set_creation(time)
                .set_modification(time)
                .set_access(time)
                .set_status(time);

            let mut published = false;
            let mut last_error = IoErrorKind::AlreadyExists;
            for attempt in 0..64 {
                // The first candidates stay deterministic for simple
                // collision handling. Time-qualified candidates make stale
                // names left by interrupted boots unlikely to exhaust the
                // bounded retry set, without deleting an entry we did not
                // create.
                let stage_name = if attempt < 8 {
                    alloc::format!(".xila-link-stage-{attempt}")
                } else {
                    alloc::format!(".xila-link-stage-{}-{}", time.as_u64(), attempt - 8)
                };
                if let Some(parent) = resolved_link.parent_directory.as_ref() {
                    let entry_name = resolved_link.entry_name(&file_systems)?;
                    let stage_attributes = attributes.clone().set_mask(
                        attributes
                            .get_mask()
                            .intersection(target_file_system.writable_attributes()),
                    );
                    match poll(|| {
                        target_file_system.create_file_entry(
                            parent.context(),
                            &stage_name,
                            &stage_attributes,
                        )
                    })
                    .await
                    {
                        Ok(()) => {}
                        Err(IoErrorKind::AlreadyExists) => continue,
                        Err(error) => return Err(error),
                    }
                    let result = async {
                        let mut context = Context::new_empty();
                        poll(|| {
                            target_file_system.lookup_file_entry(
                                parent.context(),
                                &stage_name,
                                &mut context,
                                Flags::from(AccessFlags::Write),
                            )
                        })
                        .await?;
                        let write = target_file_system.write_sync(
                            &mut context,
                            target.as_str().as_bytes(),
                            0,
                        );
                        let flush = target_file_system.flush_sync(&mut context);
                        let close = BaseOperations::close(target_file_system, &mut context);
                        let written = write?;
                        flush?;
                        close?;
                        if written != target.get_length() {
                            return Err(IoErrorKind::InputOutput);
                        }
                        // The stage entry was created with the final attributes
                        // already persisted, so no second metadata commit here.
                        poll(|| {
                            target_file_system.rename_entry(
                                parent.context(),
                                &stage_name,
                                parent.context(),
                                entry_name,
                            )
                        })
                        .await
                    }
                    .await;
                    match result {
                        Ok(()) => {
                            published = true;
                            break;
                        }
                        Err(error) => {
                            last_error = error;
                            let _ = poll(|| {
                                target_file_system.remove_entry(parent.context(), &stage_name)
                            })
                            .await;
                            break;
                        }
                    }
                } else {
                    let stage_path = stage_parent
                        .to_owned()
                        .join(Path::from_str(&stage_name))
                        .ok_or(IoErrorKind::InvalidPath)?;
                    let stage_attributes = attributes.clone().set_mask(
                        attributes
                            .get_mask()
                            .intersection(target_file_system.writable_attributes()),
                    );
                    match poll(|| target_file_system.create_file(&stage_path, &stage_attributes))
                        .await
                    {
                        Ok(()) => {}
                        Err(IoErrorKind::AlreadyExists) => continue,
                        Err(error) => return Err(error),
                    }

                    let result = async {
                        let mut context = Context::new_empty();
                        poll(|| {
                            target_file_system.lookup_file(
                                &mut context,
                                &stage_path,
                                Flags::from(AccessFlags::Write),
                            )
                        })
                        .await?;
                        let write = target_file_system.write_sync(
                            &mut context,
                            target.as_str().as_bytes(),
                            0,
                        );
                        let flush = target_file_system.flush_sync(&mut context);
                        let close = BaseOperations::close(target_file_system, &mut context);
                        let written = write?;
                        flush?;
                        close?;
                        if written != target.get_length() {
                            return Err(IoErrorKind::InputOutput);
                        }

                        // The stage entry was created with the final attributes
                        // already persisted, so no second metadata commit here.
                        poll(|| target_file_system.rename(&stage_path, relative_link)).await
                    }
                    .await;

                    match result {
                        Ok(()) => {
                            published = true;
                            break;
                        }
                        Err(error) => {
                            last_error = error;
                            let _ = poll(|| target_file_system.remove(&stage_path)).await;
                            break;
                        }
                    }
                }
            }
            if !published {
                return Err(last_error);
            }
        } else {
            // Native symlink support is an adapter detail for filesystems such
            // as HostFS that cannot persist Xila Kind metadata on ordinary files.
            let native_target_owned = if target.is_absolute() {
                let parent = resolved_link
                    .path
                    .go_parent()
                    .ok_or(IoErrorKind::InvalidPath)?;
                let parent_depth = parent
                    .get_components()
                    .filter(|component| matches!(component, file_system::Component::Normal(_)))
                    .count();
                let mut relative = alloc::string::String::with_capacity(
                    parent_depth.saturating_mul(3) + target.get_length(),
                );
                for _ in 0..parent_depth {
                    relative.push_str("../");
                }
                let target_text = target.as_str().trim_start_matches('/');
                if target_text.is_empty() && relative.is_empty() {
                    relative.push('.');
                } else {
                    relative.push_str(target_text);
                }
                Some(relative)
            } else {
                None
            };
            let native_target = native_target_owned
                .as_deref()
                .map(Path::from_str)
                .unwrap_or(target);
            let symlink_attributes = Attributes::new()
                .set_kind(Kind::SymbolicLink)
                .set_user(user)
                .set_group(self.task_manager.get_group(task).await.map_io_error()?)
                .set_permissions(Permissions::ALL_FULL)
                .set_mask(
                    AttributeFlags::Kind
                        | AttributeFlags::User
                        | AttributeFlags::Group
                        | AttributeFlags::Permissions,
                );
            if let Some(parent) = resolved_link.parent_directory.as_ref() {
                let entry_name = resolved_link.entry_name(&file_systems)?;
                poll(|| {
                    target_file_system.create_symlink_entry(
                        native_target,
                        parent.context(),
                        entry_name,
                        &symlink_attributes,
                    )
                })
                .await?;
            } else {
                let relative_link = resolved_link.relative_path(&file_systems)?;
                poll(|| {
                    target_file_system.create_symlink(
                        native_target,
                        relative_link,
                        &symlink_attributes,
                    )
                })
                .await?;
            }
        }
        Ok(())
    }

    /// Read a symbolic link target without following it into caller-owned,
    /// bounded storage. Returns the number of bytes written.
    pub async fn read_symlink(
        &self,
        task: TaskIdentifier,
        link: &Path,
        buffer: &mut [u8],
    ) -> Result<usize> {
        self.read_symlink_with_root(task, link, buffer, None).await
    }

    /// Resolve relative paths beneath the supplied root directory.
    pub async fn read_symlink_beneath(
        &self,
        task: TaskIdentifier,
        link: &Path,
        buffer: &mut [u8],
        root: &Path,
    ) -> Result<usize> {
        self.read_symlink_with_root(task, link, buffer, Some(root))
            .await
    }

    async fn read_symlink_with_root(
        &self,
        task: TaskIdentifier,
        link: &Path,
        buffer: &mut [u8],
        root: Option<&Path>,
    ) -> Result<usize> {
        if !link.is_valid() || (root.is_none() && !link.is_absolute()) || link.is_root() {
            return Err(IoErrorKind::InvalidPath);
        }

        let user = self.task_manager.get_user(task).await.map_io_error()?;
        let file_systems = self.file_systems.read().await;
        let mut resolved_link = self
            .resolve_path_with_root(&file_systems, link, false, false, user, task, root)
            .await?;
        if resolved_link.path.is_root() || resolved_link.relative_path(&file_systems)?.is_root() {
            return Err(IoErrorKind::InvalidArgument);
        }
        self.check_target_parent_permissions(
            &file_systems,
            &mut resolved_link,
            Permission::Execute,
            user,
            task,
        )
        .await?;
        let mut attributes =
            Attributes::new().set_mask(AttributeFlags::Kind | AttributeFlags::Size);
        Self::target_attributes(&file_systems, &mut resolved_link, &mut attributes).await?;
        if attributes.get_kind() != Some(&Kind::SymbolicLink) {
            // POSIX `readlink` on something that is not a link: EINVAL.
            return Err(IoErrorKind::InvalidArgument);
        }
        let size = *attributes.get_size().ok_or(IoErrorKind::InputOutput)? as usize;
        if size > MAX_SYMLINK_TARGET_LENGTH || size > buffer.len() {
            return Err(IoErrorKind::NameTooLong);
        }
        let target_file_system = file_systems[resolved_link.mount_index].file_system;
        if let Some(parent) = resolved_link.parent_directory.as_ref() {
            let entry_name = resolved_link.entry_name(&file_systems)?;
            match poll(|| {
                target_file_system.read_symlink_entry(parent.context(), entry_name, buffer)
            })
            .await
            {
                Ok(read) => Ok(read),
                Err(IoErrorKind::Unsupported) => {
                    let mut context = Context::new_empty();
                    poll(|| {
                        target_file_system.lookup_file_entry(
                            parent.context(),
                            entry_name,
                            &mut context,
                            Flags::from(AccessFlags::Read),
                        )
                    })
                    .await?;
                    let read = target_file_system.read_sync(&mut context, &mut buffer[..size], 0);
                    let close = BaseOperations::close(target_file_system, &mut context);
                    let read = read?;
                    close?;
                    if read != size {
                        return Err(IoErrorKind::CorruptData);
                    }
                    Ok(read)
                }
                Err(error) => Err(error),
            }
        } else {
            let relative_link = resolved_link.relative_path(&file_systems)?;
            match poll(|| target_file_system.read_symlink(relative_link, buffer)).await {
                Ok(read) => Ok(read),
                Err(IoErrorKind::Unsupported) => {
                    let mut context = Context::new_empty();
                    poll(|| {
                        target_file_system.lookup_file(
                            &mut context,
                            relative_link,
                            Flags::from(AccessFlags::Read),
                        )
                    })
                    .await?;
                    let read = target_file_system.read_sync(&mut context, &mut buffer[..size], 0);
                    let close = BaseOperations::close(target_file_system, &mut context);
                    let read = read?;
                    close?;
                    if read != size {
                        return Err(IoErrorKind::CorruptData);
                    }
                    Ok(read)
                }
                Err(error) => Err(error),
            }
        }
    }

    pub async fn rename(
        &self,
        task: TaskIdentifier,
        old_path: &impl AsRef<Path>,
        new_path: &impl AsRef<Path>,
    ) -> Result<()> {
        self.rename_with_root(task, old_path.as_ref(), new_path.as_ref(), None, None)
            .await
    }

    /// Resolve relative paths beneath the supplied root directory.
    pub async fn rename_beneath(
        &self,
        task: TaskIdentifier,
        old_path: &impl AsRef<Path>,
        new_path: &impl AsRef<Path>,
        old_root: &Path,
        new_root: &Path,
    ) -> Result<()> {
        self.rename_with_root(
            task,
            old_path.as_ref(),
            new_path.as_ref(),
            Some(old_root),
            Some(new_root),
        )
        .await
    }

    async fn rename_with_root(
        &self,
        task: TaskIdentifier,
        old_path: &Path,
        new_path: &Path,
        old_root: Option<&Path>,
        new_root: Option<&Path>,
    ) -> Result<()> {
        let file_systems = self.file_systems.write().await; // Serialize source/destination resolution with rename.
        let user = self.task_manager.get_user(task).await.map_io_error()?;
        let mut old_resolved = self
            .resolve_path_with_root(&file_systems, old_path, false, false, user, task, old_root)
            .await?;
        let mut new_resolved = self
            .resolve_path_with_root(&file_systems, new_path, false, true, user, task, new_root)
            .await?;
        if old_root.is_some_and(|root| old_resolved.path.as_str() == root.as_str())
            || new_root.is_some_and(|root| new_resolved.path.as_str() == root.as_str())
            || Self::protects_mount(&file_systems, &old_resolved.path, true)
            || Self::protects_mount(&file_systems, &new_resolved.path, true)
        {
            return Err(IoErrorKind::ResourceBusy);
        }
        // A directory cannot move beneath itself. The backends do not check:
        // LittleFS would detach the whole subtree from the hierarchy.
        let source = old_resolved.path.as_str().trim_end_matches('/');
        if new_resolved.path.as_str().len() > source.len()
            && new_resolved.path.as_str().starts_with(source)
            && new_resolved.path.as_str().as_bytes()[source.len()] == b'/'
        {
            return Err(IoErrorKind::InvalidArgument);
        }

        if old_resolved.mount_index != new_resolved.mount_index {
            return Err(IoErrorKind::CrossDevice);
        }

        self.check_target_parent_permissions(
            &file_systems,
            &mut old_resolved,
            Permission::Write | Permission::Execute,
            user,
            task,
        )
        .await?;
        self.check_target_parent_permissions(
            &file_systems,
            &mut new_resolved,
            Permission::Write | Permission::Execute,
            user,
            task,
        )
        .await?;

        self.check_target_sticky_permissions(&file_systems, &mut old_resolved, user)
            .await?;

        match self
            .check_target_sticky_permissions(&file_systems, &mut new_resolved, user)
            .await
        {
            Ok(()) | Err(IoErrorKind::NotFound) => {}
            Err(error) => return Err(error),
        }

        let target_file_system = file_systems[old_resolved.mount_index].file_system;
        if let (Some(old_parent), Some(new_parent)) = (
            old_resolved.parent_directory.as_ref(),
            new_resolved.parent_directory.as_ref(),
        ) {
            let old_name = old_resolved.entry_name(&file_systems)?;
            let new_name = new_resolved.entry_name(&file_systems)?;
            poll(|| {
                target_file_system.rename_entry(
                    old_parent.context(),
                    old_name,
                    new_parent.context(),
                    new_name,
                )
            })
            .await?;
        } else {
            let old_relative = old_resolved.relative_path(&file_systems)?;
            let new_relative = new_resolved.relative_path(&file_systems)?;
            poll(|| target_file_system.rename(old_relative, new_relative)).await?;
        }

        Ok(())
    }

    /// Create `new_path` as a hard link to the non-directory object at `existing`.
    pub async fn link(
        &self,
        task: TaskIdentifier,
        existing: &impl AsRef<Path>,
        follow_existing: bool,
        new_path: &impl AsRef<Path>,
    ) -> Result<()> {
        self.link_with_root(
            task,
            existing.as_ref(),
            follow_existing,
            new_path.as_ref(),
            None,
            None,
        )
        .await
    }

    /// Resolve both paths beneath their supplied root directories.
    pub async fn link_beneath(
        &self,
        task: TaskIdentifier,
        existing: &impl AsRef<Path>,
        follow_existing: bool,
        new_path: &impl AsRef<Path>,
        existing_root: &Path,
        new_root: &Path,
    ) -> Result<()> {
        self.link_with_root(
            task,
            existing.as_ref(),
            follow_existing,
            new_path.as_ref(),
            Some(existing_root),
            Some(new_root),
        )
        .await
    }

    async fn link_with_root(
        &self,
        task: TaskIdentifier,
        existing: &Path,
        follow_existing: bool,
        new_path: &Path,
        existing_root: Option<&Path>,
        new_root: Option<&Path>,
    ) -> Result<()> {
        let file_systems = self.file_systems.write().await; // Serialize source/destination resolution with the link.
        let user = self.task_manager.get_user(task).await.map_io_error()?;
        let mut existing_resolved = self
            .resolve_path_with_root(
                &file_systems,
                existing,
                follow_existing,
                false,
                user,
                task,
                existing_root,
            )
            .await?;
        let mut new_resolved = self
            .resolve_path_with_root(&file_systems, new_path, false, true, user, task, new_root)
            .await?;
        if Self::protects_mount(&file_systems, &new_resolved.path, true) {
            return Err(IoErrorKind::ResourceBusy);
        }

        if existing_resolved.mount_index != new_resolved.mount_index {
            return Err(IoErrorKind::CrossDevice);
        }

        let mut kind = Attributes::new().set_mask(AttributeFlags::Kind);
        Self::target_attributes(&file_systems, &mut existing_resolved, &mut kind).await?;
        if kind.get_kind() == Some(&Kind::Directory) {
            return Err(IoErrorKind::IsADirectory);
        }
        let mut destination = Attributes::new().set_mask(AttributeFlags::Kind);
        match Self::target_attributes(&file_systems, &mut new_resolved, &mut destination).await {
            Ok(()) => return Err(IoErrorKind::AlreadyExists),
            Err(IoErrorKind::NotFound) => {}
            Err(error) => return Err(error),
        }
        self.check_target_parent_permissions(
            &file_systems,
            &mut new_resolved,
            Permission::Write | Permission::Execute,
            user,
            task,
        )
        .await?;

        let target_file_system = file_systems[new_resolved.mount_index].file_system;
        if let (Some(existing_parent), Some(new_parent)) = (
            existing_resolved.parent_directory.as_ref(),
            new_resolved.parent_directory.as_ref(),
        ) {
            let existing_name = existing_resolved.entry_name(&file_systems)?;
            let new_name = new_resolved.entry_name(&file_systems)?;
            poll(|| {
                target_file_system.link_entry(
                    existing_parent.context(),
                    existing_name,
                    new_parent.context(),
                    new_name,
                )
            })
            .await
        } else {
            let existing_relative = existing_resolved.relative_path(&file_systems)?;
            let new_relative = new_resolved.relative_path(&file_systems)?;
            poll(|| target_file_system.link(existing_relative, new_relative)).await
        }
    }

    pub async fn get_statistics(&self, path: &impl AsRef<Path>) -> Result<Statistics> {
        let task = self
            .task_manager
            .try_get_current_task_identifier()
            .await
            .map_io_error()?;
        self.get_statistics_for_task(task, path).await
    }

    /// Read path metadata for an explicit task, including from synchronous adapters
    /// whose polling context does not carry an Embassy task waker.
    pub async fn get_statistics_for_task(
        &self,
        task: TaskIdentifier,
        path: &impl AsRef<Path>,
    ) -> Result<Statistics> {
        self.get_statistics_for_task_with_follow_with_root(task, path.as_ref(), true, None)
            .await
    }

    /// Read metadata for the directory entry itself without following its final link.
    pub async fn get_link_statistics_for_task(
        &self,
        task: TaskIdentifier,
        path: &impl AsRef<Path>,
    ) -> Result<Statistics> {
        self.get_statistics_for_task_with_follow_with_root(task, path.as_ref(), false, None)
            .await
    }

    /// Return metadata for the directory entry itself without following a final symbolic link.
    pub async fn get_link_statistics(&self, path: &impl AsRef<Path>) -> Result<Statistics> {
        let task = self
            .task_manager
            .try_get_current_task_identifier()
            .await
            .map_io_error()?;
        self.get_link_statistics_for_task(task, path).await
    }

    /// Resolve relative paths beneath the supplied root directory.
    pub async fn get_statistics_beneath(
        &self,
        task: TaskIdentifier,
        path: &impl AsRef<Path>,
        follow_final: bool,
        root: &Path,
    ) -> Result<Statistics> {
        self.get_statistics_for_task_with_follow_with_root(
            task,
            path.as_ref(),
            follow_final,
            Some(root),
        )
        .await
    }

    async fn get_statistics_for_task_with_follow_with_root(
        &self,
        task: TaskIdentifier,
        path: &Path,
        follow_final: bool,
        root: Option<&Path>,
    ) -> Result<Statistics> {
        let file_systems = self.file_systems.read().await;
        let user = self.task_manager.get_user(task).await.map_io_error()?;
        let mut resolved = self
            .resolve_path_with_root(&file_systems, path, follow_final, false, user, task, root)
            .await?;

        let mut attributes = Attributes::new().set_mask(AttributeFlags::all());
        Self::target_attributes(&file_systems, &mut resolved, &mut attributes).await?;
        Statistics::from_attributes(&attributes).ok_or(IoErrorKind::InputOutput)
    }

    /// Take one more reference on the entry `item` was opened on.
    ///
    /// `device_inode` is the inode a device opening took its reference on; it
    /// is required for devices, because two mounted devices may share a pointer.
    pub(crate) async fn retain(
        &self,
        item: &ItemStatic,
        device_inode: Option<Inode>,
    ) -> Result<()> {
        match item {
            ItemStatic::File(file_system) | ItemStatic::Directory(file_system) => {
                self.file_systems
                    .write()
                    .await
                    .iter_mut()
                    .find_map(|fs| {
                        if ptr::eq(fs.file_system, *file_system) {
                            fs.reference_count += 1;
                            Some(())
                        } else {
                            None
                        }
                    })
                    .ok_or(IoErrorKind::BadDescriptor)?;
            }
            ItemStatic::Pipe(_) => {}
            ItemStatic::BlockDevice(_) => {
                let inode = device_inode.ok_or(IoErrorKind::BadDescriptor)?;
                self.block_device
                    .write()
                    .await
                    .get_mut(&inode)
                    .ok_or(IoErrorKind::BadDescriptor)?
                    .reference_count += 1;
            }
            ItemStatic::CharacterDevice(_) => {
                let inode = device_inode.ok_or(IoErrorKind::BadDescriptor)?;
                self.character_device
                    .write()
                    .await
                    .get_mut(&inode)
                    .ok_or(IoErrorKind::BadDescriptor)?
                    .reference_count += 1;
            }
            _ => {
                return Err(IoErrorKind::BadDescriptor);
            }
        }

        Ok(())
    }

    /// Release the reference `item` took when it was opened, then close it.
    ///
    /// `device_inode` is the inode a device opening took its reference on; it
    /// is required for devices, because two mounted devices may share a pointer.
    pub async fn close(
        &self,
        item: &ItemStatic,
        device_inode: Option<Inode>,
        context: &mut Context,
    ) -> Result<()> {
        let released = match Owed::of(item, device_inode)? {
            None => true,
            Some(Owed::FileSystem(address)) => {
                let mut file_systems = self.file_systems.write().await;
                self.settle_file_systems(&mut file_systems);
                release_file_system(&mut file_systems, address)
            }
            Some(Owed::BlockDevice(inode)) => {
                let mut devices = self.block_device.write().await;
                self.settle_block_devices(&mut devices);
                release_block_device(&mut devices, inode)
            }
            Some(Owed::CharacterDevice(inode)) => {
                let mut devices = self.character_device.write().await;
                self.settle_character_devices(&mut devices);
                release_character_device(&mut devices, inode)
            }
        };
        if !released {
            return Err(IoErrorKind::BadDescriptor);
        }
        Self::close_context(item, context)
    }

    /// Close like [`Self::close`], but without ever waiting, for a destructor: closing in a
    /// drop must not suspend, or a cancellation could abandon it halfway. The item's own
    /// context is closed at once. Its reference is given back at once if the lock it is
    /// counted under is free, and otherwise owed: whoever takes that lock next to look at
    /// the counts (a close, an unmount, the storage shutdown) gives it back first.
    pub fn close_without_waiting(
        &self,
        item: &ItemStatic,
        device_inode: Option<Inode>,
        context: &mut Context,
    ) -> Result<()> {
        if let Some(entry) = Owed::of(item, device_inode)? {
            let released = match entry {
                Owed::FileSystem(address) => self.file_systems.try_write().map(|mut locked| {
                    self.settle_file_systems(&mut locked);
                    release_file_system(&mut locked, address)
                }),
                Owed::BlockDevice(inode) => self.block_device.try_write().map(|mut locked| {
                    self.settle_block_devices(&mut locked);
                    release_block_device(&mut locked, inode)
                }),
                Owed::CharacterDevice(inode) => {
                    self.character_device.try_write().map(|mut locked| {
                        self.settle_character_devices(&mut locked);
                        release_character_device(&mut locked, inode)
                    })
                }
            };
            match released {
                Ok(true) => {}
                Ok(false) => return Err(IoErrorKind::BadDescriptor),
                Err(_) => {
                    let queued = self.owed.lock(|owed| {
                        let mut owed = owed.borrow_mut();
                        let reserved = owed.try_reserve(1).is_ok();
                        if reserved {
                            owed.push(entry);
                        }
                        reserved
                    });
                    if !queued {
                        log::warning!("Out of memory: a closed item stays counted as open");
                    }
                }
            }
        }
        Self::close_context(item, context)
    }

    /// Close the item's own context: the part of a close that never waits.
    fn close_context(item: &ItemStatic, context: &mut Context) -> Result<()> {
        if let Some(operations) = item.as_directory_operations() {
            operations.close(context)
        } else {
            item.as_base_operations()
                .ok_or(IoErrorKind::Unsupported)?
                .close(context)
        }
    }

    /// Record a mutation timestamp (modification and status change time) on an
    /// open file handle.
    ///
    /// The backend stores the times in the handle's pending metadata, so they
    /// are committed atomically with the file data on the next sync or close
    /// of the handle instead of costing a separate metadata commit. A backend
    /// that cannot persist times reports `Unsupported`, which is not an error
    /// here.
    pub(crate) fn set_handle_times(
        file_system: &dyn FileSystemOperations,
        context: &mut Context,
        time: file_system::Time,
    ) -> Result<()> {
        let attributes = Attributes::new().set_modification(time).set_status(time);
        let mask = attributes
            .get_mask()
            .intersection(file_system.writable_attributes());
        if mask.is_empty() {
            return Ok(());
        }
        match AttributeOperations::set_attributes(file_system, context, &attributes.set_mask(mask))
        {
            Err(IoErrorKind::Unsupported) => Ok(()),
            result => result.map(|_| ()),
        }
    }

    /// Current UTC time when the clock is synchronized.
    pub(crate) fn current_time(&self) -> Result<file_system::Time> {
        self.get_time()
    }

    /// Record a mutation timestamp on an open handle of a regular file, using
    /// the current UTC time. When UTC is unavailable the previous timestamps
    /// are kept, as on open.
    pub(crate) fn touch_handle_modification_time(
        &self,
        item: &ItemStatic,
        context: &mut Context,
    ) -> Result<()> {
        let ItemStatic::File(file_system) = item else {
            return Ok(());
        };
        let Ok(time) = self.get_time() else {
            return Ok(());
        };
        Self::set_handle_times(*file_system, context, time)
    }

    /// Single-attempt metadata adapter: never block an executor waiting for a
    /// namespace guard held by another task. Path mutations use the same guard.
    pub(crate) fn set_handle_attributes(
        &self,
        authority: &task::TaskAuthority<'_>,
        item: &ItemStatic,
        context: &mut Context,
        attributes: &Attributes,
    ) -> Result<()> {
        if attributes.get_mask().is_empty() {
            return Ok(());
        }
        let _namespace = self
            .file_systems
            .try_write()
            .map_err(|_| IoErrorKind::WouldBlock)?;
        let user = authority
            .try_user()
            .map_io_error()?
            .ok_or(IoErrorKind::WouldBlock)?;
        let operations = item
            .as_attributes_operations()
            .ok_or(IoErrorKind::Unsupported)?;
        let mut current = Attributes::new().set_mask(attributes.get_mask() | AttributeFlags::User);
        operations.get_attributes(context, &mut current)?;
        Self::authorize_metadata_change(
            user,
            *current.get_user().ok_or(IoErrorKind::InputOutput)?,
            attributes,
        )?;
        if attributes
            .get_mask()
            .intersects(AttributeFlags::User | AttributeFlags::Group)
        {
            return Err(IoErrorKind::Unsupported);
        }
        let current_mask = current.get_mask().intersection(attributes.get_mask());
        let current_for_mask = current.set_mask(current_mask);
        if attributes_equal(&current_for_mask, attributes) {
            return Ok(());
        }
        operations.set_attributes(context, attributes)?;
        Ok(())
    }

    /// Update timestamps after resolving beneath an immutable preopen authority.
    pub async fn set_times_beneath(
        &self,
        task: TaskIdentifier,
        path: &impl AsRef<Path>,
        follow_final: bool,
        root: &Path,
        access: Option<file_system::Time>,
        modification: Option<file_system::Time>,
    ) -> Result<()> {
        self.set_times_beneath_inner(
            task,
            path.as_ref(),
            follow_final,
            root,
            access,
            modification,
        )
        .await
    }

    async fn set_times_beneath_inner(
        &self,
        task: TaskIdentifier,
        path: &Path,
        follow_final: bool,
        root: &Path,
        access: Option<file_system::Time>,
        modification: Option<file_system::Time>,
    ) -> Result<()> {
        let file_systems = self.file_systems.write().await;
        let user = self.task_manager.get_user(task).await.map_io_error()?;
        let mut resolved = self
            .resolve_path_with_root(
                &file_systems,
                path,
                follow_final,
                false,
                user,
                task,
                Some(root),
            )
            .await?;
        let mut attributes = Attributes::new();
        if let Some(time) = access {
            attributes = attributes.set_access(time);
        }
        if let Some(time) = modification {
            attributes = attributes.set_modification(time);
        }
        let mut owner = Attributes::new().set_mask(AttributeFlags::User);
        Self::target_attributes(&file_systems, &mut resolved, &mut owner).await?;
        Self::authorize_metadata_change(
            user,
            *owner.get_user().ok_or(IoErrorKind::InputOutput)?,
            &attributes,
        )?;
        Self::set_target_attributes(&file_systems, &mut resolved, &attributes).await
    }

    pub async fn set_ownership(
        &self,
        task: TaskIdentifier,
        path: impl AsRef<Path>,
        user: Option<UserIdentifier>,
        group: Option<GroupIdentifier>,
    ) -> Result<()> {
        self.set_ownership_inner(task, path.as_ref(), user, group)
            .await
    }

    async fn set_ownership_inner(
        &self,
        task: TaskIdentifier,
        path: &Path,
        user: Option<UserIdentifier>,
        group: Option<GroupIdentifier>,
    ) -> Result<()> {
        let file_systems = self.file_systems.write().await; // Serialize metadata authority with mutation.
        let current_user = self.task_manager.get_user(task).await.map_io_error()?;
        let mut resolved = self
            .resolve_path(&file_systems, path, true, false, current_user, task)
            .await?;

        let mut attributes = Attributes::new();
        if let Some(user) = user {
            attributes = attributes.set_user(user);
        }
        if let Some(group) = group {
            attributes = attributes.set_group(group);
        }
        let mut owner = Attributes::new().set_mask(AttributeFlags::User);
        Self::target_attributes(&file_systems, &mut resolved, &mut owner).await?;
        Self::authorize_metadata_change(
            current_user,
            *owner.get_user().ok_or(IoErrorKind::InputOutput)?,
            &attributes,
        )?;
        Self::set_target_attributes(&file_systems, &mut resolved, &attributes).await
    }

    pub async fn set_permissions(
        &self,
        task: TaskIdentifier,
        path: impl AsRef<Path>,
        permissions: Permissions,
    ) -> Result<()> {
        self.set_permissions_inner(task, path.as_ref(), permissions)
            .await
    }

    async fn set_permissions_inner(
        &self,
        task: TaskIdentifier,
        path: &Path,
        permissions: Permissions,
    ) -> Result<()> {
        let file_systems = self.file_systems.write().await; // Get the file systems
        let current_user = self.task_manager.get_user(task).await.map_io_error()?;
        let mut resolved = self
            .resolve_path(&file_systems, path, true, false, current_user, task)
            .await?;

        let attributes = Attributes::new().set_permissions(permissions);
        let mut owner = Attributes::new().set_mask(AttributeFlags::User);
        Self::target_attributes(&file_systems, &mut resolved, &mut owner).await?;
        Self::authorize_metadata_change(
            current_user,
            *owner.get_user().ok_or(IoErrorKind::InputOutput)?,
            &attributes,
        )?;
        Self::set_target_attributes(&file_systems, &mut resolved, &attributes).await
    }
}
