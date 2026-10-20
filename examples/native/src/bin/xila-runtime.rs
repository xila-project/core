//! Minimal headless runner for Xila executables.

use std::{
    env,
    path::PathBuf,
    process,
    string::{String, ToString},
    sync::atomic::{AtomicI32, Ordering},
    vec::Vec,
};
use xila::file_system::PathOwned;

#[path = "../storage.rs"]
mod storage;

static TASK_MANAGER: xila::task::Manager = xila::task::Manager::new();
static SHUTDOWN_SIGNAL: AtomicI32 = AtomicI32::new(0);

#[cfg(unix)]
unsafe extern "C" {
    fn signal(signal: i32, handler: usize) -> usize;
}

#[cfg(unix)]
extern "C" fn handle_signal(signal: i32) {
    SHUTDOWN_SIGNAL.store(signal, Ordering::Relaxed);
    wasm::request_shutdown();
}

#[cfg(unix)]
fn install_signal_handlers() -> Result<(), String> {
    // POSIX SIGINT=2, SIGTERM=15, and SIG_ERR=(void *)-1.
    for signal_number in [2, 15] {
        // SAFETY: this callback only writes lock-free atomics.
        let previous = unsafe { signal(signal_number, handle_signal as *const () as usize) };
        if previous == usize::MAX {
            return Err(format!(
                "failed to install handler for signal {signal_number}"
            ));
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn install_signal_handlers() -> Result<(), String> {
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MountSpec {
    host: PathBuf,
    xila: String,
    read_only: bool,
    uid: u16,
    gid: u16,
    file_mode: u16,
    directory_mode: u16,
}

#[derive(Debug, PartialEq, Eq)]
struct Config {
    disk: PathBuf,
    seed: PathBuf,
    mounts: Vec<MountSpec>,
    environment: Vec<(String, String)>,
    shell: bool,
    executable: String,
    arguments: Vec<String>,
}

const SHELL_EXECUTABLE: &str = "/binaries/command_line_shell";

const USAGE: &str = concat!(
    "Usage:\n",
    "  xila-runtime run [OPTIONS] -- <Xila executable> [arguments...]\n",
    "  xila-runtime shell [OPTIONS]\n\n",
    "Options: [--disk PATH] [--seed PATH] [--network=none]\n",
    "  [--mount host=PATH,xila=PATH,ro|rw[,uid=N,gid=N,file_mode=OCTAL,directory_mode=OCTAL]]\n",
    "  [--env KEY=VALUE]...\n\n",
    "The shell reuses Xila's command-line shell and requires stdin for login.\n",
    "Host directories are available only through explicit mounts; networking is disabled.",
);

fn parse_mount(value: &str) -> Result<MountSpec, String> {
    let mut host = None;
    let mut xila = None;
    let mut read_only = false;
    let mut uid = 0;
    let mut gid = 0;
    let mut file_mode = 0o644;
    let mut directory_mode = 0o755;
    let mut access_seen = false;

    for field in value.split(',') {
        let (key, value) = field
            .split_once('=')
            .map(|(key, value)| (Some(key), Some(value)))
            .unwrap_or((None, None));
        match (key, value, field) {
            (Some("host"), Some(path), _)
                if PathBuf::from(path).is_absolute() && host.is_none() =>
            {
                host = Some(PathBuf::from(path));
            }
            (Some("xila"), Some(path), _)
                if is_valid_vfs_path(path) && path != "/" && xila.is_none() =>
            {
                xila = Some(path.to_string())
            }
            (None, None, "ro") if !access_seen => {
                read_only = true;
                access_seen = true;
            }
            (None, None, "rw") if !access_seen => {
                read_only = false;
                access_seen = true;
            }
            (Some("uid"), Some(value), _) => uid = parse_u16(value, "uid")?,
            (Some("gid"), Some(value), _) => gid = parse_u16(value, "gid")?,
            (Some("file_mode"), Some(value), _) => {
                file_mode = parse_octal_mode(value, "file_mode")?
            }
            (Some("directory_mode"), Some(value), _) => {
                directory_mode = parse_octal_mode(value, "directory_mode")?
            }
            _ => return Err(format!("invalid mount field: {field}")),
        }
    }

    Ok(MountSpec {
        host: host.ok_or_else(|| "mount requires an absolute host=PATH".to_string())?,
        xila: xila.ok_or_else(|| "mount requires xila=/PATH".to_string())?,
        read_only,
        uid,
        gid,
        file_mode,
        directory_mode,
    })
}

fn parse_u16(value: &str, name: &str) -> Result<u16, String> {
    value
        .parse()
        .map_err(|_| format!("{name} must be an integer from 0 to 65535"))
}

fn parse_octal_mode(value: &str, name: &str) -> Result<u16, String> {
    let digits = value.strip_prefix("0o").unwrap_or(value);
    u16::from_str_radix(digits, 8)
        .ok()
        .filter(|mode| *mode <= 0o777)
        .ok_or_else(|| format!("{name} must be an octal mode from 000 to 777"))
}

fn parse_args<I, S>(arguments: I) -> Result<Config, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut arguments = arguments
        .into_iter()
        .map(|argument| argument.as_ref().to_string());
    let command = arguments.next().ok_or_else(|| USAGE.to_string())?;
    let shell = match command.as_str() {
        "run" => false,
        "shell" => true,
        _ => return Err(USAGE.to_string()),
    };

    let mut disk = PathBuf::from("/var/lib/xila/drive.img");
    let mut seed = PathBuf::from("/usr/share/xila/base.img");
    let mut mounts = Vec::new();
    let mut environment = Vec::new();
    let mut app = None;
    let mut app_arguments = Vec::new();
    let mut arguments = arguments.peekable();
    while let Some(argument) = arguments.next() {
        if argument == "--" {
            if shell {
                return Err(format!(
                    "shell does not accept an executable or arguments\n{USAGE}"
                ));
            }
            app = arguments.next();
            app_arguments.extend(arguments);
            break;
        }
        if let Some(value) = argument.strip_prefix("--disk=") {
            disk = PathBuf::from(value);
        } else if argument == "--disk" {
            disk = PathBuf::from(arguments.next().ok_or("--disk requires a path")?);
        } else if let Some(value) = argument.strip_prefix("--seed=") {
            seed = PathBuf::from(value);
        } else if argument == "--seed" {
            seed = PathBuf::from(arguments.next().ok_or("--seed requires a path")?);
        } else if let Some(value) = argument.strip_prefix("--mount=") {
            mounts.push(parse_mount(value)?);
        } else if argument == "--mount" {
            mounts.push(parse_mount(
                &arguments.next().ok_or("--mount requires a value")?,
            )?);
        } else if let Some(value) = argument.strip_prefix("--env=") {
            environment.push(parse_environment(value)?);
        } else if argument == "--env" {
            environment.push(parse_environment(
                &arguments.next().ok_or("--env requires KEY=VALUE")?,
            )?);
        } else if argument == "--network=none"
            || argument == "--network" && arguments.peek().is_some_and(|value| value == "none")
        {
            if argument == "--network" {
                arguments.next();
            }
        } else if argument.starts_with("--network") {
            return Err("only --network=none is supported by the headless runner".to_string());
        } else {
            return Err(format!("unknown runtime option: {argument}\n{USAGE}"));
        }
    }

    if shell && environment.iter().any(|(name, _)| name == "User") {
        return Err("shell mode does not allow overriding the authenticated User".to_string());
    }
    let executable = if shell {
        SHELL_EXECUTABLE.to_string()
    } else {
        app.ok_or_else(|| format!("missing Xila executable after --\n{USAGE}"))?
    };
    if !is_valid_vfs_path(&executable) {
        return Err("the executable must be an absolute Xila VFS path".to_string());
    }
    Ok(Config {
        disk,
        seed,
        mounts,
        environment,
        shell,
        executable,
        arguments: app_arguments,
    })
}

fn is_valid_vfs_path(path: &str) -> bool {
    path.starts_with('/')
        && PathOwned::new(path.to_string()).is_some()
        && !path
            .split('/')
            .any(|component| matches!(component, "." | ".."))
}

fn parse_environment(value: &str) -> Result<(String, String), String> {
    let (name, value) = value
        .split_once('=')
        .ok_or("environment must use KEY=VALUE")?;
    if name.is_empty() || name.contains('=') || name.contains('\0') || value.contains('\0') {
        return Err("environment variable name/value is invalid".to_string());
    }
    Ok((name.to_string(), value.to_string()))
}

#[xila::task::run(
    task_path = xila::task,
    executor = drivers_std::executor::instantiate_static_executor!(),
    manager = &TASK_MANAGER
)]
async fn main(task_manager: &'static xila::task::Manager) {
    drivers_std::memory::instantiate_global_allocator!();
    if let Err(error) = install_signal_handlers() {
        eprintln!("xila-runtime: {error}");
        process::exit(70);
    }
    let arguments: Vec<String> = env::args().skip(1).collect();
    if arguments.len() == 1 && matches!(arguments[0].as_str(), "--help" | "-h") {
        println!("{USAGE}");
        process::exit(0);
    }
    if arguments.len() == 1 && matches!(arguments[0].as_str(), "--version" | "-V") {
        println!("xila-runtime {}", env!("CARGO_PKG_VERSION"));
        process::exit(0);
    }
    let config = match parse_args(arguments.iter()) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            process::exit(2);
        }
    };
    match run_runtime(task_manager, config).await {
        Ok(code) => process::exit(code),
        Err(error) => {
            eprintln!("xila-runtime: {error}");
            process::exit(70);
        }
    }
}

async fn run_runtime(
    task_manager: &'static xila::task::Manager,
    config: Config,
) -> Result<i32, String> {
    use xila::{
        executable::{self, ExecutableContext, Standard},
        file_system::Permissions,
        little_fs, log as xila_log, time, users,
        virtual_file_system::{self},
    };

    xila_log::initialize(&drivers_std::log::Logger)
        .map_err(|error| format!("logging initialization failed: {error:?}"))?;
    let users_manager: &'static users::Manager = Box::leak(Box::new(users::Manager::new()));
    let time_manager: &'static time::Manager<'static> = Box::leak(Box::new(
        time::Manager::new(&drivers_std::devices::TimeDevice)
            .map_err(|error| format!("time initialization failed: {error:?}"))?,
    ));

    let size = if config.disk.exists() {
        std::fs::metadata(&config.disk)
            .map_err(|error| format!("cannot inspect disk {}: {error}", config.disk.display()))?
            .len()
    } else {
        std::fs::metadata(&config.seed)
            .map_err(|error| format!("cannot inspect seed {}: {error}", config.seed.display()))?
            .len()
    };
    if let Some(parent) = config
        .disk
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!("cannot create disk directory {}: {error}", parent.display())
        })?;
    }
    storage::prepare_disk(&config.disk, &config.seed, size)
        .map_err(|error| format!("disk initialization failed: {error}"))?;
    let drive = Box::leak(Box::new(
        drivers_std::drive_file::FileDriveDevice::open_existing(&config.disk)
            .map_err(|error| format!("cannot open disk: {error}"))?,
    ));
    let root = little_fs::mount_disk(drive, 256)
        .map_err(|error| format!("cannot mount LittleFS disk: {error:?}"))?;

    let vfs: &'static virtual_file_system::VirtualFileSystem = Box::leak(Box::new(
        virtual_file_system::initialize(task_manager, users_manager, time_manager, root)
            .map_err(|error| format!("cannot initialize VFS: {error:?}"))?,
    ));
    xila::abi_definitions::initialize(xila::abi_definitions::RuntimeContext {
        virtual_file_system: vfs,
        time_manager,
    });
    let task = task_manager.get_current_task_identifier().await;
    virtual_file_system::create_default_hierarchy(vfs, task)
        .await
        .map_err(|error| format!("cannot create Xila hierarchy: {error:?}"))?;

    // Host mount paths are explicit capabilities; the process environment is never copied.
    for mount in &config.mounts {
        ensure_vfs_directory(vfs, task, &mount.xila).await?;
        let options = drivers_native::host_file_system::HostFileSystemOptions {
            read_only: mount.read_only,
            user: users::UserIdentifier::new(mount.uid),
            group: users::GroupIdentifier::new(mount.gid),
            file_permissions: Permissions::from_octal(mount.file_mode)
                .ok_or_else(|| "invalid file permissions".to_string())?,
            directory_permissions: Permissions::from_octal(mount.directory_mode)
                .ok_or_else(|| "invalid directory permissions".to_string())?,
        };
        let filesystem =
            drivers_native::host_file_system::HostFileSystem::open(&mount.host, options).map_err(
                |error| format!("cannot open host mount {}: {error:?}", mount.host.display()),
            )?;
        let xila_path = PathOwned::new(mount.xila.clone())
            .ok_or_else(|| format!("invalid Xila mount path: {}", mount.xila))?;
        vfs.mount_file_system(filesystem, &xila_path, task)
            .await
            .map_err(|error| {
                format!(
                    "cannot mount {} at {}: {error:?}",
                    mount.host.display(),
                    mount.xila
                )
            })?;
    }

    xila::virtual_file_system::mount_static!(
        vfs,
        task,
        &[
            (
                &"/devices/standard_in",
                CharacterDevice,
                drivers_std::console::StandardInDevice
            ),
            (
                &"/devices/standard_out",
                CharacterDevice,
                drivers_std::console::StandardOutDevice
            ),
            (
                &"/devices/standard_error",
                CharacterDevice,
                drivers_std::console::StandardErrorDevice
            ),
            (
                &"/devices/time",
                CharacterDevice,
                drivers_std::devices::TimeDevice
            ),
            (
                &"/devices/random",
                CharacterDevice,
                drivers_shared::devices::RandomDevice
            ),
            (
                &"/devices/hasher",
                CharacterDevice,
                drivers_shared::devices::HashDevice
            ),
            (&"/devices/null", CharacterDevice, drivers_core::NullDevice),
        ]
    )
    .await
    .map_err(|error| format!("cannot mount standard devices: {error:?}"))?;

    let context: &'static ExecutableContext = Box::leak(Box::new(ExecutableContext {
        task_manager,
        users_manager,
        virtual_file_system: vfs,
        graphics_manager: None,
        network_manager: None,
        time_manager,
    }));
    xila::executable::mount_executables!(
        context,
        vfs,
        task,
        &[
            (&"/binaries/wasm", wasm::WasmExecutable),
            (&SHELL_EXECUTABLE, command_line_shell::ShellExecutable),
        ]
    )
    .await
    .map_err(|error| format!("cannot mount WASM executable: {error:?}"))?;

    if config.shell {
        let authentication_context = xila::authentication::Context {
            virtual_file_system: vfs,
            task_manager,
            users_manager,
            task,
        };
        xila::authentication::initialize_seeded(&authentication_context)
            .await
            .map_err(|error| format!("cannot initialize shell accounts: {error:?}"))?;
    }

    wasm::clear_shutdown_request();

    task_manager
        .set_environment_variable(task, "Paths", "/")
        .await
        .map_err(|error| format!("cannot set Xila environment: {error:?}"))?;
    task_manager
        .set_environment_variable(task, "Host", "xila")
        .await
        .map_err(|error| format!("cannot set Xila environment: {error:?}"))?;
    for (name, value) in &config.environment {
        task_manager
            .set_environment_variable(task, name, value)
            .await
            .map_err(|error| format!("cannot set environment variable {name}: {error:?}"))?;
    }

    let standard = Standard::open(
        &"/devices/standard_in",
        &"/devices/standard_out",
        &"/devices/standard_error",
        task,
        vfs,
        context,
    )
    .await
    .map_err(|error| format!("cannot open host standard streams: {error:?}"))?;
    let mut executable_arguments = config.arguments;
    if config.executable == "/binaries/wasm" && executable_arguments.len() > 1 {
        executable_arguments.insert(1, "--".to_string());
    }
    let executable_path = PathOwned::new(config.executable.clone())
        .ok_or_else(|| format!("invalid Xila executable path: {}", config.executable))?;
    let executable = match executable::execute(
        context,
        &executable_path,
        executable_arguments,
        standard,
        None,
    )
    .await
    {
        Ok(handle) => handle,
        Err(error) => return Err(format!("cannot execute {}: {error:?}", config.executable)),
    };
    let result = executable.join().await;
    let signal = SHUTDOWN_SIGNAL.load(Ordering::Relaxed);
    vfs.uninitialize().await;
    Ok(if signal != 0 {
        128 + signal
    } else if result == 0 {
        0
    } else {
        result.unsigned_abs().min(255) as i32
    })
}

async fn ensure_vfs_directory(
    vfs: &'static xila::virtual_file_system::VirtualFileSystem,
    task: xila::task::TaskIdentifier,
    path: &str,
) -> Result<(), String> {
    let mut current = String::new();
    let parent = path.rsplit_once('/').map_or("", |(parent, _)| parent);
    for part in parent.split('/').filter(|part| !part.is_empty()) {
        current.push('/');
        current.push_str(part);
        let current_path = PathOwned::new(current.clone())
            .ok_or_else(|| format!("invalid Xila mount path: {current}"))?;
        match vfs.create_directory(task, &current_path).await {
            Ok(()) | Err(xila::virtual_file_system::Error::AlreadyExists) => {}
            Err(error) => return Err(format!("cannot create mount point {current}: {error:?}")),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_repeated_mounts_environment_and_app_arguments() {
        let config = parse_args([
            "run",
            "--disk",
            "state.img",
            "--seed=base.img",
            "--mount",
            "host=/mnt/in,xila=/input,ro,uid=12,gid=13,file_mode=0444,directory_mode=0555",
            "--mount=host=/mnt/out,xila=/output,rw",
            "--env",
            "MODE=production",
            "--network=none",
            "--",
            "/binaries/wasm",
            "/input/main.wasm",
            "a",
            "b",
        ])
        .unwrap();
        assert_eq!(config.disk, PathBuf::from("state.img"));
        assert_eq!(config.mounts.len(), 2);
        assert!(config.mounts[0].read_only);
        assert_eq!(config.mounts[0].uid, 12);
        assert_eq!(config.mounts[0].file_mode, 0o444);
        assert!(!config.mounts[1].read_only);
        assert_eq!(
            config.environment,
            vec![(String::from("MODE"), String::from("production"))]
        );
        assert!(!config.shell);
        assert_eq!(config.executable, "/binaries/wasm");
        assert_eq!(
            config.arguments,
            vec![
                String::from("/input/main.wasm"),
                String::from("a"),
                String::from("b")
            ]
        );
    }

    #[test]
    fn parses_interactive_shell_with_shared_runtime_options() {
        let config = parse_args([
            "shell",
            "--disk",
            "shell.img",
            "--network=none",
            "--mount",
            "host=/mnt/apps,xila=/apps,ro",
            "--env",
            "MODE=development",
        ])
        .unwrap();
        assert!(config.shell);
        assert_eq!(config.disk, PathBuf::from("shell.img"));
        assert_eq!(config.executable, SHELL_EXECUTABLE);
        assert!(config.arguments.is_empty());
        assert_eq!(config.mounts.len(), 1);
        assert!(config.mounts[0].read_only);
        assert_eq!(
            config.environment,
            vec![("MODE".into(), "development".into())]
        );
    }

    #[test]
    fn shell_rejects_extra_arguments_and_user_impersonation() {
        assert!(parse_args(["shell", "--", "/binaries/wasm"]).is_err());
        assert!(parse_args(["shell", "--env", "User=administrator"]).is_err());
        assert!(parse_args(["shell", "--network=host"]).is_err());
    }

    #[test]
    fn refuses_ambient_executable_paths_and_unsupported_networking() {
        assert!(
            parse_args(["run", "--", "relative.wasm"])
                .unwrap_err()
                .contains("absolute")
        );
        assert!(
            parse_args(["run", "--", "/../host/binary"])
                .unwrap_err()
                .contains("absolute")
        );
        assert!(
            parse_args(["run", "--network=tuntap", "--", "/binaries/wasm"])
                .unwrap_err()
                .contains("only --network=none")
        );
        assert!(
            parse_args([
                "run",
                "--mount",
                "host=/tmp,xila=/x,ro,rw",
                "--",
                "/binaries/wasm"
            ])
            .is_err()
        );
    }

    #[test]
    fn rejects_invalid_mount_metadata_and_environment() {
        assert!(
            parse_args([
                "run",
                "--mount",
                "host=/tmp,xila=/x,uid=65536",
                "--",
                "/binaries/wasm"
            ])
            .is_err()
        );
        assert!(
            parse_args([
                "run",
                "--mount",
                "host=/tmp,xila=/x,file_mode=0999",
                "--",
                "/binaries/wasm"
            ])
            .is_err()
        );
        assert!(parse_args(["run", "--env", "NO_EQUALS", "--", "/binaries/wasm"]).is_err());
    }
}
