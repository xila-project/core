use file_system::{AccessFlags, MemoryDevice, Permissions};
use firmware_update::device::*;
use firmware_update::{
    AUTHENTICATED_UPDATE, Capabilities, Error as FirmwareError, FUNCTIONAL_REVERT, FirmwareBackend,
};
use users::{GroupIdentifier, UserIdentifier};
use virtual_file_system::{Error, File, ItemStatic, VirtualFileSystem};
extern crate abi_definitions;
testing::instantiate_global_allocator!();

#[derive(Default)]
struct FakeBackend {
    generation: u64,
    staging: Option<u64>,
    received: u64,
    activated: bool,
    confirmed: bool,
}

impl FirmwareBackend for FakeBackend {
    type Verified = u64;
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            max_image_size: 3840,
            max_chunk_size: 64,
            guarantees: AUTHENTICATED_UPDATE | FUNCTIONAL_REVERT,
        }
    }
    fn begin(&mut self, size: u64, metadata: &[u8]) -> Result<(), FirmwareError> {
        if self.staging.is_some() {
            return Err(FirmwareError::InvalidState);
        }
        if metadata.is_empty() {
            return Err(FirmwareError::Encoding);
        }
        self.generation += 1;
        self.staging = Some(size);
        self.received = 0;
        Ok(())
    }
    fn write(&mut self, bytes: &[u8]) -> Result<usize, FirmwareError> {
        let expected = self.staging.ok_or(FirmwareError::InvalidState)?;
        if bytes.len() as u64 > expected - self.received {
            return Err(FirmwareError::Length);
        }
        let n = bytes.len().min(64);
        self.received += n as u64;
        Ok(n)
    }
    fn finish(&mut self) -> Result<u64, FirmwareError> {
        let expected = self.staging.ok_or(FirmwareError::InvalidState)?;
        if self.received != expected {
            return Err(FirmwareError::Length);
        }
        Ok(self.generation)
    }
    fn activate(&mut self, verified: u64) -> Result<(), FirmwareError> {
        self.staging.ok_or(FirmwareError::InvalidState)?;
        if verified != self.generation {
            return Err(FirmwareError::InvalidState);
        }
        self.staging = None;
        self.activated = true;
        Ok(())
    }
    fn abort(&mut self) -> Result<(), FirmwareError> {
        self.staging = None;
        self.received = 0;
        Ok(())
    }
    fn confirm_running(&mut self) -> Result<(), FirmwareError> {
        if self.staging.is_some() {
            return Err(FirmwareError::InvalidState);
        }
        self.confirmed = true;
        Ok(())
    }
}

async fn fixture() -> (task::TaskIdentifier, &'static VirtualFileSystem) {
    let manager = task::test_manager();
    let users = Box::leak(Box::new(users::Manager::new()));
    let time = Box::leak(Box::new(time::Manager::new(&testing::TimeDevice).unwrap()));
    let disk = MemoryDevice::<512>::new_static(512 * 128);
    little_fs::FileSystem::format(disk, 256).unwrap();
    let fs = little_fs::FileSystem::new(disk, 256).unwrap();
    let vfs = Box::leak(Box::new(VirtualFileSystem::new(manager, users, time, fs)));
    let task = manager.get_current_task_identifier().await;
    vfs.create_directory(task, &"/devices").await.unwrap();
    vfs.create_directory(task, &"/devices/system")
        .await
        .unwrap();
    vfs.mount_static_with_permissions(
        task,
        DEVICE_PATH,
        ItemStatic::CharacterDevice(Box::leak(Box::new(FirmwareDevice::new(
            FakeBackend::default(),
        )))),
        Permissions::USER_READ_WRITE,
    )
    .await
    .unwrap();
    (task, vfs)
}
#[task::test]
async fn vfs_stream_is_exclusive_explicit_and_privileged() {
    let (task, vfs) = fixture().await;
    let mut file = File::open(vfs, task, DEVICE_PATH, AccessFlags::READ_WRITE.into())
        .await
        .unwrap();
    assert!(matches!(
        File::open(vfs, task, DEVICE_PATH, AccessFlags::Write.into()).await,
        Err(Error::FileSystem(file_system::Error::RessourceBusy))
    ));
    assert!(matches!(
        file.duplicate().await,
        Err(Error::FileSystem(file_system::Error::UnsupportedOperation))
    ));
    assert_eq!(
        file.control(ACTIVATE, &header()).await,
        Err(Error::FileSystem(file_system::Error::InvalidParameter))
    );
    let request = begin_request(137, b"metadata").unwrap();
    file.control(BEGIN, &request).await.unwrap();
    assert_eq!(file.write(&[0x5a; 137]).await.unwrap(), 64);
    assert_eq!(file.write(&[0x5a; 73]).await.unwrap(), 64);
    assert_eq!(file.write(&[0x5a; 9]).await.unwrap(), 9);
    file.control(FINISH, &header()).await.unwrap();
    let status = file.control(GET_STATUS, &header()).await.unwrap();
    assert_eq!(status[8], 2);
    file.control(ACTIVATE, &header()).await.unwrap();
    file.close(vfs).await.unwrap();
    let user = UserIdentifier::new(1001);
    vfs.users_manager()
        .add_user(user, "unprivileged", GroupIdentifier::ROOT)
        .await
        .unwrap();
    vfs.task_manager().set_user(task, user).await.unwrap();
    assert!(matches!(
        File::open(vfs, task, DEVICE_PATH, AccessFlags::Write.into()).await,
        Err(Error::PermissionDenied)
    ));
}
#[task::test]
async fn readonly_controls_and_malformed_abi_do_not_mutate() {
    let (task, vfs) = fixture().await;
    let mut f = File::open(vfs, task, DEVICE_PATH, AccessFlags::Read.into())
        .await
        .unwrap();
    assert_eq!(f.control(ABORT, &header()).await, Err(Error::InvalidMode));
    assert_eq!(
        f.as_synchronous_file_mut().control(ABORT, &header()),
        Err(Error::InvalidMode)
    );
    assert_eq!(
        f.as_synchronous_file_mut().control_raw(
            <ABORT as file_system::ControlCommand>::IDENTIFIER,
            shared::AnyByLayout::from(&header()),
            shared::AnyByLayout::from_mutable(&mut ())
        ),
        Err(Error::InvalidMode)
    );
    f.close(vfs).await.unwrap();
    let mut f = File::open(vfs, task, DEVICE_PATH, AccessFlags::Write.into())
        .await
        .unwrap();
    let mut bad = header();
    bad[7] = 1;
    assert_eq!(
        f.control(ABORT, &bad).await,
        Err(Error::FileSystem(file_system::Error::InvalidParameter))
    );
    let mut bad = begin_request(3, b"metadata").unwrap();
    bad[279] = 1;
    assert_eq!(
        f.control(BEGIN, &bad).await,
        Err(Error::FileSystem(file_system::Error::InvalidParameter))
    );
    f.control(BEGIN, &begin_request(3, b"metadata").unwrap())
        .await
        .unwrap();
    f.write(b"new").await.unwrap();
    f.control(FINISH, &header()).await.unwrap();
    drop(f);
    let mut f = File::open(vfs, task, DEVICE_PATH, AccessFlags::Write.into())
        .await
        .unwrap();
    assert_eq!(f.control(GET_STATUS, &header()).await.unwrap()[8], 0);
    f.control(BEGIN, &begin_request(3, b"metadata").unwrap())
        .await
        .unwrap();
}
