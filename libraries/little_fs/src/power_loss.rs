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
/// program's buffer and then fails like a power loss. `fail_read_at` with
/// `failing_read_length` fails a window of consecutive reads starting at that read,
/// then the device recovers.
struct FaultDevice {
    inner: MemoryDevice<512>,
    progs: AtomicUsize,
    erases: AtomicUsize,
    reads: AtomicUsize,
    fail_after: AtomicUsize,
    fail_at: AtomicUsize,
    torn_at: AtomicUsize,
    fail_read_at: AtomicUsize,
    failing_read_length: AtomicUsize,
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
            failing_read_length: AtomicUsize::new(1),
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

    /// Fail the `count`-th read and the `len` reads after it (a transient read
    /// error window), then recover.
    fn fail_read_at(&self, count: usize) {
        self.fail_read_at.store(count, Ordering::SeqCst);
    }

    fn fail_read_window(&self, count: usize, len: usize) {
        self.fail_read_at.store(count, Ordering::SeqCst);
        self.failing_read_length.store(len, Ordering::SeqCst);
    }

    fn allow_all(&self) {
        self.fail_after.store(usize::MAX, Ordering::SeqCst);
        self.fail_at.store(usize::MAX, Ordering::SeqCst);
        self.torn_at.store(usize::MAX, Ordering::SeqCst);
        self.fail_read_at.store(usize::MAX, Ordering::SeqCst);
        self.failing_read_length.store(1, Ordering::SeqCst);
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
        let start = self.fail_read_at.load(Ordering::SeqCst);
        if start != usize::MAX
            && count >= start
            && count - start < self.failing_read_length.load(Ordering::SeqCst)
        {
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

// ---------------------------------------------------------------------------
// Randomised read-fault sweep: random trees with churn, one read-error window
// injected at every read of a create over an existing entry. The cleanup of a
// failed create must never remove or alter a pre-existing entry. The fixed
// seed set includes the seeds that reproduced removals before the read cache
// was emptied before the cleanup probe.
// ---------------------------------------------------------------------------
mod randomised_sweep {
    use super::*;
    use crate::Directory;
    use alloc::string::{String, ToString};
    use alloc::vec::Vec;
    use file_system::FileSystemOperations;

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    fn sweep_attributes(uid: u16) -> Attributes {
        Attributes::new()
            .set_user(UserIdentifier::new(uid))
            .set_group(GroupIdentifier::new(43))
            .set_permissions(Permissions::USER_READ_WRITE)
            .set_kind(Kind::File)
    }

    fn sweep_dir_attributes(uid: u16) -> Attributes {
        Attributes::new()
            .set_user(UserIdentifier::new(uid))
            .set_kind(Kind::Directory)
    }

    fn put(fs: &FileSystem, path: &str, data: &[u8]) {
        FileSystemOperations::create_file(fs, Path::new(path), &sweep_attributes(42)).unwrap();
        if !data.is_empty() {
            fs.operation(|native| {
                let mut file =
                    File::lookup(native, Path::new(path), AccessFlags::READ_WRITE.into())?;
                file.write(native, data, 0)?;
                file.close(native)
            })
            .unwrap();
        }
    }

    fn mkdir(fs: &FileSystem, path: &str) {
        FileSystemOperations::create_directory(fs, Path::new(path), &sweep_dir_attributes(7))
            .unwrap();
    }

    /// Build a random tree with random churn; return the directories and files.
    fn random_layout(fs: &FileSystem, seed: u64) -> (Vec<String>, Vec<String>) {
        let mut rng = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
        let mut dirs: Vec<String> = alloc::vec!["/".to_string()];
        let mut files: Vec<String> = Vec::new();
        let mut counter = 0;
        let steps = 8 + rng.below(40);
        for _ in 0..steps {
            let parent = dirs[rng.below(dirs.len() as u64) as usize].clone();
            let name = alloc::format!("n{counter}");
            counter += 1;
            let path = if parent == "/" {
                alloc::format!("/{name}")
            } else {
                alloc::format!("{parent}/{name}")
            };
            match rng.below(10) {
                0..=3 => {
                    let size = match rng.below(4) {
                        0 => 0,
                        1 => rng.below(40),
                        2 => rng.below(300),
                        _ => rng.below(1500),
                    } as usize;
                    let data: Vec<u8> = (0..size).map(|i| (i as u64 ^ rng.next()) as u8).collect();
                    put(fs, &path, &data);
                    files.push(path);
                }
                4..=5 => {
                    if dirs.len() < 6 {
                        mkdir(fs, &path);
                        dirs.push(path);
                    }
                }
                6 => {
                    if !files.is_empty() {
                        let file = files[rng.below(files.len() as u64) as usize].clone();
                        let _ = FileSystemOperations::set_attributes(
                            fs,
                            Path::new(&file),
                            &Attributes::new().set_permissions(Permissions::ALL_READ_WRITE),
                        );
                    }
                }
                7 => {
                    if files.len() > 2 {
                        let index = rng.below(files.len() as u64) as usize;
                        let file = files.remove(index);
                        FileSystemOperations::remove(fs, Path::new(&file)).unwrap();
                    }
                }
                8 => {
                    if !files.is_empty() {
                        let file = files[rng.below(files.len() as u64) as usize].clone();
                        let extra = alloc::vec![7u8; rng.below(600) as usize + 1];
                        fs.operation(|native| {
                            let mut handle = File::lookup(
                                native,
                                Path::new(&file),
                                AccessFlags::READ_WRITE.into(),
                            )?;
                            handle.write(native, &extra, 0)?;
                            handle.close(native)
                        })
                        .unwrap();
                    }
                }
                _ => {
                    // Directory churn: add and remove a temporary file to force
                    // commits and metadata-pair compaction.
                    let temporary = alloc::format!(
                        "{}/tmp{counter}",
                        if parent == "/" { "" } else { parent.as_str() }
                    );
                    put(fs, &temporary, b"t");
                    FileSystemOperations::remove(fs, Path::new(&temporary)).unwrap();
                }
            }
        }
        (dirs, files)
    }

    fn walk(fs: &FileSystem, directory: &str, out: &mut Vec<String>) {
        let names: Vec<(String, Kind, u64)> = fs
            .operation(|native| {
                let mut dir = Directory::lookup(native, Path::new(directory))?;
                let mut entries = Vec::new();
                while let Some(entry) = dir.read(native)? {
                    if entry.name == "." || entry.name == ".." {
                        continue;
                    }
                    entries.push((entry.name.clone(), entry.kind, entry.size));
                }
                dir.close(native)?;
                Ok(entries)
            })
            .unwrap_or_else(|error| panic!("walk {directory}: {error:?}"));
        let mut names = names;
        names.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, kind, size) in names {
            let path = if directory == "/" {
                alloc::format!("/{name}")
            } else {
                alloc::format!("{directory}/{name}")
            };
            let mut owner = Attributes::new().set_mask(owner_mask());
            let attributes =
                match FileSystemOperations::get_attributes(fs, Path::new(&path), &mut owner) {
                    Ok(()) => {
                        alloc::format!("{:?}/{:?}", owner.get_user(), owner.get_permissions())
                    }
                    Err(error) => alloc::format!("ERR {error:?}"),
                };
            let content = if kind == Kind::File {
                fs.operation(|native| {
                    let mut file =
                        File::lookup(native, Path::new(&path), AccessFlags::Read.into())?;
                    let mut buffer = alloc::vec![0u8; 4096];
                    let count = file.read(native, &mut buffer, 0)?;
                    file.close(native)?;
                    let sum: u64 = buffer[..count].iter().map(|byte| *byte as u64).sum();
                    Ok(alloc::format!("{count}:{sum}"))
                })
                .unwrap_or_else(|error| alloc::format!("READERR {error:?}"))
            } else {
                String::new()
            };
            out.push(alloc::format!(
                "{path} {kind:?} size={size} attrs={attributes} content={content}"
            ));
            if kind == Kind::Directory {
                walk(fs, &path, out);
            }
        }
    }

    /// Remount with an empty read cache and snapshot the whole tree.
    fn snapshot(device: &'static FaultDevice, cache: usize) -> Vec<String> {
        device.allow_all();
        let fs = FileSystem::new(device, Options::new(cache)).unwrap();
        let mut out = Vec::new();
        walk(&fs, "/", &mut out);
        std::mem::forget(fs);
        out
    }

    #[derive(Clone, Copy, Debug)]
    enum Target {
        File,
        Directory,
    }

    fn build(
        seed: u64,
        cache: usize,
        remount: bool,
    ) -> (&'static FaultDevice, FileSystem, Vec<String>, Vec<String>) {
        let device = FaultDevice::new(DEVICE_SIZE);
        FileSystem::format(device, Options::new(cache)).unwrap();
        let fs = FileSystem::new(device, Options::new(cache)).unwrap();
        let (dirs, files) = random_layout(&fs, seed);
        let fs = if remount {
            std::mem::forget(fs);
            FileSystem::new(device, Options::new(cache)).unwrap()
        } else {
            fs
        };
        (device, fs, dirs, files)
    }

    #[test]
    fn randomised_read_fault_sweep_never_removes_a_preexisting_entry() {
        // Fixed seed set (fast enough for CI) including the seeds that removed
        // a pre-existing entry before the read cache was emptied before the
        // cleanup probe.
        const SEEDS: [u64; 3] = [18, 28, 40];
        let mut faulted_creates = 0usize;
        for seed in SEEDS {
            let cache = [128usize, 256, 512][(seed % 3) as usize];
            for remount in [false, true] {
                let (_probe_device, probe_fs, dirs, files) = build(seed, cache, remount);
                let mut rng = Rng(seed * 7 + 3);
                let mut targets: Vec<(String, Target)> = Vec::new();
                for _ in 0..3 {
                    if !files.is_empty() {
                        let file = files[rng.below(files.len() as u64) as usize].clone();
                        targets.push((file.clone(), Target::File));
                        targets.push((file, Target::Directory));
                    }
                    let directory = dirs[rng.below(dirs.len() as u64) as usize].clone();
                    if directory != "/" {
                        targets.push((directory.clone(), Target::Directory));
                        targets.push((directory, Target::File));
                    }
                }
                std::mem::forget(probe_fs);

                for (target, kind) in targets {
                    let create = |fs: &FileSystem| match kind {
                        Target::File => FileSystemOperations::create_file(
                            fs,
                            Path::new(&target),
                            &sweep_attributes(99),
                        ),
                        Target::Directory => FileSystemOperations::create_directory(
                            fs,
                            Path::new(&target),
                            &sweep_dir_attributes(99),
                        ),
                    };
                    // Probe: how many reads does a create over the existing
                    // entry perform?
                    let (probe_device, probe_fs, _, _) = build(seed, cache, remount);
                    probe_device.reset_reads();
                    let result = create(&probe_fs);
                    assert!(
                        result.is_err(),
                        "seed {seed}: create over existing must fail"
                    );
                    let total = probe_device.reads();
                    std::mem::forget(probe_fs);
                    // Reference tree from a fresh device.
                    let (reference_device, reference_fs, _, _) = build(seed, cache, remount);
                    std::mem::forget(reference_fs);
                    let reference = snapshot(reference_device, cache);

                    for window in [1usize, 2] {
                        for fault in 1..=total {
                            let (device, fs, _, _) = build(seed, cache, remount);
                            device.reset_reads();
                            device.reset_progs();
                            device.fail_read_window(fault, window);
                            let _ = create(&fs);
                            faulted_creates += 1;
                            std::mem::forget(fs);
                            let after = snapshot(device, cache);
                            for line in &reference {
                                if !after.contains(line) {
                                    std::println!(
                                        "seed {seed} cache {cache} remount {remount} target {target} {kind:?} window {window} fault {fault}/{total}: entry removed or changed: {line}"
                                    );
                                    std::println!("REFERENCE ({}):", reference.len());
                                    for l in &reference {
                                        std::println!("  R {l}");
                                    }
                                    std::println!("AFTER ({}):", after.len());
                                    for l in &after {
                                        std::println!("  A {l}");
                                    }
                                    panic!("entry removed or changed");
                                }
                            }
                        }
                    }
                }
            }
        }
        std::println!(
            "randomised sweep: {faulted_creates} faulted creates over existing entries, no pre-existing entry removed"
        );
    }

    /// An entry without any Xila attribute but with data, as an image built by
    /// a host tool would contain.
    fn layout_legacy_with_data(fs: &FileSystem) {
        fs.operation(|native| {
            let mut file = crate::littlefs::lfs_file_t::default();
            let path = alloc::ffi::CString::new("/legacy").unwrap();
            let opened = unsafe {
                crate::littlefs::lfs_file_open(
                    native,
                    &mut file,
                    path.as_ptr(),
                    crate::constants::OPEN_READ_WRITE | crate::constants::OPEN_CREATE,
                )
            };
            assert_eq!(opened, 0);
            let data = b"legacy-precious-data";
            let written = unsafe {
                crate::littlefs::lfs_file_write(
                    native,
                    &mut file,
                    data.as_ptr() as *const _,
                    data.len() as u32,
                )
            };
            assert_eq!(written, data.len() as i32);
            let closed = unsafe { crate::littlefs::lfs_file_close(native, &mut file) };
            assert_eq!(closed, 0);
            Ok(())
        })
        .unwrap();
    }

    fn build_legacy(
        cache: usize,
        remount: bool,
    ) -> (&'static FaultDevice, FileSystem, Vec<String>, Vec<String>) {
        let device = FaultDevice::new(DEVICE_SIZE);
        FileSystem::format(device, Options::new(cache)).unwrap();
        let fs = FileSystem::new(device, Options::new(cache)).unwrap();
        layout_legacy_with_data(&fs);
        let fs = if remount {
            std::mem::forget(fs);
            FileSystem::new(device, Options::new(cache)).unwrap()
        } else {
            fs
        };
        (
            device,
            fs,
            alloc::vec![],
            alloc::vec!["/legacy".to_string()],
        )
    }

    #[test]
    fn legacy_attribute_less_file_with_data_survives_read_faults() {
        // A create over a legacy attribute-less file must never remove it: the
        // pre-create lookup answers AlreadyExists for it. One window-1 read
        // fault during the create deleted it at every sweep before that.
        let mut faulted_creates = 0usize;
        for cache in [128usize, 256, 512] {
            for remount in [false, true] {
                for window in [1usize, 2] {
                    // Probe: reads a create over the existing entry performs.
                    let (probe_device, probe_fs, _, _) = build_legacy(cache, remount);
                    probe_device.reset_reads();
                    let result = FileSystemOperations::create_file(
                        &probe_fs,
                        Path::new("/legacy"),
                        &sweep_attributes(99),
                    );
                    assert!(matches!(result, Err(IoErrorKind::AlreadyExists)));
                    let total = probe_device.reads();
                    std::mem::forget(probe_fs);
                    // Reference tree from a fresh device.
                    let (reference_device, reference_fs, _, _) = build_legacy(cache, remount);
                    std::mem::forget(reference_fs);
                    let reference = snapshot(reference_device, cache);

                    for fault in 1..=total {
                        let (device, fs, _, _) = build_legacy(cache, remount);
                        device.reset_reads();
                        device.reset_progs();
                        device.fail_read_window(fault, window);
                        let _ = FileSystemOperations::create_file(
                            &fs,
                            Path::new("/legacy"),
                            &sweep_attributes(99),
                        );
                        faulted_creates += 1;
                        std::mem::forget(fs);
                        let after = snapshot(device, cache);
                        for line in &reference {
                            assert!(
                                after.contains(line),
                                "legacy cache {cache} remount {remount} window {window} fault {fault}/{total}: entry removed or changed: {line}"
                            );
                        }
                    }
                }
            }
        }
        std::println!(
            "legacy sweep: {faulted_creates} faulted creates over the legacy file, it survived every fault point"
        );
    }

    /// Pick the sweep targets for a seed, as two random file and directory pairs.
    fn pick_targets(seed: u64, dirs: &[String], files: &[String]) -> Vec<(String, Target)> {
        let mut rng = Rng(seed * 7 + 3);
        let mut targets: Vec<(String, Target)> = Vec::new();
        for _ in 0..2 {
            if !files.is_empty() {
                let file = files[rng.below(files.len() as u64) as usize].clone();
                targets.push((file.clone(), Target::File));
                targets.push((file, Target::Directory));
            }
            let directory = dirs[rng.below(dirs.len() as u64) as usize].clone();
            if directory != "/" {
                targets.push((directory.clone(), Target::Directory));
                targets.push((directory, Target::File));
            }
        }
        targets
    }

    fn create_over(
        target: &str,
        kind: Target,
    ) -> impl Fn(&FileSystem) -> file_system::Result<()> + '_ {
        move |fs| match kind {
            Target::File => {
                FileSystemOperations::create_file(fs, Path::new(target), &sweep_attributes(99))
            }
            Target::Directory => FileSystemOperations::create_directory(
                fs,
                Path::new(target),
                &sweep_dir_attributes(99),
            ),
        }
    }

    /// After a faulted create without a remount, the live mount must show the
    /// same tree as a clean mount. Fails without the operation-level cache
    /// drop, where the stale read cache serves wrong bytes after the failed
    /// read.
    #[test]
    fn live_state_after_faulted_create_matches_reference() {
        const SEEDS: [u64; 3] = [1, 2, 3];
        let mut cases = 0usize;
        let mut mismatches = 0usize;
        for seed in SEEDS {
            let cache = [128usize, 256, 512][(seed % 3) as usize];
            let (_d, fs0, dirs, files) = build(seed, cache, true);
            std::mem::forget(fs0);
            for (target, kind) in pick_targets(seed, &dirs, &files) {
                let create = create_over(&target, kind);
                let (probe_device, probe_fs, _, _) = build(seed, cache, true);
                probe_device.reset_reads();
                let _ = create(&probe_fs);
                let total = probe_device.reads();
                std::mem::forget(probe_fs);
                let (reference_device, reference_fs, _, _) = build(seed, cache, true);
                std::mem::forget(reference_fs);
                let reference = snapshot(reference_device, cache);

                for window in [1usize, 2] {
                    for fault in 1..=total {
                        let (device, fs, _, _) = build(seed, cache, true);
                        device.reset_reads();
                        device.fail_read_window(fault, window);
                        let _ = create(&fs);
                        device.allow_all();
                        let mut live = Vec::new();
                        let walked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            walk(&fs, "/", &mut live)
                        }));
                        cases += 1;
                        if walked.is_err() {
                            live.clear();
                            live.push(String::from("WALK ERROR"));
                        }
                        if live != reference {
                            mismatches += 1;
                            if mismatches <= 5 {
                                std::println!(
                                    "LIVE MISMATCH seed {seed} cache {cache} target {target} {kind:?} window {window} fault {fault}/{total}"
                                );
                                for line in &reference {
                                    if !live.contains(line) {
                                        std::println!("   LIVE MISSING/CHANGED: {line}");
                                    }
                                }
                                for line in &live {
                                    if !reference.contains(line) {
                                        std::println!("   LIVE NEW: {line}");
                                    }
                                }
                            }
                        }
                        std::mem::forget(fs);
                    }
                }
            }
        }
        std::println!("live sweep: {cases} faulted creates, {mismatches} live-mount mismatches");
        assert_eq!(mismatches, 0);
    }

    /// After a faulted create without a remount, further writes on the same
    /// mount must not damage the volume: a clean remount shows the reference
    /// tree plus exactly the new entries. Fails without the operation-level
    /// cache drop, where the stale read cache corrupts unrelated entries.
    #[test]
    fn write_after_faulted_create_does_not_damage_the_volume() {
        const SEEDS: [u64; 3] = [1, 2, 3];
        let mut cases = 0usize;
        let mut damaged = 0usize;
        for seed in SEEDS {
            let cache = [128usize, 256, 512][(seed % 3) as usize];
            let (_d, fs0, dirs, files) = build(seed, cache, true);
            std::mem::forget(fs0);
            for (target, kind) in pick_targets(seed, &dirs, &files) {
                let create = create_over(&target, kind);
                let (probe_device, probe_fs, _, _) = build(seed, cache, true);
                probe_device.reset_reads();
                let _ = create(&probe_fs);
                let total = probe_device.reads();
                std::mem::forget(probe_fs);
                let (reference_device, reference_fs, _, _) = build(seed, cache, true);
                std::mem::forget(reference_fs);
                let reference = snapshot(reference_device, cache);

                for window in [1usize, 2] {
                    for fault in 1..=total {
                        let (device, fs, _, _) = build(seed, cache, true);
                        device.reset_reads();
                        device.fail_read_window(fault, window);
                        let _ = create(&fs);
                        device.allow_all();
                        // Write more on the same mount, without a remount.
                        let mut follow_up_errors = 0usize;
                        for index in 0..6 {
                            let name = alloc::format!("/zz{index}");
                            if FileSystemOperations::create_file(
                                &fs,
                                Path::new(&name),
                                &sweep_attributes(5),
                            )
                            .is_err()
                            {
                                follow_up_errors += 1;
                            }
                        }
                        cases += 1;
                        std::mem::forget(fs);
                        device.allow_all();
                        let after = snapshot(device, cache);
                        let bad = !reference.iter().all(|line| after.contains(line))
                            || after.len() != reference.len() + 6 - follow_up_errors;
                        if bad {
                            damaged += 1;
                            if damaged <= 3 {
                                std::println!(
                                    "DAMAGED seed {seed} cache {cache} target {target} {kind:?} window {window} fault {fault}/{total} follow-up-errors {follow_up_errors}"
                                );
                                for line in &reference {
                                    if !after.contains(line) {
                                        std::println!("   MISSING/CHANGED: {line}");
                                    }
                                }
                                for line in &after {
                                    if !reference.contains(line) {
                                        std::println!("   NEW: {line}");
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        std::println!("write-after sweep: {cases} faulted creates, {damaged} damaged volumes");
        assert_eq!(damaged, 0);
    }

    /// A failed device read leaves the key of the failed block in a read cache
    /// with stale bytes. These tests check, for every kind of operation that
    /// can fail, that nothing later reads from those bytes. The oracle is a
    /// fresh mount of the same device: the live mount must show the same tree,
    /// and a write on the live mount must give the same results and the same
    /// tree as on a fresh mount.
    mod stale_read_cache {
        use super::*;
        use file_system::{BaseOperations, Context, DirectBaseOperations, DirectoryOperations};

        fn dump(device: &'static FaultDevice) -> Vec<u8> {
            let mut image = alloc::vec![0u8; DEVICE_SIZE];
            DirectBaseOperations::read_sync(&device.inner, &mut image, 0).unwrap();
            image
        }

        fn restore(image: &[u8]) -> &'static FaultDevice {
            let device = FaultDevice::new(DEVICE_SIZE);
            DirectBaseOperations::write_sync(&device.inner, image, 0).unwrap();
            device
        }

        fn rename_fixture() -> (&'static FaultDevice, FileSystem) {
            let device = FaultDevice::new(DEVICE_SIZE);
            let fs = format_and_mount(device);
            mkdir(&fs, "/directory");
            put(&fs, "/source", &[0x42; 700]);
            put(&fs, "/untouched", b"keep");
            drop(fs);
            (device, FileSystem::new(device, options()).unwrap())
        }

        fn rename_source(fs: &FileSystem) -> Result<()> {
            fs.rename(Path::new("/source"), Path::new("/directory/destination"))
        }

        #[test]
        fn create_after_failed_rename_preserves_the_file() {
            let (device, fs) = rename_fixture();
            let image = dump(device);
            device.reset_reads();
            device.reset_progs();
            rename_source(&fs).unwrap();
            let programs = device.progs();
            let reads = device.reads();
            assert!(reads > 0 && programs >= 2);

            let fault_types = [
                (
                    "read",
                    reads,
                    FaultDevice::fail_read_at as fn(&FaultDevice, usize),
                ),
                ("program", programs, FaultDevice::fail_at),
            ];
            for (operation, count, inject) in fault_types {
                let mut failures = 0;
                for fault in 1..=count {
                    let device = restore(&image);
                    let fs = FileSystem::new(device, options()).unwrap();
                    device.reset_reads();
                    device.reset_progs();
                    inject(device, fault);
                    let result = rename_source(&fs);
                    device.allow_all();
                    if result.is_ok() {
                        continue;
                    }
                    assert_eq!(result, Err(IoErrorKind::InputOutput));
                    failures += 1;

                    let reference_device = restore(&dump(device));
                    let reference = FileSystem::new(reference_device, options()).unwrap();
                    let follow_up = |fs: &FileSystem| {
                        fs.create_file(Path::new("/created"), &sweep_attributes(7))
                    };
                    follow_up(&fs).unwrap();
                    follow_up(&reference).unwrap();
                    drop(reference);

                    let mut live = Vec::new();
                    walk(&fs, "/", &mut live);
                    drop(fs);
                    let fresh = snapshot(device, CACHE_SIZE);
                    let expected = snapshot(reference_device, CACHE_SIZE);
                    assert_eq!(live, fresh, "{operation} fault {fault}/{count}");
                    assert_eq!(fresh, expected, "{operation} fault {fault}/{count}");

                    let mut moved = live.iter().filter(|entry| {
                        entry.starts_with("/source ")
                            || entry.starts_with("/directory/destination ")
                    });
                    assert!(
                        moved
                            .next()
                            .is_some_and(|entry| entry.ends_with("content=700:46200"))
                            && moved.next().is_none(),
                        "{operation} fault {fault}/{count}: {live:?}"
                    );
                }
                assert!(failures > 0, "{operation}");
            }
        }

        #[test]
        fn failed_rename_recovery_waits_for_open_handles_and_retries_mount() {
            let (device, fs) = rename_fixture();
            let mut directory = Context::new_empty();
            let mut file = Context::new_empty();
            fs.lookup_directory(&mut directory, Path::new("/")).unwrap();
            fs.lookup_file(
                &mut file,
                Path::new("/untouched"),
                AccessFlags::READ_WRITE.into(),
            )
            .unwrap();
            BaseOperations::write_sync(&fs, &mut file, b"uncommitted", 0).unwrap();

            device.reset_progs();
            device.fail_at(1);
            assert_eq!(rename_source(&fs), Err(IoErrorKind::InputOutput));
            device.allow_all();
            device.reset_progs();

            let create = || fs.create_file(Path::new("/created"), &sweep_attributes(7));
            assert_eq!(create(), Err(IoErrorKind::InputOutput));
            assert_eq!(
                DirectoryOperations::read(&fs, &mut directory).map(|_| ()),
                Err(IoErrorKind::InputOutput)
            );
            assert_eq!(
                BaseOperations::close(&fs, &mut file),
                Err(IoErrorKind::InputOutput)
            );
            assert_eq!(create(), Err(IoErrorKind::InputOutput));
            DirectoryOperations::close(&fs, &mut directory).unwrap();

            device.reset_reads();
            device.fail_read_at(1);
            assert_eq!(create(), Err(IoErrorKind::InputOutput));
            assert_eq!(device.progs(), 0);
            device.allow_all();
            create().unwrap();

            let mut tree = Vec::new();
            walk(&fs, "/", &mut tree);
            assert!(
                tree.iter().any(|entry| {
                    entry.starts_with("/untouched ") && entry.ends_with("content=4:421")
                }),
                "{tree:?}"
            );
            assert_eq!(tree, snapshot(device, CACHE_SIZE));
        }

        /// Writes that read the metadata of the volume: a directory creation, a
        /// write to an existing file and the removal of that file.
        fn follow_up(fs: &FileSystem, existing: &str) -> Vec<String> {
            let mut results = Vec::new();
            results.push(alloc::format!(
                "{:?}",
                FileSystemOperations::create_directory(
                    fs,
                    Path::new("/zzd"),
                    &sweep_dir_attributes(5)
                )
            ));
            results.push(alloc::format!(
                "{:?}",
                fs.operation(|native| {
                    let mut file =
                        File::lookup(native, Path::new(existing), AccessFlags::READ_WRITE.into())?;
                    file.write(native, &[0x5a; 40], 0)?;
                    file.close(native)
                })
            ));
            results.push(alloc::format!(
                "{:?}",
                FileSystemOperations::remove(fs, Path::new(existing))
            ));
            results
        }

        /// Run `operation` with a read error window at every read it performs.
        fn sweep(name: &str, operation: impl Fn(&FileSystem, &[String], &[String])) {
            let mut cases = 0usize;
            let mut mismatches = 0usize;
            // Seeds 1 and 2 already fail without the operation-level cache
            // drop; seed 3 is trimmed to keep the debug run fast.
            for seed in [1u64, 2] {
                let cache = [128usize, 256, 512][(seed % 3) as usize];
                let (device, fs, dirs, files) = build(seed, cache, true);
                device.reset_reads();
                operation(&fs, &dirs, &files);
                let total = device.reads();
                std::mem::forget(fs);
                for window in [1usize, 2] {
                    for fault in 1..=total {
                        // The live mount shows the tree of the device.
                        let (device, fs, dirs, files) = build(seed, cache, true);
                        device.reset_reads();
                        device.fail_read_window(fault, window);
                        operation(&fs, &dirs, &files);
                        device.allow_all();
                        let mut live = Vec::new();
                        walk(&fs, "/", &mut live);
                        let fresh = snapshot(device, cache);
                        std::mem::forget(fs);
                        cases += 1;
                        if live != fresh {
                            mismatches += 1;
                            std::println!(
                                "{name}: LIVE MISMATCH seed {seed} window {window} fault {fault}/{total}"
                            );
                        }

                        // A write on the live mount behaves as on a fresh mount.
                        let (device, fs, dirs, files) = build(seed, cache, true);
                        device.reset_reads();
                        device.fail_read_window(fault, window);
                        operation(&fs, &dirs, &files);
                        device.allow_all();
                        let image = dump(device);
                        let live_results = follow_up(&fs, &files[0]);
                        std::mem::forget(fs);
                        let live_after = snapshot(device, cache);
                        let reference_device = restore(&image);
                        let reference_fs =
                            FileSystem::new(reference_device, Options::new(cache)).unwrap();
                        let reference_results = follow_up(&reference_fs, &files[0]);
                        std::mem::forget(reference_fs);
                        let reference_after = snapshot(reference_device, cache);
                        cases += 1;
                        if live_results != reference_results || live_after != reference_after {
                            mismatches += 1;
                            std::println!(
                                "{name}: WRITE MISMATCH seed {seed} window {window} fault {fault}/{total}"
                            );
                        }
                    }
                }
            }
            std::println!("{name}: {cases} checks, {mismatches} mismatches");
            assert_eq!(mismatches, 0, "{name}");
        }

        /// Operations that fail with an I/O error through the two entry points
        /// of the file system: `operation` (path operations) and
        /// `operation_with_context` (operations on an open handle).
        #[test]
        fn failed_operations_leave_no_stale_read_cache() {
            sweep("stat", |fs, _, files| {
                let mut attributes = Attributes::new().set_mask(owner_mask());
                let _ =
                    FileSystemOperations::get_attributes(fs, Path::new(&files[0]), &mut attributes);
            });
            sweep("remove", |fs, _, files| {
                let _ = FileSystemOperations::remove(fs, Path::new(&files[0]));
            });
            sweep("directory read through a handle", |fs, _, _| {
                let mut context = Context::new_empty();
                if FileSystemOperations::lookup_directory(fs, &mut context, Path::new("/")).is_ok()
                {
                    while let Ok(Some(_)) = DirectoryOperations::read(fs, &mut context) {}
                    let _ = DirectoryOperations::close(fs, &mut context);
                }
            });
            sweep("write through a handle", |fs, _, files| {
                let mut context = Context::new_empty();
                if FileSystemOperations::lookup_file(
                    fs,
                    &mut context,
                    Path::new(&files[0]),
                    AccessFlags::READ_WRITE.into(),
                )
                .is_ok()
                {
                    let _ = BaseOperations::write_sync(fs, &mut context, &[0x33; 40], 0);
                    let _ = BaseOperations::close(fs, &mut context);
                }
            });
        }

        /// A create fails with NotFound (the parent does not exist), so the
        /// error is not an I/O error. The cleanup probe of the create can
        /// still hit a read error and ignore it.
        #[test]
        fn failed_create_with_another_error_leaves_no_stale_read_cache() {
            sweep("create in a missing directory", |fs, _, _| {
                let _ = FileSystemOperations::create_file(
                    fs,
                    Path::new("/missing_parent/x"),
                    &sweep_attributes(99),
                );
            });
        }

        fn read_at(
            fs: &FileSystem,
            context: &mut Context,
            offset: u64,
            length: usize,
        ) -> Result<Vec<u8>> {
            let mut buffer = alloc::vec![0u8; length];
            let count = BaseOperations::read_sync(fs, context, &mut buffer, offset)?;
            buffer.truncate(count);
            Ok(buffer)
        }

        /// LittleFS reads the data of a file through the cache of the file. A
        /// failed read must not make the retried read on the same handle
        /// return stale bytes.
        #[test]
        fn failed_read_can_be_retried_on_the_same_handle() {
            let mut checks = 0usize;
            let mut wrong = 0usize;
            for seed in [1u64, 2, 3, 4, 5, 6] {
                let cache = [128usize, 256, 512][(seed % 3) as usize];
                let (device, fs, _, files) = build(seed, cache, true);
                let image = dump(device);
                std::mem::forget(fs);
                for path in files.iter().take(4) {
                    for (offset, length) in [(0u64, 4096usize), (0, 16), (100, 16)] {
                        let device = restore(&image);
                        let fs = FileSystem::new(device, Options::new(cache)).unwrap();
                        let mut context = Context::new_empty();
                        device.reset_reads();
                        FileSystemOperations::lookup_file(
                            &fs,
                            &mut context,
                            Path::new(path),
                            AccessFlags::Read.into(),
                        )
                        .unwrap();
                        let expected = read_at(&fs, &mut context, offset, length).unwrap();
                        let total = device.reads();
                        BaseOperations::close(&fs, &mut context).unwrap();
                        std::mem::forget(fs);
                        if expected.is_empty() {
                            continue;
                        }
                        for window in [1usize, 2] {
                            for fault in 1..=total {
                                let device = restore(&image);
                                let fs = FileSystem::new(device, Options::new(cache)).unwrap();
                                let mut context = Context::new_empty();
                                device.reset_reads();
                                device.fail_read_window(fault, window);
                                let opened = FileSystemOperations::lookup_file(
                                    &fs,
                                    &mut context,
                                    Path::new(path),
                                    AccessFlags::Read.into(),
                                );
                                if opened.is_ok() {
                                    let _ = read_at(&fs, &mut context, offset, length);
                                    device.allow_all();
                                    if let Ok(retried) = read_at(&fs, &mut context, offset, length)
                                    {
                                        checks += 1;
                                        if retried != expected {
                                            wrong += 1;
                                            std::println!(
                                                "retry: WRONG BYTES seed {seed} {path} offset {offset} length {length} window {window} fault {fault}/{total}"
                                            );
                                        }
                                    }
                                    let _ = BaseOperations::close(&fs, &mut context);
                                }
                                std::mem::forget(fs);
                            }
                        }
                    }
                }
            }
            std::println!("retry: {checks} retried reads, {wrong} with wrong bytes");
            assert_eq!(wrong, 0);
        }
    }
}
