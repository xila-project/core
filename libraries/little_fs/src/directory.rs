use core::{ffi::CStr, mem::MaybeUninit};

use crate::{
    attributes::{InternalAttributes, drop_read_cache, entry_is_attribute_less},
    constants,
    error::convert_result,
    littlefs,
};
use alloc::{borrow::ToOwned, boxed::Box, ffi::CString, string::ToString};
use file_system::{
    AttributeFlags, Attributes, Entry, IoErrorKind, Kind, Path, PathOwned, Result, Size, Time,
};

pub struct Directory {
    directory: littlefs::lfs_dir_t,
    path: PathOwned,
}

unsafe impl Send for Directory {}
unsafe impl Sync for Directory {}

impl Directory {
    /// Create a directory, persisting `attributes` with the new entry.
    ///
    /// `lfs_mkdir` has no attribute configuration, so the attributes land in
    /// the commit immediately after the directory entry instead of in the
    /// same one. Until that commit lands, the directory exists without Xila
    /// attributes: metadata reads fail closed with `MissingAttribute` rather
    /// than reporting root with no permissions. The name is not dead, though:
    /// `get_attributes` still reports the kind and size from `lfs_stat`, and
    /// the entry can be removed through its parent or replaced by a new
    /// create. If the attribute commit fails, the half-created directory is
    /// removed again on a best-effort basis, but only when this call's own
    /// `lfs_mkdir` committed the entry and the entry is attribute-less after
    /// emptying the read cache: a stale cache left by a failed read can show
    /// an older twin of a metadata pair, so a pre-existing entry is never
    /// deleted, even when a transient read error fails the create.
    pub fn create(
        file_system: &mut littlefs::lfs_t,
        path: &Path,
        attributes: &Attributes,
    ) -> Result<()> {
        let path = CString::new(path.as_str()).map_err(|_| IoErrorKind::InvalidArgument)?;
        InternalAttributes::validate_attribute_mask(attributes.get_mask())?;

        let mut internal_attributes = InternalAttributes::from_attributes(attributes);
        if !attributes.get_mask().contains(AttributeFlags::Kind) {
            internal_attributes.kind = Kind::Directory;
        }

        // Track whether this call's own entry commit succeeded.
        let mkdir_result =
            unsafe { convert_result(littlefs::lfs_mkdir(file_system, path.as_ptr())) };
        let result = mkdir_result.and_then(|_| {
            convert_result(unsafe {
                littlefs::lfs_setattr(
                    file_system,
                    path.as_ptr(),
                    InternalAttributes::IDENTIFIER,
                    &internal_attributes as *const _ as *const _,
                    size_of::<InternalAttributes>() as u32,
                )
            })
            .map(|_| ())
        });
        match result {
            Ok(()) => Ok(()),
            // The exclusive check failed before any commit: an existing
            // entry must not be removed.
            Err(error) if error == IoErrorKind::AlreadyExists => Err(error),
            Err(error) => {
                // The directory entry may have landed before the attribute
                // commit failed. Remove the name best effort, but only when
                // this call's mkdir committed it and it is attribute-less
                // (the probe empties the read cache first, so a stale cache
                // cannot report a pre-existing entry as attribute-less); an
                // unreadable device leaves the fail-closed entry for the
                // caller to handle through the VFS.
                if mkdir_result.is_ok() && entry_is_attribute_less(file_system, &path) {
                    // No cache drop before the remove: the probe only answers
                    // from successful reads, so the cache can hold fresh bytes
                    // only.
                    unsafe {
                        let _ = littlefs::lfs_remove(file_system, path.as_ptr());
                    }
                }
                // The probe and the remove ignore their own read errors, and
                // the error returned here may not be an I/O error (for example
                // NotFound for a missing parent directory), so the operation
                // would not empty the read cache for it. A read that failed in
                // the cleanup must not leave stale bytes behind.
                drop_read_cache(file_system);
                Err(error)
            }
        }
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
            Err(IoErrorKind::Unsupported)
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
        // LittleFS directory positions are unsigned 32-bit offsets. Reject what
        // it cannot represent instead of truncating, as a wrapped position
        // would resume the listing at the wrong entry.
        let position = u32::try_from(position).map_err(|_| IoErrorKind::FileTooLarge)?;
        convert_result(unsafe {
            littlefs::lfs_dir_seek(file_system, &mut self.directory, position)
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

        let entry_path = Self::resolve_entry_path(&self.path, &name)?;
        let mut stored_attributes = InternalAttributes::new();
        let result = unsafe {
            littlefs::lfs_getattr(
                file_system,
                entry_path.as_ptr(),
                InternalAttributes::IDENTIFIER,
                &mut stored_attributes as *mut _ as *mut _,
                size_of::<InternalAttributes>() as u32,
            )
        };
        let (inode, modification, has_attributes) = match convert_result(result) {
            Ok(_) => (
                stored_attributes.inode,
                stored_attributes.modification,
                true,
            ),
            Err(IoErrorKind::MissingAttribute) => (0, Time::new(0), false),
            Err(error) => return Err(error),
        };

        let r#type = if informations.type_ == constants::TYPE_DIRECTORY {
            Kind::Directory
        } else if has_attributes && stored_attributes.kind == Kind::SymbolicLink {
            Kind::SymbolicLink
        } else {
            Kind::File
        };

        let entry = Entry::new(inode, name, r#type, informations.size as _, modification);

        Ok(Some(entry))
    }

    fn resolve_entry_path(directory: &Path, name: &str) -> Result<CString> {
        match name {
            "." => {
                let target = if directory.is_empty() {
                    "/"
                } else {
                    directory.as_str()
                };
                CString::new(target).map_err(|_| IoErrorKind::InvalidArgument)
            }
            ".." => {
                let mut components = alloc::vec::Vec::new();
                for component in directory.as_str().split('/') {
                    match component {
                        "" | "." => {}
                        ".." => {
                            components.pop();
                        }
                        other => components.push(other),
                    }
                }
                components.pop();
                if components.is_empty() {
                    CString::new("/").map_err(|_| IoErrorKind::InvalidArgument)
                } else {
                    let mut parent = alloc::string::String::new();
                    for component in components {
                        parent.push('/');
                        parent.push_str(component);
                    }
                    CString::new(parent).map_err(|_| IoErrorKind::InvalidArgument)
                }
            }
            _ => {
                let entry_path = directory
                    .join(Path::from_str(name))
                    .ok_or(IoErrorKind::InvalidPath)?;
                CString::new(entry_path.as_str()).map_err(|_| IoErrorKind::InvalidArgument)
            }
        }
    }

    pub fn close(&mut self, file_system: &mut littlefs::lfs_t) -> Result<()> {
        convert_result(unsafe { littlefs::lfs_dir_close(file_system, &mut self.directory) })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_position_rejects_positions_above_32_bits() {
        let device = file_system::MemoryDevice::<512>::new_static(512 * 128);
        let fs = crate::FileSystem::new_format(device, crate::Options::new(256)).unwrap();
        fs.operation(|native| {
            let mut directory = Directory::lookup(native, Path::ROOT)?;

            // A position LittleFS can represent still reaches it: 2 skips the
            // "." and ".." pseudo-entries.
            directory.set_position(native, 2)?;
            assert_eq!(directory.get_position(native)?, 2);

            // Positions above 32 bits are rejected before the call; a
            // truncated position would silently resume at the wrong entry.
            for position in [(u32::MAX as Size) + 1, (1u64 << 32) + 2, (1u64 << 32) + 100] {
                assert_eq!(
                    directory.set_position(native, position),
                    Err(IoErrorKind::FileTooLarge)
                );
            }

            // A rejected position must not move the directory.
            assert_eq!(directory.get_position(native)?, 2);
            directory.close(native)
        })
        .unwrap();
    }
}
