use core::ptr::null_mut;
use std::{thread::sleep, time::Duration};

use wasm::{
    EVENT_CLOSE_REQUESTED, EventCode, FlexFlow, Object, button_create, label_create,
    label_set_text, object_create, object_set_flex_flow, object_set_height, object_set_width,
    percentage, window_create, window_pop_event,
};

use crate::device::AudioOutput;

const NOTES: [u8; 13] = [60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72];
const KEY_LABELS: [&core::ffi::CStr; 13] = [
    c"C4", c"C#", c"D", c"D#", c"E", c"F", c"F#", c"G", c"G#", c"A", c"A#", c"B", c"C5",
];

pub struct Interface {
    window: *mut Object,
    status: *mut Object,
    keys: [*mut Object; NOTES.len()],
    note_active: [bool; NOTES.len()],
    audio: Option<AudioOutput>,
}

impl Interface {
    pub fn new() -> wasm::Result<Self> {
        unsafe {
            let window = window_create()?;
            object_set_flex_flow(window, FlexFlow::Column)?;

            let title = label_create(window)?;
            label_set_text(
                title,
                c"Xila Synth — click a key to toggle its note".as_ptr() as *mut _,
            )?;

            let status = label_create(window)?;
            let audio = AudioOutput::open().ok();
            let status_text = if audio.is_some() {
                c"Audio ready"
            } else {
                c"Audio unavailable; enable browser audio before launching Synth"
            };
            label_set_text(status, status_text.as_ptr() as *mut _)?;

            let keyboard = object_create(window)?;
            object_set_flex_flow(keyboard, FlexFlow::RowWrap)?;
            object_set_width(keyboard, percentage(100)?)?;

            let mut keys = [null_mut(); NOTES.len()];
            for (index, label) in KEY_LABELS.iter().enumerate() {
                let button = button_create(keyboard)?;
                object_set_width(button, percentage(7)?)?;
                object_set_height(button, 72)?;
                let text = label_create(button)?;
                label_set_text(text, label.as_ptr() as *mut _)?;
                keys[index] = button;
            }

            Ok(Self {
                window,
                status,
                keys,
                note_active: [false; NOTES.len()],
                audio,
            })
        }
    }

    fn set_status(&self, text: &'static core::ffi::CStr) {
        unsafe {
            let _ = label_set_text(self.status, text.as_ptr() as *mut _);
        }
    }

    pub unsafe fn run(&mut self) -> wasm::Result<()> {
        loop {
            let mut code = EventCode::All as u32;
            let mut target: *mut Object = null_mut();
            unsafe {
                window_pop_event(
                    self.window,
                    &mut code as *mut _ as *mut _,
                    &mut target as *mut _ as *mut _,
                )?;
            }

            if code == EVENT_CLOSE_REQUESTED {
                return Ok(());
            }
            if code == EventCode::Clicked as u32 {
                if let Some(index) = self.keys.iter().position(|button| *button == target) {
                    if self.note_active[index] {
                        if let Some(audio) = &mut self.audio {
                            audio.note_off(NOTES[index]);
                        }
                        self.note_active[index] = false;
                    } else {
                        if let Some(audio) = &mut self.audio {
                            audio.note_on(NOTES[index]);
                        }
                        self.note_active[index] = true;
                    }
                }
            }

            let audio_failed = self.audio.as_mut().is_some_and(|audio| {
                audio
                    .pump()
                    .is_err_and(|error| error.kind() != std::io::ErrorKind::WouldBlock)
            });
            if audio_failed {
                self.set_status(c"Audio stream error");
                self.audio = None;
            }
            sleep(Duration::from_millis(2));
        }
    }
}
