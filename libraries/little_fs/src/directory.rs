use core::{ffi::CStr, mem::MaybeUninit};

use crate::{attributes::InternalAttributes, error::convert_result, littlefs};
use alloc::{borrow::ToOwned, boxed::Box, ffi::CString, string::ToString};
use file_system::{Attributes, Entry, Error, Kind, Path, PathOwned, Result, Size};

pub struct Directory {
    directory: littlefs::lfs_dir_t,
    path: PathOwned,
}

unsafe impl Send for Directory {}
unsafe impl Sync for Directory {}

impl Directory {
    pub fn create(file_system: &mut littlefs::lfs_t, path: &Path) -> Result<()> {
        let path = CString::new(path.as_str()).unwrap();

        unsafe {
            convert_result(littlefs::lfs_mkdir(file_system, path.as_ptr()))?;

            let mut internal_attributes = InternalAttributes::new_uninitialized().assume_init();
            internal_attributes.kind = Kind::Directory;

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

    pub fn lookup(file_system: &mut littlefs::lfs_t, path: &Path) -> Result<Box<Self>> {
        let path_owned = path.to_owned();
        let path = CString::new(path.as_str()).unwrap();

        let directory = Self {
            directory: littlefs::lfs_dir_t::default(),
            path: path_owned,
        };
        let mut directory = Box::new(directory);

        convert_result(unsafe {
            littlefs::lfs_dir_open(file_system, &mut directory.directory, path.as_ptr())
        })?;

        Ok(directory)
    }

    pub fn get_attributes(&self, attributes: &mut Attributes) -> Result<()> {
        let requested = attributes.get_mask();
        *attributes = attributes
            .clone()
            .set_mask(requested.intersection(file_system::AttributeFlags::Kind));

        if let Some(kind) = attributes.get_mutable_kind() {
            *kind = file_system::Kind::Directory;
        }

        Ok(())
    }

    pub fn set_attributes(&self, attributes: &Attributes) -> Result<()> {
        if attributes.get_mask().is_empty() {
            Ok(())
        } else {
            Err(Error::UnsupportedOperation)
        }
    }

    pub fn rewind(&mut self, file_system: &mut littlefs::lfs_t) -> Result<()> {
        convert_result(unsafe { littlefs::lfs_dir_rewind(file_system, &mut self.directory) })?;

        Ok(())
    }

    pub fn get_position(&mut self, file_system: &mut littlefs::lfs_t) -> Result<Size> {
        let offset =
            convert_result(unsafe { littlefs::lfs_dir_tell(file_system, &mut self.directory) })?;

        Ok(offset as _)
    }

    pub fn set_position(
        &mut self,
        file_system: &mut littlefs::lfs_t,
        position: Size,
    ) -> Result<()> {
        convert_result(unsafe {
            littlefs::lfs_dir_seek(
                file_system,
                &mut self.directory,
                position as littlefs::lfs_off_t,
            )
        })?;

        Ok(())
    }

    pub fn read(&mut self, file_system: &mut littlefs::lfs_t) -> Result<Option<Entry>> {
        let informations = MaybeUninit::<littlefs::lfs_info>::uninit();

        let mut informations = unsafe { informations.assume_init() };

        let result = unsafe {
            littlefs::lfs_dir_read(
                file_system,
                &mut self.directory,
                &mut informations as *mut _,
            )
        };

        if result == 0 {
            return Ok(None);
        }

        convert_result(result)?;

        let name = unsafe { CStr::from_ptr(informations.name.as_ptr()) };
        let name = name.to_str().unwrap().to_string();

        let r#type = if informations.type_ == littlefs::lfs_type_LFS_TYPE_DIR as u8 {
            Kind::Directory
        } else {
            let entry_path = self
                .path
                .clone()
                .join(Path::from_str(&name))
                .ok_or(Error::InvalidPath)?;
            let entry_path =
                CString::new(entry_path.as_str()).map_err(|_| Error::InvalidParameter)?;
            let mut stored_attributes =
                unsafe { InternalAttributes::new_uninitialized().assume_init() };
            let result = unsafe {
                littlefs::lfs_getattr(
                    file_system,
                    entry_path.as_ptr(),
                    InternalAttributes::IDENTIFIER,
                    &mut stored_attributes as *mut _ as *mut _,
                    size_of::<InternalAttributes>() as u32,
                )
            };
            match convert_result(result) {
                Ok(_) if stored_attributes.kind == Kind::SymbolicLink => Kind::SymbolicLink,
                Ok(_) | Err(Error::NoAttribute) => Kind::File,
                Err(error) => return Err(error),
            }
        };

        let entry = Entry::new(0, name, r#type, informations.size as _);

        Ok(Some(entry))
    }

    pub fn close(&mut self, file_system: &mut littlefs::lfs_t) -> Result<()> {
        convert_result(unsafe { littlefs::lfs_dir_close(file_system, &mut self.directory) })?;

        Ok(())
    }
}
