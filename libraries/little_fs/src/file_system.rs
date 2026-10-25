use core::ptr::null_mut;

use super::{Configuration, Directory, convert_result, littlefs};
use crate::{
    File,
    attributes::InternalAttributes,
    configuration::{self},
};
use alloc::{boxed::Box, ffi::CString};
use file_system::{
    AttributeFlags, AttributeOperations, Attributes, BaseOperations, Context, DirectBlockDevice,
    DirectoryOperations, Entry, Error, FileOperations, FileSystemOperations, Flags, Kind,
    MountOperations, Path, Position, Result, Size, mount::MutexMountWrapper,
};
use synchronization::{Arc, blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};

// Duplicate contexts share a stable handle rather than copying native C caches.
type SharedContext<T> = Arc<Mutex<CriticalSectionRawMutex, Box<T>>>;

pub struct FileSystem {
    file_system: MutexMountWrapper<CriticalSectionRawMutex, littlefs::lfs_t>,
}

impl FileSystem {
    pub fn new_format(device: &'static dyn DirectBlockDevice, cache_size: usize) -> Result<Self> {
        Self::format(device, cache_size)?;

        Self::new(device, cache_size)
    }

    pub fn get_or_format(
        device: &'static dyn DirectBlockDevice,
        cache_size: usize,
    ) -> Result<Self> {
        match Self::new(device, cache_size) {
            Ok(file_system) => Ok(file_system),
            Err(_) => {
                device.set_position(0, &Position::Start(0))?;

                Self::format(device, cache_size)?;

                Self::new(device, cache_size)
            }
        }
    }

    pub fn new(device: &'static dyn DirectBlockDevice, cache_size: usize) -> Result<Self> {
        let block_size = device.get_block_size().map_err(|_| Error::InputOutput)?;
        let block_count = device.get_block_count().map_err(|_| Error::InputOutput)?;

        let configuration: littlefs::lfs_config = Configuration::new(
            device,
            block_size as _,
            block_count as _,
            cache_size,
            cache_size,
        )
        .ok_or(Error::InvalidParameter)?
        .try_into()
        .map_err(|_| Error::InvalidParameter)?;

        let configuration = Box::new(configuration);

        let mut file_system = littlefs::lfs_t::default();

        unsafe {
            convert_result(littlefs::lfs_mount(
                &mut file_system,
                Box::leak(configuration),
            ))?;

            let result = convert_result(littlefs::lfs_getattr(
                &mut file_system,
                c"/".as_ptr(),
                InternalAttributes::IDENTIFIER,
                null_mut(),
                0,
            ));

            if let Err(Error::NoAttribute) = result {
                // Set root attributes if not present
                let mut internal_attributes = InternalAttributes::new_uninitialized().assume_init();
                internal_attributes.kind = Kind::Directory;

                convert_result(littlefs::lfs_setattr(
                    &mut file_system,
                    c"/".as_ptr(),
                    InternalAttributes::IDENTIFIER,
                    &internal_attributes as *const _ as *const _,
                    size_of::<InternalAttributes>() as u32,
                ))?;
            } else {
                result?;
            }
        }

        Ok(Self {
            file_system: MutexMountWrapper::new_mounted(file_system),
        })
    }

    pub fn format(device: &'static dyn DirectBlockDevice, cache_size: usize) -> Result<()> {
        let block_size = device.get_block_size().map_err(|_| Error::InputOutput)?;
        let block_count = device.get_block_count().map_err(|_| Error::InputOutput)?;

        let configuration: littlefs::lfs_config = Configuration::new(
            device,
            block_size as _,
            block_count as _,
            cache_size,
            cache_size,
        )
        .ok_or(Error::InvalidParameter)?
        .try_into()
        .map_err(|_| Error::InvalidParameter)?;

        let mut file_system = littlefs::lfs_t::default();

        convert_result(unsafe { littlefs::lfs_format(&mut file_system, &configuration) })?;

        Ok(())
    }

    pub fn operation<T>(
        &self,
        operation: impl FnOnce(&mut littlefs::lfs_t) -> Result<T>,
    ) -> Result<T> {
        let mut file_system = self.file_system.try_get()?;

        operation(&mut file_system)
    }

    pub fn operation_with_context<I: 'static, T>(
        &self,
        context: &mut Context,
        operation: impl FnOnce(&mut littlefs::lfs_t, &mut I) -> Result<T>,
    ) -> Result<T> {
        let mut file_system = self.file_system.try_get()?;

        let file = context
            .get_private_data_of_type::<SharedContext<I>>()
            .ok_or(Error::InvalidParameter)?;
        let mut file = file.try_lock().map_err(|_| Error::RessourceBusy)?;
        operation(&mut file_system, &mut file)
    }
}

unsafe impl Send for FileSystem {}
unsafe impl Sync for FileSystem {}

impl MountOperations for FileSystem {
    fn unmount(&self) -> Result<()> {
        self.file_system.unmount()
    }
}

impl FileSystemOperations for FileSystem {
    fn writable_attributes(&self) -> file_system::AttributeFlags {
        InternalAttributes::PERSISTED_ATTRIBUTE_FLAGS
    }

    fn persistent_attributes(&self) -> file_system::AttributeFlags {
        InternalAttributes::PERSISTED_ATTRIBUTE_FLAGS
    }

    fn rename(&self, source: &Path, destination: &Path) -> Result<()> {
        self.operation(|file_system| {
            let source = CString::new(source.as_str()).map_err(|_| Error::InvalidParameter)?;

            let destination =
                CString::new(destination.as_str()).map_err(|_| Error::InvalidParameter)?;

            if source != destination {
                let mut probe = littlefs::lfs_file_t::default();
                let opened = unsafe {
                    littlefs::lfs_file_open(
                        file_system,
                        &mut probe,
                        source.as_ptr(),
                        littlefs::lfs_open_flags_LFS_O_RDONLY as i32,
                    )
                };
                if opened >= 0 {
                    let busy = crate::open_metadata::has_other_open(file_system, &probe);
                    convert_result(unsafe { littlefs::lfs_file_close(file_system, &mut probe) })?;
                    if busy {
                        return Err(Error::RessourceBusy);
                    }
                } else if opened != littlefs::lfs_error_LFS_ERR_ISDIR {
                    convert_result(opened)?;
                }
            }

            convert_result(unsafe {
                littlefs::lfs_rename(file_system, source.as_ptr(), destination.as_ptr())
            })?;

            Ok(())
        })
    }

    fn remove(&self, path: &Path) -> Result<()> {
        let path = CString::new(path.as_str()).map_err(|_| Error::InvalidParameter)?;

        self.operation(|file_system| {
            convert_result(unsafe { littlefs::lfs_remove(file_system, path.as_ptr()) })?;

            Ok(())
        })
    }

    fn create_directory(&self, path: &Path) -> Result<()> {
        self.operation(|file_system| {
            Directory::create(file_system, path)?;

            Ok(())
        })
    }

    fn lookup_directory(&self, context: &mut Context, path: &Path) -> Result<()> {
        self.operation(|file_system| {
            let directory = Directory::lookup(file_system, path)?;
            context.set_private_data(Box::new(Arc::new(
                Mutex::<CriticalSectionRawMutex, _>::new(directory),
            )));
            Ok(())
        })
    }

    fn lookup_file(&self, context: &mut Context, path: &Path, flags: Flags) -> Result<()> {
        self.operation(|file_system| {
            let file = File::lookup(file_system, path, flags)?;
            context.set_private_data(Box::new(Arc::new(
                Mutex::<CriticalSectionRawMutex, _>::new(file),
            )));
            Ok(())
        })
    }

    fn create_file(&self, path: &Path) -> Result<()> {
        self.operation(|file_system| File::create(file_system, path))
    }

    fn get_attributes(&self, path: &Path, attributes: &mut Attributes) -> Result<()> {
        let requested = attributes.get_mask();
        if requested.is_empty() {
            return Ok(());
        }

        self.operation(|file_system| {
            let path = CString::new(path.as_str()).map_err(|_| Error::InvalidParameter)?;

            let mut internal_attributes =
                unsafe { InternalAttributes::new_uninitialized().assume_init() };

            convert_result(unsafe {
                littlefs::lfs_getattr(
                    file_system,
                    path.as_ptr(),
                    InternalAttributes::IDENTIFIER,
                    &mut internal_attributes as *mut _ as *mut _,
                    size_of::<InternalAttributes>() as u32,
                )
            })?;

            internal_attributes.update_attributes(attributes)?;

            if requested.contains(AttributeFlags::Size)
                && matches!(internal_attributes.kind, Kind::File | Kind::SymbolicLink)
            {
                let mut info = littlefs::lfs_info::default();

                convert_result(unsafe {
                    littlefs::lfs_stat(file_system, path.as_ptr(), &mut info)
                })?;

                *attributes = attributes.clone().set_size(info.size as Size);
            }

            Ok(())
        })
    }

    fn set_attributes(&self, path: &Path, attributes: &Attributes) -> Result<()> {
        let mask = attributes.get_mask();
        if mask.is_empty() {
            return Ok(());
        }
        InternalAttributes::validate_attribute_mask(mask)?;

        self.operation(|file_system| {
            let path = CString::new(path.as_str()).map_err(|_| Error::InvalidParameter)?;

            let mut internal_attributes =
                unsafe { InternalAttributes::new_uninitialized().assume_init() };

            if mask != InternalAttributes::PERSISTED_ATTRIBUTE_FLAGS {
                convert_result(unsafe {
                    littlefs::lfs_getattr(
                        file_system,
                        path.as_ptr(),
                        InternalAttributes::IDENTIFIER,
                        &mut internal_attributes as *mut _ as *mut _,
                        size_of::<InternalAttributes>() as u32,
                    )
                })?;
            }

            internal_attributes.update_with_attributes(attributes)?;

            // Pin a native identity through the metadata commit (which may
            // relocate directory blocks). The probe has no configured attrs.
            let mut probe = littlefs::lfs_file_t::default();
            let opened = unsafe {
                littlefs::lfs_file_open(
                    file_system,
                    &mut probe,
                    path.as_ptr(),
                    littlefs::lfs_open_flags_LFS_O_RDONLY as i32,
                )
            };
            if opened < 0 && opened != littlefs::lfs_error_LFS_ERR_ISDIR {
                convert_result(opened)?;
            }
            let result = convert_result(unsafe {
                littlefs::lfs_setattr(
                    file_system,
                    path.as_ptr(),
                    InternalAttributes::IDENTIFIER,
                    &internal_attributes as *const _ as *const _,
                    size_of::<InternalAttributes>() as u32,
                )
            });
            if opened >= 0 {
                if result.is_ok() {
                    crate::open_metadata::synchronize(file_system, &probe, &internal_attributes);
                }
                let close =
                    convert_result(unsafe { littlefs::lfs_file_close(file_system, &mut probe) });
                result?;
                close?;
            } else {
                result?;
            }
            Ok(())
        })
    }
}

impl AttributeOperations for FileSystem {
    fn get_attributes(&self, context: &mut Context, attributes: &mut Attributes) -> Result<()> {
        if attributes.get_mask().is_empty() {
            return Ok(());
        }

        if let Some(file) = context.get_private_data_of_type::<SharedContext<File>>() {
            self.operation(|file_system| {
                file.try_lock()
                    .map_err(|_| Error::RessourceBusy)?
                    .get_attributes(file_system, attributes)
            })
        } else if let Some(directory) =
            context.get_private_data_of_type::<SharedContext<Directory>>()
        {
            directory
                .try_lock()
                .map_err(|_| Error::RessourceBusy)?
                .get_attributes(attributes)
        } else {
            Err(Error::InvalidParameter)
        }
    }

    fn set_attributes(&self, context: &mut Context, attributes: &Attributes) -> Result<()> {
        let mask = attributes.get_mask();
        if mask.is_empty() {
            return Ok(());
        }

        if let Some(file) = context.get_private_data_of_type::<SharedContext<File>>() {
            self.operation(|file_system| {
                file.try_lock()
                    .map_err(|_| Error::RessourceBusy)?
                    .set_attributes(file_system, attributes)
            })
        } else if let Some(directory) =
            context.get_private_data_of_type::<SharedContext<Directory>>()
        {
            directory
                .try_lock()
                .map_err(|_| Error::RessourceBusy)?
                .set_attributes(attributes)
        } else {
            Err(Error::InvalidParameter)
        }
    }
}

impl BaseOperations for FileSystem {
    fn read(
        &self,
        context: &mut Context,
        _cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        absolute_position: Size,
    ) -> core::task::Poll<Result<usize>> {
        file_system::ready(|| {
            self.operation_with_context(context, |file_system, file: &mut File| {
                file.read(file_system, buffer, absolute_position)
            })
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
            self.operation_with_context(context, |file_system, file: &mut File| {
                file.write(file_system, buffer, absolute_position)
            })
        })
    }

    fn set_position(
        &self,
        context: &mut Context,
        current_position: Size,
        position: &Position,
    ) -> Result<Size> {
        self.operation_with_context(context, |file_system, file: &mut File| {
            file.set_position(file_system, current_position, position)
        })
    }

    fn flush(
        &self,
        context: &mut Context,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Result<()>> {
        file_system::ready(|| {
            self.operation_with_context(context, |file_system, file: &mut File| {
                file.flush(file_system)
            })
        })
    }

    fn clone_context(&self, context: &Context) -> Result<Context> {
        if let Some(file) = context.get_private_data_of_type::<SharedContext<File>>() {
            Ok(Context::new(Some(file.clone())))
        } else if let Some(directory) =
            context.get_private_data_of_type::<SharedContext<Directory>>()
        {
            Ok(Context::new(Some(directory.clone())))
        } else {
            Err(Error::InvalidParameter)
        }
    }

    fn close(&self, context: &mut Context) -> Result<()> {
        self.operation(|file_system| {
            let file = context
                .get_private_data_of_type::<SharedContext<File>>()
                .ok_or(Error::InvalidParameter)?;
            if Arc::strong_count(file) == 1 {
                file.try_lock()
                    .map_err(|_| Error::RessourceBusy)?
                    .close(file_system)?;
            }
            context.take_private_data_of_type::<SharedContext<File>>();

            Ok(())
        })
    }
}

impl FileOperations for FileSystem {}

impl DirectoryOperations for FileSystem {
    fn read(&self, context: &mut Context) -> Result<Option<Entry>> {
        self.operation_with_context(context, |file_system, directory: &mut Directory| {
            directory.read(file_system)
        })
    }

    fn get_position(&self, context: &mut file_system::Context) -> Result<Size> {
        self.operation_with_context(context, |file_system, directory: &mut Directory| {
            directory.get_position(file_system)
        })
    }

    fn set_position(&self, context: &mut Context, position: Size) -> Result<()> {
        self.operation_with_context(context, |file_system, directory: &mut Directory| {
            directory.set_position(file_system, position)
        })
    }

    fn rewind(&self, context: &mut Context) -> Result<()> {
        self.operation_with_context(context, |file_system, directory: &mut Directory| {
            directory.rewind(file_system)
        })
    }

    fn close(&self, context: &mut Context) -> Result<()> {
        self.operation(|file_system| {
            let directory = context
                .get_private_data_of_type::<SharedContext<Directory>>()
                .ok_or(Error::InvalidParameter)?;
            if Arc::strong_count(directory) == 1 {
                directory
                    .try_lock()
                    .map_err(|_| Error::RessourceBusy)?
                    .close(file_system)?;
            }
            context.take_private_data_of_type::<SharedContext<Directory>>();

            Ok(())
        })
    }
}

impl Drop for FileSystem {
    fn drop(&mut self) {
        let _ = self.operation(|file_system| {
            unsafe {
                littlefs::lfs_unmount(file_system);
            }

            let mut configuration =
                unsafe { Box::from_raw(file_system.cfg as *mut littlefs::lfs_config) };

            unsafe {
                configuration::Context::take_from_configuration(&mut *configuration);
            }

            Ok(())
        });
    }
}

/// Mount an existing Xila disk without ever formatting it. Validate partition
/// extents before overlap arithmetic, including malicious overflowing entries.
pub fn mount_disk<D: DirectBlockDevice>(
    device: &'static D,
    cache_size: usize,
) -> Result<FileSystem> {
    use file_system::{
        XILA_DISK_SIGNATURE,
        mbr::{Mbr, PartitionKind, create_partition_device},
    };
    let mbr = Mbr::read_from_device(device).map_err(|_| Error::InvalidFile)?;
    let count = device.get_block_count()?;
    for entry in mbr.partitions.iter().filter(|entry| entry.is_valid()) {
        if entry.start_block == 0 || entry.start_block as u64 + entry.block_count as u64 > count {
            return Err(Error::InvalidFile);
        }
    }
    mbr.validate().map_err(|_| Error::InvalidFile)?;
    if mbr.get_disk_signature() != XILA_DISK_SIGNATURE {
        return Err(Error::InvalidFile);
    }
    let entry = mbr
        .partitions
        .iter()
        .find(|entry| entry.is_valid() && entry.kind == PartitionKind::Xila)
        .ok_or(Error::InvalidFile)?;
    let partition = create_partition_device(device, entry).map_err(|_| Error::InvalidFile)?;
    FileSystem::new(Box::leak(Box::new(partition)), cache_size)
}

#[cfg(test)]
mod tests {
    extern crate std;

    use file_system::{
        AttributeFlags, MemoryDevice,
        file_system::tests::{FileSystemTestCapabilities, implement_file_system_tests},
    };

    use super::*;

    const CACHE_SIZE: usize = 256;

    drivers_std::memory::instantiate_global_allocator!();

    fn initialize() -> FileSystem {
        if !log::is_initialized() {
            let _ = log::initialize(&drivers_std::log::Logger);
        }

        let device = Box::leak(Box::new(MemoryDevice::<512>::new(2048 * 512)));

        FileSystem::format(device, CACHE_SIZE).unwrap();

        FileSystem::new(device, CACHE_SIZE).unwrap()
    }

    implement_file_system_tests!(
        initialize(),
        FileSystemTestCapabilities {
            readable: AttributeFlags::all().difference(AttributeFlags::Links),
            writable: AttributeFlags::all()
                .difference(AttributeFlags::Size | AttributeFlags::Links),
        }
    );

    #[test]
    fn littlefs_unsupported_attribute_masks_are_rejected_without_mutation() {
        let fs = initialize();
        let file = Path::new("/unsupported-attributes.txt");
        FileSystemOperations::create_file(&fs, file).unwrap();

        let original_permissions = file_system::Permissions::ALL_READ_WRITE;
        FileSystemOperations::set_attributes(
            &fs,
            file,
            &Attributes::new().set_permissions(original_permissions),
        )
        .unwrap();

        for mask in [AttributeFlags::Size, AttributeFlags::Links] {
            let unsupported = Attributes::new().set_size(12).set_links(2).set_mask(mask);
            assert_eq!(
                FileSystemOperations::set_attributes(&fs, file, &unsupported),
                Err(Error::UnsupportedOperation)
            );
        }

        let mixed = Attributes::new()
            .set_permissions(file_system::Permissions::NONE)
            .set_size(12)
            .set_mask(AttributeFlags::Permissions | AttributeFlags::Size);
        assert_eq!(
            FileSystemOperations::set_attributes(&fs, file, &mixed),
            Err(Error::UnsupportedOperation)
        );

        let mut actual = Attributes::new().set_mask(AttributeFlags::Permissions);
        FileSystemOperations::get_attributes(&fs, file, &mut actual).unwrap();
        assert_eq!(actual.get_permissions(), Some(&original_permissions));

        let mut file_context = Context::new_empty();
        fs.lookup_file(
            &mut file_context,
            file,
            file_system::AccessFlags::Read.into(),
        )
        .unwrap();
        let mut file_attributes = Attributes::new()
            .set_mask(AttributeFlags::Kind | AttributeFlags::Size | AttributeFlags::Links);
        AttributeOperations::get_attributes(&fs, &mut file_context, &mut file_attributes).unwrap();
        assert_eq!(
            file_attributes.get_mask(),
            AttributeFlags::Kind | AttributeFlags::Size
        );
        assert_eq!(file_attributes.get_kind(), Some(&Kind::File));
        assert_eq!(file_attributes.get_size(), Some(&0));
        assert_eq!(
            AttributeOperations::set_attributes(
                &fs,
                &mut file_context,
                &Attributes::new().set_size(8),
            ),
            Err(Error::UnsupportedOperation)
        );
        BaseOperations::close(&fs, &mut file_context).unwrap();

        let directory = Path::new("/unsupported-attributes-dir");
        FileSystemOperations::create_directory(&fs, directory).unwrap();
        let mut path_directory_attributes = Attributes::new()
            .set_mask(AttributeFlags::Kind | AttributeFlags::Size | AttributeFlags::Links);
        FileSystemOperations::get_attributes(&fs, directory, &mut path_directory_attributes)
            .unwrap();
        assert_eq!(path_directory_attributes.get_mask(), AttributeFlags::Kind);

        let mut directory_context = Context::new_empty();
        fs.lookup_directory(&mut directory_context, directory)
            .unwrap();
        let mut directory_attributes = Attributes::new()
            .set_mask(AttributeFlags::Kind | AttributeFlags::Size | AttributeFlags::Links);
        AttributeOperations::get_attributes(&fs, &mut directory_context, &mut directory_attributes)
            .unwrap();
        assert_eq!(directory_attributes.get_mask(), AttributeFlags::Kind);
        assert_eq!(directory_attributes.get_kind(), Some(&Kind::Directory));
        assert_eq!(
            AttributeOperations::set_attributes(
                &fs,
                &mut directory_context,
                &Attributes::new().set_mask(AttributeFlags::Kind),
            ),
            Err(Error::UnsupportedOperation)
        );
        AttributeOperations::set_attributes(&fs, &mut directory_context, &Attributes::new())
            .unwrap();
        DirectoryOperations::close(&fs, &mut directory_context).unwrap();

        assert_eq!(
            FileSystemOperations::set_attributes(
                &fs,
                Path::new("/missing-attributes.txt"),
                &Attributes::new().set_permissions(file_system::Permissions::NONE),
            ),
            Err(Error::NotFound)
        );
    }

    #[test]
    fn littlefs_rejects_unsupported_attribute_all_mask_without_mutation() {
        let fs = initialize();
        let file = Path::new("/all-attributes.txt");
        FileSystemOperations::create_file(&fs, file).unwrap();
        let original = file_system::Permissions::ALL_READ_WRITE;
        FileSystemOperations::set_attributes(
            &fs,
            file,
            &Attributes::new().set_permissions(original),
        )
        .unwrap();

        let unsupported = Attributes::new()
            .set_permissions(file_system::Permissions::NONE)
            .set_inode(999)
            .set_mask(AttributeFlags::all());
        assert_eq!(
            FileSystemOperations::set_attributes(&fs, file, &unsupported),
            Err(Error::UnsupportedOperation)
        );

        let mut after_rejection = Attributes::new().set_mask(AttributeFlags::Permissions);
        FileSystemOperations::get_attributes(&fs, file, &mut after_rejection).unwrap();
        assert_eq!(after_rejection.get_permissions(), Some(&original));
    }

    #[test]
    fn path_ownership_updates_every_open_cache_and_survives_close() {
        let fs = initialize();
        let path = Path::new("/owners");
        FileSystemOperations::create_file(&fs, path).unwrap();
        let mut first = Context::new_empty();
        let mut second = Context::new_empty();
        for context in [&mut first, &mut second] {
            fs.lookup_file(context, path, file_system::AccessFlags::READ_WRITE.into())
                .unwrap();
        }
        let owner = users::UserIdentifier::new(123);
        FileSystemOperations::set_attributes(&fs, path, &Attributes::new().set_user(owner))
            .unwrap();
        for context in [&mut first, &mut second] {
            let mut attributes = Attributes::new().set_mask(AttributeFlags::User);
            AttributeOperations::get_attributes(&fs, context, &mut attributes).unwrap();
            assert_eq!(attributes.get_user(), Some(&owner));
            BaseOperations::close(&fs, context).unwrap();
        }
        let mut attributes = Attributes::new().set_mask(AttributeFlags::User);
        FileSystemOperations::get_attributes(&fs, path, &mut attributes).unwrap();
        assert_eq!(attributes.get_user(), Some(&owner));
    }

    #[test]
    fn ownership_cache_sync_uses_object_identity_after_rename_and_unlink() {
        let fs = initialize();
        let original = Path::new("/old");
        let moved = Path::new("/moved");
        FileSystemOperations::create_file(&fs, original).unwrap();
        let mut open = Context::new_empty();
        fs.lookup_file(
            &mut open,
            original,
            file_system::AccessFlags::READ_WRITE.into(),
        )
        .unwrap();
        // LittleFS detaches live source handles on rename. Reject this
        // unsupported case instead of silently changing their object identity.
        assert_eq!(
            FileSystemOperations::rename(&fs, original, moved),
            Err(Error::RessourceBusy)
        );
        BaseOperations::close(&fs, &mut open).unwrap();
        FileSystemOperations::rename(&fs, original, moved).unwrap();
        fs.lookup_file(
            &mut open,
            moved,
            file_system::AccessFlags::READ_WRITE.into(),
        )
        .unwrap();
        FileSystemOperations::create_file(&fs, original).unwrap();
        let owner = users::UserIdentifier::new(123);
        FileSystemOperations::set_attributes(&fs, moved, &Attributes::new().set_user(owner))
            .unwrap();
        FileSystemOperations::set_attributes(
            &fs,
            original,
            &Attributes::new().set_user(users::UserIdentifier::new(456)),
        )
        .unwrap();
        let mut attributes = Attributes::new().set_mask(AttributeFlags::User);
        AttributeOperations::get_attributes(&fs, &mut open, &mut attributes).unwrap();
        assert_eq!(attributes.get_user(), Some(&owner));
        FileSystemOperations::remove(&fs, moved).unwrap();
        FileSystemOperations::create_file(&fs, moved).unwrap();
        FileSystemOperations::set_attributes(
            &fs,
            moved,
            &Attributes::new().set_user(users::UserIdentifier::new(789)),
        )
        .unwrap();
        AttributeOperations::get_attributes(&fs, &mut open, &mut attributes).unwrap();
        assert_eq!(attributes.get_user(), Some(&owner));
        BaseOperations::close(&fs, &mut open).unwrap();
    }

    #[test]
    fn handle_chmod_after_flush_is_persisted_and_visible_to_other_handles() {
        let fs = initialize();
        let path = Path::new("/chmod");
        FileSystemOperations::create_file(&fs, path).unwrap();
        let mut first = Context::new_empty();
        let mut second = Context::new_empty();
        for context in [&mut first, &mut second] {
            fs.lookup_file(context, path, file_system::AccessFlags::READ_WRITE.into())
                .unwrap();
        }
        BaseOperations::flush_sync(&fs, &mut first).unwrap();
        let permissions = file_system::Permissions::USER_READ_WRITE;
        AttributeOperations::set_attributes(
            &fs,
            &mut first,
            &Attributes::new().set_permissions(permissions),
        )
        .unwrap();
        let mut attributes = Attributes::new().set_mask(AttributeFlags::Permissions);
        AttributeOperations::get_attributes(&fs, &mut second, &mut attributes).unwrap();
        assert_eq!(attributes.get_permissions(), Some(&permissions));
        BaseOperations::close(&fs, &mut second).unwrap();
        BaseOperations::close(&fs, &mut first).unwrap();
        FileSystemOperations::get_attributes(&fs, path, &mut attributes).unwrap();
        assert_eq!(attributes.get_permissions(), Some(&permissions));
    }

    #[test]
    fn stale_handle_chmod_cannot_overwrite_another_opens_data() {
        let fs = initialize();
        let path = Path::new("/stale-chmod");
        FileSystemOperations::create_file(&fs, path).unwrap();
        let mut writer = Context::new_empty();
        let mut stale = Context::new_empty();
        for context in [&mut writer, &mut stale] {
            fs.lookup_file(context, path, file_system::AccessFlags::READ_WRITE.into())
                .unwrap();
        }
        BaseOperations::flush_sync(&fs, &mut stale).unwrap();
        BaseOperations::write_sync(&fs, &mut writer, b"payload", 0).unwrap();
        BaseOperations::flush_sync(&fs, &mut writer).unwrap();
        assert_eq!(
            AttributeOperations::set_attributes(
                &fs,
                &mut stale,
                &Attributes::new().set_permissions(file_system::Permissions::ALL_FULL)
            ),
            Err(Error::RessourceBusy)
        );
        BaseOperations::close(&fs, &mut stale).unwrap();
        BaseOperations::close(&fs, &mut writer).unwrap();
        let mut check = Context::new_empty();
        fs.lookup_file(&mut check, path, file_system::AccessFlags::Read.into())
            .unwrap();
        let mut bytes = [0; 7];
        assert_eq!(
            BaseOperations::read_sync(&fs, &mut check, &mut bytes, 0).unwrap(),
            7
        );
        assert_eq!(&bytes, b"payload");
        BaseOperations::close(&fs, &mut check).unwrap();
    }

    #[test]
    fn filesystem_contents_survive_unmount_and_remount() {
        let device = Box::leak(Box::new(MemoryDevice::<512>::new(2048 * 512)));
        FileSystem::format(device, CACHE_SIZE).unwrap();

        let file_system = FileSystem::new(device, CACHE_SIZE).unwrap();
        let directory = Path::new("/persistent");
        let file = Path::new("/persistent/data");
        FileSystemOperations::create_directory(&file_system, directory).unwrap();
        FileSystemOperations::create_file(&file_system, file).unwrap();

        let mut context = Context::new_empty();
        file_system
            .lookup_file(&mut context, file, file_system::AccessFlags::Write.into())
            .unwrap();
        let expected = b"littlefs remount";
        assert_eq!(
            file_system.write_sync(&mut context, expected, 0).unwrap(),
            expected.len()
        );
        BaseOperations::close(&file_system, &mut context).unwrap();

        drop(file_system);

        let file_system = FileSystem::new(device, CACHE_SIZE).unwrap();
        let mut context = Context::new_empty();
        file_system
            .lookup_file(&mut context, file, file_system::AccessFlags::Read.into())
            .unwrap();
        let mut actual = [0; 16];
        assert_eq!(
            BaseOperations::read_sync(&file_system, &mut context, &mut actual, 0).unwrap(),
            expected.len()
        );
        assert_eq!(&actual, expected);
        BaseOperations::close(&file_system, &mut context).unwrap();
    }
}

#[cfg(test)]
mod symlink_tests {
    use super::*;
    use file_system::{AccessFlags, DirectoryOperations, Path};

    const CACHE_SIZE: usize = 256;

    #[test]
    fn persisted_link_kind_size_and_directory_entry_survive_remount() {
        let device = file_system::MemoryDevice::<512>::new_static(2048 * 512);
        FileSystem::format(device, CACHE_SIZE).unwrap();
        let filesystem = FileSystem::new(device, CACHE_SIZE).unwrap();
        let link = Path::from_str("/link");
        FileSystemOperations::create_file(&filesystem, link).unwrap();

        let target = b"../data/target";
        let mut context = Context::new_empty();
        FileSystemOperations::lookup_file(
            &filesystem,
            &mut context,
            link,
            Flags::from(AccessFlags::Write),
        )
        .unwrap();
        assert_eq!(
            BaseOperations::write_sync(&filesystem, &mut context, target, 0).unwrap(),
            target.len()
        );
        AttributeOperations::set_attributes(
            &filesystem,
            &mut context,
            &Attributes::new().set_kind(Kind::SymbolicLink),
        )
        .unwrap();
        BaseOperations::flush_sync(&filesystem, &mut context).unwrap();
        BaseOperations::close(&filesystem, &mut context).unwrap();

        let mut attributes =
            Attributes::new().set_mask(AttributeFlags::Kind | AttributeFlags::Size);
        FileSystemOperations::get_attributes(&filesystem, link, &mut attributes).unwrap();
        assert_eq!(attributes.get_kind(), Some(&Kind::SymbolicLink));
        assert_eq!(attributes.get_size(), Some(&(target.len() as Size)));

        let mut directory = Context::new_empty();
        filesystem
            .lookup_directory(&mut directory, Path::ROOT)
            .unwrap();
        let mut found = None;
        while let Some(entry) = DirectoryOperations::read(&filesystem, &mut directory).unwrap() {
            if entry.name == "link" {
                found = Some(entry);
                break;
            }
        }
        let found = found.unwrap();
        assert_eq!(found.kind, Kind::SymbolicLink);
        assert_eq!(found.size, target.len() as Size);
        DirectoryOperations::close(&filesystem, &mut directory).unwrap();

        MountOperations::unmount(&filesystem).unwrap();
        drop(filesystem);
        let filesystem = FileSystem::new(device, CACHE_SIZE).unwrap();
        let mut attributes =
            Attributes::new().set_mask(AttributeFlags::Kind | AttributeFlags::Size);
        FileSystemOperations::get_attributes(&filesystem, link, &mut attributes).unwrap();
        assert_eq!(attributes.get_kind(), Some(&Kind::SymbolicLink));
        assert_eq!(attributes.get_size(), Some(&(target.len() as Size)));
    }
}
