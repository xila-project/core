use wasmi::Caller;

use crate::{
    define_wasi_module,
    host::{
        store::GlobalStore,
        translation::{GuestPointer, WasmUsize, borrow_memory, get_memory},
        wasi::error::WasiResult,
    },
};

define_wasi_module! {
    module: "wasi_snapshot_preview1";

    fn clock_res_get(
        caller: Caller<GlobalStore>,
        clock_id: i32,
        resolution_ptr: WasmUsize,
    ) -> Result<WasiResult, wasmi::Error> {
        let mut caller = caller;
        let memory = get_memory(&caller).ok_or_else(|| wasmi::Error::new("missing memory"))?;
        let resolution = match clock_id {
            // The wall-clock device API does not expose finer resolution.
            0 => 1_000_000u64,
            1 => xila::time::MONOTONIC_RESOLUTION.as_nanos() as u64,
            _ => return Ok(58),
        };
        borrow_memory(&memory, &mut caller)
            .write(GuestPointer::new(resolution_ptr), resolution)
            .ok_or_else(|| wasmi::Error::new("invalid output pointer"))?;
        Ok(0)
    }

    fn clock_time_get(
        caller: Caller<GlobalStore>,
        clock_id: i32,
        _precision: i64,
        time_ptr: WasmUsize,
    ) -> Result<WasiResult, wasmi::Error> {
        let mut caller = caller;
        let memory = get_memory(&caller).ok_or_else(|| wasmi::Error::new("missing memory"))?;
        let manager=caller.data().context.time_manager;
        let now = match clock_id {
            0=>u64::try_from(manager.realtime_now().map_err(|_| wasmi::Error::new("time error"))?.as_nanos()).map_err(|_| wasmi::Error::new("clock overflow"))?,
            1=>crate::host::execution::monotonic_now(manager)?,
            _=>return Ok(58),
        };
        borrow_memory(&memory, &mut caller)
            .write(GuestPointer::new(time_ptr), now)
            .ok_or_else(|| wasmi::Error::new("invalid output pointer"))?;
        Ok(0)
    }
}
