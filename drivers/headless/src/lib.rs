#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
compile_error!("drivers_headless is only available on native host platforms");

mod audio;
mod screen;

pub use audio::HeadlessAudioOutput;
pub use screen::{HeadlessError, HeadlessScreenDevice};
