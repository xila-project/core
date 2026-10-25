use ::audio::{AudioSystem, Configuration, OutputHandle};
use core::pin::Pin;
use file_system::{Error, Result};

/// A bounded output sink for the real audio mixer, without a physical device.
///
/// The caller drives time explicitly with [`AudioSystem::mix_next_block`] and
/// drains mixed PCM with [`Self::read`]. No thread, timer, allocation or extra
/// queue is created. Dropping the sink releases its output reservation.
pub struct HeadlessAudioOutput<
    'a,
    const S: usize = 4,
    const B: usize = 2048,
    const F: usize = 128,
    const O: usize = 2,
    const Q: usize = 3072,
> {
    output: OutputHandle<'a, S, B, F, O, Q>,
}

impl<'a, const S: usize, const B: usize, const F: usize, const O: usize, const Q: usize>
    HeadlessAudioOutput<'a, S, B, F, O, Q>
{
    /// Reserves one output on a pinned audio system. Its storage stays caller-owned.
    pub fn new(system: Pin<&'a AudioSystem<S, B, F, O, Q>>) -> Result<Self> {
        Ok(Self {
            output: system.open_output()?,
        })
    }

    /// Returns the mixer's fixed 48 kHz stereo F32 PCM configuration.
    pub fn configuration(&self) -> Configuration {
        self.output.configuration()
    }

    /// Drains available whole frames into caller-provided storage without waiting.
    ///
    /// Returns the number of bytes copied, or zero for an empty queue or buffer.
    /// Unused buffer bytes are unchanged. A buffer not sized to whole frames is
    /// rejected before consuming any samples. This does not advance the mixer.
    pub fn read(&mut self, buffer: &mut [u8]) -> Result<usize> {
        if !buffer
            .len()
            .is_multiple_of(self.configuration().frame_size()?)
        {
            return Err(Error::InvalidParameter);
        }
        Ok(self.output.with_consumer(|consumer| consumer.read(buffer)))
    }

    /// Counts whole mix blocks lost while this output's bounded queue was full.
    pub fn dropped_blocks(&self) -> usize {
        self.output.dropped_blocks()
    }
}
