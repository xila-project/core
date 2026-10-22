use core::num::{NonZeroU32, NonZeroUsize};

use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use core::fmt::Write;
use getargs_derive::GetArgs;
use wasmi::{Caller, CompilationMode, Config, Engine, Linker, Module, Store};
use xila::{
    executable::{ExecutableContext, Standard},
    file_system::{Kind, Path},
    log,
    virtual_file_system::File,
};

use crate::host::{
    error::{Error, Result},
    store::GlobalStore,
    wasi::{self, DirectoryVariant, FileSystemItem, FileVariant, Prestat, WasiContext},
};

const DEFAULT_STACK_SIZE: usize = 4096;
const START_FUNCTION_NAME: &str = "_start";
const INSTALL_FUNCTION_NAME: &str = "__install";

#[cfg(test)]
mod parser_tests {
    use super::{WasmArguments, split_guest_arguments};
    use alloc::{
        string::{String, ToString},
        vec,
    };

    #[test]
    fn defaults_and_install_flag_preserve_existing_behavior() {
        let parsed = WasmArguments::from_args(["app.wasm"].into_iter()).unwrap();
        assert_eq!(parsed.path, "app.wasm");
        assert!(!parsed.install);
        assert_eq!(parsed.stack_size, 4096);
        assert_eq!(parsed.instruction_limit.get(), 100_000);
        let parsed = WasmArguments::from_args(["--install", "app.wasm"].into_iter()).unwrap();
        assert!(parsed.install);
    }

    #[test]
    fn preserves_named_limits_and_inferred_short_flags() {
        for stack in ["--stack-size", "-s"] {
            let parsed = WasmArguments::from_args(
                [stack, "8192", "--instruction-limit", "42", "app.wasm"].into_iter(),
            )
            .unwrap();
            assert_eq!(parsed.stack_size, 8192);
            assert_eq!(parsed.instruction_limit.get(), 42);
        }
        let parsed = WasmArguments::from_args(["-i", "app.wasm"].into_iter()).unwrap();
        assert!(parsed.install);
    }

    #[test]
    fn rejects_invalid_instruction_limit() {
        for value in ["0", "many", "4294967296"] {
            let error =
                WasmArguments::from_args(["--instruction-limit", value, "app.wasm"].into_iter())
                    .err()
                    .unwrap();
            assert!(
                matches!(error, getargs_derive::Error::InvalidValue { value: actual, .. } if actual == value)
            );
        }
    }

    #[test]
    fn generated_help_and_version_do_not_require_a_path() {
        let error = WasmArguments::from_args(["--help"].into_iter())
            .err()
            .unwrap();
        assert!(error.is_help());
        assert!(error.to_string().contains("Usage: wasm"));
        let error = WasmArguments::from_args(["--version"].into_iter())
            .err()
            .unwrap();
        assert!(error.is_version());
    }

    #[test]
    fn misspelled_install_gets_a_suggestion() {
        let error = WasmArguments::from_args(["--instal", "app.wasm"].into_iter())
            .err()
            .unwrap();
        assert!(
            error
                .to_string()
                .contains("similar argument exists: '--install'")
        );
    }

    #[test]
    fn separates_guest_arguments_from_wasm_runner_options() {
        let arguments = vec![
            "--instruction-limit".to_string(),
            "42".to_string(),
            "/application/app.wasm".to_string(),
            "--".to_string(),
            "first".to_string(),
            "--guest-option".to_string(),
        ];
        let (runner, guest) = split_guest_arguments(&arguments);
        assert_eq!(runner.len(), 3);
        assert_eq!(
            guest,
            &[String::from("first"), String::from("--guest-option")]
        );
        let parsed =
            WasmArguments::from_args(runner.iter().map(|argument| argument.as_str())).unwrap();
        assert_eq!(parsed.path, "/application/app.wasm");
    }

    #[test]
    fn guest_exit_status_preserves_representable_codes() {
        assert!(super::guest_exit_status(0).is_ok());
        assert_eq!(super::guest_exit_status(7).unwrap_err().get(), 7);
        assert_eq!(super::guest_exit_status(300).unwrap_err().get(), 255);
    }
}

#[derive(GetArgs)]
#[command(
    name = "wasm",
    about = "Run or install a WebAssembly executable.",
    version
)]
struct WasmArguments<'a> {
    /// Path to the executable in the Xila filesystem.
    path: &'a str,
    /// Run the install entry point instead of the main entry point.
    #[arg(short = 'i', long)]
    install: bool,
    /// Legacy stack-size setting; currently managed by wasmi rather than applied here.
    #[arg(short = 's', long, default_value_t = DEFAULT_STACK_SIZE)]
    stack_size: usize,
    /// Maximum number of instructions per execution slice.
    #[arg(long, default_value_t = NonZeroU32::new(100_000).unwrap())]
    instruction_limit: NonZeroU32,
}

pub async fn main(
    mut standard: Standard<'static>,
    arguments: Vec<String>,
) -> core::result::Result<(), NonZeroUsize> {
    let (runtime_arguments, guest_arguments) = split_guest_arguments(&arguments);
    let arguments = match WasmArguments::from_args(runtime_arguments.iter().map(|s| s.as_str())) {
        Ok(args) => args,
        Err(error) if error.is_help() || error.is_version() => {
            writeln!(standard.out(), "{error}").ok();
            return Ok(());
        }
        Err(e) => {
            writeln!(standard.error(), "Error parsing arguments: {}", e).unwrap();
            return Err(NonZeroUsize::new(1).unwrap());
        }
    };

    match main_inner(
        standard.duplicate().await.unwrap(),
        arguments,
        guest_arguments.to_vec(),
    )
    .await
    {
        Ok(()) | Err(Error::Runtime(0)) => Ok(()),
        Err(Error::Runtime(code)) => guest_exit_status(code),
        Err(Error::Interrupted) => Err(NonZeroUsize::new(1).unwrap()),
        Err(error) => {
            writeln!(standard.error(), "Error: {:?}", error).unwrap();
            Err(NonZeroUsize::new(1).unwrap())
        }
    }
}

fn guest_exit_status(code: i32) -> core::result::Result<(), NonZeroUsize> {
    if code == 0 {
        Ok(())
    } else {
        Err(NonZeroUsize::new(code.clamp(1, 255) as usize).unwrap())
    }
}

fn split_guest_arguments(arguments: &[String]) -> (&[String], &[String]) {
    match arguments.iter().position(|argument| argument == "--") {
        Some(separator) => (&arguments[..separator], &arguments[separator + 1..]),
        None => (arguments, &[]),
    }
}

async fn read_file(path: &str, context: &'static ExecutableContext) -> Result<Vec<u8>> {
    let virtual_file_system = context.virtual_file_system;

    let task = context.task_manager.get_current_task_identifier().await;

    let path = Path::new(path);

    let statistics = virtual_file_system.get_statistics(&path).await?;

    if statistics.kind != Kind::File {
        return Err(Error::NotAWasmFile);
    }

    let mut buffer = Vec::with_capacity(statistics.size as usize);

    File::read_from_path(virtual_file_system, task, &path, &mut buffer).await?;

    Ok(buffer)
}

async fn main_inner(
    standard: Standard<'static>,
    arguments: WasmArguments<'_>,
    guest_arguments: Vec<String>,
) -> Result<()> {
    let context = standard.context;
    let WasmArguments {
        path,
        install,
        stack_size: _stack_size,
        instruction_limit,
    } = arguments;

    let task = context.task_manager.get_current_task_identifier().await;
    context
        .task_manager
        .set_application_identifier(
            task,
            application_metadata::application_identifier_for_path(path),
        )
        .await?;
    if let Some(application_name) = application_metadata::application_name_for_path(path) {
        let user = context.task_manager.get_user(task).await?;
        context
            .virtual_file_system
            .ensure_user_application_directories(task, user, application_name)
            .await?;
        let application_directory =
            xila::virtual_file_system::user_application_directory(user, application_name);
        let configuration = application_directory
            .clone()
            .append("configuration")
            .ok_or(Error::InvalidPath)?;
        let data = application_directory
            .clone()
            .append("data")
            .ok_or(Error::InvalidPath)?;
        let cache = application_directory
            .append("cache")
            .ok_or(Error::InvalidPath)?;
        context
            .task_manager
            .set_environment_variables(
                task,
                &[
                    ("XILA_APPLICATION_CONFIGURATION", configuration.as_str()),
                    ("XILA_APPLICATION_DATA", data.as_str()),
                    ("XILA_APPLICATION_CACHE", cache.as_str()),
                ],
            )
            .await?;
    }

    let mut configuration = Config::default();
    configuration.consume_fuel(true);
    // Wasmi 2.0.0 cannot resume fuel exhaustion during lazy callee translation
    // (https://github.com/wasmi-labs/wasmi/pull/2032). Compile
    // before execution so even small scheduling slices can make progress.
    configuration.compilation_mode(CompilationMode::Eager);

    let engine = Engine::new(&configuration);

    let buffer = read_file(path, context).await?;

    let module = Module::new(&engine, &buffer)?;

    let (_standard_in, standard_out, standard_error) = standard.split();
    let root_dir = xila::virtual_file_system::SynchronousDirectory::open(
        context.virtual_file_system,
        task,
        xila::file_system::Path::from_str("/"),
    );

    let mut store = Store::new(
        &engine,
        GlobalStore {
            context,
            pending: None,
            #[cfg(feature = "graphics")]
            graphics: super::graphics::GraphicsState::new(context.graphics_manager),
            wasi: WasiContext {
                files: alloc::collections::BTreeMap::new(),
                next_fd: 3,
                arguments: {
                    let mut arguments = alloc::vec![path.to_string()];
                    arguments.extend(guest_arguments);
                    arguments
                },
                environment: context
                    .task_manager
                    .get_environment_variables(task)
                    .await?
                    .into_iter()
                    .map(|variable| {
                        (
                            variable.get_name().to_string(),
                            variable.get_value().to_string(),
                        )
                    })
                    .collect(),
                task,
                prestats: Vec::new(),
                exit_code: None,
            },
        },
    );

    {
        let data = store.data_mut();
        data.wasi.files.insert(
            0,
            FileSystemItem::StandardInput(FileVariant {
                file: _standard_in.into_synchronous_file(),
            }),
        );
        data.wasi.files.insert(
            1,
            FileSystemItem::StandardOutput(FileVariant {
                file: standard_out.into_synchronous_file(),
            }),
        );
        data.wasi.files.insert(
            2,
            FileSystemItem::StandardError(FileVariant {
                file: standard_error.into_synchronous_file(),
            }),
        );
        if let Ok(dir) = root_dir {
            data.wasi.files.insert(
                3,
                FileSystemItem::Directory(DirectoryVariant {
                    directory: dir,
                    path: xila::file_system::PathOwned::root(),
                }),
            );
            data.wasi.next_fd = 4;
            data.wasi.prestats.push(Prestat {
                name: b"/".to_vec(),
            });
        }
    }

    let mut linker = Linker::<GlobalStore>::new(&engine);

    wasi::register::add_wasi_to_linker(&mut linker)?;
    #[cfg(feature = "graphics")]
    super::graphics::wasmi::add_to_linker(&mut linker)?;

    linker.func_wrap(
        "host",
        "hello",
        |caller: Caller<'_, GlobalStore>, param: i32| {
            log::information!(
                "Hello from WebAssembly! Got {} from WebAssembly and my host state is: {}",
                param,
                caller.data()
            );
        },
    )?;

    store.set_fuel(instruction_limit.get() as u64)?;
    match linker.instantiate_and_start(&mut store, &module) {
        Ok(instance) => {
            let entry = if install {
                INSTALL_FUNCTION_NAME
            } else {
                START_FUNCTION_NAME
            };
            let start = instance.get_typed_func::<(), ()>(&store, entry)?;
            super::execution::run(&mut store, start, instruction_limit).await
        }
        Err(e) => {
            if let Some(code) = store.data().wasi.exit_code {
                if code == 0 {
                    Ok(())
                } else {
                    Err(Error::Runtime(code))
                }
            } else {
                Err(Error::Wasm(e))
            }
        }
    }
}
