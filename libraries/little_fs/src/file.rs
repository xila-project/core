use super::{convert_flags, convert_result, littlefs};
use crate::attributes::InternalAttributes;
use crate::constants;
use alloc::{boxed::Box, ffi::CString};
use core::ffi::c_void;
use file_system::{AttributeFlags, Attributes, Flags, IoErrorKind, Path, Position, Result, Size};

// LittleFS uses signed 32-bit offsets. Reject what it cannot represent
// instead of truncating, as a wrapped offset would read or overwrite the
// wrong bytes.
fn convert_position(position: &Position) -> Result<(i32, i32)> {
    let offset = match position {
        Position::Start(position) => i32::try_from(*position),
        Position::Current(position) => i32::try_from(*position),
        Position::End(position) => i32::try_from(*position),
    }
    .map_err(|_| IoErrorKind::FileTooLarge)?;
    let whence = match position {
        Position::Start(_) => constants::SEEK_START,
        Position::Current(_) => constants::SEEK_CURRENT,
        Position::End(_) => constants::SEEK_END,
    };
    Ok((offset, whence))
}

// One LittleFS call accepts at most i32::MAX bytes, as its byte count is a
// signed 32-bit value.
fn convert_length(length: usize) -> u32 {
    length.min(i32::MAX as usize) as u32
}

pub struct File {
    internal_attributes: InternalAttributes,
    attributes: littlefs::lfs_attr,
    configuration: littlefs::lfs_file_config,
    file: littlefs::lfs_file_t,
}

unsafe impl Send for File {}
unsafe impl Sync for File {}

impl File {
    pub fn lookup(
        file_system: &mut littlefs::lfs_t,
        path: &Path,
        flags: Flags,
    ) -> Result<Box<Self>> {
        let path = CString::new(path.as_str()).map_err(|_| IoErrorKind::InvalidArgument)?;

        let little_fs_flags = convert_flags(flags);

        let file = Self {
            internal_attributes: unsafe { InternalAttributes::new_uninitialized().assume_init() },
            file: littlefs::lfs_file_t::default(),
            attributes: littlefs::lfs_attr::default(),
            configuration: littlefs::lfs_file_config::default(),
        };

        let mut file = Box::new(file);

        file.attributes.type_ = InternalAttributes::IDENTIFIER;
        file.attributes.buffer = &mut file.internal_attributes as *mut _ as *mut c_void;
        file.attributes.size = core::mem::size_of::<InternalAttributes>() as u32;

        file.configuration.attr_count = 1;
        file.configuration.attrs = &mut file.attributes;

        unsafe {
            convert_result(littlefs::lfs_file_opencfg(
                file_system,
                &mut file.file,
                path.as_ptr(),
                little_fs_flags,
                &file.configuration,
            ))?;
        }

        Ok(file)
    }

    pub fn create(file_system: &mut littlefs::lfs_t, path: &Path) -> Result<()> {
        let path = CString::new(path.as_str()).map_err(|_| IoErrorKind::InvalidArgument)?;

        let mut file = littlefs::lfs_file_t::default();

        unsafe {
            convert_result(littlefs::lfs_file_open(
                file_system,
                &mut file,
                path.as_ptr(),
                constants::OPEN_CREATE | constants::OPEN_EXCLUSIVE,
            ))?;

            convert_result(littlefs::lfs_file_close(file_system, &mut file))?;

            let internal_attributes = InternalAttributes::new_uninitialized().assume_init();

            convert_result(littlefs::lfs_setattr(
                file_system,
                path.as_ptr(),
                InternalAttributes::IDENTIFIER,
                &internal_attributes as *const _ as *const _,
                size_of::<InternalAttributes>() as u32,
            ))?;
        }
        Ok(())
    }

    pub fn get_attributes(
        &mut self,
        file_system: &mut littlefs::lfs_t,
        attributes: &mut Attributes,
    ) -> Result<()> {
        let requested = attributes.get_mask();
        self.internal_attributes.update_attributes(attributes)?;

        if requested.contains(AttributeFlags::Size) {
            let size =
                convert_result(unsafe { littlefs::lfs_file_size(file_system, &mut self.file) })?;
            *attributes = attributes.clone().set_size(size as Size);
        }

        Ok(())
    }

    pub fn set_attributes(
        &mut self,
        file_system: &mut littlefs::lfs_t,
        attributes: &Attributes,
    ) -> Result<()> {
        let mask = attributes.get_mask();
        if mask.is_empty() {
            return Ok(());
        }
        InternalAttributes::validate_attribute_mask(mask.difference(AttributeFlags::Size))?;

        // Read-only opens cannot persist configured attributes. Never report a
        // successful chmod that disappears when the descriptor closes.
        if self.file.flags & constants::FILE_ACCESS_MASK == constants::FILE_READ_ONLY {
            return Err(IoErrorKind::Unsupported);
        }
        // sync skips errored files and commits data together with attributes.
        // A dusty independent open has stale data; callers must reopen it.
        if self.file.flags & constants::FILE_ERRORED != 0 {
            return Err(IoErrorKind::InputOutput);
        }
        if self.file.flags & constants::FILE_DUSTY != 0 {
            return Err(IoErrorKind::ResourceBusy);
        }
        if let Some(size) = attributes.get_size() {
            // LittleFS uses signed 32-bit offsets and limits file sizes accordingly.
            let size = i32::try_from(*size).map_err(|_| IoErrorKind::FileTooLarge)?;
            convert_result(unsafe {
                littlefs::lfs_file_truncate(file_system, &mut self.file, size as u32)
            })?;
        }
        let previous = self.internal_attributes.clone();
        self.internal_attributes
            .update_with_attributes(attributes)?;
        self.file.flags |= constants::FILE_DIRTY;
        if let Err(error) =
            convert_result(unsafe { littlefs::lfs_file_sync(file_system, &mut self.file) })
        {
            self.internal_attributes = previous;
            return Err(error);
        }
        // Publish only after a successful commit. A failed commit must not
        // make independent contexts report a metadata change as successful.
        let current = self.internal_attributes.clone();
        crate::open_metadata::synchronize(file_system, &self.file, &current);
        Ok(())
    }

    pub fn close(&mut self, file_system: &mut littlefs::lfs_t) -> Result<()> {
        unsafe {
            convert_result(littlefs::lfs_file_close(file_system, &mut self.file))?;
        }

        Ok(())
    }

    pub fn read(
        &mut self,
        file_system: &mut littlefs::lfs_t,
        buffer: &mut [u8],
        absolute_position: Size,
    ) -> Result<usize> {
        let position = i32::try_from(absolute_position).map_err(|_| IoErrorKind::FileTooLarge)?;
        let bytes_read = unsafe {
            convert_result(littlefs::lfs_file_seek(
                file_system,
                &mut self.file,
                position,
                constants::SEEK_START,
            ))?;

            convert_result(littlefs::lfs_file_read(
                file_system,
                &mut self.file,
                buffer.as_mut_ptr() as *mut _,
                convert_length(buffer.len()),
            ))?
        };

        Ok(bytes_read as _)
    }

    pub fn write(
        &mut self,
        file_system: &mut littlefs::lfs_t,
        buffer: &[u8],
        absolute_position: Size,
    ) -> Result<usize> {
        let position = i32::try_from(absolute_position).map_err(|_| IoErrorKind::FileTooLarge)?;
        let bytes_written = unsafe {
            convert_result(littlefs::lfs_file_seek(
                file_system,
                &mut self.file,
                position,
                constants::SEEK_START,
            ))?;

            convert_result(littlefs::lfs_file_write(
                file_system,
                &mut self.file,
                buffer.as_ptr() as *const _,
                convert_length(buffer.len()),
            ))?
        };

        Ok(bytes_written as _)
    }

    pub fn set_position(
        &mut self,
        file_system: &mut littlefs::lfs_t,
        _: Size,
        position: &Position,
    ) -> Result<Size> {
        let (offset, whence) = convert_position(position)?;

        let offset = unsafe {
            convert_result(littlefs::lfs_file_seek(
                file_system,
                &mut self.file,
                offset,
                whence,
            ))?
        };

        Ok(offset as _)
    }

    pub fn flush(&mut self, file_system: &mut littlefs::lfs_t) -> Result<()> {
        unsafe {
            convert_result(littlefs::lfs_file_sync(file_system, &mut self.file))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_write_and_seek_reject_positions_above_32_bits() {
        let device = file_system::MemoryDevice::<512>::new_static(512 * 128);
        let fs = crate::FileSystem::new_format(device, crate::Options::new(256)).unwrap();
        fs.operation(|native| {
            let path = Path::new("/positions");
            File::create(native, path)?;
            let mut file = File::lookup(native, path, file_system::AccessFlags::READ_WRITE.into())?;

            assert_eq!(file.write(native, b"Xila", 0)?, 4);

            let mut buffer = [0u8; 4];
            for position in [1u64 << 31, (1u64 << 32) + 100] {
                assert_eq!(
                    file.read(native, &mut buffer, position),
                    Err(IoErrorKind::FileTooLarge)
                );
                assert_eq!(
                    file.write(native, b"oops", position),
                    Err(IoErrorKind::FileTooLarge)
                );
                assert_eq!(
                    file.set_position(native, 0, &Position::Start(position)),
                    Err(IoErrorKind::FileTooLarge)
                );
            }

            // A wrapped offset must not overwrite the start of the file.
            assert_eq!(file.read(native, &mut buffer, 0)?, 4);
            assert_eq!(&buffer, b"Xila");
            file.close(native)
        })
        .unwrap();
    }

    #[test]
    fn errored_handle_rejects_metadata_without_publishing_cached_changes() {
        let device = file_system::MemoryDevice::<512>::new_static(512 * 128);
        let fs = crate::FileSystem::new_format(device, crate::Options::new(256)).unwrap();
        fs.operation(|native| {
            let path = Path::new("/errored");
            File::create(native, path)?;
            let mut file = File::lookup(native, path, file_system::AccessFlags::READ_WRITE.into())?;
            let original = file.internal_attributes.permissions;
            // Fault state left by LittleFS after a failed write/commit. Its
            // sync function returns success immediately in this state.
            file.file.flags |= constants::FILE_ERRORED;
            assert_eq!(
                file.set_attributes(
                    native,
                    &Attributes::new().set_permissions(file_system::Permissions::ALL_FULL)
                ),
                Err(IoErrorKind::InputOutput)
            );
            assert_eq!(file.internal_attributes.permissions, original);
            file.close(native)
        })
        .unwrap();
    }
}
