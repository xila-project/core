use alloc::vec::Vec;
use wasmi::Caller;
use xila::file_system::Position;

use crate::{
    define_wasi_module,
    host::{
        store::GlobalStore,
        translation::{GuestPointer, GuestSlice, WasiVector, WasmUsize, borrow_memory, get_memory},
        wasi::{
            Error,
            context::{FileSystemItem, FileVariant},
            error::{WasiResult, vfs_error, wrap_function},
            types::{Fdstat, Filestat, Prestat as PrestatAbi, PrestatDir},
        },
    },
};
use xila::{file_system::ControlCommandIdentifier, shared::AnyByLayout};

/// Maximum input or output payload size accepted by the generic descriptor control ABI.
const MAX_CONTROL_PAYLOAD: usize = 256;

#[repr(align(16))]
struct AlignedControlPayload([u8; MAX_CONTROL_PAYLOAD]);

const CONTROL_PAYLOAD_ALIGNMENT: usize = core::mem::align_of::<AlignedControlPayload>();

fn spans_overlap(
    first_start: usize,
    first_length: usize,
    second_start: usize,
    second_length: usize,
) -> Option<bool> {
    let first_end = first_start.checked_add(first_length)?;
    let second_end = second_start.checked_add(second_length)?;
    Some(first_start < second_end && second_start < first_end)
}

define_wasi_module! {
    module: "wasi_snapshot_preview1";

    fn fd_read(caller: Caller<GlobalStore>, fd: i32, iovs: WasmUsize, iovs_len: WasmUsize, nread_ptr: WasmUsize) -> Result<WasiResult, wasmi::Error> {
        wrap_function!({
            let mut caller = caller;
            let memory = get_memory(&caller).ok_or(Error::Fault)?;
            let (vectors, nread_offset) = {
                let guest = borrow_memory(&memory, &mut caller);
                guest.validate_pointer(GuestPointer::<WasmUsize>::new(nread_ptr)).ok_or(Error::Fault)?;
                let vectors = guest.values(GuestSlice::<WasiVector>::new(iovs, iovs_len)).ok_or(Error::Fault)?;
                (vectors.collect::<Vec<_>>(), nread_ptr)
            };
            for vector in &vectors {
                borrow_memory(&memory, &mut caller)
                    .validate_slice(GuestSlice::<u8>::new(vector.buffer, vector.length)).ok_or(Error::Fault)?;
            }
            let mut buffers = Vec::with_capacity(vectors.len());
            let mut total = 0usize;
            for vector in &vectors {
                let length = usize::try_from(vector.length).map_err(|_| Error::Fault)?;
                let mut buffer = alloc::vec![0; length];
                let is_directory = caller.data().wasi.is_directory(fd as u32);
                let read = match caller.data_mut().wasi.get_synchronous_file(fd as u32) {
                    Some(file) => file.read(&mut buffer).map_err(vfs_error)?,
                    None if is_directory => return Err(Error::Isdir.into()),
                    None => return Err(Error::Badf.into()),
                };
                buffer.truncate(read);
                total = total.checked_add(read).ok_or(Error::Overflow)?;
                buffers.push((vector.buffer, buffer));
                if read < length { break; }
            }
            {
                let mut guest = borrow_memory(&memory, &mut caller);
                for (offset, buffer) in &buffers {
                    guest.bytes_mut(GuestSlice::new(*offset, buffer.len() as WasmUsize))
                        .ok_or(Error::Fault)?.copy_from_slice(buffer);
                }
                guest.write(GuestPointer::new(nread_offset), WasmUsize::try_from(total).map_err(|_| Error::Overflow)?)
                    .ok_or(Error::Fault)?;
            }
            Ok(())
        })
    }

    fn fd_write(caller: Caller<GlobalStore>, fd: i32, iovs: WasmUsize, iovs_len: WasmUsize, nwritten_ptr: WasmUsize) -> Result<WasiResult, wasmi::Error> {
        wrap_function!({
            let mut caller = caller;
            let memory = get_memory(&caller).ok_or(Error::Fault)?;
            let (buffers, nwritten_offset) = {
                let guest = borrow_memory(&memory, &mut caller);
                guest.validate_pointer(GuestPointer::<WasmUsize>::new(nwritten_ptr)).ok_or(Error::Fault)?;
                let vectors = guest.values(GuestSlice::<WasiVector>::new(iovs, iovs_len)).ok_or(Error::Fault)?;
                let mut buffers: Vec<Vec<u8>> = Vec::new();
                for vector in vectors {
                    let source = guest.bytes(GuestSlice::new(vector.buffer, vector.length)).ok_or(Error::Fault)?;
                    buffers.push(source.to_vec());
                }
                (buffers, nwritten_ptr)
            };
            let mut total = 0usize;
            for buffer in buffers {
                let is_directory = caller.data().wasi.is_directory(fd as u32);
                let written = match caller.data_mut().wasi.get_synchronous_file(fd as u32) {
                    Some(file) => file.write(&buffer).map_err(vfs_error)?,
                    None if is_directory => return Err(Error::Isdir.into()),
                    None => return Err(Error::Badf.into()),
                };
                total = total.checked_add(written).ok_or(Error::Overflow)?;
                if written < buffer.len() { break; }
            }
            borrow_memory(&memory, &mut caller)
                .write(GuestPointer::new(nwritten_offset), WasmUsize::try_from(total).map_err(|_| Error::Overflow)?)
                .ok_or(Error::Fault)?;
            Ok(())
        })
    }

    fn fd_control(caller: Caller<GlobalStore>, fd: i32, command: u32, input_ptr: WasmUsize, input_len: WasmUsize, output_ptr: WasmUsize, output_len: WasmUsize) -> Result<WasiResult, wasmi::Error> {
        wrap_function!({
            let mut caller = caller;
            let fd = u32::try_from(fd).map_err(|_| Error::Badf)?;
            let is_directory = caller.data().wasi.is_directory(fd);
            if caller.data_mut().wasi.get_synchronous_file(fd).is_none() {
                return Err(if is_directory { Error::Isdir } else { Error::Badf }.into());
            }

            let input_length = usize::try_from(input_len).map_err(|_| Error::Inval)?;
            let output_length = usize::try_from(output_len).map_err(|_| Error::Inval)?;
            if input_length > MAX_CONTROL_PAYLOAD || output_length > MAX_CONTROL_PAYLOAD {
                return Err(Error::Inval.into());
            }
            let input_start = usize::try_from(input_ptr).map_err(|_| Error::Fault)?;
            let output_start = usize::try_from(output_ptr).map_err(|_| Error::Fault)?;
            if input_start % CONTROL_PAYLOAD_ALIGNMENT != 0
                || output_start % CONTROL_PAYLOAD_ALIGNMENT != 0
            {
                return Err(Error::Inval.into());
            }
            if spans_overlap(input_start, input_length, output_start, output_length).ok_or(Error::Fault)? {
                return Err(Error::Inval.into());
            }

            let memory = get_memory(&caller).ok_or(Error::Fault)?;
            let input_slice = GuestSlice::<u8>::new(input_ptr, input_len);
            let output_slice = GuestSlice::<u8>::new(output_ptr, output_len);
            let mut input = AlignedControlPayload([0; MAX_CONTROL_PAYLOAD]);
            let mut output = AlignedControlPayload([0; MAX_CONTROL_PAYLOAD]);
            {
                let guest = borrow_memory(&memory, &mut caller);
                let guest_input = guest.bytes(input_slice).ok_or(Error::Fault)?;
                input.0[..input_length].copy_from_slice(guest_input);
                guest.validate_slice(output_slice).ok_or(Error::Fault)?;
            }

            // Both scratch buffers have 16-byte alignment. Their lengths are
            // bounded above, and they remain alive throughout the VFS call.
            let input_layout = unsafe {
                AnyByLayout::from_raw_parts(input.0.as_mut_ptr(), input_length)
            };
            let output_layout = unsafe {
                AnyByLayout::from_raw_parts(output.0.as_mut_ptr(), output_length)
            };
            caller.data_mut().wasi.get_synchronous_file(fd)
                .ok_or(Error::Badf)?
                .control_raw(ControlCommandIdentifier::from_raw(command), input_layout, output_layout)
                .map_err(vfs_error)?;

            borrow_memory(&memory, &mut caller)
                .bytes_mut(output_slice)
                .ok_or(Error::Fault)?
                .copy_from_slice(&output.0[..output_length]);
            Ok(())
        })
    }

    fn fd_close(caller: Caller<GlobalStore>, fd: i32) -> Result<WasiResult, wasmi::Error> {
        wrap_function!({
            let mut caller = caller;
            let item = caller.data_mut().wasi.pop_file_system_item(fd as u32).ok_or(Error::Badf)?;
            let vfs = caller.data().context.virtual_file_system;
            match item {
                FileSystemItem::File(file) | FileSystemItem::StandardInput(file) | FileSystemItem::StandardOutput(file) | FileSystemItem::StandardError(file) => file.file.close(vfs).map_err(vfs_error)?,
                FileSystemItem::Directory(dir) => dir.directory.close(vfs).map_err(vfs_error)?,
            }
            Ok(())
        })
    }

    fn fd_seek(caller: Caller<GlobalStore>, fd: i32, offset: i64, whence: i32, newoffset_ptr: WasmUsize) -> Result<WasiResult, wasmi::Error> {
        wrap_function!({
            let mut caller = caller;
            let memory = get_memory(&caller).ok_or(Error::Fault)?;
            let position = match whence { 0 if offset >= 0 => Position::Start(offset as u64), 0 => Err(Error::Inval)?, 1 => Position::Current(offset), 2 => Position::End(offset), _ => Err(Error::Inval)? };
            let is_directory = caller.data().wasi.is_directory(fd as u32);
            let new_position = match caller.data_mut().wasi.get_synchronous_file(fd as u32) {
                Some(file) => file.set_position(&position).map_err(vfs_error)?,
                None if is_directory => return Err(Error::Isdir.into()),
                None => return Err(Error::Badf.into()),
            };
            borrow_memory(&memory, &mut caller).write(GuestPointer::new(newoffset_ptr), new_position).ok_or(Error::Fault)?;
            Ok(())
        })
    }

    fn fd_fdstat_get(caller: Caller<GlobalStore>, fd: i32, stat_ptr: WasmUsize) -> Result<WasiResult, wasmi::Error> {
        wrap_function!({
            let mut caller = caller;
            let memory = get_memory(&caller).ok_or(Error::Fault)?;
            let filetype = caller.data_mut().wasi.get_file_system_item(fd as u32).ok_or(Error::Badf)?.file_type();
            borrow_memory(&memory, &mut caller).write(GuestPointer::new(stat_ptr),
                Fdstat { filetype, flags: 0, rights_base: u64::MAX, rights_inheriting: u64::MAX }).ok_or(Error::Fault)?;
            Ok(())
        })
    }

    fn fd_filestat_get(caller: Caller<GlobalStore>, fd: i32, stat_ptr: WasmUsize) -> Result<WasiResult, wasmi::Error> {
        wrap_function!({
            let mut caller = caller;
            let memory = get_memory(&caller).ok_or(Error::Fault)?;
            let (filetype, metadata) = {
                let item = caller.data_mut().wasi.get_file_system_item(fd as u32).ok_or(Error::Badf)?;
                let filetype = item.file_type();
                let metadata = match item {
                    FileSystemItem::File(FileVariant { file }) | FileSystemItem::StandardInput(FileVariant { file }) | FileSystemItem::StandardOutput(FileVariant { file }) | FileSystemItem::StandardError(FileVariant { file }) => file.get_statistics().map_err(vfs_error)?,
                    FileSystemItem::Directory(dir) => dir.directory.get_statistics().map_err(vfs_error)?,
                };
                (filetype, metadata)
            };
            borrow_memory(&memory, &mut caller).write(GuestPointer::new(stat_ptr),
                Filestat { dev: 0, ino: metadata.inode, filetype, nlink: metadata.links, size: metadata.size, atime: metadata.access.as_u64().saturating_mul(1_000_000_000), mtime: metadata.modification.as_u64().saturating_mul(1_000_000_000), ctime: metadata.creation.as_u64().saturating_mul(1_000_000_000) }).ok_or(Error::Fault)?;
            Ok(())
        })
    }

    fn fd_prestat_get(caller: Caller<GlobalStore>, fd: i32, prestat_ptr: WasmUsize) -> Result<WasiResult, wasmi::Error> {
        wrap_function!({
            let mut caller = caller;
            let name_len = u32::try_from(caller.data().wasi.prestats.get(fd.saturating_sub(3) as usize).filter(|_| fd >= 3).ok_or(Error::Badf)?.name.len()).map_err(|_| Error::Overflow)?;
            let memory = get_memory(&caller).ok_or(Error::Fault)?;
            borrow_memory(&memory, &mut caller).write(GuestPointer::new(prestat_ptr),
                PrestatAbi { tag: 0, dir: PrestatDir { pr_name_len: name_len } }).ok_or(Error::Fault)?;
            Ok(())
        })
    }

    fn fd_prestat_dir_name(caller: Caller<GlobalStore>, fd: i32, path_ptr: WasmUsize, path_len: WasmUsize) -> Result<WasiResult, wasmi::Error> {
        wrap_function!({
            let name = caller.data().wasi.prestats.get(fd.saturating_sub(3) as usize).filter(|_| fd >= 3).ok_or(Error::Badf)?.name.clone();
            if name.len() > path_len as usize { return Err(Error::Nametoolong.into()); }
            let mut caller = caller;
            let memory = get_memory(&caller).ok_or(Error::Fault)?;
            borrow_memory(&memory, &mut caller).bytes_mut(GuestSlice::new(path_ptr, path_len))
                .ok_or(Error::Fault)?[..name.len()].copy_from_slice(&name);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use alloc::{boxed::Box, vec};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wasmi::{Config, Engine, Linker, Module, Store};
    use xila::{
        file_system::{
            self, AccessFlags, ControlDirectionFlags, DirectBaseOperations, DirectCharacterDevice,
            Flags, MountOperations, Path, PathOwned, Size,
        },
        shared::AnyByLayout,
        virtual_file_system::{SynchronousDirectory, SynchronousFile},
    };

    const CONTROL: ControlCommandIdentifier =
        ControlCommandIdentifier::new_with_size(ControlDirectionFlags::Write, 8, b'X', 7);
    static CONTROL_CALLS: AtomicUsize = AtomicUsize::new(0);

    struct ControlDevice;

    impl MountOperations for ControlDevice {}
    impl DirectCharacterDevice for ControlDevice {}

    impl DirectBaseOperations for ControlDevice {
        fn read(&self, _: &mut [u8], _: Size) -> file_system::Result<usize> {
            Err(file_system::Error::UnsupportedOperation)
        }

        fn write(&self, buffer: &[u8], _: Size) -> file_system::Result<usize> {
            Ok(buffer.len())
        }

        fn control(
            &self,
            command: ControlCommandIdentifier,
            input: &AnyByLayout,
            output: &mut AnyByLayout,
        ) -> file_system::Result<()> {
            CONTROL_CALLS.fetch_add(1, Ordering::SeqCst);
            if command != CONTROL {
                return Err(file_system::Error::UnsupportedOperation);
            }
            let input = input
                .cast::<u32>()
                .ok_or(file_system::Error::InvalidParameter)?;
            let output = output
                .cast_mutable::<u32>()
                .ok_or(file_system::Error::InvalidParameter)?;
            *output = input.wrapping_add(1);
            Ok(())
        }
    }

    #[xila::task::test(task_path = xila::task)]
    async fn fd_control_forwards_opaque_command_and_payloads() {
        drivers_std::memory::instantiate_global_allocator!();

        xila::log::initialize(&drivers_std::log::Logger).unwrap();
        let task_manager = xila::task::test_manager();
        let users_manager = Box::leak(Box::new(xila::users::Manager::new()));
        let time_manager = Box::leak(Box::new(
            xila::time::Manager::new(&drivers_std::devices::TimeDevice).unwrap(),
        ));
        let memory_device = xila::file_system::MemoryDevice::<512>::new_static(1024 * 512);
        let filesystem = xila::little_fs::FileSystem::new_format(memory_device, 256).unwrap();
        let vfs = Box::leak(Box::new(
            xila::virtual_file_system::initialize(
                task_manager,
                users_manager,
                time_manager,
                filesystem,
            )
            .unwrap(),
        ));
        let task = task_manager.get_current_task_identifier().await;
        xila::virtual_file_system::create_default_hierarchy(vfs, task)
            .await
            .unwrap();
        let context = Box::leak(Box::new(xila::executable::ExecutableContext {
            task_manager,
            users_manager,
            virtual_file_system: vfs,
            graphics_manager: None,
            network_manager: None,
            time_manager,
        }));
        vfs.mount_character_device(task, Path::from_str("/fd-control"), ControlDevice)
            .await
            .unwrap();
        let file = SynchronousFile::open(
            vfs,
            task,
            Path::from_str("/fd-control"),
            Flags::new(AccessFlags::Read, None, None),
        )
        .unwrap();
        let directory = SynchronousDirectory::open(vfs, task, Path::from_str("/")).unwrap();

        let mut config = Config::default();
        config.consume_fuel(true);
        let engine = Engine::new(&config);
        let mut linker = Linker::new(&engine);
        crate::host::wasi::register::add_wasi_to_linker(&mut linker).unwrap();
        let wasm = wat::parse_str(
            r#"(module
                (import "wasi_snapshot_preview1" "fd_control"
                    (func $control (param i32 i32 i32 i32 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (func (export "run") (param i32 i32 i32 i32 i32 i32) (result i32)
                    i32.const 32 i32.const 42 i32.store
                    local.get 0 local.get 1 local.get 2 local.get 3 local.get 4 local.get 5
                    call $control)
            )"#,
        )
        .unwrap();
        let module = Module::new(&engine, &wasm[..]).unwrap();
        let mut wasi = crate::host::wasi::WasiContext {
            files: Default::default(),
            next_fd: 5,
            arguments: vec![],
            environment: vec![],
            task,
            prestats: vec![],
            exit_code: None,
        };
        wasi.files
            .insert(4, FileSystemItem::File(FileVariant { file }));
        wasi.files.insert(
            5,
            FileSystemItem::Directory(crate::host::wasi::context::DirectoryVariant {
                path: PathOwned::root(),
                directory,
            }),
        );
        let mut store = Store::new(
            &engine,
            GlobalStore {
                context,
                pending: None,
                wasi,
                #[cfg(feature = "graphics")]
                graphics: crate::host::graphics::GraphicsState::new(context.graphics_manager),
            },
        );
        store.set_fuel(10_000).unwrap();
        let instance = linker.instantiate_and_start(&mut store, &module).unwrap();
        let run = instance
            .get_typed_func::<(i32, i32, i32, i32, i32, i32), i32>(&store, "run")
            .unwrap();
        CONTROL_CALLS.store(0, Ordering::SeqCst);
        assert_eq!(
            run.call(&mut store, (4, CONTROL.raw() as i32, 32, 4, 48, 4))
                .unwrap(),
            0
        );
        let memory = instance.get_memory(&store, "memory").unwrap();
        assert_eq!(&memory.data(&store)[48..52], &43u32.to_le_bytes());
        assert_eq!(CONTROL_CALLS.load(Ordering::SeqCst), 1);

        assert_eq!(
            run.call(&mut store, (-1, CONTROL.raw() as i32, 32, 4, 48, 4))
                .unwrap(),
            8
        );
        assert_eq!(
            run.call(&mut store, (99, CONTROL.raw() as i32, 32, 4, 48, 4))
                .unwrap(),
            8
        );
        assert_eq!(
            run.call(&mut store, (5, CONTROL.raw() as i32, 32, 4, 48, 4))
                .unwrap(),
            31
        );
        assert_eq!(
            run.call(&mut store, (4, CONTROL.raw() as i32, 65_536, 4, 48, 4))
                .unwrap(),
            21
        );
        assert_eq!(
            run.call(&mut store, (4, CONTROL.raw() as i32, 32, 4, 65_536, 4))
                .unwrap(),
            21
        );
        assert_eq!(
            run.call(&mut store, (4, CONTROL.raw() as i32, 32, 257, 48, 4))
                .unwrap(),
            28
        );
        assert_eq!(
            run.call(&mut store, (4, CONTROL.raw() as i32, 32, 4, 48, 257))
                .unwrap(),
            28
        );
        assert_eq!(
            run.call(&mut store, (4, CONTROL.raw() as i32, 33, 4, 48, 4))
                .unwrap(),
            28
        );
        assert_eq!(
            run.call(&mut store, (4, CONTROL.raw() as i32, 32, 4, 49, 4))
                .unwrap(),
            28
        );
        assert_eq!(
            run.call(&mut store, (4, CONTROL.raw() as i32, 32, 4, 32, 4))
                .unwrap(),
            28
        );
        let unsupported =
            ControlCommandIdentifier::new_with_size(ControlDirectionFlags::Write, 8, b'Z', 7).raw()
                as i32;
        assert_eq!(
            run.call(&mut store, (4, unsupported, 32, 4, 48, 4))
                .unwrap(),
            29
        );
        assert_eq!(CONTROL_CALLS.load(Ordering::SeqCst), 2);
        assert_eq!(&memory.data(&store)[48..52], &43u32.to_le_bytes());
    }

    #[test]
    fn raw_command_word_preserves_identifier_bits() {
        assert_eq!(ControlCommandIdentifier::from_raw(CONTROL.raw()), CONTROL);
    }
}
