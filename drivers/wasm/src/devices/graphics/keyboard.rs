use alloc::{boxed::Box, string::String};
use file_system::{DirectBaseOperations, DirectCharacterDevice, MountOperations, Size};
use graphics::{InputData, Key, State};
use synchronization::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, channel::Sender,
};
use wasm_bindgen::{JsCast, prelude::Closure};
use web_sys::{ClipboardEvent, HtmlCanvasElement, KeyboardEvent};

#[derive(Debug)]
pub struct KeyboardDevice(Box<Channel<CriticalSectionRawMutex, (Key, State), 512>>);

impl KeyboardDevice {
    fn handle_key_press_char<const N: usize>(
        sender: Sender<'_, CriticalSectionRawMutex, (Key, State), N>,
        character: char,
        pressed: bool,
    ) {
        let key = Key::Character(character as u8);

        let state = if pressed {
            State::Pressed
        } else {
            State::Released
        };

        if let Err(e) = sender.try_send((key, state)) {
            log::error!("Failed to send key event: {:?}", e);
        }
    }

    fn handle_key_press<const N: usize>(
        sender: Sender<'_, CriticalSectionRawMutex, (Key, State), N>,
        key: &str,
        pressed: bool,
    ) {
        let key = match map_key(key) {
            Some(k) => k,
            None => {
                log::warning!("Unsupported key: {}", key);
                return;
            }
        };

        let state = if pressed {
            State::Pressed
        } else {
            State::Released
        };

        if let Err(e) = sender.try_send((key, state)) {
            log::error!("Failed to send key event: {:?}", e);
        }
    }

    pub fn new(canvas: &HtmlCanvasElement) -> Result<Self, String> {
        // A focusable canvas receives keyboard and clipboard input without
        // intercepting the surrounding page's controls.
        canvas.set_tab_index(0);
        canvas.focus().map_err(|_| "Failed to focus canvas")?;

        let focus_closure = Closure::wrap(Box::new(move |event: web_sys::Event| {
            if let Some(canvas) = event
                .current_target()
                .and_then(|target| target.dyn_into::<HtmlCanvasElement>().ok())
            {
                let _ = canvas.focus();
            }
        }) as Box<dyn FnMut(web_sys::Event)>);
        canvas
            .add_event_listener_with_callback("pointerdown", focus_closure.as_ref().unchecked_ref())
            .map_err(|_| "Failed to add canvas focus event listener")?;
        focus_closure.forget();

        let inner = Self(Box::new(Channel::new()));

        let sender = inner.0.sender();

        let key_down_closure = Closure::wrap(Box::new(move |event: KeyboardEvent| {
            // Keep browser/OS shortcuts and IME composition available. In
            // particular Ctrl/Cmd+V must generate the separate paste event.
            if !is_focused_canvas(event.as_ref())
                || event.default_prevented()
                || event.ctrl_key()
                || event.meta_key()
                || event.alt_key()
                || event.is_composing()
            {
                return;
            }

            let key = event.key();
            if map_key(&key).is_none() {
                return;
            }

            // Cancel quick-find, scrolling and navigation only for keys that
            // Xila consumes. Unmapped keys (including Tab) retain their defaults.
            event.prevent_default();
            Self::handle_key_press(sender, &key, true);
            Self::handle_key_press(sender, &key, false);
        }) as Box<dyn FnMut(KeyboardEvent)>);

        canvas
            .add_event_listener_with_callback("keydown", key_down_closure.as_ref().unchecked_ref())
            .map_err(|_| "Failed to add keydown event listener")?;

        let sender = inner.0.sender();

        let paste_closure = Closure::wrap(Box::new(move |event: ClipboardEvent| {
            if !is_focused_canvas(event.as_ref()) || event.default_prevented() {
                return;
            }
            if let Some(clipboard_data) = event.clipboard_data()
                && let Ok(text) = clipboard_data.get_data("text")
            {
                event.prevent_default();
                for char in text.chars() {
                    Self::handle_key_press_char(sender, char, true);
                    Self::handle_key_press_char(sender, char, false);
                }
            }
        }) as Box<dyn FnMut(ClipboardEvent)>);

        canvas
            .add_event_listener_with_callback("paste", paste_closure.as_ref().unchecked_ref())
            .map_err(|_| "Failed to add paste event listener")?;

        key_down_closure.forget(); // Prevent memory leak by keeping the closure alive
        paste_closure.forget(); // Prevent memory leak by keeping the closure alive

        Ok(inner)
    }
}

impl DirectBaseOperations for KeyboardDevice {
    fn read(
        &self,
        _cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        _: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| {
            let data: &mut InputData = buffer
                .try_into()
                .map_err(|_| file_system::Error::InvalidParameter)?;

            if let Ok((key, state)) = self.0.receiver().try_receive() {
                data.set_key(key);
                data.set_state(state);
            }

            data.set_continue(!self.0.is_empty());

            Ok(buffer.len())
        })
    }

    fn write(
        &self,
        _cx: &mut core::task::Context<'_>,
        _: &[u8],
        _: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| Err(file_system::Error::UnsupportedOperation))
    }
}

impl MountOperations for KeyboardDevice {}

impl DirectCharacterDevice for KeyboardDevice {}

fn map_key(key: &str) -> Option<Key> {
    if key.len() == 1 {
        let character = key.chars().next()?;

        if character.is_ascii() {
            return Some(Key::Character(character as u8));
        }
    }

    let key = match key {
        "ArrowUp" => Key::Up,
        "ArrowDown" => Key::Down,
        "ArrowRight" => Key::Right,
        "ArrowLeft" => Key::Left,
        "Escape" => Key::Escape,
        "Delete" => Key::Delete,
        "Backspace" => Key::Backspace,
        "Enter" => Key::Enter,
        "PageDown" => Key::Next,
        "PageUp" => Key::Previous,
        "Home" => Key::Home,
        "End" => Key::End,
        _ => return None,
    };

    Some(key)
}

fn is_focused_canvas(event: &web_sys::Event) -> bool {
    let Some(canvas) = event
        .current_target()
        .and_then(|target| target.dyn_into::<HtmlCanvasElement>().ok())
    else {
        return false;
    };
    canvas
        .owner_document()
        .and_then(|document| document.active_element())
        == Some(canvas.unchecked_into())
}
