use synchronization::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    mutex::{Mutex, MutexGuard},
};

use super::{AdcController, Error, full_scale};

/// Simulated ADC with `N` channels. Each channel holds a settable raw value;
/// values above full scale are clamped to the resolution on read.
pub struct AdcMock<const N: usize> {
    resolution_bits: u8,
    reference_millivolts: Option<u32>,
    samples: Mutex<CriticalSectionRawMutex, [u16; N]>,
}

impl<const N: usize> AdcMock<N> {
    pub const fn new(
        resolution_bits: u8,
        reference_millivolts: Option<u32>,
        samples: [u16; N],
    ) -> Self {
        const {
            assert!(
                N <= u8::MAX as usize,
                "ADC mock supports at most 255 channels"
            )
        };
        Self {
            resolution_bits,
            reference_millivolts,
            samples: Mutex::new(samples),
        }
    }

    /// Sets the simulated raw value of `channel`.
    pub fn set_sample(&self, channel: u8, value: u16) -> Result<(), Error> {
        let index = self.index(channel)?;
        self.samples()[index] = value;
        Ok(())
    }

    fn samples(&self) -> MutexGuard<'_, CriticalSectionRawMutex, [u16; N]> {
        self.samples
            .try_lock()
            .expect("ADC mock samples are already locked")
    }

    fn index(&self, channel: u8) -> Result<usize, Error> {
        let index = usize::from(channel);
        if index < N {
            Ok(index)
        } else {
            Err(Error::InvalidChannel)
        }
    }
}

impl<const N: usize> AdcController for AdcMock<N> {
    fn channel_count(&self) -> u8 {
        N as u8
    }

    fn resolution_bits(&self) -> u8 {
        self.resolution_bits
    }

    fn reference_millivolts(&self) -> Option<u32> {
        self.reference_millivolts
    }

    fn read_raw(&mut self, channel: u8) -> Result<u16, Error> {
        let index = self.index(channel)?;
        Ok(self.samples()[index].min(full_scale(self.resolution_bits)))
    }
}

#[cfg(test)]
mod tests {
    use super::AdcMock;
    use crate::adc::{AdcController, Error};

    #[test]
    fn read_returns_set_value() {
        let mut mock = AdcMock::new(12, Some(3300), [0x0123, 0x0456]);
        assert_eq!(mock.read_raw(1), Ok(0x0456));
        mock.set_sample(1, 0x0ABC).unwrap();
        assert_eq!(mock.read_raw(1), Ok(0x0ABC));
    }

    #[test]
    fn invalid_channel_is_rejected() {
        let mut mock = AdcMock::new(12, None, [0; 2]);
        assert_eq!(mock.read_raw(2), Err(Error::InvalidChannel));
        assert_eq!(mock.set_sample(2, 1), Err(Error::InvalidChannel));
        assert_eq!(mock.channel_count(), 2);
    }

    #[test]
    fn resolution_clamps_value() {
        let mut mock = AdcMock::new(10, None, [0xFFFF]);
        assert_eq!(mock.read_raw(0), Ok(1023));
        assert_eq!(mock.resolution_bits(), 10);
    }

    #[test]
    fn reference_is_reported_as_configured() {
        let mock: AdcMock<1> = AdcMock::new(12, None, [0]);
        assert_eq!(mock.reference_millivolts(), None);
    }
}
