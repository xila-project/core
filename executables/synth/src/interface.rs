use core::ptr::null_mut;
use std::{ffi::CString, thread::sleep, time::Duration};

use nanoaudio_synth::Waveform;
use synth::Keyboard;
use wasm::*;

use crate::device::AudioOutput;

const KEY_LABELS: [&core::ffi::CStr; 13] = [
    c"C", c"C#", c"D", c"D#", c"E", c"F", c"F#", c"G", c"G#", c"A", c"A#", c"B", c"C",
];
const WHITE_KEY_INDICES: [usize; 8] = [0, 2, 4, 5, 7, 9, 11, 12];
const BLACK_KEY_BOUNDARIES: [(usize, usize); 5] = [(1, 1), (3, 2), (6, 4), (8, 5), (10, 6)];
const TONES: [Waveform; 3] = [Waveform::Sine, Waveform::Square, Waveform::Triangle];
const TONE_NAMES: [&str; 3] = ["Sine", "Square", "Triangle"];

pub struct Interface {
    window: *mut Object,
    status: *mut Object,
    settings: *mut Object,
    volume_label: *mut Object,
    keys: [*mut Object; 13],
    key_labels: [*mut Object; 13],
    controls: [*mut Object; 4],
    volume_slider: *mut Object,
    keyboard: Keyboard,
    tone: usize,
    volume: u8,
    audio: Option<AudioOutput>,
}

fn text(label: *mut Object, value: &str) -> wasm::Result<()> {
    let value = CString::new(value).map_err(|_| -1)?;
    unsafe { label_set_text(label, value.as_ptr() as *mut _) }
}

unsafe fn button(
    parent: *mut Object,
    name: &core::ffi::CStr,
) -> wasm::Result<(*mut Object, *mut Object)> {
    unsafe {
        let button = button_create(parent)?;
        object_set_height(button, 42)?;
        let label = label_create(button)?;
        label_set_text(label, name.as_ptr() as *mut _)?;
        Ok((button, label))
    }
}

unsafe fn piano_key(
    parent: *mut Object,
    name: &core::ffi::CStr,
    x: i32,
    width: i32,
    height: i32,
    background: u32,
    foreground: u32,
) -> wasm::Result<(*mut Object, *mut Object)> {
    unsafe {
        let (key, key_label) = button(parent, name)?;
        object_set_width(key, width)?;
        object_set_height(key, height)?;
        object_set_pos(key, x, 0)?;
        object_set_background_rgb(key, background)?;
        object_set_text_rgb(key, foreground)?;
        object_set_text_rgb(key_label, foreground)?;
        Ok((key, key_label))
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
                Color {
                    red: 124,
                    green: 92,
                    blue: 246,
                },
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
            let names = [c"Octave −", c"Octave +", c"Waveform", c"Retry audio"];
            let mut controls = [null_mut(); 4];
            for (index, name) in names.iter().enumerate() {
                let (control, _) = button(control_row, name)?;
                object_set_width(control, 92)?;
                object_set_theme_variant(
                    control,
                    if index == 3 {
                        ThemeVariant::Default as u32
                    } else {
                        ThemeVariant::Outline as u32
                    },
                )?;
                controls[index] = control;
            }
            let volume_group = object_create(controls_panel)?;
            object_set_width(volume_group, percentage(100)?)?;
            object_set_flex_flow(volume_group, FlexFlow::Column)?;
            let volume_label = label(volume_group, "Volume: 70%")?;
            let volume_slider = slider_create(volume_group)?;
            object_set_width(volume_slider, percentage(100)?)?;
            slider_set_range(volume_slider, 0, 100)?;
            slider_set_value(volume_slider, 70, AnimEnable::Off)?;

            let keyboard_panel = panel(window)?;
            let keyboard_title = label(keyboard_panel, "KEYBOARD")?;
            object_set_text_size(keyboard_title, 12)?;
            let keyboard = object_create(keyboard_panel)?;
            object_set_width(keyboard, percentage(100)?)?;
            object_set_height(keyboard, 132)?;
            object_set_layout(keyboard, 0)?;
            object_set_padding(keyboard, 0)?;
            let board_width = object_get_content_width(keyboard)?;
            let white_width = board_width / WHITE_KEY_INDICES.len() as i32;
            let black_width = white_width * 3 / 5;
            let white_height = 118;
            let black_height = 76;
            let mut keys = [null_mut(); 13];
            let mut key_labels = [null_mut(); 13];
            for (white_index, &note_index) in WHITE_KEY_INDICES.iter().enumerate() {
                let (key, key_label) = piano_key(
                    keyboard,
                    KEY_LABELS[note_index],
                    white_index as i32 * white_width,
                    white_width,
                    white_height,
                    0x00f1_eee7,
                    0x001b_1b20,
                )?;
                keys[note_index] = key;
                key_labels[note_index] = key_label;
            }
            for (note_index, boundary) in BLACK_KEY_BOUNDARIES {
                let (key, key_label) = piano_key(
                    keyboard,
                    KEY_LABELS[note_index],
                    boundary as i32 * white_width - black_width / 2,
                    black_width,
                    black_height,
                    0x0022_242c,
                    0x00ff_ffff,
                )?;
                keys[note_index] = key;
                key_labels[note_index] = key_label;
            }

            let ui = Self {
                window,
                status,
                settings,
                volume_label,
                keys,
                key_labels,
                controls,
                volume_slider,
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
            &format!(
                "Octave {}     {} wave     {}% volume",
                self.keyboard.octave(),
                TONE_NAMES[self.tone],
                self.volume
            ),
        )?;
        text(self.volume_label, &format!("Volume: {}%", self.volume))
    }

    fn act(&mut self, index: usize) -> wasm::Result<()> {
        match index {
            0 | 1 => {
                if self.keyboard.shift_octave(if index == 0 { -1 } else { 1 }) {
                    if let Some(audio) = &mut self.audio {
                        audio.stop_all();
                    }
                    for (label, name) in self.key_labels.iter().zip(KEY_LABELS) {
                        unsafe {
                            label_set_text(*label, name.as_ptr() as *mut _)?;
                        }
                    }
                }
            }
            2 => {
                self.tone = (self.tone + 1) % TONES.len();
                if let Some(audio) = &mut self.audio {
                    audio.set_waveform(TONES[self.tone]);
                }
            }
            3 if self.audio.is_none() => {
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
                    window_pop_event(
                        self.window,
                        &mut code,
                        (&mut target as *mut *mut Object).cast(),
                    )?;
                }
                if code == EVENT_CLOSE_REQUESTED {
                    self.audio = None;
                    unsafe {
                        object_delete(u16::try_from(self.window as usize).map_err(|_| -1)?)?;
                    }
                    return Ok(());
                }
                if code == EventCode::All as u32 {
                    break;
                }
                if target == self.volume_slider && code == EventCode::ValueChanged as u32 {
                    self.volume = u8::try_from(unsafe { slider_get_value(self.volume_slider)? })
                        .map_err(|_| -1)?;
                    if let Some(audio) = &mut self.audio
                        && audio.set_volume(self.volume).is_err()
                    {
                        self.audio = None;
                    }
                    self.render_status()?;
                    self.render_settings()?;
                    continue;
                }
                if let Some(index) = self.keys.iter().position(|key| *key == target) {
                    if code == EventCode::Pressed as u32 {
                        if let Some(note) = self.keyboard.press(index) {
                            if let Some(audio) = &mut self.audio {
                                audio.note_on(note);
                            }
                            text(
                                self.key_labels[index],
                                &format!("{}*", KEY_LABELS[index].to_str().unwrap()),
                            )?;
                        }
                    } else if (code == EventCode::Released as u32
                        || code == EventCode::PressLost as u32)
                        && let Some(note) = self.keyboard.release(index)
                    {
                        if let Some(audio) = &mut self.audio {
                            audio.note_off(note);
                        }
                        unsafe {
                            label_set_text(
                                self.key_labels[index],
                                KEY_LABELS[index].as_ptr() as *mut _,
                            )?;
                        }
                    }
                } else if code == EventCode::Clicked as u32
                    && let Some(index) = self.controls.iter().position(|control| *control == target)
                {
                    self.act(index)?;
                }
            }
            if self.audio.as_mut().is_some_and(|audio| {
                audio
                    .pump()
                    .is_err_and(|error| error.kind() != std::io::ErrorKind::WouldBlock)
            }) {
                self.audio = None;
                self.render_status()?;
            }
            sleep(Duration::from_millis(2));
        }
    }
}
