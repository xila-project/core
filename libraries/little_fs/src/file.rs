use super::{convert_flags, convert_result, littlefs};
use crate::attributes::{InternalAttributes, entry_is_attribute_less};
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

// The open file context must stay small on devices with 256 KB of RAM. The
// attribute buffer it carries lives in the context for as long as the file is
// open: 160 bytes on 32-bit targets, 200 bytes on 64-bit hosts.
const _: () = assert!(
    core::mem::size_of::<File>()
        == if core::mem::size_of::<usize>() == 4 {
            160
        } else {
            200
        }
);

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
            internal_attributes: InternalAttributes::new(),
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

        // LittleFS marks a write open with configured attributes dirty so the
        // attributes are rewritten on close. When the open neither creates nor
        // truncates the file, drop that flag again: an open for writing that
        // writes nothing must not cost a metadata commit. Writes and truncates
        // set the flag again, and the attributes are then committed atomically
        // with the data on sync or close.
        let write_access =
            (little_fs_flags as u32) & constants::FILE_ACCESS_MASK != constants::FILE_READ_ONLY;
        if write_access
            && little_fs_flags & (constants::OPEN_CREATE | constants::OPEN_TRUNCATE) == 0
        {
            file.file.flags &= !constants::FILE_DIRTY;
        }

        Ok(file)
    }

    /// Create a file, committing `attributes` together with the file.
    ///
    /// The attribute buffer is attached through `lfs_file_config.attrs`, so
    /// LittleFS writes it atomically with the file on close: the entry commit
    /// and the following struct+attributes commit never leave the file with
    /// default or missing owner and permissions. The buffer only has to
    /// outlive this call because the file is closed before returning.
    ///
    /// If a step fails after the entry commit landed (cache allocation, the
    /// attributes commit, close), the half-created name is removed again on a
    /// best-effort basis, so the path is either fully created or absent. Only
    /// an attribute-less entry is removed: an entry that has attributes is
    /// never one this call half-created, so a pre-existing entry is never
    /// deleted, even when a transient read error fails the create.
    pub fn create(
        file_system: &mut littlefs::lfs_t,
        path: &Path,
        attributes: &Attributes,
    ) -> Result<()> {
        let path = CString::new(path.as_str()).map_err(|_| IoErrorKind::InvalidArgument)?;
        InternalAttributes::validate_attribute_mask(attributes.get_mask())?;

        let mut internal_attributes = InternalAttributes::from_attributes(attributes);
        let mut entry_attributes = littlefs::lfs_attr {
            type_: InternalAttributes::IDENTIFIER,
            buffer: &mut internal_attributes as *mut _ as *mut c_void,
            size: core::mem::size_of::<InternalAttributes>() as u32,
        };
        let configuration = littlefs::lfs_file_config {
            attr_count: 1,
            attrs: &mut entry_attributes,
            ..littlefs::lfs_file_config::default()
        };

        let mut file = littlefs::lfs_file_t::default();

        let result = unsafe {
            convert_result(littlefs::lfs_file_opencfg(
                file_system,
                &mut file,
                path.as_ptr(),
                constants::OPEN_READ_WRITE | constants::OPEN_CREATE | constants::OPEN_EXCLUSIVE,
                &configuration,
            ))
            .map(|_| ())
            .and_then(|_| {
                convert_result(littlefs::lfs_file_close(file_system, &mut file)).map(|_| ())
            })
        };
        match result {
            Ok(()) => Ok(()),
            // The exclusive check failed before any commit: the existing
            // entry must not be removed.
            Err(error) if error == IoErrorKind::AlreadyExists => Err(error),
            Err(error) => {
                // The entry commit may have landed before the failure. Remove
                // the name best effort, but only when it is attribute-less:
                // an entry that has attributes is not one this call
                // half-created (a transient read error can make a lookup miss
                // an existing entry, so existence alone proves nothing), and
                // an unreadable device leaves the fail-closed entry for the
                // caller to handle through the VFS.
                if entry_is_attribute_less(file_system, &path) {
                    unsafe {
                        let _ = littlefs::lfs_remove(file_system, path.as_ptr());
                    }
                }
                Err(error)
            }
        }
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
        // Only time changes are deferred on a handle with pending state (a
        // write sets LFS_F_WRITING, a truncate or this call LFS_F_DIRTY): they
        // ride along with that data on the next sync or close instead of
        // costing an extra commit per write. Everything else (owner, group,
        // permissions, kind, size) is committed at once, so path-level stat
        // and other openers see it immediately. Checking only LFS_F_DIRTY for
        // the pending state would sync after every write, as lfs_file_write
        // sets LFS_F_WRITING only.
        const TIME_FLAGS: AttributeFlags = AttributeFlags::CreationTime
            .union(AttributeFlags::AccessTime)
            .union(AttributeFlags::ModificationTime)
            .union(AttributeFlags::StatusTime);
        let already_pending = self.file.flags & constants::FILE_PENDING != 0;
        let time_only = mask.difference(TIME_FLAGS).is_empty();
        let defer = already_pending && time_only;
        let previous = self.internal_attributes.clone();
        self.internal_attributes
            .update_with_attributes(attributes)?;
        self.file.flags |= constants::FILE_DIRTY;
        if !defer {
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
        }
        Ok(())
    }

    pub fn close(&mut self, file_system: &mut littlefs::lfs_t) -> Result<()> {
        unsafe {
            // Publish this handle's metadata to the other open handles only
            // when it had pending state: a clean handle (for example a
            // read-only one) holds a possibly stale copy, and publishing it
            // would revert times another handle has pending.
            let pending = self.file.flags & constants::FILE_PENDING != 0;
            let mut result: Result<()> = Ok(());
            if self.file.flags & constants::FILE_ERRORED == 0 {
                result = convert_result(littlefs::lfs_file_sync(file_system, &mut self.file))
                    .map(|_| ());
                if result.is_ok() && pending {
                    // The close below commits pending data and attributes
                    // atomically; publish them to the other open handles first.
                    let current = self.internal_attributes.clone();
                    crate::open_metadata::synchronize(file_system, &self.file, &current);
                }
            }
            let close = convert_result(littlefs::lfs_file_close(file_system, &mut self.file));
            result?;
            close?;
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
        // A zero-byte write is not a mutation: it must not dirty the file and
        // must not trigger a metadata commit on close.
        if buffer.is_empty() {
            return Ok(0);
        }
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
        // Publish only when this handle had pending state before the sync: a
        // clean handle's copy may be stale and must not revert another
        // handle's pending metadata.
        let pending = self.file.flags & constants::FILE_PENDING != 0;
        unsafe {
            convert_result(littlefs::lfs_file_sync(file_system, &mut self.file))?;
        }
        if pending {
            // The sync committed the attribute buffer together with the data;
            // publish it to the other open handles of the same object.
            let current = self.internal_attributes.clone();
            crate::open_metadata::synchronize(file_system, &self.file, &current);
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
            File::create(native, path, &Attributes::new())?;
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

            // A write at 2^32 + 100 that wrapped to 32 bits would land at offset 100
            // and grow the file. The file must still hold its 4 bytes only.
            assert_eq!(file.read(native, &mut buffer, 0)?, 4);
            assert_eq!(&buffer, b"Xila");
            assert_eq!(file.read(native, &mut buffer, 4)?, 0);
            assert_eq!(file.read(native, &mut buffer, 100)?, 0);
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
            File::create(native, path, &Attributes::new())?;
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
