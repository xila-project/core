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
        let manager = caller.data().context.time_manager;
        let resolution = match clock_id {
            0 => manager.realtime_resolution(),
            1 => manager.monotonic_resolution(),
            _ => return Ok(58),
        };
        let Ok(resolution) = u64::try_from(resolution.as_nanos()) else {
            return Ok(61);
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
        let manager = caller.data().context.time_manager;
        let now = match crate::host::execution::clock_value(manager, clock_id as usize) {
            Ok(now) => now,
            Err(error) => return Ok(error as i32),
        };
        borrow_memory(&memory, &mut caller)
            .write(GuestPointer::new(time_ptr), now)
            .ok_or_else(|| wasmi::Error::new("invalid output pointer"))?;
        Ok(0)
    }
}

#[cfg(all(test, feature = "memory_32"))]
mod tests {
    extern crate std;
    use super::*;
    use alloc::{boxed::Box, vec};
    use core::time::Duration;
    use wasmi::{Config, Engine, Linker, Module, Store};

    struct FixedClock;
    impl xila::time::MonotonicClock for FixedClock {
        fn now(&self) -> Duration {
            Duration::ZERO
        }
    }

    #[xila::task::test(task_path = xila::task)]
    async fn clock_imports_return_errno_and_keep_output_on_invalid_utc() {
        let task_manager = xila::task::test_manager();
        let users_manager = Box::leak(Box::new(xila::users::Manager::new()));
        let time_manager = Box::leak(Box::new(xila::time::Manager::with_clock(&FixedClock)));
        let disk = xila::file_system::MemoryDevice::<512>::new_static(1024 * 512);
        let filesystem = xila::little_fs::FileSystem::new_format(disk, 256).unwrap();
        let vfs = Box::leak(Box::new(xila::virtual_file_system::VirtualFileSystem::new(
            task_manager,
            users_manager,
            time_manager,
            filesystem,
        )));
        let context = Box::leak(Box::new(xila::executable::ExecutableContext {
            task_manager,
            users_manager,
            time_manager,
            virtual_file_system: vfs,
            graphics_manager: None,
            network_manager: None,
        }));
        let mut config = Config::default();
        config.consume_fuel(true);
        let engine = Engine::new(&config);
        let mut linker = Linker::new(&engine);
        crate::host::wasi::register::add_wasi_to_linker(&mut linker).unwrap();
        let bytes = wat::parse_str(r#"(module
            (import "wasi_snapshot_preview1" "clock_time_get" (func $time (param i32 i64 i32) (result i32)))
            (import "wasi_snapshot_preview1" "clock_res_get" (func $res (param i32 i32) (result i32)))
            (memory (export "memory") 1)
            (func (export "time") (param i32) (result i32) local.get 0 i64.const 0 i32.const 8 call $time)
            (func (export "res") (param i32) (result i32) local.get 0 i32.const 8 call $res))"#).unwrap();
        let module = Module::new(&engine, &bytes[..]).unwrap();
        let mut store = Store::new(
            &engine,
            GlobalStore {
                context,
                pending: None,
                wasi: crate::host::wasi::WasiContext {
                    files: Default::default(),
                    next_fd: 3,
                    arguments: vec![],
                    environment: vec![],
                    task: task_manager.get_current_task_identifier().await,
                    prestats: vec![],
                    exit_code: None,
                },
                #[cfg(feature = "graphics")]
                graphics: crate::host::graphics::GraphicsState::new(None),
            },
        );
        store.set_fuel(100_000).unwrap();
        let instance = linker.instantiate_and_start(&mut store, &module).unwrap();
        let time = instance.get_typed_func::<i32, i32>(&store, "time").unwrap();
        let res = instance.get_typed_func::<i32, i32>(&store, "res").unwrap();
        let memory = instance.get_memory(&store, "memory").unwrap();
        memory.write(&mut store, 8, &42u64.to_le_bytes()).unwrap();
        assert_eq!(time.call(&mut store, 0).unwrap(), 29);
        assert_eq!(&memory.data(&store)[8..16], &42u64.to_le_bytes());
        assert_eq!(time.call(&mut store, 1).unwrap(), 0);
        assert_eq!(time.call(&mut store, 2).unwrap(), 58);
        for nanos in [-1_000_000_000, u64::MAX as i128 + 10_000_000_000] {
            time_manager
                .set_realtime(xila::time::RealtimeSample {
                    timestamp: xila::time::Timestamp::from_nanos(nanos),
                    source: xila::time::ClockSource::Manual,
                    resolution: Duration::from_secs(1),
                    accuracy: None,
                })
                .unwrap();
            memory.write(&mut store, 8, &42u64.to_le_bytes()).unwrap();
            assert_eq!(time.call(&mut store, 0).unwrap(), 61);
            assert_eq!(&memory.data(&store)[8..16], &42u64.to_le_bytes());
        }
        time_manager
            .set_realtime(xila::time::RealtimeSample {
                timestamp: xila::time::Timestamp::from_nanos(1_234_567_890_123),
                source: xila::time::ClockSource::Manual,
                resolution: Duration::from_secs(1),
                accuracy: None,
            })
            .unwrap();
        assert_eq!(time.call(&mut store, 0).unwrap(), 0);
        assert_eq!(
            &memory.data(&store)[8..16],
            &1_234_567_890_123u64.to_le_bytes()
        );
        assert_eq!(res.call(&mut store, 0).unwrap(), 0);
        assert_eq!(&memory.data(&store)[8..16], &1_000_000_000u64.to_le_bytes());
        assert_eq!(res.call(&mut store, 1).unwrap(), 0);
        assert_eq!(
            &memory.data(&store)[8..16],
            &(time_manager.monotonic_resolution().as_nanos() as u64).to_le_bytes()
        );
    }
}
