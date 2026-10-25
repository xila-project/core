#![no_std]

extern crate alloc;
use core::time::Duration;
use drivers_std::devices::TimeDevice;
use log::information;

#[test]
fn test_get_current_time() {
    let time_manager = time::Manager::new(&TimeDevice).unwrap();

    let current_time = time_manager.get_current_time().unwrap();

    information!("Current time : {current_time:?}");

    assert_ne!(current_time, Duration::default());
}

#[test]
fn test_get_current_time_since_startup() {
    let time_manager = time::Manager::new(&TimeDevice).unwrap();

    let current_time = time_manager.get_current_time_since_startup().unwrap();

    information!("Time since startup : {current_time:?}");

    assert_ne!(current_time, Duration::default());
}
