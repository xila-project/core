mod utilities;

use crate::pipe::Pipe;
use crate::{Directory, Error, File, ItemStatic, Result, poll};
use alloc::borrow::ToOwned;
use alloc::vec;
use alloc::{boxed::Box, collections::BTreeMap};
use core::ptr;
use exported_file_system::{BlockDevice, CharacterDevice, CreateFlags, Permission, Permissions};
use file_system::{
    AccessFlags, AttributeFlags, Attributes, Context, FileSystemOperations, Flags, Kind, Path,
    StateFlags, Statistics,
};
use synchronization::{blocking_mutex::raw::CriticalSectionRawMutex, rwlock::RwLock};
use task::TaskIdentifier;
use users::{GroupIdentifier, UserIdentifier};
use utilities::*;

pub fn initialize(
    task_manager: &'static task::Manager,
    users_manager: &'static users::Manager,
    time_manager: &'static time::Manager<'static>,
    root_file_system: impl FileSystemOperations + 'static,
) -> Result<VirtualFileSystem> {
    Ok(VirtualFileSystem::new(
        task_manager,
        users_manager,
        time_manager,
        root_file_system,
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
        let file_systems = vec![InternalFileSystem {
            reference_count: 1,
            is_mounted: true,
            mount_point: Path::ROOT.to_owned(),
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
        }
    }

    pub async fn uninitialize(&self) {
        let mut file_systems = self.file_systems.write().await;

        for internal_file_system in file_systems.drain(..) {
            let _: Box<dyn FileSystemOperations> =
                unsafe { Box::from_raw(internal_file_system.file_system as *const _ as *mut _) };
        }
    }

    /// Mount a file system at a given mount point.
    pub async fn mount_file_system(
        &'static self,
        file_system: impl FileSystemOperations + 'static,
        path: impl AsRef<Path>,
        task: TaskIdentifier,
    ) -> Result<()> {
        self.mount_static(
            task,
            path,
            ItemStatic::FileSystem(Box::leak(Box::new(file_system))),
        )
        .await
    }

    pub async fn unmount(&self, path: impl AsRef<Path>, force: bool) -> Result<()> {
        let path = path.as_ref();

        if !path.is_valid() || !path.is_absolute() || path.is_root() {
            return Err(Error::InvalidPath);
        }

        let mut file_systems = self.file_systems.write().await;

        let parent_path = path.go_parent().ok_or(Error::InvalidPath)?;

        let (underlying_file_system, _, _) =
            Self::get_file_system_from_path(&file_systems, &parent_path)?; // Get the file system identifier and the relative path
        let relative_path = path
            .strip_prefix_absolute(&underlying_file_system.mount_point)
            .ok_or(Error::InvalidPath)?;

        let mut attributes =
            Attributes::new().set_mask(AttributeFlags::Kind | AttributeFlags::Inode);
        Self::get_attributes(
            underlying_file_system.file_system,
            relative_path,
            &mut attributes,
        )
        .await?;

        let kind = attributes.get_kind().ok_or(Error::MissingAttribute)?;
        let inode = *attributes.get_inode().ok_or(Error::MissingAttribute)?;

        match kind {
            Kind::Directory => {
                let mountpoint_index = file_systems
                    .iter()
                    .position(|file_system| {
                        let mount_point: &Path = file_system.mount_point.as_ref();
                        file_system.is_mounted && mount_point == path
                    })
                    .ok_or(Error::InvalidPath)?;
                let file_system = &file_systems[mountpoint_index];

                if file_system.reference_count > 1 && !force {
                    return Err(Error::RessourceBusy);
                }

                underlying_file_system.file_system.remove(relative_path)?;
                file_system.file_system.unmount()?;
                file_systems[mountpoint_index].is_mounted = false;
            }
            Kind::BlockDevice => {
                let mut block_devices = self.block_device.write().await;
                let device = block_devices.get_mut(&inode).ok_or(Error::InvalidInode)?;

                if device.reference_count > 1 && !force {
                    return Err(Error::RessourceBusy);
                }

                underlying_file_system.file_system.remove(relative_path)?;
                device.device.unmount()?;
            }
            Kind::CharacterDevice => {
                let mut character_devices = self.character_device.write().await;
                let device = character_devices
                    .get_mut(&inode)
                    .ok_or(Error::InvalidInode)?;

                if device.reference_count > 1 && !force {
                    return Err(Error::RessourceBusy);
                }

                underlying_file_system.file_system.remove(relative_path)?;
                device.device.unmount()?;
            }
            _ => {
                return Err(Error::UnsupportedOperation);
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

    pub async fn open_directory<'a>(
        &'a self,
        task: TaskIdentifier,
        path: &impl AsRef<Path>,
    ) -> Result<Directory<'a>> {
        let path = path.as_ref();

        let mut file_systems = self.file_systems.write().await; // Get the file systems

        let (time, user, _) = self.get_time_user_group(task).await?;

        self.check_permissions_with_parent(
            &file_systems,
            path,
            Permission::Read,
            Permission::Execute,
            user,
            task,
        )
        .await?;

        let (file_system, relative_path, _) =
            Self::get_mutable_file_system_from_path(&mut file_systems, &path)?; // Get the file system identifier and the relative path

        let mut attributes = Attributes::new().set_mask(
            AttributeFlags::Kind
                | AttributeFlags::User
                | AttributeFlags::Group
                | AttributeFlags::Permissions,
        );
        Self::get_attributes(file_system.file_system, relative_path, &mut attributes).await?;
        let kind = attributes.get_kind().ok_or(Error::MissingAttribute)?;

        if *kind != Kind::Directory {
            return Err(Error::NotADirectory);
        }

        let attributes = Attributes::new().set_access(time);
        let mask = attributes
            .get_mask()
            .intersection(file_system.file_system.writable_attributes());
        let attributes = attributes.set_mask(mask);
        Self::set_attributes(file_system.file_system, relative_path, &attributes).await?;

        let mut context = Context::new_empty();

        poll(|| {
            Ok(file_system
                .file_system
                .lookup_directory(&mut context, relative_path)?)
        })
        .await?;

        file_system.reference_count += 1;

        Ok(Directory::new(
            self,
            file_system.file_system,
            Flags::new(AccessFlags::Read, None, None),
            context,
        ))
    }

    pub async fn open<'a>(
        &'a self,
        path: &impl AsRef<Path>,
        flags: Flags,
        task: TaskIdentifier,
    ) -> Result<File<'a>> {
        let path = path.as_ref();

        let mut file_systems = self.file_systems.write().await; // Get the file systems

        let (time, user, group) = self.get_time_user_group(task).await?;
        let (mode, open, _) = flags.split();

        if (open.contains(CreateFlags::Truncate) || flags.get_state().contains(StateFlags::Append))
            && !mode.contains(AccessFlags::Write)
        {
            return Err(Error::InvalidOpen);
        }
        let (file_system, relative_path, _) =
            Self::get_file_system_from_path(&file_systems, &path)?;
        let mut attributes = Attributes::new().set_mask(AttributeFlags::Kind);
        let exists =
            match Self::get_attributes(file_system.file_system, relative_path, &mut attributes)
                .await
            {
                Ok(()) => true,
                Err(Error::FileSystem(file_system::Error::NotFound)) => false,
                Err(error) => return Err(error),
            };
        if exists {
            if open.contains(CreateFlags::Create) && open.contains(CreateFlags::Exclusive) {
                return Err(Error::AlreadyExists);
            }
            self.check_permissions_with_parent(
                &file_systems,
                path,
                mode.into_permission(),
                Permission::Execute,
                user,
                task,
            )
            .await?;
        } else if open.contains(CreateFlags::Create) {
            // Authorize before mutating the filesystem, including failed redirections.
            self.check_permissions(
                file_system.file_system,
                relative_path.go_parent().ok_or(Error::InvalidPath)?,
                Permission::Write | Permission::Execute,
                user,
                task,
            )
            .await?;
            poll(|| Ok(file_system.file_system.create_file(relative_path)?)).await?;
            let attributes = Attributes::new()
                .set_user(user)
                .set_group(group)
                .set_creation(time)
                .set_modification(time)
                .set_status(time)
                .set_access(time)
                .set_permissions(Permissions::FILE_DEFAULT)
                .set_kind(Kind::File);
            let mask = attributes
                .get_mask()
                .intersection(file_system.file_system.writable_attributes());
            let attributes = attributes.set_mask(mask);
            Self::set_attributes(file_system.file_system, relative_path, &attributes).await?;
        } else {
            return Err(Error::FileSystem(file_system::Error::NotFound));
        }

        let (file_system, relative_path, _) =
            Self::get_mutable_file_system_from_path(&mut file_systems, &path)?; // Get the file system identifier and the relative path

        let attributes = if mode.contains(AccessFlags::Write) {
            Attributes::new().set_modification(time).set_access(time)
        } else {
            Attributes::new().set_access(time)
        };
        let mask = attributes
            .get_mask()
            .intersection(file_system.file_system.writable_attributes());
        let attributes = attributes.set_mask(mask);
        Self::set_attributes(file_system.file_system, relative_path, &attributes).await?;

        let mut attributes =
            Attributes::new().set_mask(AttributeFlags::Inode | AttributeFlags::Kind);
        Self::get_attributes(file_system.file_system, relative_path, &mut attributes).await?;

        let kind = attributes.get_kind().ok_or(Error::MissingAttribute)?;
        let inode = *attributes.get_inode().ok_or(Error::MissingAttribute)?;

        let mut context = Context::new_empty();

        let file = match kind {
            Kind::CharacterDevice => {
                let mut devices = self.character_device.write().await;

                let device = devices.get_mut(&inode).ok_or(Error::InvalidInode)?;

                poll(|| Ok(device.device.open(&mut context)?)).await?;
                device.reference_count += 1;

                File::new(
                    self,
                    ItemStatic::CharacterDevice(device.device),
                    flags,
                    context,
                )
            }
            Kind::BlockDevice => {
                let mut devices = self.block_device.write().await;

                let device = devices.get_mut(&inode).ok_or(Error::InvalidInode)?;

                poll(|| Ok(device.device.open(&mut context)?)).await?;
                device.reference_count += 1;

                File::new(self, ItemStatic::BlockDevice(device.device), flags, context)
            }
            Kind::Pipe => {
                let mut pipes = self.pipes.write().await;
                let pipe = pipes.get_mut(&inode).ok_or(Error::InvalidInode)?;

                context = pipe.pipe.open_context(flags.get_access())?;
                File::new(self, ItemStatic::Pipe(pipe.pipe.clone()), flags, context)
            }
            Kind::File => {
                poll(|| {
                    Ok(file_system.file_system.lookup_file(
                        &mut context,
                        relative_path,
                        Flags::new(
                            flags.get_access(),
                            Some(if flags.get_create().contains(CreateFlags::Truncate) {
                                CreateFlags::Truncate
                            } else {
                                CreateFlags::from_bits_truncate(0)
                            }),
                            Some(flags.get_state()),
                        ),
                    )?)
                })
                .await?;

                file_system.reference_count += 1;

                File::new(
                    self,
                    ItemStatic::File(file_system.file_system),
                    flags,
                    context,
                )
            }
            _ => Err(Error::UnsupportedOperation)?,
        };

        Ok(file)
    }

    pub async fn mount_static(
        &self,
        task: TaskIdentifier,
        path: impl AsRef<Path>,
        item: ItemStatic,
    ) -> Result<()> {
        let path: &Path = path.as_ref();
        if !path.is_valid() || !path.is_absolute() || path.is_root() {
            return Err(Error::InvalidPath);
        }

        let mut file_systems = self.file_systems.write().await; // Get the file systems

        if file_systems.iter().any(|file_system| {
            let mount_point: &Path = file_system.mount_point.as_ref();
            file_system.is_mounted && mount_point == path
        }) {
            return Err(Error::AlreadyExists);
        }

        let (parent_file_system, relative_path, _) =
            Self::get_mutable_file_system_from_path(&mut file_systems, &path)?; // Get the file system identifier and the relative path

        let (_, user, _) = self.get_time_user_group(task).await?;

        self.check_permissions(
            parent_file_system.file_system,
            relative_path.go_parent().ok_or(Error::InvalidPath)?,
            Permission::Write,
            user,
            task,
        )
        .await?;

        if let ItemStatic::FileSystem(_) = item {
            match parent_file_system
                .file_system
                .create_directory(relative_path)
            {
                Ok(()) => {}
                Err(file_system::Error::AlreadyExists) => {
                    let mut attributes = Attributes::new().set_mask(AttributeFlags::Kind);
                    Self::get_attributes(
                        parent_file_system.file_system,
                        relative_path,
                        &mut attributes,
                    )
                    .await?;
                    if attributes.get_kind() != Some(&Kind::Directory) {
                        return Err(Error::AlreadyExists);
                    }
                }
                Err(error) => return Err(error.into()),
            }
        } else {
            parent_file_system.file_system.create_file(relative_path)?;
        }

        let inode = match item {
            ItemStatic::BlockDevice(device) => {
                let mut block_devices = self.block_device.write().await;
                let inode = Self::get_new_inode(&*block_devices).ok_or(Error::TooManyInodes)?;
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
                let inode = Self::get_new_inode(&*character_devices).ok_or(Error::TooManyInodes)?;
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
                let inode = Self::get_new_inode(&*pipes).ok_or(Error::TooManyInodes)?;
                pipes.insert(inode, InternalPipe { pipe: pipe.clone() });
                inode
            }
            _ => {
                return Err(Error::InvalidIdentifier);
            }
        };

        let (time, user, group) = self.get_time_user_group(task).await?;
        let underlying_kind = match &item {
            ItemStatic::FileSystem(_) => Kind::Directory,
            ItemStatic::BlockDevice(_) => Kind::BlockDevice,
            ItemStatic::CharacterDevice(_) => Kind::CharacterDevice,
            ItemStatic::Pipe(_) => Kind::Pipe,
            _ => {
                return Err(Error::InvalidIdentifier);
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
            .set_permissions(Permissions::DEVICE_DEFAULT)
            .set_inode(inode);
        let mask = attributes
            .get_mask()
            .intersection(parent_file_system.file_system.writable_attributes());
        let attributes = attributes.set_mask(mask);
        Self::set_attributes(parent_file_system.file_system, relative_path, &attributes).await?;

        poll(|| {
            Ok(item
                .as_mount_operations()
                .ok_or(Error::UnsupportedOperation)?
                .mount()?)
        })
        .await?;

        if let ItemStatic::FileSystem(file_system) = item {
            file_systems.push(InternalFileSystem {
                reference_count: 1,
                is_mounted: true,
                mount_point: path.to_owned(),
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
            return Err(Error::FileSystem(file_system::Error::InvalidParameter));
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
        let file_systems = self.file_systems.read().await; // Get the file systems

        let (file_system, relative_path, _) =
            Self::get_file_system_from_path(&file_systems, &path)?; // Get the file system identifier and the relative path

        let (_, user, _) = self.get_time_user_group(task).await?;

        self.check_permissions(
            file_system.file_system,
            relative_path.go_parent().ok_or(Error::InvalidPath)?,
            Permission::Write,
            user,
            task,
        )
        .await?;

        let mut attributes =
            Attributes::new().set_mask(AttributeFlags::Kind | AttributeFlags::Inode);

        Self::get_attributes(file_system.file_system, relative_path, &mut attributes).await?;

        let inode = *attributes.get_inode().ok_or(Error::MissingAttribute)?;
        let kind = attributes.get_kind().ok_or(Error::MissingAttribute)?;

        if kind == &Kind::Pipe {
            let mut named_pipes = self.pipes.write().await;

            named_pipes.remove(&inode);
        }

        poll(|| Ok(file_system.file_system.remove(relative_path)?)).await?;

        Ok(())
    }

    pub async fn create_directory(
        &self,
        task: TaskIdentifier,
        path: &impl AsRef<Path>,
    ) -> Result<()> {
        let file_systems = self.file_systems.read().await; // Get the file systems

        let (file_system, relative_path, _) = Self::get_file_system_from_path(&file_systems, path)?; // Get the file system identifier and the relative path

        let (time, user, group) = self.get_time_user_group(task).await?;

        // Get the parent directory attributes
        self.check_permissions(
            file_system.file_system,
            relative_path.go_parent().ok_or(Error::InvalidPath)?,
            Permission::Write,
            user,
            task,
        )
        .await?;

        match poll(|| Ok(file_system.file_system.create_directory(relative_path)?)).await {
            Ok(()) => {}
            Err(Error::FileSystem(file_system::Error::AlreadyExists)) => {
                return Err(Error::AlreadyExists);
            }
            Err(e) => {
                return Err(e);
            }
        }

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
            .set_permissions(Permissions::DIRECTORY_DEFAULT);
        let mask = attributes
            .get_mask()
            .intersection(file_system.file_system.writable_attributes());
        let attributes = attributes.set_mask(mask);

        Self::set_attributes(file_system.file_system, relative_path, &attributes).await?;

        Ok(())
    }

    pub async fn rename(
        &self,
        task: TaskIdentifier,
        old_path: &impl AsRef<Path>,
        new_path: &impl AsRef<Path>,
    ) -> Result<()> {
        let file_systems = self.file_systems.read().await; // Get the file systems

        let (old_file_system, old_relative_path, _) =
            Self::get_file_system_from_path(&file_systems, old_path)?; // Get the file system identifier and the relative path

        let (new_file_system, new_relative_path, _) =
            Self::get_file_system_from_path(&file_systems, new_path)?; // Get the file system identifier and the relative path

        if !ptr::eq(old_file_system, new_file_system) {
            return Err(Error::InvalidPath);
        }

        let (_, user, _) = self.get_time_user_group(task).await?;

        for parent_path in [
            old_relative_path.go_parent().ok_or(Error::InvalidPath)?,
            new_relative_path.go_parent().ok_or(Error::InvalidPath)?,
        ] {
            self.check_permissions(
                old_file_system.file_system,
                parent_path,
                Permission::Write,
                user,
                task,
            )
            .await?;
        }

        poll(|| {
            Ok(old_file_system
                .file_system
                .rename(old_relative_path, new_relative_path)?)
        })
        .await?;

        Ok(())
    }

    pub async fn get_statistics(&self, path: &impl AsRef<Path>) -> Result<Statistics> {
        let task = self.task_manager.get_current_task_identifier().await;
        self.get_statistics_for_task(task, path).await
    }

    /// Read path metadata for an explicit task, including from synchronous adapters
    /// whose polling context does not carry an Embassy task waker.
    pub async fn get_statistics_for_task(
        &self,
        task: TaskIdentifier,
        path: &impl AsRef<Path>,
    ) -> Result<Statistics> {
        let file_systems = self.file_systems.read().await;

        let (file_system, relative_path, _) = Self::get_file_system_from_path(&file_systems, path)?; // Get the file system identifier and the relative path

        let user = self.task_manager.get_user(task).await?;
        if user != users::UserIdentifier::ROOT {
            let application = self.task_manager.get_application_identifier(task).await?;
            if !utilities::application_access_allowed(relative_path, application, Permission::Read)
            {
                return Err(Error::PermissionDenied);
            }
        }

        let mut attributes = Attributes::new().set_mask(AttributeFlags::all());

        Self::get_attributes(file_system.file_system, relative_path, &mut attributes).await?;

        Statistics::from_attributes(&attributes).ok_or(Error::MissingAttribute)
    }

    pub(crate) async fn retain(&self, item: &ItemStatic) -> Result<()> {
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
                    .ok_or(Error::InvalidIdentifier)?;
            }
            ItemStatic::Pipe(_) => {}
            ItemStatic::BlockDevice(device) => {
                self.block_device
                    .write()
                    .await
                    .iter_mut()
                    .find_map(|(_, d)| {
                        if ptr::eq(d.device, *device) {
                            d.reference_count += 1;
                            Some(())
                        } else {
                            None
                        }
                    })
                    .ok_or(Error::InvalidIdentifier)?;
            }
            ItemStatic::CharacterDevice(device) => {
                self.character_device
                    .write()
                    .await
                    .iter_mut()
                    .find_map(|(_, d)| {
                        if ptr::eq(d.device, *device) {
                            d.reference_count += 1;
                            Some(())
                        } else {
                            None
                        }
                    })
                    .ok_or(Error::InvalidIdentifier)?;
            }
            _ => {
                return Err(Error::InvalidIdentifier);
            }
        }

        Ok(())
    }

    pub async fn close(&self, item: &ItemStatic, context: &mut Context) -> Result<()> {
        match item {
            ItemStatic::File(file_system) | ItemStatic::Directory(file_system) => {
                self.file_systems
                    .write()
                    .await
                    .iter_mut()
                    .find_map(|fs| {
                        if ptr::eq(fs.file_system, *file_system) {
                            if fs.reference_count > 0 {
                                fs.reference_count -= 1;
                            }
                            Some(())
                        } else {
                            None
                        }
                    })
                    .ok_or(Error::InvalidIdentifier)?;
            }
            ItemStatic::Pipe(_) => {}
            ItemStatic::BlockDevice(device) => {
                self.block_device
                    .write()
                    .await
                    .iter_mut()
                    .find_map(|(_, d)| {
                        if ptr::eq(d.device, *device) {
                            if d.reference_count > 0 {
                                d.reference_count -= 1;
                            }
                            Some(())
                        } else {
                            None
                        }
                    })
                    .ok_or(Error::InvalidIdentifier)?;
            }
            ItemStatic::CharacterDevice(device) => {
                self.character_device
                    .write()
                    .await
                    .iter_mut()
                    .find_map(|(_, d)| {
                        if ptr::eq(d.device, *device) {
                            if d.reference_count > 0 {
                                d.reference_count -= 1;
                            }
                            Some(())
                        } else {
                            None
                        }
                    })
                    .ok_or(Error::InvalidIdentifier)?;
            }
            _ => {
                return Err(Error::InvalidIdentifier);
            }
        }

        if let Some(operations) = item.as_directory_operations() {
            poll(|| Ok(operations.close(context)?)).await?;
        } else {
            poll(|| {
                Ok(item
                    .as_base_operations()
                    .ok_or(Error::UnsupportedOperation)?
                    .close(context)?)
            })
            .await?;
        }

        Ok(())
    }

    pub async fn set_ownership(
        &self,
        task: TaskIdentifier,
        path: impl AsRef<Path>,
        user: Option<UserIdentifier>,
        group: Option<GroupIdentifier>,
    ) -> Result<()> {
        let path = path.as_ref();
        let file_systems = self.file_systems.read().await; // Get the file systems 

        let (file_system, relative_path, _) =
            Self::get_file_system_from_path(&file_systems, &path)?; // Get the file system identifier and the relative path
        let (_, current_user, _) = self.get_time_user_group(task).await?;
        Self::check_owner(file_system.file_system, relative_path, current_user).await?;

        let mut attributes = Attributes::new();
        if let Some(user) = user {
            attributes = attributes.set_user(user);
        }
        if let Some(group) = group {
            attributes = attributes.set_group(group);
        }
        Self::set_attributes(file_system.file_system, relative_path, &attributes).await
    }

    pub async fn set_permissions(
        &self,
        task: TaskIdentifier,
        path: impl AsRef<Path>,
        permissions: Permissions,
    ) -> Result<()> {
        let path = path.as_ref();
        let file_systems = self.file_systems.read().await; // Get the file systems

        let (file_system, relative_path, _) =
            Self::get_file_system_from_path(&file_systems, &path)?; // Get the file system identifier and the relative path

        let (_, current_user, _) = self.get_time_user_group(task).await?;
        Self::check_owner(file_system.file_system, relative_path, current_user).await?;

        let attributes = Attributes::new().set_permissions(permissions);
        Self::set_attributes(file_system.file_system, relative_path, &attributes).await
    }
}
