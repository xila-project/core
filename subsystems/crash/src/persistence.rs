extern crate alloc;

use alloc::{format, vec::Vec};

use file_system::{AccessFlags, CreateFlags, Flags, Kind, Path, Permission, Permissions, Special};
use synchronization::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use task::{Manager, TaskIdentifier};
use users::{ADMINISTRATOR_GROUP_NAME, GroupIdentifier, UserIdentifier};
use virtual_file_system::{Directory, File, IoErrorKind, Result, VirtualFileSystem};

use crate::{CRASH_LOOP_LIMIT, NATIVE_RECORD_CAPACITY, Record, RecordKind};

const ROOT: TaskIdentifier = Manager::ROOT_TASK_IDENTIFIER;
const TEMPORARY_REPORT: &Path = Path::from_str("/system/crashes/.pending");
const SEQUENCE: &Path = Path::from_str("/system/crashes/.sequence");
const LARGE_STORAGE_MINIMUM: u64 = 1024 * 1024;
const DIRECTORY_PERMISSIONS: Permissions = Permissions::new(
    Permission::all(),
    Permission::READ_EXECUTE,
    Permission::empty(),
    Special::NONE,
);
const REPORT_PERMISSIONS: Permissions = Permissions::new(
    Permission::READ_WRITE,
    Permission::Read,
    Permission::empty(),
    Special::NONE,
);

static STORE: Mutex<CriticalSectionRawMutex, ()> = Mutex::new(());

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReportIdentifier(pub u64);

impl ReportIdentifier {
    pub fn from_name(name: &str) -> Option<Self> {
        let digits = name.strip_suffix("-kernel.xcr")?;
        let sequence: u64 = digits.parse().ok()?;
        // Reject signs and leading zeroes. Each identifier names one file.
        (digits
            .as_bytes()
            .first()
            .is_some_and(|byte| (b'1'..=b'9').contains(byte))
            && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .then_some(Self(sequence))
    }
}

impl core::fmt::Display for ReportIdentifier {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{}-kernel.xcr", self.0)
    }
}

/// Return the visible kernel reports, oldest first, using the caller's access.
pub async fn list(
    file_system: &VirtualFileSystem,
    task: TaskIdentifier,
) -> Result<Vec<ReportIdentifier>> {
    let mut directory = match Directory::open(file_system, task, Path::SYSTEM_CRASHES).await {
        Err(IoErrorKind::NotFound) => return Ok(Vec::new()),
        result => result?,
    };
    let mut reports = Vec::new();
    let read = async {
        while let Some(entry) = directory.read().await? {
            if entry.kind == Kind::File
                && let Some(identifier) = ReportIdentifier::from_name(&entry.name)
            {
                reports.push(identifier);
            }
        }
        Ok(())
    }
    .await;
    let closed = directory.close(file_system).await;
    read.and(closed)?;
    reports.sort_unstable();
    Ok(reports)
}

/// Read and validate a stored report using the caller's file permissions.
pub async fn read(
    file_system: &VirtualFileSystem,
    task: TaskIdentifier,
    identifier: ReportIdentifier,
) -> Result<Vec<u8>> {
    let name = format!("{identifier}");
    let path = report_path(&name)?;
    let mut file = File::open(file_system, task, &path, Flags::from(AccessFlags::Read)).await?;
    let result = async {
        let length = usize::try_from(file.get_statistics().await?.size)
            .ok()
            .filter(|length| *length <= NATIVE_RECORD_CAPACITY)
            .ok_or(IoErrorKind::CorruptData)?;
        let mut bytes = alloc::vec![0; length];
        let mut offset = 0;
        while offset < length {
            let count = file.read(&mut bytes[offset..]).await?;
            if count == 0 {
                return Err(IoErrorKind::CorruptData);
            }
            offset += count;
        }
        let report = Record::read(&bytes).map_err(|_| IoErrorKind::CorruptData)?;
        if report.bytes().len() != length {
            return Err(IoErrorKind::CorruptData);
        }
        Ok(bytes)
    }
    .await;
    let closed = file.close(file_system).await;
    let bytes = result?;
    closed?;
    Ok(bytes)
}

/// Clear stored kernel reports. The caller must be root or an administrator.
pub async fn clear(file_system: &VirtualFileSystem, task: TaskIdentifier) -> Result<()> {
    let _lock = STORE.lock().await;
    let user = file_system
        .task_manager()
        .get_user(task)
        .await
        .map_err(|_| IoErrorKind::PermissionDenied)?;
    let users = file_system.users_manager();
    if user != UserIdentifier::ROOT {
        let group = users
            .get_group_identifier(ADMINISTRATOR_GROUP_NAME)
            .await
            .map_err(|_| IoErrorKind::PermissionDenied)?;
        if !users.is_in_group(user, group).await {
            return Err(IoErrorKind::PermissionDenied);
        }
    }
    let reports = list(file_system, ROOT).await?;
    if let Some(last) = reports.last() {
        // Keep the last identifier across a clear. Publish it before deleting
        // reports so a reset midway through the clear cannot reuse a name.
        if last.0 > sequence_floor(file_system).await? {
            write_temporary(file_system, &last.0.to_le_bytes()).await?;
            file_system
                .rename(ROOT, &TEMPORARY_REPORT, &SEQUENCE)
                .await?;
        }
        for identifier in reports {
            remove_report(file_system, identifier).await?;
        }
    }
    Ok(())
}

/// Import one crash report after the persistent file system is mounted and
/// system users are loaded. Pass the mounted storage capacity in bytes.
pub async fn import(
    file_system: &VirtualFileSystem,
    bytes: &[u8],
    storage_capacity: u64,
) -> Result<Option<ReportIdentifier>> {
    let report = Record::read(bytes).map_err(|_| IoErrorKind::CorruptData)?;
    if report.bytes().len() > NATIVE_RECORD_CAPACITY {
        return Err(IoErrorKind::CorruptData);
    }
    if report.header.kind == RecordKind::ApplicationCrash {
        return Err(IoErrorKind::Unsupported);
    }
    if report.header.consecutive_crashes >= CRASH_LOOP_LIMIT {
        return Ok(None);
    }

    let _lock = STORE.lock().await;
    let group = file_system
        .users_manager()
        .get_group_identifier(ADMINISTRATOR_GROUP_NAME)
        .await
        .unwrap_or(GroupIdentifier::ROOT);
    let directory_path = Path::SYSTEM_CRASHES;
    match file_system
        .create_directory_with_permissions(ROOT, &directory_path, DIRECTORY_PERMISSIONS)
        .await
    {
        Ok(()) | Err(IoErrorKind::AlreadyExists) => {}
        Err(error) => return Err(error),
    }
    let directory = file_system
        .get_statistics_for_task(ROOT, &directory_path)
        .await?;
    if directory.kind != Kind::Directory || directory.user != UserIdentifier::ROOT {
        return Err(IoErrorKind::PermissionDenied);
    }
    if directory.permissions != DIRECTORY_PERMISSIONS {
        file_system
            .set_permissions(ROOT, directory_path, DIRECTORY_PERMISSIONS)
            .await?;
    }
    if directory.group != group {
        file_system
            .set_ownership(ROOT, directory_path, None, Some(group))
            .await?;
    }

    let reports = list(file_system, ROOT).await?;
    let sequence = reports
        .last()
        .map_or(0, |last| last.0)
        .max(sequence_floor(file_system).await?);
    let identifier = ReportIdentifier(sequence.checked_add(1).ok_or(IoErrorKind::InputOutput)?);
    let name = format!("{identifier}");
    let path = report_path(&name)?;
    write_temporary(file_system, report.bytes()).await?;
    file_system
        .set_ownership(ROOT, TEMPORARY_REPORT, None, Some(group))
        .await?;
    file_system
        .set_permissions(ROOT, TEMPORARY_REPORT, REPORT_PERMISSIONS)
        .await?;
    file_system.rename(ROOT, &TEMPORARY_REPORT, &path).await?;
    log::warning!(
        "Imported crash report {identifier}: {:?}",
        report.header.reset_reason
    );

    let limit = if storage_capacity < LARGE_STORAGE_MINIMUM {
        4
    } else {
        8
    };
    let excess = (reports.len() + 1).saturating_sub(limit);
    for oldest in reports.into_iter().take(excess) {
        if let Err(error) = remove_report(file_system, oldest).await {
            // The new report is already durable. A retry must not import it twice.
            log::warning!("Failed to rotate crash report {oldest}: {error}");
        }
    }
    Ok(Some(identifier))
}

async fn sequence_floor(file_system: &VirtualFileSystem) -> Result<u64> {
    let mut bytes = [0; 8];
    match File::read_slice_from_path(file_system, ROOT, SEQUENCE, &mut bytes).await {
        Ok(()) => Ok(u64::from_le_bytes(bytes)),
        Err(IoErrorKind::NotFound) => Ok(0),
        Err(error) => Err(error),
    }
}

async fn write_temporary(file_system: &VirtualFileSystem, bytes: &[u8]) -> Result<()> {
    match file_system.remove(ROOT, TEMPORARY_REPORT).await {
        Ok(()) | Err(IoErrorKind::NotFound) => {}
        Err(error) => return Err(error),
    }
    let mut file = File::open_with_permissions(
        file_system,
        ROOT,
        TEMPORARY_REPORT,
        Flags::new(
            AccessFlags::Write,
            Some(CreateFlags::CREATE_EXCLUSIVE),
            None,
        ),
        Permissions::USER_READ_WRITE,
    )
    .await?;
    let written = async {
        let mut remaining = bytes;
        while !remaining.is_empty() {
            let count = file.write(remaining).await?;
            if count == 0 {
                return Err(IoErrorKind::WriteZero);
            }
            remaining = &remaining[count..];
        }
        file.flush().await
    }
    .await;
    let closed = file.close(file_system).await;
    written.and(closed)
}

fn report_path(name: &str) -> Result<file_system::PathOwned> {
    Path::SYSTEM_CRASHES
        .append(name)
        .ok_or(IoErrorKind::InvalidPath)
}

async fn remove_report(
    file_system: &VirtualFileSystem,
    identifier: ReportIdentifier,
) -> Result<()> {
    let name = format!("{identifier}");
    file_system.remove(ROOT, report_path(&name)?).await
}
