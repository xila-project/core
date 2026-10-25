use wasmi::Caller;

use crate::{
    define_wasi_module,
    host::{
        store::GlobalStore,
        translation::{GuestSlice, WasmUsize, borrow_memory, get_memory},
        wasi::{
            Error,
            error::{WasiResult, wrap_function},
        },
    },
};

define_wasi_module! {
    module: "wasi_snapshot_preview1";

    fn random_get(
        caller: Caller<GlobalStore>,
        buf_ptr: WasmUsize,
        buf_len: WasmUsize,
    ) -> Result<WasiResult, wasmi::Error> {
        wrap_function!({
            let mut caller = caller;
            let memory = get_memory(&caller).ok_or(Error::Fault)?;
            borrow_memory(&memory, &mut caller)
                .validate_slice(GuestSlice::<u8>::new(buf_ptr, buf_len)).ok_or(Error::Fault)?;
            let mut random_bytes = alloc::vec![0; usize::try_from(buf_len).map_err(|_| Error::Fault)?];
            let random = xila::virtual_file_system::SynchronousFile::open(
                caller.data().context.virtual_file_system,
                caller.data().wasi.task,
                xila::file_system::Path::from_str("/devices/random/source"),
                xila::file_system::AccessFlags::Read.into(),
            ).map_err(|_| Error::Io)?;
            let mut random = random;
            let read_result = random.read(&mut random_bytes);
            random.close(caller.data().context.virtual_file_system).map_err(|_| Error::Io)?;
            let read = read_result.map_err(|_| Error::Io)?;
            if read != random_bytes.len() { return Err(Error::Io.into()); }
            borrow_memory(&memory, &mut caller).bytes_mut(GuestSlice::new(buf_ptr, buf_len))
                .ok_or(Error::Fault)?.copy_from_slice(&random_bytes);

            Ok(())
        })
    }
}
