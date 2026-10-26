#![cfg(all(feature = "host", feature = "graphics"))]
extern crate abi_definitions;
drivers_std::memory::instantiate_global_allocator!();
#[path = "support/graphics.rs"]
mod support;

use wasm::{
    GlobalStore, WasiContext,
    graphics::{Function, GraphicsState, wasmi::add_to_linker},
};
use wasmi::{Engine, Linker, Module, Store};

async fn store(
    context: &'static xila::executable::ExecutableContext,
    engine: &Engine,
) -> Store<GlobalStore> {
    Store::new(
        engine,
        GlobalStore {
            context,
            pending: None,
            graphics: GraphicsState::new(context.graphics_manager),
            wasi: WasiContext {
                files: Default::default(),
                next_fd: 3,
                arguments: vec![],
                environment: vec![],
                task: context.task_manager.get_current_task_identifier().await,
                prestats: vec![],
                exit_code: None,
            },
        },
    )
}

#[xila::task::test(task_path = xila::task)]
async fn wasmi_adapter_uses_context_and_checked_exports() {
    let standard = testing::initialize(false, false).await;
    let manager = support::manager().await;
    let context = support::context(&standard, Some(manager));
    let engine = Engine::default();
    let mut linker = Linker::new(&engine);
    add_to_linker(&mut linker).unwrap();
    let word = if cfg!(feature = "memory_64") {
        "i64"
    } else {
        "i32"
    };
    let import = format!(
        r#"(import "host" "xila_graphics_call" (func $call (param i32 {word} {word} {word} {word} {word} {word} {word} i32 {word}) (result i32)))"#
    );
    for export in [
        "",
        "(global (export \"memory\") i32 (i32.const 1))",
        "(memory (export \"memory\") 1)",
    ] {
        let wasm = wat::parse_str(format!(r#"(module {import} {export}
            (func (export "test") (result i32)
                i32.const {} {word}.const 0 {word}.const 0 {word}.const 0 {word}.const 0
                {word}.const 0 {word}.const 0 {word}.const 0 i32.const 0 {word}.const 1 call $call))"#, Function::WindowCreate.id())).unwrap();
        let module = Module::new(&engine, &wasm[..]).unwrap();
        let mut store = store(context, &engine).await;
        let instance = linker.instantiate_and_start(&mut store, &module).unwrap();
        let result = instance
            .get_typed_func::<(), i32>(&store, "test")
            .unwrap()
            .call(&mut store, ())
            .unwrap();
        assert_eq!(result, if export.starts_with("(memory") { 0 } else { 2 });
        drop(store);
        assert_eq!(manager.get_window_count().await.unwrap(), 0);
    }
    let missing = support::context(&standard, None);
    let wasm = wat::parse_str(format!(r#"(module {import} (memory (export "memory") 1)
        (func (export "test") (result i32) i32.const {} {word}.const 0 {word}.const 0
        {word}.const 0 {word}.const 0 {word}.const 0 {word}.const 0 {word}.const 0 i32.const 0 {word}.const 1 call $call))"#,Function::WindowCreate.id())).unwrap();
    let module = Module::new(&engine, &wasm[..]).unwrap();
    let mut store = store(missing, &engine).await;
    let instance = linker.instantiate_and_start(&mut store, &module).unwrap();
    assert_eq!(
        instance
            .get_typed_func::<(), i32>(&store, "test")
            .unwrap()
            .call(&mut store, ())
            .unwrap(),
        3
    );
    // A manually supplied graphics state cannot override the VM's capability.
    store.data_mut().graphics = GraphicsState::new(Some(manager));
    assert_eq!(
        instance
            .get_typed_func::<(), i32>(&store, "test")
            .unwrap()
            .call(&mut store, ())
            .unwrap(),
        3
    );
    store.data_mut().graphics = GraphicsState::new(None);

    let wasm = wat::parse_str(format!(r#"(module {import} (memory (export "memory") 1)
        (func (export "invoke") (param i32 {word} {word} {word} {word} {word} {word} {word} i32 {word}) (result i32)
            local.get 0 local.get 1 local.get 2 local.get 3 local.get 4 local.get 5 local.get 6 local.get 7 local.get 8 local.get 9 call $call)
        (func (export "grow") (result i32) i32.const 1 memory.grow))"#)).unwrap();
    let module = Module::new(&engine, &wasm[..]).unwrap();
    let mut store = self::store(context, &engine).await;
    let instance = linker.instantiate_and_start(&mut store, &module).unwrap();
    let invoke = instance
        .get_typed_func::<(
            i32,
            wasm::translation::WasmUsize,
            wasm::translation::WasmUsize,
            wasm::translation::WasmUsize,
            wasm::translation::WasmUsize,
            wasm::translation::WasmUsize,
            wasm::translation::WasmUsize,
            wasm::translation::WasmUsize,
            i32,
            wasm::translation::WasmUsize,
        ), i32>(&store, "invoke")
        .unwrap();
    let memory = instance.get_memory(&store, "memory").unwrap();
    assert_eq!(
        invoke
            .call(
                &mut store,
                (Function::WindowCreate.id(), 0, 0, 0, 0, 0, 0, 0, 0, 1)
            )
            .unwrap(),
        0
    );
    assert_eq!(
        invoke
            .call(
                &mut store,
                (Function::LabelCreate.id(), 1, 0, 0, 0, 0, 0, 0, 1, 1)
            )
            .unwrap(),
        0
    );
    assert_eq!(
        invoke
            .call(
                &mut store,
                (Function::ButtonmatrixCreate.id(), 1, 0, 0, 0, 0, 0, 0, 1, 1)
            )
            .unwrap(),
        0
    );
    {
        use wasm::translation::{GuestMemory, GuestPointer, GuestValue, WasmUsize};
        let mut view = GuestMemory::new(memory.data_mut(&mut store));
        view.write(GuestPointer::new(300), 301 as WasmUsize)
            .unwrap();
        // Use a disjoint table; the string itself remains byte encoded.
        view.write(GuestPointer::new(400), 600 as WasmUsize)
            .unwrap();
        view.write(
            GuestPointer::new(400 + WasmUsize::SIZE as WasmUsize),
            0 as WasmUsize,
        )
        .unwrap();
        for (i, b) in b"Test\0".iter().enumerate() {
            view.write(GuestPointer::new(600 + i as WasmUsize), *b)
                .unwrap();
        }
    }
    assert_eq!(
        invoke
            .call(
                &mut store,
                (Function::LabelSetText.id(), 2, 600, 0, 0, 0, 0, 0, 2, 0)
            )
            .unwrap(),
        0
    );
    assert_eq!(
        invoke
            .call(
                &mut store,
                (
                    Function::ButtonmatrixSetMap.id(),
                    3,
                    400,
                    0,
                    0,
                    0,
                    0,
                    0,
                    2,
                    0
                )
            )
            .unwrap(),
        0
    );
    assert_eq!(
        instance
            .get_typed_func::<(), i32>(&store, "grow")
            .unwrap()
            .call(&mut store, ())
            .unwrap(),
        1
    );
    memory.data_mut(&mut store)[600..605].fill(0xff);
    {
        use xila::graphics::lvgl;
        let root = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
        let _lock = manager.synchronous_lock();
        unsafe {
            let label = lvgl::lv_obj_get_child(root, 0);
            assert_eq!(
                core::ffi::CStr::from_ptr(lvgl::lv_label_get_text(label)).to_bytes(),
                b"Test"
            );
            let matrix = lvgl::lv_obj_get_child(root, 1);
            assert_eq!(
                core::ffi::CStr::from_ptr(*lvgl::lv_buttonmatrix_get_map(matrix)).to_bytes(),
                b"Test"
            );
        }
    }
    // GraphicsState::new has no filesystem context; the Wasmi boundary must
    // capture the trusted guest's task/VFS rather than the graphics worker.
    memory.data_mut(&mut store)[700..702].copy_from_slice(b"/\0");
    assert_eq!(
        invoke
            .call(
                &mut store,
                (
                    Function::FilePickerCreate.id(),
                    1,
                    2,
                    700,
                    0,
                    0,
                    0,
                    0,
                    3,
                    800
                )
            )
            .unwrap(),
        0
    );
    let picker = {
        use wasm::translation::{GuestMemory, GuestPointer, WasmUsize};
        GuestMemory::new(memory.data_mut(&mut store))
            .read::<WasmUsize>(GuestPointer::new(800))
            .unwrap()
    };
    assert_eq!(
        invoke
            .call(
                &mut store,
                (
                    Function::FilePickerGetStatus.id(),
                    picker,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    1,
                    820
                )
            )
            .unwrap(),
        0
    );
    assert_eq!(&memory.data(&store)[820..824], &[0, 0, 0, 0]);
    drop(store);
    assert_eq!(manager.get_window_count().await.unwrap(), 0);
}
