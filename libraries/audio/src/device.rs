use alloc::boxed::Box;
use core::{mem::align_of, pin::Pin};

use file_system::{
    BaseOperations, CharacterDevice, Context, ControlCommand, ControlCommandIdentifier, Error,
    MountOperations, Result, Size,
};
use shared::AnyByLayout;

use crate::{
    AudioSystem, GET_CONFIGURATION, GET_VOLUME, GET_WRITABLE, SET_CONFIGURATION, SET_VOLUME, START,
    STOP, StreamHandle,
};

fn validate_payload<T>(payload: &AnyByLayout) -> Result<()> {
    let address = core::ptr::from_ref(payload).cast::<u8>() as usize;
    if payload.get_size() != size_of::<T>() || !address.is_multiple_of(align_of::<T>()) {
        return Err(Error::InvalidParameter);
    }
    Ok(())
}

fn validate_command<C: ControlCommand>(input: &AnyByLayout, output: &AnyByLayout) -> Result<()> {
    validate_payload::<C::Input>(input)?;
    validate_payload::<C::Output>(output)
}

/// The local `/dev/audio/playback` character device.
///
/// The device borrows a pinned static audio service. Every successful VFS open
/// reserves a separate stream and stores that reservation in its `Context`.
/// Cloning an open context is unsupported; the VFS receives
/// [`Error::UnsupportedOperation`] rather than two contexts sharing one stream.
pub struct AudioStreamDevice<
    const STREAMS: usize = 4,
    const STREAM_BYTES: usize = 2048,
    const MIX_FRAMES: usize = 128,
    const OUTPUTS: usize = 2,
    const SINK_BYTES: usize = 3072,
> {
    system: Pin<&'static AudioSystem<STREAMS, STREAM_BYTES, MIX_FRAMES, OUTPUTS, SINK_BYTES>>,
}

impl<const S: usize, const B: usize, const F: usize, const O: usize, const Q: usize>
    AudioStreamDevice<S, B, F, O, Q>
{
    /// Borrows the pinned, static mixer service used to create per-open streams.
    pub const fn new(system: Pin<&'static AudioSystem<S, B, F, O, Q>>) -> Self {
        Self { system }
    }

    fn stream<'c>(&self, context: &'c Context) -> Result<&'c StreamHandle<'static, S, B, F, O, Q>> {
        context
            .get_private_data_of_type::<StreamHandle<'static, S, B, F, O, Q>>()
            .ok_or(Error::InvalidContext)
    }
}

impl<const S: usize, const B: usize, const F: usize, const O: usize, const Q: usize> BaseOperations
    for AudioStreamDevice<S, B, F, O, Q>
{
    fn open(&self, context: &mut Context) -> Result<()> {
        if context.get_private_data().is_some() {
            return Err(Error::InvalidContext);
        }
        let stream = self.system.get_ref().open_stream()?;
        context.set_private_data(Box::new(stream));
        Ok(())
    }

    fn close(&self, context: &mut Context) -> Result<()> {
        context
            .take_private_data_of_type::<StreamHandle<'static, S, B, F, O, Q>>()
            .map(drop)
            .ok_or(Error::InvalidContext)
    }

    fn read(
        &self,
        _context: &mut Context,
        _cx: &mut core::task::Context<'_>,
        _buffer: &mut [u8],
        _absolute_position: Size,
    ) -> core::task::Poll<Result<usize>> {
        file_system::ready(|| Err(Error::UnsupportedOperation))
    }

    fn write(
        &self,
        context: &mut Context,
        _cx: &mut core::task::Context<'_>,
        buffer: &[u8],
        _absolute_position: Size,
    ) -> core::task::Poll<Result<usize>> {
        file_system::ready(|| self.stream(context)?.write(buffer))
    }

    fn flush(
        &self,
        context: &mut Context,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Result<()>> {
        file_system::ready(|| self.stream(context)?.flush())
    }

    fn control(
        &self,
        context: &mut Context,
        _cx: &mut core::task::Context<'_>,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> core::task::Poll<Result<()>> {
        file_system::ready(|| {
            let stream = self.stream(context)?;
            match command {
                SET_CONFIGURATION::IDENTIFIER => {
                    validate_command::<SET_CONFIGURATION>(input, output)?;
                    let configuration = SET_CONFIGURATION::cast_input(input)?;
                    configuration.validate()?;
                    if *configuration != stream.configuration() {
                        return Err(Error::UnsupportedOperation);
                    }
                }
                GET_CONFIGURATION::IDENTIFIER => {
                    validate_command::<GET_CONFIGURATION>(input, output)?;
                    *GET_CONFIGURATION::cast_output(output)? = stream.configuration();
                }
                GET_WRITABLE::IDENTIFIER => {
                    validate_command::<GET_WRITABLE>(input, output)?;
                    *GET_WRITABLE::cast_output(output)? =
                        stream.writable().min(u32::MAX as usize) as u32;
                }
                SET_VOLUME::IDENTIFIER => {
                    validate_command::<SET_VOLUME>(input, output)?;
                    stream.set_volume(*SET_VOLUME::cast_input(input)?)?;
                }
                GET_VOLUME::IDENTIFIER => {
                    validate_command::<GET_VOLUME>(input, output)?;
                    *GET_VOLUME::cast_output(output)? = stream.volume()?;
                }
                START::IDENTIFIER => {
                    validate_command::<START>(input, output)?;
                    stream.start()?;
                }
                STOP::IDENTIFIER => {
                    validate_command::<STOP>(input, output)?;
                    stream.stop()?;
                }
                _ => return Err(Error::UnsupportedOperation),
            }
            Ok(())
        })
    }

    fn clone_context(&self, _context: &Context) -> Result<Context> {
        Err(Error::UnsupportedOperation)
    }
}

impl<const S: usize, const B: usize, const F: usize, const O: usize, const Q: usize> MountOperations
    for AudioStreamDevice<S, B, F, O, Q>
{
}

impl<const S: usize, const B: usize, const F: usize, const O: usize, const Q: usize> CharacterDevice
    for AudioStreamDevice<S, B, F, O, Q>
{
}
