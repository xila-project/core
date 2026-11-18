extern crate std;

use alloc::boxed::Box;
use core::{ptr, time::Duration};
use file_system::{MemoryDevice, Position};

use super::*;

#[task::test]
async fn file_operations_preserve_the_cursor_and_apply_requested_metadata() {
    let time_manager = Box::leak(Box::new(time::Manager::new(&testing::TimeDevice).unwrap()));
    let task_manager = Box::leak(Box::new(task::Manager::new()));
    let users_manager = Box::leak(Box::new(users::Manager::new()));
    let task = task_manager.get_current_task_identifier().await;
    let device = MemoryDevice::<512>::new_static(1024 * 512);
    let file_system =
        little_fs::FileSystem::new_format(device, little_fs::Options::new(256)).unwrap();
    let virtual_file_system = Box::leak(Box::new(
        virtual_file_system::initialize(task_manager, users_manager, time_manager, file_system)
            .unwrap(),
    ));
    let mut file = SynchronousFile::open(
        virtual_file_system,
        task,
        "/file",
        Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None),
    )
    .unwrap();
    file.write(b"sample").unwrap();
    let file_pointer = XilaFileSystemFile::from_real(&mut file);

    unsafe {
        assert_eq!(
            xila_file_system_set_times(file_pointer, 0, 0, 0b0010),
            u32::from(Error::Time(time::Error::NotInitialized))
        );
        let requested_state = StateFlags::Append | StateFlags::NonBlocking;
        assert_eq!(
            xila_file_system_file_set_flags(file_pointer, requested_state.bits()),
            0
        );
        let mut state = 0;
        assert_eq!(xila_file_system_file_get_state(file_pointer, &mut state), 0);
        assert_eq!(state, requested_state.bits());
        assert_eq!(
            xila_file_system_file_set_flags(file_pointer, StateFlags::Synchronous.bits()),
            u32::from(Error::UnsupportedOperation)
        );
        assert_eq!(
            xila_file_system_file_set_flags(file_pointer, 1 << 7),
            u32::from(Error::InvalidParameter)
        );
        assert_eq!(xila_file_system_file_get_state(file_pointer, &mut state), 0);
        assert_eq!(state, requested_state.bits());

        assert_eq!(xila_file_system_truncate(file_pointer, 2), 0);
        assert_eq!((*file_pointer).get_statistics().unwrap().size, 2);
        assert_eq!(xila_file_system_file_truncate(file_pointer, 4), 0);
        assert_eq!((*file_pointer).get_statistics().unwrap().size, 4);
        assert_eq!(
            (*file_pointer).set_position(&Position::Current(0)).unwrap(),
            6
        );
        assert_eq!(
            xila_file_system_file_allocate(file_pointer, 6, 2),
            u32::from(Error::UnsupportedOperation)
        );
        assert_eq!((*file_pointer).get_statistics().unwrap().size, 4);

        assert_eq!(
            xila_file_system_set_times(file_pointer, 12_500_000_000, 34_000_000_000, 0b0101),
            0
        );
        let statistics = (*file_pointer).get_statistics().unwrap();
        assert_eq!(statistics.access, Time::new(12));
        assert_eq!(statistics.modification, Time::new(34));

        crate::initialize(crate::RuntimeContext {
            virtual_file_system,
            time_manager,
        });
        time_manager
            .set_realtime(time::RealtimeSample {
                timestamp: time::Timestamp::from_nanos(56_000_000_000),
                resolution: Duration::from_secs(1),
                source: time::ClockSource::Manual,
                accuracy: None,
            })
            .unwrap();
        assert_eq!(xila_file_system_set_times(file_pointer, 0, 0, 0b0010), 0);
        let statistics = (*file_pointer).get_statistics().unwrap();
        assert_eq!(statistics.access, Time::new(56));
        assert_eq!(statistics.modification, Time::new(34));
        for invalid_flags in [0b0011, 0b1100, 0b1_0000] {
            assert_eq!(
                xila_file_system_set_times(file_pointer, 0, 0, invalid_flags),
                u32::from(Error::InvalidParameter)
            );
        }
        assert_eq!(
            xila_file_system_file_advise(file_pointer, 0, 0, 0),
            u32::from(Error::UnsupportedOperation)
        );
    }
}

#[test]
fn path_mutations_validate_their_arguments() {
    unsafe {
        assert_eq!(
            xila_file_system_set_times_from_path(0, ptr::null(), 0, 0, 0b0100, false),
            u32::from(Error::InvalidParameter)
        );
        assert_eq!(
            xila_file_system_set_times_from_path(0, c"/file".as_ptr(), 0, 0, 0b0011, false),
            u32::from(Error::InvalidParameter)
        );
        assert_eq!(
            xila_file_system_set_times_from_path(0, c"/file".as_ptr(), 0, 0, 0, false),
            0
        );
        assert_eq!(
            xila_file_system_link(0, ptr::null(), c"/link".as_ptr()),
            u32::from(Error::InvalidParameter)
        );
        assert_eq!(
            xila_file_system_link(0, c"/file".as_ptr(), ptr::null()),
            u32::from(Error::InvalidParameter)
        );
    }
}
