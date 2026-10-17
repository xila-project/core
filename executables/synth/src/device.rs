use core::mem::size_of;
use std::{
    fs::{File, OpenOptions},
    io,
    os::fd::AsRawFd,
    thread::sleep,
    time::Duration,
};

use synth::{
    AudioConfiguration, GET_CONFIGURATION, GET_VOLUME, GET_WRITABLE, Instrument, PcmWriter,
    SET_VOLUME, START, STOP, WriteProgress,
};

#[repr(align(16))]
struct Aligned<T>(T);

fn call_control(
    file: &File,
    command: u32,
    input: *const u8,
    input_length: usize,
    output: *mut u8,
    output_length: usize,
) -> io::Result<()> {
    const RETRIES: usize = 8;
    for attempt in 0..=RETRIES {
        let result = unsafe {
            wasm::fd_control(
                file.as_raw_fd(),
                command,
                input,
                input_length as u32,
                output,
                output_length as u32,
            )
        };
        if result == 0 {
            return Ok(());
        }
        let error = io::Error::from_raw_os_error(result);
        if error.kind() != io::ErrorKind::WouldBlock || attempt == RETRIES {
            return Err(error);
        }
        sleep(Duration::from_millis(1));
    }
    unreachable!()
}

fn read_control<T>(file: &File, command: u32, output: &mut Aligned<T>) -> io::Result<()> {
    let input = Aligned(());
    call_control(
        file,
        command,
        core::ptr::from_ref(&input).cast(),
        0,
        core::ptr::from_mut(output).cast(),
        size_of::<T>(),
    )
}

fn write_control<T>(file: &File, command: u32, input_value: T) -> io::Result<()> {
    let input = Aligned(input_value);
    let mut output = Aligned(());
    call_control(
        file,
        command,
        core::ptr::from_ref(&input).cast(),
        size_of::<T>(),
        core::ptr::from_mut(&mut output).cast(),
        0,
    )
}

pub struct AudioOutput {
    file: WasiPcmFile,
    instrument: Instrument,
    started: bool,
}

impl AudioOutput {
    pub fn open() -> io::Result<Self> {
        let file = OpenOptions::new()
            .write(true)
            .open("/devices/audio/playback")?;

        let mut configuration = Aligned(AudioConfiguration::new(1, 2, 4));
        read_control(&file, GET_CONFIGURATION, &mut configuration)?;
        let instrument = Instrument::new(configuration.0).map_err(|error| match error {
            synth::ConfigurationError::Invalid => {
                io::Error::new(io::ErrorKind::InvalidData, "invalid audio configuration")
            }
            synth::ConfigurationError::Unsupported => io::Error::new(
                io::ErrorKind::Unsupported,
                "unsupported audio configuration",
            ),
        })?;

        let mut _current_volume = Aligned(0u8);
        read_control(&file, GET_VOLUME, &mut _current_volume)?;
        write_control(&file, SET_VOLUME, 70u8)?;
        write_control(&file, START, ())?;

        Ok(Self {
            file: WasiPcmFile(file),
            instrument,
            started: true,
        })
    }

    pub fn note_on(&mut self, note: u8) {
        self.instrument.note_on(note);
    }

    pub fn note_off(&mut self, note: u8) {
        self.instrument.note_off(note);
    }

    pub fn pump(&mut self) -> io::Result<WriteProgress> {
        let mut writable = Aligned(0u32);
        if let Err(error) = read_control(&self.file.0, GET_WRITABLE, &mut writable) {
            if error.kind() == io::ErrorKind::WouldBlock {
                return Ok(WriteProgress::Backpressure);
            }
            return Err(error);
        }
        self.instrument
            .write_once(writable.0 as usize, &mut self.file)
    }
}

struct WasiPcmFile(File);

impl PcmWriter for WasiPcmFile {
    type Error = io::Error;

    fn write_pcm(&mut self, bytes: &[u8]) -> Result<usize, Self::Error> {
        match io::Write::write(&mut self.0, bytes) {
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(0),
            result => result,
        }
    }

    fn invalid_write(&self) -> Self::Error {
        io::Error::new(io::ErrorKind::InvalidData, "audio write split a PCM frame")
    }
}

impl Drop for AudioOutput {
    fn drop(&mut self) {
        self.instrument.stop_all();
        if self.started {
            let _ = write_control(&self.file.0, STOP, ());
            self.started = false;
        }
    }
}
