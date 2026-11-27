use core::ptr::null_mut;

use super::{Configuration, Directory, convert_result, littlefs};
use crate::{
    File,
    attributes::{self, InternalAttributes},
    configuration::{self},
    constants,
};
use alloc::{boxed::Box, ffi::CString};
use file_system::{
    AttributeFlags, AttributeOperations, Attributes, BaseOperations, Context, DirectBlockDevice,
    DirectoryOperations, Entry, FileOperations, FileSystemOperations, Flags, IoErrorKind, Kind,
    MountOperations, Path, Position, Result, Size, mount::MutexMountWrapper,
};
use synchronization::{Arc, blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};

// Duplicate contexts share a stable handle rather than copying native C caches.
type SharedContext<T> = Arc<Mutex<CriticalSectionRawMutex, Box<T>>>;

/// Mount and format options for [`FileSystem`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// Size of the read and program caches in bytes.
    pub cache_size: usize,
    /// Number of erase cycles before LittleFS evicts a metadata block for wear
    /// levelling, or `None` for the LittleFS default (`-1`, wear levelling
    /// disabled).
    ///
    /// LittleFS recommends values in the range `100`–`1000`. Flash-backed
    /// devices should set this (for example, `Some(500)`), while RAM or
    /// host-file storage can leave it unset.
    pub block_cycles: Option<u16>,
}

impl Options {
    /// Creates options with `cache_size` and `block_cycles` unset (`None`).
    pub const fn new(cache_size: usize) -> Self {
        Self {
            cache_size,
            block_cycles: None,
        }
    }

    /// Sets the read and program cache size in bytes.
    pub const fn set_cache_size(mut self, cache_size: usize) -> Self {
        self.cache_size = cache_size;
        self
    }

    /// Sets the metadata block wear-levelling cycle count.
    pub const fn set_block_cycles(mut self, block_cycles: u16) -> Self {
        self.block_cycles = Some(block_cycles);
        self
    }
}

pub struct FileSystem {
    file_system: MutexMountWrapper<CriticalSectionRawMutex, MountedFileSystem>,
}

struct MountedFileSystem {
    native: littlefs::lfs_t,
    recovery_required: bool,
}

impl MountedFileSystem {
    fn recover(&mut self) -> Result<()> {
        if !self.recovery_required {
            return Ok(());
        }
        if !self.native.mlist.is_null() {
            return Err(IoErrorKind::InputOutput);
        }

        // Mount reads the committed move state from disk. Keep the old mount
        // blocked if recovery fails, so the next operation can try again.
        let mut recovered = littlefs::lfs_t::default();
        unsafe {
            convert_result(littlefs::lfs_mount(&mut recovered, self.native.cfg))?;
            littlefs::lfs_unmount(&mut self.native);
        }
        self.native = recovered;
        self.recovery_required = false;
        Ok(())
    }
}

impl FileSystem {
    fn build_configuration(
        device: &'static dyn DirectBlockDevice,
        options: Options,
    ) -> Result<littlefs::lfs_config> {
        let block_size = device
            .get_block_size()
            .map_err(|_| IoErrorKind::InputOutput)?;
        // LittleFS counts blocks in a 32-bit field. Reject a device with more
        // blocks than it can address instead of truncating the count.
        let block_count = u32::try_from(
            device
                .get_block_count()
                .map_err(|_| IoErrorKind::InputOutput)?,
        )
        .map_err(|_| IoErrorKind::InvalidArgument)?;

        let mut configuration = Configuration::new(
            device,
            block_size,
            block_count,
            options.cache_size,
            options.cache_size,
        )
        .ok_or(IoErrorKind::InvalidArgument)?;

        if let Some(block_cycles) = options.block_cycles {
            configuration = configuration.set_block_cycles(block_cycles);
        }

        configuration
            .try_into()
            .map_err(|_| IoErrorKind::InvalidArgument)
    }

    pub fn new_format(device: &'static dyn DirectBlockDevice, options: Options) -> Result<Self> {
        Self::format(device, options)?;

        Self::new(device, options)
    }

    pub fn get_or_format(device: &'static dyn DirectBlockDevice, options: Options) -> Result<Self> {
        match Self::new(device, options) {
            Ok(file_system) => Ok(file_system),
            Err(_) => {
                device.set_position(0, &Position::Start(0))?;

                Self::format(device, options)?;

                Self::new(device, options)
            }
        }
    }

    pub fn new(device: &'static dyn DirectBlockDevice, options: Options) -> Result<Self> {
        let configuration = Self::build_configuration(device, options)?;

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

            if let Err(IoErrorKind::MissingAttribute) = result {
                // Set root attributes if not present
                let mut internal_attributes = InternalAttributes::new();
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
            file_system: MutexMountWrapper::new_mounted(MountedFileSystem {
                native: file_system,
                recovery_required: false,
            }),
        })
    }

    pub fn format(device: &'static dyn DirectBlockDevice, options: Options) -> Result<()> {
        let configuration = Self::build_configuration(device, options)?;

        let mut file_system = littlefs::lfs_t::default();

        convert_result(unsafe { littlefs::lfs_format(&mut file_system, &configuration) })?;

        Ok(())
    }

    pub fn operation<T>(
        &self,
        operation: impl FnOnce(&mut littlefs::lfs_t) -> Result<T>,
    ) -> Result<T> {
        self.operation_with_mount(|mount| operation(&mut mount.native))
    }

    fn operation_with_mount<T>(
        &self,
        operation: impl FnOnce(&mut MountedFileSystem) -> Result<T>,
    ) -> Result<T> {
        let mut mount = self.file_system.try_get()?;
        mount.recover()?;
        let result = operation(&mut mount);
        Self::drop_read_cache_on_error(&mut mount.native, &result);
        result
    }

    fn drop_read_cache_on_error<T>(file_system: &mut littlefs::lfs_t, result: &Result<T>) {
        // A failed device read can leave stale bytes under the failed block's
        // cache key. Invalidate the cache before another operation uses it.
        if matches!(
            result,
            Err(IoErrorKind::InputOutput | IoErrorKind::CorruptData)
        ) {
            attributes::drop_read_cache(file_system);
        }
    }

    pub fn operation_with_context<I: 'static, T>(
        &self,
        context: &mut Context,
        operation: impl FnOnce(&mut littlefs::lfs_t, &mut I) -> Result<T>,
    ) -> Result<T> {
        self.operation(|file_system| {
            let file = context
                .get_private_data_of_type::<SharedContext<I>>()
                .ok_or(IoErrorKind::InvalidArgument)?;
            let mut file = file.try_lock().map_err(|_| IoErrorKind::WouldBlock)?;
            operation(file_system, &mut file)
        })
    }
}

unsafe impl Send for FileSystem {}
unsafe impl Sync for FileSystem {}

impl MountOperations for FileSystem {
    /// Without open handles, sync the backing device, then `lfs_unmount` and
    /// release the configuration; a failed sync leaves the filesystem mounted.
    /// With open handles (a forced detach), only stop serving: their caches
    /// stay allocated and their unsynchronized data is not committed.
    fn unmount(&self) -> Result<()> {
        let mut mount = self.file_system.try_get()?;
        let file_system = &mut mount.native;
        if file_system.mlist.is_null() {
            // Directory updates are committed immediately; `lfs_unmount` only
            // frees caches, so the backing device is synced explicitly first.
            if unsafe { super::callbacks::flush_callback(file_system.cfg) } != 0 {
                return Err(IoErrorKind::InputOutput);
            }
            convert_result(unsafe { littlefs::lfs_unmount(file_system) })?;
            let mut configuration =
                unsafe { Box::from_raw(file_system.cfg as *mut littlefs::lfs_config) };
            unsafe {
                configuration::Context::take_from_configuration(&mut *configuration);
            }
        }
        drop(mount);
        self.file_system.unmount()
    }
}

impl FileSystemOperations for FileSystem {
    fn supports_confined_namespace(&self) -> bool {
        true
    }

    fn writable_attributes(&self) -> file_system::AttributeFlags {
        InternalAttributes::PERSISTED_ATTRIBUTE_FLAGS
    }

    fn persistent_attributes(&self) -> file_system::AttributeFlags {
        InternalAttributes::PERSISTED_ATTRIBUTE_FLAGS
    }

    fn rename(&self, source: &Path, destination: &Path) -> Result<()> {
        self.operation_with_mount(|mount| {
            let file_system = &mut mount.native;
            let source = CString::new(source.as_str()).map_err(|_| IoErrorKind::InvalidArgument)?;

            let destination =
                CString::new(destination.as_str()).map_err(|_| IoErrorKind::InvalidArgument)?;

            if source != destination {
                let mut probe = littlefs::lfs_file_t::default();
                let opened = unsafe {
                    littlefs::lfs_file_open(
                        file_system,
                        &mut probe,
                        source.as_ptr(),
                        constants::OPEN_READ_ONLY,
                    )
                };
                if opened >= 0 {
                    let busy = crate::open_metadata::has_other_open(file_system, &probe);
                    convert_result(unsafe { littlefs::lfs_file_close(file_system, &mut probe) })?;
                    if busy {
                        return Err(IoErrorKind::ResourceBusy);
                    }
                } else if opened != littlefs::lfs_error_LFS_ERR_ISDIR {
                    convert_result(opened)?;
                }
            }

            let result = convert_result(unsafe {
                littlefs::lfs_rename(file_system, source.as_ptr(), destination.as_ptr())
            });
            if matches!(
                result,
                Err(IoErrorKind::InputOutput | IoErrorKind::CorruptData | IoErrorKind::NoSpace)
            ) {
                // A failed commit may already be on disk. Before any further
                // operation, reload the move state. See littlefs-project/littlefs#1229.
                mount.recovery_required = true;
            }
            result.map(|_| ())
        })
    }

    fn remove(&self, path: &Path) -> Result<()> {
        let path = CString::new(path.as_str()).map_err(|_| IoErrorKind::InvalidArgument)?;

        self.operation(|file_system| {
            convert_result(unsafe { littlefs::lfs_remove(file_system, path.as_ptr()) })?;

            Ok(())
        })
    }

    fn create_directory(&self, path: &Path, attributes: &Attributes) -> Result<()> {
        self.operation(|file_system| {
            Directory::create(file_system, path, attributes)?;

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

    fn create_file(&self, path: &Path, attributes: &Attributes) -> Result<()> {
        self.operation(|file_system| File::create(file_system, path, attributes))
    }

    fn get_attributes(&self, path: &Path, attributes: &mut Attributes) -> Result<()> {
        let requested = attributes.get_mask();
        if requested.is_empty() {
            return Ok(());
        }

        self.operation(|file_system| {
            let path = CString::new(path.as_str()).map_err(|_| IoErrorKind::InvalidArgument)?;

            let mut internal_attributes = InternalAttributes::new();

            let result = unsafe {
                littlefs::lfs_getattr(
                    file_system,
                    path.as_ptr(),
                    InternalAttributes::IDENTIFIER,
                    &mut internal_attributes as *mut _ as *mut _,
                    size_of::<InternalAttributes>() as u32,
                )
            };
            match convert_result(result) {
                Ok(_) => {
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
                }
                // A power loss during creation can leave an entry without
                // attributes. The name still exists: report the fields
                // `lfs_stat` can provide (kind, size, inode 0) so it can be
                // inspected and removed. Owner, permissions and times stay
                // unavailable: requests for them fail closed with
                // `MissingAttribute` instead of reporting root with no rights.
                Err(IoErrorKind::MissingAttribute) => {
                    const STAT_FIELDS: AttributeFlags = AttributeFlags::Kind
                        .union(AttributeFlags::Inode)
                        .union(AttributeFlags::Size);
                    // Only requests fully answerable from `lfs_stat` fall back;
                    // anything asking for stored metadata (owner, permissions,
                    // times) still fails closed with `MissingAttribute`.
                    if !requested.difference(STAT_FIELDS).is_empty() {
                        return Err(IoErrorKind::MissingAttribute);
                    }
                    let stat_only = requested;
                    let mut info = littlefs::lfs_info::default();
                    convert_result(unsafe {
                        littlefs::lfs_stat(file_system, path.as_ptr(), &mut info)
                    })?;
                    *attributes = attributes.clone().set_mask(stat_only);
                    if stat_only.contains(AttributeFlags::Kind) {
                        let kind = if info.type_ == constants::TYPE_DIRECTORY {
                            Kind::Directory
                        } else {
                            Kind::File
                        };
                        *attributes = attributes.clone().set_kind(kind);
                    }
                    if stat_only.contains(AttributeFlags::Inode) {
                        *attributes = attributes.clone().set_inode(0);
                    }
                    if stat_only.contains(AttributeFlags::Size)
                        && info.type_ == constants::TYPE_REGULAR
                    {
                        *attributes = attributes.clone().set_size(info.size as Size);
                    }
                }
                Err(error) => return Err(error),
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
            let path = CString::new(path.as_str()).map_err(|_| IoErrorKind::InvalidArgument)?;

            let mut internal_attributes = InternalAttributes::new();

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
                    constants::OPEN_READ_ONLY,
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
                    .map_err(|_| IoErrorKind::WouldBlock)?
                    .get_attributes(file_system, attributes)
            })
        } else if let Some(directory) =
            context.get_private_data_of_type::<SharedContext<Directory>>()
        {
            directory
                .try_lock()
                .map_err(|_| IoErrorKind::WouldBlock)?
                .get_attributes(attributes)
        } else {
            Err(IoErrorKind::InvalidArgument)
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
                    .map_err(|_| IoErrorKind::WouldBlock)?
                    .set_attributes(file_system, attributes)
            })
        } else if let Some(directory) =
            context.get_private_data_of_type::<SharedContext<Directory>>()
        {
            directory
                .try_lock()
                .map_err(|_| IoErrorKind::WouldBlock)?
                .set_attributes(attributes)
        } else {
            Err(IoErrorKind::InvalidArgument)
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
            Err(IoErrorKind::InvalidArgument)
        }
    }

    fn close(&self, context: &mut Context) -> Result<()> {
        let mut mount = self.file_system.try_get()?;
        let file = context
            .get_private_data_of_type::<SharedContext<File>>()
            .ok_or(IoErrorKind::InvalidArgument)?;
        let mut result = Ok(());
        if Arc::strong_count(file) == 1 {
            let mut file = file.try_lock().map_err(|_| IoErrorKind::WouldBlock)?;
            result = if mount.recovery_required {
                file.close_without_sync(&mut mount.native)
            } else {
                file.close(&mut mount.native)
            };
            Self::drop_read_cache_on_error(&mut mount.native, &result);
        }
        context.take_private_data_of_type::<SharedContext<File>>();
        result
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
        let mut mount = self.file_system.try_get()?;
        let directory = context
            .get_private_data_of_type::<SharedContext<Directory>>()
            .ok_or(IoErrorKind::InvalidArgument)?;
        if Arc::strong_count(directory) == 1 {
            directory
                .try_lock()
                .map_err(|_| IoErrorKind::WouldBlock)?
                .close(&mut mount.native)?;
        }
        context.take_private_data_of_type::<SharedContext<Directory>>();

        Ok(())
    }
}

impl Drop for FileSystem {
    fn drop(&mut self) {
        if let Ok(mut mount) = self.file_system.try_get() {
            let file_system = &mut mount.native;
            unsafe {
                littlefs::lfs_unmount(file_system);

                let mut configuration = Box::from_raw(file_system.cfg as *mut littlefs::lfs_config);
                configuration::Context::take_from_configuration(&mut *configuration);
            }
        }
    }
}

/// Mount an existing Xila disk without ever formatting it. Validate partition
/// extents before overlap arithmetic, including malicious overflowing entries.
pub fn mount_disk<D: DirectBlockDevice>(
    device: &'static D,
    options: Options,
) -> Result<FileSystem> {
    use file_system::{
        XILA_DISK_SIGNATURE,
        mbr::{Mbr, PartitionKind, create_partition_device},
    };
    let mbr = Mbr::read_from_device(device).map_err(|_| IoErrorKind::CorruptData)?;
    let count = device.get_block_count()?;
    for entry in mbr.partitions.iter().filter(|entry| entry.is_valid()) {
        if entry.start_block == 0 || entry.start_block as u64 + entry.block_count as u64 > count {
            return Err(IoErrorKind::CorruptData);
        }
    }
    mbr.validate().map_err(|_| IoErrorKind::CorruptData)?;
    if mbr.get_disk_signature() != XILA_DISK_SIGNATURE {
        return Err(IoErrorKind::CorruptData);
    }
    let entry = mbr
        .partitions
        .iter()
        .find(|entry| entry.is_valid() && entry.kind == PartitionKind::Xila)
        .ok_or(IoErrorKind::CorruptData)?;
    let partition = create_partition_device(device, entry).map_err(|_| IoErrorKind::CorruptData)?;
    FileSystem::new(Box::leak(Box::new(partition)), options)
}

#[cfg(test)]
mod tests {
    extern crate std;

    use file_system::{
        AttributeFlags, MemoryDevice, PartitionDevice,
        file_system::tests::{FileSystemTestCapabilities, implement_file_system_tests},
    };

    use super::*;

    const CACHE_SIZE: usize = 256;

    testing_support::instantiate_global_allocator!();

    fn initialize() -> FileSystem {
        if !log::is_initialized() {
            let _ = log::initialize(&testing_support::Logger);
        }

        let device = Box::leak(Box::new(MemoryDevice::<512>::new(2048 * 512)));

        let options = Options::new(CACHE_SIZE);
        FileSystem::format(device, options).unwrap();

        FileSystem::new(device, options).unwrap()
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
        FileSystemOperations::create_file(&fs, file, &Attributes::new()).unwrap();

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
                Err(IoErrorKind::Unsupported)
            );
        }

        let mixed = Attributes::new()
            .set_permissions(file_system::Permissions::NONE)
            .set_size(12)
            .set_mask(AttributeFlags::Permissions | AttributeFlags::Size);
        assert_eq!(
            FileSystemOperations::set_attributes(&fs, file, &mixed),
            Err(IoErrorKind::Unsupported)
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
            Err(IoErrorKind::Unsupported)
        );
        BaseOperations::close(&fs, &mut file_context).unwrap();

        let directory = Path::new("/unsupported-attributes-dir");
        FileSystemOperations::create_directory(&fs, directory, &Attributes::new()).unwrap();
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
            Err(IoErrorKind::Unsupported)
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
            Err(IoErrorKind::NotFound)
        );
    }

    #[test]
    fn littlefs_rejects_unsupported_attribute_all_mask_without_mutation() {
        let fs = initialize();
        let file = Path::new("/all-attributes.txt");
        FileSystemOperations::create_file(&fs, file, &Attributes::new()).unwrap();
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
            Err(IoErrorKind::Unsupported)
        );

        let mut after_rejection = Attributes::new().set_mask(AttributeFlags::Permissions);
        FileSystemOperations::get_attributes(&fs, file, &mut after_rejection).unwrap();
        assert_eq!(after_rejection.get_permissions(), Some(&original));
    }

    #[test]
    fn path_ownership_updates_every_open_cache_and_survives_close() {
        let fs = initialize();
        let path = Path::new("/owners");
        FileSystemOperations::create_file(&fs, path, &Attributes::new()).unwrap();
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
        FileSystemOperations::create_file(&fs, original, &Attributes::new()).unwrap();
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
            Err(IoErrorKind::ResourceBusy)
        );
        BaseOperations::close(&fs, &mut open).unwrap();
        FileSystemOperations::rename(&fs, original, moved).unwrap();
        fs.lookup_file(
            &mut open,
            moved,
            file_system::AccessFlags::READ_WRITE.into(),
        )
        .unwrap();
        FileSystemOperations::create_file(&fs, original, &Attributes::new()).unwrap();
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
        FileSystemOperations::create_file(&fs, moved, &Attributes::new()).unwrap();
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
        FileSystemOperations::create_file(&fs, path, &Attributes::new()).unwrap();
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
        FileSystemOperations::create_file(&fs, path, &Attributes::new()).unwrap();
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
            Err(IoErrorKind::ResourceBusy)
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
        let options = Options::new(CACHE_SIZE);
        FileSystem::format(device, options).unwrap();

        let file_system = FileSystem::new(device, options).unwrap();
        let directory = Path::new("/persistent");
        let file = Path::new("/persistent/data");
        FileSystemOperations::create_directory(&file_system, directory, &Attributes::new())
            .unwrap();
        FileSystemOperations::create_file(&file_system, file, &Attributes::new()).unwrap();

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

        let file_system = FileSystem::new(device, options).unwrap();
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

    #[test]
    fn special_permission_bits_survive_unmount_and_remount() {
        let device = Box::leak(Box::new(MemoryDevice::<512>::new(2048 * 512)));
        FileSystem::format(device, Options::new(CACHE_SIZE)).unwrap();

        let file_system = FileSystem::new(device, Options::new(CACHE_SIZE)).unwrap();
        let directory = Path::new("/shared");
        FileSystemOperations::create_directory(&file_system, directory, &Attributes::new())
            .unwrap();
        FileSystemOperations::set_attributes(
            &file_system,
            directory,
            &Attributes::new().set_permissions(file_system::Permissions::SHARED_DIRECTORY),
        )
        .unwrap();
        MountOperations::unmount(&file_system).unwrap();
        drop(file_system);

        let file_system = FileSystem::new(device, Options::new(CACHE_SIZE)).unwrap();
        let mut attributes = Attributes::new().set_mask(AttributeFlags::Permissions);
        FileSystemOperations::get_attributes(&file_system, directory, &mut attributes).unwrap();
        assert_eq!(
            attributes.get_permissions(),
            Some(&file_system::Permissions::SHARED_DIRECTORY)
        );
        assert_eq!(
            attributes.get_permissions().unwrap().get_special(),
            file_system::Special::STICKY
        );
    }

    #[test]
    fn options_block_cycles_reaches_c_configuration() {
        let device = Box::leak(Box::new(MemoryDevice::<512>::new(2048 * 512)));
        let options = Options::new(CACHE_SIZE).set_block_cycles(500);
        let file_system = FileSystem::new_format(device, options).unwrap();

        let block_cycles = file_system
            .operation(|native| Ok(unsafe { (*native.cfg).block_cycles }))
            .unwrap();
        assert_eq!(block_cycles, 500);
    }

    #[test]
    fn options_none_block_cycles_maps_to_minus_one() {
        let device = Box::leak(Box::new(MemoryDevice::<512>::new(2048 * 512)));
        let options = Options::new(CACHE_SIZE);
        assert_eq!(options.block_cycles, None);
        let file_system = FileSystem::new_format(device, options).unwrap();

        let block_cycles = file_system
            .operation(|native| Ok(unsafe { (*native.cfg).block_cycles }))
            .unwrap();
        assert_eq!(block_cycles, -1);
    }

    #[test]
    fn block_counts_above_32_bits_are_rejected() {
        let base = MemoryDevice::<512>::new_static(512 * 128);
        // A partition can claim more blocks than LittleFS can address without
        // backing them with storage.
        let device: &'static dyn DirectBlockDevice = Box::leak(Box::new(PartitionDevice::new(
            base,
            0,
            (1u64 << 32) + 16,
            512,
        )));
        let options = Options::new(CACHE_SIZE);

        assert_eq!(
            FileSystem::new(device, options).err(),
            Some(IoErrorKind::InvalidArgument)
        );
        assert_eq!(
            FileSystem::format(device, options).err(),
            Some(IoErrorKind::InvalidArgument)
        );
        assert_eq!(
            FileSystem::new_format(device, options).err(),
            Some(IoErrorKind::InvalidArgument)
        );
    }

    fn find_entry(fs: &FileSystem, directory_path: &Path, name: &str) -> Entry {
        let mut directory = Context::new_empty();
        fs.lookup_directory(&mut directory, directory_path).unwrap();
        let mut found = None;
        while let Some(entry) = DirectoryOperations::read(fs, &mut directory).unwrap() {
            if entry.name == name {
                found = Some(entry);
                break;
            }
        }
        DirectoryOperations::close(fs, &mut directory).unwrap();
        found.unwrap_or_else(|| panic!("entry {name:?} not found in {directory_path:?}"))
    }

    #[test]
    fn directory_listing_reports_file_inode_and_modification_time() {
        let fs = initialize();
        let path = Path::new("/note.txt");
        FileSystemOperations::create_file(&fs, path, &Attributes::new()).unwrap();

        let payload = b"hello littlefs";
        let mut file = Context::new_empty();
        fs.lookup_file(&mut file, path, file_system::AccessFlags::Write.into())
            .unwrap();
        assert_eq!(
            BaseOperations::write_sync(&fs, &mut file, payload, 0).unwrap(),
            payload.len()
        );
        BaseOperations::flush_sync(&fs, &mut file).unwrap();
        BaseOperations::close(&fs, &mut file).unwrap();

        let initial_mtime = file_system::Time::new(1_700_000_100);
        FileSystemOperations::set_attributes(
            &fs,
            path,
            &Attributes::new()
                .set_inode(42)
                .set_modification(initial_mtime),
        )
        .unwrap();

        let entry = find_entry(&fs, Path::ROOT, "note.txt");
        assert_eq!(entry.inode, 42);
        assert_eq!(entry.kind, Kind::File);
        assert_eq!(entry.size, payload.len() as Size);
        assert_eq!(entry.modification, initial_mtime);

        let updated_mtime = file_system::Time::new(1_700_000_500);
        FileSystemOperations::set_attributes(
            &fs,
            path,
            &Attributes::new().set_modification(updated_mtime),
        )
        .unwrap();

        let updated = find_entry(&fs, Path::ROOT, "note.txt");
        assert_eq!(updated.inode, 42);
        assert_eq!(updated.kind, Kind::File);
        assert_eq!(updated.size, payload.len() as Size);
        assert_eq!(updated.modification, updated_mtime);
    }

    #[test]
    fn directory_listing_reports_subdirectory_inode_and_modification_time() {
        let fs = initialize();
        let root_mtime = file_system::Time::new(1_700_000_000);
        FileSystemOperations::set_attributes(
            &fs,
            Path::ROOT,
            &Attributes::new().set_inode(1).set_modification(root_mtime),
        )
        .unwrap();

        let subdir = Path::new("/folder");
        FileSystemOperations::create_directory(&fs, subdir, &Attributes::new()).unwrap();

        let initial_mtime = file_system::Time::new(1_700_000_200);
        FileSystemOperations::set_attributes(
            &fs,
            subdir,
            &Attributes::new()
                .set_inode(77)
                .set_modification(initial_mtime),
        )
        .unwrap();

        let entry = find_entry(&fs, Path::ROOT, "folder");
        assert_eq!(entry.inode, 77);
        assert_eq!(entry.kind, Kind::Directory);
        assert_eq!(entry.modification, initial_mtime);

        let updated_mtime = file_system::Time::new(1_700_000_900);
        FileSystemOperations::set_attributes(
            &fs,
            subdir,
            &Attributes::new().set_modification(updated_mtime),
        )
        .unwrap();

        let updated = find_entry(&fs, Path::ROOT, "folder");
        assert_eq!(updated.inode, 77);
        assert_eq!(updated.kind, Kind::Directory);
        assert_eq!(updated.modification, updated_mtime);

        let dot = find_entry(&fs, subdir, ".");
        assert_eq!(dot.inode, 77);
        assert_eq!(dot.kind, Kind::Directory);
        assert_eq!(dot.modification, updated_mtime);

        let dot_dot = find_entry(&fs, subdir, "..");
        assert_eq!(dot_dot.inode, 1);
        assert_eq!(dot_dot.kind, Kind::Directory);
        assert_eq!(dot_dot.modification, root_mtime);
    }

    #[test]
    fn directory_listing_falls_back_to_zero_when_attributes_are_missing() {
        let fs = initialize();
        fs.operation(|native| {
            let mut raw_file = littlefs::lfs_file_t::default();
            unsafe {
                convert_result(littlefs::lfs_file_open(
                    native,
                    &mut raw_file,
                    c"/external-file".as_ptr(),
                    constants::OPEN_CREATE | constants::OPEN_WRITE_ONLY,
                ))?;
                convert_result(littlefs::lfs_file_close(native, &mut raw_file))?;
                convert_result(littlefs::lfs_mkdir(native, c"/external-dir".as_ptr()))?;
            }
            Ok(())
        })
        .unwrap();

        let file_entry = find_entry(&fs, Path::ROOT, "external-file");
        assert_eq!(file_entry.inode, 0);
        assert_eq!(file_entry.kind, Kind::File);
        assert_eq!(file_entry.modification, file_system::Time::new(0));

        let dir_entry = find_entry(&fs, Path::ROOT, "external-dir");
        assert_eq!(dir_entry.inode, 0);
        assert_eq!(dir_entry.kind, Kind::Directory);
        assert_eq!(dir_entry.modification, file_system::Time::new(0));
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
        let options = Options::new(CACHE_SIZE);
        FileSystem::format(device, options).unwrap();
        let filesystem = FileSystem::new(device, options).unwrap();
        let link = Path::from_str("/link");
        FileSystemOperations::create_file(&filesystem, link, &Attributes::new()).unwrap();

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
        let filesystem = FileSystem::new(device, options).unwrap();
        let mut attributes =
            Attributes::new().set_mask(AttributeFlags::Kind | AttributeFlags::Size);
        FileSystemOperations::get_attributes(&filesystem, link, &mut attributes).unwrap();
        assert_eq!(attributes.get_kind(), Some(&Kind::SymbolicLink));
        assert_eq!(attributes.get_size(), Some(&(target.len() as Size)));
    }
}
