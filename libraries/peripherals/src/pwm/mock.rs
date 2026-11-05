use synchronization::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    mutex::{Mutex, MutexGuard},
};

use embedded_hal::pwm::{ErrorType, SetDutyCycle};

use super::{Capabilities, ChannelId, Error, PwmController};

#[derive(Clone, Copy)]
struct ChannelState {
    reserved: bool,
    duty: u16,
    frequency_hz: u32,
}

/// Simulated PWM controller with `N` channels. It records the duty and the
/// period of every channel so tests can observe what a driver requested.
pub struct PwmMock<const N: usize> {
    capabilities: Capabilities,
    channels: Mutex<CriticalSectionRawMutex, [ChannelState; N]>,
}

impl<const N: usize> PwmMock<N> {
    /// Every channel starts at zero duty and `min_frequency_hz`.
    pub const fn new(min_frequency_hz: u32, max_frequency_hz: u32, max_duty_cycle: u16) -> Self {
        assert!(
            N <= u8::MAX as usize,
            "PWM mock supports at most 255 channels"
        );
        Self {
            capabilities: Capabilities {
                channel_count: N as u8,
                min_frequency_hz,
                max_frequency_hz,
                max_duty_cycle,
            },
            channels: Mutex::new(
                [ChannelState {
                    reserved: false,
                    duty: 0,
                    frequency_hz: min_frequency_hz,
                }; N],
            ),
        }
    }

    /// Returns the last duty recorded on `channel`.
    pub fn duty_cycle(&self, channel: ChannelId) -> Result<u16, Error> {
        let index = self.index(channel)?;
        Ok(self.lock()[index].duty)
    }

    fn lock(&self) -> MutexGuard<'_, CriticalSectionRawMutex, [ChannelState; N]> {
        self.channels
            .try_lock()
            .expect("PWM mock channels are already locked")
    }

    fn index(&self, channel: ChannelId) -> Result<usize, Error> {
        let index = usize::from(channel.index());
        if index < N {
            Ok(index)
        } else {
            Err(Error::InvalidChannel)
        }
    }

    fn release(&self, index: usize) {
        self.lock()[index].reserved = false;
    }
}

impl<const N: usize> PwmController for PwmMock<N> {
    type Channel<'a>
        = MockPwmChannel<'a, N>
    where
        Self: 'a;

    fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    fn open(&self, channel: ChannelId) -> Result<Self::Channel<'_>, Error> {
        let index = self.index(channel)?;
        let mut channels = self.lock();
        let state = &mut channels[index];
        if state.reserved {
            return Err(Error::Busy);
        }
        state.reserved = true;
        Ok(MockPwmChannel {
            controller: self,
            index,
        })
    }

    fn set_frequency(&self, channel: ChannelId, frequency_hz: u32) -> Result<(), Error> {
        let index = self.index(channel)?;
        let limits = self.capabilities;
        if limits.min_frequency_hz == limits.max_frequency_hz {
            return Err(Error::Unsupported);
        }
        if frequency_hz < limits.min_frequency_hz || frequency_hz > limits.max_frequency_hz {
            return Err(Error::InvalidFrequency);
        }
        self.lock()[index].frequency_hz = frequency_hz;
        Ok(())
    }

    fn frequency_hz(&self, channel: ChannelId) -> Result<u32, Error> {
        let index = self.index(channel)?;
        Ok(self.lock()[index].frequency_hz)
    }
}

/// Exclusive handle on one channel of a [`PwmMock`]. Dropping it releases the channel.
pub struct MockPwmChannel<'a, const N: usize> {
    controller: &'a PwmMock<N>,
    index: usize,
}

impl<const N: usize> ErrorType for MockPwmChannel<'_, N> {
    type Error = Error;
}

impl<const N: usize> SetDutyCycle for MockPwmChannel<'_, N> {
    fn max_duty_cycle(&self) -> u16 {
        self.controller.capabilities.max_duty_cycle
    }

    fn set_duty_cycle(&mut self, duty: u16) -> Result<(), Error> {
        if duty > self.max_duty_cycle() {
            return Err(Error::InvalidDuty);
        }
        self.controller.lock()[self.index].duty = duty;
        Ok(())
    }
}

impl<const N: usize> Drop for MockPwmChannel<'_, N> {
    fn drop(&mut self) {
        self.controller.release(self.index);
    }
}

#[cfg(all(test, feature = "mock"))]
mod tests {
    use embedded_hal::pwm::SetDutyCycle;

    use super::{ChannelId, Error, PwmController, PwmMock};

    fn mock() -> PwmMock<2> {
        PwmMock::new(1_000, 100_000, 1_200)
    }

    #[test]
    fn duty_and_period_are_recorded_per_channel() {
        let mock = mock();
        let mut channel = mock.open(ChannelId::new(1)).unwrap();
        channel.set_duty_cycle(600).unwrap();
        drop(channel);
        mock.set_frequency(ChannelId::new(1), 20_000).unwrap();

        assert_eq!(mock.duty_cycle(ChannelId::new(1)), Ok(600));
        assert_eq!(mock.duty_cycle(ChannelId::new(0)), Ok(0));
        assert_eq!(mock.frequency_hz(ChannelId::new(1)), Ok(20_000));
        assert_eq!(mock.frequency_hz(ChannelId::new(0)), Ok(1_000));
    }

    #[test]
    fn invalid_channel_is_rejected() {
        let mock = mock();
        assert!(matches!(
            mock.open(ChannelId::new(2)),
            Err(Error::InvalidChannel)
        ));
        assert_eq!(
            mock.set_frequency(ChannelId::new(2), 5_000),
            Err(Error::InvalidChannel)
        );
        assert_eq!(
            mock.frequency_hz(ChannelId::new(2)),
            Err(Error::InvalidChannel)
        );
        assert_eq!(
            mock.duty_cycle(ChannelId::new(2)),
            Err(Error::InvalidChannel)
        );
    }

    #[test]
    fn channel_is_exclusive_until_dropped() {
        let mock = mock();
        let id = ChannelId::new(0);
        let channel = mock.open(id).unwrap();
        assert!(matches!(mock.open(id), Err(Error::Busy)));
        // Another channel is independent.
        assert!(mock.open(ChannelId::new(1)).is_ok());
        drop(channel);
        assert!(mock.open(id).is_ok());
    }

    #[test]
    fn set_duty_cycle_trait_works_through_the_mock() {
        let mock = mock();
        let mut channel = mock.open(ChannelId::new(0)).unwrap();
        assert_eq!(channel.max_duty_cycle(), 1_200);
        channel.set_duty_cycle_percent(50).unwrap();
        assert_eq!(mock.duty_cycle(ChannelId::new(0)), Ok(600));
        channel.set_duty_cycle_fully_on().unwrap();
        assert_eq!(mock.duty_cycle(ChannelId::new(0)), Ok(1_200));
        channel.set_duty_cycle_fully_off().unwrap();
        assert_eq!(mock.duty_cycle(ChannelId::new(0)), Ok(0));
    }

    #[test]
    fn duty_above_maximum_is_rejected_and_keeps_previous_duty() {
        let mock = mock();
        let mut channel = mock.open(ChannelId::new(0)).unwrap();
        channel.set_duty_cycle(300).unwrap();
        assert_eq!(channel.set_duty_cycle(1_201), Err(Error::InvalidDuty));
        assert_eq!(mock.duty_cycle(ChannelId::new(0)), Ok(300));
    }

    #[test]
    fn frequency_outside_bounds_is_rejected() {
        let mock = mock();
        assert_eq!(
            mock.set_frequency(ChannelId::new(0), 999),
            Err(Error::InvalidFrequency)
        );
        assert_eq!(
            mock.set_frequency(ChannelId::new(0), 100_001),
            Err(Error::InvalidFrequency)
        );
        assert_eq!(mock.frequency_hz(ChannelId::new(0)), Ok(1_000));
    }

    #[test]
    fn fixed_period_controller_rejects_frequency_changes() {
        let mock = PwmMock::<1>::new(50, 50, 100);
        assert_eq!(
            mock.set_frequency(ChannelId::new(0), 50),
            Err(Error::Unsupported)
        );
    }

    #[test]
    fn channel_handle_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<super::MockPwmChannel<'static, 2>>();
    }
}
