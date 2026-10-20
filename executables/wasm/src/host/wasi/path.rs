use alloc::vec::Vec;
use wasmi::Caller;
use xila::{
    file_system::{AccessFlags, CreateFlags, Flags, Path, PathOwned, StateFlags},
    task,
};

use crate::{
    define_wasi_module,
    host::{
        store::GlobalStore,
        translation::{GuestPointer, GuestSlice, WasmUsize, borrow_memory, get_memory},
        wasi::{
            Error,
            context::{DirectoryVariant, FileSystemItem, FileVariant},
            error::{WasiResult, vfs_error, wrap_function},
            types::Filestat,
        },
    },
};

fn kind_to_filetype(kind: &xila::file_system::Kind) -> u8 {
    use xila::file_system::Kind;
    match kind {
        Kind::Directory => 3,
        Kind::CharacterDevice => 2,
        Kind::BlockDevice => 1,
        Kind::Pipe => 10,
        Kind::Socket => 11,
        Kind::SymbolicLink => 7,
        _ => 4,
    }
}

fn make_filestat(stats: &xila::file_system::Statistics) -> Filestat {
    Filestat {
        dev: 0,
        ino: stats.inode,
        filetype: kind_to_filetype(&stats.kind),
        nlink: stats.links,
        size: stats.size,
        atime: stats.access.as_u64().saturating_mul(1_000_000_000),
        mtime: stats.modification.as_u64().saturating_mul(1_000_000_000),
        ctime: stats.creation.as_u64().saturating_mul(1_000_000_000),
    }
}

fn copy_guest_path(
    caller: &mut Caller<'_, GlobalStore>,
    ptr: WasmUsize,
    len: WasmUsize,
) -> Result<Vec<u8>, Error> {
    let memory = get_memory(caller).ok_or(Error::Fault)?;
    let guest = borrow_memory(&memory, caller);
    Ok(guest
        .bytes(GuestSlice::new(ptr, len))
        .ok_or(Error::Fault)?
        .to_vec())
}

fn base_path(
    caller: &Caller<'_, GlobalStore>,
    fd: i32,
) -> Result<(xila::task::TaskIdentifier, PathOwned), Error> {
    let item = caller
        .data()
        .wasi
        .files
        .get(&(fd as u32))
        .ok_or(Error::Badf)?;
    let FileSystemItem::Directory(dir) = item else {
        return Err(Error::Notdir);
    };
    Ok((caller.data().wasi.task, dir.path.clone()))
}

fn resolve_guest_path(directory: &PathOwned, path: &str) -> Result<PathOwned, Error> {
    if path.split('/').any(|component| component == "..") {
        return Err(Error::Notcapable);
    }
    let relative = path.strip_prefix('/').unwrap_or(path);
    directory
        .clone()
        .join(Path::from_str(relative))
        .ok_or(Error::Inval)
}

define_wasi_module! {
    module: "wasi_snapshot_preview1";

    fn path_create_directory(caller: Caller<GlobalStore>, fd: i32, path_ptr: WasmUsize, path_len: WasmUsize) -> Result<WasiResult, wasmi::Error> {
        wrap_function!({
            let mut caller = caller;
            let path_bytes = copy_guest_path(&mut caller, path_ptr, path_len)?;
            let (task_id, directory) = base_path(&caller, fd)?;
            let relative = core::str::from_utf8(&path_bytes).map_err(|_| Error::Ilseq)?;
            let absolute_path = resolve_guest_path(&directory, relative)?;
            task::block_on(caller.data().context.virtual_file_system.create_directory(task_id, &absolute_path)).map_err(vfs_error)?;
            Ok(())
        })
    }

    fn path_symlink(caller: Caller<GlobalStore>, old_path_ptr: WasmUsize, old_path_len: WasmUsize, fd: i32, new_path_ptr: WasmUsize, new_path_len: WasmUsize) -> Result<WasiResult, wasmi::Error> {
        wrap_function!({
            let mut caller = caller;
            let target = copy_guest_path(&mut caller, old_path_ptr, old_path_len)?;
            let link = copy_guest_path(&mut caller, new_path_ptr, new_path_len)?;
            let (task_id, directory) = base_path(&caller, fd)?;
            let target = core::str::from_utf8(&target).map_err(|_| Error::Ilseq)?;
            let link = core::str::from_utf8(&link).map_err(|_| Error::Ilseq)?;
            let link = resolve_guest_path(&directory, link)?;
            task::block_on(caller.data().context.virtual_file_system.create_symlink(task_id, Path::from_str(target), &link)).map_err(vfs_error)?;
            Ok(())
        })
    }

    fn path_readlink(caller: Caller<GlobalStore>, fd: i32, path_ptr: WasmUsize, path_len: WasmUsize, buf_ptr: WasmUsize, buf_len: WasmUsize, used_ptr: WasmUsize) -> Result<WasiResult, wasmi::Error> {
        wrap_function!({
            let mut caller = caller;
            let path_bytes = copy_guest_path(&mut caller, path_ptr, path_len)?;
            let (task_id, directory) = base_path(&caller, fd)?;
            let relative = core::str::from_utf8(&path_bytes).map_err(|_| Error::Ilseq)?;
            let absolute_path = resolve_guest_path(&directory, relative)?;
            let memory = get_memory(&caller).ok_or(Error::Fault)?;
            let output = GuestSlice::new(buf_ptr, buf_len);
            let used = GuestPointer::<WasmUsize>::new(used_ptr);
            let mut target = alloc::vec![0; 4096];
            let length = task::block_on(caller.data().context.virtual_file_system.read_symlink(task_id, &absolute_path, &mut target)).map_err(vfs_error)?;
            let mut guest = borrow_memory(&memory, &mut caller);
            guest.validate_pointer(used).ok_or(Error::Fault)?;
            let copied = core::cmp::min(length, buf_len as usize);
            guest.bytes_mut(output).ok_or(Error::Fault)?[..copied].copy_from_slice(&target[..copied]);
            guest.write(used, copied as WasmUsize).ok_or(Error::Fault)?;
            Ok(())
        })
    }

    fn path_unlink_file(caller: Caller<GlobalStore>, fd: i32, path_ptr: WasmUsize, path_len: WasmUsize) -> Result<WasiResult, wasmi::Error> {
        wrap_function!({
            let mut caller = caller;
            let path_bytes = copy_guest_path(&mut caller, path_ptr, path_len)?;
            let (task_id, directory) = base_path(&caller, fd)?;
            let relative = core::str::from_utf8(&path_bytes).map_err(|_| Error::Ilseq)?;
            let absolute_path = resolve_guest_path(&directory, relative)?;
            let vfs = caller.data().context.virtual_file_system;
            let statistics = task::block_on(vfs.get_link_statistics(&absolute_path)).map_err(vfs_error)?;
            if statistics.kind == xila::file_system::Kind::Directory {
                return Err(Error::Isdir.into());
            }
            task::block_on(vfs.remove(task_id, &absolute_path)).map_err(vfs_error)?;
            Ok(())
        })
    }

    fn path_open(caller: Caller<GlobalStore>, fd: i32, _dirflags: i32, path_ptr: WasmUsize, path_len: WasmUsize, oflags: i32, rights_base: i64, _rights_inheriting: i64, fdflags: i32, opened_fd_ptr: WasmUsize) -> Result<WasiResult, wasmi::Error> {
        wrap_function!({
            let mut caller = caller;
            let path_bytes = copy_guest_path(&mut caller, path_ptr, path_len)?;
            let (task_id, directory) = base_path(&caller, fd)?;
            let relative = core::str::from_utf8(&path_bytes).map_err(|_| Error::Ilseq)?;
            let absolute_path = resolve_guest_path(&directory, relative)?;
            let memory = get_memory(&caller).ok_or(Error::Fault)?;
            let opened_fd_out = GuestPointer::<i32>::new(opened_fd_ptr);
            borrow_memory(&memory, &mut caller).validate_pointer(opened_fd_out).ok_or(Error::Fault)?;
            let read = (rights_base & 2) != 0;
            let write = (rights_base & 64) != 0;
            let access = match (read, write) { (true, true) => AccessFlags::READ_WRITE, (true, false) => AccessFlags::Read, (false, true) => AccessFlags::Write, (false, false) => AccessFlags::Read };
            let create = if oflags & 1 != 0 { Some(if oflags & 4 != 0 { CreateFlags::CREATE_EXCLUSIVE } else if oflags & 8 != 0 { CreateFlags::CREATE_TRUNCATE } else { CreateFlags::Create }) } else { None };
            let state = if fdflags & 1 != 0 { Some(StateFlags::Append) } else { None };
            let vfs = caller.data().context.virtual_file_system;
            let item = if oflags & 2 != 0 {
                let directory_handle = xila::virtual_file_system::SynchronousDirectory::open(vfs, task_id, &absolute_path).map_err(vfs_error)?;
                FileSystemItem::Directory(DirectoryVariant { directory: directory_handle, path: absolute_path })
            } else {
                let file = xila::virtual_file_system::SynchronousFile::open(vfs, task_id, &absolute_path, Flags::new(access, create, state)).map_err(vfs_error)?;
                FileSystemItem::File(FileVariant { file })
            };
            let new_fd = caller.data_mut().wasi.insert_file_system_item(item).ok_or(Error::Mfile)?;
            borrow_memory(&memory, &mut caller)
                .write(opened_fd_out, i32::try_from(new_fd).map_err(|_| Error::Overflow)?).ok_or(Error::Fault)?;
            Ok(())
        })
    }

    fn path_filestat_get(caller: Caller<GlobalStore>, fd: i32, flags: i32, path_ptr: WasmUsize, path_len: WasmUsize, buf_ptr: WasmUsize) -> Result<WasiResult, wasmi::Error> {
        wrap_function!({
            let mut caller = caller;
            let path_bytes = copy_guest_path(&mut caller, path_ptr, path_len)?;
            let (task_id, directory) = base_path(&caller, fd)?;
            let relative = core::str::from_utf8(&path_bytes).map_err(|_| Error::Ilseq)?;
            let absolute_path = resolve_guest_path(&directory, relative)?;
            let vfs = caller.data().context.virtual_file_system;
            let stats = if flags & 1 != 0 {
                task::block_on(vfs.get_statistics_for_task(task_id, &absolute_path))
            } else {
                task::block_on(vfs.get_link_statistics(&absolute_path))
            }.map_err(vfs_error)?;
            let filestat = make_filestat(&stats);
            let memory = get_memory(&caller).ok_or(Error::Fault)?;
            borrow_memory(&memory, &mut caller).write(GuestPointer::new(buf_ptr), filestat).ok_or(Error::Fault)?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Error, resolve_guest_path};
    use xila::file_system::PathOwned;

    #[test]
    fn absolute_guest_paths_are_root_relative_to_the_preopen() {
        let root = PathOwned::root();
        let path = resolve_guest_path(&root, "/test.txt").unwrap();
        assert_eq!(<PathOwned as AsRef<str>>::as_ref(&path), "/test.txt");
    }

    #[test]
    fn parent_components_cannot_escape_the_preopen() {
        let root = PathOwned::root();
        assert!(matches!(
            resolve_guest_path(&root, "../outside"),
            Err(Error::Notcapable)
        ));
        assert!(matches!(
            resolve_guest_path(&root, "nested/../../outside"),
            Err(Error::Notcapable)
        ));
    }
}
