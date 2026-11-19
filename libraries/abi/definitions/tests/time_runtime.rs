#![cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]

extern crate alloc;

use abi_definitions::{
    RawMutex, RuntimeContext, XilaTimeClockIdentifier, initialize,
    xila_condition_variable_attribute_initialize, xila_condition_variable_attribute_set_clock,
    xila_condition_variable_initialize, xila_condition_variable_remove,
    xila_condition_variable_try_wait, xila_time_get_resolution, xila_time_get_time,
};
use alloc::boxed::Box;
use core::time::Duration;
use virtual_file_system::Error;

testing_support::instantiate_global_allocator!();

#[task::test]
async fn clock_values_and_expired_condition_deadlines_use_milliseconds() {
    struct FixedClock;
    impl time::MonotonicClock for FixedClock {
        fn now(&self) -> Duration {
            Duration::from_micros(12_345)
        }

        fn resolution(&self) -> Duration {
            Duration::from_micros(250)
        }
    }

    let manager = Box::leak(Box::new(time::Manager::with_clock(&FixedClock)));
    let tasks = Box::leak(Box::new(task::Manager::new()));
    let users = Box::leak(Box::new(users::Manager::new()));
    let device = file_system::MemoryDevice::<512>::new_static(1024 * 512);
    let file_system =
        little_fs::FileSystem::new_format(device, little_fs::Options::new(256)).unwrap();
    let virtual_file_system = Box::leak(Box::new(
        virtual_file_system::initialize(tasks, users, manager, file_system).unwrap(),
    ));
    initialize(RuntimeContext {
        virtual_file_system,
        time_manager: manager,
    });

    let mut value = 999;
    assert_eq!(
        unsafe { xila_time_get_time(XilaTimeClockIdentifier::Realtime, 0, &mut value) },
        u32::from(Error::Time(time::Error::Unsynchronized))
    );
    assert_eq!(value, 999);
    assert_eq!(
        unsafe { xila_time_get_time(XilaTimeClockIdentifier::Monotonic, 100, &mut value) },
        0
    );
    assert_eq!(value, 12);
    assert_eq!(
        unsafe { xila_time_get_resolution(XilaTimeClockIdentifier::Monotonic, &mut value) },
        0
    );
    assert_eq!(value, 1);

    manager
        .set_realtime(time::RealtimeSample {
            timestamp: time::Timestamp::from_nanos(1_234_567_890),
            source: time::ClockSource::Manual,
            resolution: Duration::from_micros(1_500),
            accuracy: None,
        })
        .unwrap();
    assert_eq!(
        unsafe { xila_time_get_time(XilaTimeClockIdentifier::Realtime, 0, &mut value) },
        0
    );
    assert_eq!(value, 1_234);
    assert_eq!(
        unsafe { xila_time_get_resolution(XilaTimeClockIdentifier::Realtime, &mut value) },
        0
    );
    assert_eq!(value, 2);

    let mut attribute = 0;
    let mut condition_variable = 0;
    let mut mutex = RawMutex::new(false);
    assert!(mutex.lock(1usize.into()));
    assert_eq!(
        unsafe { xila_condition_variable_attribute_initialize(&mut attribute) },
        0
    );
    assert_eq!(
        unsafe {
            xila_condition_variable_attribute_set_clock(
                &mut attribute,
                XilaTimeClockIdentifier::Monotonic,
            )
        },
        0
    );
    assert_eq!(
        unsafe { xila_condition_variable_initialize(&mut condition_variable, &attribute) },
        0
    );
    assert_eq!(
        unsafe { xila_condition_variable_try_wait(&mut condition_variable, &mut mutex, 12) },
        u32::from(Error::RessourceBusy)
    );
    assert!(mutex.unlock(1usize.into()));
    assert_eq!(
        unsafe { xila_condition_variable_remove(&mut condition_variable) },
        0
    );
}
