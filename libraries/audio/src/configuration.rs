pub use device::audio::{Configuration, SampleFormat};

pub(crate) const fn native_format(format: SampleFormat) -> nanoaudio::Format {
    match format {
        SampleFormat::U8 => nanoaudio::Format::U8,
        SampleFormat::I16 => nanoaudio::Format::I16,
        SampleFormat::I24 => nanoaudio::Format::I24,
        SampleFormat::I32 => nanoaudio::Format::I32,
        SampleFormat::F32 => nanoaudio::Format::F32,
    }
}
