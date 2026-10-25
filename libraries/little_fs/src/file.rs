use super::{convert_flags, convert_result, littlefs};
use crate::attributes::InternalAttributes;
use alloc::{boxed::Box, ffi::CString};
use core::ffi::c_void;
use file_system::{AttributeFlags, Attributes, Error, Flags, Path, Position, Result, Size};

fn convert_position(position: &Position) -> (i32, i32) {
    match position {
        Position::Start(position) => (
            *position as i32,
            littlefs::lfs_whence_flags_LFS_SEEK_SET as i32,
        ),
        Position::Current(position) => (
            *position as i32,
            littlefs::lfs_whence_flags_LFS_SEEK_CUR as i32,
        ),
        Position::End(position) => (
            *position as i32,
            littlefs::lfs_whence_flags_LFS_SEEK_END as i32,
        ),
    }
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
        let path = CString::new(path.as_str()).map_err(|_| Error::InvalidParameter)?;

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
        let path = CString::new(path.as_str()).map_err(|_| Error::InvalidParameter)?;

        let mut file = littlefs::lfs_file_t::default();

        unsafe {
            convert_result(littlefs::lfs_file_open(
                file_system,
                &mut file,
                path.as_ptr(),
                littlefs::lfs_open_flags_LFS_O_CREAT as i32
                    | littlefs::lfs_open_flags_LFS_O_EXCL as i32,
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
        InternalAttributes::validate_attribute_mask(mask)?;

        // Read-only opens cannot persist configured attributes. Never report a
        // successful chmod that disappears when the descriptor closes.
        if self.file.flags & 3 == littlefs::lfs_open_flags_LFS_O_RDONLY {
            return Err(Error::UnsupportedOperation);
        }
        // sync skips errored files and commits data together with attributes.
        // A dusty independent open has stale data; callers must reopen it.
        if self.file.flags & littlefs::lfs_open_flags_LFS_F_ERRED != 0 {
            return Err(Error::InputOutput);
        }
        if self.file.flags & littlefs::lfs_open_flags_LFS_F_DUSTY != 0 {
            return Err(Error::RessourceBusy);
        }
        let previous = self.internal_attributes.clone();
        self.internal_attributes
            .update_with_attributes(attributes)?;
        self.file.flags |= littlefs::lfs_open_flags_LFS_F_DIRTY;
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
        absolue_position: Size,
    ) -> Result<usize> {
        let bytes_read = unsafe {
            convert_result(littlefs::lfs_file_seek(
                file_system,
                &mut self.file,
                absolue_position as i32,
                littlefs::lfs_whence_flags_LFS_SEEK_SET as i32,
            ))?;

            convert_result(littlefs::lfs_file_read(
                file_system,
                &mut self.file,
                buffer.as_mut_ptr() as *mut _,
                buffer.len() as u32,
            ))?
        };

        Ok(bytes_read as _)
    }

    pub fn write(
        &mut self,
        file_system: &mut littlefs::lfs_t,
        buffer: &[u8],
        absolue_position: Size,
    ) -> Result<usize> {
        let bytes_written = unsafe {
            convert_result(littlefs::lfs_file_seek(
                file_system,
                &mut self.file,
                absolue_position as i32,
                littlefs::lfs_whence_flags_LFS_SEEK_SET as i32,
            ))?;

            convert_result(littlefs::lfs_file_write(
                file_system,
                &mut self.file,
                buffer.as_ptr() as *const _,
                buffer.len() as u32,
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
        let (offset, whence) = convert_position(position);

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
    fn errored_handle_rejects_metadata_without_publishing_cached_changes() {
        let device = file_system::MemoryDevice::<512>::new_static(512 * 128);
        let fs = crate::FileSystem::new_format(device, 256).unwrap();
        fs.operation(|native| {
            let path = Path::new("/errored");
            File::create(native, path)?;
            let mut file = File::lookup(native, path, file_system::AccessFlags::READ_WRITE.into())?;
            let original = file.internal_attributes.permissions;
            // Fault state left by LittleFS after a failed write/commit. Its
            // sync function returns success immediately in this state.
            file.file.flags |= littlefs::lfs_open_flags_LFS_F_ERRED;
            assert_eq!(
                file.set_attributes(
                    native,
                    &Attributes::new().set_permissions(file_system::Permissions::ALL_FULL)
                ),
                Err(Error::InputOutput)
            );
            assert_eq!(file.internal_attributes.permissions, original);
            file.close(native)
        })
        .unwrap();
    }
}
