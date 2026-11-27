#![cfg(all(feature = "persistence", target_os = "linux"))]

testing_support::instantiate_global_allocator!();

use crash::{
    Architecture, CRASH_LOOP_LIMIT, Header, Limits, Record, RecordKind, ReportIdentifier, Writer,
    clear, import, list, read,
};
use file_system::{IoErrorKind, MemoryDevice, Path};
use task::test;
use users::{ADMINISTRATOR_GROUP_NAME, GroupIdentifier, UserIdentifier};
use virtual_file_system::VirtualFileSystem;

#[test]
async fn imports_rotates_and_protects_reports() {
    let task_manager = task::test_manager();
    let root = task_manager.get_current_task_identifier().await;
    let users = Box::leak(Box::new(users::Manager::new()));
    let administrators = GroupIdentifier::new(1);
    let user = UserIdentifier::new(1);
    users
        .add_group(administrators, ADMINISTRATOR_GROUP_NAME, &[])
        .await
        .unwrap();
    users
        .add_user(user, "test", GroupIdentifier::ROOT, "/users/test")
        .await
        .unwrap();
    let time = Box::leak(Box::new(
        time::Manager::new(&testing_support::TimeDevice).unwrap(),
    ));
    let disk = MemoryDevice::<512>::new_static(256 * 512);
    let filesystem =
        little_fs::FileSystem::get_or_format(disk, little_fs::Options::new(256)).unwrap();
    let vfs: &'static VirtualFileSystem = Box::leak(Box::new(
        virtual_file_system::initialize(task_manager, users, time, filesystem).unwrap(),
    ));
    virtual_file_system::create_default_hierarchy(vfs, root)
        .await
        .unwrap();
    assert!(list(vfs, root).await.unwrap().is_empty());
    assert!(matches!(
        vfs.get_statistics(&Path::SYSTEM_CRASHES).await,
        Err(IoErrorKind::NotFound)
    ));

    let mut slot = [0; 512];
    let mut header = Header::new(RecordKind::KernelPanic, Architecture::XtensaEsp32S3);
    for sequence in 1..=6 {
        let record = Writer::new(&mut slot, &header, Limits::ESPRESSIF)
            .unwrap()
            .finish(format_args!("Panic {sequence}"), &[], |_| Some(0));
        assert_eq!(
            import(vfs, record, 128 * 1024).await.unwrap(),
            Some(ReportIdentifier(sequence))
        );
    }
    header.consecutive_crashes = CRASH_LOOP_LIMIT;
    let record = Writer::new(&mut slot, &header, Limits::ESPRESSIF)
        .unwrap()
        .finish(format_args!("Crash loop"), &[], |_| Some(0));
    assert_eq!(import(vfs, record, 128 * 1024).await.unwrap(), None);
    assert!(
        list(vfs, root)
            .await
            .unwrap()
            .into_iter()
            .eq((3..=6).map(ReportIdentifier))
    );

    let newest = ReportIdentifier(6);
    let bytes = read(vfs, root, newest).await.unwrap();
    assert_eq!(Record::read(&bytes).unwrap().message, "Panic 6");
    let statistics = vfs
        .get_statistics(&"/system/crashes/6-kernel.xcr")
        .await
        .unwrap();
    assert_eq!(statistics.permissions.as_u16(), 0o640);
    assert_eq!(statistics.user, UserIdentifier::ROOT);
    assert_eq!(statistics.group, administrators);

    let (reader, _) = task_manager
        .spawn(root, "Reader", None, move |reader| async move {
            task_manager.set_user(reader, user).await.unwrap();
            assert_eq!(list(vfs, reader).await, Err(IoErrorKind::PermissionDenied));
            assert_eq!(
                read(vfs, reader, newest).await,
                Err(IoErrorKind::PermissionDenied)
            );
            assert_eq!(clear(vfs, reader).await, Err(IoErrorKind::PermissionDenied));
        })
        .await
        .unwrap();
    reader.join().await;
    clear(vfs, root).await.unwrap();
    assert!(list(vfs, root).await.unwrap().is_empty());
    header.consecutive_crashes = 1;
    let record = Writer::new(&mut slot, &header, Limits::ESPRESSIF)
        .unwrap()
        .finish(format_args!("After clear"), &[], |_| Some(0));
    assert_eq!(
        import(vfs, record, 128 * 1024).await.unwrap(),
        Some(ReportIdentifier(7))
    );
}
