use core::ptr::null_mut;
use std::{ffi::CString, thread::sleep, time::Duration};

use nanoaudio_synth::Waveform;
use synth::Keyboard;
use wasm::*;

use crate::device::AudioOutput;

const KEY_LABELS: [&core::ffi::CStr; 13] = [
    c"C", c"C♯", c"D", c"D♯", c"E", c"F", c"F♯", c"G", c"G♯", c"A", c"A♯", c"B", c"C",
];
const TONES: [Waveform; 3] = [Waveform::Sine, Waveform::Square, Waveform::Triangle];
const TONE_NAMES: [&str; 3] = ["Sine", "Square", "Triangle"];

pub struct Interface {
    window: *mut Object,
    status: *mut Object,
    settings: *mut Object,
    keys: [*mut Object; 13],
    key_labels: [*mut Object; 13],
    controls: [*mut Object; 6],
    keyboard: Keyboard,
    tone: usize,
    volume: u8,
    audio: Option<AudioOutput>,
}

fn text(label: *mut Object, value: &str) -> wasm::Result<()> {
    let value = CString::new(value).map_err(|_| -1)?;
    unsafe { label_set_text(label, value.as_ptr() as *mut _) }
}

unsafe fn button(parent: *mut Object, name: &core::ffi::CStr) -> wasm::Result<(*mut Object, *mut Object)> {
    unsafe {
        let button = button_create(parent)?;
        object_set_height(button, 42)?;
        let label = label_create(button)?;
        label_set_text(label, name.as_ptr() as *mut _)?;
        Ok((button, label))
    }
}

unsafe fn label(parent: *mut Object, value: &str) -> wasm::Result<*mut Object> {
    unsafe {
        let label = label_create(parent)?;
        object_set_width(label, percentage(100)?)?;
        text(label, value)?;
        Ok(label)
    }
}

unsafe fn panel(parent: *mut Object) -> wasm::Result<*mut Object> {
    unsafe {
        let panel = object_create(parent)?;
        object_set_width(panel, percentage(100)?)?;
        object_set_height(panel, size_content())?;
        object_set_flex_flow(panel, FlexFlow::Column)?;
        object_set_padding(panel, 12)?;
        Ok(panel)
    }
}

impl Interface {
    pub fn new() -> wasm::Result<Self> {
        unsafe {
            let window = window_create()?;
            window_set_icon(
                window,
                c"Sy".as_ptr(),
                Color { red: 124, green: 92, blue: 246 },
            )?;
            object_set_flex_flow(window, FlexFlow::Column)?;

            let heading = label(window, "Synthesizer")?;
            object_set_text_size(heading, 24)?;
            let subtitle = label(window, "Play a note by pressing and holding a key")?;
            object_set_text_size(subtitle, 14)?;

            let status_panel = panel(window)?;
            let status_title = label(status_panel, "INSTRUMENT")?;
            object_set_text_size(status_title, 12)?;
            let settings = label(status_panel, "")?;
            object_set_text_size(settings, 18)?;
            let status = label(status_panel, "")?;

            let controls_panel = panel(window)?;
            let controls_title = label(controls_panel, "CONTROLS")?;
            object_set_text_size(controls_title, 12)?;
            let control_row = object_create(controls_panel)?;
            object_set_width(control_row, percentage(100)?)?;
            object_set_flex_flow(control_row, FlexFlow::RowWrap)?;
            let names = [c"Octave −", c"Octave +", c"Waveform", c"Volume −", c"Volume +", c"Retry audio"];
            let mut controls = [null_mut(); 6];
            for (index, name) in names.iter().enumerate() {
                let (control, _) = button(control_row, name)?;
                object_set_width(control, 92)?;
                controls[index] = control;
            }

            let keyboard_panel = panel(window)?;
            let keyboard_title = label(keyboard_panel, "KEYBOARD")?;
            object_set_text_size(keyboard_title, 12)?;
            let keyboard = object_create(keyboard_panel)?;
            object_set_width(keyboard, percentage(100)?)?;
            object_set_flex_flow(keyboard, FlexFlow::RowWrap)?;
            let mut keys = [null_mut(); 13];
            let mut key_labels = [null_mut(); 13];
            for (index, name) in KEY_LABELS.iter().enumerate() {
                let (key, key_label) = button(keyboard, name)?;
                object_set_width(key, 32)?;
                object_set_height(key, 64)?;
                keys[index] = key;
                key_labels[index] = key_label;
            }

            let ui = Self {
                window,
                status,
                settings,
                keys,
                key_labels,
                controls,
                keyboard: Keyboard::default(),
                tone: 0,
                volume: 70,
                audio: AudioOutput::open().ok(),
            };
            ui.render_settings()?;
            ui.render_status()?;
            Ok(ui)
        }
    }

    fn render_status(&self) -> wasm::Result<()> {
        text(
            self.status,
            if self.audio.is_some() {
                "Audio output ready"
            } else {
                "Silent mode · enable host audio, then Retry audio"
            },
        )
    }

    fn render_settings(&self) -> wasm::Result<()> {
        text(
            self.settings,
            &format!("Octave {}     {} wave     {}% volume", self.keyboard.octave(), TONE_NAMES[self.tone], self.volume),
        )
    }

    fn act(&mut self, index: usize) -> wasm::Result<()> {
        match index {
            0 | 1 => {
                if self.keyboard.shift_octave(if index == 0 { -1 } else { 1 }) {
                    if let Some(audio) = &mut self.audio {
                        audio.stop_all();
                    }
                    for (label, name) in self.key_labels.iter().zip(KEY_LABELS) {
                        unsafe { label_set_text(*label, name.as_ptr() as *mut _)?; }
                    }
                }
            }
            2 => {
                self.tone = (self.tone + 1) % TONES.len();
                if let Some(audio) = &mut self.audio {
                    audio.set_waveform(TONES[self.tone]);
                }
            }
            3 | 4 => {
                self.volume = if index == 3 {
                    self.volume.saturating_sub(10)
                } else {
                    self.volume.saturating_add(10).min(100)
                };
                if let Some(audio) = &mut self.audio
                    && audio.set_volume(self.volume).is_err()
                {
                    self.audio = None;
                }
            }
            5 if self.audio.is_none() => {
                self.audio = AudioOutput::open().ok();
                if let Some(audio) = &mut self.audio {
                    audio.set_waveform(TONES[self.tone]);
                    if audio.set_volume(self.volume).is_err() {
                        self.audio = None;
                    }
                }
            }
            _ => {}
        }
        self.render_status()?;
        self.render_settings()
    }

    pub unsafe fn run(&mut self) -> wasm::Result<()> {
        loop {
            for _ in 0..32 {
                let mut code = EventCode::All as u32;
                let mut target: *mut Object = null_mut();
                unsafe {
                    window_pop_event(self.window, &mut code, (&mut target as *mut *mut Object).cast())?;
                }
                if code == EVENT_CLOSE_REQUESTED {
                    self.audio = None;
                    unsafe { object_delete(u16::try_from(self.window as usize).map_err(|_| -1)?)?; }
                    return Ok(());
                }
                if code == EventCode::All as u32 { break; }
                if let Some(index) = self.keys.iter().position(|key| *key == target) {
                    if code == EventCode::Pressed as u32 {
                        if let Some(note) = self.keyboard.press(index) {
                            if let Some(audio) = &mut self.audio { audio.note_on(note); }
                            text(self.key_labels[index], &format!("{} ·", KEY_LABELS[index].to_str().unwrap()))?;
                        }
                    } else if (code == EventCode::Released as u32 || code == EventCode::PressLost as u32)
                        && let Some(note) = self.keyboard.release(index)
                    {
                        if let Some(audio) = &mut self.audio { audio.note_off(note); }
                        unsafe { label_set_text(self.key_labels[index], KEY_LABELS[index].as_ptr() as *mut _)?; }
                    }
                } else if code == EventCode::Clicked as u32
                    && let Some(index) = self.controls.iter().position(|control| *control == target)
                {
                    self.act(index)?;
                }
            }
            if self.audio.as_mut().is_some_and(|audio| {
                audio.pump().is_err_and(|error| error.kind() != std::io::ErrorKind::WouldBlock)
            }) {
                self.audio = None;
                self.render_status()?;
            }
            sleep(Duration::from_millis(2));
        }
    }
}
