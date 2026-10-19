use crate::{Error, Result};
use core::time::Duration;
use file_system::DirectCharacterDevice;
pub struct Manager<'a> {
    device: &'a (dyn DirectCharacterDevice + Send + Sync),
    start_time: Duration,
}

impl<'a> Manager<'a> {
    pub fn new(device: &'a (dyn DirectCharacterDevice + Send + Sync)) -> Result<Self> {
        let start_time = Self::get_current_time_from_device(device)?;

        Ok(Self { device, start_time })
    }

    pub fn get_current_time_since_startup(&self) -> Result<Duration> {
        let current_time = self.get_current_time()?;

        Ok(current_time.abs_diff(self.start_time))
    }

    pub fn get_current_time(&self) -> Result<Duration> {
        Self::get_current_time_from_device(self.device)
    }

    fn get_current_time_from_device(device: &dyn DirectCharacterDevice) -> Result<Duration> {
        let mut current_time = Duration::default();

        let current_time_raw = unsafe {
            core::slice::from_raw_parts_mut(
                &mut current_time as *mut Duration as *mut u8,
                core::mem::size_of::<Duration>(),
            )
        };

        device
            .read_sync(current_time_raw, 0)
            .map_err(Error::DeviceError)?;

        Ok(current_time)
    }
}

pub fn initialize(
    driver: &'static (dyn DirectCharacterDevice + Send + Sync),
) -> Result<Manager<'static>> {
    Manager::new(driver)
}
