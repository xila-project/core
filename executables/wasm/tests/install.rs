#![cfg(feature = "host")]
extern crate abi_definitions;
drivers_std::memory::instantiate_global_allocator!();

use xila::{
    executable::{self, Standard},
    virtual_file_system::File,
};

async fn execute(standard: &Standard<'static>, arguments: Vec<String>) -> isize {
    executable::execute(
        standard.context,
        "/binaries/wasm",
        arguments,
        standard.duplicate().await.unwrap(),
        None,
    )
    .await
    .unwrap()
    .join()
    .await
}

#[xila::task::test(task_path = xila::task)]
async fn install_selects_install_export_and_never_falls_back_to_main() {
    let standard = testing::initialize(false, false).await;
    let context = standard.context;
    let task = context.task_manager.get_current_task_identifier().await;
    executable::mount_executables!(
        context,
        context.virtual_file_system,
        task,
        &[("/binaries/wasm", wasm::WasmExecutable)]
    )
    .await
    .unwrap();
    let path = "/binaries/install-fixture.wasm";
    let import = r#"(import "wasi_snapshot_preview1" "proc_exit" (func $exit (param i32)))"#;
    let expensive_callee = format!(
        "(func $work {}) (func (export \"_start\") call $work)",
        "i32.const 42 drop ".repeat(256)
    );
    // Exit codes expose which export actually executed through the whole CLI.
    for (index, (exports, arguments, expected)) in [
        (
            r#"(func (export "_start") i32.const 7 call $exit)
             (func (export "__install") i32.const 0 call $exit)"#,
            vec!["--install".into(), path.into()],
            0,
        ),
        (
            r#"(func (export "_start") i32.const 7 call $exit)
             (func (export "__install") i32.const 0 call $exit)"#,
            vec![path.into(), "--install".into()],
            0,
        ),
        (
            r#"(func (export "_start") i32.const 0 call $exit)
             (func (export "__install") i32.const 7 call $exit)"#,
            vec![path.into()],
            0,
        ),
        (
            r#"(func (export "_start") i32.const 0 call $exit)
             (func (export "__install") i32.const 7 call $exit)"#,
            vec!["--install".into(), path.into()],
            -7,
        ),
        (
            r#"(func (export "_start") i32.const 0 call $exit)"#,
            vec!["--install".into(), path.into()],
            -1,
        ),
        (
            r#"(func (export "_start") i32.const 0 call $exit)
             (func (export "__install") (param i32))"#,
            vec!["--install".into(), path.into()],
            -1,
        ),
        (
            r#"(func (export "__install"))"#,
            vec!["--install".into(), path.into()],
            0,
        ),
        // The entry fits within the fuel budget, but lazy translation of its
        // callee would exhaust fuel after execution has already begun.
        (
            expensive_callee.as_str(),
            vec!["--instruction-limit".into(), "1000".into(), path.into()],
            0,
        ),
        // Even a one-fuel slice must reach and resume guest functions whose
        // translation costs exceed the slice budget.
        (
            r#"(func $work i32.const 42 drop)
             (func (export "_start") call $work)"#,
            vec!["--instruction-limit".into(), "1".into(), path.into()],
            0,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let binary = wat::parse_str(format!("(module {import} {exports})")).unwrap();
        let fixture_path = format!("/binaries/install-fixture-{index}.wasm");
        let arguments = arguments
            .into_iter()
            .map(|argument: String| {
                if argument == path {
                    fixture_path.clone()
                } else {
                    argument
                }
            })
            .collect();
        File::write_to_path(
            context.virtual_file_system,
            task,
            fixture_path.as_str(),
            &binary,
        )
        .await
        .unwrap();
        assert_eq!(
            execute(&standard, arguments).await,
            expected,
            "exports: {exports}"
        );
    }

    #[cfg(all(
        feature = "graphics",
        feature = "memory_32",
        not(feature = "memory_64")
    ))]
    {
        assert!(context.graphics_manager.is_none());
        context
            .virtual_file_system
            .create_directory(task, &"/configuration/shared/shortcuts")
            .await
            .unwrap();
        for (app, name) in [
            ("calculator", "Calculator"),
            ("weather", "Weather"),
            ("clock", "Clock"),
            ("synth", "Synth"),
            ("text_editor", "Text Editor"),
        ] {
            let binary = executable::build_crate(app).unwrap();
            let path = format!("/binaries/{app}");
            drivers_std::loader::load_to_virtual_file_system(
                context.task_manager,
                context.virtual_file_system,
                binary,
                path.as_str(),
            )
            .await
            .unwrap();
            let exit = execute(&standard, vec!["--install".into(), path.clone()]).await;
            context
                .virtual_file_system
                .remove(task, path.as_str())
                .await
                .unwrap();
            assert_eq!(exit, 0, "{app}");
            let mut contents = Vec::new();
            let shortcut_path = format!("/configuration/shared/shortcuts/{app}.json");
            File::read_from_path(
                context.virtual_file_system,
                task,
                shortcut_path.as_str(),
                &mut contents,
            )
            .await
            .unwrap();
            let shortcut = core::str::from_utf8(&contents).unwrap();
            assert!(shortcut.contains(&format!(r#""name": "{name}""#)));
            assert!(shortcut.contains(r#""command": "/binaries/wasm""#));
            assert!(shortcut.contains(&format!(r#""arguments": ["{path}"]"#)));
        }
    }
}
