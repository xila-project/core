//! Play a quiet 440 Hz tone through the shared native audio service.

use audio::AudioSystem;
use std::{
    pin::Pin,
    thread,
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    const RATE: u32 = 48_000;
    const FRAMES: usize = 128;
    let system: Pin<&'static AudioSystem> = Box::leak(Box::pin(AudioSystem::new()?)).as_ref();
    let stream = system.open_stream()?;
    stream.set_volume(25)?;
    stream.start()?;
    let runner = drivers_native::audio::new(system, 1)?;

    println!("Playing a two-second 440 Hz tone through the shared native audio service.");
    let mut bytes = [0u8; FRAMES * 2 * size_of::<f32>()];
    let mut frame = 0u32;
    let end = Instant::now() + Duration::from_secs(2);
    while Instant::now() < end {
        runner.check()?;
        for offset in 0..FRAMES {
            let phase = ((frame + offset as u32) * 440) % RATE;
            let sample: f32 = if phase < RATE / 2 { 0.08 } else { -0.08 };
            let start = offset * 8;
            bytes[start..start + 4].copy_from_slice(&sample.to_le_bytes());
            bytes[start + 4..start + 8].copy_from_slice(&sample.to_le_bytes());
        }
        let mut remaining = &bytes[..];
        while !remaining.is_empty() {
            match stream.write(remaining) {
                Ok(0) => thread::yield_now(),
                Ok(written) => remaining = &remaining[written..],
                Err(file_system::Error::RessourceBusy) => thread::yield_now(),
                Err(error) => return Err(error.into()),
            }
        }
        frame += FRAMES as u32;
    }
    stream.stop()?;
    println!(
        "Sink underruns: {:?}",
        runner.underruns().collect::<Vec<_>>()
    );
    Ok(())
}
