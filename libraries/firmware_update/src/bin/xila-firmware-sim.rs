//! Native reference harness. All update/confirmation operations use the real VFS.
extern crate abi_definitions;
use file_system::{AccessFlags, MemoryDevice, Permissions};
use firmware_update::{
    device::*,
    nor::{Fault, FileNor, PAGE},
    simulator::{FLASH_SIZE, IMAGE_CAPACITY, MANIFEST_SIZE, Simulator},
};
use std::{collections::BTreeMap, fmt::Debug, fs::File as HostFile, io::Read, path::Path};
use virtual_file_system::{File, ItemStatic, VirtualFileSystem};
static MANAGER: task::Manager = task::Manager::new();
type Result<T> = std::result::Result<T, String>;
fn checked<T, E: Debug>(r: std::result::Result<T, E>) -> Result<T> {
    r.map_err(|e| format!("{e:?}"))
}
const HELP: &str = "EXPERIMENTAL native NOR simulator; no hardware root or production bootloader.\nUsage: xila-firmware-sim <init|update|boot|inspect> --flash PATH --key PUBLIC_KEY_FILE --target TARGET16\n  init/update: --manifest SIGNED_MANIFEST_FILE --image IMAGE_FILE\n  update: [--stage-only] [--fault-operation N --fault-byte N]\n  boot: [--confirm] [--fault-operation N --fault-byte N]\n  init refuses an existing flash file. Never use production signing keys.";
struct Args {
    command: String,
    options: BTreeMap<String, String>,
    confirm: bool,
    stage_only: bool,
}
impl Args {
    fn parse() -> Result<Self> {
        let mut args = std::env::args().skip(1);
        let command = args.next().ok_or_else(|| HELP.to_string())?;
        if command == "--help" {
            println!("{HELP}");
            std::process::exit(0);
        }
        if !["init", "update", "boot", "inspect"].contains(&command.as_str()) {
            return Err(HELP.into());
        }
        let mut result = Self {
            command,
            options: BTreeMap::new(),
            confirm: false,
            stage_only: false,
        };
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--confirm" if result.command == "boot" && !result.confirm => result.confirm = true,
                "--stage-only" if result.command == "update" && !result.stage_only => {
                    result.stage_only = true
                }
                "--flash" | "--key" | "--target" | "--manifest" | "--image"
                | "--fault-operation" | "--fault-byte" => {
                    let value = args
                        .next()
                        .ok_or_else(|| format!("missing value for {arg}"))?;
                    if result.options.insert(arg, value).is_some() {
                        return Err("duplicate option".into());
                    }
                }
                _ => return Err(format!("unknown or inappropriate option: {arg}")),
            }
        }
        Ok(result)
    }
    fn get(&self, name: &str) -> Result<&str> {
        self.options
            .get(name)
            .map(String::as_str)
            .ok_or_else(|| format!("missing {name}"))
    }
    fn fault(&self) -> Result<Option<Fault>> {
        match (
            self.options.get("--fault-operation"),
            self.options.get("--fault-byte"),
        ) {
            (None, None) => Ok(None),
            (Some(op), Some(byte)) => Ok(Some(Fault {
                operation: checked(op.parse())?,
                after_bytes: checked(byte.parse())?,
            })),
            _ => Err("fault requires both --fault-operation and --fault-byte".into()),
        }
    }
}
fn fixed<const N: usize>(path: &str) -> Result<[u8; N]> {
    let mut file = checked(HostFile::open(path))?;
    let mut bytes = [0; N];
    checked(file.read_exact(&mut bytes))?;
    let mut extra = [0];
    if checked(file.read(&mut extra))? != 0 {
        return Err(format!("expected exactly {N} bytes in {path}"));
    }
    Ok(bytes)
}
async fn filesystem(
    manager: &'static task::Manager,
    sim: Simulator<FileNor>,
) -> Result<(&'static VirtualFileSystem, task::TaskIdentifier)> {
    let task = manager.get_current_task_identifier().await;
    if checked(manager.get_user(task).await)? != users::UserIdentifier::ROOT {
        return Err("firmware mounting requires root".into());
    }
    let users = Box::leak(Box::new(users::Manager::new()));
    let time = Box::leak(Box::new(checked(time::Manager::new(
        &drivers_std::devices::TimeDevice,
    ))?));
    let disk = MemoryDevice::<512>::new_static(512 * 128);
    checked(little_fs::FileSystem::format(disk, 256))?;
    let fs = checked(little_fs::FileSystem::new(disk, 256))?;
    let vfs = Box::leak(Box::new(VirtualFileSystem::new(manager, users, time, fs)));
    checked(vfs.create_directory(task, &"/devices").await)?;
    checked(vfs.create_directory(task, &"/devices/system").await)?;
    checked(
        vfs.mount_static_with_permissions(
            task,
            DEVICE_PATH,
            ItemStatic::CharacterDevice(Box::leak(Box::new(FirmwareDevice::new(sim)))),
            Permissions::USER_READ_WRITE,
        )
        .await,
    )?;
    Ok((vfs, task))
}
async fn execute(manager: &'static task::Manager) -> Result<()> {
    let args = Args::parse()?;
    let key = fixed::<32>(args.get("--key")?)?;
    let target: [u8; 16] = args
        .get("--target")?
        .as_bytes()
        .try_into()
        .map_err(|_| "target must be exactly 16 bytes")?;
    let path = Path::new(args.get("--flash")?);
    let fault = args.fault()?;
    if args.command == "init" {
        if fault.is_some() {
            return Err("init does not accept fault injection".into());
        }
        let metadata = fixed::<MANIFEST_SIZE>(args.get("--manifest")?)?;
        let mut image = checked(HostFile::open(args.get("--image")?))?;
        let mut bytes = [0; IMAGE_CAPACITY];
        let mut count = 0;
        loop {
            let n = checked(image.read(&mut bytes[count..]))?;
            if n == 0 {
                break;
            }
            count += n;
            if count == bytes.len() {
                break;
            }
        }
        let mut extra = [0];
        if checked(image.read(&mut extra))? != 0 {
            return Err("factory image too large".into());
        }
        let flash = checked(FileNor::create(path, FLASH_SIZE))?;
        checked(Simulator::provision(
            flash,
            key,
            target,
            &metadata,
            &bytes[..count],
        ))?;
        println!("initialized authenticated simulator fixture; hardware_root=false");
        return Ok(());
    }
    let flash = checked(FileNor::open(path, fault))?;
    let mut sim = checked(Simulator::new(flash, key, target))?;
    if args.command == "inspect" {
        println!("{:?}", sim.status());
        println!("security_floor={}", sim.status().security_floor);
        return Ok(());
    }
    if args.command == "boot" {
        let boot = checked(sim.boot())?;
        println!(
            "slot={} trial={} security_version={}",
            boot.slot, boot.trial, boot.security_version
        );
        if args.confirm {
            let (vfs, task) = filesystem(manager, sim).await?;
            let mut file =
                checked(File::open(vfs, task, DEVICE_PATH, AccessFlags::Write.into()).await)?;
            checked(file.control(CONFIRM, &header()).await)?;
            checked(file.close(vfs).await)?;
            println!("confirmed via privileged VFS handle");
        }
        return Ok(());
    }
    let metadata = fixed::<MANIFEST_SIZE>(args.get("--manifest")?)?;
    let size = u64::from_le_bytes(metadata[32..40].try_into().unwrap());
    let request = checked(begin_request(size, &metadata))?;
    let mut image = checked(HostFile::open(args.get("--image")?))?;
    let (vfs, task) = filesystem(manager, sim).await?;
    let mut file = checked(File::open(vfs, task, DEVICE_PATH, AccessFlags::Write.into()).await)?;
    checked(file.control(BEGIN, &request).await)?;
    let mut buffer = [0; PAGE];
    let mut total = 0;
    loop {
        let n = checked(image.read(&mut buffer))?;
        if n == 0 {
            break;
        }
        let mut written = 0;
        while written < n {
            let count = checked(file.write(&buffer[written..n]).await)?;
            if count == 0 {
                return Err("zero-length write".into());
            }
            written += count;
            total += count;
        }
    }
    checked(file.control(FINISH, &header()).await)?;
    if !args.stage_only {
        checked(file.control(ACTIVATE, &header()).await)?;
    }
    checked(file.close(vfs).await)?;
    println!(
        "streamed={total} via {DEVICE_PATH}; activated={}",
        !args.stage_only
    );
    Ok(())
}
#[task::run(manager=&MANAGER)]
async fn main(manager: &'static task::Manager) {
    drivers_std::memory::instantiate_global_allocator!();
    match execute(manager).await {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
