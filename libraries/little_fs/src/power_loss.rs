//! Flash-write policy tests: commit counts and power-loss windows, using a
//! test block device that counts program operations and can stop writes after
//! a chosen number of them (a simulated power loss).
//!
//! A LittleFS "commit" is one metadata commit (`lfs_dir_commit`), which lands
//! as one program operation on the device in these tests. Creating a file
//! with its attributes through `lfs_file_config.attrs` costs two commits: the
//! entry commit (name plus empty inline struct, no attributes) and the
//! following commit that writes the file struct and the custom attributes
//! atomically. The public LittleFS API cannot attach custom attributes to the
//! entry commit itself, so a power loss between the two leaves an entry
//! without attributes; that state fails closed (`MissingAttribute`) and never
//! reports wrong owner or permissions.
//!
//! What the fault device models: a power cut at program-operation granularity.
//! After the cut, every later program and erase fails (the power is gone). The
//! `torn` mode additionally applies only half of the failing program's buffer
//! before failing, modelling a torn page program; LittleFS rejects such a
//! commit on the next mount through its metadata CRC and sequence numbers. The
//! commit counts asserted here assume a clean mount: after a power loss a
//! mount consistency check (`lfs_fs_forceconsistency`) can commit once.

extern crate std;

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use core::task::{Context as TaskContext, Poll};

use alloc::boxed::Box;

use file_system::{
    AccessFlags, AttributeFlags, Attributes, ControlCommand, ControlCommandIdentifier, CreateFlags,
    DirectBaseOperations, DirectBlockDevice, Flags, IoErrorKind, Kind, MemoryDevice,
    MountOperations, Operation, Path, Permissions, Result, Size, Time, block_device,
};
use shared::AnyByLayout;
use users::{GroupIdentifier, UserIdentifier};

use crate::{File, FileSystem, Options};

/// In-memory flash-like device: program operations are counted. After
/// `fail_after` of them, writes and erases fail and are not applied, simulating
/// a power loss. `fail_at` fails exactly one program operation and then recovers
/// (a transient I/O error, not a power loss). `torn_at` applies half of that
/// program's buffer and then fails like a power loss.
struct FaultDevice {
    inner: MemoryDevice<512>,
    progs: AtomicUsize,
    erases: AtomicUsize,
    reads: AtomicUsize,
    fail_after: AtomicUsize,
    fail_at: AtomicUsize,
    torn_at: AtomicUsize,
    fail_read_at: AtomicUsize,
    failed: AtomicBool,
}

impl FaultDevice {
    fn new(size: usize) -> &'static Self {
        Box::leak(Box::new(Self {
            inner: MemoryDevice::<512>::new(size),
            progs: AtomicUsize::new(0),
            erases: AtomicUsize::new(0),
            reads: AtomicUsize::new(0),
            fail_after: AtomicUsize::new(usize::MAX),
            fail_at: AtomicUsize::new(usize::MAX),
            torn_at: AtomicUsize::new(usize::MAX),
            fail_read_at: AtomicUsize::new(usize::MAX),
            failed: AtomicBool::new(false),
        }))
    }

    fn progs(&self) -> usize {
        self.progs.load(Ordering::SeqCst)
    }

    fn reset_progs(&self) {
        self.progs.store(0, Ordering::SeqCst);
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }

    fn reset_reads(&self) {
        self.reads.store(0, Ordering::SeqCst);
    }

    /// Simulate a power loss after `count` program operations.
    fn fail_after(&self, count: usize) {
        self.fail_after.store(count, Ordering::SeqCst);
    }

    /// Fail exactly the `count`-th program operation, then recover.
    fn fail_at(&self, count: usize) {
        self.fail_at.store(count, Ordering::SeqCst);
    }

    /// Apply half of the `count`-th program's buffer, then lose power.
    fn torn_at(&self, count: usize) {
        self.torn_at.store(count, Ordering::SeqCst);
    }

    /// Fail exactly the `count`-th read (a transient read error), then recover.
    fn fail_read_at(&self, count: usize) {
        self.fail_read_at.store(count, Ordering::SeqCst);
    }

    fn allow_all(&self) {
        self.fail_after.store(usize::MAX, Ordering::SeqCst);
        self.fail_at.store(usize::MAX, Ordering::SeqCst);
        self.torn_at.store(usize::MAX, Ordering::SeqCst);
        self.fail_read_at.store(usize::MAX, Ordering::SeqCst);
        self.failed.store(false, Ordering::SeqCst);
    }
}

impl DirectBaseOperations for FaultDevice {
    fn read(
        &self,
        cx: &mut TaskContext<'_>,
        buffer: &mut [u8],
        position: Size,
    ) -> Poll<Result<usize>> {
        let count = self.reads.fetch_add(1, Ordering::SeqCst) + 1;
        if count == self.fail_read_at.load(Ordering::SeqCst) {
            // Transient read error: the device keeps working afterwards.
            return Poll::Ready(Err(IoErrorKind::InputOutput));
        }
        self.inner.read(cx, buffer, position)
    }

    fn write(
        &self,
        cx: &mut TaskContext<'_>,
        buffer: &[u8],
        position: Size,
    ) -> Poll<Result<usize>> {
        let count = self.progs.fetch_add(1, Ordering::SeqCst) + 1;
        if self.failed.load(Ordering::SeqCst) {
            return Poll::Ready(Err(IoErrorKind::InputOutput));
        }
        if count > self.fail_after.load(Ordering::SeqCst) {
            self.failed.store(true, Ordering::SeqCst);
            return Poll::Ready(Err(IoErrorKind::InputOutput));
        }
        if count == self.fail_at.load(Ordering::SeqCst) {
            // Transient error: the device keeps working afterwards.
            return Poll::Ready(Err(IoErrorKind::InputOutput));
        }
        if count == self.torn_at.load(Ordering::SeqCst) {
            // Torn program: half of the buffer lands, then the power is gone.
            let half = &buffer[..buffer.len() / 2];
            let applied = self.inner.write(cx, half, position);
            self.failed.store(true, Ordering::SeqCst);
            if let Poll::Ready(Err(error)) = applied {
                return Poll::Ready(Err(error));
            }
            return Poll::Ready(Err(IoErrorKind::InputOutput));
        }
        self.inner.write(cx, buffer, position)
    }

    fn control(
        &self,
        cx: &mut TaskContext<'_>,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Poll<Result<()>> {
        if command == block_device::ERASE::IDENTIFIER {
            // After the power cut, erases fail too.
            if self.failed.load(Ordering::SeqCst) {
                return Poll::Ready(Err(IoErrorKind::InputOutput));
            }
            self.erases.fetch_add(1, Ordering::SeqCst);
            let range = block_device::ERASE::cast_input(input)?;
            let block = alloc::vec![0u8; range.size as usize];
            // Zero the block directly on the inner device: an erase is not a
            // program operation and does not count towards the commit limit.
            return match self.inner.write(cx, &block, range.position) {
                Poll::Ready(Ok(_)) => Poll::Ready(Ok(())),
                Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
                Poll::Pending => Poll::Pending,
            };
        }
        self.inner.control(cx, command, input, output)
    }

    fn cancel_operation(&self, _operation: Operation) {}
}

impl MountOperations for FaultDevice {}
impl DirectBlockDevice for FaultDevice {}

const CACHE_SIZE: usize = 256;
const DEVICE_SIZE: usize = 512 * 128;

fn options() -> Options {
    Options::new(CACHE_SIZE)
}

fn format_and_mount(device: &'static FaultDevice) -> FileSystem {
    FileSystem::format(device, options()).unwrap();
    FileSystem::new(device, options()).unwrap()
}

fn creation_attributes() -> Attributes {
    Attributes::new()
        .set_user(UserIdentifier::new(42))
        .set_group(GroupIdentifier::new(43))
        .set_permissions(Permissions::USER_READ_WRITE)
        .set_creation(Time::new(1_000))
        .set_modification(Time::new(2_000))
        .set_access(Time::new(3_000))
        .set_status(Time::new(4_000))
        .set_kind(Kind::File)
}

fn owner_mask() -> AttributeFlags {
    AttributeFlags::User | AttributeFlags::Group | AttributeFlags::Permissions
}

fn read_owner(path: &Path, fs: &FileSystem) -> Result<Attributes> {
    let mut attributes = Attributes::new().set_mask(owner_mask());
    file_system::FileSystemOperations::get_attributes(fs, path, &mut attributes)?;
    Ok(attributes)
}

/// After a power cut during a create, remount and classify the entry: absent,
/// complete with exactly the requested attributes, or attribute-less (fail
/// closed, but present and removable).
fn classify_after_cut(device: &'static FaultDevice, path: &Path) -> CutState {
    device.allow_all();
    let fs = FileSystem::new(device, options()).unwrap();
    let state = match read_owner(path, &fs) {
        Err(IoErrorKind::NotFound) => CutState::Absent,
        Ok(attributes) => {
            assert_eq!(attributes.get_user(), Some(&UserIdentifier::new(42)));
            assert_eq!(attributes.get_group(), Some(&GroupIdentifier::new(43)));
            assert_eq!(
                attributes.get_permissions(),
                Some(&Permissions::USER_READ_WRITE)
            );
            CutState::Complete
        }
        Err(IoErrorKind::MissingAttribute) => {
            // Fail closed for stored metadata, but the name exists and the
            // kind is still reported from the directory entry.
            let mut kind = Attributes::new().set_mask(AttributeFlags::Kind | AttributeFlags::Inode);
            file_system::FileSystemOperations::get_attributes(&fs, path, &mut kind).unwrap();
            assert_eq!(kind.get_kind(), Some(&Kind::File));
            assert_eq!(kind.get_inode(), Some(&0));
            CutState::AttributeLess
        }
        Err(error) => panic!("unexpected error after power loss: {error:?}"),
    };
    std::mem::forget(fs);
    state
}

enum CutState {
    Absent,
    Complete,
    AttributeLess,
}

#[test]
fn create_commits_file_and_attributes_together() {
    let device = FaultDevice::new(DEVICE_SIZE);
    let fs = format_and_mount(device);
    let path = Path::new("/file");

    device.reset_progs();
    let before = device.progs();
    file_system::FileSystemOperations::create_file(&fs, path, &creation_attributes()).unwrap();
    // Two commits: the entry commit, then the struct plus attributes commit.
    // The previous implementation needed a third commit to rewrite the
    // default root/none attributes with the real ones.
    assert_eq!(device.progs() - before, 2);

    let attributes = read_owner(path, &fs).unwrap();
    assert_eq!(attributes.get_user(), Some(&UserIdentifier::new(42)));
    assert_eq!(attributes.get_group(), Some(&GroupIdentifier::new(43)));
    assert_eq!(
        attributes.get_permissions(),
        Some(&Permissions::USER_READ_WRITE)
    );

    let mut times = Attributes::new().set_mask(
        AttributeFlags::CreationTime
            | AttributeFlags::ModificationTime
            | AttributeFlags::AccessTime
            | AttributeFlags::StatusTime
            | AttributeFlags::Kind,
    );
    file_system::FileSystemOperations::get_attributes(&fs, path, &mut times).unwrap();
    assert_eq!(times.get_creation(), Some(&Time::new(1_000)));
    assert_eq!(times.get_modification(), Some(&Time::new(2_000)));
    assert_eq!(times.get_access(), Some(&Time::new(3_000)));
    assert_eq!(times.get_status(), Some(&Time::new(4_000)));
    assert_eq!(times.get_kind(), Some(&Kind::File));
}

#[test]
fn create_rejects_unsupported_attribute_masks() {
    let device = FaultDevice::new(DEVICE_SIZE);
    let fs = format_and_mount(device);
    let path = Path::new("/file");
    for mask in [AttributeFlags::Size, AttributeFlags::Links] {
        let attributes = Attributes::new().set_size(8).set_links(2).set_mask(mask);
        assert_eq!(
            file_system::FileSystemOperations::create_file(&fs, path, &attributes),
            Err(IoErrorKind::Unsupported)
        );
    }
}

#[test]
fn open_for_write_without_write_costs_no_commit() {
    let device = FaultDevice::new(DEVICE_SIZE);
    let fs = format_and_mount(device);
    let path = Path::new("/file");
    file_system::FileSystemOperations::create_file(&fs, path, &creation_attributes()).unwrap();

    // Open for writing and close without writing: no metadata commit.
    device.reset_progs();
    fs.operation(|native| {
        let mut file = File::lookup(native, path, AccessFlags::READ_WRITE.into()).unwrap();
        file.close(native)
    })
    .unwrap();
    assert_eq!(device.progs(), 0);

    // A zero-byte write is not a mutation either.
    device.reset_progs();
    fs.operation(|native| {
        let mut file = File::lookup(native, path, AccessFlags::READ_WRITE.into()).unwrap();
        assert_eq!(file.write(native, &[], 0).unwrap(), 0);
        file.close(native)
    })
    .unwrap();
    assert_eq!(device.progs(), 0);

    // A read-only open and close never commits.
    device.reset_progs();
    fs.operation(|native| {
        let mut file = File::lookup(native, path, AccessFlags::Read.into()).unwrap();
        file.close(native)
    })
    .unwrap();
    assert_eq!(device.progs(), 0);

    // Write plus close: exactly one commit, carrying data and attributes.
    device.reset_progs();
    fs.operation(|native| {
        let mut file = File::lookup(native, path, AccessFlags::READ_WRITE.into()).unwrap();
        assert_eq!(file.write(native, b"payload", 0).unwrap(), 7);
        file.close(native)
    })
    .unwrap();
    assert_eq!(device.progs(), 1);

    // Write, explicit flush, close: still one commit (the close is a no-op).
    device.reset_progs();
    fs.operation(|native| {
        let mut file = File::lookup(native, path, AccessFlags::READ_WRITE.into()).unwrap();
        assert_eq!(file.write(native, b"more", 7).unwrap(), 4);
        file.flush(native)?;
        file.close(native)
    })
    .unwrap();
    assert_eq!(device.progs(), 1);

    // Metadata change on a handle with pending data rides along with the data
    // commit instead of adding another one.
    device.reset_progs();
    fs.operation(|native| {
        let mut file = File::lookup(native, path, AccessFlags::READ_WRITE.into()).unwrap();
        assert_eq!(file.write(native, b"x", 0).unwrap(), 1);
        file.set_attributes(
            native,
            &Attributes::new().set_permissions(Permissions::ALL_READ_WRITE),
        )?;
        file.close(native)
    })
    .unwrap();
    assert_eq!(device.progs(), 1);
    let attributes = read_owner(path, &fs).unwrap();
    assert_eq!(
        attributes.get_permissions(),
        Some(&Permissions::ALL_READ_WRITE)
    );
}

#[test]
fn several_writes_with_time_touch_then_close_is_one_commit() {
    let device = FaultDevice::new(DEVICE_SIZE);
    let fs = format_and_mount(device);
    let path = Path::new("/file");
    file_system::FileSystemOperations::create_file(&fs, path, &creation_attributes()).unwrap();

    // The VFS records the modification time on the handle after the first
    // write; here each write is followed by such a metadata update. All of
    // them ride along with the single data commit on close: LittleFS sets
    // LFS_F_WRITING (not only LFS_F_DIRTY) on write, and both count as
    // pending, so no intermediate sync happens.
    device.reset_progs();
    fs.operation(|native| {
        let mut file = File::lookup(native, path, AccessFlags::READ_WRITE.into()).unwrap();
        for index in 0..20u64 {
            assert_eq!(file.write(native, b"x", index).unwrap(), 1);
            file.set_attributes(
                native,
                &Attributes::new()
                    .set_modification(Time::new(10_000 + index))
                    .set_status(Time::new(10_000 + index)),
            )?;
        }
        file.close(native)
    })
    .unwrap();
    assert_eq!(device.progs(), 1);

    let mut times = Attributes::new().set_mask(AttributeFlags::ModificationTime);
    file_system::FileSystemOperations::get_attributes(&fs, path, &mut times).unwrap();
    assert_eq!(times.get_modification(), Some(&Time::new(10_019)));
}

#[test]
fn clean_handle_close_does_not_revert_pending_times_of_another_handle() {
    let device = FaultDevice::new(DEVICE_SIZE);
    let fs = format_and_mount(device);
    let path = Path::new("/file");
    file_system::FileSystemOperations::create_file(&fs, path, &creation_attributes()).unwrap();

    fs.operation(|native| {
        // Handle A: truncate open, then record new times. Both stay pending on
        // A until its close commits them.
        let flags = Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Truncate), None);
        let mut a = File::lookup(native, path, flags).unwrap();
        a.set_attributes(
            native,
            &Attributes::new()
                .set_modification(Time::new(9_000))
                .set_status(Time::new(9_000)),
        )?;
        // Handle B: read-only open and close. B has no pending state; its close
        // must not publish its stale attribute copy over A's pending times.
        let mut b = File::lookup(native, path, AccessFlags::Read.into()).unwrap();
        b.close(native)?;
        a.close(native)
    })
    .unwrap();

    let mut times = Attributes::new().set_mask(AttributeFlags::ModificationTime);
    file_system::FileSystemOperations::get_attributes(&fs, path, &mut times).unwrap();
    assert_eq!(times.get_modification(), Some(&Time::new(9_000)));
}

#[test]
fn truncate_open_updates_times_in_the_same_commit() {
    let device = FaultDevice::new(DEVICE_SIZE);
    let fs = format_and_mount(device);
    let path = Path::new("/file");
    file_system::FileSystemOperations::create_file(&fs, path, &creation_attributes()).unwrap();
    fs.operation(|native| {
        let mut file = File::lookup(native, path, AccessFlags::READ_WRITE.into()).unwrap();
        file.write(native, b"payload", 0)?;
        file.close(native)
    })
    .unwrap();

    // Open with O_TRUNC and record the mutation time on the handle: the
    // truncation and the new modification time are committed together.
    device.reset_progs();
    fs.operation(|native| {
        let flags = Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Truncate), None);
        let mut file = File::lookup(native, path, flags).unwrap();
        file.set_attributes(
            native,
            &Attributes::new()
                .set_modification(Time::new(9_000))
                .set_status(Time::new(9_000)),
        )?;
        file.close(native)
    })
    .unwrap();
    assert_eq!(device.progs(), 1);

    let mut times = Attributes::new().set_mask(AttributeFlags::ModificationTime);
    file_system::FileSystemOperations::get_attributes(&fs, path, &mut times).unwrap();
    assert_eq!(times.get_modification(), Some(&Time::new(9_000)));
    let mut size = Attributes::new().set_mask(AttributeFlags::Size);
    file_system::FileSystemOperations::get_attributes(&fs, path, &mut size).unwrap();
    assert_eq!(size.get_size(), Some(&0));
}

#[test]
fn create_directory_commits_attributes_with_the_entry() {
    let device = FaultDevice::new(DEVICE_SIZE);
    let fs = format_and_mount(device);
    let path = Path::new("/directory");

    let attributes = Attributes::new()
        .set_user(UserIdentifier::new(7))
        .set_group(GroupIdentifier::new(8))
        .set_permissions(Permissions::DIRECTORY_DEFAULT)
        .set_modification(Time::new(5_000))
        .set_kind(Kind::Directory);

    device.reset_progs();
    let before = device.progs();
    let erases_before = device.erases.load(Ordering::SeqCst);
    file_system::FileSystemOperations::create_directory(&fs, path, &attributes).unwrap();
    // `lfs_mkdir` has no attribute configuration: one commit for the directory
    // entry (plus a metadata relocation erase when the root pair is full) and
    // one for the attributes. The window between them leaves the directory
    // without attributes (fail closed), never root with no rights.
    let progs = device.progs() - before;
    let erases = device.erases.load(Ordering::SeqCst) - erases_before;
    assert_eq!(progs, 3);
    assert!(erases >= 1);

    let mut read = Attributes::new().set_mask(
        AttributeFlags::User
            | AttributeFlags::Group
            | AttributeFlags::Permissions
            | AttributeFlags::Kind
            | AttributeFlags::ModificationTime,
    );
    file_system::FileSystemOperations::get_attributes(&fs, path, &mut read).unwrap();
    assert_eq!(read.get_user(), Some(&UserIdentifier::new(7)));
    assert_eq!(read.get_group(), Some(&GroupIdentifier::new(8)));
    assert_eq!(read.get_kind(), Some(&Kind::Directory));
    assert_eq!(read.get_modification(), Some(&Time::new(5_000)));
}

#[test]
fn power_loss_during_create_never_leaves_wrong_attributes() {
    let path = Path::new("/file");

    // Total program operations a create needs on a freshly mounted device.
    let probe = FaultDevice::new(DEVICE_SIZE);
    let fs = format_and_mount(probe);
    probe.reset_progs();
    file_system::FileSystemOperations::create_file(&fs, path, &creation_attributes()).unwrap();
    let total = probe.progs();
    assert_eq!(total, 2);
    std::mem::forget(fs);

    for allowed in 0..=total {
        let device = FaultDevice::new(DEVICE_SIZE);
        let fs = format_and_mount(device);
        device.reset_progs();
        device.fail_after(allowed);
        // The create may fail when the power loss hits inside it.
        let _ = file_system::FileSystemOperations::create_file(&fs, path, &creation_attributes());
        // Simulate the power loss: no unmount, no further writes.
        std::mem::forget(fs);

        match classify_after_cut(device, path) {
            // The file does not exist.
            CutState::Absent => {}
            // The file exists with exactly the requested owner and permissions.
            CutState::Complete => {}
            // The entry commit landed but the attribute commit did not: the
            // entry exists without attributes and fails closed. It is never
            // reported with wrong (for example root/none) attributes, its kind
            // is still available, and it can be removed through its parent.
            CutState::AttributeLess => {
                let fs = FileSystem::new(device, options()).unwrap();
                fs.operation(|native| {
                    let mut info = crate::littlefs::lfs_info::default();
                    let result =
                        unsafe { crate::littlefs::lfs_stat(native, c"/file".as_ptr(), &mut info) };
                    assert_eq!(result, 0, "entry exists without attributes");
                    Ok(())
                })
                .unwrap();
                file_system::FileSystemOperations::remove(&fs, path).unwrap();
                std::mem::forget(fs);
            }
        }
    }
}

#[test]
fn torn_program_during_create_never_leaves_wrong_attributes() {
    let path = Path::new("/file");

    let probe = FaultDevice::new(DEVICE_SIZE);
    let fs = format_and_mount(probe);
    probe.reset_progs();
    file_system::FileSystemOperations::create_file(&fs, path, &creation_attributes()).unwrap();
    let total = probe.progs();
    std::mem::forget(fs);

    // Fail the n-th program operation half-applied: LittleFS rejects the torn
    // commit on remount through its metadata CRC, so the outcome is the same
    // as a clean cut at that point.
    for torn in 1..=total {
        let device = FaultDevice::new(DEVICE_SIZE);
        let fs = format_and_mount(device);
        device.reset_progs();
        device.torn_at(torn);
        let _ = file_system::FileSystemOperations::create_file(&fs, path, &creation_attributes());
        std::mem::forget(fs);

        match classify_after_cut(device, path) {
            CutState::Absent | CutState::Complete | CutState::AttributeLess => {}
        }
    }
}

#[test]
fn power_loss_during_directory_create_never_leaves_wrong_attributes() {
    let path = Path::new("/directory");

    let attributes = || {
        Attributes::new()
            .set_user(UserIdentifier::new(7))
            .set_group(GroupIdentifier::new(8))
            .set_permissions(Permissions::DIRECTORY_DEFAULT)
            .set_modification(Time::new(5_000))
            .set_kind(Kind::Directory)
    };

    let probe = FaultDevice::new(DEVICE_SIZE);
    let fs = format_and_mount(probe);
    probe.reset_progs();
    file_system::FileSystemOperations::create_directory(&fs, path, &attributes()).unwrap();
    let total = probe.progs();
    std::mem::forget(fs);

    for allowed in 0..=total {
        let device = FaultDevice::new(DEVICE_SIZE);
        let fs = format_and_mount(device);
        device.reset_progs();
        device.fail_after(allowed);
        let _ = file_system::FileSystemOperations::create_directory(&fs, path, &attributes());
        std::mem::forget(fs);

        device.allow_all();
        let fs = FileSystem::new(device, options()).unwrap();
        let mut read = Attributes::new().set_mask(owner_mask());
        match file_system::FileSystemOperations::get_attributes(&fs, path, &mut read) {
            Err(IoErrorKind::NotFound) => {}
            Ok(()) => {
                assert_eq!(read.get_user(), Some(&UserIdentifier::new(7)));
                assert_eq!(read.get_group(), Some(&GroupIdentifier::new(8)));
                assert_eq!(
                    read.get_permissions(),
                    Some(&Permissions::DIRECTORY_DEFAULT)
                );
            }
            Err(IoErrorKind::MissingAttribute) => {
                // The directory entry exists without attributes: kind is still
                // reported from the directory entry, and the entry is removable.
                let mut kind =
                    Attributes::new().set_mask(AttributeFlags::Kind | AttributeFlags::Inode);
                file_system::FileSystemOperations::get_attributes(&fs, path, &mut kind).unwrap();
                assert_eq!(kind.get_kind(), Some(&Kind::Directory));
                assert_eq!(kind.get_inode(), Some(&0));
                file_system::FileSystemOperations::remove(&fs, path).unwrap();
            }
            Err(error) => panic!("unexpected error after power loss: {error:?}"),
        }
        std::mem::forget(fs);
    }
}

#[test]
fn failed_create_removes_the_half_created_entry() {
    let path = Path::new("/file");

    let probe = FaultDevice::new(DEVICE_SIZE);
    let fs = format_and_mount(probe);
    probe.reset_progs();
    file_system::FileSystemOperations::create_file(&fs, path, &creation_attributes()).unwrap();
    let total = probe.progs();
    std::mem::forget(fs);

    // A transient I/O error (not a power loss) at any program operation of the
    // create: the half-created name is removed again, so the path is absent.
    for failure in 1..=total {
        let device = FaultDevice::new(DEVICE_SIZE);
        let fs = format_and_mount(device);
        device.reset_progs();
        device.fail_at(failure);
        let result =
            file_system::FileSystemOperations::create_file(&fs, path, &creation_attributes());
        assert!(result.is_err(), "create must fail at program {failure}");
        device.allow_all();
        let mut kind = Attributes::new().set_mask(AttributeFlags::Kind);
        assert_eq!(
            file_system::FileSystemOperations::get_attributes(&fs, path, &mut kind),
            Err(IoErrorKind::NotFound),
            "half-created entry must be removed after failure at program {failure}"
        );
        std::mem::forget(fs);
    }
}

#[test]
fn failed_directory_create_removes_the_half_created_entry() {
    let path = Path::new("/directory");

    let attributes = || {
        Attributes::new()
            .set_user(UserIdentifier::new(7))
            .set_kind(Kind::Directory)
    };

    let probe = FaultDevice::new(DEVICE_SIZE);
    let fs = format_and_mount(probe);
    probe.reset_progs();
    file_system::FileSystemOperations::create_directory(&fs, path, &attributes()).unwrap();
    let total = probe.progs();
    std::mem::forget(fs);

    for failure in 1..=total {
        let device = FaultDevice::new(DEVICE_SIZE);
        let fs = format_and_mount(device);
        device.reset_progs();
        device.fail_at(failure);
        let result = file_system::FileSystemOperations::create_directory(&fs, path, &attributes());
        assert!(result.is_err(), "create must fail at program {failure}");
        device.allow_all();
        let mut kind = Attributes::new().set_mask(AttributeFlags::Kind);
        assert_eq!(
            file_system::FileSystemOperations::get_attributes(&fs, path, &mut kind),
            Err(IoErrorKind::NotFound),
            "half-created entry must be removed after failure at program {failure}"
        );
        std::mem::forget(fs);
    }
}

#[test]
fn read_fault_during_create_never_removes_a_preexisting_entry() {
    // Scenario A: a pre-existing file with data. First measure how many reads
    // a create on the populated device performs (it fails with
    // AlreadyExists, but the reads are the fault points that matter).
    let probe = FaultDevice::new(DEVICE_SIZE);
    let fs = format_and_mount(probe);
    let path = Path::new("/existing");
    file_system::FileSystemOperations::create_file(&fs, path, &creation_attributes()).unwrap();
    fs.operation(|native| {
        let mut file = File::lookup(native, path, AccessFlags::READ_WRITE.into()).unwrap();
        file.write(native, b"precious", 0)?;
        file.close(native)
    })
    .unwrap();
    probe.reset_reads();
    let result = file_system::FileSystemOperations::create_file(&fs, path, &creation_attributes());
    assert!(matches!(result, Err(IoErrorKind::AlreadyExists)));
    let total_reads = probe.reads();
    assert!(total_reads > 0);
    std::mem::forget(fs);

    for fault in 1..=total_reads {
        let device = FaultDevice::new(DEVICE_SIZE);
        let fs = format_and_mount(device);
        file_system::FileSystemOperations::create_file(&fs, path, &creation_attributes()).unwrap();
        fs.operation(|native| {
            let mut file = File::lookup(native, path, AccessFlags::READ_WRITE.into()).unwrap();
            file.write(native, b"precious", 0)?;
            file.close(native)
        })
        .unwrap();
        device.reset_reads();
        device.fail_read_at(fault);
        // The create may fail, or may even report success: a failed read
        // poisons the LittleFS read cache, so a later lookup can miss the
        // existing entry and the exclusive create adds a duplicate name. What
        // must never happen is the old entry being removed.
        let _ = file_system::FileSystemOperations::create_file(&fs, path, &creation_attributes());
        device.allow_all();
        // Remount with a fresh read cache before checking: the poisoned cache
        // of the faulted mount can hide the entry even though it is on disk.
        std::mem::forget(fs);
        let fs = FileSystem::new(device, options()).unwrap();
        // The pre-existing file is still there, with its owner and its data.
        let mut owner = Attributes::new().set_mask(owner_mask());
        file_system::FileSystemOperations::get_attributes(&fs, path, &mut owner)
            .unwrap_or_else(|e| panic!("fault {fault}: get_attributes -> {e:?}"));
        assert_eq!(owner.get_user(), Some(&UserIdentifier::new(42)));
        fs.operation(|native| {
            let mut file = File::lookup(native, path, AccessFlags::Read.into()).unwrap();
            let mut buffer = [0u8; 8];
            let read = file
                .read(native, &mut buffer, 0)
                .unwrap_or_else(|e| panic!("fault {fault}: read -> {e:?}"));
            assert_eq!(read, 8, "fault {fault}");
            assert_eq!(&buffer, b"precious");
            file.close(native)
        })
        .unwrap();
        std::mem::forget(fs);
    }

    // Scenario B: a pre-existing directory, same sweep.
    let probe = FaultDevice::new(DEVICE_SIZE);
    let fs = format_and_mount(probe);
    let directory = Path::new("/existing-dir");
    file_system::FileSystemOperations::create_directory(
        &fs,
        directory,
        &Attributes::new()
            .set_user(UserIdentifier::new(7))
            .set_kind(Kind::Directory),
    )
    .unwrap();
    probe.reset_reads();
    let result = file_system::FileSystemOperations::create_directory(
        &fs,
        directory,
        &Attributes::new().set_kind(Kind::Directory),
    );
    assert!(matches!(result, Err(IoErrorKind::AlreadyExists)));
    let total_reads = probe.reads();
    assert!(total_reads > 0);
    std::mem::forget(fs);

    for fault in 1..=total_reads {
        let device = FaultDevice::new(DEVICE_SIZE);
        let fs = format_and_mount(device);
        file_system::FileSystemOperations::create_directory(
            &fs,
            directory,
            &Attributes::new()
                .set_user(UserIdentifier::new(7))
                .set_kind(Kind::Directory),
        )
        .unwrap();
        device.reset_reads();
        device.fail_read_at(fault);
        // Same as for files: the mkdir may fail or report success through a
        // poisoned read cache, but the pre-existing directory must survive.
        let _ = file_system::FileSystemOperations::create_directory(
            &fs,
            directory,
            &Attributes::new().set_kind(Kind::Directory),
        );
        device.allow_all();
        // Remount with a fresh read cache before checking.
        std::mem::forget(fs);
        let fs = FileSystem::new(device, options()).unwrap();
        let mut owner = Attributes::new().set_mask(owner_mask());
        file_system::FileSystemOperations::get_attributes(&fs, directory, &mut owner)
            .unwrap_or_else(|e| panic!("fault {fault}: get_attributes -> {e:?}"));
        assert_eq!(owner.get_user(), Some(&UserIdentifier::new(7)));
        std::mem::forget(fs);
    }
}

#[test]
fn size_change_is_committed_at_once_and_visible_immediately() {
    let device = FaultDevice::new(DEVICE_SIZE);
    let fs = format_and_mount(device);
    let path = Path::new("/file");
    file_system::FileSystemOperations::create_file(&fs, path, &creation_attributes()).unwrap();
    fs.operation(|native| {
        let mut file = File::lookup(native, path, AccessFlags::READ_WRITE.into()).unwrap();
        file.write(native, b"0123456789", 0)?;
        file.close(native)
    })
    .unwrap();

    // A size change is committed at the call, not deferred to close: a path
    // stat and a freshly opened handle see the new size right away.
    device.reset_progs();
    fs.operation(|native| {
        let mut file = File::lookup(native, path, AccessFlags::READ_WRITE.into()).unwrap();
        file.set_attributes(native, &Attributes::new().set_size(3))?;
        assert_eq!(device.progs(), 1, "size change commits at once");
        file.close(native)
    })
    .unwrap();
    assert_eq!(
        device.progs(),
        1,
        "close after a committed size change adds nothing"
    );

    let mut size = Attributes::new().set_mask(AttributeFlags::Size);
    file_system::FileSystemOperations::get_attributes(&fs, path, &mut size).unwrap();
    assert_eq!(size.get_size(), Some(&3));
    fs.operation(|native| {
        let mut file = File::lookup(native, path, AccessFlags::Read.into()).unwrap();
        let mut buffer = [0u8; 16];
        assert_eq!(file.read(native, &mut buffer, 0).unwrap(), 3);
        file.close(native)
    })
    .unwrap();
}

#[test]
fn permission_change_with_pending_write_commits_at_once() {
    let device = FaultDevice::new(DEVICE_SIZE);
    let fs = format_and_mount(device);
    let path = Path::new("/file");
    file_system::FileSystemOperations::create_file(&fs, path, &creation_attributes()).unwrap();

    // A handle-level chmod with a pending write is committed at the call, so
    // path checks by other openers see the new permissions immediately.
    device.reset_progs();
    fs.operation(|native| {
        let mut file = File::lookup(native, path, AccessFlags::READ_WRITE.into()).unwrap();
        assert_eq!(file.write(native, b"x", 0)?, 1);
        assert_eq!(device.progs(), 0, "the write itself is still pending");
        file.set_attributes(
            native,
            &Attributes::new().set_permissions(Permissions::ALL_READ_WRITE),
        )?;
        assert_eq!(
            device.progs(),
            1,
            "the chmod commits data and metadata at once"
        );
        file.close(native)
    })
    .unwrap();
    assert_eq!(
        device.progs(),
        1,
        "close adds nothing after the chmod commit"
    );
    let attributes = read_owner(path, &fs).unwrap();
    assert_eq!(
        attributes.get_permissions(),
        Some(&Permissions::ALL_READ_WRITE)
    );
}

#[test]
fn time_change_with_pending_write_still_defers_to_close() {
    let device = FaultDevice::new(DEVICE_SIZE);
    let fs = format_and_mount(device);
    let path = Path::new("/file");
    file_system::FileSystemOperations::create_file(&fs, path, &creation_attributes()).unwrap();

    // Time-only changes keep riding along with pending data: 20 writes with a
    // time update after each, then close, cost exactly one commit.
    device.reset_progs();
    fs.operation(|native| {
        let mut file = File::lookup(native, path, AccessFlags::READ_WRITE.into()).unwrap();
        for index in 0..20u64 {
            assert_eq!(file.write(native, b"x", index)?, 1);
            file.set_attributes(
                native,
                &Attributes::new()
                    .set_modification(Time::new(10_000 + index))
                    .set_status(Time::new(10_000 + index)),
            )?;
        }
        file.close(native)
    })
    .unwrap();
    assert_eq!(device.progs(), 1);
}

#[test]
fn open_file_context_size_is_stable() {
    // The attribute buffer lives in the open file context for as long as the
    // file is open. Assert its size so growth is caught by the tests: 160
    // bytes on 32-bit targets, 200 bytes on 64-bit hosts.
    let expected = if core::mem::size_of::<usize>() == 4 {
        160
    } else {
        200
    };
    assert_eq!(core::mem::size_of::<File>(), expected);
    assert_eq!(
        core::mem::size_of::<crate::attributes::InternalAttributes>(),
        48
    );
}
